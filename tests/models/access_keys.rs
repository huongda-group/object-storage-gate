use chrono::{Duration, Utc};
use loco_rs::testing::prelude::*;
use object_storage_gate::{
    app::App,
    models::{access_key_permissions, access_key_prefixes, access_keys, users},
};
use sea_orm::{ActiveModelTrait, ActiveValue};
use serial_test::serial;

async fn user(db: &sea_orm::DatabaseConnection) -> i32 {
    users::ActiveModel {
        email: ActiveValue::set("k@ex.com".to_string()),
        password: ActiveValue::set("x".to_string()),
        name: ActiveValue::set("Us".to_string()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
    .id
}

#[tokio::test]
#[serial]
async fn create_key_secret_recoverable() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let uid = user(db).await;

    let (key, secret) = access_keys::Model::create_key(db, uid, "primary")
        .await
        .unwrap();
    assert!(key.access_key_id.starts_with("OSG"));
    assert_ne!(key.secret_encrypted, secret.as_bytes());
    assert_eq!(key.decrypt_secret().unwrap(), secret);
    assert!(key.is_usable());

    let found = access_keys::Model::find_by_access_key_id(db, &key.access_key_id)
        .await
        .unwrap();
    assert_eq!(found.id, key.id);
}

#[tokio::test]
#[serial]
async fn is_usable_respects_status_and_expiry() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let uid = user(db).await;
    let (key, _) = access_keys::Model::create_key(db, uid, "primary")
        .await
        .unwrap();

    let mut am: access_keys::ActiveModel = key.clone().into();
    am.status = ActiveValue::set(access_keys::KEY_DISABLED.to_string());
    assert!(!am.update(db).await.unwrap().is_usable());

    let mut am2: access_keys::ActiveModel = key.into();
    am2.expires_at = ActiveValue::set(Some((Utc::now() - Duration::hours(1)).into()));
    assert!(!am2.update(db).await.unwrap().is_usable());
}

#[tokio::test]
#[serial]
async fn effective_status_derives_expired() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let uid = user(db).await;
    let (key, _) = access_keys::Model::create_key(db, uid, "temporary")
        .await
        .unwrap();

    // No expiry → stored status, nothing to derive.
    assert_eq!(key.effective_status(), access_keys::KEY_ACTIVE);
    assert!(!key.is_expired());
    assert_eq!(key.days_until_expiry(), None);

    // Future expiry → still active, with a day count for the console column.
    let mut am: access_keys::ActiveModel = key.clone().into();
    am.expires_at = ActiveValue::set(Some((Utc::now() + Duration::days(3)).into()));
    let soon = am.update(db).await.unwrap();
    assert_eq!(soon.effective_status(), access_keys::KEY_ACTIVE);
    assert_eq!(soon.days_until_expiry(), Some(2)); // 2 full days + change
    assert!(soon.is_usable());

    // Past expiry on an active key → derived "expired".
    let mut am: access_keys::ActiveModel = soon.into();
    am.expires_at = ActiveValue::set(Some((Utc::now() - Duration::hours(1)).into()));
    let lapsed = am.update(db).await.unwrap();
    assert!(lapsed.is_expired());
    assert_eq!(lapsed.effective_status(), access_keys::KEY_EXPIRED);
    assert_eq!(lapsed.days_until_expiry(), Some(0));

    // A revoked key stays revoked even after lapsing — not "merely expired".
    let mut am: access_keys::ActiveModel = lapsed.into();
    am.status = ActiveValue::set(access_keys::KEY_REVOKED.to_string());
    let revoked = am.update(db).await.unwrap();
    assert_eq!(revoked.effective_status(), access_keys::KEY_REVOKED);
}

#[tokio::test]
#[serial]
async fn policy_children_load() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let uid = user(db).await;
    let (key, _) = access_keys::Model::create_key(db, uid, "primary")
        .await
        .unwrap();

    access_key_permissions::ActiveModel {
        access_key_id: ActiveValue::set(key.id),
        action: ActiveValue::set(access_keys::ACTION_READ.to_string()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
    access_key_prefixes::ActiveModel {
        access_key_id: ActiveValue::set(key.id),
        prefix: ActiveValue::set("images/*".to_string()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();

    assert_eq!(key.permissions(db).await.unwrap(), vec!["read"]);
    assert_eq!(key.prefixes(db).await.unwrap(), vec!["images/*"]);
}
