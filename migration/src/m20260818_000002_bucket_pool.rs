use loco_rs::schema::*;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// Placeholder physical bucket for the backfill pool.
/// Deliberately not a plausible name: every S3 request against this pool must fail until an admin replaces it, and the failure should read as unconfigured rather than as a typo.
const BACKFILL_PHYSICAL_BUCKET: &str = "CHANGE-ME";

/// The five upstream-store columns that move from `buckets` to `pools`.
const MOVED_COLUMNS: &[&str] = &[
    "provider",
    "region",
    "api_endpoint",
    "access_id",
    "access_secret_encrypted",
];

/// `SQLite` keeps `pool_id` nullable and gains no foreign key: it can neither modify a column nor add a constraint after the fact.
/// `buckets::Model::create` always supplies a `pool_id` and checks the pool exists, so no path can create a bucket without one on any backend.
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        let conn = m.get_connection();
        let backend = m.get_database_backend();

        // Count existing buckets before adding a NOT NULL column with no default.
        let existing: i64 = {
            let row = conn
                .query_one(Statement::from_string(
                    backend,
                    "SELECT COUNT(*) AS c FROM buckets".to_string(),
                ))
                .await?;
            row.map_or(Ok(0), |r| r.try_get::<i64>("", "c"))?
        };

        // Nullable first, backfill, then tighten — the only order that works with rows present.
        add_column(m, "buckets", "pool_id", ColType::IntegerNull).await?;

        if existing > 0 {
            // Built through sea-query rather than interpolated: `pid` is `uuid` on Postgres but `binary(16)` on MySQL, and only the query builder knows how to encode a Uuid for each.
            // `created_at` and `updated_at` are left out because `create_table` gave them a CURRENT_TIMESTAMP default on all three backends.
            let insert = Query::insert()
                .into_table(Alias::new("pools"))
                .columns([
                    Alias::new("pid"),
                    Alias::new("name"),
                    Alias::new("provider"),
                    Alias::new("physical_bucket"),
                ])
                .values_panic([
                    uuid::Uuid::new_v4().into(),
                    "default".into(),
                    "custom".into(),
                    BACKFILL_PHYSICAL_BUCKET.into(),
                ])
                .to_owned();
            conn.execute(backend.build(&insert)).await?;
            conn.execute(Statement::from_string(
                backend,
                "UPDATE buckets SET pool_id = (SELECT id FROM pools WHERE name = 'default')"
                    .to_string(),
            ))
            .await?;
        }

        if !matches!(backend, DatabaseBackend::Sqlite) {
            m.alter_table(
                Table::alter()
                    .table(Alias::new("buckets"))
                    .modify_column(
                        ColumnDef::new(Alias::new("pool_id"))
                            .integer()
                            .not_null()
                            .to_owned(),
                    )
                    .to_owned(),
            )
            .await?;

            m.alter_table(
                Table::alter()
                    .table(Alias::new("buckets"))
                    .add_foreign_key(
                        TableForeignKey::new()
                            .name("fk_buckets_pool")
                            .from_tbl(Alias::new("buckets"))
                            .from_col(Alias::new("pool_id"))
                            .to_tbl(Alias::new("pools"))
                            .to_col(Alias::new("id"))
                            // RESTRICT, not SET NULL: a silently orphaned bucket is exactly the bug m20260817 had to fix on the users side.
                            .on_delete(ForeignKeyAction::Restrict),
                    )
                    .to_owned(),
            )
            .await?;
        }

        for col in MOVED_COLUMNS {
            remove_column(m, "buckets", col).await?;
        }

        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
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
        add_column(m, "buckets", "access_secret_encrypted", ColType::BlobNull).await?;
        remove_column(m, "buckets", "pool_id").await?;
        Ok(())
    }
}
