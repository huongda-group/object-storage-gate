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
            "objects",
            &[
                ("id", ColType::PkAuto),
                ("pid", ColType::UuidUniq),
                ("object_key", ColType::String),
                ("size", ColType::BigIntegerWithDefault(0)),
                ("etag", ColType::StringWithDefault(String::new())),
                (
                    "content_type",
                    ColType::StringWithDefault("application/octet-stream".to_string()),
                ),
            ],
            &[("buckets", "")],
        )
        .await?;
        m.get_connection()
            .execute_unprepared(
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_objects_bucket_key \
                 ON objects (bucket_id, object_key)",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        drop_table(m, "objects").await?;
        Ok(())
    }
}
