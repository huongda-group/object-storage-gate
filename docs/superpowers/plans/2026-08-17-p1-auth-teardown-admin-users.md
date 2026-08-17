# P1 — Xoá đăng ký, admin quản lý user — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Xoá tự đăng ký và toàn bộ luồng mail; thay bằng admin tạo user với mật khẩu tạm, và một extractor `AdminCaller` gác mọi endpoint admin phía server.

**Architecture:** Gỡ trước, dựng sau. Xoá route/model/mailer/cột DB của các luồng bỏ đi, chạy test cho xanh, rồi thêm cờ `must_change_password`, extractor `AdminCaller`, và controller `admin.rs`. Extractor `Caller` hiện tại được tách làm hai tầng — `RawCaller` giải người gọi, `Caller` bọc thêm kiểm cờ đổi mật khẩu — nên mọi endpoint hiện có tự động bị chặn khi user chưa đổi mật khẩu tạm mà không phải sửa từng handler.

**Tech Stack:** Rust, loco-rs 0.16, SeaORM 1.1, Axum 0.8, insta, serial_test, rstest. Frontend React 19 + TanStack Router.

**Spec:** `docs/superpowers/specs/2026-08-17-go-live-hardening-design.md`

## Global Constraints

- Ba backend hạng nhất: Postgres, MySQL >= 8.0.13, SQLite. Mọi query phải chạy được trên cả ba. Cấm `ILIKE`, `RETURNING`, `ON CONFLICT` / `ON DUPLICATE KEY`, `jsonb`, cột array, `pg_advisory_lock`, `SELECT ... FOR UPDATE SKIP LOCKED`.
- Migration dùng `ColType` + `SchemaManager` trước; raw SQL chỉ khi không tránh được, và branch theo `m.get_database_backend()`.
- Cột `TIMESTAMP` mới phải khai `TIMESTAMP(6)` trên MySQL. Plan này không thêm cột timestamp nào.
- `src/models/_entities/` là generated — không sửa tay, sinh bằng `cargo loco db entities` **đối với Postgres**.
- Comment trong code: tiếng Anh, một câu một dòng, không xuống dòng giữa câu.
- Không tự commit hay push ngoài các bước commit ghi trong plan. Không AI attribution trong message.
- Sau mỗi task, `cargo clippy --all-targets` phải sạch.

---

## File Structure

**Xoá hẳn:**
- `src/mailers/auth.rs`, `src/mailers/mod.rs`, `src/mailers/auth/` (9 template `.t`)

**Tạo mới:**
- `migration/src/m20260817_000001_auth_teardown.rs` — bỏ 7 cột mail, thêm `must_change_password`
- `src/controllers/admin.rs` — CRUD user cho admin
- `src/views/admin.rs` — shaper JSON cho user
- `tests/requests/admin.rs` — test cho controller trên
- `frontend/src/routes/_app/change-password.tsx` — màn đổi mật khẩu bắt buộc

**Sửa:**
- `src/controllers/auth.rs` — còn 4 handler: `setup_status`, `setup_admin`, `login`, `current`
- `src/controllers/api.rs` — tách `Caller` thành `RawCaller` + `Caller`, thêm `AdminCaller`, thêm `POST /api/me/password`
- `src/controllers/mod.rs` — khai báo `admin`
- `src/models/users.rs` — bỏ 8 method của luồng mail, thêm `create_by_admin`, `set_password`, `list_all`, `update_profile`
- `src/views/auth.rs` — `LoginResponse.is_verified` → `must_change_password`; `CurrentResponse` thêm cờ
- `src/views/mod.rs` — khai báo `admin`
- `src/app.rs` — thêm `controllers::admin::routes()`
- `src/lib.rs` — bỏ `pub mod mailers`
- `src/fixtures/users.yaml` — bỏ cột mail, thêm `must_change_password`
- `tests/requests/auth.rs` — xoá test của luồng bỏ đi
- `tests/requests/prepare_data.rs` — tạo user bằng model, không qua endpoint
- `tests/models/users.rs`, `tests/models/users_account.rs` — bỏ test của method đã xoá
- `frontend/src/lib/auth.ts` — bỏ 6 hàm, thêm `changePassword`
- `frontend/src/routes/_auth/login.tsx` — bỏ link tới forgot/register/magic-link
- `frontend/src/routes/_app.tsx` — chặn khi `must_change_password`
- `Cargo.toml` — không đổi (loco vẫn cần mailer feature cho boot, chỉ là không dùng)

**Xoá ở frontend:**
- `frontend/src/routes/_auth/register.tsx`
- `frontend/src/routes/_auth/forgot.tsx`
- `frontend/src/routes/_auth/reset.tsx`
- `frontend/src/routes/_auth/magic-link.tsx`
- `frontend/src/routes/_auth/verify.$token.tsx`

---

## Task 1: Gỡ luồng mail khỏi controller và model

**Files:**
- Modify: `src/controllers/auth.rs`
- Modify: `src/models/users.rs`
- Modify: `src/views/auth.rs:14`
- Delete: `src/mailers/auth.rs`, `src/mailers/mod.rs`, `src/mailers/auth/` (toàn thư mục)
- Modify: `src/lib.rs`
- Test: `tests/requests/auth.rs`, `tests/requests/prepare_data.rs`

**Interfaces:**
- Consumes: — (task đầu)
- Produces: `src/controllers/auth::routes()` chỉ còn 4 route: `GET /api/auth/setup`, `POST /api/auth/setup`, `POST /api/auth/login`, `GET /api/auth/current`. `users::Model` mất `find_by_verification_token`, `find_by_magic_token`, `find_by_reset_token`, `create_with_password`; `users::ActiveModel` mất `set_email_verification_sent`, `set_forgot_password_sent`, `verified`, `create_magic_link`, `clear_magic_link`. `reset_password(db, password) -> ModelResult<Model>` giữ nguyên chữ ký, task 4 dùng lại.

- [x] **Step 1: Viết test khẳng định route đã biến mất**

Thay toàn bộ `tests/requests/auth.rs` bằng file dưới. Các test cũ của luồng mail bị xoá theo; `can_login_with_verify`, `can_login_without_verify`, `can_register`, `invalid_verification_token`, `can_reset_password`, `can_auth_with_magic_link`, `can_reject_invalid_magic_link_token`, `can_resend_verification_email`, `cannot_resend_email_if_already_verified` không còn đối tượng để kiểm.

```rust
use insta::{assert_debug_snapshot, with_settings};
use loco_rs::testing::prelude::*;
use object_storage_gate::{app::App, models::users};
use serial_test::serial;

macro_rules! configure_insta {
    ($($expr:expr),*) => {
        let mut settings = insta::Settings::clone_current();
        settings.set_prepend_module_to_snapshot(false);
        settings.set_snapshot_suffix("auth_request");
        let _guard = settings.bind_to_scope();
    };
}

/// Every route the mail flows used to own must be gone from the router.
/// A 404 here is the assertion, not an accident.
#[tokio::test]
#[serial]
async fn removed_mail_routes_are_gone() {
    configure_insta!();

    request::<App, _, _>(|request, _ctx| async move {
        for (method, path) in [
            ("POST", "/api/auth/register"),
            ("POST", "/api/auth/forgot"),
            ("POST", "/api/auth/reset"),
            ("POST", "/api/auth/magic-link"),
            ("POST", "/api/auth/resend-verification-mail"),
        ] {
            let res = match method {
                "POST" => request.post(path).json(&serde_json::json!({})).await,
                _ => unreachable!(),
            };
            assert_eq!(
                res.status_code(),
                404,
                "{method} {path} should no longer be routed"
            );
        }

        let res = request.get("/api/auth/verify/anything").await;
        assert_eq!(res.status_code(), 404);
        let res = request.get("/api/auth/magic-link/anything").await;
        assert_eq!(res.status_code(), 404);
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
        let user = crate::requests::prepare_data::init_user_login(&request, &ctx).await;

        let (auth_key, auth_value) = crate::requests::prepare_data::auth_header(&user.token);
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
```

- [x] **Step 2: Chạy test để chắc nó fail**

Run: `cargo test --test mod requests::auth 2>&1 | tail -30`
Expected: FAIL — `removed_mail_routes_are_gone` báo 200/405 chứ không phải 404, vì route vẫn còn.

- [x] **Step 3: Rút gọn `src/controllers/auth.rs`**

Thay toàn bộ file bằng:

