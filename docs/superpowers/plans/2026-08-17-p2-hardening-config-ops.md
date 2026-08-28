# P2 — Siết config, deploy, CI — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Đóng bốn blocker vận hành (guard master key, rate limit, giới hạn body 2 MB, workflow publish không có gate) và các finding High cùng nhóm, để một lần deploy không tự mở lỗ hổng.

**Architecture:** Phần lớn là config và CI, không đụng logic nghiệp vụ. Ba chỗ có code thật: guard master key trong `app.rs`, middleware rate limit, và hash PAT. Mỗi task đứng độc lập — hỏng một task không chặn task khác — nên thứ tự dưới đây là thứ tự rủi ro giảm dần, không phải thứ tự phụ thuộc.

**Tech Stack:** Rust, loco-rs 0.16, Axum 0.8, tower-governor, GitHub Actions, Docker.

**Spec:** `docs/superpowers/specs/2026-08-17-go-live-hardening-design.md`

## Global Constraints

- Ba backend hạng nhất: Postgres, MySQL >= 8.0.13, SQLite. Cấm `ILIKE`, `RETURNING`, `ON CONFLICT`, `jsonb`, cột array, `pg_advisory_lock`, `FOR UPDATE SKIP LOCKED`.
- Migration dùng `ColType` + `SchemaManager`; raw SQL phải branch theo `m.get_database_backend()`.
- Cột `TIMESTAMP` mới khai `TIMESTAMP(6)` trên MySQL.
- `src/models/_entities/` generated từ Postgres, không sửa tay.
- Comment tiếng Anh, một câu một dòng.
- Không tự commit/push ngoài bước commit trong plan. Không AI attribution.
- Sau mỗi task, `cargo clippy --all-targets` phải sạch.

---

## File Structure

**Tạo mới:**
- `migration/src/m20260817_000002_hash_api_key.rs` — đổi `users.api_key` sang lưu hash

**Sửa:**
- `src/app.rs:54-61` — guard master key kiểm giá trị, không chỉ kiểm sự tồn tại
- `src/models/crypto.rs` — export `DEV_KEY_B64` để guard so sánh
- `src/models/users.rs` — PAT sinh + hash + tra bằng hash
- `src/controllers/api.rs` — `GET /api/token` biến mất, `POST /api/token/rotate` trả token một lần
- `config/production.yaml` — body limit, timeout DB, pool, SMTP auth, `SERVER_HOST`, secure headers, timeout request
- `config/development.yaml`, `config/test.yaml` — body limit cho khớp
- `.dockerignore` — thêm `.env`, `data/`
- `Dockerfile` — pin base image, thêm `HEALTHCHECK`
- `docker-compose.yml`, `docker-compose/postgres.yml`, `docker-compose/mysql.yml` — bỏ mật khẩu mặc định, bỏ mapping cổng DB, thêm healthcheck và restart
- `.github/workflows/ci.yaml` — pnpm thay npm, thêm vitest/biome/tsc, `--all-targets` cho clippy, thêm `cargo audit`
- `.github/workflows/docker.yaml` — gate sau CI, `latest` không dính RC
- `docs/docker.md` — sửa khẳng định sai về guard master key
- `Cargo.toml` — thêm `tower_governor`
- `frontend/biome.json` — ignore `dist/`

---

## Task 1: Guard master key kiểm giá trị thật

**Files:**
- Modify: `src/app.rs:54-61`
- Modify: `src/models/crypto.rs:14`
- Modify: `docs/docker.md:40-41`
- Test: `src/models/crypto.rs` (unit test cuối file)

**Interfaces:**
- Consumes: —
- Produces: `crypto::DEV_KEY_B64` thành `pub`. `crypto::validate_master_key(&str) -> Result<()>` — decode base64, kiểm 32 byte, từ chối key dev.

- [x] **Step 1: Viết test**

Thêm vào khối `mod tests` cuối `src/models/crypto.rs`:

```rust
    #[test]
    fn validate_rejects_the_dev_key() {
        assert!(validate_master_key(DEV_KEY_B64).is_err());
    }

    #[test]
    fn validate_rejects_bad_base64_and_wrong_length() {
        assert!(validate_master_key("not base64!!").is_err());
        assert!(validate_master_key("").is_err());
        // 31 bytes, one short.
        assert!(validate_master_key(&STANDARD.encode([7u8; 31])).is_err());
    }

    #[test]
    fn validate_accepts_a_real_key() {
        assert!(validate_master_key(&STANDARD.encode([7u8; 32])).is_ok());
    }
```

- [x] **Step 2: Chạy để chắc nó fail**

Run: `cargo test --lib crypto 2>&1 | tail -20`
Expected: FAIL biên dịch — `cannot find function 'validate_master_key'`.

- [x] **Step 3: Viết `validate_master_key`**

Trong `src/models/crypto.rs`, đổi `DEV_KEY_B64` thành `pub` và thêm hàm:

