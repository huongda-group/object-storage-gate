use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// `(index name, table, column, unique)`.
///
/// Postgres and `SQLite` do not index foreign-key columns automatically — only `MySQL` `InnoDB` does — and `users.pid` had no index at all despite `find_by_pid` running on every authenticated request.
const INDEXES: &[(&str, &str, &str, bool)] = &[
    ("idx_users_pid", "users", "pid", true),
    ("idx_users_api_key_prefix", "users", "api_key_prefix", false),
    ("idx_access_keys_user", "access_keys", "user_id", false),
    ("idx_buckets_user", "buckets", "user_id", false),
    (
        "idx_access_key_prefixes_key",
        "access_key_prefixes",
        "access_key_id",
        false,
    ),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        for (name, table, column, unique) in INDEXES {
            // `has_index` instead of `IF NOT EXISTS`: MySQL has no such syntax for indexes.
            if m.has_index(table, name).await? {
                continue;
            }
            let mut idx = Index::create();
            idx.name(*name)
                .table(Alias::new(*table))
                .col(Alias::new(*column));
            if *unique {
                idx.unique();
            }
            m.create_index(idx.clone()).await?;
        }
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        for (name, table, _column, _unique) in INDEXES {
            if !m.has_index(table, name).await? {
                continue;
            }
            m.drop_index(
                Index::drop()
                    .name(*name)
                    .table(Alias::new(*table))
                    .to_owned(),
            )
            .await?;
        }
        Ok(())
    }
}
