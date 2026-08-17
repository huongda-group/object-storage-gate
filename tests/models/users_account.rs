use loco_rs::testing::prelude::*;
use object_storage_gate::{app::App, models::users};
use sea_orm::{ActiveModelTrait, ActiveValue};
use serial_test::serial;

#[tokio::test]
#[serial]
async fn new_user_defaults_role_and_unlimited_quota() {
    let boot = boot_test::<App>().await.expect("boot");
    let u = users::ActiveModel {
        email: ActiveValue::set("a@ex.com".to_string()),
        password: ActiveValue::set("x".to_string()),
        name: ActiveValue::set("Aa".to_string()),
        ..Default::default()
    }
    .insert(&boot.app_context.db)
    .await
    .expect("insert");

    assert_eq!(u.role, users::ROLE_USER);
    assert!(!u.is_admin());
    assert_eq!(u.max_bytes, 0);
    assert!(u.is_unlimited());
    assert_eq!(u.used_bytes, 0);
    assert_eq!(u.reserved_bytes, 0);
}

#[tokio::test]
#[serial]
async fn admin_role_and_limited_quota() {
    let boot = boot_test::<App>().await.expect("boot");
    let u = users::ActiveModel {
        email: ActiveValue::set("admin@ex.com".to_string()),
        password: ActiveValue::set("x".to_string()),
        name: ActiveValue::set("Admin".to_string()),
        role: ActiveValue::set(users::ROLE_ADMIN.to_string()),
        max_bytes: ActiveValue::set(1000),
        ..Default::default()
    }
    .insert(&boot.app_context.db)
    .await
    .expect("insert");

    assert!(u.is_admin());
    assert!(!u.is_unlimited());
}

/// The flag is only set by an admin issuing a temporary password, never by a plain insert.
#[tokio::test]
#[serial]
async fn new_user_does_not_require_password_change_by_default() {
    let boot = boot_test::<App>().await.expect("boot");
    seed::<App>(&boot.app_context).await.expect("seed");

    let user = users::Model::find_by_email(&boot.app_context.db, "user1@example.com")
        .await
        .expect("find seeded user");

    assert!(!user.must_change_password);
}
