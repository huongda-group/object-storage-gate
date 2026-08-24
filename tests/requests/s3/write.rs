//! The write path: `PutObject`, `DeleteObject`, `DeleteObjects`.
use serial_test::serial;

use super::{canned, etag_ok, header, with_gateway};

/// The happy path, and the physical key under test.
#[tokio::test]
#[serial]
async fn put_uploads_and_records_metadata() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(etag_ok("\"abc123\""));

        let res = g.put(&signer, "/media-cdn/img/a.png", b"png bytes").await;

        assert_eq!(res.status_code(), 200, "{}", res.text());
        assert_eq!(header(&res, "etag"), "\"abc123\"");
        g.mock.assert_key(0, &g.physical("img/a.png"));
        assert_eq!(g.mock.requests()[0].body, b"png bytes");

        // Metadata records the upstream ETag verbatim, not a recomputed one.
        let row = g.object_row("media-cdn", "img/a.png").await.unwrap();
        assert_eq!(row.etag, "\"abc123\"");
        assert_eq!(row.size, 9);

        let b = g.bucket_row("media-cdn").await;
        assert_eq!(b.used_bytes, 9);
        assert_eq!(b.reserved_bytes, 0);
        assert_eq!(b.object_count, 1);
    })
    .await;
}

/// The whole point of reserve-before-upload: an over-quota write must not move a byte.
#[tokio::test]
#[serial]
async fn an_over_quota_put_never_reaches_upstream() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.set_bucket_quota("media-cdn", 100).await;

        let res = g.put(&signer, "/media-cdn/big.bin", &vec![0u8; 500]).await;

        assert_eq!(res.status_code(), 403, "{}", res.text());
        assert!(res.text().contains("QuotaExceeded"), "{}", res.text());
        g.mock.assert_untouched();
        assert_eq!(g.bucket_row("media-cdn").await.reserved_bytes, 0);
        assert!(g.object_row("media-cdn", "big.bin").await.is_none());
    })
    .await;
}

/// A failed upload releases the hold; a leaked hold is a bucket that slowly refuses writes.
#[tokio::test]
#[serial]
async fn a_failed_upload_releases_the_reservation() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(canned(503, b"upstream unavailable"));

        let res = g.put(&signer, "/media-cdn/a.bin", b"bytes").await;

        assert_eq!(res.status_code(), 500, "{}", res.text());
        let b = g.bucket_row("media-cdn").await;
        assert_eq!(b.reserved_bytes, 0, "the hold leaked");
        assert_eq!(b.used_bytes, 0);
        assert!(g.object_row("media-cdn", "a.bin").await.is_none());
    })
    .await;
}

/// Content type and user metadata reach the store.
#[tokio::test]
#[serial]
async fn content_type_and_user_metadata_reach_upstream() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(etag_ok("\"e\""));

        let res = g
            .put_with(
                &signer,
                "/media-cdn/a.png",
                b"bytes",
                &[
                    ("content-type", "image/png"),
                    ("x-amz-meta-owner", "team-a"),
                    ("cache-control", "max-age=3600"),
                ],
            )
            .await;
        assert_eq!(res.status_code(), 200, "{}", res.text());

        let seen = &g.mock.requests()[0];
        assert_eq!(seen.header("content-type"), "image/png");
        assert_eq!(seen.header("x-amz-meta-owner"), "team-a");
        assert_eq!(seen.header("cache-control"), "max-age=3600");
        assert_eq!(
            g.object_row("media-cdn", "a.png")
                .await
                .unwrap()
                .content_type,
            "image/png"
        );
    })
    .await;
}

/// A header that changes how the store treats the object must not be forwarded.
///
/// `x-amz-acl` is the one that matters: passing it through would let a client make their object
/// public inside the shared physical bucket, over a path the gateway knows nothing about.
#[tokio::test]
#[serial]
async fn acl_and_encryption_headers_are_not_forwarded() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(etag_ok("\"e\""));

        g.put_with(
            &signer,
            "/media-cdn/a.png",
            b"bytes",
            &[
                ("x-amz-acl", "public-read"),
                ("x-amz-server-side-encryption", "aws:kms"),
                ("x-amz-storage-class", "GLACIER"),
            ],
        )
        .await;

        let seen = &g.mock.requests()[0];
        assert!(seen.header("x-amz-acl").is_empty(), "ACL was forwarded");
        assert!(seen.header("x-amz-server-side-encryption").is_empty());
        assert!(seen.header("x-amz-storage-class").is_empty());
    })
    .await;
}

/// An overwrite keeps one row and charges the difference.
#[tokio::test]
#[serial]
async fn an_overwrite_keeps_one_row_and_charges_the_delta() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(etag_ok("\"e1\""));
        g.mock.push(etag_ok("\"e2\""));

        g.put(&signer, "/media-cdn/a.bin", &vec![0u8; 300]).await;
        g.put(&signer, "/media-cdn/a.bin", &vec![0u8; 500]).await;

        let b = g.bucket_row("media-cdn").await;
        assert_eq!(b.used_bytes, 500);
        assert_eq!(b.object_count, 1);
        assert_eq!(b.reserved_bytes, 0);
        assert_eq!(
            g.object_row("media-cdn", "a.bin").await.unwrap().etag,
            "\"e2\""
        );
    })
    .await;
}

