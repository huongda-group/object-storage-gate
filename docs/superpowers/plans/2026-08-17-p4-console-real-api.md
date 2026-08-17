# P4 — Console bỏ mock, nối API thật — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Không màn hình nào còn hiển thị số bịa, và không nút nào còn báo thành công khi không có gì xảy ra.

**Architecture:** Ba nhóm màn hình, ba cách xử lý. Nhóm có API rồi (`/api/buckets`, `/api/usage`, `/api/admin/users` sau P1) — nối thẳng. Nhóm cần API mới nhưng thuộc phạm vi hợp lý (bucket CRUD, `/api/me`) — viết API rồi nối. Nhóm phụ thuộc tầng S3 chưa tồn tại (object browser, pool backend store) — khoá lại sau màn "Sắp có", đúng kiểu `settings.tsx` đang làm. `mock.ts` bị xoá ở task cuối; một test khẳng định không route nào import lại nó.

**Tech Stack:** React 19, TanStack Router, rsbuild, vitest, biome. Rust/loco cho các endpoint mới.

**Spec:** `docs/superpowers/specs/2026-08-17-go-live-hardening-design.md`

**Phụ thuộc:** P1 phải xong trước — task 4 và 5 gọi `/api/admin/users`.

## Global Constraints

- Ba backend hạng nhất: Postgres, MySQL >= 8.0.13, SQLite. Cấm `ILIKE`, `RETURNING`, `ON CONFLICT`, `jsonb`, cột array, `pg_advisory_lock`, `FOR UPDATE SKIP LOCKED`.
- Migration dùng `ColType` + `SchemaManager`; raw SQL branch theo `m.get_database_backend()`.
- `src/models/_entities/` generated từ Postgres.
- Comment tiếng Anh, một câu một dòng.
- Frontend: `pnpm biome check`, `pnpm exec tsc --noEmit`, `pnpm vitest run` phải sạch sau mỗi task.
- Không tự commit/push ngoài bước commit trong plan. Không AI attribution.
- **Không màn hình nào được hiển thị dữ liệu không đến từ API.** Nếu chưa có endpoint, hiển thị trạng thái "chưa khả dụng" — đừng bịa số.

---

## File Structure

**Tạo mới:**
- `src/controllers/buckets.rs` — CRUD bucket cho chủ sở hữu
- `src/views/buckets.rs` — shaper JSON (tách khỏi `views/api.rs`)
- `tests/requests/buckets.rs`
- `frontend/src/lib/api-client.ts` — wrapper gọi API có xử lý lỗi thống nhất
- `frontend/src/lib/buckets.ts` — hàm gọi bucket API
- `frontend/src/lib/admin.ts` — hàm gọi admin API
- `frontend/src/components/ComingSoon.tsx` — màn "Sắp có" dùng chung
- `frontend/src/lib/no-mock.test.ts` — test khẳng định không ai import `mock`

**Sửa:**
- `src/controllers/api.rs` — thêm `/api/me`, `/api/me/summary`
- `src/app.rs` — đăng ký `controllers::buckets::routes()`
- `frontend/src/lib/mock.ts` — **xoá** ở task 6, sau khi `UNITS` được dời
- `frontend/src/components/ui.tsx` — nhận `UNITS` tại chỗ thay vì import từ mock
- 9 route file đang import mock

---

## Task 1: API bucket CRUD

**Files:**
- Create: `src/controllers/buckets.rs`, `src/views/buckets.rs`, `tests/requests/buckets.rs`
- Modify: `src/app.rs`, `src/controllers/mod.rs`, `src/views/mod.rs`, `src/controllers/api.rs`, `tests/requests/mod.rs`

**Interfaces:**
- Consumes: `Caller` (P1 task 3), `buckets::Model::list_for_user`, `find_by_user_and_name`.
- Produces: `GET /api/buckets`, `POST /api/buckets`, `GET /api/buckets/{pid}`, `PATCH /api/buckets/{pid}`, `DELETE /api/buckets/{pid}`. `BucketResponse` giữ nguyên hình dạng đang có ở `src/views/api.rs`.

- [x] **Step 1: Viết test**

Tạo `tests/requests/buckets.rs`:

```rust
use loco_rs::testing::prelude::*;
use object_storage_gate::{app::App, models::buckets};
use serial_test::serial;

use super::prepare_data;

#[tokio::test]
#[serial]
async fn owner_can_create_list_rename_and_delete_a_bucket() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&user.token);
        let created = request
            .post("/api/buckets")
            .add_header(k, v)
            .json(&serde_json::json!({ "name": "media-cdn", "max_bytes": 1_073_741_824i64 }))
            .await;
        assert_eq!(created.status_code(), 200);

        let pid = created.json::<serde_json::Value>()["pid"]
            .as_str()
            .unwrap()
            .to_string();

        let (k, v) = prepare_data::auth_header(&user.token);
        let listed = request.get("/api/buckets").add_header(k, v).await;
        assert_eq!(listed.json::<Vec<serde_json::Value>>().len(), 1);

        let (k, v) = prepare_data::auth_header(&user.token);
        let patched = request
            .patch(&format!("/api/buckets/{pid}"))
            .add_header(k, v)
            .json(&serde_json::json!({ "max_bytes": 2_147_483_648i64 }))
            .await;
        assert_eq!(patched.status_code(), 200);
        assert_eq!(
            patched.json::<serde_json::Value>()["max_bytes"].as_i64(),
            Some(2_147_483_648)
        );

        let (k, v) = prepare_data::auth_header(&user.token);
        let deleted = request
            .delete(&format!("/api/buckets/{pid}"))
            .add_header(k, v)
            .await;
        assert_eq!(deleted.status_code(), 200);

        let (k, v) = prepare_data::auth_header(&user.token);
        let empty = request.get("/api/buckets").add_header(k, v).await;
        assert_eq!(empty.json::<Vec<serde_json::Value>>().len(), 0);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn bucket_names_are_validated_and_unique_per_owner() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;

        for bad in ["", "A", "has spaces", "UPPER", "-leading", &"x".repeat(64)] {
            let (k, v) = prepare_data::auth_header(&user.token);
            let res = request
                .post("/api/buckets")
                .add_header(k, v)
                .json(&serde_json::json!({ "name": bad, "max_bytes": 0 }))
                .await;
            assert_eq!(res.status_code(), 400, "name {bad:?} should be rejected");
        }

        let (k, v) = prepare_data::auth_header(&user.token);
        request
            .post("/api/buckets")
            .add_header(k, v)
            .json(&serde_json::json!({ "name": "taken", "max_bytes": 0 }))
            .await;

        let (k, v) = prepare_data::auth_header(&user.token);
        let dup = request
            .post("/api/buckets")
            .add_header(k, v)
            .json(&serde_json::json!({ "name": "taken", "max_bytes": 0 }))
            .await;
        assert_eq!(dup.status_code(), 400);
    })
    .await;
}

/// A bucket belonging to someone else must read as absent, not as forbidden.
#[tokio::test]
#[serial]
async fn another_users_bucket_is_not_found() {
    request::<App, _, _>(|request, ctx| async move {
        let owner = prepare_data::init_user_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&owner.token);
        let created = request
            .post("/api/buckets")
            .add_header(k, v)
            .json(&serde_json::json!({ "name": "private", "max_bytes": 0 }))
            .await;
        let pid = created.json::<serde_json::Value>()["pid"].as_str().unwrap().to_string();

        // A second user, created through the admin API.
        let (k, v) = prepare_data::auth_header(&owner.token);
        request
            .post("/api/admin/users")
            .add_header(k, v)
            .json(&serde_json::json!({
                "email": "other@congty.vn", "name": "Other",
                "password": "temp-password-1", "role": "user", "max_bytes": 0
            }))
            .await;

        let login = request
            .post("/api/auth/login")
            .json(&serde_json::json!({
                "email": "other@congty.vn", "password": "temp-password-1"
            }))
            .await;
        let other: object_storage_gate::views::auth::LoginResponse =
            serde_json::from_str(&login.text()).unwrap();

        let (k, v) = prepare_data::auth_header(&other.token);
        request
            .post("/api/me/password")
            .add_header(k, v)
            .json(&serde_json::json!({
                "current_password": "temp-password-1",
                "new_password": "other-user-secret"
            }))
            .await;

        let login = request
            .post("/api/auth/login")
            .json(&serde_json::json!({
                "email": "other@congty.vn", "password": "other-user-secret"
            }))
            .await;
        let other: object_storage_gate::views::auth::LoginResponse =
            serde_json::from_str(&login.text()).unwrap();

        let (k, v) = prepare_data::auth_header(&other.token);
        let res = request.get(&format!("/api/buckets/{pid}")).add_header(k, v).await;
        assert_eq!(res.status_code(), 404);

        let (k, v) = prepare_data::auth_header(&other.token);
        let res = request.delete(&format!("/api/buckets/{pid}")).add_header(k, v).await;
        assert_eq!(res.status_code(), 404);

        // And it is still there for its owner.
        assert!(buckets::Model::find_by_user_and_name(&ctx.db, owner.user.id, "private")
            .await
            .unwrap()
            .is_some());
    })
    .await;
}
```

Thêm `mod buckets;` vào `tests/requests/mod.rs`.

- [x] **Step 2: Chạy để chắc nó fail**

Run: `cargo test --test mod requests::buckets 2>&1 | tail -20`
Expected: FAIL — `POST /api/buckets` trả 404.

- [x] **Step 3: Thêm helper model nếu thiếu**

Kiểm `src/models/buckets.rs` đã có `create_for_user`, `find_by_pid_for_user`,
`validate_name` chưa:

Run: `grep -n "pub async fn\|pub fn validate" src/models/buckets.rs`

Thiếu cái nào thì thêm. `find_by_pid_for_user` bắt buộc phải có và phải scope
`user_id` **trong câu query**, không phải kiểm sau khi load:

```rust
    /// A bucket by its public id, scoped to its owner.
    /// The ownership condition lives in the query so a wrong owner reads as absent rather than as forbidden, which is what `access_keys::Model::find_by_pid_for_user` already does.
    ///
    /// # Errors
    /// Returns an error when no such bucket belongs to this user, or on DB failure.
    pub async fn find_by_pid_for_user(
        db: &DatabaseConnection,
        pid: &str,
        user_id: i32,
    ) -> ModelResult<Self> {
        let parsed = Uuid::parse_str(pid).map_err(|e| ModelError::Any(e.into()))?;
        Entity::find()
            .filter(Column::Pid.eq(parsed))
            .filter(Column::UserId.eq(user_id))
            .one(db)
            .await?
            .ok_or(ModelError::EntityNotFound)
    }
```

- [x] **Step 4: Viết view**

Tạo `src/views/buckets.rs`, dời `BucketResponse` từ `src/views/api.rs` sang và
thêm trường console cần:

```rust
use serde::{Deserialize, Serialize};

use crate::models::_entities::buckets;

/// The owner-facing shape of a bucket.
/// Lists fields by hand: the model carries `access_secret_encrypted`, which must never reach a response.
#[derive(Debug, Deserialize, Serialize)]
pub struct BucketResponse {
    pub pid: String,
    pub name: String,
    pub max_bytes: i64,
    pub used_bytes: i64,
    pub object_count: i64,
    pub public_enabled: bool,
    pub created_at: String,
}

impl BucketResponse {
    #[must_use]
    pub fn new(bucket: &buckets::Model) -> Self {
        Self {
            pid: bucket.pid.to_string(),
            name: bucket.name.clone(),
            max_bytes: bucket.max_bytes,
            used_bytes: bucket.used_bytes,
            object_count: bucket.object_count,
            public_enabled: bucket.public_enabled,
            created_at: bucket.created_at.to_rfc3339(),
        }
    }
}
```

