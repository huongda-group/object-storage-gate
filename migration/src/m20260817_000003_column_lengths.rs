use sea_orm::{ConnectionTrait, DatabaseBackend};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// `ColType::String` becomes `varchar(255)` on MySQL and an unbounded `varchar` on Postgres, so the same value is accepted on one backend and rejected on another.
/// These lengths are the ones the application already promises: `MAX_PREFIX_LEN` is 512, and S3 allows object keys up to 1024 bytes.
const WIDENINGS: &[(&str, &str, u32)] = &[
    ("access_key_prefixes", "prefix", 512),
    ("objects", "object_key", 1024),
    ("buckets", "name", 255),
];

/// The composite unique index that has to be rebuilt around the `object_key` widening on MySQL.
const IDX_OBJECTS_BUCKET_KEY: &str = "idx_objects_bucket_key";

/// A scratch index on `objects.bucket_id`, created only so the foreign key has something to lean on while the composite index is dropped.
/// InnoDB refuses to drop the last index a foreign key can use, and the composite one is it.
const IDX_OBJECTS_FK_SCRATCH: &str = "idx_objects_bucket_fk_scratch";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        // SQLite cannot modify a column, and does not need to: it ignores varchar lengths entirely, so these columns already hold any length.
        if matches!(m.get_database_backend(), DatabaseBackend::Sqlite) {
            return Ok(());
        }
        let is_mysql = matches!(m.get_database_backend(), DatabaseBackend::MySql);

        // ponytail: on MySQL the rebuilt index covers a 700-character prefix of the key, not the whole key.
        // InnoDB caps a single index at 3072 bytes and a utf8mb4 varchar(1024) alone is 4096, so the full key cannot be indexed.
        // Ceiling: two keys identical in their first 700 characters and different after that collide as duplicates on MySQL only.
        // Upgrade path: index a hash column of the full key if that ever happens in practice.
        if is_mysql && m.has_index("objects", IDX_OBJECTS_BUCKET_KEY).await? {
            m.create_index(
                Index::create()
                    .name(IDX_OBJECTS_FK_SCRATCH)
                    .table(Alias::new("objects"))
                    .col(Alias::new("bucket_id"))
                    .to_owned(),
            )
            .await?;
            m.drop_index(
                Index::drop()
                    .name(IDX_OBJECTS_BUCKET_KEY)
                    .table(Alias::new("objects"))
                    .to_owned(),
            )
            .await?;
        }

        for (table, column, len) in WIDENINGS {
            m.alter_table(
                Table::alter()
                    .table(Alias::new(*table))
                    .modify_column(
                        ColumnDef::new(Alias::new(*column))
                            .string_len(*len)
                            .not_null()
                            .to_owned(),
                    )
                    .to_owned(),
            )
            .await?;
        }

        if is_mysql {
            m.get_connection()
                .execute_unprepared(&format!(
                    "CREATE UNIQUE INDEX {IDX_OBJECTS_BUCKET_KEY} ON objects (bucket_id, object_key(700))"
                ))
                .await?;

            if m.has_index("objects", IDX_OBJECTS_FK_SCRATCH).await? {
                m.drop_index(
                    Index::drop()
                        .name(IDX_OBJECTS_FK_SCRATCH)
                        .table(Alias::new("objects"))
                        .to_owned(),
                )
                .await?;
            }
        }

        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        if matches!(m.get_database_backend(), DatabaseBackend::Sqlite) {
            return Ok(());
        }
        let is_mysql = matches!(m.get_database_backend(), DatabaseBackend::MySql);

        if is_mysql && m.has_index("objects", IDX_OBJECTS_BUCKET_KEY).await? {
            m.create_index(
                Index::create()
                    .name(IDX_OBJECTS_FK_SCRATCH)
                    .table(Alias::new("objects"))
                    .col(Alias::new("bucket_id"))
                    .to_owned(),
            )
            .await?;
            m.drop_index(
                Index::drop()
                    .name(IDX_OBJECTS_BUCKET_KEY)
                    .table(Alias::new("objects"))
                    .to_owned(),
            )
            .await?;
        }

        for (table, column, _len) in WIDENINGS {
            m.alter_table(
                Table::alter()
                    .table(Alias::new(*table))
                    .modify_column(
                        ColumnDef::new(Alias::new(*column))
                            .string()
                            .not_null()
                            .to_owned(),
                    )
                    .to_owned(),
            )
            .await?;
        }

        if is_mysql {
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

            if m.has_index("objects", IDX_OBJECTS_FK_SCRATCH).await? {
                m.drop_index(
                    Index::drop()
                        .name(IDX_OBJECTS_FK_SCRATCH)
                        .table(Alias::new("objects"))
                        .to_owned(),
                )
                .await?;
            }
        }

        Ok(())
    }
}
