use crate::{
    models::{
        _entities::users,
        users::{LoginParams, RegisterParams},
    },
    views::auth::{CurrentResponse, LoginResponse},
};
use axum::http::StatusCode;
use loco_rs::{controller::ErrorDetail, prelude::*};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct SetupStatusResponse {
    pub needs_setup: bool,
}

/// Reports whether this instance still has no user, i.e. the console should send the visitor to the first-run admin setup page.
/// Public on purpose: the setup page is reachable before any credential exists.
#[debug_handler]
async fn setup_status(State(ctx): State<AppContext>) -> Result<Response> {
    format::json(SetupStatusResponse {
        needs_setup: !users::Model::any_exists(&ctx.db).await?,
    })
}

/// Creates the first admin of a fresh instance and logs it straight in.
/// Refused with 403 once any user exists.
/// This is the only self-service account creation left; every later account is created by an admin.
#[debug_handler]
async fn setup_admin(
    State(ctx): State<AppContext>,
    Json(params): Json<RegisterParams>,
) -> Result<Response> {
    if users::Model::any_exists(&ctx.db).await? {
        return Err(Error::CustomError(
            StatusCode::FORBIDDEN,
            ErrorDetail::new("setup_done", "setup has already been completed"),
        ));
    }

    let user = users::Model::create_first_admin(&ctx.db, &params).await?;

    let jwt_secret = ctx.config.get_jwt_config()?;
    let token = user
        .generate_jwt(&jwt_secret.secret, jwt_secret.expiration)
        .or_else(|_| unauthorized("unauthorized!"))?;

    format::json(LoginResponse::new(&user, &token))
}

/// Creates a user login and returns a token.
#[debug_handler]
async fn login(State(ctx): State<AppContext>, Json(params): Json<LoginParams>) -> Result<Response> {
    let Ok(user) = users::Model::find_by_email(&ctx.db, &params.email).await else {
        tracing::debug!(
            email = params.email,
            "login attempt with non-existent email"
        );
        return unauthorized("Invalid credentials!");
    };

    if !user.verify_password(&params.password) {
        return unauthorized("Invalid credentials!");
    }

    let jwt_secret = ctx.config.get_jwt_config()?;

    let token = user
        .generate_jwt(&jwt_secret.secret, jwt_secret.expiration)
        .or_else(|_| unauthorized("unauthorized!"))?;

    format::json(LoginResponse::new(&user, &token))
}

#[debug_handler]
async fn current(auth: auth::JWT, State(ctx): State<AppContext>) -> Result<Response> {
    let user = users::Model::find_by_pid(&ctx.db, &auth.claims.pid).await?;
    format::json(CurrentResponse::new(&user))
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/auth")
        .add("/setup", get(setup_status))
        .add("/setup", post(setup_admin))
        .add("/login", post(login))
        .add("/current", get(current))
}
