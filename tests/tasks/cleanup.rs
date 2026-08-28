//! The two sweep tasks.
use loco_rs::{boot::run_task, task, testing::prelude::*};
use object_storage_gate::{
    app::App,
    models::{audit_logs, buckets, multipart_uploads, pools, users},
};
use sea_orm::{ActiveModelTrait, ActiveValue};
use serial_test::serial;

use crate::support::mock_upstream::MockUpstream;

async fn a_bucket(ctx: &loco_rs::app::AppContext, mock: &MockUpstream) -> buckets::Model {
    let pool = pools::Model::create(
        &ctx.db,
        &pools::CreateParams {
            // Not "main": the seeded fixture pool already owns that name, and it has no credentials anyway.
            name: "cleanup-pool".to_string(),
            provider: pools::PROVIDER_CUSTOM.to_string(),
            api_endpoint: Some(mock.base_url.clone()),
            physical_bucket: "osg-main".to_string(),
            access_id: Some("ID".to_string()),
            access_secret: Some("SECRET".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let user = users::Model::find_by_email(&ctx.db, "user1@example.com")
        .await
        .unwrap();
    buckets::Model::create(&ctx.db, user.id, pool.id, "media-cdn", 0)
        .await
        .unwrap()
}

async fn age(ctx: &loco_rs::app::AppContext, upload: multipart_uploads::Model, days: i64) {
    let mut am: multipart_uploads::ActiveModel = upload.into();
    am.created_at = ActiveValue::set((chrono::Utc::now() - chrono::Duration::days(days)).into());
    am.update(&ctx.db).await.unwrap();
}

/// An abandoned upload is aborted upstream and its quota given back.
#[tokio::test]
#[serial]
async fn cleanup_multipart_aborts_and_releases() {
    let boot = boot_test::<App>().await.unwrap();
    let ctx = boot.app_context.clone();
    seed::<App>(&ctx).await.unwrap();
    let mock = MockUpstream::start().await;
    let bucket = a_bucket(&ctx, &mock).await;

    let upload = multipart_uploads::Model::create(&ctx.db, bucket.id, "big.bin", "U1")
        .await
        .unwrap();
    multipart_uploads::Model::add_reserved(&ctx.db, upload.id, 500)
        .await
        .unwrap();
    object_storage_gate::models::quota::reserve(&ctx.db, bucket.id, 500)
        .await
        .unwrap();
    age(&ctx, upload, 10).await;

    run_task::<App>(
        &ctx,
        Some(&"cleanup_multipart".to_string()),
        &task::Vars::default(),
    )
    .await
    .unwrap();

    assert!(
        multipart_uploads::Model::list_for_bucket(&ctx.db, bucket.id, "")
            .await
            .unwrap()
            .is_empty()
    );
    let fresh =
        buckets::Model::find_by_user_and_name(&ctx.db, bucket.user_id.unwrap(), "media-cdn")
            .await
            .unwrap()
            .unwrap();
    assert_eq!(fresh.reserved_bytes, 0, "the hold was not released");
    assert_eq!(mock.requests().len(), 1, "the store was not told to abort");
    assert_eq!(mock.requests()[0].method, "DELETE");
}

/// A fresh upload is left alone: a sweep that aborted uploads in progress would break every large upload.
#[tokio::test]
#[serial]
async fn cleanup_multipart_leaves_fresh_uploads_alone() {
    let boot = boot_test::<App>().await.unwrap();
    let ctx = boot.app_context.clone();
    seed::<App>(&ctx).await.unwrap();
    let mock = MockUpstream::start().await;
    let bucket = a_bucket(&ctx, &mock).await;

    multipart_uploads::Model::create(&ctx.db, bucket.id, "big.bin", "U1")
        .await
        .unwrap();

    run_task::<App>(
        &ctx,
        Some(&"cleanup_multipart".to_string()),
        &task::Vars::default(),
    )
    .await
    .unwrap();

    assert_eq!(
        multipart_uploads::Model::list_for_bucket(&ctx.db, bucket.id, "")
            .await
            .unwrap()
            .len(),
        1
    );
    mock.assert_untouched();
}

/// One upload the gateway cannot abort must not stop the sweep, or a single bad row keeps every other abandoned upload holding quota forever.
#[tokio::test]
#[serial]
async fn one_failure_does_not_stop_the_sweep() {
    let boot = boot_test::<App>().await.unwrap();
    let ctx = boot.app_context.clone();
    seed::<App>(&ctx).await.unwrap();
    let mock = MockUpstream::start().await;
    let bucket = a_bucket(&ctx, &mock).await;

    // The first one the store refuses, the second it accepts.
    mock.push(crate::support::mock_upstream::Canned {
        status: 500,
        headers: Vec::new(),
        body: b"nope".to_vec(),
    });

    for key in ["a.bin", "b.bin"] {
        let up = multipart_uploads::Model::create(&ctx.db, bucket.id, key, "U")
            .await
            .unwrap();
        age(&ctx, up, 10).await;
    }

    run_task::<App>(
        &ctx,
        Some(&"cleanup_multipart".to_string()),
        &task::Vars::default(),
    )
    .await
    .unwrap();

    let left = multipart_uploads::Model::list_for_bucket(&ctx.db, bucket.id, "")
        .await
        .unwrap();
    assert_eq!(left.len(), 1, "the sweep stopped at the first failure");
    assert_eq!(
        mock.requests().len(),
        2,
        "the second upload was never attempted"
    );
}

/// Old audit goes, recent audit stays.
#[tokio::test]
#[serial]
async fn cleanup_audit_removes_only_what_is_past_retention() {
    let boot = boot_test::<App>().await.unwrap();
    let ctx = boot.app_context.clone();

    let entry = audit_logs::AuditEntry {
        user_id: None,
        access_key_id: None,
        bucket_id: None,
        object_key: None,
        action: audit_logs::ACTION_AUTH.to_string(),
        outcome: audit_logs::OUTCOME_DENIED.to_string(),
        status_code: 403,
        bytes: 0,
        duration_ms: 1,
        request_id: "req".to_string(),
        ip: "203.0.113.7".to_string(),
        user_agent: None,
    };
    let old = audit_logs::Model::record(&ctx.db, &entry).await.unwrap();
    audit_logs::Model::record(&ctx.db, &entry).await.unwrap();

    let mut am: audit_logs::ActiveModel = old.into();
    am.occurred_at = ActiveValue::set((chrono::Utc::now() - chrono::Duration::days(200)).into());
    am.update(&ctx.db).await.unwrap();

    run_task::<App>(
        &ctx,
        Some(&"cleanup_audit".to_string()),
        &task::Vars::default(),
    )
    .await
    .unwrap();

    let left = audit_logs::Model::list_recent(&ctx.db, 100).await.unwrap();
    assert_eq!(left.len(), 1, "retention removed the wrong rows");
}
