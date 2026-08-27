//! Aborts multipart uploads nobody finished.
//!
//! An abandoned upload holds quota and holds parts in the store, and nothing else ever notices:
//! the client that started it is gone.
use loco_rs::prelude::*;

use crate::{
    models::{multipart_uploads, pools, quota},
    s3::upstream::{self, UpstreamRequest},
};

/// How old an upload has to be before it is considered abandoned.
fn max_age_days() -> i64 {
    std::env::var("OSG_MULTIPART_MAX_AGE_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(7)
}

pub struct CleanupMultipart;

#[async_trait]
impl Task for CleanupMultipart {
    fn task(&self) -> TaskInfo {
        TaskInfo {
            name: "cleanup_multipart".to_string(),
            detail: "abort multipart uploads left open, release their quota".to_string(),
        }
    }

    async fn run(&self, ctx: &AppContext, _vars: &task::Vars) -> Result<()> {
        let stale = multipart_uploads::Model::older_than(&ctx.db, max_age_days()).await?;
        let total = stale.len();
        let mut aborted = 0_usize;
        let mut failed = 0_usize;

        for upload in stale {
            // Per-item error handling on purpose: one upload whose pool is unreachable must not stop the sweep, or a single bad row keeps every other abandoned upload holding quota forever.
            match abort_one(ctx, &upload).await {
                Ok(()) => aborted += 1,
                Err(e) => {
                    failed += 1;
                    tracing::error!(
                        upload = %upload.pid,
                        error = %e,
                        "could not abort an abandoned multipart upload"
                    );
                }
            }
        }

        tracing::info!(total, aborted, failed, "multipart cleanup finished");
        println!("multipart cleanup: {aborted} aborted, {failed} failed, {total} considered");
        Ok(())
    }
}

async fn abort_one(ctx: &AppContext, upload: &multipart_uploads::Model) -> Result<()> {
    let bucket = crate::models::buckets::Entity::find_by_id(upload.bucket_id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| Error::string("bucket is gone"))?;
    let pool = pools::Model::find_by_id(&ctx.db, bucket.pool_id).await?;

    // The store is told first: dropping the row before the store has forgotten the parts leaves them paid for by nobody and invisible to everything.
    let client = upstream::Client::new(&pool).map_err(|e| Error::string(&e.to_string()))?;
    let user = crate::models::users::Model::find_by_id(&ctx.db, bucket.user_id.unwrap_or_default())
        .await
        .ok();
    let physical = format!(
        "{}/{}/{}",
        user.map(|u| u.pid.to_string()).unwrap_or_default(),
        bucket.name,
        upload.object_key
    );

    client
        .send(UpstreamRequest {
            method: "DELETE".to_string(),
            key: physical,
            query: vec![("uploadId".to_string(), upload.upstream_upload_id.clone())],
            headers: Vec::new(),
            body: upstream::Body::Empty,
        })
        .await
        .map_err(|e| Error::string(&e.to_string()))?;

    let hold = quota::held(&ctx.db, upload.bucket_id, upload.reserved_bytes).await?;
    quota::release(&ctx.db, &hold).await?;
    upload.clone().remove(&ctx.db).await?;
    Ok(())
}