```rust
// ponytail: dev/test fallback key.
// Production MUST set OSG_MASTER_KEY to a base64-encoded 32-byte key — enforced in `app::App::after_context`, which refuses to start a production app whose key is missing, malformed, or equal to this one.
// Upgrade path: KMS-backed key if rotation needed.
pub const DEV_KEY_B64: &str = "ZGV2LW9ubHktMzJieXRlLW1hc3Rlci1rZXktMDEyMzQ=";

/// Checks a candidate master key before the process commits to it.
///
/// Called at boot rather than at first use: `master_key()` caches in a `OnceLock` and panics on a bad key, and a panic at first key creation is a much worse failure than a refused boot.
///
/// # Errors
///
/// Returns an error when the value is not valid base64, does not decode to exactly 32 bytes, or is the development key committed to this repository.
pub fn validate_master_key(b64: &str) -> Result<()> {
    let trimmed = b64.trim();
    if trimmed == DEV_KEY_B64 {
        return Err(Error::string(
            "OSG_MASTER_KEY is the development key committed to this repository; generate a new one with `openssl rand -base64 32`",
        ));
    }
    let bytes = STANDARD
        .decode(trimmed)
        .map_err(|_| Error::string("OSG_MASTER_KEY must be valid base64"))?;
    if bytes.len() != 32 {
        return Err(Error::string(
            "OSG_MASTER_KEY must decode to exactly 32 bytes",
        ));
    }
    Ok(())
}
```

- [x] **Step 4: Nối vào `after_context`**

`src/app.rs`, thay khối guard:

```rust
    // Refuse to start production with a missing, malformed, or publicly known master key: every access-key secret and backend-store credential would otherwise be encrypted at rest with a key anyone can read from git.
    // See `models::crypto`.
    // This hook (not `boot`) is the guard point because the loco CLI calls `create_app` directly and never goes through `Hooks::boot`.
    async fn after_context(ctx: AppContext) -> Result<AppContext> {
        if ctx.environment == Environment::Production {
            let key = std::env::var("OSG_MASTER_KEY").map_err(|_| {
                Error::string(
                    "OSG_MASTER_KEY must be set in production (base64-encoded 32-byte key)",
                )
            })?;
            crate::models::crypto::validate_master_key(&key)?;
        }
        Ok(ctx)
    }
```

- [x] **Step 5: Chạy test**

Run: `cargo test --lib crypto 2>&1 | tail -10`
Expected: PASS 7 test (4 cũ + 3 mới).

- [x] **Step 6: Kiểm bằng tay rằng boot thật sự bị từ chối**

```bash
LOCO_ENV=production DATABASE_URL=sqlite::memory: JWT_SECRET=x \
  OSG_MASTER_KEY=ZGV2LW9ubHktMzJieXRlLW1hc3Rlci1rZXktMDEyMzQ= \
  cargo run -- start 2>&1 | head -5
```

Expected: thoát với thông điệp "development key committed to this repository".

```bash
LOCO_ENV=production DATABASE_URL=sqlite::memory: JWT_SECRET=x \
  cargo loco db migrate 2>&1 | head -5
```

Expected: cũng bị từ chối — đây là cái xác nhận guard nằm đúng ở `after_context`
chứ không phải ở `boot`, nên CLI cũng dính.

- [x] **Step 7: Sửa tài liệu nói sai**

`docs/docker.md:40-41` đang khẳng định guard đã làm việc này từ trước. Giữ nguyên
câu, giờ nó mới thành đúng — nhưng thêm cách sinh key:

```markdown
Reusing the checked-in development key in production is refused by
`App::after_context`, cùng với key sai định dạng hoặc không đủ 32 byte. Sinh key
mới bằng `openssl rand -base64 32`.
```

- [x] **Step 8: Commit**

```bash
git add src/ docs/
git commit -m "fix(security): validate OSG_MASTER_KEY at boot, not at first use

The guard only checked that the variable was set, so the development key
committed to this repository passed it, and an empty value passed it and then
panicked at the first key creation instead of at boot."
```

---

## Task 2: Bỏ giới hạn body 2 MB và bật timeout request

**Files:**
- Modify: `config/production.yaml:12-22`
- Modify: `config/development.yaml:26-34`
- Modify: `config/test.yaml:24`
- Test: `tests/requests/api.rs`

**Interfaces:**
- Consumes: —
- Produces: middleware `limit_payload` tắt, `timeout_request` bật ở 300s, `secure_headers` bật.

- [x] **Step 1: Viết test khẳng định body lớn qua được**

Thêm vào `tests/requests/api.rs`:

```rust
/// loco's default body limit is 2 MB, which would reject every real S3 upload once the gateway lands.
/// This asserts the limit is off by sending a body larger than it and expecting anything other than 413.
#[tokio::test]
#[serial]
async fn body_limit_is_disabled() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;
        let (k, v) = prepare_data::auth_header(&user.token);

        let big_label = "x".repeat(3_000_000);
        let res = request
            .post("/api/keys")
            .add_header(k, v)
            .json(&serde_json::json!({
                "label": big_label,
                "permissions": ["read"],
                "prefixes": []
            }))
            .await;

        // 400 (label too long) is the expected outcome; 413 means the middleware ate it first.
        assert_ne!(
            res.status_code(),
            413,
            "payload limit middleware is still on"
        );
    })
    .await;
}
```

- [x] **Step 2: Chạy để chắc nó fail**

Run: `cargo test --test mod requests::api::body_limit 2>&1 | tail -10`
Expected: FAIL với 413.

- [x] **Step 3: Sửa `config/production.yaml`**

Thay khối `middlewares`:

```yaml
  middlewares:
    fallback:
      enable: false
    # loco defaults this to 2 MB. An object-storage gateway must accept multi-gigabyte
    # uploads, and S3 multipart parts have a 5 MB minimum, so the framework-wide cap is
    # the wrong place to enforce anything. The real per-route cap belongs in the S3
    # handler once it exists.
    limit_payload:
      body_limit: disable
    # Generous on purpose: a 5 GiB upload over a slow link is a legitimate request.
    timeout_request:
      enable: true
      timeout: 300000
    secure_headers:
      preset: github
    static:
      enable: true
      must_exist: true
      precompressed: false
      folder:
        uri: "/"
        path: "frontend/dist"
      fallback: "frontend/dist/index.html"
```

