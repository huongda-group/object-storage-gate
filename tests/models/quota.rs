use loco_rs::testing::prelude::*;
use object_storage_gate::{
    app::App,
    models::{buckets, objects, quota, users},
};
use sea_orm::{ActiveModelTrait, ActiveValue, DatabaseConnection, EntityTrait};
use serial_test::serial;

/// Sets up the seeded owner with `user_max` and one bucket with `bucket_max`.
async fn setup(db: &DatabaseConnection, user_max: i64, bucket_max: i64) -> (i32, i32) {
    let user = users::Model::find_by_email(db, "user1@example.com")
        .await
        .unwrap();
    let mut am: users::ActiveModel = user.into();
    am.max_bytes = ActiveValue::set(user_max);
    let user = am.update(db).await.unwrap();

    let pool_id = super::any_pool(db).await;
    let bucket = buckets::Model::create(db, user.id, pool_id, "quota-test", bucket_max)
        .await
        .unwrap();

    (user.id, bucket.id)
}

async fn bucket_of(db: &DatabaseConnection, bucket_id: i32) -> buckets::Model {
    buckets::Entity::find_by_id(bucket_id)
        .one(db)
        .await
        .unwrap()
        .unwrap()
}

async fn user_of(db: &DatabaseConnection) -> users::Model {
    users::Model::find_by_email(db, "user1@example.com")
        .await
        .unwrap()
}

#[tokio::test]
#[serial]
async fn reserve_then_commit_moves_bytes_from_reserved_to_used() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (_user_id, bucket_id) = setup(db, 1000, 500).await;

    let reservation = quota::reserve(db, bucket_id, 100).await.unwrap();

    let b = bucket_of(db, bucket_id).await;
    assert_eq!(b.reserved_bytes, 100);
    assert_eq!(b.used_bytes, 0);

    quota::commit(db, &reservation, 1).await.unwrap();

    let b = bucket_of(db, bucket_id).await;
    assert_eq!(b.reserved_bytes, 0);
    assert_eq!(b.used_bytes, 100);
    assert_eq!(b.object_count, 1);

    let u = user_of(db).await;
    assert_eq!(u.reserved_bytes, 0);
    assert_eq!(u.used_bytes, 100);
}

#[tokio::test]
#[serial]
async fn release_gives_the_reservation_back() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (_user_id, bucket_id) = setup(db, 1000, 500).await;

    let reservation = quota::reserve(db, bucket_id, 200).await.unwrap();
    quota::release(db, &reservation).await.unwrap();

    let b = bucket_of(db, bucket_id).await;
    assert_eq!(b.reserved_bytes, 0);
    assert_eq!(b.used_bytes, 0);
    assert_eq!(user_of(db).await.reserved_bytes, 0);
}

#[tokio::test]
#[serial]
async fn reserve_refuses_past_the_bucket_quota() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (_user_id, bucket_id) = setup(db, 10_000, 500).await;

    quota::reserve(db, bucket_id, 400).await.unwrap();
    let refused = quota::reserve(db, bucket_id, 200).await;

    assert!(refused.is_err());
    assert!(refused.unwrap_err().to_string().contains("quota exceeded"));
}

/// The account quota is the outer bound: a bucket with room must still be refused when the
/// account has none, and the bucket-level hold must not be left behind.
#[tokio::test]
#[serial]
async fn reserve_refuses_past_the_account_quota_and_rolls_the_bucket_back() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (_user_id, bucket_id) = setup(db, 300, 10_000).await;

    let refused = quota::reserve(db, bucket_id, 400).await;
    assert!(refused.is_err());

    let b = bucket_of(db, bucket_id).await;
    assert_eq!(
        b.reserved_bytes, 0,
        "a failed account reserve leaked a bucket reservation"
    );
}

/// `max_bytes == 0` means unlimited, which is what `is_unlimited()` already documents.
#[tokio::test]
#[serial]
async fn zero_max_bytes_means_unlimited() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (_user_id, bucket_id) = setup(db, 0, 0).await;

    let r = quota::reserve(db, bucket_id, 999_999_999_999)
        .await
        .unwrap();
    quota::commit(db, &r, 1).await.unwrap();
}

