use loco_rs::testing::prelude::*;
use object_storage_gate::{
    app::App,
    models::{buckets, multipart_uploads, users},
};
use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};
use serial_test::serial;

async fn a_bucket(db: &sea_orm::DatabaseConnection, name: &str) -> i32 {
    let user = users::Model::find_by_email(db, "user1@example.com")
        .await
        .unwrap();
    let pool_id = super::any_pool(db).await;
    buckets::Model::create(db, user.id, pool_id, name, 0)
        .await
        .unwrap()
        .id
}

/// The `UploadId` a client sees is this row's pid, and the lookup must pin it to the bucket and key from the path — otherwise an `UploadId` from one bucket can be replayed to write into another.
#[tokio::test]
#[serial]
async fn find_for_pins_the_upload_to_its_bucket_and_key() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let a = a_bucket(db, "bucket-a").await;
    let b = a_bucket(db, "bucket-b").await;
    let up = multipart_uploads::Model::create(db, a, "img/big.bin", "upstream-1")
        .await
        .unwrap();
    let pid = up.pid.to_string();

    assert!(
        multipart_uploads::Model::find_for(db, &pid, a, "img/big.bin")
            .await
            .is_ok()
    );
    assert!(
        multipart_uploads::Model::find_for(db, &pid, b, "img/big.bin")
            .await
            .is_err(),
        "right pid, wrong bucket"
    );
    assert!(
        multipart_uploads::Model::find_for(db, &pid, a, "img/other.bin")
            .await
            .is_err(),
        "right pid, right bucket, wrong key"
    );
}

/// `add_reserved` is a guarded UPDATE, so two concurrent `UploadPart` calls both count.
#[tokio::test]
#[serial]
async fn add_reserved_accumulates_across_concurrent_parts() {
    let boot = boot_test::<App>().await.unwrap();
    let db = boot.app_context.db.clone();
    seed::<App>(&boot.app_context).await.unwrap();

    let bucket_id = a_bucket(&db, "concurrent").await;
    let up = multipart_uploads::Model::create(&db, bucket_id, "big.bin", "upstream-1")
        .await
        .unwrap();
    let id = up.id;

    let a = {
        let db = db.clone();
        tokio::spawn(async move { multipart_uploads::Model::add_reserved(&db, id, 500).await })
    };
    let b = {
        let db = db.clone();
        tokio::spawn(async move { multipart_uploads::Model::add_reserved(&db, id, 300).await })
    };
    let (ra, rb) = tokio::join!(a, b);
    ra.unwrap().unwrap();
    rb.unwrap().unwrap();

    let fresh = multipart_uploads::Model::find_by_id(&db, id).await.unwrap();
    assert_eq!(
        fresh.reserved_bytes, 800,
        "a lost update means Abort releases too little"
    );
}

/// Deleting a bucket takes its open uploads with it; a row pointing at a gone bucket is one the cleanup task can never resolve.
#[tokio::test]
#[serial]
async fn uploads_cascade_with_their_bucket() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let bucket_id = a_bucket(db, "doomed").await;
    multipart_uploads::Model::create(db, bucket_id, "big.bin", "upstream-1")
        .await
        .unwrap();

    let bucket = buckets::Entity::find_by_id(bucket_id)
        .one(db)
        .await
        .unwrap()
        .unwrap();
    let am: buckets::ActiveModel = bucket.into();
    am.delete(db).await.unwrap();

    assert!(multipart_uploads::Model::list_for_bucket(db, bucket_id, "")
        .await
        .unwrap()
        .is_empty());
}

/// S3 allows several open uploads on the same key, so the index must not be unique.
#[tokio::test]
#[serial]
async fn several_uploads_may_be_open_on_one_key() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let bucket_id = a_bucket(db, "many").await;
    let a = multipart_uploads::Model::create(db, bucket_id, "big.bin", "u1")
        .await
        .unwrap();
    let b = multipart_uploads::Model::create(db, bucket_id, "big.bin", "u2")
        .await
        .unwrap();

    assert_ne!(a.pid, b.pid);
    assert_eq!(
        multipart_uploads::Model::list_for_bucket(db, bucket_id, "")
            .await
            .unwrap()
            .len(),
        2
    );
}

/// An object key can be 1024 bytes; `ColType::String` is varchar(255) on `MySQL`, which would refuse a legal key.
#[tokio::test]
#[serial]
async fn a_thousand_byte_key_is_accepted() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let bucket_id = a_bucket(db, "long-keys").await;
    let key = "k".repeat(1024);
    let up = multipart_uploads::Model::create(db, bucket_id, &key, "u1")
        .await
        .unwrap();

    assert_eq!(up.object_key.len(), 1024);
    assert!(
        multipart_uploads::Model::find_for(db, &up.pid.to_string(), bucket_id, &key)
            .await
            .is_ok()
    );
}

#[tokio::test]
#[serial]
async fn older_than_finds_only_stale_uploads() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let bucket_id = a_bucket(db, "stale").await;
    let fresh = multipart_uploads::Model::create(db, bucket_id, "new.bin", "u1")
        .await
        .unwrap();
    let old = multipart_uploads::Model::create(db, bucket_id, "old.bin", "u2")
        .await
        .unwrap();

    let mut am: multipart_uploads::ActiveModel = old.into();
    am.created_at = ActiveValue::set((chrono::Utc::now() - chrono::Duration::days(10)).into());
    let old = am.update(db).await.unwrap();

    let stale = multipart_uploads::Model::older_than(db, 7).await.unwrap();
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].id, old.id);
    assert_ne!(stale[0].id, fresh.id);
}
