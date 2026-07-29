use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "access_keys",
            &[
                ("id", ColType::PkAuto),
                ("pid", ColType::UuidUniq),
                ("access_key_id", ColType::StringUniq),
                ("secret_encrypted", ColType::Blob),
                ("label", ColType::StringWithDefault("primary".to_string())),
                ("status", ColType::StringWithDefault("active".to_string())),
                ("expires_at", ColType::TimestampWithTimeZoneNull),
            ],
            &[("users", "")],
        )
        .await?;
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        drop_table(m, "access_keys").await?;
        Ok(())
    }
}