```rust
use crate::{
    models::{
        _entities::users,
        users::{LoginParams, RegisterParams},
    },
    views::auth::{CurrentResponse, LoginResponse},
};
use axum::http::StatusCode;
use loco_rs::{controller::ErrorDetail, prelude::*};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct SetupStatusResponse {
    pub needs_setup: bool,
}

/// Reports whether this instance still has no user, i.e. the console should send the visitor to the first-run admin setup page.
/// Public on purpose: the setup page is reachable before any credential exists.
#[debug_handler]
async fn setup_status(State(ctx): State<AppContext>) -> Result<Response> {
    format::json(SetupStatusResponse {
        needs_setup: !users::Model::any_exists(&ctx.db).await?,
    })
}

/// Creates the first admin of a fresh instance and logs it straight in.
/// Refused with 403 once any user exists.
/// This is the only self-service account creation left; every later account is created by an admin.
#[debug_handler]
async fn setup_admin(
    State(ctx): State<AppContext>,
    Json(params): Json<RegisterParams>,
) -> Result<Response> {
    if users::Model::any_exists(&ctx.db).await? {
        return Err(Error::CustomError(
            StatusCode::FORBIDDEN,
            ErrorDetail::new("setup_done", "setup has already been completed"),
        ));
    }

    let user = users::Model::create_first_admin(&ctx.db, &params).await?;

    let jwt_secret = ctx.config.get_jwt_config()?;
    let token = user
        .generate_jwt(&jwt_secret.secret, jwt_secret.expiration)
        .or_else(|_| unauthorized("unauthorized!"))?;

    format::json(LoginResponse::new(&user, &token))
}

/// Creates a user login and returns a token.
#[debug_handler]
async fn login(State(ctx): State<AppContext>, Json(params): Json<LoginParams>) -> Result<Response> {
    let Ok(user) = users::Model::find_by_email(&ctx.db, &params.email).await else {
        tracing::debug!(
            email = params.email,
            "login attempt with non-existent email"
        );
        return unauthorized("Invalid credentials!");
    };

    if !user.verify_password(&params.password) {
        return unauthorized("Invalid credentials!");
    }

    let jwt_secret = ctx.config.get_jwt_config()?;

    let token = user
        .generate_jwt(&jwt_secret.secret, jwt_secret.expiration)
        .or_else(|_| unauthorized("unauthorized!"))?;

    format::json(LoginResponse::new(&user, &token))
}

#[debug_handler]
async fn current(auth: auth::JWT, State(ctx): State<AppContext>) -> Result<Response> {
    let user = users::Model::find_by_pid(&ctx.db, &auth.claims.pid).await?;
    format::json(CurrentResponse::new(&user))
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/auth")
        .add("/setup", get(setup_status))
        .add("/setup", post(setup_admin))
        .add("/login", post(login))
        .add("/current", get(current))
}
```

Ghi chú: thông điệp lỗi của login được thống nhất thành `"Invalid credentials!"` cho cả hai nhánh (email sai và mật khẩu sai) — trước đây một nhánh trả `"unauthorized!"`, đủ để phân biệt email tồn tại hay không.

- [x] **Step 4: Gỡ 8 method luồng mail khỏi `src/models/users.rs`**

Xoá khỏi `impl Model`: `find_by_verification_token`, `find_by_magic_token`,
`find_by_reset_token`, `create_with_password`.

Xoá khỏi `impl ActiveModel`: `set_email_verification_sent`,
`set_forgot_password_sent`, `verified`, `create_magic_link`, `clear_magic_link`.

Giữ `reset_password` nhưng bỏ hai dòng đụng tới cột sắp xoá:

```rust
    /// Replaces the user's password hash.
    /// Used by the admin reset endpoint and by the self-service change-password endpoint.
    ///
    /// # Errors
    ///
    /// when has DB query error or could not hash the given password
    pub async fn reset_password(
        mut self,
        db: &DatabaseConnection,
        password: &str,
    ) -> ModelResult<Model> {
        self.password =
            ActiveValue::set(hash::hash_password(password).map_err(|e| ModelError::Any(e.into()))?);
        self.update(db).await.map_err(ModelError::from)
    }
```

Bỏ khỏi `create_first_admin` dòng
`email_verified_at: ActiveValue::set(Some(Local::now().into())),`.

Sửa import đầu file — `Duration` và `Local` không còn ai dùng:

```rust
use async_trait::async_trait;
use loco_rs::{auth::jwt, hash, prelude::*};
use serde::{Deserialize, Serialize};
use serde_json::Map;
use uuid::Uuid;
```

Xoá hai hằng `MAGIC_LINK_LENGTH` và `MAGIC_LINK_EXPIRATION_MIN`.

- [x] **Step 5: Xoá mailer**

```bash
git rm -r src/mailers
```

Bỏ dòng `pub mod mailers;` khỏi `src/lib.rs`.

- [x] **Step 6: Sửa `src/views/auth.rs`**

`is_verified` không còn nguồn dữ liệu. Đổi thành cờ mật khẩu tạm — cột chưa tồn
tại nên tạm hardcode `false`, task 2 nối vào cột thật.

```rust
#[derive(Debug, Deserialize, Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub pid: String,
    pub name: String,
    pub must_change_password: bool,
}

impl LoginResponse {
    #[must_use]
    pub fn new(user: &users::Model, token: &str) -> Self {
        Self {
            token: token.to_string(),
            pid: user.pid.to_string(),
            name: user.name.clone(),
            // Wired to the real column in task 2, once the migration adds it.
            must_change_password: false,
        }
    }
}
```

- [x] **Step 7: Viết lại `tests/requests/prepare_data.rs`**

Không còn endpoint đăng ký nên helper phải tạo user thẳng qua model.

```rust
use axum::http::{HeaderName, HeaderValue};
use loco_rs::{app::AppContext, TestServer};
use object_storage_gate::{
    models::users::{self, RegisterParams},
    views::auth::LoginResponse,
};

const USER_EMAIL: &str = "test@loco.com";
const USER_PASSWORD: &str = "12341234";

pub struct LoggedInUser {
    pub user: users::Model,
    pub token: String,
}

/// Creates a plain (non-admin) user straight through the model, then logs it in over the API.
/// There is no registration endpoint any more, so the model is the only way in.
pub async fn init_user_login(request: &TestServer, ctx: &AppContext) -> LoggedInUser {
    users::Model::create_first_admin(
        &ctx.db,
        &RegisterParams {
            name: "loco".to_string(),
            email: USER_EMAIL.to_string(),
            password: USER_PASSWORD.to_string(),
        },
    )
    .await
    .expect("create user");

    let response = request
        .post("/api/auth/login")
        .json(&serde_json::json!({
            "email": USER_EMAIL,
            "password": USER_PASSWORD
        }))
        .await;

    let login_response: LoginResponse = serde_json::from_str(&response.text()).unwrap();

    LoggedInUser {
        user: users::Model::find_by_email(&ctx.db, USER_EMAIL)
            .await
            .unwrap(),
        token: login_response.token,
    }
}

pub fn auth_header(token: &str) -> (HeaderName, HeaderValue) {
    let auth_header_value = HeaderValue::from_str(&format!("Bearer {}", &token)).unwrap();

    (HeaderName::from_static("authorization"), auth_header_value)
}
```

Ghi chú: helper dùng `create_first_admin` nên user test là admin. Task 3 thêm
`init_plain_user_login` cho các test cần user thường; các test hiện có không phân
biệt nên đổi này không làm hỏng gì.

- [x] **Step 8: Dọn `tests/models/users.rs`**

Xoá mọi test gọi tới method đã bị gỡ. Chạy để biết chính xác cái nào:

Run: `cargo test --test mod models::users 2>&1 | grep -E "^error|cannot find"`

Xoá các test khớp, giữ lại test của `find_by_email`, `find_by_pid`,
`verify_password`, `create_first_admin`, và `any_exists`.

- [x] **Step 9: Chạy toàn bộ test**

Run: `cargo test 2>&1 | tail -30`
Expected: PASS. Snapshot của login sẽ lệch vì `is_verified` đổi tên.

Run: `cargo insta review`
Duyệt snapshot mới cho `login_with_valid_password`, `can_get_current_user`.
Xoá file snapshot mồ côi của test đã bị bỏ:

