use loco_rs::testing::prelude::*;
use object_storage_gate::{
    app::App,
    models::{access_keys, buckets, objects, users},
};
use serial_test::serial;

async fn a_key(db: &sea_orm::DatabaseConnection, user_id: i32) -> (access_keys::Model, String) {
    access_keys::Model::create_key(
        db,
        user_id,
        &access_keys::CreateKeyParams {
            label: "primary".to_string(),
            expires_at: None,
            permissions: vec![access_keys::ACTION_READ.to_string()],
            prefixes: vec![],
        },
    )
    .await
    .unwrap()
}

/// The guard must live in the UPDATE, not in a snapshot read beforehand.
///
/// Reproduces the window an admin actually hits: the console loaded the key, the admin
/// revoked it, and the console's pending PATCH then reactivated a key that was supposed to
/// be dead.
#[tokio::test]
#[serial]
async fn a_revoked_key_cannot_be_reactivated_from_a_stale_model() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(db, "user1@example.com")
        .await
        .unwrap();
    let (key, _secret) = a_key(db, user.id).await;

    // The console loaded this before the admin acted.
    let stale = key.clone();

    key.revoke(db).await.unwrap();

    let result = stale.set_status(db, access_keys::KEY_ACTIVE).await;
    assert!(result.is_err(), "a stale model reactivated a revoked key");

    let all = access_keys::Model::list_for_user(db, user.id)
        .await
        .unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].key.status, access_keys::KEY_REVOKED);
}

/// Rotating a key revoked in the meantime must fail without leaving a live replacement whose
/// secret nobody ever saw.
#[tokio::test]
#[serial]
async fn a_revoked_key_cannot_be_rotated_from_a_stale_model() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(db, "user1@example.com")
        .await
        .unwrap();
    let (key, _secret) = a_key(db, user.id).await;

    let stale = key.clone();
    key.revoke(db).await.unwrap();

    assert!(stale.rotate(db).await.is_err());

    let all = access_keys::Model::list_for_user(db, user.id)
        .await
        .unwrap();
    assert_eq!(all.len(), 1, "rotate left an orphan key behind");
}

/// Revoking twice is not an error: it is the thing you do when containing an incident, and
/// a second click must not fail.
#[tokio::test]
#[serial]
async fn revoking_twice_is_idempotent() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(db, "user1@example.com")
        .await
        .unwrap();
    let (key, _secret) = a_key(db, user.id).await;

    let revoked = key.revoke(db).await.unwrap();
    let again = revoked.revoke(db).await;

    assert!(again.is_ok());
    assert_eq!(again.unwrap().status, access_keys::KEY_REVOKED);
}

/// Two writes to the same key must both succeed; the second overwrites the first.
/// The old read-then-insert let both see no row and both insert, and one hit the unique index
/// with a 500 — which S3 clients trigger routinely, because retrying is what they do.
#[tokio::test]
#[serial]
async fn concurrent_put_object_on_the_same_key_does_not_error() {
    let boot = boot_test::<App>().await.unwrap();
    let db = boot.app_context.db.clone();
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(&db, "user1@example.com")
        .await
        .unwrap();
    let bucket = buckets::Model::create(&db, user.id, "races", 0)
        .await
        .unwrap();

    let a = {
        let db = db.clone();
        tokio::spawn(async move {
            objects::Model::put_object(&db, bucket.id, "same/key", 1, "e1", "text/plain").await
        })
    };
    let b = {
        let db = db.clone();
        tokio::spawn(async move {
            objects::Model::put_object(&db, bucket.id, "same/key", 2, "e2", "text/plain").await
        })
    };

    let (ra, rb) = tokio::join!(a, b);
    assert!(ra.unwrap().is_ok(), "first concurrent put failed");
    assert!(rb.unwrap().is_ok(), "second concurrent put failed");

    let rows = objects::Model::list_by_prefix(&db, bucket.id, "same/", 100)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
}