- [x] **Step 4: Đồng bộ development và test**

Thêm cùng khối `limit_payload` vào `config/development.yaml` (dưới `fallback`) và
`config/test.yaml`. Test phải khớp production, không thì test ở bước 1 xanh trên
CI mà production vẫn 413.

- [x] **Step 5: Chạy test**

Run: `cargo test 2>&1 | tail -10`
Expected: PASS.

- [x] **Step 6: Kiểm secure headers có thật sự ra**

```bash
cargo loco start &
sleep 3
curl -sI localhost:5150/_ping | grep -iE "x-content-type|x-frame|strict-transport"
kill %1
```

Expected: có ít nhất `x-content-type-options: nosniff`.
Ghi chú: `secure_headers` chỉ bật ở production config, nên chạy lệnh trên với
`LOCO_ENV=production` cộng đủ biến môi trường, hoặc thêm khối này vào
development config luôn.

- [x] **Step 7: Commit**

```bash
git add config/ tests/
git commit -m "fix(config): disable the 2MB body limit, enable request timeout and secure headers

The framework default would have rejected every S3 upload over 2MB, including
every multipart part, before reaching a handler."
```

---

## Task 3: Rate limit trên auth và tạo key

**Files:**
- Modify: `Cargo.toml`
- Create: `src/initializers/rate_limit.rs`
- Modify: `src/initializers/mod.rs`
- Modify: `src/app.rs:63-65`
- Test: `tests/requests/rate_limit.rs`, `tests/requests/mod.rs`

**Interfaces:**
- Consumes: —
- Produces: `initializers::rate_limit::RateLimitInitializer` — layer `tower_governor` áp lên `/api/auth/login` và `/api/admin/users`, cấu hình qua env `RATE_LIMIT_PER_MINUTE` (default 20) và `RATE_LIMIT_BURST` (default 5).

- [x] **Step 1: Thêm dependency**

Trong `Cargo.toml`, mục `[dependencies]`:

```toml
tower_governor = { version = "0.7" }
```

Run: `cargo build 2>&1 | tail -5` để chốt phiên bản vào `Cargo.lock`.

Ghi chú: nếu `tower_governor` 0.7 không tương thích Axum 0.8, dùng phiên bản mới
nhất tương thích — kiểm bằng `cargo add tower_governor --dry-run`. Đây là thư
viện duy nhất được thêm trong cả P2; loco 0.16.4 không ship middleware rate limit
nào (đã kiểm `loco-rs/src/controller/middleware/`).

- [x] **Step 2: Viết test**

Tạo `tests/requests/rate_limit.rs`:

```rust
use loco_rs::testing::prelude::*;
use object_storage_gate::app::App;
use serial_test::serial;

/// Hammers the login endpoint past the configured burst and expects a 429.
/// Without this, an attacker gets unlimited password guesses against every account.
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
```

Thêm `mod rate_limit;` vào `tests/requests/mod.rs`.

- [x] **Step 3: Chạy để chắc nó fail**

Run: `cargo test --test mod requests::rate_limit 2>&1 | tail -10`
Expected: FAIL — không bao giờ thấy 429.

- [x] **Step 4: Viết initializer**

Tạo `src/initializers/rate_limit.rs`:

```rust
//! Per-IP rate limiting for the endpoints that are cheap to attack and expensive to lose.
//!
//! loco 0.16 ships no rate-limit middleware, so this is a plain tower layer mounted through an initializer.
//! Scope is deliberately narrow: login is the brute-force target, and admin user creation is the one write that an attacker with a stolen admin token could use to flood the table.
use async_trait::async_trait;
use axum::Router as AxumRouter;
use loco_rs::{
    app::{AppContext, Initializer},
    Result,
};
use std::sync::Arc;
use tower_governor::{governor::GovernorConfigBuilder, GovernorLayer};

pub struct RateLimitInitializer;

/// Requests allowed per minute per IP once the burst is spent.
fn per_minute() -> u64 {
    std::env::var("RATE_LIMIT_PER_MINUTE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20)
}

/// How many requests an IP may fire back-to-back before the per-minute rate applies.
fn burst() -> u32 {
    std::env::var("RATE_LIMIT_BURST")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5)
}

#[async_trait]
impl Initializer for RateLimitInitializer {
    fn name(&self) -> String {
        "rate-limit".to_string()
    }

    async fn after_routes(&self, router: AxumRouter, _ctx: &AppContext) -> Result<AxumRouter> {
        let seconds_per_request = (60 / per_minute()).max(1);
        let config = Arc::new(
            GovernorConfigBuilder::default()
                .per_second(seconds_per_request)
                .burst_size(burst())
                .finish()
                .ok_or_else(|| loco_rs::Error::string("invalid rate limit configuration"))?,
        );

        Ok(router.layer(GovernorLayer { config }))
    }
}
```

Ghi chú phạm vi: layer này áp lên toàn router, không chỉ `/api/auth/login`. Đó là
lựa chọn có ý thức — `tower_governor` không nhận route filter, và tách router con
để áp riêng là nhiều code hơn giá trị nó mang lại ở giai đoạn này. Ngưỡng mặc
định (20/phút, burst 5) rộng rãi cho console và chật cho brute-force. Khi tầng S3
lên, nó phải được loại trừ — ghi lại bằng comment:

```rust
// ponytail: applied to the whole router, not per-route — tower_governor takes no route filter and a nested router is more code than this is worth today.
// Ceiling: the S3 data plane must be excluded before slice #3 ships, or a legitimate multipart upload will trip it.
```

