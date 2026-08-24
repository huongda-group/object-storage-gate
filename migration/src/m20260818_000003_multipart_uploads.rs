use loco_rs::schema::*;
use sea_orm::DatabaseBackend;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        // The client's UploadId is this row's pid, never the upstream one: an upstream identifier in a client's hands is a piece of the physical layout the gateway exists to hide.
        // There is no parts table — the store keeps the parts, and the client sends every part ETag back in CompleteMultipartUpload. The only thing the gateway must remember is how much quota it is holding.
        create_table(
            m,
            "multipart_uploads",
            &[
                ("id", ColType::PkAuto),
                ("pid", ColType::UuidUniq),
                ("object_key", ColType::String),
                ("upstream_upload_id", ColType::String),
                ("reserved_bytes", ColType::BigIntegerWithDefault(0)),
            ],
            &[("buckets", "")],
        )
        .await?;

        // Not unique: S3 allows several open uploads on the same key at once.
        m.create_index(
            Index::create()
                .name("idx_multipart_bucket_key")
                .table(Alias::new("multipart_uploads"))
                .col(Alias::new("bucket_id"))
                .to_owned(),
        )
        .await?;

        // An object key is up to 1024 bytes; ColType::String is varchar(255) on MySQL, which would silently refuse a legal key.
        // SQLite ignores varchar lengths and cannot modify a column, so it is skipped — the same shape m20260817_000003 uses.
        if matches!(m.get_database_backend(), DatabaseBackend::MySql) {
            m.get_connection()
                .execute_unprepared(
                    "ALTER TABLE multipart_uploads MODIFY object_key VARCHAR(1024) NOT NULL",
                )
                .await?;
        }
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        drop_table(m, "multipart_uploads").await?;
        Ok(())
    }
}