```bash
git rm tests/requests/snapshots/can_register@auth_request.snap \
       tests/requests/snapshots/can_reset_password@auth_request.snap \
       tests/requests/snapshots/can_auth_with_magic_link@auth_request.snap \
       tests/requests/snapshots/resend_verification_user@auth_request.snap \
       tests/requests/snapshots/can_login_without_verify@auth_request.snap
```

- [x] **Step 10: Clippy**

Run: `cargo clippy --all-targets 2>&1 | tail -20`
Expected: không cảnh báo. Nếu báo import thừa trong `auth.rs` hoặc `users.rs` thì gỡ.

- [x] **Step 11: Commit**

```bash
git add -A src/ tests/ migration/ Cargo.toml
git commit -m "feat(auth): remove self-registration and all mail-based auth flows

Registration, email verification, magic link and password reset are gone.
Accounts are now created only by the first-run setup or by an admin.
Drops src/mailers entirely; the production SMTP block was never configured."
```

---

## Task 2: Migration — bỏ cột mail, thêm `must_change_password`

**Files:**
- Create: `migration/src/m20260817_000001_auth_teardown.rs`
- Modify: `migration/src/lib.rs`
- Modify: `src/models/_entities/users.rs` (regenerated, không sửa tay)
- Modify: `src/fixtures/users.yaml`
- Modify: `src/views/auth.rs`
- Test: `tests/models/users_account.rs`

**Interfaces:**
- Consumes: task 1 đã gỡ mọi code đọc/ghi 7 cột sắp xoá.
- Produces: `users.must_change_password: bool` trên entity. `LoginResponse.must_change_password` và `CurrentResponse.must_change_password` đọc từ cột thật.

- [x] **Step 1: Viết test cho cột mới**

Thêm vào cuối `tests/models/users_account.rs`:

```rust
#[tokio::test]
#[serial]
async fn new_user_does_not_require_password_change_by_default() {
    let boot = boot_test::<App>().await.unwrap();
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(&boot.app_context.db, "user1@example.com")
        .await
        .unwrap();

    assert!(!user.must_change_password);
}
```

- [x] **Step 2: Chạy để chắc nó fail**

Run: `cargo test --test mod models::users_account 2>&1 | tail -20`
Expected: FAIL biên dịch — `no field 'must_change_password' on type 'Model'`.

- [x] **Step 3: Viết migration**

Tạo `migration/src/m20260817_000001_auth_teardown.rs`:

```rust
use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// Columns that only ever served the mail-based auth flows removed in P1.
const MAIL_COLUMNS: &[&str] = &[
    "email_verification_token",
    "email_verification_sent_at",
    "email_verified_at",
    "magic_link_token",
    "magic_link_expiration",
    "reset_token",
    "reset_sent_at",
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        for col in MAIL_COLUMNS {
            remove_column(m, "users", col).await?;
        }
        add_column(
            m,
            "users",
            "must_change_password",
            ColType::BooleanWithDefault(false),
        )
        .await?;
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        remove_column(m, "users", "must_change_password").await?;
        add_column(m, "users", "reset_token", ColType::StringNull).await?;
        add_column(
            m,
            "users",
            "reset_sent_at",
            ColType::TimestampWithTimeZoneNull,
        )
        .await?;
        add_column(
            m,
            "users",
            "email_verification_token",
            ColType::StringNull,
        )
        .await?;
        add_column(
            m,
            "users",
            "email_verification_sent_at",
            ColType::TimestampWithTimeZoneNull,
        )
        .await?;
        add_column(
            m,
            "users",
            "email_verified_at",
            ColType::TimestampWithTimeZoneNull,
        )
        .await?;
        add_column(m, "users", "magic_link_token", ColType::StringNull).await?;
        add_column(
            m,
            "users",
            "magic_link_expiration",
            ColType::TimestampWithTimeZoneNull,
        )
        .await?;
        Ok(())
    }
}
```

Ghi chú về `down()`: các cột phục hồi lại là timestamp nullable. Trên MySQL chúng
sinh ra với precision 0 vì `m20260815_000001` đã chạy từ trước và chỉ widen những
cột tồn tại lúc đó. Chấp nhận được — `down()` là đường thoát khẩn cấp, và các cột
này không còn code nào đọc.

- [x] **Step 4: Đăng ký migration**

Trong `migration/src/lib.rs`, thêm `mod m20260817_000001_auth_teardown;` sau
`mod m20260815_000001_mysql_timestamp_precision;`, và thêm
`Box::new(m20260817_000001_auth_teardown::Migration),` ngay **trên** dòng
`// inject-above (do not remove this comment)`.

- [x] **Step 5: Chạy migration và sinh lại entity**

```bash
DB_TYPE=postgres cargo loco db reset
DB_TYPE=postgres cargo loco db entities
```

Ràng buộc CLAUDE.md: `db entities` phải chạy đối với Postgres. Chạy trên MySQL
hoặc SQLite sẽ ra kiểu cột khác và làm hỏng model.

Sau lệnh này `src/models/_entities/users.rs` mất 7 field và có thêm
`pub must_change_password: bool`.

- [x] **Step 6: Sửa fixture**

`src/fixtures/users.yaml` — bỏ mọi khoá của cột đã xoá (fixture hiện tại không có
khoá nào trong số đó, kiểm lại cho chắc), thêm `must_change_password: false` vào
cả hai bản ghi:

```yaml
---
- id: 1
  pid: 11111111-1111-1111-1111-111111111111
  email: user1@example.com
  password: "$argon2id$v=19$m=19456,t=2,p=1$ETQBx4rTgNAZhSaeYZKOZg$eYTdH26CRT6nUJtacLDEboP0li6xUwUF/q5nSlQ8uuc"
  api_key: lo-95ec80d7-cb60-4b70-9b4b-9ef74cb88758
  name: user1
  role: user
  max_bytes: 0
  used_bytes: 0
  reserved_bytes: 0
  must_change_password: false
  created_at: "2023-11-12T12:34:56.789Z"
  updated_at: "2023-11-12T12:34:56.789Z"
- id: 2
  pid: 22222222-2222-2222-2222-222222222222
  email: user2@example.com
  password: "$argon2id$v=19$m=19456,t=2,p=1$ETQBx4rTgNAZhSaeYZKOZg$eYTdH26CRT6nUJtacLDEboP0li6xUwUF/q5nSlQ8uuc"
  api_key: lo-153561ca-fa84-4e1b-813a-c62526d0a77e
  name: user2
  role: user
  max_bytes: 0
  used_bytes: 0
  reserved_bytes: 0
  must_change_password: false
  created_at: "2023-11-12T12:34:56.789Z"
  updated_at: "2023-11-12T12:34:56.789Z"
```

- [x] **Step 7: Nối view vào cột thật**

`src/views/auth.rs`:

```rust
use serde::{Deserialize, Serialize};

use crate::models::_entities::users;

#[derive(Debug, Deserialize, Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub pid: String,
    pub name: String,
    pub must_change_password: bool,
}

impl LoginResponse {
    #[must_use]
    pub fn new(user: &users::Model, token: &str) -> Self {
        Self {
            token: token.to_string(),
            pid: user.pid.to_string(),
            name: user.name.clone(),
            must_change_password: user.must_change_password,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CurrentResponse {
    pub pid: String,
    pub name: String,
    pub email: String,
    pub role: String,
    pub max_bytes: i64,
    pub must_change_password: bool,
}

impl CurrentResponse {
    #[must_use]
    pub fn new(user: &users::Model) -> Self {
        Self {
            pid: user.pid.to_string(),
            name: user.name.clone(),
            email: user.email.clone(),
            role: user.role.clone(),
            max_bytes: user.max_bytes,
            must_change_password: user.must_change_password,
        }
    }
}
```

- [x] **Step 8: Chạy test trên cả ba backend**

```bash
cargo test 2>&1 | tail -5
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test 2>&1 | tail -5
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test 2>&1 | tail -5
```

Expected: PASS cả ba. Snapshot của `can_get_current_user` lệch vì thêm field —
`cargo insta review` để duyệt.

- [x] **Step 9: Commit**

```bash
git add migration/ src/models/_entities/ src/fixtures/ src/views/ tests/
git commit -m "feat(db): drop mail-flow columns, add users.must_change_password

Seven columns existed only for email verification, magic link and password
reset. Replaced by a single flag that forces a password change on first login
after an admin creates the account."
```

---

## Task 3: Tách `Caller`, thêm `AdminCaller` và cổng đổi mật khẩu

