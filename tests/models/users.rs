use insta::assert_debug_snapshot;
use loco_rs::testing::prelude::*;
use object_storage_gate::{
    app::App,
    models::users::{self, Model, RegisterParams},
};
use sea_orm::{ActiveModelTrait, ActiveValue, IntoActiveModel};
use serial_test::serial;

macro_rules! configure_insta {
    ($($expr:expr),*) => {
        let mut settings = insta::Settings::clone_current();
        settings.set_prepend_module_to_snapshot(false);
        settings.set_snapshot_suffix("users");
        let _guard = settings.bind_to_scope();
    };
}

#[tokio::test]
#[serial]
async fn test_can_validate_model() {
    configure_insta!();

    let boot = boot_test::<App>()
        .await
        .expect("Failed to boot test application");

    let invalid_user = users::ActiveModel {
        name: ActiveValue::set("1".to_string()),
        email: ActiveValue::set("invalid-email".to_string()),
        ..Default::default()
    };

    let res = invalid_user.insert(&boot.app_context.db).await;

    assert_debug_snapshot!(res);
}

/// The first-run setup is the only self-service account creation left.
#[tokio::test]
#[serial]
async fn can_create_first_admin() {
    let boot = boot_test::<App>()
        .await
        .expect("Failed to boot test application");

    let params = RegisterParams {
        email: "root@osgate.vn".to_string(),
        password: "correct-horse-battery".to_string(),
        name: "root".to_string(),
    };

    let user = Model::create_first_admin(&boot.app_context.db, &params)
        .await
        .expect("first admin must be creatable on an empty instance");

    assert!(user.is_admin());
    assert!(user.verify_password("correct-horse-battery"));
}

/// Refused once any user exists, so a second setup call cannot mint another admin.
#[tokio::test]
#[serial]
async fn create_first_admin_is_refused_once_a_user_exists() {
    let boot = boot_test::<App>()
        .await
        .expect("Failed to boot test application");
    seed::<App>(&boot.app_context)
        .await
        .expect("Failed to seed database");

    let res = Model::create_first_admin(
        &boot.app_context.db,
        &RegisterParams {
            email: "second@osgate.vn".to_string(),
            password: "correct-horse-battery".to_string(),
            name: "second".to_string(),
        },
    )
    .await;

    assert!(res.is_err());
}

#[tokio::test]
#[serial]
async fn can_find_by_email() {
    configure_insta!();

    let boot = boot_test::<App>()
        .await
        .expect("Failed to boot test application");
    seed::<App>(&boot.app_context)
        .await
        .expect("Failed to seed database");

    let existing_user = Model::find_by_email(&boot.app_context.db, "user1@example.com").await;
    let non_existing_user_results =
        Model::find_by_email(&boot.app_context.db, "un@existing-email.com").await;

    assert_debug_snapshot!(existing_user);
    assert_debug_snapshot!(non_existing_user_results);
}

#[tokio::test]
#[serial]
async fn can_find_by_pid() {
    configure_insta!();

    let boot = boot_test::<App>()
        .await
        .expect("Failed to boot test application");
    seed::<App>(&boot.app_context)
        .await
        .expect("Failed to seed database");

    let existing_user =
        Model::find_by_pid(&boot.app_context.db, "11111111-1111-1111-1111-111111111111").await;
    let non_existing_user_results =
        Model::find_by_pid(&boot.app_context.db, "23232323-2323-2323-2323-232323232323").await;

    assert_debug_snapshot!(existing_user);
    assert_debug_snapshot!(non_existing_user_results);
}

#[tokio::test]
#[serial]
async fn can_reset_password() {
    configure_insta!();

    let boot = boot_test::<App>()
        .await
        .expect("Failed to boot test application");
    seed::<App>(&boot.app_context)
        .await
        .expect("Failed to seed database");

    let user = Model::find_by_pid(&boot.app_context.db, "11111111-1111-1111-1111-111111111111")
        .await
        .expect("Failed to find user by PID");

    assert!(
        user.verify_password("12341234"),
        "Password verification failed for original password"
    );

    let result = user
        .clone()
        .into_active_model()
        .reset_password(&boot.app_context.db, "new-password")
        .await;

    assert!(result.is_ok(), "Failed to reset password");

    let user = Model::find_by_pid(&boot.app_context.db, "11111111-1111-1111-1111-111111111111")
        .await
        .expect("Failed to find user by PID after password reset");

    assert!(
        user.verify_password("new-password"),
        "Password verification failed for new password"
    );
}

#[tokio::test]
#[serial]
async fn seed_leaves_id_sequence_past_fixtures() {
    let boot = boot_test::<App>()
        .await
        .expect("Failed to boot test application");
    seed::<App>(&boot.app_context)
        .await
        .expect("seed must work on every supported backend");

    // src/fixtures/users.yaml has 2 rows, id 1 and 2.
    // A new user must not touch an already-used id.
    let fresh = users::ActiveModel {
        email: ActiveValue::set("fresh@example.com".to_string()),
        password: ActiveValue::set("x".to_string()),
        name: ActiveValue::set("Fresh".to_string()),
        ..Default::default()
    }
    .insert(&boot.app_context.db)
    .await
    .expect("insert after seed");

    assert!(
        fresh.id > 2,
        "id sau seed phải vượt 2 dòng fixture, nhận được {}",
        fresh.id
    );
}
