use serde::{Deserialize, Serialize};

use crate::models::_entities::buckets;

/// The owner-facing shape of a bucket.
/// Lists fields by hand: the model carries `access_secret_encrypted`, which must never reach a response.
#[derive(Debug, Deserialize, Serialize)]
pub struct BucketDetail {
    pub pid: String,
    pub name: String,
    pub max_bytes: i64,
    pub used_bytes: i64,
    pub reserved_bytes: i64,
    pub object_count: i64,
    pub public_enabled: bool,
    pub created_at: String,
}

impl BucketDetail {
    #[must_use]
    pub fn new(bucket: &buckets::Model) -> Self {
        Self {
            pid: bucket.pid.to_string(),
            name: bucket.name.clone(),
            max_bytes: bucket.max_bytes,
            used_bytes: bucket.used_bytes,
            reserved_bytes: bucket.reserved_bytes,
            object_count: bucket.object_count,
            public_enabled: bucket.public_enabled,
            created_at: bucket.created_at.to_rfc3339(),
        }
    }
}