/// Two reservations racing for the last slot: exactly one must win.
/// This is the whole reason the guard lives in the UPDATE rather than in a read beforehand.
#[tokio::test]
#[serial]
async fn concurrent_reserves_cannot_both_win_the_last_slot() {
    let boot = boot_test::<App>().await.unwrap();
    let db = boot.app_context.db.clone();
    seed::<App>(&boot.app_context).await.unwrap();
    let (_user_id, bucket_id) = setup(&db, 10_000, 100).await;

    let a = {
        let db = db.clone();
        tokio::spawn(async move { quota::reserve(&db, bucket_id, 60).await })
    };
    let b = {
        let db = db.clone();
        tokio::spawn(async move { quota::reserve(&db, bucket_id, 60).await })
    };

    let (ra, rb) = tokio::join!(a, b);
    let wins = [ra.unwrap().is_ok(), rb.unwrap().is_ok()]
        .iter()
        .filter(|ok| **ok)
        .count();

    assert_eq!(wins, 1, "both reservations fit into a 100-byte bucket");
}

/// A bucket row with no owner charges no account quota.
///
/// Nothing in the API creates one any more — `create_system` went away with the pools table — but the column is still nullable, so the branch is still reachable from a hand-written row or an old backup.
/// Inserted directly here on purpose: this is the state the quota engine must survive, not a state it should offer.
#[tokio::test]
#[serial]
async fn an_ownerless_bucket_has_no_account_quota() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let pool_id = super::any_pool(db).await;
    let orphan = buckets::ActiveModel {
        user_id: ActiveValue::set(None),
        pool_id: ActiveValue::set(pool_id),
        name: ActiveValue::set("archive".to_string()),
        max_bytes: ActiveValue::set(0),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();

    let r = quota::reserve(db, orphan.id, 5000).await.unwrap();
    assert_eq!(r.user_id, None);
    quota::commit(db, &r, 1).await.unwrap();
}

#[tokio::test]
#[serial]
async fn put_object_charges_the_quota() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (_user_id, bucket_id) = setup(db, 10_000, 1000).await;

    objects::Model::put_object(
        db,
        bucket_id,
        "a.bin",
        300,
        "e1",
        "application/octet-stream",
    )
    .await
    .unwrap();

    let b = bucket_of(db, bucket_id).await;
    assert_eq!(b.used_bytes, 300);
    assert_eq!(b.object_count, 1);
    assert_eq!(b.reserved_bytes, 0);
    assert_eq!(user_of(db).await.used_bytes, 300);
}

/// Overwriting must charge the difference, not the whole new size again.
#[tokio::test]
#[serial]
async fn overwriting_charges_only_the_delta() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (_user_id, bucket_id) = setup(db, 10_000, 1000).await;

    objects::Model::put_object(
        db,
        bucket_id,
        "a.bin",
        300,
        "e1",
        "application/octet-stream",
    )
    .await
    .unwrap();
    objects::Model::put_object(
        db,
        bucket_id,
        "a.bin",
        500,
        "e2",
        "application/octet-stream",
    )
    .await
    .unwrap();

    let b = bucket_of(db, bucket_id).await;
    assert_eq!(b.used_bytes, 500, "overwrite double-charged");
    assert_eq!(b.object_count, 1, "overwrite counted a second object");
}

/// Shrinking an object must give the difference back.
#[tokio::test]
#[serial]
async fn overwriting_smaller_returns_the_difference() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (_user_id, bucket_id) = setup(db, 10_000, 1000).await;

    objects::Model::put_object(
        db,
        bucket_id,
        "a.bin",
        800,
        "e1",
        "application/octet-stream",
    )
    .await
    .unwrap();
    objects::Model::put_object(
        db,
        bucket_id,
        "a.bin",
        100,
        "e2",
        "application/octet-stream",
    )
    .await
    .unwrap();

    let b = bucket_of(db, bucket_id).await;
    assert_eq!(b.used_bytes, 100);
    assert_eq!(user_of(db).await.used_bytes, 100);
}

