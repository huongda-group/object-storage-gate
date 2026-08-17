//! The account API: access keys, the management token, and read-only account state.
//!
//! One route tree, no version prefix.
//! Every endpoint accepts either the console's JWT or a personal access token (PAT) — see `Caller`.
use axum::extract::{FromRef, FromRequestParts};
use axum::http::request::Parts;
use loco_rs::prelude::*;
use sea_orm::prelude::DateTimeWithTimeZone;
use serde::{Deserialize, Serialize};

use crate::{
    models::{_entities::users, access_keys, buckets},
    views::{
        api::{BucketResponse, UsageResponse, WhoamiResponse},
        keys::{CreateKeyResponse, KeyResponse, TokenResponse},
    },
};

/// Whoever is calling, already resolved to a user.
///
/// A console session (JWT) and a service token (PAT) reach the same endpoints with the same powers: the console could already create, rotate and revoke keys over JWT, so refusing JWT on a separate management tree would have fenced off nothing.
pub struct Caller {
    pub user: users::Model,
}

impl<S> FromRequestParts<S> for Caller
where
    AppContext: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Error> {
        let ctx = AppContext::from_ref(state);

        // JWT first: it verifies from the signature alone, no DB round trip.
        if let Ok(jwt) = auth::JWT::from_request_parts(parts, state).await {
            let user = users::Model::find_by_pid(&ctx.db, &jwt.claims.pid)
                .await
                .map_err(|_| Error::Unauthorized("user not found".to_string()))?;
            return Ok(Self { user });
        }

        let token = auth::ApiToken::<users::Model>::from_request_parts(parts, state).await?;
        Ok(Self { user: token.user })
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateParams {
    pub label: Option<String>,
    pub status: Option<String>,
    /// Double option: absent leaves the expiry alone, explicit `null` clears it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<Option<DateTimeWithTimeZone>>,
    pub permissions: Option<Vec<String>>,
    pub prefixes: Option<Vec<String>>,
}

fn bad_request(e: &ModelError) -> Error {
    Error::BadRequest(e.to_string())
}

async fn load(
    db: &DatabaseConnection,
    user: &users::Model,
    pid: &str,
) -> Result<access_keys::Model> {
    access_keys::Model::find_by_pid_for_user(db, pid, user.id)
        .await
        .map_err(|_| Error::NotFound)
}

async fn render_key(db: &DatabaseConnection, key: &access_keys::Model) -> Result<Response> {
    let permissions = key.permissions(db).await?;
    let prefixes = key.prefixes(db).await?;
    format::json(KeyResponse::new(key, permissions, prefixes))
}

#[debug_handler]
async fn whoami(caller: Caller, State(_ctx): State<AppContext>) -> Result<Response> {
    format::json(WhoamiResponse::new(&caller.user))
}

#[debug_handler]
async fn list_keys(caller: Caller, State(ctx): State<AppContext>) -> Result<Response> {
    let rows = access_keys::Model::list_for_user(&ctx.db, caller.user.id).await?;
    format::json(
        rows.into_iter()
            .map(KeyResponse::from_policy)
            .collect::<Vec<_>>(),
    )
}

#[debug_handler]
async fn create_key(
    caller: Caller,
    State(ctx): State<AppContext>,
    Json(params): Json<access_keys::CreateKeyParams>,
) -> Result<Response> {
    let (key, secret) = access_keys::Model::create_key(&ctx.db, caller.user.id, &params)
        .await
        .map_err(|e| bad_request(&e))?;
    format::json(CreateKeyResponse::new(
        &key,
        params.permissions,
        params.prefixes,
        secret,
    ))
}

#[debug_handler]
async fn show_key(
    caller: Caller,
    Path(pid): Path<String>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    let key = load(&ctx.db, &caller.user, &pid).await?;
    render_key(&ctx.db, &key).await
}

#[debug_handler]
async fn update_key(
    caller: Caller,
    Path(pid): Path<String>,
    State(ctx): State<AppContext>,
    Json(params): Json<UpdateParams>,
) -> Result<Response> {
    let db = &ctx.db;
    let mut key = load(db, &caller.user, &pid).await?;

    if let Some(permissions) = &params.permissions {
        key.set_permissions(db, permissions)
            .await
            .map_err(|e| bad_request(&e))?;
    }
    if let Some(prefixes) = &params.prefixes {
        key.set_prefixes(db, prefixes)
            .await
            .map_err(|e| bad_request(&e))?;
    }
    if let Some(label) = &params.label {
        access_keys::validate_label(label).map_err(|e| bad_request(&e))?;
        let mut am: access_keys::ActiveModel = key.clone().into();
        am.label = ActiveValue::set(label.clone());
        key = am.update(db).await?;
    }
    if let Some(expires_at) = params.expires_at {
        let mut am: access_keys::ActiveModel = key.clone().into();
        am.expires_at = ActiveValue::set(expires_at);
        key = am.update(db).await?;
    }
    if let Some(status) = &params.status {
        key = key
            .set_status(db, status)
            .await
            .map_err(|e| bad_request(&e))?;
    }

    render_key(db, &key).await
}

#[debug_handler]
async fn rotate_key(
    caller: Caller,
    Path(pid): Path<String>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    let db = &ctx.db;
    let key = load(db, &caller.user, &pid).await?;
    let (new_key, secret) = key.rotate(db).await.map_err(|e| bad_request(&e))?;
    let permissions = new_key.permissions(db).await?;
    let prefixes = new_key.prefixes(db).await?;
    format::json(CreateKeyResponse::new(
        &new_key,
        permissions,
        prefixes,
        secret,
    ))
}

#[debug_handler]
async fn revoke_key(
    caller: Caller,
    Path(pid): Path<String>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    let db = &ctx.db;
    let key = load(db, &caller.user, &pid).await?;
    let revoked = key.revoke(db).await?;
    render_key(db, &revoked).await
}

#[debug_handler]
async fn list_buckets(caller: Caller, State(ctx): State<AppContext>) -> Result<Response> {
    let rows = buckets::Model::list_for_user(&ctx.db, caller.user.id).await?;
    format::json(rows.iter().map(BucketResponse::new).collect::<Vec<_>>())
}

#[debug_handler]
async fn usage(caller: Caller, State(ctx): State<AppContext>) -> Result<Response> {
    let rows = buckets::Model::list_for_user(&ctx.db, caller.user.id).await?;
    format::json(UsageResponse::new(&caller.user, &rows))
}

#[debug_handler]
async fn token(caller: Caller, State(_ctx): State<AppContext>) -> Result<Response> {
    format::json(TokenResponse {
        token: caller.user.api_key,
    })
}

#[debug_handler]
async fn token_rotate(caller: Caller, State(ctx): State<AppContext>) -> Result<Response> {
    let token = format!("osg_pat_{}", uuid::Uuid::new_v4().simple());
    let mut am: users::ActiveModel = caller.user.into();
    am.api_key = ActiveValue::set(token.clone());
    am.update(&ctx.db).await?;
    format::json(TokenResponse { token })
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api")
        .add("/whoami", get(whoami))
        .add("/keys", get(list_keys).post(create_key))
        .add(
            "/keys/{pid}",
            get(show_key).patch(update_key).delete(revoke_key),
        )
        .add("/keys/{pid}/rotate", post(rotate_key))
        .add("/buckets", get(list_buckets))
        .add("/usage", get(usage))
        .add("/token", get(token))
        .add("/token/rotate", post(token_rotate))
}
