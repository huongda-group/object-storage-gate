//! Presigned URLs: the query form of `SigV4`, with no credentials on the wire.
use serial_test::serial;

use super::with_gateway;
use crate::support::mock_upstream::Canned;

/// The point of a presigned URL: it works with no Authorization header at all.
#[tokio::test]
#[serial]
async fn a_presigned_url_reads_without_credentials() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(Canned::ok(b"png bytes"));

        let res = g
            .fetch_presigned(&signer, "/media-cdn/img/a.png", 3600)
            .await;

        assert_eq!(res.status_code(), 200, "{}", res.text());
        assert_eq!(res.text(), "png bytes");
        g.mock.assert_key(0, &g.physical("img/a.png"));
    })
    .await;
}

/// An expired link is `AccessDenied`, not a clock error: fixing a clock and retrying would fail again.
#[tokio::test]
#[serial]
async fn an_expired_presigned_url_is_refused() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        let long_ago = chrono::Utc::now() - chrono::Duration::minutes(30);

        let res = g
            .fetch_presigned_at(&signer, "/media-cdn/img/a.png", 60, long_ago)
            .await;

        assert_eq!(res.status_code(), 403, "{}", res.text());
        assert!(res.text().contains("AccessDenied"), "{}", res.text());
        g.mock.assert_untouched();
    })
    .await;
}

/// A link whose start time has not arrived is refused too, or a fast clock mints one good for twice its stated life.
#[tokio::test]
#[serial]
async fn a_presigned_url_from_the_future_is_refused() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        let later = chrono::Utc::now() + chrono::Duration::minutes(10);

        let res = g
            .fetch_presigned_at(&signer, "/media-cdn/img/a.png", 60, later)
            .await;

        assert_eq!(res.status_code(), 403, "{}", res.text());
        g.mock.assert_untouched();
    })
    .await;
}

/// A presigned URL is bound to the key it was minted for.
#[tokio::test]
#[serial]
async fn a_presigned_url_cannot_be_pointed_at_another_key() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        let query = signer.presign("GET", "/media-cdn/img/a.png", 3600);

        let res = g.raw_get_query("/media-cdn/img/secret.png", &query).await;

        assert_eq!(res.status_code(), 403, "{}", res.text());
        assert!(
            res.text().contains("SignatureDoesNotMatch"),
            "{}",
            res.text()
        );
        g.mock.assert_untouched();
    })
    .await;
}

/// The prefix scope applies to a presigned URL like any other request.
#[tokio::test]
#[serial]
async fn a_scoped_key_cannot_presign_outside_its_prefix() {
    with_gateway(|g| async move {
        let signer = g.scoped_key("img/").await;

        let res = g
            .fetch_presigned(&signer, "/media-cdn/docs/secret.pdf", 3600)
            .await;

        assert_eq!(res.status_code(), 403, "{}", res.text());
        g.mock.assert_untouched();
    })
    .await;
}

/// A tampered signature is refused rather than treated as unsigned.
#[tokio::test]
#[serial]
async fn a_tampered_presigned_signature_is_refused() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        let query = signer.presign("GET", "/media-cdn/img/a.png", 3600);
        let tampered = query
            .rsplit_once("X-Amz-Signature=")
            .map(|(head, _)| format!("{head}X-Amz-Signature={}", "0".repeat(64)))
            .unwrap();

        let res = g.raw_get_query("/media-cdn/img/a.png", &tampered).await;

        assert_eq!(res.status_code(), 403, "{}", res.text());
        assert!(
            res.text().contains("SignatureDoesNotMatch"),
            "{}",
            res.text()
        );
        g.mock.assert_untouched();
    })
    .await;
}