**Files:**
- Modify: `src/controllers/api.rs:20-46` (khối `Caller`)
- Modify: `src/models/users.rs`
- Test: `tests/requests/api.rs`

**Interfaces:**
- Consumes: `users::Model.must_change_password` (task 2), `users::Model::is_admin()`.
- Produces:
  - `pub struct RawCaller { pub user: users::Model }` — giải JWT hoặc PAT, không kiểm cờ.
  - `pub struct Caller { pub user: users::Model }` — như trên cộng kiểm cờ, trả `403 password_change_required` khi cờ bật.
  - `pub struct AdminCaller { pub user: users::Model }` — `Caller` cộng kiểm `is_admin()`, trả `403 admin_required`.
  - `users::ActiveModel::set_password(db, password, must_change) -> ModelResult<Model>`.

- [x] **Step 1: Viết test cho ba tầng extractor**

Thêm vào `tests/requests/api.rs`:

```rust
#[tokio::test]
#[serial]
async fn caller_is_blocked_until_temp_password_is_changed() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;

        // Flip the flag the way an admin-created account would arrive.
        let mut am: users::ActiveModel = user.user.clone().into();
        am.must_change_password = ActiveValue::set(true);
        am.update(&ctx.db).await.unwrap();

        let (k, v) = prepare_data::auth_header(&user.token);
        let blocked = request.get("/api/keys").add_header(k, v).await;
        assert_eq!(blocked.status_code(), 403);
        assert!(blocked.text().contains("password_change_required"));

        // The change-password endpoint itself must stay reachable.
        let (k, v) = prepare_data::auth_header(&user.token);
        let allowed = request
            .post("/api/me/password")
            .add_header(k, v)
            .json(&serde_json::json!({
                "current_password": "12341234",
                "new_password": "a-much-better-secret"
            }))
            .await;
        assert_eq!(allowed.status_code(), 200);

        // And after changing it, everything else opens up again.
        let login = request
            .post("/api/auth/login")
            .json(&serde_json::json!({
                "email": "test@loco.com",
                "password": "a-much-better-secret"
            }))
            .await;
        let fresh: LoginResponse = serde_json::from_str(&login.text()).unwrap();
        assert!(!fresh.must_change_password);

        let (k, v) = prepare_data::auth_header(&fresh.token);
        let ok = request.get("/api/keys").add_header(k, v).await;
        assert_eq!(ok.status_code(), 200);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn change_password_rejects_wrong_current_password() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;
        let (k, v) = prepare_data::auth_header(&user.token);

        let res = request
            .post("/api/me/password")
            .add_header(k, v)
            .json(&serde_json::json!({
                "current_password": "not-the-password",
                "new_password": "a-much-better-secret"
            }))
            .await;

        assert_eq!(res.status_code(), 401);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn change_password_rejects_short_password() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;
        let (k, v) = prepare_data::auth_header(&user.token);

        let res = request
            .post("/api/me/password")
            .add_header(k, v)
            .json(&serde_json::json!({
                "current_password": "12341234",
                "new_password": "short"
            }))
            .await;

        assert_eq!(res.status_code(), 400);
    })
    .await;
}
```

Thêm import đầu file `tests/requests/api.rs` nếu chưa có:

```rust
use object_storage_gate::{models::_entities::users, views::auth::LoginResponse};
use sea_orm::ActiveValue;
```

- [x] **Step 2: Chạy để chắc nó fail**

Run: `cargo test --test mod requests::api 2>&1 | tail -20`
Expected: FAIL — `/api/me/password` trả 404, chưa có route.

- [x] **Step 3: Thêm `set_password` vào model**

Trong `src/models/users.rs`, `impl ActiveModel`, thay `reset_password` bằng:

```rust
    /// Replaces the password hash and sets whether the user must change it at next login.
    /// An admin-issued temporary password passes `must_change = true`; a self-service change passes `false`.
    ///
    /// # Errors
    ///
    /// when has DB query error or could not hash the given password
    pub async fn set_password(
        mut self,
        db: &DatabaseConnection,
        password: &str,
        must_change: bool,
    ) -> ModelResult<Model> {
        self.password =
            ActiveValue::set(hash::hash_password(password).map_err(|e| ModelError::Any(e.into()))?);
        self.must_change_password = ActiveValue::set(must_change);
        self.update(db).await.map_err(ModelError::from)
    }
```

Thêm hằng và hàm kiểm độ dài vào `impl Model` cùng file:

```rust
/// Shortest password the API will accept.
/// The starter allowed four characters, which is not a password.
pub const MIN_PASSWORD_LEN: usize = 8;

/// Longest password the API will accept, so a multi-megabyte body cannot reach Argon2.
pub const MAX_PASSWORD_LEN: usize = 256;

/// Validates a password before it is hashed.
///
/// # Errors
///
/// Returns a message error when the password is too short or too long.
pub fn validate_password(password: &str) -> ModelResult<()> {
    if password.len() < MIN_PASSWORD_LEN {
        return Err(ModelError::msg("password must be at least 8 characters"));
    }
    if password.len() > MAX_PASSWORD_LEN {
        return Err(ModelError::msg("password must be at most 256 characters"));
    }
    Ok(())
}
```

Đặt `validate_password` ở phạm vi module (ngoài `impl`), cạnh `LoginParams`.

- [x] **Step 4: Tách extractor trong `src/controllers/api.rs`**

Thay khối `Caller` hiện tại (dòng 20–46) bằng:

```rust
/// Whoever is calling, already resolved to a user, with no policy applied.
///
/// Only the change-password endpoint uses this directly: a user holding a temporary password must be able to replace it while every other endpoint is closed to them.
pub struct RawCaller {
    pub user: users::Model,
}

impl<S> FromRequestParts<S> for RawCaller
where
    AppContext: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Error> {
        let ctx = AppContext::from_ref(state);

        // JWT first: it verifies from the signature alone, no DB round trip.
        if let Ok(jwt) = auth::JWT::from_request_parts(parts, state).await {
            let user = users::Model::find_by_pid(&ctx.db, &jwt.claims.pid)
                .await
                .map_err(|_| Error::Unauthorized("user not found".to_string()))?;
            return Ok(Self { user });
        }

        let token = auth::ApiToken::<users::Model>::from_request_parts(parts, state).await?;
        Ok(Self { user: token.user })
    }
}

/// A caller who is allowed to use the account API.
///
/// A console session (JWT) and a service token (PAT) reach the same endpoints with the same powers: the console could already create, rotate and revoke keys over JWT, so refusing JWT on a separate management tree would have fenced off nothing.
/// A user still holding an admin-issued temporary password is refused here until they change it.
pub struct Caller {
    pub user: users::Model,
}

impl<S> FromRequestParts<S> for Caller
where
    AppContext: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Error> {
        let RawCaller { user } = RawCaller::from_request_parts(parts, state).await?;
        if user.must_change_password {
            return Err(Error::CustomError(
                StatusCode::FORBIDDEN,
                ErrorDetail::new(
                    "password_change_required",
                    "change the temporary password before using the API",
                ),
            ));
        }
        Ok(Self { user })
    }
}

/// A caller who is additionally an admin.
///
/// This is the only server-side admin gate; the console's role check is a UX affordance and must never be the model.
pub struct AdminCaller {
    pub user: users::Model,
}

impl<S> FromRequestParts<S> for AdminCaller
where
    AppContext: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Error> {
        let Caller { user } = Caller::from_request_parts(parts, state).await?;
        if !user.is_admin() {
            return Err(Error::CustomError(
                StatusCode::FORBIDDEN,
                ErrorDetail::new("admin_required", "this endpoint requires an admin account"),
            ));
        }
        Ok(Self { user })
    }
}
```

Thêm import đầu file:

```rust
use axum::http::StatusCode;
use loco_rs::controller::ErrorDetail;
```

- [x] **Step 5: Thêm endpoint đổi mật khẩu**

Vẫn trong `src/controllers/api.rs`, thêm params, handler và route:

