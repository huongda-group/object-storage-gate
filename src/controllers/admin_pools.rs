//! Admin-only pool management, plus the one reduced listing a non-admin needs.
//!
//! A pool is the upstream store a bucket proxies to.
//! Without at least one configured pool the gateway cannot serve a single S3 request, so this tree is a prerequisite for the whole data plane.
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    controllers::api::{AdminCaller, Caller},
    models::{buckets, pools},
    views::pools::{PoolChoice, PoolResponse},
};

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateBody {
    pub name: String,
    pub provider: String,
    pub region: Option<String>,
    pub api_endpoint: Option<String>,
    pub physical_bucket: String,
    pub access_id: Option<String>,
    /// Plaintext on the way in, stored AES-GCM encrypted, never returned.
    pub access_secret: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateBody {
    pub region: Option<String>,
    pub api_endpoint: Option<String>,
    pub physical_bucket: Option<String>,
    pub access_id: Option<String>,
    /// Absent means keep the stored secret.
    /// The form never echoes it back, so absent cannot mean erase.
    pub access_secret: Option<String>,
}

fn bad_request(e: &ModelError) -> Error {
    Error::BadRequest(e.to_string())
}

async fn load(db: &DatabaseConnection, pid: &str) -> Result<pools::Model> {
    pools::Model::find_by_pid(db, pid)
        .await
        .map_err(|_| Error::NotFound)
}

#[debug_handler]
async fn index(_admin: AdminCaller, State(ctx): State<AppContext>) -> Result<Response> {
    let rows = pools::Model::list_all(&ctx.db).await?;
    format::json(rows.iter().map(PoolResponse::new).collect::<Vec<_>>())
}

/// Every pool, as a bucket-creation form needs to see them: name and provider, no credentials, no physical bucket.
#[debug_handler]
async fn choices(_caller: Caller, State(ctx): State<AppContext>) -> Result<Response> {
    let rows = pools::Model::list_all(&ctx.db).await?;
    format::json(rows.iter().map(PoolChoice::new).collect::<Vec<_>>())
}

#[debug_handler]
async fn create(
    _admin: AdminCaller,
    State(ctx): State<AppContext>,
    Json(body): Json<CreateBody>,
) -> Result<Response> {
    let pool = pools::Model::create(
        &ctx.db,
        &pools::CreateParams {
            name: body.name,
            provider: body.provider,
            region: body.region,
            api_endpoint: body.api_endpoint,
            physical_bucket: body.physical_bucket,
            access_id: body.access_id,
            access_secret: body.access_secret,
        },
    )
    .await
    .map_err(|e| bad_request(&e))?;

    format::json(PoolResponse::new(&pool))
}

#[debug_handler]
async fn show(
    _admin: AdminCaller,
    Path(pid): Path<String>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    let pool = load(&ctx.db, &pid).await?;
    format::json(PoolResponse::new(&pool))
}

#[debug_handler]
async fn update(
    _admin: AdminCaller,
    Path(pid): Path<String>,
    State(ctx): State<AppContext>,
    Json(body): Json<UpdateBody>,
) -> Result<Response> {
    let pool = load(&ctx.db, &pid).await?;
    let updated = pool
        .update_config(
            &ctx.db,
            &pools::UpdateParams {
                region: body.region,
                api_endpoint: body.api_endpoint,
                physical_bucket: body.physical_bucket,
                access_id: body.access_id,
                access_secret: body.access_secret,
            },
        )
        .await
        .map_err(|e| bad_request(&e))?;

    format::json(PoolResponse::new(&updated))
}

#[debug_handler]
async fn destroy(
    _admin: AdminCaller,
    Path(pid): Path<String>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    let db = &ctx.db;
    let pool = load(db, &pid).await?;

    // The foreign key is RESTRICT, so the DB would refuse anyway — but a 400 with a sentence beats a 500 with a constraint name, and SQLite has no such key at all.
    let count = buckets::Model::count_for_pool(db, pool.id).await?;
    if count > 0 {
        return Err(Error::BadRequest(format!(
            "{count} bucket(s) still use this pool; move or delete them first"
        )));
    }

    let am: pools::ActiveModel = pool.into();
    am.delete(db).await?;

    format::json(())
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/admin/pools")
        .add("/", get(index).post(create))
        .add("/{pid}", get(show).patch(update).delete(destroy))
}

/// The non-admin listing lives on its own prefix so it cannot inherit the admin tree's gate by accident.
pub fn user_routes() -> Routes {
    Routes::new().prefix("/api/pools").add("/", get(choices))
}
