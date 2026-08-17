//! Recomputes quota counters from the object rows.
//!
//! Run it on a schedule. The guarded UPDATEs in `models::quota` survive concurrency but not a process that dies between reserve and commit, and this is what cleans up after that.
use loco_rs::prelude::*;

use crate::models::quota;

pub struct ReconcileQuota;

#[async_trait]
impl Task for ReconcileQuota {
    fn task(&self) -> TaskInfo {
        TaskInfo {
            name: "reconcile_quota".to_string(),
            detail: "recompute bucket and account quota counters from the object rows".to_string(),
        }
    }

    async fn run(&self, app_context: &AppContext, _vars: &task::Vars) -> Result<()> {
        let report = quota::reconcile(&app_context.db).await?;
        tracing::info!(
            buckets_fixed = report.buckets_fixed,
            users_fixed = report.users_fixed,
            "quota reconcile finished"
        );
        println!(
            "reconciled: {} buckets, {} accounts",
            report.buckets_fixed, report.users_fixed
        );
        Ok(())
    }
}
