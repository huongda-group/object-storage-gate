use sea_orm::{ConnectionTrait, DatabaseBackend};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// Identifier columns that Postgres still compares with the cluster's collation.
/// `(table, column, varchar length)`.
const POSTGRES_COLUMNS: &[(&str, &str, u32)] = &[
    ("objects", "object_key", 1024),
    ("buckets", "name", 255),
    ("access_key_prefixes", "prefix", 512),
    ("access_keys", "access_key_id", 255),
    ("multipart_uploads", "object_key", 1024),
    ("multipart_uploads", "upstream_upload_id", 255),
];

/// `multipart_uploads` was created after `m20260817_000004` ran, so its identifiers never got `utf8mb4_bin`.
/// The four columns that migration did cover are left alone; a second `MODIFY` would rewrite the whole table to reach the collation it already has.
const MYSQL_COLUMNS: &[(&str, &str, u32)] = &[
    ("multipart_uploads", "object_key", 1024),
    ("multipart_uploads", "upstream_upload_id", 255),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        // `m20260817_000004` assumed Postgres compares these byte for byte. It does not — that holds only when the cluster was initialised with the C or POSIX collation.
        // On the en_US.UTF-8 default, `ORDER BY object_key` returns a, A, b, B where S3 promises A, B, a, b, and the half-open range that backs both list and paging silently drops rows: 'same/key' does not sort inside ['same/', 'same0') once the collation stops treating '/' as a character that counts.
        // Punctuation is what makes this bite so hard, because every prefix boundary in an S3 key is punctuation.
        //
        // Raw SQL is unavoidable here: sea-query has no API for a collation on modify_column.
        // SQLite is left out — it compares BINARY already and cannot modify a column.
        let (columns, statement): (_, fn(&str, &str, u32) -> String) = match m
            .get_database_backend()
        {
            DatabaseBackend::Postgres => (POSTGRES_COLUMNS, |table, column, len| {
                format!(
                    r#"ALTER TABLE {table} ALTER COLUMN {column} TYPE VARCHAR({len}) COLLATE "C""#
                )
            }),
            DatabaseBackend::MySql => (MYSQL_COLUMNS, |table, column, len| {
                format!(
                        "ALTER TABLE {table} MODIFY {column} VARCHAR({len}) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL"
                    )
            }),
            DatabaseBackend::Sqlite => return Ok(()),
        };

        let conn = m.get_connection();
        for (table, column, len) in columns {
            conn.execute_unprepared(&statement(table, column, *len))
                .await?;
        }
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        let (columns, statement): (_, fn(&str, &str, u32) -> String) = match m
            .get_database_backend()
        {
            DatabaseBackend::Postgres => (POSTGRES_COLUMNS, |table, column, len| {
                format!(
                    r#"ALTER TABLE {table} ALTER COLUMN {column} TYPE VARCHAR({len}) COLLATE "default""#
                )
            }),
            DatabaseBackend::MySql => (MYSQL_COLUMNS, |table, column, len| {
                format!("ALTER TABLE {table} MODIFY {column} VARCHAR({len}) NOT NULL")
            }),
            DatabaseBackend::Sqlite => return Ok(()),
        };

        let conn = m.get_connection();
        for (table, column, len) in columns {
            conn.execute_unprepared(&statement(table, column, *len))
                .await?;
        }
        Ok(())
    }
}
