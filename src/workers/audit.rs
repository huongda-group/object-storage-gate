//! Writes audit entries off the request path.
//!
//! One INSERT is about a millisecond, but it is a millisecond on every S3 request including the ones that are already slow — and a database hiccup would turn successful uploads into 500s.
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};

use crate::models::audit_logs::{self, AuditEntry};

/// The queue payload.
/// A newtype so the queue's type registry keys on this worker, not on a type the models own.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditArgs(pub AuditEntry);

pub struct AuditWorker {
    pub ctx: AppContext,
}

#[async_trait]
impl BackgroundWorker<AuditArgs> for AuditWorker {
    fn build(ctx: &AppContext) -> Self {
        Self { ctx: ctx.clone() }
    }

    async fn perform(&self, args: AuditArgs) -> Result<()> {
        audit_logs::Model::record(&self.ctx.db, &args.0).await?;
        Ok(())
    }
}
