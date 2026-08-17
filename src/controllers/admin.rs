//! Admin-only user management.
//!
//! Self-registration was removed; this tree is the only way an account comes into existence after first-run setup.
//! Every handler takes `AdminCaller`, which is the server-side gate — the console's role check is a UX affordance only.
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    controllers::api::AdminCaller,
    models::{buckets, users},
    views::admin::AdminUserResponse,
};

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateUserParams {
    pub name: Option<String>,
    pub role: Option<String>,
    pub max_bytes: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SetPasswordParams {
    pub password: String,
}

fn bad_request(e: &ModelError) -> Error {
    Error::BadRequest(e.to_string())
}

async fn load(db: &DatabaseConnection, pid: &str) -> Result<users::Model> {
    users::Model::find_by_pid(db, pid)
        .await
        .map_err(|_| Error::NotFound)
}

#[debug_handler]
async fn list(_admin: AdminCaller, State(ctx): State<AppContext>) -> Result<Response> {
    let rows = users::Model::list_all(&ctx.db).await?;
    format::json(rows.iter().map(AdminUserResponse::new).collect::<Vec<_>>())
}

#[debug_handler]
async fn create(
    _admin: AdminCaller,
    State(ctx): State<AppContext>,
    Json(params): Json<users::CreateUserParams>,
) -> Result<Response> {
    let user = users::Model::create_by_admin(&ctx.db, &params)
        .await
        .map_err(|e| bad_request(&e))?;
    format::json(AdminUserResponse::new(&user))
}

#[debug_handler]
async fn show(
    _admin: AdminCaller,
    Path(pid): Path<String>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    let user = load(&ctx.db, &pid).await?;
    format::json(AdminUserResponse::new(&user))
}

#[debug_handler]
async fn update(
    _admin: AdminCaller,
    Path(pid): Path<String>,
    State(ctx): State<AppContext>,
    Json(params): Json<UpdateUserParams>,
) -> Result<Response> {
    let db = &ctx.db;
    let user = load(db, &pid).await?;

    // Demoting the last admin would lock everyone out of this tree.
    if let Some(role) = &params.role {
        users::validate_role(role).map_err(|e| bad_request(&e))?;
        if user.is_admin() && role != users::ROLE_ADMIN && users::Model::admin_count(db).await? <= 1
        {
            return Err(Error::BadRequest(
                "cannot demote the last admin".to_string(),
            ));
        }
    }
    if let Some(max_bytes) = params.max_bytes {
        if max_bytes < 0 {
            return Err(Error::BadRequest(
                "max_bytes must not be negative".to_string(),
            ));
        }
    }

    let mut am: users::ActiveModel = user.into();
    if let Some(name) = &params.name {
        am.name = ActiveValue::set(name.clone());
    }
    if let Some(role) = &params.role {
        am.role = ActiveValue::set(role.clone());
    }
    if let Some(max_bytes) = params.max_bytes {
        am.max_bytes = ActiveValue::set(max_bytes);
    }
    let updated = am.update(db).await?;

    format::json(AdminUserResponse::new(&updated))
}

/// Issues a new temporary password and forces the user to replace it at next login.
/// This replaces the removed self-service password-reset flow.
#[debug_handler]
async fn set_password(
    _admin: AdminCaller,
    Path(pid): Path<String>,
    State(ctx): State<AppContext>,
    Json(params): Json<SetPasswordParams>,
) -> Result<Response> {
    let db = &ctx.db;
    let user = load(db, &pid).await?;
    users::validate_password(&params.password).map_err(|e| bad_request(&e))?;

    let am: users::ActiveModel = user.into();
    am.set_password(db, &params.password, true).await?;

    format::json(())
}

#[debug_handler]
async fn destroy(
    admin: AdminCaller,
    Path(pid): Path<String>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    let db = &ctx.db;
    let user = load(db, &pid).await?;

    if user.id == admin.user.id {
        return Err(Error::BadRequest(
            "cannot delete your own account".to_string(),
        ));
    }
    if user.is_admin() && users::Model::admin_count(db).await? <= 1 {
        return Err(Error::BadRequest(
            "cannot delete the last admin".to_string(),
        ));
    }

    // ponytail: buckets are ON DELETE SET NULL, so deleting an owner would turn their private bucket into a system pool along with its encrypted upstream credentials.
    // Refuse instead of leaking; P3 fixes the cascade and this guard then becomes a cascading delete.
    let owned = buckets::Model::list_for_user(db, user.id).await?;
    if !owned.is_empty() {
        return Err(Error::BadRequest(
            "delete or reassign this user's buckets first".to_string(),
        ));
    }

    let am: users::ActiveModel = user.into();
    am.delete(db).await?;

    format::json(())
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/admin")
        .add("/users", get(list).post(create))
        .add("/users/{pid}", get(show).patch(update).delete(destroy))
        .add("/users/{pid}/password", post(set_password))
}