/// Delete is idempotent and credits the quota.
#[tokio::test]
#[serial]
async fn delete_is_idempotent_and_credits_the_quota() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(etag_ok("\"e1\""));
        g.put(&signer, "/media-cdn/a.bin", &vec![0u8; 300]).await;
        assert_eq!(g.bucket_row("media-cdn").await.used_bytes, 300);

        g.mock.push(canned(204, b""));
        let res = g.delete(&signer, "/media-cdn/a.bin").await;
        assert_eq!(res.status_code(), 204, "{}", res.text());
        assert!(res.text().is_empty());

        let b = g.bucket_row("media-cdn").await;
        assert_eq!(b.used_bytes, 0);
        assert_eq!(b.object_count, 0);

        // Again: still 204, and the counters do not go negative.
        g.mock.push(canned(204, b""));
        let res = g.delete(&signer, "/media-cdn/a.bin").await;
        assert_eq!(res.status_code(), 204);
        let b = g.bucket_row("media-cdn").await;
        assert_eq!(b.used_bytes, 0);
        assert_eq!(b.object_count, 0);
    })
    .await;
}

/// A batch delete reports every key it removed.
#[tokio::test]
#[serial]
async fn delete_objects_reports_every_key() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        for _ in 0..3 {
            g.mock.push(etag_ok("\"e\""));
        }
        for k in ["a.bin", "b.bin", "c.bin"] {
            g.put(&signer, &format!("/media-cdn/{k}"), b"x").await;
        }

        g.mock.push(canned(200, b"<DeleteResult/>"));
        let res = g
            .post_delete(&signer, "/media-cdn", &["a.bin", "b.bin", "c.bin"], false)
            .await;

        assert_eq!(res.status_code(), 200, "{}", res.text());
        let body = res.text();
        for k in ["a.bin", "b.bin", "c.bin"] {
            assert!(
                body.contains(&format!("<Key>{k}</Key>")),
                "missing {k}: {body}"
            );
        }
        assert!(body.contains("<Deleted>"));
        assert_eq!(g.bucket_row("media-cdn").await.object_count, 0);
        assert_eq!(g.bucket_row("media-cdn").await.used_bytes, 0);
    })
    .await;
}

/// Quiet mode omits the Deleted list, which is the only difference S3 defines.
#[tokio::test]
#[serial]
async fn delete_objects_quiet_omits_the_deleted_list() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(etag_ok("\"e\""));
        g.put(&signer, "/media-cdn/a.bin", b"x").await;

        g.mock.push(canned(200, b"<DeleteResult/>"));
        let res = g.post_delete(&signer, "/media-cdn", &["a.bin"], true).await;

        assert_eq!(res.status_code(), 200, "{}", res.text());
        assert!(!res.text().contains("<Deleted>"), "{}", res.text());
        assert_eq!(g.bucket_row("media-cdn").await.object_count, 0);
    })
    .await;
}

/// A key outside the prefix becomes one `<Error>` entry, not a 403 for the whole batch — that is S3's batch semantics, and a whole-request refusal would make one bad key undo 999 good ones.
#[tokio::test]
#[serial]
async fn a_key_outside_the_prefix_becomes_an_error_entry() {
    with_gateway(|g| async move {
        let signer = g
            .key_with(&["read", "write", "delete", "list"], &["img/"])
            .await;
        g.mock.push(etag_ok("\"e\""));
        g.put(&signer, "/media-cdn/img/a.png", b"x").await;

        g.mock.push(canned(200, b"<DeleteResult/>"));
        let res = g
            .post_delete(&signer, "/media-cdn", &["img/a.png", "docs/b.pdf"], false)
            .await;

        assert_eq!(res.status_code(), 200, "{}", res.text());
        let body = res.text();
        assert!(body.contains("<Deleted>"), "{body}");
        assert!(body.contains("<Key>img/a.png</Key>"), "{body}");
        assert!(body.contains("<Error>"), "{body}");
        assert!(body.contains("<Code>AccessDenied</Code>"), "{body}");
        assert!(body.contains("<Key>docs/b.pdf</Key>"), "{body}");

        // The denied key must not appear in the upstream request at all. An implementation that authorises and then sends the whole list passes every response assertion while still deleting data out of scope.
        let sent = String::from_utf8_lossy(&g.mock.requests().last().unwrap().body).to_string();
        assert!(
            !sent.contains("docs/b.pdf"),
            "denied key was sent upstream: {sent}"
        );
    })
    .await;
}

/// S3 caps a batch at 1000 keys.
#[tokio::test]
#[serial]
async fn more_than_a_thousand_keys_is_malformed_xml() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        let keys: Vec<String> = (0..1001).map(|i| format!("k{i}")).collect();
        let refs: Vec<&str> = keys.iter().map(String::as_str).collect();

        let res = g.post_delete(&signer, "/media-cdn", &refs, false).await;

        assert_eq!(res.status_code(), 400, "{}", res.text());
        assert!(res.text().contains("MalformedXML"), "{}", res.text());
        g.mock.assert_untouched();
    })
    .await;
}

/// A batch delete needs the delete action, like any other delete.
#[tokio::test]
#[serial]
async fn a_read_only_key_cannot_batch_delete() {
    with_gateway(|g| async move {
        let signer = g.key_with(&["read", "list"], &[]).await;

        let res = g
            .post_delete(&signer, "/media-cdn", &["a.bin"], false)
            .await;

        assert_eq!(res.status_code(), 403, "{}", res.text());
        g.mock.assert_untouched();
    })
    .await;
}
