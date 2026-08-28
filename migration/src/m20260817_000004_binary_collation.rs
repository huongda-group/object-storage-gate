use sea_orm::{ConnectionTrait, DatabaseBackend};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// Columns whose values are identifiers, not human text, so a case-insensitive or accent-insensitive comparison is always wrong.
/// `(table, column, varchar length)`.
const IDENTIFIER_COLUMNS: &[(&str, &str, u32)] = &[
    ("objects", "object_key", 1024),
    ("buckets", "name", 255),
    ("access_key_prefixes", "prefix", 512),
    ("access_keys", "access_key_id", 255),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        // SQLite already compares these byte for byte; Postgres does so only on a C or POSIX cluster, which m20260828_000001 stops assuming.
        // MySQL 8 defaults to utf8mb4_0900_ai_ci, which folds case and accents, so PUT Photos/A.JPG followed by PUT photos/a.jpg collides on the unique index and one object silently overwrites the other.
        // S3 object keys are case-sensitive.
        //
        // Raw SQL is unavoidable here: sea-query has no API for a collation on modify_column.
        if !matches!(m.get_database_backend(), DatabaseBackend::MySql) {
            return Ok(());
        }

        let conn = m.get_connection();
        for (table, column, len) in IDENTIFIER_COLUMNS {
            conn.execute_unprepared(&format!(
                "ALTER TABLE {table} MODIFY {column} VARCHAR({len}) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL"
            ))
            .await?;
        }
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        if !matches!(m.get_database_backend(), DatabaseBackend::MySql) {
            return Ok(());
        }

        let conn = m.get_connection();
        for (table, column, len) in IDENTIFIER_COLUMNS {
            conn.execute_unprepared(&format!(
                "ALTER TABLE {table} MODIFY {column} VARCHAR({len}) NOT NULL"
            ))
            .await?;
        }
        Ok(())
    }
}