Đặt comment này ngay trên `after_routes`.

- [x] **Step 5: Đăng ký initializer**

`src/initializers/mod.rs`:

```rust
pub mod rate_limit;
```

`src/app.rs`:

```rust
    async fn initializers(_ctx: &AppContext) -> Result<Vec<Box<dyn Initializer>>> {
        Ok(vec![Box::new(
            crate::initializers::rate_limit::RateLimitInitializer,
        )])
    }
```

- [x] **Step 6: Chạy test**

Run: `cargo test --test mod requests::rate_limit 2>&1 | tail -10`
Expected: PASS.

Run: `cargo test 2>&1 | tail -10`
Expected: PASS. Nếu test khác bắt đầu ăn 429 vì bắn nhiều request trong một
`request::<App,_,_>` block, nâng `RATE_LIMIT_BURST` trong `config/test.yaml` qua
biến môi trường của test, hoặc nới burst mặc định lên 30. Đừng tắt hẳn ở test —
tắt là mất luôn cái test vừa viết.

- [x] **Step 7: Ghi tài liệu**

Thêm vào `README.md` mục biến môi trường:

```markdown
| `RATE_LIMIT_PER_MINUTE` | `20` | Request mỗi phút mỗi IP sau khi hết burst. |
| `RATE_LIMIT_BURST` | `5` | Số request liên tiếp cho phép trước khi áp nhịp trên. |
```

- [x] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock src/ tests/ README.md
git commit -m "feat(security): add per-IP rate limiting

loco ships no rate-limit middleware and none of the three configs added one,
so login, and every other endpoint, accepted unlimited attempts."
```

---

## Task 4: PAT lưu hash, không lưu plaintext

**Files:**
- Create: `migration/src/m20260817_000002_hash_api_key.rs`
- Modify: `migration/src/lib.rs`
- Modify: `src/models/users.rs`
- Modify: `src/controllers/api.rs` (bỏ `GET /api/token`)
- Modify: `src/views/keys.rs`
- Modify: `src/fixtures/users.yaml`
- Modify: `frontend/src/routes/_app/api.tsx`, `frontend/src/lib/keys.ts`
- Test: `tests/requests/api.rs`

**Interfaces:**
- Consumes: —
- Produces: `users.api_key` lưu Argon2 hash. `users::Model::rotate_api_token(db) -> ModelResult<(Model, String)>` trả token plaintext đúng một lần. `Authenticable::find_by_api_key` tra bằng prefix + verify hash.

- [x] **Step 1: Viết test**

Thay test PAT hiện có trong `tests/requests/api.rs`:

```rust
#[tokio::test]
#[serial]
async fn pat_is_never_readable_after_creation() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;

        // The read endpoint is gone: a stolen JWT must not be upgradeable to a permanent token.
        let (k, v) = prepare_data::auth_header(&user.token);
        let gone = request.get("/api/token").add_header(k, v).await;
        assert_eq!(gone.status_code(), 404);

        // Rotation hands the token back exactly once.
        let (k, v) = prepare_data::auth_header(&user.token);
        let rotated = request.post("/api/token/rotate").add_header(k, v).await;
        assert_eq!(rotated.status_code(), 200);
        let token = rotated.json::<serde_json::Value>()["token"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(token.starts_with("osg_pat_"));

        // And what is stored is not the token.
        let stored = users::Model::find_by_pid(&ctx.db, &user.user.pid.to_string())
            .await
            .unwrap();
        assert_ne!(stored.api_key, token);
        assert!(stored.api_key.contains("$argon2"));

        // The token still authenticates.
        let (k, v) = prepare_data::auth_header(&token);
        let ok = request.get("/api/whoami").add_header(k, v).await;
        assert_eq!(ok.status_code(), 200);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn rotating_the_pat_invalidates_the_old_one() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&user.token);
        let first = request.post("/api/token/rotate").add_header(k, v).await;
        let old = first.json::<serde_json::Value>()["token"].as_str().unwrap().to_string();

        let (k, v) = prepare_data::auth_header(&user.token);
        request.post("/api/token/rotate").add_header(k, v).await;

        let (k, v) = prepare_data::auth_header(&old);
        let dead = request.get("/api/whoami").add_header(k, v).await;
        assert_eq!(dead.status_code(), 401);
    })
    .await;
}
```

- [x] **Step 2: Chạy để chắc nó fail**

Run: `cargo test --test mod requests::api::pat 2>&1 | tail -10`
Expected: FAIL — `GET /api/token` vẫn trả 200.

- [x] **Step 3: Viết migration**

Tạo `migration/src/m20260817_000002_hash_api_key.rs`:

```rust
use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        // The column now holds an Argon2 hash, which is longer than the old token and no longer unique-by-value in a useful way.
        // A lookup index would be pointless: a hash cannot be looked up by the plaintext it hashes.
        // The token carries a plaintext prefix instead, stored separately, and that is what the lookup uses.
        add_column(m, "users", "api_key_prefix", ColType::StringNull).await?;
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        remove_column(m, "users", "api_key_prefix").await?;
        Ok(())
    }
}
```

Đăng ký trong `migration/src/lib.rs` phía trên marker `inject-above`.

Ghi chú thiết kế: token có dạng `osg_pat_<prefix12>_<secret32>`. Cột
`api_key_prefix` lưu `<prefix12>` plaintext và có index; `api_key` lưu Argon2 hash
của toàn bộ token. Tra cứu là một truy vấn bằng prefix rồi một lần
`verify_password`. Đây là mô hình mà GitHub và Stripe dùng cho token của họ, và
nó chạy trên cả ba backend vì không cần `ILIKE` hay hàm riêng của backend nào.

- [x] **Step 4: Sinh lại entity**

```bash
DB_TYPE=postgres cargo loco db reset
DB_TYPE=postgres cargo loco db entities
```

- [x] **Step 5: Sửa model**

Trong `src/models/users.rs`:

```rust
/// Prefix length of a personal access token, stored in the clear so the hash can be looked up.
const PAT_PREFIX_LEN: usize = 12;

