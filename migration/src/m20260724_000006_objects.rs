use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

const IDX_OBJECTS_BUCKET_KEY: &str = "idx_objects_bucket_key";

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
        // `has_index` instead of `IF NOT EXISTS`: MySQL has no such syntax for indexes.
        if !m.has_index("objects", IDX_OBJECTS_BUCKET_KEY).await? {
            m.create_index(
                Index::create()
                    .name(IDX_OBJECTS_BUCKET_KEY)
                    .table(Alias::new("objects"))
                    .col(Alias::new("bucket_id"))
                    .col(Alias::new("object_key"))
                    .unique()
                    .to_owned(),
            )
            .await?;
        }
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        drop_table(m, "objects").await?;
        Ok(())
    }
}
