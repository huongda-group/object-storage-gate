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
    buckets::Model::create(db, uid, "b", 0).await.unwrap().id
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
