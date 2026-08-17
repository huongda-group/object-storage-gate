use loco_rs::{testing::prelude::*, TestServer};
use object_storage_gate::{app::App, models::_entities::users, views::auth::LoginResponse};
use sea_orm::{ActiveModelTrait, ActiveValue};
use serial_test::serial;

use super::prepare_data;

/// Grab the account PAT through the console API, the way the /api page does.
async fn pat(request: &TestServer, token: &str) -> String {
    let (ak, av) = prepare_data::auth_header(token);
    let res = request.get("/api/token").add_header(ak, av).await;
    res.json::<serde_json::Value>()["token"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
#[serial]
async fn whoami_accepts_pat_and_jwt_but_not_junk() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;

        assert_eq!(request.get("/api/whoami").await.status_code(), 401);

        let (bad_k, bad_v) = prepare_data::auth_header("osg_pat_nope");
        assert_eq!(
            request
                .get("/api/whoami")
                .add_header(bad_k, bad_v)
                .await
                .status_code(),
            401
        );

        let token = pat(&request, &user.token).await;
        let (ak, av) = prepare_data::auth_header(&token);
        let res = request.get("/api/whoami").add_header(ak, av).await;
        assert_eq!(res.status_code(), 200);
        let body: serde_json::Value = res.json();
        assert_eq!(body["email"], "test@loco.com");
        assert_eq!(body["role"], "user");

        // Same endpoint, console session instead of a service token.
        let (jk, jv) = prepare_data::auth_header(&user.token);
        let via_jwt = request.get("/api/whoami").add_header(jk, jv).await;
        assert_eq!(via_jwt.status_code(), 200);
        assert_eq!(
            via_jwt.json::<serde_json::Value>()["email"],
            "test@loco.com"
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn pat_can_manage_keys() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;
        let token = pat(&request, &user.token).await;
        let (ak, av) = prepare_data::auth_header(&token);

        let created = request
            .post("/api/keys")
            .add_header(ak.clone(), av.clone())
            .json(&serde_json::json!({"label": "ci", "permissions": ["read"]}))
            .await;
        assert_eq!(created.status_code(), 200);
        let pid = created.json::<serde_json::Value>()["pid"]
            .as_str()
            .unwrap()
            .to_string();

        let listed = request
            .get("/api/keys")
            .add_header(ak.clone(), av.clone())
            .await;
        assert_eq!(listed.status_code(), 200);
        assert!(listed.text().contains(&pid));

        let rotated = request
            .post(&format!("/api/keys/{pid}/rotate"))
            .add_header(ak.clone(), av.clone())
            .await;
        assert_eq!(rotated.status_code(), 200);

        let revoked = request
            .delete(&format!("/api/keys/{pid}"))
            .add_header(ak, av)
            .await;
        assert_eq!(revoked.status_code(), 200);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn usage_and_buckets_report_the_account() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;
        object_storage_gate::models::buckets::Model::create(&ctx.db, user.user.id, "my-bucket", 0)
            .await
            .unwrap();

        let token = pat(&request, &user.token).await;
        let (ak, av) = prepare_data::auth_header(&token);

        let buckets = request
            .get("/api/buckets")
            .add_header(ak.clone(), av.clone())
            .await;
        assert_eq!(buckets.status_code(), 200);
        let list: serde_json::Value = buckets.json();
        assert_eq!(list[0]["name"], "my-bucket");

        let usage = request.get("/api/usage").add_header(ak, av).await;
        assert_eq!(usage.status_code(), 200);
        let body: serde_json::Value = usage.json();
        assert_eq!(body["bucket_count"], 1);
        assert_eq!(body["used_bytes"], 0);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn caller_is_blocked_until_temp_password_is_changed() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;

        // Flip the flag the way an admin-created account would arrive.
        let mut am: users::ActiveModel = user.user.clone().into();
        am.must_change_password = ActiveValue::set(true);
        am.update(&ctx.db).await.unwrap();

        let (k, v) = prepare_data::auth_header(&user.token);
        let blocked = request.get("/api/keys").add_header(k, v).await;
        assert_eq!(blocked.status_code(), 403);
        assert!(blocked.text().contains("password_change_required"));

        // The change-password endpoint itself must stay reachable.
        let (k, v) = prepare_data::auth_header(&user.token);
        let allowed = request
            .post("/api/me/password")
            .add_header(k, v)
            .json(&serde_json::json!({
                "current_password": "12341234",
                "new_password": "a-much-better-secret"
            }))
            .await;
        assert_eq!(allowed.status_code(), 200);

        // And after changing it, everything else opens up again.
        let login = request
            .post("/api/auth/login")
            .json(&serde_json::json!({
                "email": "test@loco.com",
                "password": "a-much-better-secret"
            }))
            .await;
        let fresh: LoginResponse = serde_json::from_str(&login.text()).unwrap();
        assert!(!fresh.must_change_password);

        let (k, v) = prepare_data::auth_header(&fresh.token);
        let ok = request.get("/api/keys").add_header(k, v).await;
        assert_eq!(ok.status_code(), 200);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn change_password_rejects_wrong_current_password() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;
        let (k, v) = prepare_data::auth_header(&user.token);

        let res = request
            .post("/api/me/password")
            .add_header(k, v)
            .json(&serde_json::json!({
                "current_password": "not-the-password",
                "new_password": "a-much-better-secret"
            }))
            .await;

        assert_eq!(res.status_code(), 401);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn change_password_rejects_short_password() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;
        let (k, v) = prepare_data::auth_header(&user.token);

        let res = request
            .post("/api/me/password")
            .add_header(k, v)
            .json(&serde_json::json!({
                "current_password": "12341234",
                "new_password": "short"
            }))
            .await;

        assert_eq!(res.status_code(), 400);
    })
    .await;
}

/// loco's default body limit is 2 MB, which would reject every real S3 upload once the
/// gateway lands, including every multipart part (5 MB minimum per the S3 spec).
/// A 413 here means the middleware ate the body before any handler saw it.
#[tokio::test]
#[serial]
async fn body_limit_is_disabled() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;
        let (k, v) = prepare_data::auth_header(&user.token);

        let big_label = "x".repeat(3_000_000);
        let res = request
            .post("/api/keys")
            .add_header(k, v)
            .json(&serde_json::json!({
                "label": big_label,
                "permissions": ["read"],
                "prefixes": []
            }))
            .await;

        assert_ne!(
            res.status_code(),
            413,
            "payload limit middleware is still on"
        );
    })
    .await;
}
