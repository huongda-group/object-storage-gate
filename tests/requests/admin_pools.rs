use loco_rs::testing::prelude::*;
use object_storage_gate::{app::App, models::pools};
use serial_test::serial;

use super::prepare_data;

fn body(name: &str) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "provider": "minio",
        "region": "ap-southeast-1",
        "api_endpoint": "https://minio.internal:9000",
        "physical_bucket": "osg-main",
        "access_id": "UPSTREAMKEYID",
        "access_secret": "upstream-secret-value"
    })
}

#[tokio::test]
#[serial]
async fn admin_can_create_and_list_a_pool() {
    request::<App, _, _>(|request, ctx| async move {
        let admin = prepare_data::init_admin_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&admin.token);
        let created = request
            .post("/api/admin/pools")
            .add_header(k, v)
            .json(&body("main"))
            .await;
        assert_eq!(created.status_code(), 200);

        // The secret must never come back out.
        let text = created.text();
        assert!(!text.contains("upstream-secret-value"));
        assert!(!text.contains("access_secret"));
        assert!(text.contains("UPSTREAMKEYID"));
        assert!(text.contains("osg-main"));
        assert_eq!(
            created.json::<serde_json::Value>()["is_configured"].as_bool(),
            Some(true)
        );

        let (k, v) = prepare_data::auth_header(&admin.token);
        let listed = request.get("/api/admin/pools").add_header(k, v).await;
        assert_eq!(listed.json::<Vec<serde_json::Value>>().len(), 1);

        // And the row really carries the encrypted secret.
        let pool = pools::Model::find_by_name(&ctx.db, "main").await.unwrap();
        assert_eq!(pool.decrypt_secret().unwrap(), "upstream-secret-value");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_plain_user_is_refused() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&user.token);
        let listed = request.get("/api/admin/pools").add_header(k, v).await;
        assert_eq!(listed.status_code(), 403);
        assert!(listed.text().contains("admin_required"));

        let (k, v) = prepare_data::auth_header(&user.token);
        let created = request
            .post("/api/admin/pools")
            .add_header(k, v)
            .json(&body("sneaky"))
            .await;
        assert_eq!(created.status_code(), 403);
    })
    .await;
}

/// A non-admin still has to pick a pool when creating a bucket, so there is a reduced listing for them.
/// It carries no credentials and not even the physical bucket name — a tenant learning the real layout is the thing the gateway exists to prevent.
#[tokio::test]
#[serial]
async fn a_plain_user_sees_names_and_providers_only() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;
        prepare_data::a_pool(&ctx).await;

        let (k, v) = prepare_data::auth_header(&user.token);
        let listed = request.get("/api/pools").add_header(k, v).await;
        assert_eq!(listed.status_code(), 200);

        let text = listed.text();
        assert!(text.contains("main"));
        assert!(text.contains("minio"));
        assert!(!text.contains("osg-main"), "physical bucket leaked: {text}");
        assert!(
            !text.contains("access_id"),
            "credential field leaked: {text}"
        );
        assert!(!text.contains("api_endpoint"), "endpoint leaked: {text}");

        let rows = listed.json::<Vec<serde_json::Value>>();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["name"].as_str(), Some("main"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn unknown_provider_and_empty_physical_bucket_are_rejected() {
    request::<App, _, _>(|request, ctx| async move {
        let admin = prepare_data::init_admin_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&admin.token);
        let mut bad = body("weird");
        bad["provider"] = serde_json::json!("dropbox");
        assert_eq!(
            request
                .post("/api/admin/pools")
                .add_header(k, v)
                .json(&bad)
                .await
                .status_code(),
            400
        );

        let (k, v) = prepare_data::auth_header(&admin.token);
        let mut bad = body("empty");
        bad["physical_bucket"] = serde_json::json!("");
        assert_eq!(
            request
                .post("/api/admin/pools")
                .add_header(k, v)
                .json(&bad)
                .await
                .status_code(),
            400
        );
    })
    .await;
}

