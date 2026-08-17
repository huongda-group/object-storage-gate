use loco_rs::testing::prelude::*;
use object_storage_gate::{
    app::App,
    models::{buckets, objects, users},
};
use serial_test::serial;

use super::prepare_data;

#[tokio::test]
#[serial]
async fn owner_can_create_list_retune_and_delete_a_bucket() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&user.token);
        let created = request
            .post("/api/buckets")
            .add_header(k, v)
            .json(&serde_json::json!({ "name": "media-cdn", "max_bytes": 1_073_741_824i64 }))
            .await;
        assert_eq!(created.status_code(), 200);

        let pid = created.json::<serde_json::Value>()["pid"]
            .as_str()
            .unwrap()
            .to_string();

        let (k, v) = prepare_data::auth_header(&user.token);
        let listed = request.get("/api/buckets").add_header(k, v).await;
        assert_eq!(listed.json::<Vec<serde_json::Value>>().len(), 1);

        let (k, v) = prepare_data::auth_header(&user.token);
        let patched = request
            .patch(&format!("/api/buckets/{pid}"))
            .add_header(k, v)
            .json(&serde_json::json!({ "max_bytes": 2_147_483_648i64, "public_enabled": true }))
            .await;
        assert_eq!(patched.status_code(), 200);
        let body = patched.json::<serde_json::Value>();
        assert_eq!(body["max_bytes"].as_i64(), Some(2_147_483_648));
        assert_eq!(body["public_enabled"].as_bool(), Some(true));

        let (k, v) = prepare_data::auth_header(&user.token);
        let deleted = request
            .delete(&format!("/api/buckets/{pid}"))
            .add_header(k, v)
            .await;
        assert_eq!(deleted.status_code(), 200);

        let (k, v) = prepare_data::auth_header(&user.token);
        let empty = request.get("/api/buckets").add_header(k, v).await;
        assert_eq!(empty.json::<Vec<serde_json::Value>>().len(), 0);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn bucket_names_are_validated_and_unique_per_owner() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;

        for bad in ["", "ab", "has spaces", "UPPER", "-leading", "trailing-"] {
            let (k, v) = prepare_data::auth_header(&user.token);
            let res = request
                .post("/api/buckets")
                .add_header(k, v)
                .json(&serde_json::json!({ "name": bad, "max_bytes": 0 }))
                .await;
            assert_eq!(res.status_code(), 400, "name {bad:?} should be rejected");
        }

        let (k, v) = prepare_data::auth_header(&user.token);
        let first = request
            .post("/api/buckets")
            .add_header(k, v)
            .json(&serde_json::json!({ "name": "taken", "max_bytes": 0 }))
            .await;
        assert_eq!(first.status_code(), 200);

        let (k, v) = prepare_data::auth_header(&user.token);
        let dup = request
            .post("/api/buckets")
            .add_header(k, v)
            .json(&serde_json::json!({ "name": "taken", "max_bytes": 0 }))
            .await;
        assert_ne!(dup.status_code(), 200, "duplicate name was accepted");
    })
    .await;
}

/// A quota below what is already stored would make every future write fail with no way back.
#[tokio::test]
#[serial]
async fn quota_cannot_be_set_below_what_is_stored() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&user.token);
        let created = request
            .post("/api/buckets")
            .add_header(k, v)
            .json(&serde_json::json!({ "name": "shrink", "max_bytes": 0 }))
            .await;
        let pid = created.json::<serde_json::Value>()["pid"]
            .as_str()
            .unwrap()
            .to_string();

        let bucket = buckets::Model::find_by_pid_for_user(&ctx.db, &pid, user.user.id)
            .await
            .unwrap();
        objects::Model::put_object(&ctx.db, bucket.id, "big.bin", 500, "e", "text/plain")
            .await
            .unwrap();

        let (k, v) = prepare_data::auth_header(&user.token);
        let res = request
            .patch(&format!("/api/buckets/{pid}"))
            .add_header(k, v)
            .json(&serde_json::json!({ "max_bytes": 100 }))
            .await;
        assert_eq!(res.status_code(), 400);
    })
    .await;
}

