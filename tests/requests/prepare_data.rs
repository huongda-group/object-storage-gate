use axum::http::{HeaderName, HeaderValue};
use loco_rs::{app::AppContext, hash, TestServer};
use object_storage_gate::{
    models::{pools, users},
    views::auth::LoginResponse,
};
use sea_orm::{ActiveModelTrait, ActiveValue};

const USER_EMAIL: &str = "test@loco.com";
const USER_PASSWORD: &str = "12341234";

pub struct LoggedInUser {
    pub user: users::Model,
    pub token: String,
}

/// Inserts a user straight through the model.
/// There is no registration endpoint any more, so this is the only way a test gets an account.
pub async fn create_user(
    ctx: &AppContext,
    email: &str,
    password: &str,
    name: &str,
    role: &str,
) -> users::Model {
    users::ActiveModel {
        email: ActiveValue::set(email.to_string()),
        password: ActiveValue::set(hash::hash_password(password).expect("hash password")),
        name: ActiveValue::set(name.to_string()),
        role: ActiveValue::set(role.to_string()),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await
    .expect("insert user")
}

/// Logs in over the API and returns the bearer token.
pub async fn login(request: &TestServer, email: &str, password: &str) -> String {
    let response = request
        .post("/api/auth/login")
        .json(&serde_json::json!({ "email": email, "password": password }))
        .await;
    let body: LoginResponse = serde_json::from_str(&response.text()).unwrap();
    body.token
}

/// A plain, non-admin account, logged in.
pub async fn init_user_login(request: &TestServer, ctx: &AppContext) -> LoggedInUser {
    let user = create_user(ctx, USER_EMAIL, USER_PASSWORD, "loco", users::ROLE_USER).await;
    let token = login(request, USER_EMAIL, USER_PASSWORD).await;

    LoggedInUser { user, token }
}

/// An admin account, logged in.
/// Used by the tests that exercise the admin tree.
pub async fn init_admin_login(request: &TestServer, ctx: &AppContext) -> LoggedInUser {
    let user = create_user(
        ctx,
        "admin@loco.com",
        USER_PASSWORD,
        "admin",
        users::ROLE_ADMIN,
    )
    .await;
    let token = login(request, "admin@loco.com", USER_PASSWORD).await;

    LoggedInUser { user, token }
}

/// A pool for tests that need to create a bucket.
/// Returns the model, whose `pid` goes in the request body.
pub async fn a_pool(ctx: &AppContext) -> pools::Model {
    pools::Model::create(
        &ctx.db,
        &pools::CreateParams {
            name: "main".to_string(),
            provider: pools::PROVIDER_MINIO.to_string(),
            physical_bucket: "osg-main".to_string(),
            ..Default::default()
        },
    )
    .await
    .expect("create test pool")
}

pub fn auth_header(token: &str) -> (HeaderName, HeaderValue) {
    let auth_header_value = HeaderValue::from_str(&format!("Bearer {token}")).unwrap();

    (HeaderName::from_static("authorization"), auth_header_value)
}