/// Builds a fresh personal access token and its stored representation.
/// Returns `(plaintext, prefix, hash)`; only the plaintext ever leaves the process, and only once.
fn mint_api_token() -> ModelResult<(String, String, String)> {
    let prefix = Uuid::new_v4().simple().to_string()[..PAT_PREFIX_LEN].to_string();
    let secret = Uuid::new_v4().simple().to_string();
    let token = format!("osg_pat_{prefix}_{secret}");
    let hashed = hash::hash_password(&token).map_err(|e| ModelError::Any(e.into()))?;
    Ok((token, prefix, hashed))
}

/// Extracts the lookup prefix from a presented token.
fn token_prefix(token: &str) -> Option<&str> {
    let rest = token.strip_prefix("osg_pat_")?;
    rest.get(..PAT_PREFIX_LEN)
}
```

Thay `before_save` để token đầu tiên cũng đi qua đường này:

```rust
        if insert {
            let mut this = self;
            this.pid = ActiveValue::Set(Uuid::new_v4());
            let (_plaintext, prefix, hashed) = mint_api_token().map_err(|e| DbErr::Custom(e.to_string()))?;
            // The token minted at insert is intentionally discarded: a user who wants a PAT rotates one, and rotation is the only path that ever reveals it.
            this.api_key = ActiveValue::Set(hashed);
            this.api_key_prefix = ActiveValue::Set(Some(prefix));
            Ok(this)
        } else {
            Ok(self)
        }
```

Thay cả hai `find_by_api_key` (bản trong `impl Authenticable` và bản trong
`impl Model`) bằng một bản chung:

```rust
    /// Finds a user by a presented personal access token.
    ///
    /// Looks up by the token's plaintext prefix, then verifies the full token against the stored hash.
    /// A prefix collision costs one extra hash verification and nothing else.
    ///
    /// # Errors
    ///
    /// When no user matches, or on a DB query error
    pub async fn find_by_api_key(db: &DatabaseConnection, api_key: &str) -> ModelResult<Self> {
        let Some(prefix) = token_prefix(api_key) else {
            return Err(ModelError::EntityNotFound);
        };
        let candidates = users::Entity::find()
            .filter(
                model::query::condition()
                    .eq(users::Column::ApiKeyPrefix, prefix)
                    .build(),
            )
            .all(db)
            .await?;

        candidates
            .into_iter()
            .find(|u| hash::verify_password(api_key, &u.api_key))
            .ok_or(ModelError::EntityNotFound)
    }
```

`impl Authenticable for Model` gọi lại hàm trên:

```rust
    async fn find_by_api_key(db: &DatabaseConnection, api_key: &str) -> ModelResult<Self> {
        Self::find_by_api_key(db, api_key).await
    }
```

Thêm phương thức xoay:

```rust
    /// Issues a fresh personal access token, invalidating the previous one.
    /// Returns the plaintext exactly once; it is not recoverable afterwards.
    ///
    /// # Errors
    ///
    /// When hashing or the DB write fails
    pub async fn rotate_api_token(self, db: &DatabaseConnection) -> ModelResult<(Model, String)> {
        let (token, prefix, hashed) = mint_api_token()?;
        let mut am: ActiveModel = self.into();
        am.api_key = ActiveValue::set(hashed);
        am.api_key_prefix = ActiveValue::set(Some(prefix));
        let user = am.update(db).await?;
        Ok((user, token))
    }
```

- [x] **Step 6: Sửa controller**

Trong `src/controllers/api.rs`, xoá handler `token` và sửa `token_rotate`:

```rust
/// Issues a fresh personal access token and returns it once.
/// There is no read endpoint: a token that can be re-read turns any stolen JWT into a permanent credential.
#[debug_handler]
async fn token_rotate(caller: Caller, State(ctx): State<AppContext>) -> Result<Response> {
    let (_user, token) = caller.user.rotate_api_token(&ctx.db).await?;
    format::json(TokenResponse { token })
}
```

Trong `routes()`, bỏ `.add("/token", get(token))`, giữ
`.add("/token/rotate", post(token_rotate))`.

- [x] **Step 7: Sửa fixture**

`src/fixtures/users.yaml` — `api_key` phải là hash chứ không phải token. Dùng
chính hash của mật khẩu đã có trong fixture để khỏi sinh mới, và đặt
`api_key_prefix: null` (fixture không dùng PAT trong test nào):

```yaml
  api_key: "$argon2id$v=19$m=19456,t=2,p=1$ETQBx4rTgNAZhSaeYZKOZg$eYTdH26CRT6nUJtacLDEboP0li6xUwUF/q5nSlQ8uuc"
  api_key_prefix: null
```

Áp cho cả hai bản ghi. Bỏ `api_key: lo-...` cũ.

- [x] **Step 8: Sửa console**

`frontend/src/lib/keys.ts` — bỏ hàm gọi `GET /api/token`.
`frontend/src/routes/_app/api.tsx` — màn PAT không còn hiển thị token hiện tại
được. Thay khối hiển thị bằng dòng trạng thái và nút xoay:

```tsx
<div style={{ fontSize: 13, color: "var(--dim)", lineHeight: 1.55 }}>
  Token chỉ hiện đúng một lần lúc tạo. Nếu bạn không còn giữ nó, hãy tạo token
  mới — token cũ sẽ ngừng hoạt động ngay.
