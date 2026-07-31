use loco_rs::schema::*;
use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::ConnectionTrait;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "buckets",
            &[
                ("id", ColType::PkAuto),
                ("pid", ColType::UuidUniq),
                ("name", ColType::String),
                ("max_bytes", ColType::BigIntegerWithDefault(0)),
                ("used_bytes", ColType::BigIntegerWithDefault(0)),
                ("reserved_bytes", ColType::BigIntegerWithDefault(0)),
                ("object_count", ColType::BigIntegerWithDefault(0)),
            ],
            // `users?` = nullable owner: a NULL user_id is a system pool, the
            // gateway-wide bucket the admin Pool screen lists as "hệ thống".
            // ponytail: FK is ON DELETE SET NULL, so deleting a user would turn
            // their buckets into system pools — the delete-user API must drop the
            // user's buckets first (slice #7).
            &[("users?", "")],
        )
        .await?;
        // Unique per owner. COALESCE, not a plain (user_id, name) index: NULLs
        // compare distinct, which would let two system pools share a name. 0 is a
        // safe sentinel because user ids start at 1.
        m.get_connection()
            .execute_unprepared(
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_buckets_owner_name \
                 ON buckets (COALESCE(user_id, 0), name)",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        drop_table(m, "buckets").await?;
        Ok(())
    }
}