#[tokio::test]
#[serial]
async fn put_object_is_refused_past_the_quota_and_stores_nothing() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (_user_id, bucket_id) = setup(db, 10_000, 500).await;

    let refused = objects::Model::put_object(
        db,
        bucket_id,
        "big.bin",
        900,
        "e",
        "application/octet-stream",
    )
    .await;
    assert!(refused.is_err());

    assert!(objects::Model::get(db, bucket_id, "big.bin")
        .await
        .unwrap()
        .is_none());

    let b = bucket_of(db, bucket_id).await;
    assert_eq!(b.used_bytes, 0);
    assert_eq!(b.reserved_bytes, 0, "a refused write leaked a reservation");
}

#[tokio::test]
#[serial]
async fn deleting_returns_the_bytes() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (_user_id, bucket_id) = setup(db, 10_000, 1000).await;

    objects::Model::put_object(
        db,
        bucket_id,
        "a.bin",
        300,
        "e1",
        "application/octet-stream",
    )
    .await
    .unwrap();
    objects::Model::delete(db, bucket_id, "a.bin")
        .await
        .unwrap();

    let b = bucket_of(db, bucket_id).await;
    assert_eq!(b.used_bytes, 0);
    assert_eq!(b.object_count, 0);
    assert_eq!(user_of(db).await.used_bytes, 0);
}

/// Deleting something that is not there must not drive the counters negative.
#[tokio::test]
#[serial]
async fn deleting_a_missing_object_changes_nothing() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (_user_id, bucket_id) = setup(db, 10_000, 1000).await;

    objects::Model::delete(db, bucket_id, "never-existed")
        .await
        .unwrap();

    let b = bucket_of(db, bucket_id).await;
    assert_eq!(b.used_bytes, 0);
    assert_eq!(b.object_count, 0);
}

/// Drift is the normal state after a crash between reserve and commit.
/// Reconcile must recompute both levels from the object rows, which are the only truth.
#[tokio::test]
#[serial]
async fn reconcile_recomputes_totals_from_the_object_rows() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (_user_id, bucket_id) = setup(db, 0, 0).await;

    objects::Model::put_object(db, bucket_id, "a", 100, "e", "text/plain")
        .await
        .unwrap();
    objects::Model::put_object(db, bucket_id, "b", 250, "e", "text/plain")
        .await
        .unwrap();

    // Simulate a crash between reserve and commit, plus a bogus used_bytes.
    let mut am: buckets::ActiveModel = bucket_of(db, bucket_id).await.into();
    am.reserved_bytes = ActiveValue::set(9_999);
    am.used_bytes = ActiveValue::set(7);
    am.object_count = ActiveValue::set(42);
    am.update(db).await.unwrap();

    let report = quota::reconcile(db).await.unwrap();
    assert!(report.buckets_fixed >= 1);

    let b = bucket_of(db, bucket_id).await;
    assert_eq!(b.used_bytes, 350);
    assert_eq!(b.object_count, 2);
    assert_eq!(
        b.reserved_bytes, 0,
        "reconcile must clear stale reservations"
    );

    let u = user_of(db).await;
    assert_eq!(u.used_bytes, 350);
    assert_eq!(u.reserved_bytes, 0);
}

/// A bucket that is already correct must not be reported as fixed.
#[tokio::test]
#[serial]
async fn reconcile_is_idempotent() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (_user_id, bucket_id) = setup(db, 0, 0).await;

    objects::Model::put_object(db, bucket_id, "a", 100, "e", "text/plain")
        .await
        .unwrap();

    quota::reconcile(db).await.unwrap();
    let second = quota::reconcile(db).await.unwrap();

    assert_eq!(second.buckets_fixed, 0);
    assert_eq!(second.users_fixed, 0);
}
