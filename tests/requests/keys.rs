use loco_rs::testing::prelude::*;
use object_storage_gate::{app::App, models::users};
use serial_test::serial;

use super::prepare_data;

#[tokio::test]
#[serial]
async fn create_returns_secret_once_then_never_again() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;
        let (auth_key, auth_value) = prepare_data::auth_header(&user.token);

        let created = request
            .post("/api/keys")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "label": "ci",
                "permissions": ["read", "list"],
                "prefixes": ["ci/"]
            }))
            .await;
        assert_eq!(created.status_code(), 200);
        let body: serde_json::Value = created.json();
        assert!(body["secret"].as_str().unwrap().len() > 20);
        assert!(body["access_key_id"].as_str().unwrap().starts_with("OSG"));
        let pid = body["pid"].as_str().unwrap().to_string();

        let listed = request
            .get("/api/keys")
            .add_header(auth_key, auth_value)
            .await;
        assert_eq!(listed.status_code(), 200);
        let text = listed.text();
        assert!(
            !text.contains("secret"),
            "list must not leak secrets: {text}"
        );
        assert!(!text.contains("secret_encrypted"));
        assert!(text.contains(&pid));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn requires_authentication() {
    request::<App, _, _>(|request, _ctx| async move {
        assert_eq!(request.get("/api/keys").await.status_code(), 401);
        assert_eq!(
            request
                .post("/api/keys")
                .json(&serde_json::json!({"label": "primary"}))
                .await
                .status_code(),
            401
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn other_users_key_is_404_not_403() {
    request::<App, _, _>(|request, ctx| async move {
        let owner = prepare_data::init_user_login(&request, &ctx).await;
        let (ok_key, ok_value) = prepare_data::auth_header(&owner.token);

        let created = request
            .post("/api/keys")
            .add_header(ok_key, ok_value)
            .json(&serde_json::json!({"label": "primary"}))
            .await;
        let pid = created.json::<serde_json::Value>()["pid"]
            .as_str()
            .unwrap()
            .to_string();

        // A second account, created through the model and logged in independently.
        prepare_data::create_user(
            &ctx,
            "other@loco.com",
            "12341234",
            "other",
            users::ROLE_USER,
        )
        .await;
        let other_token = prepare_data::login(&request, "other@loco.com", "12341234").await;
        let (other_key, other_value) = prepare_data::auth_header(&other_token);

        let res = request
            .get(&format!("/api/keys/{pid}"))
            .add_header(other_key, other_value)
            .await;
        assert_eq!(res.status_code(), 404);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn update_rotate_revoke_flow() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;
        let (ak, av) = prepare_data::auth_header(&user.token);

        let created = request
            .post("/api/keys")
            .add_header(ak.clone(), av.clone())
            .json(&serde_json::json!({"label": "primary", "permissions": ["read"]}))
            .await;
        let pid = created.json::<serde_json::Value>()["pid"]
            .as_str()
            .unwrap()
            .to_string();

        let patched = request
            .patch(&format!("/api/keys/{pid}"))
            .add_header(ak.clone(), av.clone())
            .json(&serde_json::json!({
                "permissions": ["read", "write"],
                "prefixes": ["img/"],
                "status": "disabled"
            }))
            .await;
        assert_eq!(patched.status_code(), 200);
        let body: serde_json::Value = patched.json();
        assert_eq!(body["status"], "disabled");
        assert_eq!(body["prefixes"][0], "img/");

        let bad = request
            .patch(&format!("/api/keys/{pid}"))
            .add_header(ak.clone(), av.clone())
            .json(&serde_json::json!({"permissions": ["sudo"]}))
            .await;
        assert_eq!(bad.status_code(), 400);

        let rotated = request
            .post(&format!("/api/keys/{pid}/rotate"))
            .add_header(ak.clone(), av.clone())
            .await;
        assert_eq!(rotated.status_code(), 200);
        let new_pid = rotated.json::<serde_json::Value>()["pid"]
            .as_str()
            .unwrap()
            .to_string();
        assert_ne!(new_pid, pid);

        let deleted = request
            .delete(&format!("/api/keys/{new_pid}"))
            .add_header(ak.clone(), av.clone())
            .await;
        assert_eq!(deleted.status_code(), 200);

        let after = request
            .get(&format!("/api/keys/{new_pid}"))
            .add_header(ak, av)
            .await;
        assert_eq!(after.json::<serde_json::Value>()["status"], "revoked");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn token_is_revealed_once_at_rotation_and_never_readable() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;
        let (ak, av) = prepare_data::auth_header(&user.token);

        // There is no read endpoint: one that hands the token back turns any stolen JWT into
        // a permanent credential that a password change does not evict.
        // An unrouted GET falls through to the SPA index, so assert on the body, not the status.
        let gone = request
            .get("/api/token")
            .add_header(ak.clone(), av.clone())
            .await;
        assert!(
            !gone.text().contains("\"token\""),
            "GET /api/token still returns a token"
        );

        assert_eq!(
            request.post("/api/token/rotate").await.status_code(),
            401,
            "rotation must require a caller"
        );

        let rotated = request
            .post("/api/token/rotate")
            .add_header(ak.clone(), av.clone())
            .await;
        assert_eq!(rotated.status_code(), 200);
        let first = rotated.json::<serde_json::Value>()["token"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(first.starts_with("osg_pat_"));

        // What is stored is a hash, not the token.
        let stored = users::Model::find_by_pid(&ctx.db, &user.user.pid.to_string())
            .await
            .unwrap();
        assert_ne!(stored.api_key, first);
        assert!(stored.api_key.contains("$argon2"));

        // The token authenticates.
        let (tk, tv) = prepare_data::auth_header(&first);
        assert_eq!(
            request
                .get("/api/whoami")
                .add_header(tk, tv)
                .await
                .status_code(),
            200
        );

        // Rotating again invalidates the previous one.
        let second_res = request.post("/api/token/rotate").add_header(ak, av).await;
        let second = second_res.json::<serde_json::Value>()["token"]
            .as_str()
            .unwrap()
            .to_string();
        assert_ne!(second, first);

        let (ok, ov) = prepare_data::auth_header(&first);
        assert_eq!(
            request
                .get("/api/whoami")
                .add_header(ok, ov)
                .await
                .status_code(),
            401
        );
    })
    .await;
}
