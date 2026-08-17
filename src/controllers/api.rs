//! The account API: access keys, the management token, and read-only account state.
//!
//! One route tree, no version prefix.
//! Every endpoint accepts either the console's JWT or a personal access token (PAT) — see `Caller`.
use axum::extract::{FromRef, FromRequestParts};
use axum::http::request::Parts;
use axum::http::StatusCode;
use loco_rs::controller::ErrorDetail;
use loco_rs::prelude::*;
use sea_orm::prelude::DateTimeWithTimeZone;
use serde::{Deserialize, Serialize};

use crate::{
    models::{_entities::users, access_keys, buckets},
    views::{
        api::{SummaryResponse, UsageResponse, WhoamiResponse},
        auth::CurrentResponse,
        keys::{CreateKeyResponse, KeyResponse, TokenResponse},
    },
};

/// Whoever is calling, already resolved to a user, with no policy applied.
///
/// Only the change-password endpoint uses this directly: a user holding a temporary password must be able to replace it while every other endpoint is closed to them.
pub struct RawCaller {
    pub user: users::Model,
}

impl<S> FromRequestParts<S> for RawCaller
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

/// A caller who is allowed to use the account API.
///
/// A console session (JWT) and a service token (PAT) reach the same endpoints with the same powers: the console could already create, rotate and revoke keys over JWT, so refusing JWT on a separate management tree would have fenced off nothing.
/// A user still holding an admin-issued temporary password is refused here until they change it.
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
        let RawCaller { user } = RawCaller::from_request_parts(parts, state).await?;
        if user.must_change_password {
            return Err(Error::CustomError(
                StatusCode::FORBIDDEN,
                ErrorDetail::new(
                    "password_change_required",
                    "change the temporary password before using the API",
                ),
            ));
        }
        Ok(Self { user })
    }
}

/// A caller who is additionally an admin.
///
/// This is the only server-side admin gate; the console's role check is a UX affordance and must never be the model.
pub struct AdminCaller {
    pub user: users::Model,
}

impl<S> FromRequestParts<S> for AdminCaller
where
    AppContext: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Error> {
        let Caller { user } = Caller::from_request_parts(parts, state).await?;
        if !user.is_admin() {
            return Err(Error::CustomError(
                StatusCode::FORBIDDEN,
                ErrorDetail::new("admin_required", "this endpoint requires an admin account"),
            ));
        }
        Ok(Self { user })
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
async fn usage(caller: Caller, State(ctx): State<AppContext>) -> Result<Response> {
    let rows = buckets::Model::list_for_user(&ctx.db, caller.user.id).await?;
    format::json(UsageResponse::new(&caller.user, &rows))
}

/// Issues a fresh personal access token and returns it once.
/// There is no read endpoint: a token that can be re-read turns any stolen JWT into a permanent credential that a password change does not evict.
#[debug_handler]
async fn token_rotate(caller: Caller, State(ctx): State<AppContext>) -> Result<Response> {
    let (_user, token) = caller.user.rotate_api_token(&ctx.db).await?;
    format::json(TokenResponse { token })
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateMeParams {
    pub name: Option<String>,
}

/// Renames the calling user.
/// Deliberately narrow: role and quota are an admin's decision, and a struct with only `name` makes that structural rather than a check someone can forget.
#[debug_handler]
async fn update_me(
    caller: Caller,
    State(ctx): State<AppContext>,
    Json(params): Json<UpdateMeParams>,
) -> Result<Response> {
    let mut am: users::ActiveModel = caller.user.into();
    if let Some(name) = &params.name {
        am.name = ActiveValue::set(name.clone());
    }
    let updated = am.update(&ctx.db).await?;
    format::json(CurrentResponse::new(&updated))
}

#[debug_handler]
async fn summary(caller: Caller, State(ctx): State<AppContext>) -> Result<Response> {
    let db = &ctx.db;
    let bucket_rows = buckets::Model::list_for_user(db, caller.user.id).await?;
    let key_rows = access_keys::Model::list_for_user(db, caller.user.id).await?;

    format::json(SummaryResponse {
        used_bytes: caller.user.used_bytes,
        reserved_bytes: caller.user.reserved_bytes,
        max_bytes: caller.user.max_bytes,
        bucket_count: i64::try_from(bucket_rows.len()).unwrap_or(i64::MAX),
        object_count: bucket_rows.iter().map(|b| b.object_count).sum(),
        active_key_count: i64::try_from(
            key_rows
                .iter()
                .filter(|k| k.key.status == access_keys::KEY_ACTIVE)
                .count(),
        )
        .unwrap_or(i64::MAX),
    })
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ChangePasswordParams {
    pub current_password: String,
    pub new_password: String,
}

/// Lets a user replace their own password, including the temporary one an admin issued.
/// Uses `RawCaller` on purpose: this is the one endpoint that stays open while `must_change_password` is set.
#[debug_handler]
async fn change_password(
    caller: RawCaller,
    State(ctx): State<AppContext>,
    Json(params): Json<ChangePasswordParams>,
) -> Result<Response> {
    if !caller.user.verify_password(&params.current_password) {
        return Err(Error::Unauthorized("current password is wrong".to_string()));
    }
    crate::models::users::validate_password(&params.new_password)
        .map_err(|e| Error::BadRequest(e.to_string()))?;

    let am: crate::models::users::ActiveModel = caller.user.into();
    am.set_password(&ctx.db, &params.new_password, false)
        .await?;

    format::json(())
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api")
        .add("/whoami", get(whoami))
        .add("/me", patch(update_me))
        .add("/me/summary", get(summary))
        .add("/me/password", post(change_password))
        .add("/keys", get(list_keys).post(create_key))
        .add(
            "/keys/{pid}",
            get(show_key).patch(update_key).delete(revoke_key),
        )
        .add("/keys/{pid}/rotate", post(rotate_key))
        .add("/usage", get(usage))
        .add("/token/rotate", post(token_rotate))
}
