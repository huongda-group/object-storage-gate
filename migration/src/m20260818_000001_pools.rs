use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        // A pool is an upstream store plus the physical bucket inside it.
        // It is deliberately not a `buckets` row: `user_id IS NULL` as a sentinel for "system pool" is what turned a deleted owner's private bucket into a shared one, which m20260817 had to fix.
        // A client can never address a pool.
        create_table(
            m,
            "pools",
            &[
                ("id", ColType::PkAuto),
                ("pid", ColType::UuidUniq),
                ("name", ColType::StringUniq),
                ("provider", ColType::StringWithDefault("aws".to_string())),
                ("region", ColType::StringNull),
                ("api_endpoint", ColType::StringNull),
                ("physical_bucket", ColType::String),
                ("access_id", ColType::StringNull),
                // Same AES-GCM envelope as access_keys.secret_encrypted (models/crypto.rs).
                // Reversible on purpose: the gateway signs upstream requests with it.
                ("access_secret_encrypted", ColType::BlobNull),
            ],
            &[],
        )
        .await?;

        // create_table adds created_at/updated_at, which are precision 0 on MySQL and round to the second.
        crate::mysql_timestamps::widen_all(m).await?;
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        drop_table(m, "pools").await?;
        Ok(())
    }
}
