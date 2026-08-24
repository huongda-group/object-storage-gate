//! Listing, served from the database and never from the store.
use serial_test::serial;

use super::with_gateway;

fn keys_in(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(i) = rest.find("<Key>") {
        rest = &rest[i + 5..];
        let Some(j) = rest.find("</Key>") else { break };
        out.push(rest[..j].to_string());
        rest = &rest[j + 6..];
    }
    out
}

fn next_token_in(body: &str) -> Option<String> {
    let i = body.find("<NextContinuationToken>")? + "<NextContinuationToken>".len();
    let rest = &body[i..];
    let j = rest.find("</NextContinuationToken>")?;
    Some(rest[..j].to_string())
}

#[tokio::test]
#[serial]
async fn list_reads_from_the_database_only() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.seed_objects("media-cdn", &["img/a.png", "img/b.png", "docs/c.pdf"])
            .await;

        let res = g.get(&signer, "/media-cdn?list-type=2&prefix=img/").await;

        assert_eq!(res.status_code(), 200, "{}", res.text());
        let body = res.text();
        assert!(body.contains("<Key>img/a.png</Key>"), "{body}");
        assert!(body.contains("<Key>img/b.png</Key>"));
        assert!(!body.contains("docs/c.pdf"));
        g.mock.assert_untouched();
    })
    .await;
}

#[tokio::test]
#[serial]
async fn the_xml_is_s3_shaped() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.seed_objects("media-cdn", &["a.png"]).await;

        let body = g.get(&signer, "/media-cdn?list-type=2").await.text();

        assert!(
            body.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"),
            "{body}"
        );
        assert!(body.contains("<ListBucketResult"));
        assert!(body.contains("<Name>media-cdn</Name>"));
        assert!(body.contains("<KeyCount>1</KeyCount>"));
        assert!(body.contains("<MaxKeys>1000</MaxKeys>"));
        assert!(body.contains("<IsTruncated>false</IsTruncated>"));
        assert!(body.contains("<Size>"));
        assert!(body.contains("<ETag>"));
        assert!(body.contains("<LastModified>"));
        assert!(body.contains("<StorageClass>STANDARD</StorageClass>"));

        // The bucket name is the logical one; the physical bucket must not appear anywhere.
        assert!(!body.contains("osg-main"), "physical bucket leaked: {body}");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn pagination_walks_every_key_exactly_once() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        let keys: Vec<String> = (0..25).map(|i| format!("k{i:02}")).collect();
        g.seed_objects(
            "media-cdn",
            &keys.iter().map(String::as_str).collect::<Vec<_>>(),
        )
        .await;

        let mut seen: Vec<String> = Vec::new();
        let mut token: Option<String> = None;
        for _ in 0..10 {
            let q = token.as_ref().map_or_else(
                || "/media-cdn?list-type=2&max-keys=10".to_string(),
                |t| format!("/media-cdn?list-type=2&max-keys=10&continuation-token={t}"),
            );
            let body = g.get(&signer, &q).await.text();
            seen.extend(keys_in(&body));
            token = next_token_in(&body);
            if token.is_none() {
                break;
            }
        }

        assert_eq!(seen.len(), 25, "every key exactly once, got {}", seen.len());
        let mut sorted = seen.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 25, "a key was repeated across pages");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn start_after_excludes_the_marker() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.seed_objects("media-cdn", &["a", "b", "c"]).await;

        let body = g
            .get(&signer, "/media-cdn?list-type=2&start-after=b")
            .await
            .text();

        assert!(!body.contains("<Key>a</Key>"), "{body}");
        assert!(!body.contains("<Key>b</Key>"));
        assert!(body.contains("<Key>c</Key>"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn an_empty_listing_omits_contents_entirely() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;

        let body = g
            .get(&signer, "/media-cdn?list-type=2&prefix=nothing/")
            .await
            .text();

        assert!(
            !body.contains("<Contents>"),
            "S3 omits the tag rather than emitting an empty one: {body}"
        );
        assert!(body.contains("<KeyCount>0</KeyCount>"));
    })
    .await;
}

