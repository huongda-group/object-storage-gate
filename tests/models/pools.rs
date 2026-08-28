use loco_rs::testing::prelude::*;
use object_storage_gate::{app::App, models::pools};
use serial_test::serial;

fn params(name: &str) -> pools::CreateParams {
    pools::CreateParams {
        name: name.to_string(),
        provider: pools::PROVIDER_MINIO.to_string(),
        region: Some("ap-southeast-1".to_string()),
        api_endpoint: Some("https://minio.internal:9000".to_string()),
        physical_bucket: "osg-main".to_string(),
        access_id: Some("UPSTREAMKEYID".to_string()),
        access_secret: Some("upstream-secret-value".to_string()),
    }
}

#[tokio::test]
#[serial]
async fn create_round_trips_and_encrypts_the_secret() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    let pool = pools::Model::create(db, &params("main")).await.unwrap();

    assert_eq!(pool.name, "main");
    assert_eq!(pool.physical_bucket, "osg-main");
    assert_eq!(pool.access_id.as_deref(), Some("UPSTREAMKEYID"));

    // Stored encrypted, recoverable in process.
    let blob = pool.access_secret_encrypted.clone().unwrap();
    assert_ne!(blob, b"upstream-secret-value".to_vec());
    assert_eq!(pool.decrypt_secret().unwrap(), "upstream-secret-value");
}

#[tokio::test]
#[serial]
async fn pool_names_are_unique() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    pools::Model::create(db, &params("main")).await.unwrap();
    assert!(pools::Model::create(db, &params("main")).await.is_err());
}

#[tokio::test]
#[serial]
async fn unknown_provider_is_rejected() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    let mut p = params("weird");
    p.provider = "dropbox".to_string();
    assert!(pools::Model::create(db, &p).await.is_err());
}

#[tokio::test]
#[serial]
async fn physical_bucket_is_required() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    let mut p = params("empty");
    p.physical_bucket = String::new();
    assert!(pools::Model::create(db, &p).await.is_err());
}

/// Updating without a new secret keeps the stored one — the admin form does not echo it back, so an empty field must mean "unchanged", never "erase".
#[tokio::test]
#[serial]
async fn update_without_a_secret_keeps_the_stored_one() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    let pool = pools::Model::create(db, &params("main")).await.unwrap();
    let updated = pool
        .update_config(
            db,
            &pools::UpdateParams {
                region: Some("us-east-1".to_string()),
                api_endpoint: None,
                physical_bucket: None,
                access_id: None,
                access_secret: None,
            },
        )
        .await
        .unwrap();

    assert_eq!(updated.region.as_deref(), Some("us-east-1"));
    assert_eq!(updated.decrypt_secret().unwrap(), "upstream-secret-value");
}

#[tokio::test]
#[serial]
async fn update_with_a_secret_replaces_it() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    let pool = pools::Model::create(db, &params("main")).await.unwrap();
    let updated = pool
        .update_config(
            db,
            &pools::UpdateParams {
                region: None,
                api_endpoint: None,
                physical_bucket: None,
                access_id: None,
                access_secret: Some("rotated-secret".to_string()),
            },
        )
        .await
        .unwrap();

    assert_eq!(updated.decrypt_secret().unwrap(), "rotated-secret");
}

/// A pool created without credentials is the backfill case: it exists so buckets can point at something, and every S3 request must fail loudly until an admin fills it in.
#[tokio::test]
#[serial]
async fn a_pool_without_a_secret_reports_it_rather_than_panicking() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    let mut p = params("bare");
    p.access_id = None;
    p.access_secret = None;
    let pool = pools::Model::create(db, &p).await.unwrap();

    assert!(pool.decrypt_secret().is_err());
    assert!(!pool.is_configured());
}

#[tokio::test]
#[serial]
async fn find_by_name_and_list_all_see_what_was_created() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    pools::Model::create(db, &params("zulu")).await.unwrap();
    pools::Model::create(db, &params("alpha")).await.unwrap();

    let found = pools::Model::find_by_name(db, "alpha").await.unwrap();
    assert_eq!(found.name, "alpha");

    // Ordered by name so the console table is stable across reloads.
    let all = pools::Model::list_all(db).await.unwrap();
    assert_eq!(
        all.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        vec!["alpha", "zulu"]
    );

    assert!(pools::Model::find_by_name(db, "nope").await.is_err());
}
