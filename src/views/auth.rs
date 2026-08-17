use serde::{Deserialize, Serialize};

use crate::models::_entities::users;

#[derive(Debug, Deserialize, Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub pid: String,
    pub name: String,
    pub must_change_password: bool,
}

impl LoginResponse {
    #[must_use]
    pub fn new(user: &users::Model, token: &str) -> Self {
        Self {
            token: token.to_string(),
            pid: user.pid.to_string(),
            name: user.name.clone(),
            // Wired to the real column in task 2, once the migration adds it.
            must_change_password: false,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CurrentResponse {
    pub pid: String,
    pub name: String,
    pub email: String,
    pub role: String,
    pub max_bytes: i64,
}

impl CurrentResponse {
    #[must_use]
    pub fn new(user: &users::Model) -> Self {
        Self {
            pid: user.pid.to_string(),
            name: user.name.clone(),
            email: user.email.clone(),
            role: user.role.clone(),
            max_bytes: user.max_bytes,
        }
    }
}
