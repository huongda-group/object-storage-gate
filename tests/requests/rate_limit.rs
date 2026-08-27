use loco_rs::testing::prelude::*;
use object_storage_gate::app::App;
use serial_test::serial;

/// Hammers the login endpoint past the configured burst and expects a 429.
/// Without this an attacker gets unlimited password guesses against every account, and
/// loco 0.16 ships no rate-limit middleware of its own.
#[tokio::test]
#[serial]
async fn login_is_rate_limited() {
    request::<App, _, _>(|request, _ctx| async move {
        let mut saw_429 = false;

        for _ in 0..40 {
            let res = request
                .post("/api/auth/login")
                .json(&serde_json::json!({
                    "email": "nobody@example.com",
                    "password": "guess"
                }))
                .await;
            if res.status_code() == 429 {
                saw_429 = true;
                break;
            }
        }

        assert!(saw_429, "login accepted 40 attempts without throttling");
    })
    .await;
}

/// The data plane must not be throttled by the same layer.
///
/// The limiter exists to stop password guessing on login. Applied to S3 as well it breaks the
/// product: `aws s3 sync` of 1200 objects stopped at the ~999th with a 429 before this was fixed,
/// and a multipart upload of a large file trips it too. `SigV4` per access key is the data plane's
/// control, and it is a stronger one than a per-IP bucket.
#[tokio::test]
#[serial]
async fn the_s3_data_plane_is_not_rate_limited() {
    request::<App, _, _>(|request, _ctx| async move {
        for i in 0..80 {
            let res = request
                .get("/media-cdn/img/a.png")
                .add_header(
                    axum::http::HeaderName::from_static("authorization"),
                    axum::http::HeaderValue::from_static("AWS4-HMAC-SHA256 not-a-real-credential"),
                )
                .await;
            assert_ne!(
                res.status_code(),
                429,
                "the data plane was throttled on request {i}"
            );
        }
    })
    .await;
}

/// The console and its assets are outside the limiter too — a page load fetches many of them.
#[tokio::test]
#[serial]
async fn the_console_is_not_rate_limited() {
    request::<App, _, _>(|request, _ctx| async move {
        for i in 0..80 {
            let res = request.get("/static/js/does-not-exist.js").await;
            assert_ne!(
                res.status_code(),
                429,
                "the console was throttled on request {i}"
            );
        }
    })
    .await;
}