Kiểm tên cột thật bằng `grep -n "pub " src/models/_entities/buckets.rs` và sửa
cho khớp.

Trong `src/views/api.rs`, bỏ `BucketResponse` cũ và re-export:
`pub use super::buckets::BucketResponse;`

- [x] **Step 5: Viết controller**

Tạo `src/controllers/buckets.rs`:

```rust
//! Bucket CRUD for the account that owns them.
//!
//! System pools (`user_id IS NULL`) are not reachable here at all — they belong to the admin tree.
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{controllers::api::Caller, models::buckets, views::buckets::BucketResponse};

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateParams {
    pub name: String,
    /// Required: `0` means unlimited, and unlimited must be a decision, never a default.
    pub max_bytes: i64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateParams {
    pub max_bytes: Option<i64>,
    pub public_enabled: Option<bool>,
}

fn bad_request(e: &ModelError) -> Error {
    Error::BadRequest(e.to_string())
}

async fn load(
    db: &DatabaseConnection,
    user_id: i32,
    pid: &str,
) -> Result<buckets::Model> {
    buckets::Model::find_by_pid_for_user(db, pid, user_id)
        .await
        .map_err(|_| Error::NotFound)
}

#[debug_handler]
async fn index(caller: Caller, State(ctx): State<AppContext>) -> Result<Response> {
    let rows = buckets::Model::list_for_user(&ctx.db, caller.user.id).await?;
    format::json(rows.iter().map(BucketResponse::new).collect::<Vec<_>>())
}

#[debug_handler]
async fn create(
    caller: Caller,
    State(ctx): State<AppContext>,
    Json(params): Json<CreateParams>,
) -> Result<Response> {
    if params.max_bytes < 0 {
        return Err(Error::BadRequest("max_bytes must not be negative".to_string()));
    }
    let bucket = buckets::Model::create_for_user(&ctx.db, caller.user.id, &params.name)
        .await
        .map_err(|e| bad_request(&e))?;

    let bucket = if params.max_bytes == bucket.max_bytes {
        bucket
    } else {
        let mut am: buckets::ActiveModel = bucket.into();
        am.max_bytes = ActiveValue::set(params.max_bytes);
        am.update(&ctx.db).await?
    };

    format::json(BucketResponse::new(&bucket))
}

#[debug_handler]
async fn show(
    caller: Caller,
    Path(pid): Path<String>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    let bucket = load(&ctx.db, caller.user.id, &pid).await?;
    format::json(BucketResponse::new(&bucket))
}

#[debug_handler]
async fn update(
    caller: Caller,
    Path(pid): Path<String>,
    State(ctx): State<AppContext>,
    Json(params): Json<UpdateParams>,
) -> Result<Response> {
    let bucket = load(&ctx.db, caller.user.id, &pid).await?;

    if let Some(max_bytes) = params.max_bytes {
        if max_bytes < 0 {
            return Err(Error::BadRequest("max_bytes must not be negative".to_string()));
        }
        // A quota below what is already stored would make every future write fail with no way back.
        if max_bytes != 0 && max_bytes < bucket.used_bytes {
            return Err(Error::BadRequest(
                "quota is below the bytes already stored in this bucket".to_string(),
            ));
        }
    }

    let mut am: buckets::ActiveModel = bucket.into();
    if let Some(max_bytes) = params.max_bytes {
        am.max_bytes = ActiveValue::set(max_bytes);
    }
    if let Some(public_enabled) = params.public_enabled {
        am.public_enabled = ActiveValue::set(public_enabled);
    }
    let updated = am.update(&ctx.db).await?;

    format::json(BucketResponse::new(&updated))
}

#[debug_handler]
async fn destroy(
    caller: Caller,
    Path(pid): Path<String>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    let bucket = load(&ctx.db, caller.user.id, &pid).await?;

    // ponytail: deletes the metadata rows only; nothing is removed from the backend store, because there is no backend store client yet.
    // Ceiling: once the proxy slice lands this must either delete upstream or refuse while the bucket is non-empty.
    if bucket.object_count > 0 {
        return Err(Error::BadRequest(
            "bucket is not empty; delete its objects first".to_string(),
        ));
    }

    let am: buckets::ActiveModel = bucket.into();
    am.delete(&ctx.db).await?;

    format::json(())
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/buckets")
        .add("/", get(index).post(create))
        .add("/{pid}", get(show).patch(update).delete(destroy))
}
```

Bỏ `.add("/buckets", get(list_buckets))` khỏi `src/controllers/api.rs` và xoá
handler `list_buckets` — nó đã chuyển sang đây.

Đăng ký trong `src/controllers/mod.rs` và `src/app.rs`.

- [x] **Step 6: Chạy test ba backend**

```bash
cargo test --test mod requests::buckets 2>&1 | tail -20
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test 2>&1 | tail -5
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test 2>&1 | tail -5
```

- [x] **Step 7: Commit**

```bash
git add src/ tests/
git commit -m "feat(api): bucket CRUD for the owning account

The console had create, rename, quota and delete controls for buckets with no
endpoint behind any of them."
```

---

## Task 2: API `/api/me` và `/api/me/summary`

**Files:**
- Modify: `src/controllers/api.rs`, `src/views/api.rs`
- Test: `tests/requests/api.rs`

**Interfaces:**
- Consumes: `Caller`.
- Produces: `PATCH /api/me` (`{name?}`), `GET /api/me/summary` → `{used_bytes, max_bytes, bucket_count, active_key_count}`.

