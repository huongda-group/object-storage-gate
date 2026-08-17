use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        // `users.api_key` now holds an Argon2 hash, which cannot be looked up by the plaintext it hashes.
        // The token carries a plaintext prefix instead: `osg_pat_<prefix12>_<secret32>`.
        // This column stores that prefix and is what the lookup queries; the hash then verifies the full token.
        // Same shape GitHub and Stripe use, and it needs no backend-specific SQL.
        add_column(m, "users", "api_key_prefix", ColType::StringNull).await?;
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        remove_column(m, "users", "api_key_prefix").await?;
        Ok(())
    }
}
