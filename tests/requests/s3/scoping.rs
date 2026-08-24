//! The isolation boundary.
//!
//! Mirrors what `tests/s3/test_scoping.py` will assert, but against a mock upstream — so each test can assert the stronger property: the store was never touched.
use serial_test::serial;

use super::{header, with_gateway};
use crate::support::mock_upstream::Canned;

/// A scoped key reading inside its folder reaches upstream with the rewritten key.
#[tokio::test]
#[serial]
async fn a_scoped_key_reads_inside_its_folder() {
    with_gateway(|g| async move {
        let signer = g.scoped_key("img/").await;
        g.mock.push(Canned::ok(b"png bytes"));

        let res = g.get(&signer, "/media-cdn/img/a.png").await;

        assert_eq!(res.status_code(), 200, "{}", res.text());
        g.mock.assert_key(0, &g.physical("img/a.png"));
    })
    .await;
}

/// Four verbs, all outside the prefix, none of which may reach the store.
///
/// The assertion that matters is `assert_untouched`, not the 403: a gateway that calls upstream and only then refuses has already let the request cross the boundary, and a status-only assertion cannot tell the two apart.
#[tokio::test]
#[serial]
async fn a_scoped_key_cannot_touch_anything_outside() {
    with_gateway(|g| async move {
        let signer = g
            .key_with(&["read", "write", "delete", "list"], &["img/"])
            .await;

        for method in ["GET", "HEAD", "PUT", "DELETE"] {
            let res = g
                .request(&signer, method, "/media-cdn/docs/a.pdf", b"", &[])
                .await;
            assert_eq!(res.status_code(), 403, "{method} /media-cdn/docs/a.pdf");
            if method != "HEAD" {
                assert!(
                    res.text().contains("AccessDenied"),
                    "{method}: {}",
                    res.text()
                );
            }
        }

        g.mock.assert_untouched();
    })
    .await;
}

/// The separator rule, end to end: a key scoped to `img` must not reach `imgsecret/`.
#[tokio::test]
#[serial]
async fn a_key_scoped_to_img_cannot_read_imgsecret() {
    with_gateway(|g| async move {
        let signer = g.scoped_key("img").await;

        let res = g.get(&signer, "/media-cdn/imgsecret/a.png").await;

        assert_eq!(res.status_code(), 403);
        g.mock.assert_untouched();
    })
    .await;
}

/// Another user's bucket reads as absent, not as forbidden — the same posture the management API takes, and it does not confirm the bucket exists.
#[tokio::test]
#[serial]
async fn another_users_bucket_is_no_such_bucket() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.other_user_bucket("their-private").await;

        let res = g.get(&signer, "/their-private/a.png").await;

        assert_eq!(res.status_code(), 404);
        assert!(res.text().contains("NoSuchBucket"));
        g.mock.assert_untouched();
    })
    .await;
}

/// Traversal cannot be delivered over HTTP at all, which is worth recording rather than assuming.
///
/// Both the literal `/media-cdn/../other/a.png` and the percent-encoded `/media-cdn/%2E%2E/other/a.png` are decoded and collapsed in transit, so the handler sees `/other/a.png` and the request fails as a signature mismatch — the client signed a path the server never received.
/// That means this test cannot exercise `validate_logical_key`; the unit tests in `src/s3/request.rs` do that, and this one asserts the property that still matters here: nothing reaches the store.
#[tokio::test]
#[serial]
async fn traversal_is_collapsed_in_transit_and_never_reaches_the_store() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;

        for path in ["/media-cdn/../other/a.png", "/media-cdn/img/../../escape"] {
            let res = g.get(&signer, path).await;
            assert_eq!(res.status_code(), 403, "path {path}: {}", res.text());
            assert!(
                res.text().contains("SignatureDoesNotMatch"),
                "{}",
                res.text()
            );
        }

        for encoded in [
            "/media-cdn/%2E%2E/other/a.png",
            "/media-cdn/img/%2E%2E/%2E%2E/escape",
        ] {
            let res = g.request_encoded(&signer, "GET", encoded, b"", &[]).await;
            assert_eq!(res.status_code(), 403, "path {encoded}: {}", res.text());
        }

        g.mock.assert_untouched();
    })
    .await;
}

/// A key missing the action is refused even inside its prefix.
#[tokio::test]
#[serial]
async fn a_read_only_key_cannot_write_inside_its_prefix() {
    with_gateway(|g| async move {
        let signer = g.key_with(&["read"], &["img/"]).await;

        let res = g.put(&signer, "/media-cdn/img/a.png", b"bytes").await;

        assert_eq!(res.status_code(), 403, "{}", res.text());
        g.mock.assert_untouched();
    })
    .await;
}

/// Every auth failure shape, and the code each one must produce.
#[tokio::test]
#[serial]
async fn auth_failures_map_to_the_right_codes() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;

        // Credentials presented but unusable. A request with no SigV4 credentials at all is a browser navigation and reaches the console instead — see `unauthenticated_get`.
        let res = g.unauthenticated_get("/media-cdn/a.png").await;
        assert_eq!(res.status_code(), 403, "{}", res.text());
        assert!(res.text().contains("AccessDenied"), "{}", res.text());

        // A key id nobody issued.
        let unknown = signer.with_id("OSGDOESNOTEXIST0000");
        let res = g.get(&unknown, "/media-cdn/a.png").await;
        assert!(res.text().contains("InvalidAccessKeyId"), "{}", res.text());

        // The right key, the wrong secret.
        let wrong = signer.with_secret("not-the-secret");
        let res = g.get(&wrong, "/media-cdn/a.png").await;
        assert!(
            res.text().contains("SignatureDoesNotMatch"),
            "{}",
            res.text()
        );

        // A well-formed signature that does not match the request.
        let res = g.get_tampered(&signer, "/media-cdn/a.png").await;
        assert!(
            res.text().contains("SignatureDoesNotMatch"),
            "{}",
            res.text()
        );

        // A clock far outside the window.
        let long_ago = chrono::Utc::now() - chrono::Duration::hours(3);
        let res = g.get_at(&signer, "/media-cdn/a.png", long_ago).await;
        assert!(
            res.text().contains("RequestTimeTooSkewed"),
            "{}",
            res.text()
        );

        // A revoked key reads as unknown, not as denied: one answer for every unusable state.
        g.revoke_key(&signer).await;
        let res = g.get(&signer, "/media-cdn/a.png").await;
        assert!(res.text().contains("InvalidAccessKeyId"), "{}", res.text());

        g.mock.assert_untouched();
    })
    .await;
}

/// A bucket that does not exist at all, for the owner's own account.
#[tokio::test]
#[serial]
async fn an_unknown_bucket_is_no_such_bucket() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;

        let res = g.get(&signer, "/no-such-bucket/a.png").await;

        assert_eq!(res.status_code(), 404);
        assert!(res.text().contains("NoSuchBucket"));
        g.mock.assert_untouched();
    })
    .await;
}

/// The physical layout must not appear in any response the client can read.
#[tokio::test]
#[serial]
async fn a_refusal_never_names_the_physical_bucket() {
    with_gateway(|g| async move {
        let signer = g.scoped_key("img/").await;

        let res = g.get(&signer, "/media-cdn/docs/a.pdf").await;
        let body = res.text();

        assert!(!body.contains("osg-main"), "physical bucket leaked: {body}");
        assert!(
            !body.contains(&g.user.pid.to_string()),
            "user pid leaked: {body}"
        );
        assert!(!header(&res, "x-amz-request-id").is_empty());
    })
    .await;
}