/// Deleting a non-empty bucket would orphan its metadata and, later, its upstream objects.
#[tokio::test]
#[serial]
async fn a_non_empty_bucket_cannot_be_deleted() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&user.token);
        let created = request
            .post("/api/buckets")
            .add_header(k, v)
            .json(&serde_json::json!({ "name": "not-empty", "max_bytes": 0 }))
            .await;
        let pid = created.json::<serde_json::Value>()["pid"]
            .as_str()
            .unwrap()
            .to_string();

        let bucket = buckets::Model::find_by_pid_for_user(&ctx.db, &pid, user.user.id)
            .await
            .unwrap();
        objects::Model::put_object(&ctx.db, bucket.id, "a.txt", 1, "e", "text/plain")
            .await
            .unwrap();

        let (k, v) = prepare_data::auth_header(&user.token);
        let res = request
            .delete(&format!("/api/buckets/{pid}"))
            .add_header(k, v)
            .await;
        assert_eq!(res.status_code(), 400);
    })
    .await;
}

/// A bucket belonging to someone else must read as absent, not as forbidden.
#[tokio::test]
#[serial]
async fn another_users_bucket_is_not_found() {
    request::<App, _, _>(|request, ctx| async move {
        let owner = prepare_data::init_user_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&owner.token);
        let created = request
            .post("/api/buckets")
            .add_header(k, v)
            .json(&serde_json::json!({ "name": "private", "max_bytes": 0 }))
            .await;
        let pid = created.json::<serde_json::Value>()["pid"]
            .as_str()
            .unwrap()
            .to_string();

        prepare_data::create_user(
            &ctx,
            "other@congty.vn",
            "12341234",
            "Other",
            users::ROLE_USER,
        )
        .await;
        let other = prepare_data::login(&request, "other@congty.vn", "12341234").await;

        let (k, v) = prepare_data::auth_header(&other);
        let shown = request
            .get(&format!("/api/buckets/{pid}"))
            .add_header(k, v)
            .await;
        assert_eq!(shown.status_code(), 404);

        let (k, v) = prepare_data::auth_header(&other);
        let deleted = request
            .delete(&format!("/api/buckets/{pid}"))
            .add_header(k, v)
            .await;
        assert_eq!(deleted.status_code(), 404);

        // And it is still there for its owner.
        assert!(
            buckets::Model::find_by_user_and_name(&ctx.db, owner.user.id, "private")
                .await
                .unwrap()
                .is_some()
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn summary_counts_real_buckets_and_keys() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&user.token);
        request
            .post("/api/buckets")
            .add_header(k, v)
            .json(&serde_json::json!({ "name": "one", "max_bytes": 0 }))
            .await;

        let (k, v) = prepare_data::auth_header(&user.token);
        request
            .post("/api/keys")
            .add_header(k, v)
            .json(&serde_json::json!({
                "label": "ci", "permissions": ["read"], "prefixes": []
            }))
            .await;

        let (k, v) = prepare_data::auth_header(&user.token);
        let res = request.get("/api/me/summary").add_header(k, v).await;
        assert_eq!(res.status_code(), 200);

        let body = res.json::<serde_json::Value>();
        assert_eq!(body["bucket_count"].as_i64(), Some(1));
        assert_eq!(body["active_key_count"].as_i64(), Some(1));
        assert_eq!(body["used_bytes"].as_i64(), Some(0));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_user_can_rename_themselves_but_not_change_role_or_quota() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;
        let before = user.user.clone();

        let (k, v) = prepare_data::auth_header(&user.token);
        let res = request
            .patch("/api/me")
            .add_header(k, v)
            .json(&serde_json::json!({
                "name": "Tên Mới", "role": "admin", "max_bytes": 99_999
            }))
            .await;
        assert_eq!(res.status_code(), 200);

        let after = users::Model::find_by_pid(&ctx.db, &before.pid.to_string())
            .await
            .unwrap();
        assert_eq!(after.name, "Tên Mới");
        assert_eq!(after.role, before.role);
        assert_eq!(after.max_bytes, before.max_bytes);
    })
    .await;
}