Bối cảnh: `frontend/src/routes/_app/profile.tsx:153-161` lấy tên và email thật từ
`useShell()` nhưng số byte đã dùng, số bucket và số key hoạt động thì lấy từ
`mock.ts`. Một người xem trang hồ sơ của chính mình đang đọc số của người khác.

- [x] **Step 1: Viết test**

Thêm vào `tests/requests/api.rs`:

```rust
#[tokio::test]
#[serial]
async fn summary_counts_real_buckets_and_keys() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&user.token);
        request
            .post("/api/buckets")
            .add_header(k, v)
            .json(&serde_json::json!({ "name": "one", "max_bytes": 0 }))
            .await;

        let (k, v) = prepare_data::auth_header(&user.token);
        request
            .post("/api/keys")
            .add_header(k, v)
            .json(&serde_json::json!({
                "label": "ci", "permissions": ["read"], "prefixes": []
            }))
            .await;

        let (k, v) = prepare_data::auth_header(&user.token);
        let res = request.get("/api/me/summary").add_header(k, v).await;
        assert_eq!(res.status_code(), 200);

        let body = res.json::<serde_json::Value>();
        assert_eq!(body["bucket_count"].as_i64(), Some(1));
        assert_eq!(body["active_key_count"].as_i64(), Some(1));
        assert_eq!(body["used_bytes"].as_i64(), Some(0));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_user_can_rename_themselves() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&user.token);
        let res = request
            .patch("/api/me")
            .add_header(k, v)
            .json(&serde_json::json!({ "name": "Tên Mới" }))
            .await;
        assert_eq!(res.status_code(), 200);

        let reloaded = users::Model::find_by_pid(&ctx.db, &user.user.pid.to_string())
            .await
            .unwrap();
        assert_eq!(reloaded.name, "Tên Mới");
    })
    .await;
}

/// Renaming must not be a way to change the role or the quota.
#[tokio::test]
#[serial]
async fn patch_me_ignores_role_and_quota() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;
        let before = user.user.clone();

        let (k, v) = prepare_data::auth_header(&user.token);
        request
            .patch("/api/me")
            .add_header(k, v)
            .json(&serde_json::json!({
                "name": "Still Me", "role": "admin", "max_bytes": 99_999
            }))
            .await;

        let after = users::Model::find_by_pid(&ctx.db, &before.pid.to_string())
            .await
            .unwrap();
        assert_eq!(after.role, before.role);
        assert_eq!(after.max_bytes, before.max_bytes);
    })
    .await;
}
```

- [x] **Step 2: Chạy để chắc nó fail**

Run: `cargo test --test mod requests::api::summary 2>&1 | tail -10`
Expected: FAIL — 404.

- [x] **Step 3: Viết view**

Thêm vào `src/views/api.rs`:

```rust
/// The account summary the profile screen shows.
/// Every number comes from the database; the screen previously read them from a fixture.
#[derive(Debug, Deserialize, Serialize)]
pub struct SummaryResponse {
    pub used_bytes: i64,
    pub max_bytes: i64,
    pub bucket_count: i64,
    pub active_key_count: i64,
}
```

- [x] **Step 4: Viết handler**

Trong `src/controllers/api.rs`:

```rust
#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateMeParams {
    pub name: Option<String>,
}

/// Renames the calling user.
/// Deliberately narrow: role and quota are an admin's decision, and a struct with only `name` makes that structural rather than a check that can be forgotten.
#[debug_handler]
async fn update_me(
    caller: Caller,
    State(ctx): State<AppContext>,
    Json(params): Json<UpdateMeParams>,
) -> Result<Response> {
    let mut am: users::ActiveModel = caller.user.into();
    if let Some(name) = &params.name {
        am.name = ActiveValue::set(name.clone());
    }
    let updated = am.update(&ctx.db).await?;
    format::json(CurrentResponse::new(&updated))
}

#[debug_handler]
async fn summary(caller: Caller, State(ctx): State<AppContext>) -> Result<Response> {
    let db = &ctx.db;
    let bucket_rows = buckets::Model::list_for_user(db, caller.user.id).await?;
    let key_rows = access_keys::Model::list_for_user(db, caller.user.id).await?;

    format::json(SummaryResponse {
        used_bytes: caller.user.used_bytes,
        max_bytes: caller.user.max_bytes,
        bucket_count: bucket_rows.len() as i64,
        active_key_count: key_rows
            .iter()
            .filter(|k| k.status == access_keys::KEY_ACTIVE)
            .count() as i64,
    })
}
```

Thêm vào `routes()`:

```rust
        .add("/me", patch(update_me))
        .add("/me/summary", get(summary))
```

Import `CurrentResponse` và `SummaryResponse`.

Ghi chú: `list_for_user` của access_keys trả về model kèm policy — kiểm kiểu trả
về thật bằng `grep -n "pub async fn list_for_user" -A3 src/models/access_keys.rs`
và sửa `.filter(|k| ...)` cho khớp (có thể là tuple `(Model, Vec<String>, Vec<String>)`).

- [x] **Step 5: Chạy test và commit**

```bash
cargo test --test mod requests::api 2>&1 | tail -10
cargo clippy --all-targets 2>&1 | tail -10
git add src/ tests/
git commit -m "feat(api): add PATCH /api/me and GET /api/me/summary

The profile screen showed a real name and email next to fixture numbers for
bytes used, bucket count and active keys."
```

---

## Task 3: Wrapper gọi API có xử lý lỗi thống nhất

**Files:**
- Create: `frontend/src/lib/api-client.ts`, `frontend/src/lib/api-client.test.ts`
- Modify: mọi chỗ đang gọi `void doThing()`

**Interfaces:**
- Consumes: `api`, `ApiError` từ `lib/auth.ts`.
- Produces: `run<T>(fn: () => Promise<T>, opts?: { onError?: (msg: string) => void }): Promise<T | undefined>` — bắt `ApiError`, dịch sang thông điệp tiếng Việt, gọi `onError`, và không bao giờ ném ra ngoài.

