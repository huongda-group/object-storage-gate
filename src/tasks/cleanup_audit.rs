//! Drops audit entries past their retention window.
//!
//! Audit is append-only on the request path, so without this the table only grows.
use loco_rs::prelude::*;

use crate::models::audit_logs;

/// How long an audit entry is kept.
///
/// Ninety days by default, which is the shortest window that still covers a quarterly review.
fn retention_days() -> i64 {
    std::env::var("OSG_AUDIT_RETENTION_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(90)
}

pub struct CleanupAudit;

#[async_trait]
impl Task for CleanupAudit {
    fn task(&self) -> TaskInfo {
        TaskInfo {
            name: "cleanup_audit".to_string(),
            detail: "delete audit entries past the retention window".to_string(),
        }
    }

    async fn run(&self, ctx: &AppContext, _vars: &task::Vars) -> Result<()> {
        let days = retention_days();
        let removed = audit_logs::Model::delete_older_than(&ctx.db, days).await?;
        tracing::info!(removed, days, "audit cleanup finished");
        println!("audit cleanup: {removed} entries older than {days} days removed");
        Ok(())
    }
}
