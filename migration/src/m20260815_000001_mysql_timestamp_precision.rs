use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

/// `MySQL`-only: widen every `TIMESTAMP` column to `TIMESTAMP(6)`.
///
/// `MySQL`'s `TIMESTAMP` defaults to precision 0 — it *rounds* to the second, including rounding up.
/// Postgres `timestamptz` and `SQLite` (which stores ISO strings) both keep the fractional part, so the same line of code drifts by up to half a second on `MySQL`: `expires_at` reads later than it was written, `days_until_expiry()` jumps a day, magic links expire later than the computed ceiling.
///
/// Scan `information_schema` instead of listing every column by hand: timestamp columns are scattered across `users`, `access_keys`, `buckets`, `objects`... and columns added by later migrations still pass through here when this runs on a blank DB.
///
/// Only columns that exist when this migration runs can be patched.
/// Migrations generated later sit below the `inject-above` marker in `lib.rs`, meaning they run *after* it, so new `TIMESTAMP` columns come back at precision 0 and the rounding bug returns.
/// Timestamp columns added later must declare `TIMESTAMP(6)` themselves on `MySQL` — or copy this whole scan block into that migration.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        if m.get_database_backend() != DatabaseBackend::MySql {
            return Ok(());
        }
        let conn = m.get_connection();
        let rows = conn
            .query_all(Statement::from_string(
                DatabaseBackend::MySql,
                "SELECT TABLE_NAME, COLUMN_NAME, IS_NULLABLE, COLUMN_DEFAULT, EXTRA \
                 FROM information_schema.COLUMNS \
                 WHERE TABLE_SCHEMA = DATABASE() AND DATA_TYPE = 'timestamp' \
                   AND DATETIME_PRECISION = 0",
            ))
            .await?;

        for row in rows {
            let table: String = row.try_get("", "TABLE_NAME")?;
            let column: String = row.try_get("", "COLUMN_NAME")?;
            let nullable: String = row.try_get("", "IS_NULLABLE")?;
            let default: Option<String> = row.try_get("", "COLUMN_DEFAULT")?;
            let extra: String = row.try_get("", "EXTRA")?;

            let mut sql = format!("ALTER TABLE `{table}` MODIFY COLUMN `{column}` TIMESTAMP(6)");
            sql.push_str(if nullable == "YES" {
                " NULL"
            } else {
                " NOT NULL"
            });
            if let Some(default) = default {
                // CURRENT_TIMESTAMP must match the column's precision, otherwise MySQL rejects it; constant values get quoted.
                if default.to_uppercase().starts_with("CURRENT_TIMESTAMP") {
                    sql.push_str(" DEFAULT CURRENT_TIMESTAMP(6)");
                } else {
                    sql.push_str(" DEFAULT '");
                    sql.push_str(&default.replace('\'', "''"));
                    sql.push('\'');
                }
            }
            // EXTRA lumps several things together, most of them not valid DDL (MySQL 8 stuffs "DEFAULT_GENERATED" in here too).
            // Only the ON UPDATE clause needs to be kept, and it must match the column's new precision.
            if extra.to_lowercase().contains("on update") {
                sql.push_str(" ON UPDATE CURRENT_TIMESTAMP(6)");
            }
            conn.execute_unprepared(&sql).await?;
        }
        Ok(())
    }

    async fn down(&self, _m: &SchemaManager) -> Result<(), DbErr> {
        // Lowering the precision back only loses data without restoring anything — no-op.
        Ok(())
    }
}
