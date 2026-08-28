//! `GetObject` and `HeadObject` against a mock store.
use serial_test::serial;

use super::{canned, header, with_gateway};
use crate::support::mock_upstream::Canned;

/// Body and headers come from upstream; the `objects` row is metadata for listing and quota, not the source of truth for content.
#[tokio::test]
#[serial]
async fn get_streams_the_upstream_body_and_headers() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(Canned {
            status: 200,
            headers: vec![
                ("content-type".into(), "image/png".into()),
                ("etag".into(), "\"abc123\"".into()),
                (
                    "last-modified".into(),
                    "Mon, 17 Aug 2026 08:00:00 GMT".into(),
                ),
                ("x-amz-meta-owner".into(), "team-a".into()),
            ],
            body: b"png bytes".to_vec(),
        });

        let res = g.get(&signer, "/media-cdn/img/a.png").await;

        assert_eq!(res.status_code(), 200, "{}", res.text());
        assert_eq!(res.text(), "png bytes");
        assert_eq!(header(&res, "content-type"), "image/png");
        assert_eq!(header(&res, "etag"), "\"abc123\"");
        assert_eq!(header(&res, "x-amz-meta-owner"), "team-a");
        g.mock.assert_key(0, &g.physical("img/a.png"));
    })
    .await;
}

/// A header upstream sent that is not on the whitelist must not reach the client: some providers answer with debug headers that name the physical bucket.
#[tokio::test]
#[serial]
async fn an_unlisted_upstream_header_is_dropped() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(Canned {
            status: 200,
            headers: vec![
                ("content-type".into(), "image/png".into()),
                ("x-amz-id-2".into(), "osg-main/internal/debug/token".into()),
                ("x-minio-deployment-id".into(), "osg-main".into()),
            ],
            body: b"png".to_vec(),
        });

        let res = g.get(&signer, "/media-cdn/img/a.png").await;

        assert_eq!(res.status_code(), 200);
        assert!(header(&res, "x-amz-id-2").is_empty());
        assert!(header(&res, "x-minio-deployment-id").is_empty());
    })
    .await;
}

/// A range request is forwarded and its 206 comes back intact.
#[tokio::test]
#[serial]
async fn a_range_request_is_forwarded_and_206_comes_back() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(Canned {
            status: 206,
            headers: vec![("content-range".into(), "bytes 0-3/9".into())],
            body: b"png ".to_vec(),
        });

        let res = g
            .get_with(&signer, "/media-cdn/img/a.png", &[("range", "bytes=0-3")])
            .await;

        assert_eq!(res.status_code(), 206, "{}", res.text());
        assert_eq!(header(&res, "content-range"), "bytes 0-3/9");
        assert_eq!(g.mock.requests()[0].header("range"), "bytes=0-3");
    })
    .await;
}

/// Conditional headers are forwarded and a 304 comes back with no body.
#[tokio::test]
#[serial]
async fn conditional_headers_are_forwarded_and_304_comes_back() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(Canned {
            status: 304,
            headers: vec![],
            body: vec![],
        });

        let res = g
            .get_with(
                &signer,
                "/media-cdn/img/a.png",
                &[("if-none-match", "\"abc123\"")],
            )
            .await;

        assert_eq!(res.status_code(), 304);
        assert!(res.text().is_empty());
        assert_eq!(g.mock.requests()[0].header("if-none-match"), "\"abc123\"");
    })
    .await;
}

/// A client header that is not on the whitelist must not reach upstream: an unknown header can change store behaviour the gateway did not intend.
#[tokio::test]
#[serial]
async fn an_unlisted_client_header_is_not_forwarded() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(Canned::ok(b"png"));

        let res = g
            .get_with(
                &signer,
                "/media-cdn/img/a.png",
                &[("x-amz-server-side-encryption", "aws:kms")],
            )
            .await;

        assert_eq!(res.status_code(), 200, "{}", res.text());
        assert!(g.mock.requests()[0]
            .header("x-amz-server-side-encryption")
            .is_empty());
    })
    .await;
}

/// A missing key is `NoSuchKey`, and the upstream body that named the physical path is dropped.
#[tokio::test]
#[serial]
async fn a_missing_key_is_no_such_key_without_leaking_the_physical_path() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(canned(
            404,
            br"<Error><Code>NoSuchKey</Code><Message>The specified key does not exist.</Message><Key>osg-main/1111/media-cdn/gone.png</Key></Error>",
        ));

        let res = g.get(&signer, "/media-cdn/gone.png").await;

        assert_eq!(res.status_code(), 404);
        let body = res.text();
        assert!(body.contains("NoSuchKey"), "{body}");
        assert!(!body.contains("osg-main"), "physical bucket leaked: {body}");
        assert!(body.contains("<Resource>/media-cdn/gone.png</Resource>"), "{body}");
    })
    .await;
}

/// HEAD reports metadata and carries no body.
#[tokio::test]
#[serial]
async fn head_reports_metadata_without_a_body() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(Canned {
            status: 200,
            headers: vec![
                ("content-length".into(), "9".into()),
                ("etag".into(), "\"abc123\"".into()),
                ("content-type".into(), "image/png".into()),
            ],
            body: vec![],
        });

        let res = g.head(&signer, "/media-cdn/img/a.png").await;

        assert_eq!(res.status_code(), 200);
        assert_eq!(header(&res, "etag"), "\"abc123\"");
        assert_eq!(header(&res, "content-type"), "image/png");
        assert!(res.text().is_empty());
        assert_eq!(g.mock.requests()[0].method, "HEAD");
    })
    .await;
}

/// A key with characters that need encoding must reach the store as the same object the client named.
#[tokio::test]
#[serial]
async fn a_key_with_spaces_and_unicode_round_trips() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(Canned::ok(b"bytes"));

        let res = g.get(&signer, "/media-cdn/ảnh của tôi/a b.png").await;

        assert_eq!(res.status_code(), 200, "{}", res.text());
        // The mock decodes the path it received, so this compares logical keys.
        g.mock.assert_key(0, &g.physical("ảnh của tôi/a b.png"));
    })
    .await;
}

/// An unconfigured pool must fail loudly rather than send an unsigned request.
#[tokio::test]
#[serial]
async fn a_pool_without_credentials_fails_the_read() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.strip_pool_credentials().await;

        let res = g.get(&signer, "/media-cdn/img/a.png").await;

        assert_eq!(res.status_code(), 500);
        assert!(res.text().contains("InternalError"), "{}", res.text());
        g.mock.assert_untouched();
    })
    .await;
}
