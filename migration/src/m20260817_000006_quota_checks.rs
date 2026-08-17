use sea_orm::{ConnectionTrait, DatabaseBackend};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// `(table, column)` pairs that must never go negative.
const NON_NEGATIVE: &[(&str, &str)] = &[
    ("buckets", "used_bytes"),
    ("buckets", "reserved_bytes"),
    ("buckets", "object_count"),
    ("users", "used_bytes"),
    ("users", "reserved_bytes"),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        // A backstop, not the mechanism: the guarded UPDATEs in models::quota already refuse to go
        // negative. This turns a future accounting bug into a write error instead of a negative
        // number quietly served over the API.
        //
        // MySQL only enforces CHECK from 8.0.16; on 8.0.13–8.0.15 it parses and ignores it. The
        // project floor is 8.0.13, so on those three patch versions this is decorative — acceptable
        // for a backstop.
        //
        // SQLite cannot add a CHECK to an existing table without rebuilding it, which is not worth
        // it for something the application layer already enforces.
        if matches!(m.get_database_backend(), DatabaseBackend::Sqlite) {
            return Ok(());
        }

        let conn = m.get_connection();
        for (table, column) in NON_NEGATIVE {
            let name = format!("chk_{table}_{column}_non_negative");
            conn.execute_unprepared(&format!(
                "ALTER TABLE {table} ADD CONSTRAINT {name} CHECK ({column} >= 0)"
            ))
            .await?;
        }
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        let backend = m.get_database_backend();
        if matches!(backend, DatabaseBackend::Sqlite) {
            return Ok(());
        }

        let conn = m.get_connection();
        for (table, column) in NON_NEGATIVE {
            let name = format!("chk_{table}_{column}_non_negative");
            let sql = if matches!(backend, DatabaseBackend::MySql) {
                format!("ALTER TABLE {table} DROP CHECK {name}")
            } else {
                format!("ALTER TABLE {table} DROP CONSTRAINT {name}")
            };
            conn.execute_unprepared(&sql).await?;
        }
        Ok(())
    }
}
