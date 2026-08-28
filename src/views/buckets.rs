use serde::{Deserialize, Serialize};

use crate::models::_entities::{buckets, pools};

/// The owner-facing shape of a bucket.
/// Lists fields by hand so a column added to the table never leaks into a response by default.
#[derive(Debug, Deserialize, Serialize)]
pub struct BucketDetail {
    pub pid: String,
    pub name: String,
    pub max_bytes: i64,
    pub used_bytes: i64,
    pub reserved_bytes: i64,
    pub object_count: i64,
    pub public_enabled: bool,
    /// The pool's `pid`, never its row id.
    pub pool_id: String,
    pub pool_name: String,
    pub created_at: String,
}

impl BucketDetail {
    /// `pool` is optional only so a bucket whose pool vanished still renders.
    /// The foreign key is `RESTRICT`, so that cannot happen on Postgres or `MySQL`; on `SQLite`, which has no such constraint, an empty pool name is a visible symptom rather than a 500.
    #[must_use]
    pub fn new(bucket: &buckets::Model, pool: Option<&pools::Model>) -> Self {
        Self {
            pid: bucket.pid.to_string(),
            name: bucket.name.clone(),
            max_bytes: bucket.max_bytes,
            used_bytes: bucket.used_bytes,
            reserved_bytes: bucket.reserved_bytes,
            object_count: bucket.object_count,
            public_enabled: bucket.public_enabled,
            pool_id: pool.map(|p| p.pid.to_string()).unwrap_or_default(),
            pool_name: pool.map(|p| p.name.clone()).unwrap_or_default(),
            created_at: bucket.created_at.to_rfc3339(),
        }
    }
}
