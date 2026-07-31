use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

/// Backend-store config per bucket, as the admin Pool form edits it
/// (`console-object-storage-gate/project/Admin Buckets.dc.html`): which object
/// store this bucket proxies to, and whether its objects are publicly readable.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        add_column(
            m,
            "buckets",
            "provider",
            ColType::StringWithDefault("internal".to_string()),
        )
        .await?;
        add_column(m, "buckets", "region", ColType::StringNull).await?;
        add_column(m, "buckets", "api_endpoint", ColType::StringNull).await?;
        add_column(m, "buckets", "access_id", ColType::StringNull).await?;
        // Same AES-GCM envelope as access_keys.secret_encrypted (models/crypto.rs).
        // Reversible on purpose: the gateway has to sign upstream requests with it.
        add_column(m, "buckets", "access_secret_encrypted", ColType::BlobNull).await?;
        add_column(
            m,
            "buckets",
            "public_enabled",
            ColType::BooleanWithDefault(false),
        )
        .await?;
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        remove_column(m, "buckets", "public_enabled").await?;
        remove_column(m, "buckets", "access_secret_encrypted").await?;
        remove_column(m, "buckets", "access_id").await?;
        remove_column(m, "buckets", "api_endpoint").await?;
        remove_column(m, "buckets", "region").await?;
        remove_column(m, "buckets", "provider").await?;
        Ok(())
    }
}