</div>
```

Token mới trả về từ `POST /api/token/rotate` hiển thị qua `SecretRevealModal`
đang có sẵn.

- [x] **Step 9: Chạy test ba backend**

```bash
cargo test 2>&1 | tail -5
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test 2>&1 | tail -5
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test 2>&1 | tail -5
cd frontend && corepack pnpm vitest run && npx tsc --noEmit
```

- [x] **Step 10: Commit**

```bash
git add -A
git commit -m "feat(security): store personal access tokens hashed

The PAT was a plaintext column that GET /api/token handed back to any JWT
bearer, so a stolen session token upgraded itself to a permanent credential
that a password change did not evict. Tokens are now prefix-indexed Argon2
hashes, revealed once at rotation."
```

---

## Task 5: Sửa config production còn lại

**Files:**
- Modify: `config/production.yaml`
- Modify: `.dockerignore`
- Modify: `Dockerfile`
- Modify: `docker-compose.yml`, `docker-compose/postgres.yml`, `docker-compose/mysql.yml`
- Modify: `docs/docker.md`, `README.md`

**Interfaces:**
- Consumes: —
- Produces: config production khởi động được trên hạ tầng thật; compose không còn credential mặc định.

- [x] **Step 1: Sửa `config/production.yaml`**

Ba khối. Thứ nhất, `server.host` bỏ default để boot fail to thay vì gửi mail sai
địa chỉ — mà giờ không còn mail, nhưng link trong console và `full_url()` vẫn
dùng:

```yaml
server:
  port: {{ get_env(name="PORT", default="5150") }}
  binding: 0.0.0.0
  # No default on purpose: a forgotten SERVER_HOST should fail the boot, the way a
  # forgotten DATABASE_URL already does.
  host: {{ get_env(name="SERVER_HOST") }}
```

Thứ hai, database:

```yaml
database:
  uri: {{ get_env(name="DATABASE_URL") }}
  enable_logging: false
  # 5s, not the starter's 500ms: a TLS connect to a managed database across an AZ
  # routinely overruns half a second, and the boot panics with PoolTimedOut.
  # config/test.yaml already carries this fix and the same note.
  connect_timeout: {{ get_env(name="DB_CONNECT_TIMEOUT", default="5000") }}
  # 5 minutes, not 500ms: at half a second sqlx closes a connection almost as soon as
  # it goes idle, so bursty traffic pays a full TCP+TLS+auth handshake per request.
  idle_timeout: {{ get_env(name="DB_IDLE_TIMEOUT", default="300000") }}
  min_connections: {{ get_env(name="DB_MIN_CONNECTIONS", default="2") }}
  max_connections: {{ get_env(name="DB_MAX_CONNECTIONS", default="25") }}
  # Migrations run as a separate pre-deploy step. Several replicas booting at once
  # would otherwise race Migrator::up, and on MySQL a half-applied migration wedges
  # the schema with no documented recovery.
  auto_migrate: false
  dangerously_truncate: false
  dangerously_recreate: false
```

Thứ ba, mailer — không còn code nào gửi mail sau P1, nên gỡ hẳn khối `mailer:`
khỏi production config. Nếu loco đòi khối này tồn tại, giữ với `enable: false`
và thêm comment:

```yaml
# No mail is sent any more: P1 removed every mail-based auth flow and deleted
# src/mailers. Kept disabled rather than deleted so a future slice re-enabling it
# has to make a deliberate decision, including adding the missing `auth:` block.
mailer:
  smtp:
    enable: false
```

- [x] **Step 2: Chạy thử boot production**

```bash
LOCO_ENV=production DATABASE_URL=sqlite://./data/prod-test.sqlite?mode=rwc \
  JWT_SECRET=$(openssl rand -hex 32) \
  OSG_MASTER_KEY=$(openssl rand -base64 32) \
  SERVER_HOST=https://osg.example.com \
  cargo run -- start 2>&1 | head -20
```

Expected: khởi động được. Rồi thử bỏ `SERVER_HOST` và xác nhận nó fail to.

Lưu ý: `auto_migrate: false` nghĩa là phải chạy migrate trước:

```bash
LOCO_ENV=production DATABASE_URL=... cargo loco db migrate
```

- [x] **Step 3: Sửa `.dockerignore`**

```
target/
frontend/dist/
frontend/node_modules/
console-object-storage-gate/
.git/
.idea/
docs/
examples/
tests/s3/.venv/
*.sqlite
*.sqlite-*
# Real JWT_SECRET and OSG_MASTER_KEY live here. The final image does not copy it,
# but the builder layer would, and an exported build cache carries that layer.
.env
data/
```

- [x] **Step 4: Sửa Dockerfile**

Pin base image và thêm healthcheck:

```dockerfile
FROM node:22.11-slim AS frontend
...
FROM rust:1.83-slim-bookworm AS builder
...
FROM debian:bookworm-20241202-slim
```

Trước `CMD`, thêm:

```dockerfile
# Liveness only needs the process to answer; readiness is what actually pings the DB.
# Orchestrators should point their readiness probe at /_readiness.
HEALTHCHECK --interval=30s --timeout=3s --start-period=20s --retries=3 \
  CMD ["object_storage_gate-cli", "--help"]
