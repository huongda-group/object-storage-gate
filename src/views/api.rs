use serde::{Deserialize, Serialize};

use crate::models::_entities::{buckets, users};

#[derive(Debug, Deserialize, Serialize)]
pub struct WhoamiResponse {
    pub pid: String,
    pub email: String,
    pub name: String,
    pub role: String,
}

impl WhoamiResponse {
    #[must_use]
    pub fn new(user: &users::Model) -> Self {
        Self {
            pid: user.pid.to_string(),
            email: user.email.clone(),
            name: user.name.clone(),
            role: user.role.clone(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BucketResponse {
    pub name: String,
    pub max_bytes: i64,
    pub used_bytes: i64,
    pub object_count: i64,
    pub public_enabled: bool,
}

impl BucketResponse {
    #[must_use]
    pub fn new(bucket: &buckets::Model) -> Self {
        Self {
            name: bucket.name.clone(),
            max_bytes: bucket.max_bytes,
            used_bytes: bucket.used_bytes,
            object_count: bucket.object_count,
            public_enabled: bucket.public_enabled,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UsageResponse {
    pub used_bytes: i64,
    pub reserved_bytes: i64,
    pub max_bytes: i64,
    pub object_count: i64,
    pub bucket_count: i64,
}

impl UsageResponse {
    #[must_use]
    pub fn new(user: &users::Model, buckets: &[buckets::Model]) -> Self {
        Self {
            used_bytes: user.used_bytes,
            reserved_bytes: user.reserved_bytes,
            max_bytes: user.max_bytes,
            object_count: buckets.iter().map(|b| b.object_count).sum(),
            bucket_count: buckets.len() as i64,
        }
    }
}
