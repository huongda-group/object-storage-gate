use serde::{Deserialize, Serialize};

use crate::models::_entities::audit_logs;

/// The admin-facing shape of one audit entry.
///
/// Lists fields by hand, like every other view here, so a column added to the table never leaks into a response by default.
#[derive(Debug, Deserialize, Serialize)]
pub struct AuditEntryResponse {
    pub pid: String,
    pub occurred_at: String,
    pub access_key_id: Option<String>,
    pub object_key: Option<String>,
    pub action: String,
    pub outcome: String,
    pub status_code: i32,
    pub bytes: i64,
    pub duration_ms: i32,
    pub request_id: String,
    pub ip: String,
    pub user_agent: Option<String>,
}

impl AuditEntryResponse {
    #[must_use]
    pub fn new(row: &audit_logs::Model) -> Self {
        Self {
            pid: row.pid.to_string(),
            occurred_at: row.occurred_at.to_rfc3339(),
            access_key_id: row.access_key_id.clone(),
            object_key: row.object_key.clone(),
            action: row.action.clone(),
            outcome: row.outcome.clone(),
            status_code: row.status_code,
            bytes: row.bytes,
            duration_ms: row.duration_ms,
            request_id: row.request_id.clone(),
            ip: row.ip.clone(),
            user_agent: row.user_agent.clone(),
        }
    }
}
