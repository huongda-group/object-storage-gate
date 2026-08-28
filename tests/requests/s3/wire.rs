//! S3 wire shape: error XML, request ids, and the dispatch table's unimplemented branches.
use serial_test::serial;

use super::{canned, header, with_gateway};
use crate::support::mock_upstream::Canned;

#[tokio::test]
#[serial]
async fn an_error_is_s3_shaped_xml() {
    with_gateway(|g| async move {
        let res = g.unauthenticated_get("/media-cdn/a.png").await;

        assert_eq!(res.status_code(), 403, "{}", res.text());
        assert_eq!(header(&res, "content-type"), "application/xml");

        let body = res.text();
        assert!(body.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(body.contains("<Error>"));
        assert!(body.contains("<Code>AccessDenied</Code>"));
        assert!(body.contains("<Message>"));
        assert!(body.contains("<Resource>/media-cdn/a.png</Resource>"));
        assert!(body.contains("<RequestId>"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn every_response_carries_an_amz_request_id() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(Canned::ok(b"bytes"));

        let ok = g.get(&signer, "/media-cdn/a.png").await;
        assert_eq!(ok.status_code(), 200, "{}", ok.text());
        assert!(!header(&ok, "x-amz-request-id").is_empty());

        let err = g.unauthenticated_get("/media-cdn/a.png").await;
        assert!(!header(&err, "x-amz-request-id").is_empty());
    })
    .await;
}

/// HEAD never has a body — botocore reads Content-Length and would mis-parse or hang otherwise.
#[tokio::test]
#[serial]
async fn a_head_error_has_no_body() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock
            .push(canned(404, b"<Error><Code>NoSuchKey</Code></Error>"));

        let res = g.head(&signer, "/media-cdn/missing.png").await;

        assert_eq!(res.status_code(), 404);
        assert!(res.text().is_empty(), "HEAD must not carry a body");
        assert!(!header(&res, "x-amz-request-id").is_empty());
    })
    .await;
}

/// A missing branch in the dispatch table is a verb that silently does the wrong thing.
#[tokio::test]
#[serial]
async fn unimplemented_shapes_say_so() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;

        // ListObjects V1 — no list-type param.
        let res = g.get(&signer, "/media-cdn").await;
        assert_eq!(res.status_code(), 501, "{}", res.text());
        assert!(res.text().contains("NotImplemented"));
        assert!(res.text().contains("list-type=2"));

        // CreateBucket over S3.
        let res = g.put(&signer, "/new-bucket", b"").await;
        assert_eq!(res.status_code(), 501);
        assert!(res.text().contains("console"));

        g.mock.assert_untouched();
    })
    .await;
}

/// aws-chunked signing is out of scope; it must say so rather than store the framing as object bytes.
#[tokio::test]
#[serial]
async fn aws_chunked_payload_is_not_implemented() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;

        let res = g
            .put_with_payload_hash(
                &signer,
                "/media-cdn/a.bin",
                "STREAMING-AWS4-HMAC-SHA256-PAYLOAD",
                b"chunked framing",
            )
            .await;

        assert_eq!(res.status_code(), 501, "{}", res.text());
        assert!(res.text().contains("aws-chunked"), "{}", res.text());
        g.mock.assert_untouched();
    })
    .await;
}

/// The S3 catch-all must not shadow the management API or the console.
///
/// This failure shows up as a blank console, which does not point at routing — hence a test.
#[tokio::test]
#[serial]
async fn the_s3_catch_all_does_not_shadow_the_management_api() {
    with_gateway(|g| async move {
        // Unauthenticated /api/keys answers from the management API, not with S3 error XML.
        let res = g.raw_get("/api/keys", &[]).await;
        assert!(!res.text().contains("<Error><Code>"), "{}", res.text());
        assert!(
            matches!(res.status_code().as_u16(), 401 | 403),
            "got {}",
            res.status_code()
        );

        // The console still owns `/` for an unsigned request.
        // ListBuckets is routed there too, but S3 has no anonymous ListBuckets, so credentials are what tells the two apart.
        let res = g.raw_get("/", &[]).await;
        assert_eq!(res.status_code(), 200);
        assert!(!res.text().contains("<Error><Code>"), "{}", res.text());

        // And the pool listing is still the management route.
        let res = g.raw_get("/api/pools", &[]).await;
        assert!(!res.text().contains("<Error>"));

        // An /api path with no route at all must not be answered by the S3 tree either.
        let res = g.raw_get("/api/does/not/exist", &[]).await;
        assert!(!res.text().contains("<Error><Code>"), "{}", res.text());

        // A console deep link is a browser navigation, not an object request, and must reach the SPA.
        let res = g
            .raw_get(
                "/buckets/media-cdn",
                &[("accept".to_string(), "text/html".to_string())],
            )
            .await;
        assert_eq!(res.status_code(), 200);
        assert!(!res.text().contains("<Error><Code>"), "{}", res.text());

        // So must a bundled asset, which is two segments deep and looks exactly like an object path.
        let res = g.raw_get("/static/js/does-not-exist.js", &[]).await;
        assert!(!res.text().contains("<Error><Code>"), "{}", res.text());
    })
    .await;
}
