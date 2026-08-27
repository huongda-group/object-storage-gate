use loco_rs::schema::*;
use sea_orm::DatabaseBackend;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        // No foreign key to users or buckets on purpose: audit has to outlive the account it describes, or deleting a user erases the record of what its keys did.
        create_table(
            m,
            "audit_logs",
            &[
                ("id", ColType::PkAuto),
                ("pid", ColType::UuidUniq),
                ("occurred_at", ColType::TimestampWithTimeZone),
                ("user_id", ColType::IntegerNull),
                ("access_key_id", ColType::StringNull),
                ("bucket_id", ColType::IntegerNull),
                ("object_key", ColType::StringNull),
                ("action", ColType::String),
                ("outcome", ColType::String),
                ("status_code", ColType::Integer),
                ("bytes", ColType::BigIntegerWithDefault(0)),
                ("duration_ms", ColType::IntegerWithDefault(0)),
                ("request_id", ColType::String),
                ("ip", ColType::String),
                ("user_agent", ColType::StringNull),
            ],
            &[],
        )
        .await?;

        for (name, col) in [
            ("idx_audit_occurred", "occurred_at"),
            ("idx_audit_user", "user_id"),
        ] {
            m.create_index(
                Index::create()
                    .name(name)
                    .table(Alias::new("audit_logs"))
                    .col(Alias::new(col))
                    .to_owned(),
            )
            .await?;
        }

        if matches!(m.get_database_backend(), DatabaseBackend::MySql) {
            // A new TIMESTAMP column on MySQL defaults to precision 0 and rounds to the second, so two requests 100ms apart would share a timestamp and the order of events in an incident would be lost.
            // m20260815_000001 only widened columns that existed when it ran.
            m.get_connection()
                .execute_unprepared(
                    "ALTER TABLE audit_logs MODIFY occurred_at TIMESTAMP(6) NOT NULL",
                )
                .await?;
            // An object key is up to 1024 bytes; ColType::String is varchar(255) here.
            m.get_connection()
                .execute_unprepared("ALTER TABLE audit_logs MODIFY object_key VARCHAR(1024) NULL")
                .await?;
        }
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        drop_table(m, "audit_logs").await?;
        Ok(())
    }
}
