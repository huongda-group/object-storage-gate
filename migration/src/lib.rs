#![allow(elided_lifetimes_in_paths)]
#![allow(clippy::wildcard_imports)]
pub use sea_orm_migration::prelude::*;
mod m20220101_000001_users;
mod m20260724_000001_users_account;
mod m20260724_000002_buckets;
mod m20260724_000003_access_keys;
mod m20260724_000004_access_key_permissions;
mod m20260724_000005_access_key_prefixes;
mod m20260724_000006_objects;
mod m20260724_000007_bucket_backend_store;
mod m20260815_000001_mysql_timestamp_precision;
mod m20260817_000001_auth_teardown;
mod m20260817_000002_hash_api_key;
mod m20260817_000003_column_lengths;
mod m20260817_000004_binary_collation;
mod m20260817_000005_hot_indexes;
mod m20260817_000006_quota_checks;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20220101_000001_users::Migration),
            Box::new(m20260724_000001_users_account::Migration),
            Box::new(m20260724_000002_buckets::Migration),
            Box::new(m20260724_000003_access_keys::Migration),
            Box::new(m20260724_000004_access_key_permissions::Migration),
            Box::new(m20260724_000005_access_key_prefixes::Migration),
            Box::new(m20260724_000006_objects::Migration),
            Box::new(m20260724_000007_bucket_backend_store::Migration),
            Box::new(m20260815_000001_mysql_timestamp_precision::Migration),
            Box::new(m20260817_000001_auth_teardown::Migration),
            Box::new(m20260817_000002_hash_api_key::Migration),
            Box::new(m20260817_000003_column_lengths::Migration),
            Box::new(m20260817_000004_binary_collation::Migration),
            Box::new(m20260817_000005_hot_indexes::Migration),
            Box::new(m20260817_000006_quota_checks::Migration),
            // inject-above (do not remove this comment)
        ]
    }
}
