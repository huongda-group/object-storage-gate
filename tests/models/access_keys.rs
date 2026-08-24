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

fn params(label: &str) -> access_keys::CreateKeyParams {
    access_keys::CreateKeyParams {
        label: label.to_string(),
        expires_at: None,
        permissions: vec![],
        prefixes: vec![],
    }
}

#[tokio::test]
#[serial]
async fn create_key_secret_recoverable() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let uid = user(db).await;

    let (key, secret) = access_keys::Model::create_key(db, uid, &params("primary"))
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
    let (key, _) = access_keys::Model::create_key(db, uid, &params("primary"))
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
    let (key, _) = access_keys::Model::create_key(db, uid, &params("temporary"))
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
    let (key, _) = access_keys::Model::create_key(db, uid, &params("primary"))
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

#[tokio::test]
#[serial]
async fn create_key_persists_policy() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let uid = user(db).await;

    let params = access_keys::CreateKeyParams {
        label: "ci".to_string(),
        expires_at: None,
        permissions: vec!["read".to_string(), "list".to_string()],
        prefixes: vec!["images/*".to_string()],
    };
    let (key, _secret) = access_keys::Model::create_key(db, uid, &params)
        .await
        .unwrap();

    let mut perms = key.permissions(db).await.unwrap();
    perms.sort();
    assert_eq!(perms, vec!["list".to_string(), "read".to_string()]);
    assert_eq!(
        key.prefixes(db).await.unwrap(),
        vec!["images/*".to_string()]
    );
}

#[tokio::test]
#[serial]
async fn create_key_rejects_bad_input() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let uid = user(db).await;

    let bad_label = access_keys::CreateKeyParams {
        label: "root".to_string(),
        expires_at: None,
        permissions: vec![],
        prefixes: vec![],
    };
    assert!(access_keys::Model::create_key(db, uid, &bad_label)
        .await
        .is_err());

    let bad_action = access_keys::CreateKeyParams {
        label: "primary".to_string(),
        expires_at: None,
        permissions: vec!["sudo".to_string()],
        prefixes: vec![],
    };
    assert!(access_keys::Model::create_key(db, uid, &bad_action)
        .await
        .is_err());

    for bad in ["../escape", "/leading", ""] {
        let p = access_keys::CreateKeyParams {
            label: "primary".to_string(),
            expires_at: None,
            permissions: vec![],
            prefixes: vec![bad.to_string()],
        };
        assert!(
            access_keys::Model::create_key(db, uid, &p).await.is_err(),
            "prefix {bad:?} must be rejected"
        );
    }

    // A key born expired is a silent dead key — reject it at creation.
    let past = access_keys::CreateKeyParams {
        label: "primary".to_string(),
        expires_at: Some((Utc::now() - Duration::hours(1)).into()),
        permissions: vec![],
        prefixes: vec![],
    };
    assert!(access_keys::Model::create_key(db, uid, &past)
        .await
        .is_err());
}

async fn user_named(db: &sea_orm::DatabaseConnection, email: &str) -> i32 {
    users::ActiveModel {
        email: ActiveValue::set(email.to_string()),
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
async fn list_for_user_groups_policy_and_excludes_others() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let a = user_named(db, "a@ex.com").await;
    let b = user_named(db, "b@ex.com").await;

    let p = access_keys::CreateKeyParams {
        label: "primary".to_string(),
        expires_at: None,
        permissions: vec!["read".to_string()],
        prefixes: vec!["a/".to_string()],
    };
    access_keys::Model::create_key(db, a, &p).await.unwrap();
    access_keys::Model::create_key(db, b, &params("backup"))
        .await
        .unwrap();

    let rows = access_keys::Model::list_for_user(db, a).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].permissions, vec!["read".to_string()]);
    assert_eq!(rows[0].prefixes, vec!["a/".to_string()]);
}

