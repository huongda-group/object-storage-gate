use serde::{Deserialize, Serialize};

use crate::models::_entities::pools;

/// The admin-facing shape of a pool.
/// Lists fields by hand and has no secret field at all: `access_secret_encrypted` exists to be signed with, never to be read back.
#[derive(Debug, Deserialize, Serialize)]
pub struct PoolResponse {
    pub pid: String,
    pub name: String,
    pub provider: String,
    pub region: Option<String>,
    pub api_endpoint: Option<String>,
    pub physical_bucket: String,
    pub access_id: Option<String>,
    /// Whether a credential is stored at all — enough for the console to warn, without revealing it.
    pub is_configured: bool,
    pub created_at: String,
}

impl PoolResponse {
    #[must_use]
    pub fn new(pool: &pools::Model) -> Self {
        Self {
            pid: pool.pid.to_string(),
            name: pool.name.clone(),
            provider: pool.provider.clone(),
            region: pool.region.clone(),
            api_endpoint: pool.api_endpoint.clone(),
            physical_bucket: pool.physical_bucket.clone(),
            access_id: pool.access_id.clone(),
            is_configured: pool.access_id.is_some() && pool.access_secret_encrypted.is_some(),
            created_at: pool.created_at.to_rfc3339(),
        }
    }
}

/// What a non-admin is allowed to know about a pool: enough to choose one when creating a bucket, nothing more.
///
/// The physical bucket name is deliberately absent — a tenant learning the real layout is what the gateway exists to prevent.
#[derive(Debug, Deserialize, Serialize)]
pub struct PoolChoice {
    pub pid: String,
    pub name: String,
    pub provider: String,
}

impl PoolChoice {
    #[must_use]
    pub fn new(pool: &pools::Model) -> Self {
        Self {
            pid: pool.pid.to_string(),
            name: pool.name.clone(),
            provider: pool.provider.clone(),
        }
    }
}