```rust
#[derive(Debug, Deserialize, Serialize)]
pub struct ChangePasswordParams {
    pub current_password: String,
    pub new_password: String,
}

/// Lets a user replace their own password, including the temporary one an admin issued.
/// Uses `RawCaller` on purpose: this is the one endpoint that stays open while `must_change_password` is set.
#[debug_handler]
async fn change_password(
    caller: RawCaller,
    State(ctx): State<AppContext>,
    Json(params): Json<ChangePasswordParams>,
) -> Result<Response> {
    if !caller.user.verify_password(&params.current_password) {
        return Err(Error::Unauthorized("current password is wrong".to_string()));
    }
    users::validate_password(&params.new_password).map_err(|e| Error::BadRequest(e.to_string()))?;

    let am: users::ActiveModel = caller.user.into();
    am.set_password(&ctx.db, &params.new_password, false).await?;

    format::json(())
}
```

Trong `routes()`, thêm `.add("/me/password", post(change_password))`.

Import `users` ở đầu file phải trỏ tới module logic chứ không phải entity, vì
`validate_password` và `set_password` nằm ở đó:

```rust
use crate::{
    models::{_entities::users as users_entity, access_keys, buckets, users},
    ...
};
```

Chỗ nào đang dùng `users::Model` cho entity thì đổi sang `users::Model` của module
logic — nó re-export `pub use super::_entities::users::{self, ActiveModel, Entity, Model};`
nên cùng một kiểu, không cần alias. Bỏ `users_entity` nếu clippy báo thừa.

- [x] **Step 6: Chạy test**

Run: `cargo test --test mod requests::api 2>&1 | tail -20`
Expected: PASS cả ba test mới.

- [x] **Step 7: Clippy và commit**

```bash
cargo clippy --all-targets 2>&1 | tail -20
git add src/ tests/
git commit -m "feat(api): split Caller into RawCaller/Caller/AdminCaller

Caller now refuses any request from a user still holding an admin-issued
temporary password, and AdminCaller is the first server-side admin gate.
Adds POST /api/me/password, the one endpoint reachable while the flag is set."
```

---

## Task 4: Controller admin — CRUD user

**Files:**
- Create: `src/controllers/admin.rs`
- Create: `src/views/admin.rs`
- Create: `tests/requests/admin.rs`
- Modify: `src/controllers/mod.rs`, `src/views/mod.rs`, `src/app.rs:67-71`, `tests/requests/mod.rs`
- Modify: `src/models/users.rs`

**Interfaces:**
- Consumes: `AdminCaller` (task 3), `users::validate_password` (task 3), `users::ActiveModel::set_password` (task 3).
- Produces: 6 route dưới `/api/admin`. `users::Model::create_by_admin(db, &CreateUserParams) -> ModelResult<Model>`, `users::Model::list_all(db) -> ModelResult<Vec<Model>>`.

- [x] **Step 1: Viết test**

Tạo `tests/requests/admin.rs`:

```rust
use loco_rs::testing::prelude::*;
use object_storage_gate::{app::App, models::users, views::auth::LoginResponse};
use serial_test::serial;

use super::prepare_data;

/// Logs in the seeded admin, then creates a plain user through the admin API.
async fn admin_token(request: &loco_rs::TestServer, ctx: &loco_rs::app::AppContext) -> String {
    let admin = prepare_data::init_user_login(request, ctx).await;
    admin.token
}

#[tokio::test]
#[serial]
async fn admin_can_create_a_user_who_must_change_password() {
    request::<App, _, _>(|request, ctx| async move {
        let token = admin_token(&request, &ctx).await;
        let (k, v) = prepare_data::auth_header(&token);

        let res = request
            .post("/api/admin/users")
            .add_header(k, v)
            .json(&serde_json::json!({
                "email": "tenant@congty.vn",
                "name": "Tenant One",
                "password": "temp-password-1",
                "role": "user",
                "max_bytes": 10_737_418_240i64
            }))
            .await;

        assert_eq!(res.status_code(), 200);

        let created = users::Model::find_by_email(&ctx.db, "tenant@congty.vn")
            .await
            .unwrap();
        assert!(created.must_change_password);
        assert_eq!(created.role, "user");
        assert_eq!(created.max_bytes, 10_737_418_240);

        // The response must never carry the password back.
        assert!(!res.text().contains("temp-password-1"));

        // And the new user can log in, but is told to change the password.
        let login = request
            .post("/api/auth/login")
            .json(&serde_json::json!({
                "email": "tenant@congty.vn",
                "password": "temp-password-1"
            }))
            .await;
        let body: LoginResponse = serde_json::from_str(&login.text()).unwrap();
        assert!(body.must_change_password);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn create_user_requires_max_bytes() {
    request::<App, _, _>(|request, ctx| async move {
        let token = admin_token(&request, &ctx).await;
        let (k, v) = prepare_data::auth_header(&token);

        let res = request
            .post("/api/admin/users")
            .add_header(k, v)
            .json(&serde_json::json!({
                "email": "nobody@congty.vn",
                "name": "No Quota",
                "password": "temp-password-1",
                "role": "user"
            }))
            .await;

        assert_eq!(res.status_code(), 400);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn create_user_rejects_short_password_and_duplicate_email() {
    request::<App, _, _>(|request, ctx| async move {
        let token = admin_token(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&token);
        let short = request
            .post("/api/admin/users")
            .add_header(k, v)
            .json(&serde_json::json!({
                "email": "a@congty.vn", "name": "A",
                "password": "short", "role": "user", "max_bytes": 0
            }))
            .await;
        assert_eq!(short.status_code(), 400);

        let (k, v) = prepare_data::auth_header(&token);
        let first = request
            .post("/api/admin/users")
            .add_header(k, v)
            .json(&serde_json::json!({
                "email": "b@congty.vn", "name": "B",
                "password": "temp-password-1", "role": "user", "max_bytes": 0
            }))
            .await;
        assert_eq!(first.status_code(), 200);

        let (k, v) = prepare_data::auth_header(&token);
        let dup = request
            .post("/api/admin/users")
            .add_header(k, v)
            .json(&serde_json::json!({
                "email": "b@congty.vn", "name": "B again",
                "password": "temp-password-1", "role": "user", "max_bytes": 0
            }))
            .await;
        assert_eq!(dup.status_code(), 400);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn create_user_rejects_unknown_role() {
    request::<App, _, _>(|request, ctx| async move {
        let token = admin_token(&request, &ctx).await;
        let (k, v) = prepare_data::auth_header(&token);

        let res = request
            .post("/api/admin/users")
            .add_header(k, v)
            .json(&serde_json::json!({
                "email": "c@congty.vn", "name": "C",
                "password": "temp-password-1", "role": "superuser", "max_bytes": 0
            }))
            .await;

        assert_eq!(res.status_code(), 400);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn non_admin_is_refused_on_every_admin_route() {
    request::<App, _, _>(|request, ctx| async move {
        let token = admin_token(&request, &ctx).await;

        // Create a plain user, log in as them.
        let (k, v) = prepare_data::auth_header(&token);
        request
            .post("/api/admin/users")
            .add_header(k, v)
            .json(&serde_json::json!({
                "email": "plain@congty.vn", "name": "Plain",
                "password": "temp-password-1", "role": "user", "max_bytes": 0
            }))
            .await;

        let login = request
            .post("/api/auth/login")
            .json(&serde_json::json!({
                "email": "plain@congty.vn", "password": "temp-password-1"
            }))
            .await;
        let body: LoginResponse = serde_json::from_str(&login.text()).unwrap();

        // Clear the temp-password gate so the 403 we assert is the admin gate, not the password gate.
        let (k, v) = prepare_data::auth_header(&body.token);
        request
            .post("/api/me/password")
            .add_header(k, v)
            .json(&serde_json::json!({
                "current_password": "temp-password-1",
                "new_password": "plain-user-secret"
            }))
            .await;

        let login = request
            .post("/api/auth/login")
            .json(&serde_json::json!({
                "email": "plain@congty.vn", "password": "plain-user-secret"
            }))
            .await;
        let body: LoginResponse = serde_json::from_str(&login.text()).unwrap();

        let (k, v) = prepare_data::auth_header(&body.token);
        let res = request.get("/api/admin/users").add_header(k, v).await;
        assert_eq!(res.status_code(), 403);
        assert!(res.text().contains("admin_required"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn admin_can_reset_a_user_password_and_it_forces_a_change() {
    request::<App, _, _>(|request, ctx| async move {
        let token = admin_token(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&token);
        request
            .post("/api/admin/users")
            .add_header(k, v)
            .json(&serde_json::json!({
                "email": "forgot@congty.vn", "name": "Forgot",
                "password": "temp-password-1", "role": "user", "max_bytes": 0
            }))
            .await;

        let target = users::Model::find_by_email(&ctx.db, "forgot@congty.vn")
            .await
            .unwrap();

        // The user changes it once, so must_change_password is false.
        let login = request
            .post("/api/auth/login")
            .json(&serde_json::json!({
                "email": "forgot@congty.vn", "password": "temp-password-1"
            }))
            .await;
        let body: LoginResponse = serde_json::from_str(&login.text()).unwrap();
        let (k, v) = prepare_data::auth_header(&body.token);
        request
            .post("/api/me/password")
            .add_header(k, v)
            .json(&serde_json::json!({
                "current_password": "temp-password-1",
                "new_password": "chosen-by-the-user"
            }))
            .await;

        // Admin resets it again.
        let (k, v) = prepare_data::auth_header(&token);
        let res = request
            .post(&format!("/api/admin/users/{}/password", target.pid))
            .add_header(k, v)
            .json(&serde_json::json!({ "password": "issued-again-1" }))
            .await;
        assert_eq!(res.status_code(), 200);

        let after = users::Model::find_by_email(&ctx.db, "forgot@congty.vn")
            .await
            .unwrap();
        assert!(after.must_change_password);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn admin_cannot_delete_the_last_admin() {
    request::<App, _, _>(|request, ctx| async move {
        let token = admin_token(&request, &ctx).await;
        let me = users::Model::find_by_email(&ctx.db, "test@loco.com")
            .await
            .unwrap();

        let (k, v) = prepare_data::auth_header(&token);
        let res = request
            .delete(&format!("/api/admin/users/{}", me.pid))
            .add_header(k, v)
            .await;

        assert_eq!(res.status_code(), 400);
    })
    .await;
}
```

