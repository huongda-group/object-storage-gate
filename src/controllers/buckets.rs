//! Bucket CRUD for the account that owns them.
//!
//! Pools are not reachable here at all — they belong to the admin tree.
use std::collections::HashMap;

use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    controllers::api::Caller,
    models::{buckets, pools},
    views::buckets::BucketDetail,
};

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateParams {
    pub name: String,
    /// The pool's `pid`, not its row id: the API never exposes an incrementing id.
    /// Required — a bucket with no pool has nowhere to proxy to.
    pub pool_id: String,
    /// Required on purpose: `0` means unlimited, and unlimited must be a decision, never a default.
    pub max_bytes: i64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateParams {
    pub max_bytes: Option<i64>,
    pub public_enabled: Option<bool>,
}

fn bad_request(e: &ModelError) -> Error {
    Error::BadRequest(e.to_string())
}

async fn load(db: &DatabaseConnection, user_id: i32, pid: &str) -> Result<buckets::Model> {
    buckets::Model::find_by_pid_for_user(db, pid, user_id)
        .await
        .map_err(|_| Error::NotFound)
}

/// The pool a bucket belongs to, for the response shaper.
/// Looked up rather than joined so `index` can resolve N buckets against one pool query instead of N.
async fn pool_of(db: &DatabaseConnection, bucket: &buckets::Model) -> Result<pools::Model> {
    pools::Model::find_by_id(db, bucket.pool_id)
        .await
        .map_err(|_| Error::NotFound)
}

#[debug_handler]
async fn index(caller: Caller, State(ctx): State<AppContext>) -> Result<Response> {
    let rows = buckets::Model::list_for_user(&ctx.db, caller.user.id).await?;
    // One query for every pool, not one per bucket.
    let pools = pools::Model::list_all(&ctx.db).await?;
    let by_id: HashMap<i32, &pools::Model> = pools.iter().map(|p| (p.id, p)).collect();

    let out: Vec<BucketDetail> = rows
        .iter()
        .map(|b| BucketDetail::new(b, by_id.get(&b.pool_id).copied()))
        .collect();
    format::json(out)
}

#[debug_handler]
async fn create(
    caller: Caller,
    State(ctx): State<AppContext>,
    Json(params): Json<CreateParams>,
) -> Result<Response> {
    let pool = pools::Model::find_by_pid(&ctx.db, &params.pool_id)
        .await
        .map_err(|_| Error::BadRequest("unknown pool".to_string()))?;

    let bucket = buckets::Model::create(
        &ctx.db,
        caller.user.id,
        pool.id,
        &params.name,
        params.max_bytes,
    )
    .await
    .map_err(|e| bad_request(&e))?;

    format::json(BucketDetail::new(&bucket, Some(&pool)))
}

#[debug_handler]
async fn show(
    caller: Caller,
    Path(pid): Path<String>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    let bucket = load(&ctx.db, caller.user.id, &pid).await?;
    let pool = pool_of(&ctx.db, &bucket).await?;
    format::json(BucketDetail::new(&bucket, Some(&pool)))
}

#[debug_handler]
async fn update(
    caller: Caller,
    Path(pid): Path<String>,
    State(ctx): State<AppContext>,
    Json(params): Json<UpdateParams>,
) -> Result<Response> {
    let bucket = load(&ctx.db, caller.user.id, &pid).await?;

    if let Some(max_bytes) = params.max_bytes {
        if max_bytes < 0 {
            return Err(Error::BadRequest(
                "max_bytes must not be negative".to_string(),
            ));
        }
        // A quota below what is already stored would make every future write fail with no way back.
        if max_bytes != 0 && max_bytes < bucket.used_bytes {
            return Err(Error::BadRequest(
                "quota is below the bytes already stored in this bucket".to_string(),
            ));
        }
    }

    let mut am: buckets::ActiveModel = bucket.into();
    if let Some(max_bytes) = params.max_bytes {
        am.max_bytes = ActiveValue::set(max_bytes);
    }
    if let Some(public_enabled) = params.public_enabled {
        am.public_enabled = ActiveValue::set(public_enabled);
    }
    let updated = am.update(&ctx.db).await?;
    let pool = pool_of(&ctx.db, &updated).await?;

    format::json(BucketDetail::new(&updated, Some(&pool)))
}

#[debug_handler]
async fn destroy(
    caller: Caller,
    Path(pid): Path<String>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    let bucket = load(&ctx.db, caller.user.id, &pid).await?;

    // ponytail: deletes the metadata rows only; nothing is removed from the backend store, because there is no backend-store client yet.
    // Ceiling: once the proxy slice lands this must either delete upstream or keep refusing while the bucket is non-empty.
    if bucket.object_count > 0 {
        return Err(Error::BadRequest(
            "bucket is not empty; delete its objects first".to_string(),
        ));
    }

    let am: buckets::ActiveModel = bucket.into();
    am.delete(&ctx.db).await?;

    format::json(())
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/buckets")
        .add("/", get(index).post(create))
        .add("/{pid}", get(show).patch(update).delete(destroy))
}