```

Ghi chú: image cuối không có `curl`. Nếu muốn healthcheck chạm HTTP thật thì phải
thêm `curl` vào lớp runtime — đánh đổi giữa kích thước image và chất lượng
healthcheck. Ghi lại lựa chọn:

```dockerfile
# ponytail: the healthcheck only proves the binary runs, because the runtime layer has no HTTP client.
# Ceiling: add curl and hit /_readiness if the orchestrator cannot probe HTTP itself.
```

- [x] **Step 5: Sửa compose**

`docker-compose/postgres.yml`:

```yaml
    environment:
      POSTGRES_USER: ${POSTGRES_USER:?set POSTGRES_USER}
      POSTGRES_PASSWORD: ${POSTGRES_PASSWORD:?set POSTGRES_PASSWORD}
      POSTGRES_DB: ${POSTGRES_DB:-osg}
    # No ports mapping: the app reaches the database over the compose network.
    # Publishing 5432 with a default password put the production database on the host's
    # network behind a two-word credential.
```

Tương tự cho `docker-compose/mysql.yml` với `MYSQL_ROOT_PASSWORD`,
`MYSQL_PASSWORD`, `MYSQL_USER`, `MYSQL_DATABASE`, và bỏ `ports:`.

Trong `docker-compose.yml`, thêm cho service `app`:

```yaml
    restart: unless-stopped
    healthcheck:
      test: ["CMD", "object_storage_gate-cli", "--help"]
      interval: 30s
      timeout: 3s
      retries: 3
```

Và gỡ service `valkey` cùng biến `REDIS_URL` — không code nào đọc nó (đã kiểm
bằng grep toàn `src/`), nên nó chỉ đốt bộ nhớ và một volume, đồng thời làm người
vận hành tin rằng queue đã bền và có distributed lock. Thêm comment ở chỗ vừa gỡ:

```yaml
# Redis/Valkey was here but nothing read REDIS_URL. It comes back in the slice that
# actually wires the queue and the quota locks — see the roadmap, phase 6.
```

- [x] **Step 6: Chạy thử compose**

```bash
POSTGRES_USER=osg POSTGRES_PASSWORD=$(openssl rand -hex 16) \
JWT_SECRET=$(openssl rand -hex 32) OSG_MASTER_KEY=$(openssl rand -base64 32) \
SERVER_HOST=http://localhost:5150 \
docker compose -f docker-compose.yml -f docker-compose/postgres.yml up -d
sleep 20
curl -s localhost:5150/_readiness
docker compose -f docker-compose.yml -f docker-compose/postgres.yml down -v
```

Expected: `/_readiness` trả 200. Nếu fail vì chưa migrate, thêm bước migrate vào
`docs/docker.md` (xem bước sau).

- [x] **Step 7: Cập nhật tài liệu vận hành**

`docs/docker.md` thêm mục:

```markdown
## Migration là bước riêng trước khi rollout

`auto_migrate` đã tắt trên production. Nhiều replica cùng boot sẽ đua nhau chạy
`Migrator::up`, và trên MySQL một migration áp dụng dở làm kẹt schema mà không có
đường phục hồi được ghi lại.

```bash
docker compose run --rm app object_storage_gate-cli db migrate
docker compose up -d
```

## Sao lưu

Trước mỗi lần rollout có migration, chụp một bản dump. `cargo loco db dump` chỉ
chạy trên Postgres và SQLite — trên MySQL dùng `mysqldump`.
```

`README.md` — cập nhật bảng biến môi trường: bỏ `REDIS_URL`, thêm `SERVER_HOST`
là bắt buộc, `DB_CONNECT_TIMEOUT`/`DB_IDLE_TIMEOUT`/`DB_MAX_CONNECTIONS` với giá
trị mặc định mới.

- [x] **Step 8: Commit**

```bash
git add config/ Dockerfile .dockerignore docker-compose.yml docker-compose/ docs/ README.md
git commit -m "fix(ops): production config, compose credentials, image hardening

Raises the 500ms DB timeouts that the test config already documented as a boot
panic, turns off auto_migrate in favour of a pre-deploy step, removes the
default database passwords and the published DB ports from the compose
overlays, keeps .env out of the build context, and drops the Valkey service
that no code reads."
```

---

## Task 6: CI và workflow publish

**Files:**
- Modify: `.github/workflows/ci.yaml`
- Modify: `.github/workflows/docker.yaml`
- Modify: `frontend/biome.json`

**Interfaces:**
- Consumes: —
- Produces: CI chạy pnpm, frontend test/lint/typecheck, clippy `--all-targets`, `cargo audit`. Workflow publish chỉ chạy sau CI xanh, và `latest` không dính tag RC.

- [x] **Step 1: Sửa biome ignore**

`frontend/biome.json`:

```json
  "files": {
    "ignore": ["src/routeTree.gen.ts", "dist/**", "node_modules/**"]
  },
```

Kiểm: `cd frontend && corepack pnpm biome check` phải sạch. Trước đó lệnh này nổ
582 lỗi vì quét cả bundle đã build — và nó là lệnh đang được ghi trong `CLAUDE.md`.

- [x] **Step 2: Sửa job frontend trong `ci.yaml`**

Thay ba step (`Setup node`, `Build frontend`) bằng:

```yaml
      - name: Setup node
        uses: actions/setup-node@v4
        with:
          node-version: "22"
      - name: Setup pnpm
        run: corepack enable
      - name: Install frontend deps
        run: pnpm install --frozen-lockfile
        working-directory: ./frontend
      - name: Frontend lint
        run: pnpm biome check
        working-directory: ./frontend
      - name: Frontend typecheck
        run: pnpm exec tsc --noEmit
        working-directory: ./frontend
      - name: Frontend tests
        run: pnpm vitest run
        working-directory: ./frontend
      - name: Build frontend
        run: pnpm build
        working-directory: ./frontend
