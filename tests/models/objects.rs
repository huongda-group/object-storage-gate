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

async fn seed_keys(db: &sea_orm::DatabaseConnection, bucket_id: i32, keys: &[&str]) {
    for k in keys {
        objects::Model::put_object(db, bucket_id, k, 1, "e", "text/plain")
            .await
            .unwrap();
    }
}

fn page_query(bucket_id: i32, prefix: &str, after: Option<&str>, limit: u64) -> objects::ListQuery {
    objects::ListQuery {
        bucket_id,
        prefix: prefix.to_string(),
        after: after.map(str::to_string),
        limit,
    }
}

/// The extra row is how `IsTruncated` is decided without a second COUNT query.
#[tokio::test]
#[serial]
async fn list_page_returns_one_more_than_the_limit_when_there_is_more() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    let bucket_id = bucket(db).await;
    seed_keys(db, bucket_id, &["a", "b", "c", "d", "e"]).await;

    let page = objects::Model::list_page(db, &page_query(bucket_id, "", None, 3))
        .await
        .unwrap();

    assert_eq!(page.len(), 4, "3 rows plus the lookahead");
    assert_eq!(page[0].object_key, "a");
}

#[tokio::test]
#[serial]
async fn list_page_stops_at_the_end_without_a_lookahead_row() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    let bucket_id = bucket(db).await;
    seed_keys(db, bucket_id, &["a", "b"]).await;

    let page = objects::Model::list_page(db, &page_query(bucket_id, "", None, 10))
        .await
        .unwrap();

    assert_eq!(page.len(), 2);
}

/// `after` is exclusive: the marker itself must not come back, or every page repeats one key.
#[tokio::test]
#[serial]
async fn after_is_exclusive() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    let bucket_id = bucket(db).await;
    seed_keys(db, bucket_id, &["a", "b", "c"]).await;

    let page = objects::Model::list_page(db, &page_query(bucket_id, "", Some("b"), 10))
        .await
        .unwrap();

    assert_eq!(
        page.iter()
            .map(|o| o.object_key.as_str())
            .collect::<Vec<_>>(),
        vec!["c"]
    );
}

/// Ordering must be byte order, not collation order.
///
/// S3 lists keys byte-ascending, and on `MySQL` a case-insensitive collation would interleave `B` between `a` and `b` — the list still holds every key, only in the wrong order, so a paging client silently skips or repeats.
/// The binary collation from `m20260817_000004` is what makes this hold.
#[tokio::test]
#[serial]
async fn ordering_is_byte_ascending_across_case() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    let bucket_id = bucket(db).await;
    seed_keys(db, bucket_id, &["b", "A", "a", "B"]).await;

    let page = objects::Model::list_page(db, &page_query(bucket_id, "", None, 10))
        .await
        .unwrap();

    // 'A'(0x41) 'B'(0x42) 'a'(0x61) 'b'(0x62)
    assert_eq!(
        page.iter()
            .map(|o| o.object_key.as_str())
            .collect::<Vec<_>>(),
        vec!["A", "B", "a", "b"]
    );
}

/// A wildcard in the prefix is literal, asserted again on the paging path because it is a separate query.
#[tokio::test]
#[serial]
async fn list_page_treats_wildcards_literally() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    let bucket_id = bucket(db).await;
    seed_keys(db, bucket_id, &["a_/one", "ab/two", "a%/three"]).await;

    let page = objects::Model::list_page(db, &page_query(bucket_id, "a_/", None, 10))
        .await
        .unwrap();

    assert_eq!(page.len(), 1);
    assert_eq!(page[0].object_key, "a_/one");
}

/// Paging inside a prefix must not walk out of it.
#[tokio::test]
#[serial]
async fn after_stays_inside_the_prefix() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    let bucket_id = bucket(db).await;
    seed_keys(db, bucket_id, &["img/a", "img/b", "zz/c"]).await;

    let page = objects::Model::list_page(db, &page_query(bucket_id, "img/", Some("img/a"), 10))
        .await
        .unwrap();

    assert_eq!(
        page.iter()
            .map(|o| o.object_key.as_str())
            .collect::<Vec<_>>(),
        vec!["img/b"],
        "paging must not walk past the prefix into zz/"
    );
}
