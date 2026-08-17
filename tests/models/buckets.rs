use loco_rs::testing::prelude::*;
use object_storage_gate::{
    app::App,
    models::{buckets, pools},
};
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

async fn pool(db: &sea_orm::DatabaseConnection, name: &str) -> pools::Model {
    pools::Model::create(
        db,
        &pools::CreateParams {
            name: name.to_string(),
            provider: pools::PROVIDER_MINIO.to_string(),
            physical_bucket: "osg-main".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
#[serial]
async fn create_and_find_bucket() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let uid = user(db, "u1@ex.com").await;
    let p = pool(db, "main").await;

    let b = buckets::Model::create(db, uid, p.id, "photos", 0)
        .await
        .unwrap();
    assert!(!b.pid.is_nil());
    assert!(b.is_unlimited());
    assert_eq!(b.object_count, 0);
    assert_eq!(b.pool_id, p.id);

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
    let p = pool(db, "main").await;

    buckets::Model::create(db, u1, p.id, "photos", 0)
        .await
        .unwrap();
    // Same name, different user → OK.
    buckets::Model::create(db, u2, p.id, "photos", 0)
        .await
        .unwrap();
    // Same name, same user → unique-index violation.
    assert!(buckets::Model::create(db, u1, p.id, "photos", 0)
        .await
        .is_err());
}

/// A bucket cannot exist without a pool: the gateway would have nowhere to proxy it.
#[tokio::test]
#[serial]
async fn a_bucket_belongs_to_a_pool() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let uid = user(db, "bind@ex.com").await;
    let one = pool(db, "one").await;
    let two = pool(db, "two").await;

    let a = buckets::Model::create(db, uid, one.id, "media-cdn", 0)
        .await
        .unwrap();
    let b = buckets::Model::create(db, uid, two.id, "backup-db", 0)
        .await
        .unwrap();

    assert_eq!(a.pool_id, one.id);
    assert_eq!(b.pool_id, two.id);

    // A pool_id that names no pool is refused rather than stored.
    assert!(buckets::Model::create(db, uid, 999_999, "orphan", 0)
        .await
        .is_err());
}

#[tokio::test]
#[serial]
async fn count_for_pool_sees_every_owner() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let a = user(db, "count-a@ex.com").await;
    let b = user(db, "count-b@ex.com").await;
    let shared = pool(db, "shared").await;
    let spare = pool(db, "spare").await;

    buckets::Model::create(db, a, shared.id, "a-one", 0)
        .await
        .unwrap();
    buckets::Model::create(db, b, shared.id, "b-one", 0)
        .await
        .unwrap();

    assert_eq!(
        buckets::Model::count_for_pool(db, shared.id).await.unwrap(),
        2
    );
    assert_eq!(
        buckets::Model::count_for_pool(db, spare.id).await.unwrap(),
        0
    );
}

#[tokio::test]
#[serial]
async fn buckets_default_to_private() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let uid = user(db, "priv@ex.com").await;
    let p = pool(db, "main").await;

    let b = buckets::Model::create(db, uid, p.id, "media-cdn", 0)
        .await
        .unwrap();
    assert!(!b.is_public());
}

#[tokio::test]
#[serial]
async fn list_for_user_excludes_other_owners() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let a = user(db, "list-a@ex.com").await;
    let b = user(db, "list-b@ex.com").await;
    let p = pool(db, "main").await;

    buckets::Model::create(db, a, p.id, "a-two", 0)
        .await
        .unwrap();
    buckets::Model::create(db, a, p.id, "a-one", 0)
        .await
        .unwrap();
    buckets::Model::create(db, b, p.id, "b-one", 0)
        .await
        .unwrap();

    let rows = buckets::Model::list_for_user(db, a).await.unwrap();
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, vec!["a-one", "a-two"]); // ordered by name
}