Bối cảnh: mọi mutation trên hai màn hình thật (`keys/index.tsx:332,345,522,547`,
`keys/$pid.tsx:221,229,423,590,666-671`, `api.tsx:166,264`) đều là
`onClick={() => void doThing()}` không `.catch`. `PATCH /api/keys/{pid}` trả
400/500 thì user không thấy gì cả. Tệ nhất `keys/$pid.tsx:666`: revoke lỗi thì
`setRevoking(false)` không bao giờ chạy, modal treo, user bấm lại.

- [x] **Step 1: Viết test**

Tạo `frontend/src/lib/api-client.test.ts`:

```ts
import { describe, expect, it, vi } from "vitest";
import { ApiError } from "./auth";
import { run } from "./api-client";

describe("run", () => {
  it("returns the value on success and never calls onError", async () => {
    const onError = vi.fn();
    const value = await run(() => Promise.resolve(42), { onError });
    expect(value).toBe(42);
    expect(onError).not.toHaveBeenCalled();
  });

  it("swallows the error and reports a message", async () => {
    const onError = vi.fn();
    const value = await run(
      () => Promise.reject(new ApiError(400, "label too long")),
      { onError },
    );
    expect(value).toBeUndefined();
    expect(onError).toHaveBeenCalledWith("label too long");
  });

  it("reports a generic message for a 500", async () => {
    const onError = vi.fn();
    await run(() => Promise.reject(new ApiError(500, "")), { onError });
    expect(onError).toHaveBeenCalledWith("Máy chủ gặp lỗi. Thử lại sau.");
  });

  it("reports a network failure distinctly", async () => {
    const onError = vi.fn();
    await run(() => Promise.reject(new TypeError("fetch failed")), { onError });
    expect(onError).toHaveBeenCalledWith("Không kết nối được máy chủ.");
  });
});
```

- [x] **Step 2: Chạy để chắc nó fail**

Run: `cd frontend && corepack pnpm vitest run src/lib/api-client.test.ts`
Expected: FAIL — module không tồn tại.

- [x] **Step 3: Viết wrapper**

Tạo `frontend/src/lib/api-client.ts`:

```ts
import { ApiError } from "./auth";

/** Turns a failure into something a person can act on. */
function messageFor(e: unknown): string {
  if (e instanceof ApiError) {
    if (e.status >= 500) return "Máy chủ gặp lỗi. Thử lại sau.";
    if (e.status === 403) return "Bạn không có quyền thực hiện thao tác này.";
    if (e.status === 404) return "Không tìm thấy. Có thể nó đã bị xoá.";
    return e.message || "Yêu cầu không hợp lệ.";
  }
  if (e instanceof TypeError) return "Không kết nối được máy chủ.";
  return "Có lỗi không xác định.";
}

/**
 * Runs an API call and reports failures instead of dropping them.
 *
 * Every mutation in this console used to be `onClick={() => void doThing()}`, so a
 * rejected request produced no toast, no error and no state change — the user saw
 * nothing at all and clicked again.
 */
export async function run<T>(
  fn: () => Promise<T>,
  opts: { onError?: (message: string) => void } = {},
): Promise<T | undefined> {
  try {
    return await fn();
  } catch (e) {
    opts.onError?.(messageFor(e));
    return undefined;
  }
}
```

- [x] **Step 4: Áp vào mọi mutation hiện có**

Tìm hết:

```bash
cd frontend && grep -rn "void [a-zA-Z]*(" src/routes src/components | grep -v "test"
```

Với mỗi chỗ, đổi từ:

```tsx
onClick={() => void doThing()}
```

sang:

```tsx
onClick={() => void run(doThing, { onError: (m) => toast(m, "danger") })}
```

Và mọi khối `finally` phải đặt lại cờ busy — đặc biệt `keys/$pid.tsx:666-671`:

```tsx
async function revoke() {
  setRevoking(true);
  const ok = await run(() => revokeKey(pid), {
    onError: (m) => toast(m, "danger"),
  });
  setRevoking(false);
  if (ok !== undefined) {
    setConfirmOpen(false);
    await reload();
  }
}
```

- [x] **Step 5: Sửa thứ tự trong rotate key**

`keys/index.tsx:114-120` gọi `rotateKey()` rồi `await reload()` **trước khi** đặt
secret vào state. `reload()` lỗi là secret bị vứt và modal không mở, để lại một
key đã xoay mà không ai có secret.

```tsx
const rotated = await run(() => rotateKey(pid), {
  onError: (m) => toast(m, "danger"),
});
if (!rotated) return;
setSecret(rotated.secret);   // Set it first: a failed reload must not lose the one-time secret.
await run(reload, { onError: (m) => toast(m, "danger") });
```

- [x] **Step 6: Sửa copy clipboard báo sai**

`keys/index.tsx:130-139`, `SecretRevealModal.tsx:48-57`, `api.tsx:75-82`,
`buckets/$name/index.tsx:170-179`, `admin/buckets.tsx:727-734` đều `catch {}` rồi
toast "Đã copy vào clipboard". Trên ngữ cảnh không bảo mật hoặc bị từ chối quyền
thì không có gì được copy.

```tsx
try {
  await navigator.clipboard.writeText(value);
  toast("Đã copy vào clipboard");
} catch {
  toast("Trình duyệt không cho copy. Chọn và copy thủ công.", "danger");
}
```

- [x] **Step 7: Sửa tải file CSV secret**

`SecretRevealModal.tsx:59-68` tạo thẻ `<a>` rời, click, rồi `revokeObjectURL`
đồng bộ. Firefox đòi thẻ phải nằm trong document, và revoke cùng tick đua với
việc tải.