/// The delimiter roll-up, over HTTP, with the parameter names a client actually sends.
#[tokio::test]
#[serial]
async fn a_delimiter_rolls_up_folders() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.seed_objects(
            "media-cdn",
            &["img/2025/c.png", "img/2026/a.png", "docs/d.pdf", "top.txt"],
        )
        .await;

        let body = g
            .get(&signer, "/media-cdn?list-type=2&delimiter=/")
            .await
            .text();
        assert!(body.contains("<Prefix>docs/</Prefix>"), "{body}");
        assert!(body.contains("<Prefix>img/</Prefix>"), "{body}");
        assert!(body.contains("<Key>top.txt</Key>"), "{body}");
        assert!(!body.contains("<Key>img/2026/a.png</Key>"), "{body}");

        let body = g
            .get(&signer, "/media-cdn?list-type=2&delimiter=/&prefix=img/")
            .await
            .text();
        assert!(body.contains("<Prefix>img/2025/</Prefix>"), "{body}");
        assert!(body.contains("<Prefix>img/2026/</Prefix>"), "{body}");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_scoped_key_cannot_list_the_bucket_root() {
    with_gateway(|g| async move {
        let signer = g.scoped_key("img/").await;
        g.seed_objects("media-cdn", &["img/a.png", "docs/secret.pdf"])
            .await;

        let res = g.get(&signer, "/media-cdn?list-type=2").await;

        assert_eq!(res.status_code(), 403, "{}", res.text());
        assert!(res.text().contains("AccessDenied"));
        assert!(!res.text().contains("secret.pdf"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_scoped_key_cannot_list_another_folder() {
    with_gateway(|g| async move {
        let signer = g.scoped_key("img/").await;
        g.seed_objects("media-cdn", &["docs/secret.pdf"]).await;

        let res = g.get(&signer, "/media-cdn?list-type=2&prefix=docs/").await;

        assert_eq!(res.status_code(), 403);
        assert!(!res.text().contains("secret.pdf"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_scoped_key_can_list_its_own_folder() {
    with_gateway(|g| async move {
        let signer = g.scoped_key("img/").await;
        g.seed_objects("media-cdn", &["img/a.png", "docs/secret.pdf"])
            .await;

        let body = g
            .get(&signer, "/media-cdn?list-type=2&prefix=img/")
            .await
            .text();

        assert!(body.contains("<Key>img/a.png</Key>"), "{body}");
        assert!(!body.contains("secret.pdf"));
    })
    .await;
}

/// A prefix that is a parent of the allowed one is refused: listing `im` would return `img/...` and disclose the folder structure the scope exists to fence off.
#[tokio::test]
#[serial]
async fn a_prefix_above_the_allowed_one_is_refused() {
    with_gateway(|g| async move {
        let signer = g.scoped_key("img/").await;

        let res = g.get(&signer, "/media-cdn?list-type=2&prefix=im").await;

        assert_eq!(res.status_code(), 403, "{}", res.text());
    })
    .await;
}

#[tokio::test]
#[serial]
async fn max_keys_is_capped_not_rejected() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.seed_objects("media-cdn", &["a"]).await;

        let body = g
            .get(&signer, "/media-cdn?list-type=2&max-keys=99999")
            .await
            .text();

        assert!(body.contains("<MaxKeys>1000</MaxKeys>"), "{body}");
    })
    .await;
}

/// A tampered token is refused rather than silently producing a wrong page.
#[tokio::test]
#[serial]
async fn a_tampered_continuation_token_is_refused() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;

        let res = g
            .get(
                &signer,
                "/media-cdn?list-type=2&continuation-token=not-a-real-token!!",
            )
            .await;

        assert_eq!(res.status_code(), 400, "{}", res.text());
        assert!(res.text().contains("InvalidArgument"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn list_buckets_returns_only_this_users_buckets() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.extra_bucket("archive").await;
        g.other_user_bucket("not-mine").await;

        let body = g.get(&signer, "/").await.text();

        assert!(body.contains("<ListAllMyBucketsResult"), "{body}");
        assert!(body.contains("<Name>media-cdn</Name>"));
        assert!(body.contains("<Name>archive</Name>"));
        assert!(
            !body.contains("not-mine"),
            "another user's bucket must not appear"
        );
        assert!(
            !body.contains("osg-main"),
            "the physical bucket must not appear"
        );
        assert!(body.contains("<CreationDate>"));
        // The owner's email must not travel in a response any access key can read.
        assert!(!body.contains("@osg.vn"), "owner email leaked: {body}");
        g.mock.assert_untouched();
    })
    .await;
}

#[tokio::test]
#[serial]
async fn list_buckets_needs_no_object_permission() {
    with_gateway(|g| async move {
        let signer = g.key_with(&["read"], &["img/"]).await;

        let res = g.get(&signer, "/").await;

        assert_eq!(res.status_code(), 200, "{}", res.text());
        assert!(res.text().contains("<Name>media-cdn</Name>"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn head_bucket_is_200_with_no_body() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;

        let res = g.head(&signer, "/media-cdn").await;

        assert_eq!(res.status_code(), 200);
        assert!(res.text().is_empty());
        g.mock.assert_untouched();
    })
    .await;
}

#[tokio::test]
#[serial]
async fn head_bucket_on_a_missing_bucket_is_a_bare_404() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;

        let res = g.head(&signer, "/nope").await;

        assert_eq!(res.status_code(), 404);
        assert!(
            res.text().is_empty(),
            "HEAD carries no body, not even an error one"
        );
    })
    .await;
}

/// A scope limits objects, not whether the bucket exists.
#[tokio::test]
#[serial]
async fn head_bucket_works_with_a_scoped_key() {
    with_gateway(|g| async move {
        let signer = g.scoped_key("img/").await;

        let res = g.head(&signer, "/media-cdn").await;

        assert_eq!(res.status_code(), 200, "{}", res.text());
    })
    .await;
}
