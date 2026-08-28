use serde::{Deserialize, Serialize};

use crate::models::_entities::users;

/// The admin-facing shape of a user.
/// Lists fields by hand so a new column never leaks into the API by accident — the password hash and the PAT both live on this model.
#[derive(Debug, Deserialize, Serialize)]
pub struct AdminUserResponse {
    pub pid: String,
    pub email: String,
    pub name: String,
    pub role: String,
    pub max_bytes: i64,
    pub used_bytes: i64,
    pub reserved_bytes: i64,
    pub must_change_password: bool,
    pub created_at: String,
}

impl AdminUserResponse {
    #[must_use]
    pub fn new(user: &users::Model) -> Self {
        Self {
            pid: user.pid.to_string(),
            email: user.email.clone(),
            name: user.name.clone(),
            role: user.role.clone(),
            max_bytes: user.max_bytes,
            used_bytes: user.used_bytes,
            reserved_bytes: user.reserved_bytes,
            must_change_password: user.must_change_password,
            created_at: user.created_at.to_rfc3339(),
        }
    }
}
