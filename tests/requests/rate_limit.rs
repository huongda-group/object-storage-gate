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
