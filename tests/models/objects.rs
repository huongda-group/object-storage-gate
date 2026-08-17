use loco_rs::testing::prelude::*;
use object_storage_gate::{
    app::App,
    models::{buckets, objects, users},
};
use sea_orm::{ActiveModelTrait, ActiveValue};
use serial_test::serial;

async fn bucket(db: &sea_orm::DatabaseConnection) -> i32 {
    let uid = users::ActiveModel {
        email: ActiveValue::set("o@ex.com".to_string()),
        password: ActiveValue::set("x".to_string()),
        name: ActiveValue::set("Us".to_string()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
    .id;
    let pool_id = super::any_pool(db).await;
    buckets::Model::create(db, uid, pool_id, "bkt", 0)
        .await
        .unwrap()
        .id
}

#[tokio::test]
#[serial]
async fn put_then_overwrite_same_row() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let bid = bucket(db).await;

    let v1 = objects::Model::put_object(db, bid, "a.txt", 10, "e1", "text/plain")
        .await
        .unwrap();
    let v2 = objects::Model::put_object(db, bid, "a.txt", 20, "e2", "text/plain")
        .await
        .unwrap();

    assert_eq!(v1.id, v2.id, "same row overwritten");
    let got = objects::Model::get(db, bid, "a.txt")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.size, 20);
    assert_eq!(got.etag, "e2");
}

#[tokio::test]
#[serial]
async fn delete_removes_object() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let bid = bucket(db).await;

    objects::Model::put_object(db, bid, "a.txt", 10, "e", "text/plain")
        .await
        .unwrap();
    objects::Model::delete(db, bid, "a.txt").await.unwrap();
    assert!(objects::Model::get(db, bid, "a.txt")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
#[serial]
async fn list_by_prefix_filters() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let bid = bucket(db).await;

    objects::Model::put_object(db, bid, "images/1.png", 1, "e", "image/png")
        .await
        .unwrap();
    objects::Model::put_object(db, bid, "images/2.png", 1, "e", "image/png")
        .await
        .unwrap();
    objects::Model::put_object(db, bid, "docs/1.txt", 1, "e", "text/plain")
        .await
        .unwrap();

    let listed = objects::Model::list_by_prefix(db, bid, "images/", 100)
        .await
        .unwrap();
    let keys: Vec<_> = listed.iter().map(|o| o.object_key.as_str()).collect();
    assert_eq!(keys, vec!["images/1.png", "images/2.png"]);
}

/// A LIKE wildcard in the prefix must match literally, not as a pattern.
/// Once access-key prefix scoping is wired to this query, an unescaped `_` lets a key
/// confined to `tenants/a/` read `tenants/ab/`, and `%` lists the whole bucket.
#[tokio::test]
#[serial]
async fn list_by_prefix_treats_wildcards_literally() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(db, "user1@example.com")
        .await
        .unwrap();
    let pool_id = super::any_pool(db).await;
    let bucket = buckets::Model::create(db, user.id, pool_id, "wildcards", 0)
        .await
        .unwrap();

    for key in ["a_/one", "ab/two", "a%/three", "az/four"] {
        objects::Model::put_object(db, bucket.id, key, 1, "e", "text/plain")
            .await
            .unwrap();
    }

    let underscore = objects::Model::list_by_prefix(db, bucket.id, "a_/", 100)
        .await
        .unwrap();
    assert_eq!(underscore.len(), 1);
    assert_eq!(underscore[0].object_key, "a_/one");

    let percent = objects::Model::list_by_prefix(db, bucket.id, "a%", 100)
        .await
        .unwrap();
    assert_eq!(percent.len(), 1);
    assert_eq!(percent[0].object_key, "a%/three");
}

/// An empty prefix lists the whole bucket, which is what `ListObjectsV2` with no prefix means.
#[tokio::test]
#[serial]
async fn list_by_prefix_with_empty_prefix_lists_everything() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(db, "user1@example.com")
        .await
        .unwrap();
    let pool_id = super::any_pool(db).await;
    let bucket = buckets::Model::create(db, user.id, pool_id, "everything", 0)
        .await
        .unwrap();

    for key in ["one", "two", "three"] {
        objects::Model::put_object(db, bucket.id, key, 1, "e", "text/plain")
            .await
            .unwrap();
    }

    let all = objects::Model::list_by_prefix(db, bucket.id, "", 100)
        .await
        .unwrap();
    assert_eq!(all.len(), 3);
}
