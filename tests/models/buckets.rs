use loco_rs::testing::prelude::*;
use object_storage_gate::{app::App, models::buckets};
use sea_orm::{ActiveModelTrait, ActiveValue};
use serial_test::serial;

async fn user(db: &sea_orm::DatabaseConnection, email: &str) -> i32 {
    use object_storage_gate::models::users;
    users::ActiveModel {
        email: ActiveValue::set(email.to_string()),
        password: ActiveValue::set("x".to_string()),
        name: ActiveValue::set("Us".to_string()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
    .id
}

#[tokio::test]
#[serial]
async fn create_and_find_bucket() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let uid = user(db, "u1@ex.com").await;

    let b = buckets::Model::create(db, uid, "photos", 0).await.unwrap();
    assert!(!b.pid.is_nil());
    assert!(b.is_unlimited());
    assert_eq!(b.object_count, 0);

    let found = buckets::Model::find_by_user_and_name(db, uid, "photos")
        .await
        .unwrap();
    assert_eq!(found.unwrap().id, b.id);
    assert!(buckets::Model::find_by_user_and_name(db, uid, "nope")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
#[serial]
async fn bucket_name_unique_per_user() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let u1 = user(db, "a@ex.com").await;
    let u2 = user(db, "b@ex.com").await;

    buckets::Model::create(db, u1, "photos", 0).await.unwrap();
    // Same name, different user → OK.
    buckets::Model::create(db, u2, "photos", 0).await.unwrap();
    // Same name, same user → unique-index violation.
    assert!(buckets::Model::create(db, u1, "photos", 0).await.is_err());
}

#[tokio::test]
#[serial]
async fn system_pool_has_no_owner_and_unique_name() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let uid = user(db, "owner@ex.com").await;

    let pool = buckets::Model::create_system(db, "system-archive", 0)
        .await
        .unwrap();
    assert!(pool.is_system());
    assert!(pool.user_id.is_none());

    // A user may own a bucket named like a system pool.
    let owned = buckets::Model::create(db, uid, "system-archive", 0)
        .await
        .unwrap();
    assert!(!owned.is_system());

    // Two system pools may not share a name (COALESCE(user_id,0) unique index).
    assert!(buckets::Model::create_system(db, "system-archive", 0)
        .await
        .is_err());

    let found = buckets::Model::find_system_by_name(db, "system-archive")
        .await
        .unwrap()
        .expect("system pool");
    assert_eq!(found.id, pool.id);
}

#[tokio::test]
#[serial]
async fn store_config_round_trips_with_encrypted_secret() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let uid = user(db, "store@ex.com").await;
    let b = buckets::Model::create(db, uid, "media-cdn", 0)
        .await
        .unwrap();

    // Defaults: internal provider, private, no upstream credentials.
    assert_eq!(b.provider, buckets::PROVIDER_INTERNAL);
    assert!(!b.is_public());
    assert!(b.decrypt_store_secret().is_err());

    let b = b
        .set_store(
            db,
            &buckets::StoreParams {
                provider: buckets::PROVIDER_R2.to_string(),
                region: Some("apac".to_string()),
                api_endpoint: Some("https://acc.r2.cloudflarestorage.com".to_string()),
                access_id: Some("R2AK7X9Q2M4N".to_string()),
                access_secret: Some("upstream-secret".to_string()),
                public_enabled: true,
            },
        )
        .await
        .unwrap();

    assert_eq!(b.provider, buckets::PROVIDER_R2);
    assert!(b.is_public());
    assert_eq!(b.region.as_deref(), Some("apac"));
    // Stored encrypted, not in the clear.
    let blob = b.access_secret_encrypted.as_deref().expect("secret stored");
    assert_ne!(blob, b"upstream-secret");
    assert_eq!(b.decrypt_store_secret().unwrap(), "upstream-secret");
}

#[tokio::test]
#[serial]
async fn store_rejects_unknown_provider_and_keeps_secret_on_omit() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let uid = user(db, "prov@ex.com").await;
    let b = buckets::Model::create(db, uid, "backup-db", 0)
        .await
        .unwrap();

    assert!(b
        .set_store(
            db,
            &buckets::StoreParams {
                provider: "dropbox".to_string(),
                ..Default::default()
            },
        )
        .await
        .is_err());

    let b = b
        .set_store(
            db,
            &buckets::StoreParams {
                provider: buckets::PROVIDER_AWS.to_string(),
                access_secret: Some("keep-me".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    // Editing without re-typing the secret must not wipe it.
    let b = b
        .set_store(
            db,
            &buckets::StoreParams {
                provider: buckets::PROVIDER_AWS.to_string(),
                region: Some("ap-southeast-1".to_string()),
                access_secret: None,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(b.decrypt_store_secret().unwrap(), "keep-me");
}
