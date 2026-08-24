use loco_rs::testing::prelude::*;
use object_storage_gate::{
    app::App,
    models::pools,
    s3::upstream::{self, Body, UpstreamRequest},
};
use serial_test::serial;

use crate::support::mock_upstream::{Canned, MockUpstream};

/// A pool whose upstream is the mock, with credentials so a client can be built.
async fn pool_pointing_at(db: &sea_orm::DatabaseConnection, mock: &MockUpstream) -> pools::Model {
    pools::Model::create(
        db,
        &pools::CreateParams {
            name: "main".to_string(),
            provider: pools::PROVIDER_CUSTOM.to_string(),
            region: Some("ap-southeast-1".to_string()),
            api_endpoint: Some(mock.base_url.clone()),
            physical_bucket: "osg-main".to_string(),
            access_id: Some("FIXTUREUPSTREAMID".to_string()),
            access_secret: Some("fixture-upstream-secret".to_string()),
        },
    )
    .await
    .unwrap()
}

/// The state the backfill migration leaves behind.
async fn unconfigured_pool(db: &sea_orm::DatabaseConnection, mock: &MockUpstream) -> pools::Model {
    pools::Model::create(
        db,
        &pools::CreateParams {
            name: "bare".to_string(),
            provider: pools::PROVIDER_CUSTOM.to_string(),
            api_endpoint: Some(mock.base_url.clone()),
            physical_bucket: "CHANGE-ME".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

async fn drain(res: upstream::UpstreamResponse) -> Vec<u8> {
    use futures_util::StreamExt;
    let mut body = res.body;
    let mut out = Vec::new();
    while let Some(chunk) = body.next().await {
        out.extend_from_slice(&chunk.unwrap());
    }
    out
}

/// Every upstream request must be signed, and the physical bucket must be in the path.
#[tokio::test]
#[serial]
async fn a_get_is_signed_and_addresses_the_physical_bucket() {
    let boot = boot_test::<App>().await.unwrap();
    let mock = MockUpstream::start().await;
    let pool = pool_pointing_at(&boot.app_context.db, &mock).await;

    let client = upstream::Client::new(&pool).unwrap();
    client
        .send(UpstreamRequest::get("11111111/media-cdn/photos/a.jpg"))
        .await
        .unwrap();

    let seen = mock.requests();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, "GET");
    mock.assert_key(0, "osg-main/11111111/media-cdn/photos/a.jpg");

    let auth = seen[0].header("authorization");
    assert!(
        auth.starts_with("AWS4-HMAC-SHA256 Credential=FIXTUREUPSTREAMID/"),
        "unexpected authorization: {auth}"
    );
    assert!(auth.contains("SignedHeaders="));
    assert!(auth.contains("Signature="));
    assert!(!seen[0].header("x-amz-date").is_empty());
    assert!(!seen[0].header("x-amz-content-sha256").is_empty());
    // The scope names s3, not the pool's provider.
    assert!(auth.contains("/ap-southeast-1/s3/aws4_request"));
}

/// A pool with no credentials must fail before sending anything, not send an unsigned request.
#[tokio::test]
#[serial]
async fn an_unconfigured_pool_refuses_to_build_a_client() {
    let boot = boot_test::<App>().await.unwrap();
    let mock = MockUpstream::start().await;
    let pool = unconfigured_pool(&boot.app_context.db, &mock).await;

    assert!(upstream::Client::new(&pool).is_err());
    mock.assert_untouched();
}

/// Body streams through; nothing is buffered.
#[tokio::test]
#[serial]
async fn a_put_streams_its_body() {
    let boot = boot_test::<App>().await.unwrap();
    let mock = MockUpstream::start().await;
    let pool = pool_pointing_at(&boot.app_context.db, &mock).await;
    let client = upstream::Client::new(&pool).unwrap();

    let payload = vec![7u8; 3 * 1024 * 1024];
    let chunks: Vec<Result<bytes::Bytes, std::io::Error>> = payload
        .chunks(64 * 1024)
        .map(|c| Ok(bytes::Bytes::copy_from_slice(c)))
        .collect();
    let stream = futures_util::stream::iter(chunks);

    client
        .send(UpstreamRequest::put(
            "11111111/media-cdn/big.bin",
            Body::Stream(Box::pin(stream)),
        ))
        .await
        .unwrap();

    let seen = mock.requests();
    assert_eq!(seen[0].body.len(), payload.len());
    // A streamed body cannot be hashed up front, so it signs as UNSIGNED-PAYLOAD.
    assert_eq!(seen[0].header("x-amz-content-sha256"), "UNSIGNED-PAYLOAD");
}

/// A body small enough to hold is hashed, so the signature covers the bytes.
#[tokio::test]
#[serial]
async fn a_bytes_body_is_hashed_into_the_signature() {
    let boot = boot_test::<App>().await.unwrap();
    let mock = MockUpstream::start().await;
    let pool = pool_pointing_at(&boot.app_context.db, &mock).await;
    let client = upstream::Client::new(&pool).unwrap();

    client
        .send(UpstreamRequest::put(
            "11111111/media-cdn/small.txt",
            Body::Bytes(b"hello".to_vec()),
        ))
        .await
        .unwrap();

    let seen = mock.requests();
    assert_eq!(seen[0].body, b"hello");
    assert_eq!(
        seen[0].header("x-amz-content-sha256"),
        // sha256("hello")
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

/// The response body comes back as a stream, not a buffer.
#[tokio::test]
#[serial]
async fn a_response_body_streams_back() {
    let boot = boot_test::<App>().await.unwrap();
    let mock = MockUpstream::start().await;
    mock.push(Canned::ok(b"the object bytes"));
    let pool = pool_pointing_at(&boot.app_context.db, &mock).await;
    let client = upstream::Client::new(&pool).unwrap();

    let res = client
        .send(UpstreamRequest::get("11111111/media-cdn/a.jpg"))
        .await
        .unwrap();
    assert_eq!(res.status, 200);
    assert_eq!(drain(res).await, b"the object bytes");
}

/// Spec §12: the upstream error body names the physical bucket. Keep the code, drop the body.
#[tokio::test]
#[serial]
async fn an_upstream_error_keeps_its_code_and_loses_its_body() {
    let boot = boot_test::<App>().await.unwrap();
    let mock = MockUpstream::start().await;
    mock.push(Canned {
        status: 404,
        headers: vec![("content-type".into(), "application/xml".into())],
        body: br#"<?xml version="1.0"?><Error><Code>NoSuchKey</Code>
                  <Message>The specified key does not exist.</Message>
                  <Key>osg-main/11111111/media-cdn/gone.jpg</Key>
                  <BucketName>osg-main</BucketName></Error>"#
            .to_vec(),
    });
    let pool = pool_pointing_at(&boot.app_context.db, &mock).await;
    let client = upstream::Client::new(&pool).unwrap();

    let err = client
        .send(UpstreamRequest::get("11111111/media-cdn/gone.jpg"))
        .await
        .unwrap_err();

    assert_eq!(err.code(), "NoSuchKey");
    let rendered = format!("{}{}", err.code(), err.message());
    assert!(
        !rendered.contains("osg-main"),
        "physical bucket leaked: {rendered}"
    );
}

/// An upstream 5xx is not the client's fault and must not carry upstream detail.
#[tokio::test]
#[serial]
async fn an_upstream_5xx_becomes_internal_error() {
    let boot = boot_test::<App>().await.unwrap();
    let mock = MockUpstream::start().await;
    mock.push(Canned {
        status: 503,
        headers: vec![],
        body: b"upstream is having a day".to_vec(),
    });
    let pool = pool_pointing_at(&boot.app_context.db, &mock).await;
    let client = upstream::Client::new(&pool).unwrap();

    let err = client
        .send(UpstreamRequest::get("11111111/media-cdn/a.jpg"))
        .await
        .unwrap_err();

    assert_eq!(err.code(), "InternalError");
    assert!(!err.message().contains("having a day"));
}

/// A query string must reach upstream in the same form it was signed in, or the store rejects the signature.
#[tokio::test]
#[serial]
async fn a_query_string_reaches_upstream_in_its_signed_form() {
    let boot = boot_test::<App>().await.unwrap();
    let mock = MockUpstream::start().await;
    let pool = pool_pointing_at(&boot.app_context.db, &mock).await;
    let client = upstream::Client::new(&pool).unwrap();

    client
        .send(UpstreamRequest::get("11111111/media-cdn/").with_query(vec![
            ("prefix".to_string(), "photos/2024".to_string()),
            ("list-type".to_string(), "2".to_string()),
        ]))
        .await
        .unwrap();

    // Sorted and encoded exactly as the canonical query, because the URL is built from it.
    assert_eq!(mock.requests()[0].query, "list-type=2&prefix=photos%2F2024");
}