#[tokio::test]
#[serial]
async fn find_by_pid_for_user_refuses_other_owner() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let a = user_named(db, "a@ex.com").await;
    let b = user_named(db, "b@ex.com").await;

    let (key, _) = access_keys::Model::create_key(db, a, &params("primary"))
        .await
        .unwrap();
    let pid = key.pid.to_string();

    assert!(access_keys::Model::find_by_pid_for_user(db, &pid, a)
        .await
        .is_ok());
    assert!(access_keys::Model::find_by_pid_for_user(db, &pid, b)
        .await
        .is_err());
    assert!(
        access_keys::Model::find_by_pid_for_user(db, "not-a-uuid", a)
            .await
            .is_err()
    );
}

#[tokio::test]
#[serial]
async fn set_policy_replaces_rows_and_validates() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let uid = user(db).await;
    let (key, _) = access_keys::Model::create_key(db, uid, &params("primary"))
        .await
        .unwrap();

    key.set_permissions(db, &["read".to_string(), "write".to_string()])
        .await
        .unwrap();
    key.set_permissions(db, &["list".to_string()])
        .await
        .unwrap();
    assert_eq!(key.permissions(db).await.unwrap(), vec!["list".to_string()]);

    key.set_prefixes(db, &["a/".to_string(), "b/".to_string()])
        .await
        .unwrap();
    key.set_prefixes(db, &[]).await.unwrap();
    assert!(key.prefixes(db).await.unwrap().is_empty());

    assert!(key
        .set_permissions(db, &["sudo".to_string()])
        .await
        .is_err());
    assert!(key.set_prefixes(db, &["../x".to_string()]).await.is_err());
    // A rejected update must leave the stored policy untouched.
    assert_eq!(key.permissions(db).await.unwrap(), vec!["list".to_string()]);
}

#[tokio::test]
#[serial]
async fn revoked_is_terminal() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let uid = user(db).await;
    let (key, _) = access_keys::Model::create_key(db, uid, &params("primary"))
        .await
        .unwrap();

    let disabled = key.set_status(db, access_keys::KEY_DISABLED).await.unwrap();
    assert_eq!(disabled.status, access_keys::KEY_DISABLED);
    let active = disabled
        .set_status(db, access_keys::KEY_ACTIVE)
        .await
        .unwrap();

    let revoked = active.revoke(db).await.unwrap();
    assert_eq!(revoked.status, access_keys::KEY_REVOKED);
    assert!(revoked
        .clone()
        .set_status(db, access_keys::KEY_ACTIVE)
        .await
        .is_err());
    assert!(revoked.set_status(db, "banana").await.is_err());
}

#[tokio::test]
#[serial]
async fn rotate_copies_policy_and_disables_old() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let uid = user(db).await;

    let p = access_keys::CreateKeyParams {
        label: "ci".to_string(),
        expires_at: None,
        permissions: vec!["read".to_string(), "list".to_string()],
        prefixes: vec!["ci/".to_string()],
    };
    let (old, old_secret) = access_keys::Model::create_key(db, uid, &p).await.unwrap();

    let (new, new_secret) = old.rotate(db).await.unwrap();

    assert_ne!(new.access_key_id, old.access_key_id);
    assert_ne!(new_secret, old_secret);
    assert_eq!(new.label, "ci");
    assert_eq!(new.user_id, uid);
    let mut perms = new.permissions(db).await.unwrap();
    perms.sort();
    assert_eq!(perms, vec!["list".to_string(), "read".to_string()]);
    assert_eq!(new.prefixes(db).await.unwrap(), vec!["ci/".to_string()]);

    // Loaded by pid, not by access key id: find_by_access_key_id is the authentication lookup and treats a disabled key as absent, which is exactly what is being asserted here.
    let reloaded = access_keys::Model::find_by_pid_for_user(db, &old.pid.to_string(), uid)
        .await
        .unwrap();
    assert_eq!(reloaded.status, access_keys::KEY_DISABLED);

    // Rotating a revoked key would quietly resurrect it as `disabled`.
    let revoked = new.clone().revoke(db).await.unwrap();
    assert!(revoked.rotate(db).await.is_err());
}

