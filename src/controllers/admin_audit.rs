//! Admin-only audit read.
//!
//! Read-only on purpose: an audit log an operator can edit is not one anybody can rely on, and
//! retention is the job of the `cleanup_audit` task rather than of a delete button.
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{controllers::api::AdminCaller, models::audit_logs, views::audit::AuditEntryResponse};

/// How many entries one page returns, and the ceiling on what a caller may ask for.
const DEFAULT_LIMIT: u64 = 100;
const MAX_LIMIT: u64 = 1000;

#[derive(Debug, Deserialize, Serialize)]
pub struct ListQuery {
    pub limit: Option<u64>,
    /// Restrict to one account, by row id.
    pub user_id: Option<i32>,
}

#[debug_handler]
async fn index(
    _admin: AdminCaller,
    State(ctx): State<AppContext>,
    Query(params): Query<ListQuery>,
) -> Result<Response> {
    let limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let rows = match params.user_id {
        Some(user_id) => audit_logs::Model::list_for_user(&ctx.db, user_id, limit).await?,
        None => audit_logs::Model::list_recent(&ctx.db, limit).await?,
    };
    format::json(rows.iter().map(AuditEntryResponse::new).collect::<Vec<_>>())
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/admin/audit")
        .add("/", get(index))
}
