use loco_rs::{testing::prelude::*, TestServer};
use object_storage_gate::app::App;
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