/// An untouched secret field means unchanged, never erase.
#[tokio::test]
#[serial]
async fn patch_without_a_secret_keeps_the_stored_one() {
    request::<App, _, _>(|request, ctx| async move {
        let admin = prepare_data::init_admin_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&admin.token);
        let created = request
            .post("/api/admin/pools")
            .add_header(k, v)
            .json(&body("main"))
            .await;
        let pid = created.json::<serde_json::Value>()["pid"]
            .as_str()
            .unwrap()
            .to_string();

        let (k, v) = prepare_data::auth_header(&admin.token);
        let patched = request
            .patch(&format!("/api/admin/pools/{pid}"))
            .add_header(k, v)
            .json(&serde_json::json!({ "region": "us-east-1" }))
            .await;
        assert_eq!(patched.status_code(), 200);

        let pool = pools::Model::find_by_pid(&ctx.db, &pid).await.unwrap();
        assert_eq!(pool.region.as_deref(), Some("us-east-1"));
        assert_eq!(pool.decrypt_secret().unwrap(), "upstream-secret-value");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_pool_with_buckets_cannot_be_deleted() {
    request::<App, _, _>(|request, ctx| async move {
        let admin = prepare_data::init_admin_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&admin.token);
        let created = request
            .post("/api/admin/pools")
            .add_header(k, v)
            .json(&body("main"))
            .await;
        let pid = created.json::<serde_json::Value>()["pid"]
            .as_str()
            .unwrap()
            .to_string();
        let pool = pools::Model::find_by_pid(&ctx.db, &pid).await.unwrap();

        object_storage_gate::models::buckets::Model::create(
            &ctx.db,
            admin.user.id,
            pool.id,
            "media-cdn",
            0,
        )
        .await
        .unwrap();

        let (k, v) = prepare_data::auth_header(&admin.token);
        let deleted = request
            .delete(&format!("/api/admin/pools/{pid}"))
            .add_header(k, v)
            .await;
        assert_eq!(deleted.status_code(), 400);
        assert!(deleted.text().contains("still use this pool"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn an_empty_pool_can_be_deleted() {
    request::<App, _, _>(|request, ctx| async move {
        let admin = prepare_data::init_admin_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&admin.token);
        let created = request
            .post("/api/admin/pools")
            .add_header(k, v)
            .json(&body("spare"))
            .await;
        let pid = created.json::<serde_json::Value>()["pid"]
            .as_str()
            .unwrap()
            .to_string();

        let (k, v) = prepare_data::auth_header(&admin.token);
        assert_eq!(
            request
                .delete(&format!("/api/admin/pools/{pid}"))
                .add_header(k, v)
                .await
                .status_code(),
            200
        );
        assert!(pools::Model::find_by_pid(&ctx.db, &pid).await.is_err());
    })
    .await;
}

/// A pool with no credentials is what the backfill migration leaves behind, and the console has to be able to see that state.
#[tokio::test]
#[serial]
async fn a_pool_without_credentials_reports_itself_unconfigured() {
    request::<App, _, _>(|request, ctx| async move {
        let admin = prepare_data::init_admin_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&admin.token);
        let mut bare = body("bare");
        bare["access_id"] = serde_json::Value::Null;
        bare["access_secret"] = serde_json::Value::Null;
        let created = request
            .post("/api/admin/pools")
            .add_header(k, v)
            .json(&bare)
            .await;
        assert_eq!(created.status_code(), 200);
        assert_eq!(
            created.json::<serde_json::Value>()["is_configured"].as_bool(),
            Some(false)
        );

        let pid = created.json::<serde_json::Value>()["pid"]
            .as_str()
            .unwrap()
            .to_string();

        // And filling it in later flips the flag.
        let (k, v) = prepare_data::auth_header(&admin.token);
        let patched = request
            .patch(&format!("/api/admin/pools/{pid}"))
            .add_header(k, v)
            .json(&serde_json::json!({
                "access_id": "LATEKEYID", "access_secret": "late-secret"
            }))
            .await;
        assert_eq!(
            patched.json::<serde_json::Value>()["is_configured"].as_bool(),
            Some(true)
        );

        let pool = pools::Model::find_by_pid(&ctx.db, &pid).await.unwrap();
        assert_eq!(pool.decrypt_secret().unwrap(), "late-secret");
    })
    .await;
}