```

`node-version: ${{matrix.node-version}}` cũ đọc từ một matrix chỉ định nghĩa `db`,
nên giá trị rỗng và runner dùng Node mặc định của nó — khác với `node:22` trong
image.

`npm install` trước đây chạy trên một repo chỉ có `pnpm-lock.yaml`, nên CI giải
phiên bản dependency khác hẳn image. CI xanh không có nghĩa image build được.

- [x] **Step 3: Clippy quét cả test**

Trong job clippy, đổi `args` thành `--all-features --all-targets`. `CLAUDE.md` và
`README.md:196` đều kê `--all-targets`; CI thì không, nên `tests/` chưa bao giờ
được lint.

- [x] **Step 4: Thêm job audit**

```yaml
  audit:
    name: Cargo audit
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: rustsec/audit-check@v2
        with:
          token: ${{ secrets.GITHUB_TOKEN }}
```

- [x] **Step 5: Thay `actions-rs/cargo@v1`**

Ba chỗ dùng action này (`ci.yaml:29,50,129`). Nó không được bảo trì từ 2021 và
ghim vào Node runtime đã bị gỡ. Thay bằng `run:` thẳng:

```yaml
      - name: Run cargo fmt
        run: cargo fmt --all -- --check
```

```yaml
      - name: Run cargo clippy
        run: cargo clippy --all-features --all-targets -- -D warnings
```

```yaml
      - name: Run cargo test
        run: cargo test --all-features --all
```

- [x] **Step 6: Gate workflow publish**

`.github/workflows/docker.yaml`, thêm vào job `publish`:

```yaml
    needs: [verify]
```

và thêm job `verify` phía trên, gọi lại CI:

```yaml
  verify:
    name: Tests must pass first
    uses: ./.github/workflows/ci.yaml
```

Để `ci.yaml` gọi lại được, thêm `workflow_call:` vào khối `on:` của nó.

- [x] **Step 7: `latest` không dính RC**

Trong `docker.yaml` step `meta`:

```yaml
        with:
          images: ${{ secrets.DOCKERHUB_USERNAME }}/${{ env.IMAGE_NAME }}
          flavor: |
            latest=false
          tags: |
            type=semver,pattern={{version}}
            type=semver,pattern={{major}}.{{minor}}
            type=raw,value=${{ inputs.tag }},enable=${{ github.event_name == 'workflow_dispatch' }}
            # Only a plain release tag moves latest. The tag filter is
            # "[0-9]+.[0-9]+.[0-9]+*", whose trailing star matches 0.2.0-rc1, and an RC
            # must not become what everyone pulls by default.
            type=raw,value=latest,enable=${{ github.event_name == 'push' && !contains(github.ref_name, '-') }}
```

- [x] **Step 8: Kiểm workflow cú pháp**

```bash
npx --yes @action-validator/cli --verbose .github/workflows/ci.yaml
npx --yes @action-validator/cli --verbose .github/workflows/docker.yaml
```

Nếu không có mạng, ít nhất chạy `yq . .github/workflows/*.yaml` để bắt lỗi YAML.

- [x] **Step 9: Commit**

```bash
git add .github/ frontend/biome.json
git commit -m "ci: gate publish on tests, run the frontend suite, drop archived actions

The publish workflow had no needs: so a tag pushed from a branch that never
passed CI shipped an image and moved latest, and the trailing star in the tag
filter let a release candidate move it too. CI installed frontend deps with npm
against a pnpm lockfile and never ran the frontend tests, lint or typecheck."
```

---

## Self-review

**Phủ blocker.** Blocker 2 (guard master key) → task 1. Blocker 3 (rate limit) →
task 3. Blocker 4 (body limit) → task 2. Blocker 5 (workflow publish) → task 6.

**Phủ High cùng nhóm.** PAT plaintext → task 4. Timeout/pool DB → task 5.
`SERVER_HOST` → task 5. Compose credential → task 5. Healthcheck → task 5. CI
pnpm → task 6. SMTP auth → không còn cần vì P1 đã xoá mail, task 5 gỡ khối mailer
và ghi rõ lý do.

**Chưa phủ, cố ý.** JWT 7 ngày không revoke được (High) — cần cột `token_version`
trên `users` và claim `jti`; để lại vì nó đụng đúng chỗ P1 vừa sửa và nên đi cùng
một slice session management riêng. Crypto không xoay key được (High) — thuộc P3,
vì nó là thay đổi định dạng dữ liệu đã lưu. Rate limit hiện áp toàn router thay
vì theo route, đã ghi `ponytail:` với trần và đường nâng cấp.

**Nhất quán kiểu.** `validate_master_key(&str) -> Result<()>` định nghĩa task 1,
gọi ở task 1. `mint_api_token() -> ModelResult<(String, String, String)>` và
`token_prefix(&str) -> Option<&str>` định nghĩa task 4, dùng trong cùng task.
`rotate_api_token(self, db) -> ModelResult<(Model, String)>` định nghĩa task 4,
gọi từ controller cùng task.

**Rủi ro đã biết.** Task 4 đổi ý nghĩa cột `api_key` trên dữ liệu đã có: mọi PAT
đang lưu hành sẽ chết vì giá trị cũ là plaintext, không phải hash, và không có
`api_key_prefix`. Với môi trường đã có user thật, phải báo trước và cho họ xoay
token. Task 5 tắt `auto_migrate` — deploy kế tiếp sẽ không tự migrate nữa, phải
đổi quy trình rollout trước khi merge.
