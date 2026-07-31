use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        add_column(
            m,
            "users",
            "role",
            ColType::StringWithDefault("user".to_string()),
        )
        .await?;
        add_column(m, "users", "max_bytes", ColType::BigIntegerWithDefault(0)).await?;
        add_column(m, "users", "used_bytes", ColType::BigIntegerWithDefault(0)).await?;
        add_column(
            m,
            "users",
            "reserved_bytes",
            ColType::BigIntegerWithDefault(0),
        )
        .await?;
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        remove_column(m, "users", "reserved_bytes").await?;
        remove_column(m, "users", "used_bytes").await?;
        remove_column(m, "users", "max_bytes").await?;
        remove_column(m, "users", "role").await?;
        Ok(())
    }
}
