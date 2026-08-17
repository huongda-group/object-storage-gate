use insta::{assert_debug_snapshot, with_settings};
use loco_rs::testing::prelude::*;
use object_storage_gate::{app::App, models::users};
use serial_test::serial;

use super::prepare_data;

// TODO: see how to dedup / extract this to app-local test utils not to framework, because that would require a runtime dep on insta
macro_rules! configure_insta {
    ($($expr:expr),*) => {
        let mut settings = insta::Settings::clone_current();
        settings.set_prepend_module_to_snapshot(false);
        settings.set_snapshot_suffix("auth_request");
        let _guard = settings.bind_to_scope();
    };
}

/// Every route the mail flows used to own must be gone from the router.
///
/// The SPA static fallback answers unmatched GETs, so an unrouted POST comes back as 405
/// rather than 404; either one means nothing handles the path any more, and a 2xx would
/// mean the endpoint is still live.
#[tokio::test]
#[serial]
async fn removed_mail_routes_are_gone() {
    configure_insta!();

    request::<App, _, _>(|request, _ctx| async move {
        for path in [
            "/api/auth/register",
            "/api/auth/forgot",
            "/api/auth/reset",
            "/api/auth/magic-link",
            "/api/auth/resend-verification-mail",
        ] {
            let status = request
                .post(path)
                .json(&serde_json::json!({}))
                .await
                .status_code();
            assert!(
                status == 404 || status == 405,
                "POST {path} should no longer be routed, got {status}"
            );
        }

        // The token routes were GETs, and a GET falls through to the SPA index instead of the
        // handler, so assert on the body rather than on the status.
        for path in ["/api/auth/verify/anything", "/api/auth/magic-link/anything"] {
            let res = request.get(path).await;
            assert!(
                !res.text().contains("token"),
                "GET {path} still looks like it is handled"
            );
        }
    })
    .await;
}

#[tokio::test]
#[serial]
async fn can_setup_first_admin_then_refuses_second() {
    configure_insta!();

    request::<App, _, _>(|request, ctx| async move {
        let status = request.get("/api/auth/setup").await;
        assert_eq!(status.status_code(), 200);
        assert_eq!(
            status.json::<serde_json::Value>()["needs_setup"],
            serde_json::Value::Bool(true)
        );

        let res = request
            .post("/api/auth/setup")
            .json(&serde_json::json!({
                "name": "root",
                "email": "root@osgate.vn",
                "password": "correct-horse-battery"
            }))
            .await;
        assert_eq!(res.status_code(), 200);

        let admin = users::Model::find_by_email(&ctx.db, "root@osgate.vn")
            .await
            .unwrap();
        assert!(admin.is_admin());

        let second = request
            .post("/api/auth/setup")
            .json(&serde_json::json!({
                "name": "other",
                "email": "other@osgate.vn",
                "password": "correct-horse-battery"
            }))
            .await;
        assert_eq!(second.status_code(), 403);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn login_with_valid_password() {
    configure_insta!();

    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();

        let response = request
            .post("/api/auth/login")
            .json(&serde_json::json!({
                "email": "user1@example.com",
                "password": "12341234"
            }))
            .await;

        assert_eq!(response.status_code(), 200);
        with_settings!({
            filters => cleanup_user_model()
        }, {
            assert_debug_snapshot!(response.text());
        });
    })
    .await;
}

#[tokio::test]
#[serial]
async fn login_with_invalid_password() {
    configure_insta!();

    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();

        let response = request
            .post("/api/auth/login")
            .json(&serde_json::json!({
                "email": "user1@example.com",
                "password": "wrong-password"
            }))
            .await;

        with_settings!({
            filters => cleanup_user_model()
        }, {
            assert_debug_snapshot!((response.status_code(), response.text()));
        });
    })
    .await;
}

#[tokio::test]
#[serial]
async fn login_with_un_existing_email() {
    configure_insta!();

    request::<App, _, _>(|request, _ctx| async move {
        let response = request
            .post("/api/auth/login")
            .json(&serde_json::json!({
                "email": "nobody@example.com",
                "password": "whatever"
            }))
            .await;

        assert_eq!(response.status_code(), 401);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn can_get_current_user() {
    configure_insta!();

    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;

        let (auth_key, auth_value) = prepare_data::auth_header(&user.token);
        let response = request
            .get("/api/auth/current")
            .add_header(auth_key, auth_value)
            .await;

        with_settings!({
            filters => cleanup_user_model()
        }, {
            assert_debug_snapshot!((response.status_code(), response.text()));
        });
    })
    .await;
}