Thêm `mod admin;` vào `tests/requests/mod.rs`.

- [x] **Step 2: Chạy để chắc nó fail**

Run: `cargo test --test mod requests::admin 2>&1 | tail -20`
Expected: FAIL — mọi route `/api/admin/*` trả 404.

- [x] **Step 3: Thêm model helper**

Trong `src/models/users.rs`, phạm vi module, thêm params:

```rust
#[derive(Debug, Deserialize, Serialize)]
pub struct CreateUserParams {
    pub email: String,
    pub name: String,
    pub password: String,
    pub role: String,
    /// Required on purpose: `0` means unlimited, and unlimited must be a decision, never a default.
    pub max_bytes: i64,
}

/// Validates a role string against the two roles the system knows.
///
/// # Errors
///
/// Returns a message error for anything else.
pub fn validate_role(role: &str) -> ModelResult<()> {
    if role == ROLE_ADMIN || role == ROLE_USER {
        return Ok(());
    }
    Err(ModelError::msg("role must be 'admin' or 'user'"))
}
```

Trong `impl Model`:

```rust
    /// Creates a user on an admin's behalf, with a temporary password the user must replace at first login.
    ///
    /// # Errors
    ///
    /// When the email is taken, the role or password is invalid, or the DB write fails
    pub async fn create_by_admin(
        db: &DatabaseConnection,
        params: &CreateUserParams,
    ) -> ModelResult<Self> {
        validate_role(&params.role)?;
        validate_password(&params.password)?;
        if params.max_bytes < 0 {
            return Err(ModelError::msg("max_bytes must not be negative"));
        }

        let txn = db.begin().await?;

        if users::Entity::find()
            .filter(
                model::query::condition()
                    .eq(users::Column::Email, &params.email)
                    .build(),
            )
            .one(&txn)
            .await?
            .is_some()
        {
            return Err(ModelError::EntityAlreadyExists {});
        }

        let password_hash =
            hash::hash_password(&params.password).map_err(|e| ModelError::Any(e.into()))?;
        let user = users::ActiveModel {
            email: ActiveValue::set(params.email.clone()),
            password: ActiveValue::set(password_hash),
            name: ActiveValue::set(params.name.clone()),
            role: ActiveValue::set(params.role.clone()),
            max_bytes: ActiveValue::set(params.max_bytes),
            must_change_password: ActiveValue::set(true),
            ..Default::default()
        }
        .insert(&txn)
        .await?;

        txn.commit().await?;

        Ok(user)
    }

    /// Lists every user, newest first.
    ///
    /// # Errors
    ///
    /// When the query fails
    pub async fn list_all(db: &DatabaseConnection) -> ModelResult<Vec<Self>> {
        Ok(users::Entity::find()
            .order_by_desc(users::Column::Id)
            .all(db)
            .await?)
    }

    /// Counts admins, so the last one cannot be demoted or deleted.
    ///
    /// # Errors
    ///
    /// When the query fails
    pub async fn admin_count(db: &DatabaseConnection) -> ModelResult<u64> {
        Ok(users::Entity::find()
            .filter(
                model::query::condition()
                    .eq(users::Column::Role, ROLE_ADMIN)
                    .build(),
            )
            .count(db)
            .await?)
    }
```

Thêm import `use sea_orm::{PaginatorTrait, QueryOrder};` đầu file (loco prelude
không kéo `count` và `order_by_desc` vào).

- [x] **Step 4: Viết view**

Tạo `src/views/admin.rs`:

```rust
use serde::{Deserialize, Serialize};

use crate::models::_entities::users;

/// The admin-facing shape of a user.
/// Lists fields by hand so a new column never leaks into the API by accident — the password hash and the PAT both live on this model.
#[derive(Debug, Deserialize, Serialize)]
pub struct AdminUserResponse {
    pub pid: String,
    pub email: String,
    pub name: String,
    pub role: String,
    pub max_bytes: i64,
    pub used_bytes: i64,
    pub reserved_bytes: i64,
    pub must_change_password: bool,
    pub created_at: String,
}

impl AdminUserResponse {
    #[must_use]
    pub fn new(user: &users::Model) -> Self {
        Self {
            pid: user.pid.to_string(),
            email: user.email.clone(),
            name: user.name.clone(),
            role: user.role.clone(),
            max_bytes: user.max_bytes,
            used_bytes: user.used_bytes,
            reserved_bytes: user.reserved_bytes,
            must_change_password: user.must_change_password,
            created_at: user.created_at.to_rfc3339(),
        }
    }
}
```

Thêm `pub mod admin;` vào `src/views/mod.rs`.

- [x] **Step 5: Viết controller**

Tạo `src/controllers/admin.rs`:

```rust
//! Admin-only user management.
//!
//! Self-registration was removed; this tree is the only way an account comes into existence after first-run setup.
//! Every handler takes `AdminCaller`, which is the server-side gate — the console's role check is a UX affordance only.
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    controllers::api::AdminCaller,
    models::users,
    views::admin::AdminUserResponse,
};

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateUserParams {
    pub name: Option<String>,
    pub role: Option<String>,
    pub max_bytes: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SetPasswordParams {
    pub password: String,
}

fn bad_request(e: &ModelError) -> Error {
    Error::BadRequest(e.to_string())
}

async fn load(db: &DatabaseConnection, pid: &str) -> Result<users::Model> {
    users::Model::find_by_pid(db, pid)
        .await
        .map_err(|_| Error::NotFound)
}

#[debug_handler]
async fn list(_admin: AdminCaller, State(ctx): State<AppContext>) -> Result<Response> {
    let rows = users::Model::list_all(&ctx.db).await?;
    format::json(
        rows.iter()
            .map(AdminUserResponse::new)
            .collect::<Vec<_>>(),
    )
}

#[debug_handler]
async fn create(
    _admin: AdminCaller,
    State(ctx): State<AppContext>,
    Json(params): Json<users::CreateUserParams>,
) -> Result<Response> {
    let user = users::Model::create_by_admin(&ctx.db, &params)
        .await
        .map_err(|e| bad_request(&e))?;
    format::json(AdminUserResponse::new(&user))
}

#[debug_handler]
async fn show(
    _admin: AdminCaller,
    Path(pid): Path<String>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    let user = load(&ctx.db, &pid).await?;
    format::json(AdminUserResponse::new(&user))
}

#[debug_handler]
async fn update(
    _admin: AdminCaller,
    Path(pid): Path<String>,
    State(ctx): State<AppContext>,
    Json(params): Json<UpdateUserParams>,
) -> Result<Response> {
    let db = &ctx.db;
    let user = load(db, &pid).await?;

    // Demoting the last admin would lock everyone out of this tree.
    if let Some(role) = &params.role {
        users::validate_role(role).map_err(|e| bad_request(&e))?;
        if user.is_admin() && role != users::ROLE_ADMIN && users::Model::admin_count(db).await? <= 1
        {
            return Err(Error::BadRequest(
                "cannot demote the last admin".to_string(),
            ));
        }
    }
    if let Some(max_bytes) = params.max_bytes {
        if max_bytes < 0 {
            return Err(Error::BadRequest("max_bytes must not be negative".to_string()));
        }
    }

    let mut am: users::ActiveModel = user.into();
    if let Some(name) = &params.name {
        am.name = ActiveValue::set(name.clone());
    }
    if let Some(role) = &params.role {
        am.role = ActiveValue::set(role.clone());
    }
    if let Some(max_bytes) = params.max_bytes {
        am.max_bytes = ActiveValue::set(max_bytes);
    }
    let updated = am.update(db).await?;

    format::json(AdminUserResponse::new(&updated))
}

/// Issues a new temporary password and forces the user to replace it at next login.
/// This replaces the removed self-service password-reset flow.
#[debug_handler]
async fn set_password(
    _admin: AdminCaller,
    Path(pid): Path<String>,
    State(ctx): State<AppContext>,
    Json(params): Json<SetPasswordParams>,
) -> Result<Response> {
    let db = &ctx.db;
    let user = load(db, &pid).await?;
    users::validate_password(&params.password).map_err(|e| bad_request(&e))?;

    let am: users::ActiveModel = user.into();
    am.set_password(db, &params.password, true).await?;

    format::json(())
}

#[debug_handler]
async fn destroy(
    admin: AdminCaller,
    Path(pid): Path<String>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    let db = &ctx.db;
    let user = load(db, &pid).await?;

    if user.id == admin.user.id {
        return Err(Error::BadRequest("cannot delete your own account".to_string()));
    }
    if user.is_admin() && users::Model::admin_count(db).await? <= 1 {
        return Err(Error::BadRequest("cannot delete the last admin".to_string()));
    }

    // ponytail: buckets are ON DELETE SET NULL, so deleting an owner would turn their private bucket into a system pool along with its encrypted upstream credentials.
    // Refuse instead of leaking; P3 fixes the cascade and this guard can then become a cascading delete.
    let owned = buckets::Model::list_for_user(db, user.id).await?;
    if !owned.is_empty() {
        return Err(Error::BadRequest(
            "delete or reassign this user's buckets first".to_string(),
        ));
    }

    let am: users::ActiveModel = user.into();
    am.delete(db).await?;

    format::json(())
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/admin")
        .add("/users", get(list).post(create))
        .add("/users/{pid}", get(show).patch(update).delete(destroy))
        .add("/users/{pid}/password", post(set_password))
}
```

Thêm `use crate::models::buckets;` vào khối import.

- [x] **Step 6: Đăng ký controller**

`src/controllers/mod.rs`:

```rust
pub mod admin;
pub mod api;
pub mod auth;
```

`src/app.rs`, trong `routes()`:

```rust
        AppRoutes::with_default_routes() // controller routes below
            .add_route(controllers::auth::routes())
            .add_route(controllers::api::routes())
            .add_route(controllers::admin::routes())
```

- [x] **Step 7: Chạy test**

Run: `cargo test --test mod requests::admin 2>&1 | tail -30`
Expected: PASS cả 7 test.

Run: `cargo loco routes | grep admin`
Expected: 6 dòng, đúng như bảng trong spec.

- [x] **Step 8: Chạy toàn bộ trên ba backend**

```bash
cargo test 2>&1 | tail -5
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test 2>&1 | tail -5
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test 2>&1 | tail -5
```

- [x] **Step 9: Clippy và commit**

```bash
cargo clippy --all-targets 2>&1 | tail -20
git add src/ tests/
git commit -m "feat(admin): add admin user management API

Six routes under /api/admin, every one gated by AdminCaller. Creating a user
issues a temporary password and sets must_change_password. Guards the last
admin against demotion and deletion, and refuses to delete an owner while
their buckets would fall back to the system pool."
```

---

## Task 5: Console — bỏ màn đăng ký và luồng mail, thêm màn đổi mật khẩu

**Files:**
- Delete: `frontend/src/routes/_auth/register.tsx`, `forgot.tsx`, `reset.tsx`, `magic-link.tsx`, `verify.$token.tsx`
- Create: `frontend/src/routes/_app/change-password.tsx`
- Modify: `frontend/src/lib/auth.ts`, `frontend/src/lib/auth.test.ts`
- Modify: `frontend/src/routes/_auth/login.tsx`, `frontend/src/routes/_app.tsx`

**Interfaces:**
- Consumes: `POST /api/me/password` (task 3), `LoginResponse.must_change_password` và `CurrentUser.must_change_password` (task 2).
- Produces: `changePassword(current, next): Promise<void>` trong `lib/auth.ts`; route `/change-password`.

- [x] **Step 1: Viết test cho lib**

Thêm vào `frontend/src/lib/auth.test.ts`:

```ts
import { describe, expect, it, vi } from "vitest";
import { changePassword, setToken } from "./auth";

describe("changePassword", () => {
  it("posts current and new password with the bearer token", async () => {
    setToken("tok-123");
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(new Response("", { status: 200 }));

    await changePassword("old-one", "new-one-please");

    const [path, init] = fetchMock.mock.calls[0];
    expect(path).toBe("/api/me/password");
    expect(init?.method).toBe("POST");
    expect(JSON.parse(init?.body as string)).toEqual({
      current_password: "old-one",
      new_password: "new-one-please",
    });
    expect(new Headers(init?.headers).get("Authorization")).toBe("Bearer tok-123");

    fetchMock.mockRestore();
  });
});
```

Xoá mọi test trong file này nhắc tới `register`, `forgot`, `reset`, `magicLink`,
`verify`, `resendVerification`.

- [x] **Step 2: Chạy để chắc nó fail**

Run: `cd frontend && corepack pnpm vitest run src/lib/auth.test.ts`
Expected: FAIL — `changePassword` chưa tồn tại.

- [x] **Step 3: Sửa `frontend/src/lib/auth.ts`**

Xoá sáu hàm `register`, `forgot`, `reset`, `magicLink`, `resendVerification`,
`verify`. Sửa hai kiểu và thêm một hàm:

```ts
export type CurrentUser = {
  pid: string;
  name: string;
  email: string;
  role: "user" | "admin";
  max_bytes: number;
  must_change_password: boolean;
};

export type LoginResponse = {
  token: string;
  pid: string;
  name: string;
  must_change_password: boolean;
};

export const changePassword = (current_password: string, new_password: string) =>
  post<void>("/api/me/password", { current_password, new_password });
```

Sửa `api()` để 401 vừa xoá token vừa đưa người dùng về `/login` — hiện tại nó chỉ
xoá token, nên phiên hết hạn để lại một console dựng đầy đủ mà mọi thao tác đều
lặng lẽ hỏng:

```ts
  const res = await fetch(path, { ...init, headers });
  if (!res.ok) {
    if (res.status === 401) {
      clearToken();
      currentCache = null;
      if (globalThis.location?.pathname !== "/login") {
        globalThis.location.assign("/login");
      }
    }
    throw new ApiError(res.status, (await res.text()) || res.statusText);
  }
```

`currentCache` được khai báo bên dưới `api()` nên phải dời khai báo lên trên hàm.

- [x] **Step 4: Xoá 5 route file**

```bash
cd frontend
git rm src/routes/_auth/register.tsx \
       src/routes/_auth/forgot.tsx \
       src/routes/_auth/reset.tsx \
       src/routes/_auth/magic-link.tsx \
       src/routes/_auth/verify.\$token.tsx
```

- [x] **Step 5: Dọn `login.tsx`**

Bỏ ba liên kết trỏ tới route vừa xoá: "Quên mật khẩu", "Đăng nhập bằng email"
(magic link), và "Chưa có tài khoản? Đăng ký". Thay bằng một dòng tĩnh:

```tsx
<div style={{ textAlign: "center", marginTop: 16, fontSize: 13, color: "var(--dim)" }}>
  Tài khoản do quản trị viên cấp. Liên hệ quản trị viên nếu bạn quên mật khẩu.
</div>
```

- [x] **Step 6: Viết màn đổi mật khẩu**

Tạo `frontend/src/routes/_app/change-password.tsx`:

