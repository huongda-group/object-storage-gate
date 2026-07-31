use sea_orm::prelude::DateTimeWithTimeZone;
use serde::{Deserialize, Serialize};

use crate::models::access_keys;

/// Fields are listed by hand on purpose: `secret_encrypted` must never reach a
/// response, and a `#[serde(skip)]` on the generated entity would be wiped by
/// the next `cargo loco db entities`.
#[derive(Debug, Deserialize, Serialize)]
pub struct KeyResponse {
    pub pid: String,
    pub access_key_id: String,
    pub label: String,
    pub status: String,
    pub expires_at: Option<DateTimeWithTimeZone>,
    pub days_until_expiry: Option<i64>,
    pub permissions: Vec<String>,
    pub prefixes: Vec<String>,
    pub created_at: DateTimeWithTimeZone,
}

impl KeyResponse {
    #[must_use]
    pub fn new(key: &access_keys::Model, permissions: Vec<String>, prefixes: Vec<String>) -> Self {
        Self {
            pid: key.pid.to_string(),
            access_key_id: key.access_key_id.clone(),
            label: key.label.clone(),
            status: key.effective_status().to_string(),
            expires_at: key.expires_at,
            days_until_expiry: key.days_until_expiry(),
            permissions,
            prefixes,
            created_at: key.created_at,
        }
    }

    #[must_use]
    pub fn from_policy(row: access_keys::KeyWithPolicy) -> Self {
        Self::new(&row.key, row.permissions, row.prefixes)
    }
}

/// The only response that ever carries a secret, and only at create/rotate.
#[derive(Debug, Deserialize, Serialize)]
pub struct CreateKeyResponse {
    #[serde(flatten)]
    pub key: KeyResponse,
    pub secret: String,
}

impl CreateKeyResponse {
    #[must_use]
    pub fn new(
        key: &access_keys::Model,
        permissions: Vec<String>,
        prefixes: Vec<String>,
        secret: String,
    ) -> Self {
        Self {
            key: KeyResponse::new(key, permissions, prefixes),
            secret,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TokenResponse {
    pub token: String,
}
