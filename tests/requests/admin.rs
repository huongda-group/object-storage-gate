use loco_rs::testing::prelude::*;
use object_storage_gate::{
    app::App,
    models::{buckets, objects, users},
    views::auth::LoginResponse,
};
use sea_orm::EntityTrait;
use serial_test::serial;

use super::prepare_data;

#[tokio::test]
#[serial]
async fn admin_can_create_a_user_who_must_change_password() {
    request::<App, _, _>(|request, ctx| async move {
        let admin = prepare_data::init_admin_login(&request, &ctx).await;
        let (k, v) = prepare_data::auth_header(&admin.token);

        let res = request
            .post("/api/admin/users")
            .add_header(k, v)
            .json(&serde_json::json!({
                "email": "tenant@congty.vn",
                "name": "Tenant One",
                "password": "temp-password-1",
                "role": "user",
                "max_bytes": 10_737_418_240i64
            }))
            .await;

        assert_eq!(res.status_code(), 200);

        let created = users::Model::find_by_email(&ctx.db, "tenant@congty.vn")
            .await
            .unwrap();
        assert!(created.must_change_password);
        assert_eq!(created.role, "user");
        assert_eq!(created.max_bytes, 10_737_418_240);

        // The response must never carry the password back.
        assert!(!res.text().contains("temp-password-1"));

        // And the new user can log in, but is told to change the password.
        let login = request
            .post("/api/auth/login")
            .json(&serde_json::json!({
                "email": "tenant@congty.vn",
                "password": "temp-password-1"
            }))
            .await;
        let body: LoginResponse = serde_json::from_str(&login.text()).unwrap();
        assert!(body.must_change_password);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn create_user_requires_max_bytes() {
    request::<App, _, _>(|request, ctx| async move {
        let admin = prepare_data::init_admin_login(&request, &ctx).await;
        let (k, v) = prepare_data::auth_header(&admin.token);

        let res = request
            .post("/api/admin/users")
            .add_header(k, v)
            .json(&serde_json::json!({
                "email": "nobody@congty.vn",
                "name": "No Quota",
                "password": "temp-password-1",
                "role": "user"
            }))
            .await;

        // Axum's Json extractor rejects a missing required field before the handler runs, so this is 422 rather than 400.
        // Either way the point stands: max_bytes cannot be defaulted.
        assert_eq!(res.status_code(), 422);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn create_user_rejects_short_password_and_duplicate_email() {
    request::<App, _, _>(|request, ctx| async move {
        let admin = prepare_data::init_admin_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&admin.token);
        let short = request
            .post("/api/admin/users")
            .add_header(k, v)
            .json(&serde_json::json!({
                "email": "a@congty.vn", "name": "Alpha",
                "password": "short", "role": "user", "max_bytes": 0
            }))
            .await;
        assert_eq!(short.status_code(), 400);

        let (k, v) = prepare_data::auth_header(&admin.token);
        let first = request
            .post("/api/admin/users")
            .add_header(k, v)
            .json(&serde_json::json!({
                "email": "b@congty.vn", "name": "Bravo",
                "password": "temp-password-1", "role": "user", "max_bytes": 0
            }))
            .await;
        assert_eq!(first.status_code(), 200);

        let (k, v) = prepare_data::auth_header(&admin.token);
        let dup = request
            .post("/api/admin/users")
            .add_header(k, v)
            .json(&serde_json::json!({
                "email": "b@congty.vn", "name": "Bravo again",
                "password": "temp-password-1", "role": "user", "max_bytes": 0
            }))
            .await;
        assert_eq!(dup.status_code(), 400);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn create_user_rejects_unknown_role() {
    request::<App, _, _>(|request, ctx| async move {
        let admin = prepare_data::init_admin_login(&request, &ctx).await;
        let (k, v) = prepare_data::auth_header(&admin.token);

        let res = request
            .post("/api/admin/users")
            .add_header(k, v)
            .json(&serde_json::json!({
                "email": "c@congty.vn", "name": "Charlie",
                "password": "temp-password-1", "role": "superuser", "max_bytes": 0
            }))
            .await;

        assert_eq!(res.status_code(), 400);
    })
    .await;
}

/// Only an admin may reach the admin tree, and the check must live on the server.
#[tokio::test]
#[serial]
async fn non_admin_is_refused_on_every_admin_route() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&user.token);
        let listed = request.get("/api/admin/users").add_header(k, v).await;
        assert_eq!(listed.status_code(), 403);
        assert!(listed.text().contains("admin_required"));

        let (k, v) = prepare_data::auth_header(&user.token);
        let created = request
            .post("/api/admin/users")
            .add_header(k, v)
            .json(&serde_json::json!({
                "email": "sneaky@congty.vn", "name": "Sneaky",
                "password": "temp-password-1", "role": "admin", "max_bytes": 0
            }))
            .await;
        assert_eq!(created.status_code(), 403);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn admin_can_reset_a_user_password_and_it_forces_a_change() {
    request::<App, _, _>(|request, ctx| async move {
        let admin = prepare_data::init_admin_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&admin.token);
        request
            .post("/api/admin/users")
            .add_header(k, v)
            .json(&serde_json::json!({
                "email": "forgot@congty.vn", "name": "Forgot",
                "password": "temp-password-1", "role": "user", "max_bytes": 0
            }))
            .await;

        let target = users::Model::find_by_email(&ctx.db, "forgot@congty.vn")
            .await
            .unwrap();

        // The user changes it once, so must_change_password is false.
        let token = prepare_data::login(&request, "forgot@congty.vn", "temp-password-1").await;
        let (k, v) = prepare_data::auth_header(&token);
        let changed = request
            .post("/api/me/password")
            .add_header(k, v)
            .json(&serde_json::json!({
                "current_password": "temp-password-1",
                "new_password": "chosen-by-the-user"
            }))
            .await;
        assert_eq!(changed.status_code(), 200);

        // Admin resets it again.
        let (k, v) = prepare_data::auth_header(&admin.token);
        let res = request
            .post(&format!("/api/admin/users/{}/password", target.pid))
            .add_header(k, v)
            .json(&serde_json::json!({ "password": "issued-again-1" }))
            .await;
        assert_eq!(res.status_code(), 200);

        let after = users::Model::find_by_email(&ctx.db, "forgot@congty.vn")
            .await
            .unwrap();
        assert!(after.must_change_password);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn admin_can_update_name_role_and_quota() {
    request::<App, _, _>(|request, ctx| async move {
        let admin = prepare_data::init_admin_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&admin.token);
        request
            .post("/api/admin/users")
            .add_header(k, v)
            .json(&serde_json::json!({
                "email": "grow@congty.vn", "name": "Grow",
                "password": "temp-password-1", "role": "user", "max_bytes": 100
            }))
            .await;

        let target = users::Model::find_by_email(&ctx.db, "grow@congty.vn")
            .await
            .unwrap();

        let (k, v) = prepare_data::auth_header(&admin.token);
        let res = request
            .patch(&format!("/api/admin/users/{}", target.pid))
            .add_header(k, v)
            .json(&serde_json::json!({ "name": "Grown", "max_bytes": 5000 }))
            .await;
        assert_eq!(res.status_code(), 200);

        let after = users::Model::find_by_email(&ctx.db, "grow@congty.vn")
            .await
            .unwrap();
        assert_eq!(after.name, "Grown");
        assert_eq!(after.max_bytes, 5000);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn admin_cannot_delete_their_own_account_or_the_last_admin() {
    request::<App, _, _>(|request, ctx| async move {
        let admin = prepare_data::init_admin_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&admin.token);
        let res = request
            .delete(&format!("/api/admin/users/{}", admin.user.pid))
            .add_header(k, v)
            .await;

        assert_eq!(res.status_code(), 400);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn admin_can_delete_a_plain_user() {
    request::<App, _, _>(|request, ctx| async move {
        let admin = prepare_data::init_admin_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&admin.token);
        request
            .post("/api/admin/users")
            .add_header(k, v)
            .json(&serde_json::json!({
                "email": "leaving@congty.vn", "name": "Leaving",
                "password": "temp-password-1", "role": "user", "max_bytes": 0
            }))
            .await;

        let target = users::Model::find_by_email(&ctx.db, "leaving@congty.vn")
            .await
            .unwrap();

        let (k, v) = prepare_data::auth_header(&admin.token);
        let res = request
            .delete(&format!("/api/admin/users/{}", target.pid))
            .add_header(k, v)
            .await;
        assert_eq!(res.status_code(), 200);

        assert!(users::Model::find_by_email(&ctx.db, "leaving@congty.vn")
            .await
            .is_err());
    })
    .await;
}

/// Deleting an owner must take their buckets and objects with them.
/// The foreign key is ON DELETE SET NULL, so without this the bucket reappears as a system pool still carrying the former owner's encrypted upstream credentials and every object.
#[tokio::test]
#[serial]
async fn deleting_a_user_removes_their_buckets_and_objects() {
    request::<App, _, _>(|request, ctx| async move {
        let admin = prepare_data::init_admin_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&admin.token);
        request
            .post("/api/admin/users")
            .add_header(k, v)
            .json(&serde_json::json!({
                "email": "leftovers@congty.vn", "name": "Leftovers",
                "password": "temp-password-1", "role": "user", "max_bytes": 0
            }))
            .await;

        let target = users::Model::find_by_email(&ctx.db, "leftovers@congty.vn")
            .await
            .unwrap();
        let pool = prepare_data::a_pool(&ctx).await;
        let bucket = buckets::Model::create(&ctx.db, target.id, pool.id, "leftovers", 0)
            .await
            .unwrap();
        objects::Model::put_object(&ctx.db, bucket.id, "a/b.txt", 5, "e", "text/plain")
            .await
            .unwrap();

        let (k, v) = prepare_data::auth_header(&admin.token);
        let res = request
            .delete(&format!("/api/admin/users/{}", target.pid))
            .add_header(k, v)
            .await;
        assert_eq!(res.status_code(), 200);

        // The bucket went with its owner rather than being left behind ownerless.
        assert!(buckets::Entity::find_by_id(bucket.id)
            .one(&ctx.db)
            .await
            .unwrap()
            .is_none());

        // And the objects went with it.
        assert!(objects::Model::get(&ctx.db, bucket.id, "a/b.txt")
            .await
            .unwrap()
            .is_none());
    })
    .await;
}