```tsx
const url = URL.createObjectURL(blob);
const a = document.createElement("a");
a.href = url;
a.download = "osgate-credentials.csv";
document.body.appendChild(a);
a.click();
document.body.removeChild(a);
// Revoke on the next tick: revoking in the same one races the download in several browsers.
setTimeout(() => URL.revokeObjectURL(url), 0);
```

- [x] **Step 8: Chạy kiểm và commit**

```bash
cd frontend
corepack pnpm vitest run
corepack pnpm biome check
corepack pnpm exec tsc --noEmit
```

```bash
git add frontend/
git commit -m "fix(console): report API failures instead of dropping them

Every mutation was an unhandled promise rejection, so a 400 or 500 produced no
toast, no error and no state change. Also fixes a revoke that left its modal
hanging, a rotate that discarded the one-time secret when the reload failed,
clipboard failures reported as success, and a CSV download racing its own
object URL revocation."
```

---

## Task 4: Nối các màn hình đã có API

**Files:**
- Create: `frontend/src/lib/buckets.ts`, `frontend/src/lib/admin.ts`
- Modify: `frontend/src/routes/_app/index.tsx`, `profile.tsx`, `buckets/index.tsx`, `buckets/$name/settings.tsx`, `admin/users/index.tsx`, `admin/users/$pid.tsx`, `admin/index.tsx`

**Interfaces:**
- Consumes: task 1 (bucket CRUD), task 2 (`/api/me/summary`), task 3 (`run`), P1 task 4 (`/api/admin/users`).
- Produces: `lib/buckets.ts` xuất `listBuckets`, `createBucket`, `getBucket`, `updateBucket`, `deleteBucket`. `lib/admin.ts` xuất `listUsers`, `createUser`, `getUser`, `updateUser`, `setUserPassword`, `deleteUser`.

- [x] **Step 1: Viết hai module gọi API**

`frontend/src/lib/buckets.ts`:

```ts
import { api } from "./auth";

export type Bucket = {
  pid: string;
  name: string;
  max_bytes: number;
  used_bytes: number;
  object_count: number;
  public_enabled: boolean;
  created_at: string;
};

export const listBuckets = () => api<Bucket[]>("/api/buckets");

export const createBucket = (name: string, max_bytes: number) =>
  api<Bucket>("/api/buckets", {
    method: "POST",
    body: JSON.stringify({ name, max_bytes }),
  });

export const getBucket = (pid: string) => api<Bucket>(`/api/buckets/${pid}`);

export const updateBucket = (
  pid: string,
  patch: { max_bytes?: number; public_enabled?: boolean },
) =>
  api<Bucket>(`/api/buckets/${pid}`, {
    method: "PATCH",
    body: JSON.stringify(patch),
  });

export const deleteBucket = (pid: string) =>
  api<void>(`/api/buckets/${pid}`, { method: "DELETE" });
```

`frontend/src/lib/admin.ts`:

```ts
import { api } from "./auth";

export type AdminUser = {
  pid: string;
  email: string;
  name: string;
  role: "user" | "admin";
  max_bytes: number;
  used_bytes: number;
  reserved_bytes: number;
  must_change_password: boolean;
  created_at: string;
};

export const listUsers = () => api<AdminUser[]>("/api/admin/users");

export const createUser = (body: {
  email: string;
  name: string;
  password: string;
  role: "user" | "admin";
  max_bytes: number;
}) => api<AdminUser>("/api/admin/users", { method: "POST", body: JSON.stringify(body) });

export const getUser = (pid: string) => api<AdminUser>(`/api/admin/users/${pid}`);

export const updateUser = (
  pid: string,
  patch: { name?: string; role?: "user" | "admin"; max_bytes?: number },
) =>
  api<AdminUser>(`/api/admin/users/${pid}`, {
    method: "PATCH",
    body: JSON.stringify(patch),
  });

export const setUserPassword = (pid: string, password: string) =>
  api<void>(`/api/admin/users/${pid}/password`, {
    method: "POST",
    body: JSON.stringify({ password }),
  });

export const deleteUser = (pid: string) =>
  api<void>(`/api/admin/users/${pid}`, { method: "DELETE" });
```

- [x] **Step 2: Nối `/buckets`**

`frontend/src/routes/_app/buckets/index.tsx` — bỏ `import { BUCKETS } from "../../../lib/mock"`, thay bằng `loader` gọi `listBuckets()`, và nối ba mutation:

```tsx
export const Route = createFileRoute("/_app/buckets/")({
  loader: () => listBuckets(),
  component: Buckets,
});
```

Nút tạo gọi `createBucket(name, maxBytes)` rồi `router.invalidate()`. Nút xoá gọi
`deleteBucket(pid)` rồi `router.invalidate()`. Cả hai bọc trong `run`.

**Xoá form "đổi tên bucket"** nếu có — API không hỗ trợ đổi tên, và tên bucket là
một phần định danh trong S3 nên đổi tên là một thao tác di dữ liệu, không phải
một `PATCH`. Thay bằng dòng giải thích:

```tsx
<div style={{ fontSize: 13, color: "var(--dim)" }}>
  Tên bucket không đổi được sau khi tạo — nó là một phần đường dẫn S3 mà client
  của bạn đang dùng.
</div>
```

- [x] **Step 3: Nối `/buckets/$name/settings`**

Chuyển route param từ `$name` sang `$pid` để khớp API — sửa mọi `Link` trỏ tới nó.
Nối "Lưu quota" vào `updateBucket(pid, { max_bytes })`, và công tắc public vào
`updateBucket(pid, { public_enabled })`. Nút xoá gọi `deleteBucket` rồi điều hướng
về `/buckets`.

- [x] **Step 4: Nối `/` và `/profile`**

Dashboard (`_app/index.tsx`) hiện đọc `ACCOUNT`, `ACCOUNT_STATS`, `BUCKETS`,
`KEYS`, `ENDPOINT`, `REGION` từ mock. Thay bằng loader gọi song song:

