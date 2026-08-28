//! Every S3 request leaves a row behind.
use object_storage_gate::models::audit_logs;
use serial_test::serial;

use super::{etag_ok, with_gateway};
use crate::support::mock_upstream::Canned;

/// Drains the queue the way the worker would, so the assertions read the table.
async fn drain(g: &super::TestGateway) -> Vec<audit_logs::Model> {
    audit_logs::Model::list_recent(&g.ctx.db, 100)
        .await
        .unwrap()
}

/// A successful read records the account, the bucket, the key and the outcome.
#[tokio::test]
#[serial]
async fn a_successful_read_is_recorded() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(Canned::ok(b"png"));

        let res = g.get(&signer, "/media-cdn/img/a.png").await;
        assert_eq!(res.status_code(), 200, "{}", res.text());

        let rows = drain(&g).await;
        assert_eq!(rows.len(), 1, "expected one audit row, got {}", rows.len());
        let row = &rows[0];
        assert_eq!(row.action, audit_logs::ACTION_READ);
        assert_eq!(row.outcome, audit_logs::OUTCOME_OK);
        assert_eq!(row.status_code, 200);
        assert_eq!(row.object_key.as_deref(), Some("img/a.png"));
        assert_eq!(row.user_id, Some(g.user.id));
        assert_eq!(row.bucket_id, Some(g.bucket.id));
        assert_eq!(
            row.access_key_id.as_deref(),
            Some(signer.access_key_id.as_str())
        );
        assert!(!row.request_id.is_empty());
    })
    .await;
}

/// An unusable credential records the id the client presented, even though no such key exists — that is how key probing shows up.
#[tokio::test]
#[serial]
async fn a_probe_with_an_unknown_key_is_recorded_as_auth() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        let unknown = signer.with_id("OSGDOESNOTEXIST0000");

        let res = g.get(&unknown, "/media-cdn/img/a.png").await;
        assert_eq!(res.status_code(), 403);

        let rows = drain(&g).await;
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.action, audit_logs::ACTION_AUTH);
        assert_eq!(row.outcome, audit_logs::OUTCOME_DENIED);
        assert_eq!(row.user_id, None);
        assert_eq!(row.access_key_id.as_deref(), Some("OSGDOESNOTEXIST0000"));
    })
    .await;
}

/// A quota refusal is its own outcome, not just another 403: the same status covers a wrong signature and a full bucket, and those are different problems.
#[tokio::test]
#[serial]
async fn a_quota_refusal_has_its_own_outcome() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.set_bucket_quota("media-cdn", 1).await;

        let res = g.put(&signer, "/media-cdn/big.bin", &vec![0u8; 500]).await;
        assert_eq!(res.status_code(), 403, "{}", res.text());

        let rows = drain(&g).await;
        let row = rows.first().expect("an audit row");
        assert_eq!(row.outcome, audit_logs::OUTCOME_QUOTA);
        assert_eq!(row.status_code, 403);
        assert_eq!(row.action, audit_logs::ACTION_WRITE);
    })
    .await;
}

/// A refused authorisation is denied, not an auth failure: the key was real, the policy was not enough.
#[tokio::test]
#[serial]
async fn a_policy_refusal_keeps_the_action_and_the_user() {
    with_gateway(|g| async move {
        let signer = g.scoped_key("img/").await;

        let res = g.get(&signer, "/media-cdn/docs/a.pdf").await;
        assert_eq!(res.status_code(), 403);

        let rows = drain(&g).await;
        let row = rows.first().expect("an audit row");
        assert_eq!(row.action, audit_logs::ACTION_READ);
        assert_eq!(row.outcome, audit_logs::OUTCOME_DENIED);
        assert_eq!(row.user_id, Some(g.user.id));
    })
    .await;
}

/// A missing object is `not_found`, which is what makes "how often do clients ask for things that are not there" answerable.
#[tokio::test]
#[serial]
async fn a_missing_key_is_recorded_as_not_found() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock
            .push(super::canned(404, b"<Error><Code>NoSuchKey</Code></Error>"));

        g.get(&signer, "/media-cdn/gone.png").await;

        let rows = drain(&g).await;
        let row = rows.first().expect("an audit row");
        assert_eq!(row.outcome, audit_logs::OUTCOME_NOT_FOUND);
        assert_eq!(row.status_code, 404);
    })
    .await;
}

/// A write records what it wrote, so byte accounting can be reconstructed from the log alone.
#[tokio::test]
#[serial]
async fn a_write_is_recorded_with_its_action() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(etag_ok("\"e\""));

        g.put(&signer, "/media-cdn/img/a.png", b"png bytes").await;

        let rows = drain(&g).await;
        let row = rows.first().expect("an audit row");
        assert_eq!(row.action, audit_logs::ACTION_WRITE);
        assert_eq!(row.outcome, audit_logs::OUTCOME_OK);
        assert_eq!(row.object_key.as_deref(), Some("img/a.png"));
    })
    .await;
}

/// A browser navigation is not an S3 request and must not fill the audit log with page loads.
#[tokio::test]
#[serial]
async fn a_console_request_is_not_audited() {
    with_gateway(|g| async move {
        // Accept: text/html is what a navigation actually sends, and it is what tells the gateway this is not an S3 request.
        let navigation = [("accept".to_string(), "text/html".to_string())];
        g.raw_get("/buckets/media-cdn", &navigation).await;
        g.raw_get("/static/js/app.js", &[]).await;

        assert!(drain(&g).await.is_empty());
    })
    .await;
}

/// The physical layout must not reach the audit log either — it is read by admins, but it is still the layout the product promises to hide.
#[tokio::test]
#[serial]
async fn audit_records_the_logical_key_not_the_physical_one() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(Canned::ok(b"png"));

        g.get(&signer, "/media-cdn/img/a.png").await;

        let rows = drain(&g).await;
        let key = rows[0].object_key.clone().unwrap_or_default();
        assert_eq!(key, "img/a.png");
        assert!(!key.contains("osg-main"));
        assert!(!key.contains(&g.user.pid.to_string()));
    })
    .await;
}

/// A broken audit sink must not turn a good request into a 500.
///
/// The table is dropped mid-test, which is the bluntest possible version of "the place audit goes is unavailable".
/// If the request path treated that as fatal, an outage in a logging dependency would take the whole data plane with it.
#[tokio::test]
#[serial]
async fn a_broken_audit_sink_does_not_fail_the_request() {
    with_gateway(|g| async move {
        use sea_orm::ConnectionTrait;

        let signer = g.full_key().await;
        g.mock.push(Canned::ok(b"png"));

        g.ctx
            .db
            .execute_unprepared("DROP TABLE audit_logs")
            .await
            .unwrap();

        let res = g.get(&signer, "/media-cdn/img/a.png").await;

        assert_eq!(res.status_code(), 200, "{}", res.text());
        assert_eq!(res.text(), "png");
    })
    .await;
}
