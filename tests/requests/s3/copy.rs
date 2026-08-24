//! `CopyObject`: both ends of a copy go through the same resolver.
use serial_test::serial;

use super::{etag_ok, with_gateway};

/// The happy path, and the header rewrite that makes it work.
#[tokio::test]
#[serial]
async fn copy_rewrites_both_ends_and_charges_the_destination() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.seed_objects("media-cdn", &["img/a.png"]).await;
        g.mock.push(etag_ok("\"copied\""));

        let res = g
            .put_with(
                &signer,
                "/media-cdn/img/b.png",
                b"",
                &[("x-amz-copy-source", "/media-cdn/img/a.png")],
            )
            .await;

        assert_eq!(res.status_code(), 200, "{}", res.text());
        assert!(res.text().contains("<CopyObjectResult"), "{}", res.text());

        // The destination is the physical key.
        g.mock.assert_key(0, &g.physical("img/b.png"));
        // And the source header was rewritten to the physical source, not forwarded as the client wrote it.
        let sent = g.mock.requests()[0].header("x-amz-copy-source");
        assert_eq!(
            sent,
            format!(
                "/osg-main/{}",
                g.physical("img/a.png").trim_start_matches("osg-main/")
            )
        );
        assert!(
            sent.contains(&g.user.pid.to_string()),
            "source was not rewritten: {sent}"
        );

        let row = g.object_row("media-cdn", "img/b.png").await.unwrap();
        assert_eq!(row.size, 1);
        assert_eq!(row.etag, "\"copied\"");
        assert_eq!(g.bucket_row("media-cdn").await.object_count, 2);
    })
    .await;
}

/// A source the key may not read is refused, and nothing reaches the store.
#[tokio::test]
#[serial]
async fn a_source_outside_the_prefix_is_refused() {
    with_gateway(|g| async move {
        let signer = g.key_with(&["read", "write", "list"], &["img/"]).await;
        g.seed_objects("media-cdn", &["docs/secret.pdf"]).await;

        let res = g
            .put_with(
                &signer,
                "/media-cdn/img/stolen.pdf",
                b"",
                &[("x-amz-copy-source", "/media-cdn/docs/secret.pdf")],
            )
            .await;

        assert_eq!(res.status_code(), 403, "{}", res.text());
        g.mock.assert_untouched();
    })
    .await;
}

/// Another user's bucket is not a valid source: `resolve_copy_source` scopes to the caller's account.
#[tokio::test]
#[serial]
async fn another_users_bucket_is_not_a_valid_source() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.other_user_bucket("theirs").await;

        let res = g
            .put_with(
                &signer,
                "/media-cdn/copied.png",
                b"",
                &[("x-amz-copy-source", "/theirs/a.png")],
            )
            .await;

        assert_eq!(res.status_code(), 404, "{}", res.text());
        assert!(res.text().contains("NoSuchBucket"));
        g.mock.assert_untouched();
    })
    .await;
}

/// A missing source is `NoSuchKey`, before any reservation is taken.
#[tokio::test]
#[serial]
async fn a_missing_source_is_no_such_key() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;

        let res = g
            .put_with(
                &signer,
                "/media-cdn/copied.png",
                b"",
                &[("x-amz-copy-source", "/media-cdn/gone.png")],
            )
            .await;

        assert_eq!(res.status_code(), 404, "{}", res.text());
        assert!(res.text().contains("NoSuchKey"));
        assert_eq!(g.bucket_row("media-cdn").await.reserved_bytes, 0);
        g.mock.assert_untouched();
    })
    .await;
}

/// A copy that would cross the quota is refused before the store is asked.
#[tokio::test]
#[serial]
async fn an_over_quota_copy_never_reaches_the_store() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.seed_objects("media-cdn", &["img/a.png"]).await;
        g.set_bucket_quota("media-cdn", 1).await;

        let res = g
            .put_with(
                &signer,
                "/media-cdn/img/b.png",
                b"",
                &[("x-amz-copy-source", "/media-cdn/img/a.png")],
            )
            .await;

        assert_eq!(res.status_code(), 403, "{}", res.text());
        assert!(res.text().contains("QuotaExceeded"));
        g.mock.assert_untouched();
    })
    .await;
}

/// A malformed copy source is an argument error, not a 500.
#[tokio::test]
#[serial]
async fn a_malformed_copy_source_is_invalid_argument() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;

        for bad in ["not-a-path", "/", "/onlybucket"] {
            let res = g
                .put_with(
                    &signer,
                    "/media-cdn/copied.png",
                    b"",
                    &[("x-amz-copy-source", bad)],
                )
                .await;
            assert_eq!(res.status_code(), 400, "source {bad:?}: {}", res.text());
            assert!(res.text().contains("InvalidArgument"), "{}", res.text());
        }
        g.mock.assert_untouched();
    })
    .await;
}

/// A versionId suffix is parsed off rather than treated as part of the key.
#[tokio::test]
#[serial]
async fn a_version_id_suffix_is_ignored_not_treated_as_key() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.seed_objects("media-cdn", &["img/a.png"]).await;
        g.mock.push(etag_ok("\"copied\""));

        let res = g
            .put_with(
                &signer,
                "/media-cdn/img/b.png",
                b"",
                &[("x-amz-copy-source", "/media-cdn/img/a.png?versionId=null")],
            )
            .await;

        assert_eq!(res.status_code(), 200, "{}", res.text());
    })
    .await;
}
