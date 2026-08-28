//! `MySQL`-only: widen every `TIMESTAMP` column still at precision 0 to `TIMESTAMP(6)`.
//!
//! `MySQL`'s `TIMESTAMP` defaults to precision 0 — it *rounds* to the second, including rounding up.
//! Postgres `timestamptz` and `SQLite` (which stores ISO strings) both keep the fractional part, so the same line of code drifts by up to half a second on `MySQL`.
//!
//! Lives here rather than inside one migration because only columns that exist when a migration runs can be patched, and `create_table` adds `created_at`/`updated_at` to every table it makes.
//! A migration that creates a table calls [`widen_all`] at the end of its `up` and the new table's timestamps come out at the same precision as everything else.

use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

/// Scans `information_schema` and widens what it finds; a no-op on Postgres and `SQLite`.
///
/// Scanning beats listing columns by hand: timestamp columns are scattered across `users`, `access_keys`, `buckets`, `objects`, and every table a later migration adds.
/// Running it twice costs one query and changes nothing the second time.
///
/// # Errors
/// When the scan query or one of the `ALTER TABLE` statements fails.
pub async fn widen_all(m: &SchemaManager<'_>) -> Result<(), DbErr> {
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