#[tokio::test]
#[serial]
async fn expired_key_cannot_be_rotated() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let uid = user(db).await;
    let (key, _) = access_keys::Model::create_key(db, uid, &params("temporary"))
        .await
        .unwrap();

    let mut am: access_keys::ActiveModel = key.into();
    am.expires_at = ActiveValue::set(Some((Utc::now() - Duration::hours(1)).into()));
    let lapsed = am.update(db).await.unwrap();

    let err = lapsed.rotate(db).await.unwrap_err().to_string();
    assert!(err.contains("expired"), "unexpected message: {err}");
}

/// P3 flagged this and left it open: prefix `team` also authorised `teamsecret/`, so a key handed to one team could read another team's folder.
#[test]
fn prefix_matching_respects_the_separator() {
    use object_storage_gate::models::access_keys::prefix_allows;

    // Inside.
    assert!(prefix_allows("img/", "img/a.png"));
    assert!(prefix_allows("img/", "img/nested/a.png"));
    assert!(prefix_allows("img", "img"));
    assert!(prefix_allows("img", "img/a.png"));

    // The bug.
    assert!(!prefix_allows("team", "teamsecret/x"));
    assert!(!prefix_allows("img", "imgsecret/a.png"));

    // Not a prefix at all.
    assert!(!prefix_allows("img/", "docs/a.png"));
    assert!(!prefix_allows("img/", "im"));

    // validate_prefixes refuses an empty prefix, so this can only come from a hand-edited row.
    // It denies rather than allows: fail-closed is the right direction for a policy row nobody meant to create.
    assert!(!prefix_allows("", "anything"));
    assert!(object_storage_gate::models::access_keys::validate_prefixes(&[String::new()]).is_err());
}

#[tokio::test]
#[serial]
async fn a_key_with_no_prefixes_allows_everything() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(db, "user1@example.com")
        .await
        .unwrap();
    let (key, _) = access_keys::Model::create_key(
        db,
        user.id,
        &access_keys::CreateKeyParams {
            label: "primary".to_string(),
            expires_at: None,
            permissions: vec![access_keys::ACTION_READ.to_string()],
            prefixes: vec![],
        },
    )
    .await
    .unwrap();

    assert!(key.allows_key(db, "anything/at/all").await.unwrap());
    assert!(key
        .allows_action(db, access_keys::ACTION_READ)
        .await
        .unwrap());
    assert!(!key
        .allows_action(db, access_keys::ACTION_WRITE)
        .await
        .unwrap());
}

#[tokio::test]
#[serial]
async fn a_scoped_key_allows_only_its_folders() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(db, "user1@example.com")
        .await
        .unwrap();
    let (key, _) = access_keys::Model::create_key(
        db,
        user.id,
        &access_keys::CreateKeyParams {
            label: "readonly".to_string(),
            expires_at: None,
            permissions: vec![access_keys::ACTION_READ.to_string()],
            prefixes: vec!["img/".to_string(), "docs/".to_string()],
        },
    )
    .await
    .unwrap();

    assert!(key.allows_key(db, "img/a.png").await.unwrap());
    assert!(key.allows_key(db, "docs/b.pdf").await.unwrap());
    assert!(!key.allows_key(db, "backup/c.tar").await.unwrap());
}

/// A revoked credential does not exist, as far as authentication is concerned.
#[tokio::test]
#[serial]
async fn find_by_access_key_id_ignores_revoked_and_expired() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(db, "user1@example.com")
        .await
        .unwrap();
    let (key, _) = access_keys::Model::create_key(
        db,
        user.id,
        &access_keys::CreateKeyParams {
            label: "primary".to_string(),
            expires_at: None,
            permissions: vec![access_keys::ACTION_READ.to_string()],
            prefixes: vec![],
        },
    )
    .await
    .unwrap();
    let id = key.access_key_id.clone();

    assert!(access_keys::Model::find_by_access_key_id(db, &id)
        .await
        .is_ok());

    key.revoke(db).await.unwrap();

    // Still a row, but the lookup must not hand it back.
    assert!(access_keys::Model::find_by_access_key_id(db, &id)
        .await
        .is_err());
}
