use loco_rs::testing::prelude::*;
use object_storage_gate::{app::App, models::audit_logs};
use serial_test::serial;

use super::prepare_data;

fn an_entry(user_id: Option<i32>, key: &str) -> audit_logs::AuditEntry {
    audit_logs::AuditEntry {
        user_id,
        access_key_id: Some(key.to_string()),
        bucket_id: None,
        object_key: Some("img/a.png".to_string()),
        action: audit_logs::ACTION_READ.to_string(),
        outcome: audit_logs::OUTCOME_OK.to_string(),
        status_code: 200,
        bytes: 9,
        duration_ms: 4,
        request_id: "req-1".to_string(),
        ip: "203.0.113.7".to_string(),
        user_agent: Some("aws-cli/2.36".to_string()),
    }
}

#[tokio::test]
#[serial]
async fn an_admin_reads_the_audit_log() {
    request::<App, _, _>(|request, ctx| async move {
        let admin = prepare_data::init_admin_login(&request, &ctx).await;
        audit_logs::Model::record(&ctx.db, &an_entry(Some(admin.user.id), "OSGONE"))
            .await
            .unwrap();

        let (k, v) = prepare_data::auth_header(&admin.token);
        let res = request.get("/api/admin/audit").add_header(k, v).await;

        assert_eq!(res.status_code(), 200, "{}", res.text());
        let rows = res.json::<Vec<serde_json::Value>>();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["access_key_id"].as_str(), Some("OSGONE"));
        assert_eq!(rows[0]["outcome"].as_str(), Some("ok"));
        assert_eq!(rows[0]["object_key"].as_str(), Some("img/a.png"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_plain_user_cannot_read_the_audit_log() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;
        audit_logs::Model::record(&ctx.db, &an_entry(None, "OSGSECRET"))
            .await
            .unwrap();

        let (k, v) = prepare_data::auth_header(&user.token);
        let res = request.get("/api/admin/audit").add_header(k, v).await;

        assert_eq!(res.status_code(), 403);
        assert!(res.text().contains("admin_required"));
        assert!(!res.text().contains("OSGSECRET"));
    })
    .await;
}

/// A caller asking for more than the ceiling gets the ceiling, not an error — the same posture max-keys takes.
#[tokio::test]
#[serial]
async fn the_limit_is_capped_not_rejected() {
    request::<App, _, _>(|request, ctx| async move {
        let admin = prepare_data::init_admin_login(&request, &ctx).await;
        for _ in 0..3 {
            audit_logs::Model::record(&ctx.db, &an_entry(Some(admin.user.id), "OSGONE"))
                .await
                .unwrap();
        }

        let (k, v) = prepare_data::auth_header(&admin.token);
        let res = request
            .get("/api/admin/audit?limit=99999")
            .add_header(k, v)
            .await;

        assert_eq!(res.status_code(), 200, "{}", res.text());
        assert_eq!(res.json::<Vec<serde_json::Value>>().len(), 3);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn filtering_by_user_returns_only_that_account() {
    request::<App, _, _>(|request, ctx| async move {
        let admin = prepare_data::init_admin_login(&request, &ctx).await;
        let other = prepare_data::create_user(
            &ctx,
            "other@osg.vn",
            "12341234",
            "Other",
            object_storage_gate::models::users::ROLE_USER,
        )
        .await;

        audit_logs::Model::record(&ctx.db, &an_entry(Some(admin.user.id), "OSGMINE"))
            .await
            .unwrap();
        audit_logs::Model::record(&ctx.db, &an_entry(Some(other.id), "OSGTHEIRS"))
            .await
            .unwrap();

        let (k, v) = prepare_data::auth_header(&admin.token);
        let res = request
            .get(&format!("/api/admin/audit?user_id={}", other.id))
            .add_header(k, v)
            .await;

        let rows = res.json::<Vec<serde_json::Value>>();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["access_key_id"].as_str(), Some("OSGTHEIRS"));
    })
    .await;
}