```tsx
loader: async () => {
  const [summary, buckets, keys] = await Promise.all([
    getSummary(),
    listBuckets(),
    listKeys(),
  ]);
  return { summary, buckets, keys };
},
```

`ENDPOINT` và `REGION` là hằng bịa (`mock.ts:10-11` — `https://s3.osgate.vn`,
`ap-southeast-1`) được in vào đoạn lệnh copy-paste ở `_app/index.tsx:32-52` và
`buckets/$name/index.tsx:329`. Một user copy `aws s3 cp ... --endpoint-url
https://s3.osgate.vn` là trỏ credential vào một tên miền không phải deployment
của họ. Thay bằng `globalThis.location.origin`, và bỏ hẳn dòng `--region` cho tới
khi tầng S3 quyết định region nghĩa là gì:

```tsx
const endpoint = globalThis.location.origin;
```

Profile (`_app/profile.tsx:153-161`) đổi ba số sang `getSummary()`.

- [x] **Step 5: Nối `/admin/users` và `/admin/users/$pid`**

Đổi route param từ email sang `pid` — `admin/index.tsx:126,226` và
`admin/users/index.tsx` đang truyền `params={{ pid: u.email }}`, nên mọi lịch sử
duyệt web của admin và mọi access log ghi lại
`/admin/users/an.nguyen@osgate.vn`. Phần còn lại của app đã dùng `pid` mờ.

Nối: danh sách vào `listUsers()`, form tạo vào `createUser()`, sửa tên/role/quota
vào `updateUser()`, đặt lại mật khẩu vào `setUserPassword()` (hiện màn hình chưa
có nút này — thêm vào, nó thay thế luồng quên-mật-khẩu đã bị xoá ở P1), xoá vào
`deleteUser()`.

Mật khẩu tạm sinh ra khi tạo user phải hiển thị đúng một lần — dùng lại
`SecretRevealModal` đang có.

**Bỏ hẳn** phần "key của user này" ở `admin/users/$pid.tsx:443-460`: hai nút
disable/revoke ở đó là toast trần không đổi state, và không có endpoint admin nào
cho key của người khác. Thay bằng dòng "Chưa khả dụng".

- [x] **Step 6: Nối `/admin`**

`admin/index.tsx` hiện đọc `ADMIN_STATS` với các chuỗi cứng "8 user",
"2.41M object", "2.3 TiB". Thay bằng số đếm được từ `listUsers()`:

```tsx
loader: () => listUsers(),
```

Tổng dung lượng cộng từ `used_bytes` của các user. Bỏ thẻ "oversubscribe 127%" —
không có nguồn dữ liệu nào tính được nó cho tới khi có pool thật.

- [x] **Step 7: Kiểm bằng tay**

```bash
cargo loco start &
cd frontend && corepack pnpm dev
```

Đăng nhập, tạo bucket, đổi quota, xoá bucket, tạo user, đặt lại mật khẩu, xoá
user. Mỗi thao tác phải: đổi thật trong DB (kiểm bằng `psql`), và hiện lỗi rõ
ràng khi thất bại.

- [x] **Step 8: Chạy kiểm và commit**

```bash
cd frontend
corepack pnpm vitest run && corepack pnpm biome check && corepack pnpm exec tsc --noEmit
```

```bash
git add frontend/
git commit -m "feat(console): wire dashboard, buckets, profile and admin to the real API

These screens rendered fixture data: 246.5 GiB used, five buckets, eight system
users, none of which existed, and every viewer saw the same numbers. The
connection snippets also pointed at a hardcoded third-party endpoint."
```

---

## Task 5: Khoá màn hình chưa có backend

**Files:**
- Create: `frontend/src/components/ComingSoon.tsx`
- Modify: `frontend/src/routes/_app/buckets/$name/index.tsx`, `admin/buckets.tsx`

**Interfaces:**
- Consumes: —
- Produces: `<ComingSoon title reason />` — màn hình trạng thái, không có control giả.

Hai màn hình còn lại phụ thuộc tầng S3 chưa tồn tại:

- **Object browser** (`buckets/$name/index.tsx`) — liệt kê và xoá object cần
  ListObjectsV2 và DeleteObject. Không có gì phía sau.
- **Pool backend store** (`admin/buckets.tsx`) — form thu thập access key ID và
  secret của provider rồi cất vào state React. Admin dán credential S3 thật vào
  một form vứt chúng đi khi tải lại trang.

- [x] **Step 1: Viết component**

```tsx
type Props = { title: string; reason: string };

/**
 * A screen with no backend yet.
 *
 * Deliberately has no controls: a disabled button still suggests the feature exists and
 * is merely switched off, and this console previously shipped forms that accepted
 * production credentials and discarded them.
 */
export function ComingSoon({ title, reason }: Props) {
  return (
    <div style={{ maxWidth: 460, margin: "64px auto", textAlign: "center", display: "flex", flexDirection: "column", gap: 12 }}>
      <div style={{ fontSize: 20, fontWeight: 600 }}>{title}</div>
      <div style={{ fontSize: 13.5, color: "var(--dim)", lineHeight: 1.6 }}>{reason}</div>
    </div>
  );
}
```

- [x] **Step 2: Thay hai màn hình**

`buckets/$name/index.tsx` — giữ phần header bucket (tên, quota, endpoint) vì nó
lấy từ `getBucket()` thật, thay phần duyệt object bằng:

```tsx
<ComingSoon
  title="Duyệt object"
  reason="Màn hình này cần API S3 của gateway, hiện chưa được triển khai. Dùng aws-cli hoặc rclone với access key của bạn khi tầng S3 lên."
/>
```

`admin/buckets.tsx` — thay toàn bộ component bằng:

```tsx
<ComingSoon
  title="Pool và backend store"
  reason="Cấu hình pool cần tầng proxy tới object store, hiện chưa được triển khai. Đừng nhập credential provider vào đây."
/>
```

