use loco_rs::testing::prelude::*;
use object_storage_gate::{
    app::App,
    models::{access_keys, buckets, objects, users},
};
use serial_test::serial;

/// A 512-character prefix is exactly what `MAX_PREFIX_LEN` promises callers.
/// On `MySQL` a varchar(255) column silently makes that promise a lie: the same request succeeds on Postgres and fails with "Data too long" there.
#[tokio::test]
#[serial]
async fn accepts_a_prefix_at_the_documented_maximum() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(db, "user1@example.com")
        .await
        .unwrap();

    let long_prefix = format!("{}/", "a".repeat(access_keys::MAX_PREFIX_LEN - 1));
    assert_eq!(long_prefix.len(), access_keys::MAX_PREFIX_LEN);

    let (key, _secret) = access_keys::Model::create_key(
        db,
        user.id,
        &access_keys::CreateKeyParams {
            label: "primary".to_string(),
            expires_at: None,
            permissions: vec![access_keys::ACTION_READ.to_string()],
            prefixes: vec![long_prefix.clone()],
        },
    )
    .await
    .expect("a prefix at the documented maximum must be storable");

    assert_eq!(key.prefixes(db).await.unwrap(), vec![long_prefix]);
}

/// S3 allows object keys up to 1024 bytes.
/// This is the smallest test that fails on a varchar(255) column.
#[tokio::test]
#[serial]
async fn accepts_an_object_key_at_the_s3_maximum() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(db, "user1@example.com")
        .await
        .unwrap();
    let pool_id = super::any_pool(db).await;
    let bucket = buckets::Model::create(db, user.id, pool_id, "keys-are-long", 0)
        .await
        .unwrap();

    let key = "k".repeat(1024);
    let stored = objects::Model::put_object(db, bucket.id, &key, 1, "etag", "text/plain")
        .await
        .expect("a 1024-byte object key must be storable");

    assert_eq!(stored.object_key.len(), 1024);
}

/// S3 object keys are case-sensitive.
/// `MySQL`'s default collation is not, so the unique index would treat two distinct keys as one and silently overwrite the first.
#[tokio::test]
#[serial]
async fn object_keys_are_case_sensitive() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(db, "user1@example.com")
        .await
        .unwrap();
    let pool_id = super::any_pool(db).await;
    let bucket = buckets::Model::create(db, user.id, pool_id, "case-matters", 0)
        .await
        .unwrap();

    objects::Model::put_object(db, bucket.id, "Photos/A.JPG", 10, "e1", "image/jpeg")
        .await
        .unwrap();
    objects::Model::put_object(db, bucket.id, "photos/a.jpg", 20, "e2", "image/jpeg")
        .await
        .unwrap();

    let upper = objects::Model::get(db, bucket.id, "Photos/A.JPG")
        .await
        .unwrap()
        .expect("the uppercase key must still exist");
    let lower = objects::Model::get(db, bucket.id, "photos/a.jpg")
        .await
        .unwrap()
        .expect("the lowercase key must exist");

    assert_eq!(upper.size, 10);
    assert_eq!(lower.size, 20);
}

/// Bucket names are lowercase-only by validation, so two names differing only in case can never both exist and the `MySQL` collation cannot bite there.
/// It still bites on object keys, which `object_keys_are_case_sensitive` covers — this asserts the validation that makes the bucket case moot.
#[tokio::test]
#[serial]
async fn bucket_names_reject_uppercase() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(db, "user1@example.com")
        .await
        .unwrap();

    let pool_id = super::any_pool(db).await;
    buckets::Model::create(db, user.id, pool_id, "media", 0)
        .await
        .unwrap();
    let second = buckets::Model::create(db, user.id, pool_id, "Media", 0).await;

    assert!(second.is_err(), "an uppercase bucket name must be rejected");
}