```tsx
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { ApiError, changePassword, currentCached } from "../../lib/auth";

export const Route = createFileRoute("/_app/change-password")({
  component: ChangePassword,
});

function ChangePassword() {
  const navigate = useNavigate();
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  async function submit() {
    if (busy) return;
    if (next.length < 8) {
      setError("Mật khẩu mới phải từ 8 ký tự.");
      return;
    }
    if (next !== confirm) {
      setError("Hai lần nhập không khớp.");
      return;
    }
    setBusy(true);
    setError("");
    try {
      await changePassword(current, next);
      // The cached user still carries must_change_password: true, so drop it.
      globalThis.location.assign("/");
    } catch (e) {
      setError(
        e instanceof ApiError && e.status === 401
          ? "Mật khẩu hiện tại không đúng."
          : "Không đổi được mật khẩu. Thử lại sau.",
      );
      setBusy(false);
    }
  }

  return (
    <div style={{ maxWidth: 380, margin: "48px auto", display: "flex", flexDirection: "column", gap: 15 }}>
      <div style={{ fontSize: 22, fontWeight: 600 }}>Đổi mật khẩu</div>
      <div style={{ fontSize: 13.5, color: "var(--dim)", lineHeight: 1.55 }}>
        Tài khoản đang dùng mật khẩu tạm do quản trị viên cấp. Đặt mật khẩu riêng
        để tiếp tục.
      </div>
      {error && (
        <div role="alert" style={{ fontSize: 13, color: "#FF9AA2" }}>
          {error}
        </div>
      )}
      <label style={{ display: "flex", flexDirection: "column", gap: 7, fontSize: 12, color: "var(--dim)" }}>
        Mật khẩu hiện tại
        <input
          type="password"
          autoComplete="current-password"
          value={current}
          onChange={(e) => setCurrent(e.target.value)}
        />
      </label>
      <label style={{ display: "flex", flexDirection: "column", gap: 7, fontSize: 12, color: "var(--dim)" }}>
        Mật khẩu mới
        <input
          type="password"
          autoComplete="new-password"
          value={next}
          onChange={(e) => setNext(e.target.value)}
        />
      </label>
      <label style={{ display: "flex", flexDirection: "column", gap: 7, fontSize: 12, color: "var(--dim)" }}>
        Nhập lại mật khẩu mới
        <input
          type="password"
          autoComplete="new-password"
          value={confirm}
          onChange={(e) => setConfirm(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && void submit()}
        />
      </label>
      <button type="button" disabled={busy} onClick={() => void submit()}>
        {busy ? "Đang đổi…" : "Đổi mật khẩu"}
      </button>
    </div>
  );
}

// Referenced so the import is not flagged; the guard in _app.tsx uses the same cache.
void currentCached;
```

Bỏ dòng `void currentCached;` và import tương ứng nếu không dùng tới.

- [x] **Step 7: Chặn ở `_app.tsx`**

Trong `beforeLoad` của `frontend/src/routes/_app.tsx`, sau khi lấy được user:

```tsx
    const user = await currentCached();
    const onChangePassword = location.pathname === "/change-password";
    if (user.must_change_password && !onChangePassword) {
      throw redirect({ to: "/change-password" });
    }
    if (!user.must_change_password && onChangePassword) {
      throw redirect({ to: "/" });
    }
```

Sidebar và Header không nên hiện khi đang bị ép đổi mật khẩu — bọc phần shell
trong `user.must_change_password ? <Outlet /> : <FullShell />`.

- [x] **Step 8: Chạy test, lint, typecheck**

```bash
cd frontend
corepack pnpm vitest run
corepack pnpm biome check src
npx tsc --noEmit
corepack pnpm build
```

Expected: tất cả sạch. `routeTree.gen.ts` được sinh lại trong bước build.

- [x] **Step 9: Commit**

```bash
git add -A frontend/
git commit -m "feat(console): drop registration and mail-recovery screens

Removes the register, forgot, reset, magic-link and verify routes together
with their API wrappers. Adds a forced change-password screen for accounts
created by an admin, and makes a 401 send the user back to /login instead of
leaving a rendered console where every action silently fails."
```

---

## Task 6: Đồng bộ tài liệu

**Files:**
- Modify: `README.md`
- Modify: `CLAUDE.md`
- Modify: `docs/superpowers/plans/2026-07-30-management-api.md`

**Interfaces:**
- Consumes: bề mặt API sau task 4.
- Produces: — (task cuối)

- [x] **Step 1: Cập nhật bảng trạng thái README**

Trong `README.md` mục Status, sửa dòng auth và thêm dòng admin:

```markdown
| JWT user auth — login, first-run setup, forced password change | **done** |
| Self-registration, email verification, magic link, password reset | **removed** (2026-08-17) — accounts are admin-created |
| Admin user management API | **done** (P1) |
```

Trong mục mô tả route, thay `POST /api/auth/register` bằng bảng route mới của
spec mục 3.

- [x] **Step 2: Cập nhật CLAUDE.md**

Mục "Status" của `CLAUDE.md` đang mô tả repo như "unmodified loco.rs SaaS
starter" — đã sai từ trước plan này. Thay bằng:

```markdown
## Status

Data foundation, management API và console SPA đã có. Toàn bộ tầng dữ liệu S3
gateway (SigV4, route S3, proxy, quota, audit) chưa tồn tại — xem
`docs/superpowers/plans/2026-08-17-go-live-roadmap.md`.

Tự đăng ký đã bị xoá (2026-08-17). Tài khoản chỉ sinh ra từ first-run setup hoặc
từ `/api/admin/users`. Không còn luồng mail nào; `src/mailers/` đã bị xoá.
```

Thêm vào mục "Constraints that bite":

```markdown
- **Không có luồng mail.** Verify email, magic link và password reset đã bị gỡ; `src/mailers/` không còn. Đừng thêm lại endpoint gửi mail mà chưa sửa block mailer trong `config/production.yaml` (thiếu `auth:`, nên SMTP thật không cấu hình được).
- **`AdminCaller` là cổng admin duy nhất phía server.** Kiểm `role` ở console chỉ là tiện ích UX. Mọi route `/api/admin/*` mới phải nhận `AdminCaller`.
```

- [x] **Step 3: Ghi chú supersession vào plan cũ**

Thêm vào đầu `docs/superpowers/plans/2026-07-30-management-api.md`, ngay dưới
tiêu đề:

```markdown
> **Cập nhật 2026-08-17:** `POST /api/auth/register` và toàn bộ luồng mail mà
> plan này giả định đã bị xoá. Xem
> `docs/superpowers/specs/2026-08-17-go-live-hardening-design.md`.
```

- [x] **Step 4: Commit**

```bash
git add README.md CLAUDE.md docs/
git commit -m "docs: record the removal of self-registration and mail flows"
```

---

## Self-review

**Phủ spec.** Mục 2.1 (xoá đăng ký) → task 1 + 5. Mục 2.2 (mật khẩu tạm) → task
2 + 3 + 4 + 5. Mục 2.3 (xoá mail) → task 1. Mục 2.4 (`max_bytes` tường minh) →
task 4, `CreateUserParams.max_bytes` không có serde default và có test riêng.
Mục 3 (bề mặt API) → task 1 xoá, task 3 và 4 thêm. Mục 4 (schema) → task 2.

**Chưa phủ, cố ý.** Blocker "không rate limit" thuộc P2, không thuộc plan này —
xoá đăng ký đã bỏ đi ba endpoint gửi mail nhưng `login` vẫn brute-force được.
`AdminCaller` giải blocker 9 nhưng màn admin trên console vẫn là mock cho tới P4.
PAT vẫn lưu plaintext (High, thuộc P2) — task 3 không đụng tới.

**Nhất quán kiểu.** `set_password(db, password, must_change)` được định nghĩa ở
task 3 và dùng ở task 3 (self-service, `false`) và task 4 (admin, `true`).
`validate_password` định nghĩa task 3, dùng task 3 và 4. `AdminCaller` định nghĩa
task 3 ở `controllers::api`, import ở task 4 từ `crate::controllers::api::AdminCaller`.
`CreateUserParams` định nghĩa ở `models::users` (task 4) và dùng làm body của
handler cùng task.

**Rủi ro đã biết.** Task 2 xoá cột trên bảng có dữ liệu. Trên môi trường đã có
user thật, chạy backup trước — `down()` phục hồi được cấu trúc nhưng không phục
hồi được dữ liệu trong bảy cột đó.