Xoá toàn bộ form và state của hai màn — không giữ lại code chết.

- [x] **Step 3: Chạy kiểm và commit**

```bash
cd frontend && corepack pnpm biome check && corepack pnpm exec tsc --noEmit && corepack pnpm build
```

```bash
git add frontend/
git commit -m "feat(console): replace the two backend-less screens with a status page

The object browser deleted objects from local state only, and the pool form
collected provider access keys and secrets into a React state object that a page
reload threw away."
```

---

## Task 6: Xoá `mock.ts` và chặn nó quay lại

**Files:**
- Delete: `frontend/src/lib/mock.ts`
- Create: `frontend/src/lib/no-mock.test.ts`
- Modify: `frontend/src/components/ui.tsx`

**Interfaces:**
- Consumes: task 4 và 5 đã gỡ mọi lời gọi.
- Produces: `UNITS` sống trong `components/ui.tsx`; không file nào import `lib/mock`.

- [x] **Step 1: Viết test bảo vệ**

Tạo `frontend/src/lib/no-mock.test.ts`:

```ts
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

function walk(dir: string): string[] {
  return readdirSync(dir).flatMap((entry) => {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) return walk(full);
    return full.endsWith(".tsx") || full.endsWith(".ts") ? [full] : [];
  });
}

/**
 * Nine screens once rendered fixture data as if it were the signed-in user's account, and
 * the fixtures shipped in the production bundle. This is the cheapest guard against that
 * returning: no source file may import the mock module.
 */
describe("no fixture data in the app", () => {
  it("no source file imports lib/mock", () => {
    const offenders = walk("src")
      .filter((f) => !f.endsWith("no-mock.test.ts"))
      .filter((f) => /from\s+["'].*lib\/mock["']|from\s+["']\.\.?\/mock["']/.test(readFileSync(f, "utf8")));

    expect(offenders).toEqual([]);
  });
});
```

- [x] **Step 2: Chạy để chắc nó fail**

Run: `cd frontend && corepack pnpm vitest run src/lib/no-mock.test.ts`
Expected: FAIL với ít nhất `src/components/ui.tsx` trong danh sách.

- [x] **Step 3: Dời `UNITS`**

`components/ui.tsx:5` đang `import type { UNITS } from "../lib/mock"`. Định nghĩa
tại chỗ:

```tsx
/** Byte units used by the quota formatter, largest last. */
export const UNITS = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"] as const;
export type Unit = (typeof UNITS)[number];
```

Kiểm giá trị thật trong `mock.ts` trước khi chép — `grep -n "UNITS" src/lib/mock.ts`.

- [x] **Step 4: Xoá mock**

```bash
cd frontend && git rm src/lib/mock.ts
```

- [x] **Step 5: Xác nhận bundle sạch**

```bash
corepack pnpm build
grep -rl "osgate.vn\|an.nguyen\|246.5\|oversubscribe" dist/ && echo "FIXTURE STILL IN BUNDLE" || echo "clean"
```

Expected: `clean`.

- [x] **Step 6: Chạy toàn bộ và commit**

```bash
corepack pnpm vitest run && corepack pnpm biome check && corepack pnpm exec tsc --noEmit
```

```bash
git add -A frontend/
git commit -m "chore(console): delete the fixture module

mock.ts was imported by nine route files and one component, and its contents
shipped in nine production chunks. A test now fails if anything imports it
again."
```

---

## Self-review

**Phủ blocker.** Blocker 7 (9 màn hình số bịa) → task 4, 5, 6. Blocker 8 (nút ghi
nói dối) → task 3, 4, 5 — mỗi control hoặc được nối vào endpoint thật hoặc bị gỡ.
Blocker 9 (không có gác admin phía server) đã do P1 task 3 giải; task 4 ở đây chỉ
tiêu thụ nó.

**Phủ High và Medium liên quan.** Unhandled rejection → task 3. Endpoint bịa
trong đoạn lệnh copy-paste → task 4 bước 4. 401 không điều hướng → P1 task 5 đã
sửa trong `api()`. Email làm route param → task 4 bước 5. Rotate vứt secret →
task 3 bước 5. Clipboard báo sai → task 3 bước 6. Tải CSV đua revoke → task 3
bước 7.

**Chưa phủ, cố ý.** JWT trong `localStorage` (Medium) — sửa thật là chuyển sang
cookie HttpOnly cộng CSRF token, đụng cả server lẫn client và xứng đáng một slice
riêng; giảm thiểu rẻ là thêm CSP header, đã nằm trong P2 task 2 qua
`secure_headers`. Không có render test (`@testing-library/react` chưa là
dependency) — test ở task 6 là bảo vệ rẻ nhất cho đúng lỗi đã xảy ra;
thêm bộ render test đầy đủ là việc riêng.

**Nhất quán kiểu.** `Bucket` và `AdminUser` khai ở `lib/buckets.ts` và
`lib/admin.ts` (task 4), khớp `BucketResponse` (task 1) và `AdminUserResponse`
(P1 task 4) từng trường một. `run<T>` khai task 3, dùng ở task 3 và 4.
`getSummary()` dùng ở task 4 phải được thêm vào `lib/auth.ts` hoặc
`lib/api-client.ts` — thêm vào `lib/buckets.ts` là sai chỗ; đặt trong `lib/auth.ts`
cạnh `current()`:

```ts
export type Summary = {
  used_bytes: number;
  max_bytes: number;
  bucket_count: number;
  active_key_count: number;
};

export const getSummary = () => api<Summary>("/api/me/summary");
```

**Rủi ro đã biết.** Task 4 đổi route param của bucket từ `$name` sang `$pid` và
của admin user từ email sang `pid` — mọi liên kết đã lưu (bookmark) sẽ hỏng. Chấp
nhận được vì console chưa mở cho ai.
