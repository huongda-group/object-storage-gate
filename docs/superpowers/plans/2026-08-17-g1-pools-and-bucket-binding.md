# G1 — Pools và ràng buộc bucket → pool — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Một pool giữ credential upstream và tên physical bucket; mọi bucket của user trỏ tới đúng một pool; admin quản lý pool qua API và console.

**Architecture:** Tách sáu cột store khỏi `buckets` sang bảng `pools` mới, thêm `buckets.pool_id` NOT NULL với backfill an toàn. Rồi `/api/admin/pools` CRUD gác bằng `AdminCaller`, và màn Pool trên console — mà P4 cho thành `ComingSoon` vì không có backend — nối vào backend thật.

**Tech Stack:** Rust, loco-rs 0.16, SeaORM 1.1, sea-orm-migration, serial_test. React 19 + TanStack Router.

**Spec:** `docs/superpowers/specs/2026-08-17-s3-gateway-design.md` mục 3.1, 3.2, 18.2

**Deliverable:** Admin tạo được pool, sửa credential, và tạo bucket gắn vào pool. Không có route S3 nào trong plan này — nó dựng nền cho G2–G7.

## Global Constraints

- Ba backend hạng nhất: Postgres, MySQL >= 8.0.13, SQLite. Cấm `ILIKE`, `RETURNING`, `ON CONFLICT` / `ON DUPLICATE KEY`, `jsonb`, cột array, `pg_advisory_lock`, `SELECT ... FOR UPDATE SKIP LOCKED`.
- Migration dùng `ColType` + `SchemaManager` trước; raw SQL chỉ khi không tránh được và phải branch theo `m.get_database_backend()`.
- Cột `TIMESTAMP` mới phải khai `TIMESTAMP(6)` trên MySQL. Plan này không thêm cột timestamp nào ngoài `created_at`/`updated_at` mà `create_table` của loco tự thêm.
- `src/models/_entities/` generated từ Postgres bằng `cargo loco db entities`. Không sửa tay.
- SQLite không `MODIFY COLUMN` và không ép độ dài varchar — migration đổi cột phải bỏ qua SQLite.
- Comment trong code: tiếng Anh, một câu một dòng, không xuống dòng giữa câu.
- Không tự commit/push ngoài các bước commit trong plan. Không AI attribution trong message.
- Sau mỗi task: `cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms` phải sạch, và test phải xanh trên cả ba backend.

```bash
cargo test
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test
```

DB dev dựng bằng overlay dev-only (production overlay không publish cổng):

```bash
docker compose -p osgdev-pg -f docker-compose.yml -f docker-compose/postgres.yml \
               -f docker-compose/dev-ports.yml up -d db
```

---

## File Structure

**Tạo mới:**
- `migration/src/m20260818_000001_pools.rs`
- `migration/src/m20260818_000002_bucket_pool.rs`
- `src/models/pools.rs` — logic: validate provider, credential, decrypt
- `src/controllers/admin_pools.rs` — 5 route, `AdminCaller`
- `src/views/pools.rs` — shaper JSON, không bao giờ chứa secret
- `tests/models/pools.rs`
- `tests/requests/admin_pools.rs`
- `frontend/src/lib/pools.ts`

**Sửa:**
- `migration/src/lib.rs`
- `src/models/buckets.rs` — `create` nhận `pool_id`; bỏ `set_store`/`decrypt_store_secret`; bỏ `create_system`/`find_system_by_name`
- `src/controllers/buckets.rs` — `CreateParams` thêm `pool_id`
- `src/views/buckets.rs` — thêm `pool_id`, `pool_name`
- `src/controllers/mod.rs`, `src/views/mod.rs`, `src/models/mod.rs`, `src/app.rs`
- `src/fixtures/` — thêm `pools.yaml`, sửa `buckets` nếu có fixture
- `tests/models/buckets.rs`, `tests/models/quota.rs`, `tests/models/portability.rs`, `tests/models/concurrency.rs`, `tests/requests/buckets.rs`, `tests/requests/admin.rs` — mọi chỗ gọi `buckets::Model::create`
- `frontend/src/routes/_app/admin/buckets.tsx` — bỏ `ComingSoon`, nối API thật
- `frontend/src/routes/_app/buckets/index.tsx` — form tạo bucket chọn pool
- `frontend/src/lib/buckets.ts` — `createBucket` nhận `pool_id`
- `docs/docker.md`, `README.md`, `CLAUDE.md`

---

## Task 1: Bảng `pools` và model

**Files:**
- Create: `migration/src/m20260818_000001_pools.rs`, `src/models/pools.rs`, `tests/models/pools.rs`
- Modify: `migration/src/lib.rs`, `src/models/mod.rs`, `tests/models/mod.rs`

**Interfaces:**
- Consumes: `crypto::encrypt` / `crypto::decrypt`.
- Produces:
  - `pools::PROVIDERS: &[&str]`, `pools::validate_provider(&str) -> ModelResult<()>`
  - `pools::CreateParams { name, provider, region, api_endpoint, physical_bucket, access_id, access_secret }`
  - `pools::Model::create(db, &CreateParams) -> ModelResult<Model>`
  - `pools::Model::find_by_pid(db, &str) -> ModelResult<Model>`
  - `pools::Model::find_by_id(db, i32) -> ModelResult<Model>`
  - `pools::Model::list_all(db) -> ModelResult<Vec<Model>>`
  - `pools::Model::update_config(self, db, &UpdateParams) -> ModelResult<Model>`
  - `pools::Model::decrypt_secret(&self) -> ModelResult<String>`

- [ ] **Step 1: Viết test**

Tạo `tests/models/pools.rs`:

```rust
use loco_rs::testing::prelude::*;
use object_storage_gate::{app::App, models::pools};
use serial_test::serial;

fn params(name: &str) -> pools::CreateParams {
    pools::CreateParams {
        name: name.to_string(),
        provider: pools::PROVIDER_MINIO.to_string(),
        region: Some("ap-southeast-1".to_string()),
        api_endpoint: Some("https://minio.internal:9000".to_string()),
        physical_bucket: "osg-main".to_string(),
        access_id: Some("UPSTREAMKEYID".to_string()),
        access_secret: Some("upstream-secret-value".to_string()),
    }
}

#[tokio::test]
#[serial]
async fn create_round_trips_and_encrypts_the_secret() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    let pool = pools::Model::create(db, &params("main")).await.unwrap();

    assert_eq!(pool.name, "main");
    assert_eq!(pool.physical_bucket, "osg-main");
    assert_eq!(pool.access_id.as_deref(), Some("UPSTREAMKEYID"));

    // Stored encrypted, recoverable in process.
    let blob = pool.access_secret_encrypted.clone().unwrap();
    assert_ne!(blob, b"upstream-secret-value".to_vec());
    assert_eq!(pool.decrypt_secret().unwrap(), "upstream-secret-value");
}

#[tokio::test]
#[serial]
async fn pool_names_are_unique() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    pools::Model::create(db, &params("main")).await.unwrap();
    assert!(pools::Model::create(db, &params("main")).await.is_err());
}

#[tokio::test]
#[serial]
async fn unknown_provider_is_rejected() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    let mut p = params("weird");
    p.provider = "dropbox".to_string();
    assert!(pools::Model::create(db, &p).await.is_err());
}

#[tokio::test]
#[serial]
async fn physical_bucket_is_required() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    let mut p = params("empty");
    p.physical_bucket = String::new();
    assert!(pools::Model::create(db, &p).await.is_err());
}

/// Updating without a new secret keeps the stored one — the admin form does not echo it back,
/// so an empty field must mean "unchanged", never "erase".
#[tokio::test]
#[serial]
async fn update_without_a_secret_keeps_the_stored_one() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    let pool = pools::Model::create(db, &params("main")).await.unwrap();
    let updated = pool
        .update_config(
            db,
            &pools::UpdateParams {
                region: Some("us-east-1".to_string()),
                api_endpoint: None,
                physical_bucket: None,
                access_id: None,
                access_secret: None,
            },
        )
        .await
        .unwrap();

    assert_eq!(updated.region.as_deref(), Some("us-east-1"));
    assert_eq!(updated.decrypt_secret().unwrap(), "upstream-secret-value");
}

#[tokio::test]
#[serial]
async fn update_with_a_secret_replaces_it() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    let pool = pools::Model::create(db, &params("main")).await.unwrap();
    let updated = pool
        .update_config(
            db,
            &pools::UpdateParams {
                region: None,
                api_endpoint: None,
                physical_bucket: None,
                access_id: None,
                access_secret: Some("rotated-secret".to_string()),
            },
        )
        .await
        .unwrap();

    assert_eq!(updated.decrypt_secret().unwrap(), "rotated-secret");
}

/// A pool created without credentials is the backfill case: it exists so buckets can point at
/// something, and every S3 request must fail loudly until an admin fills it in.
#[tokio::test]
#[serial]
async fn a_pool_without_a_secret_reports_it_rather_than_panicking() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    let mut p = params("bare");
    p.access_id = None;
    p.access_secret = None;
    let pool = pools::Model::create(db, &p).await.unwrap();

    assert!(pool.decrypt_secret().is_err());
    assert!(!pool.is_configured());
}
```

Thêm `mod pools;` vào `tests/models/mod.rs`.

- [ ] **Step 2: Chạy để chắc nó fail**

Run: `cargo test --test mod models::pools 2>&1 | tail -20`
Expected: FAIL biên dịch — `unresolved import object_storage_gate::models::pools`.

- [ ] **Step 3: Viết migration**

Tạo `migration/src/m20260818_000001_pools.rs`:

```rust
use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        // A pool is an upstream store plus the physical bucket inside it.
        // It is deliberately not a `buckets` row: `user_id IS NULL` as a sentinel for "system pool" is what turned a deleted owner's private bucket into a shared one, which m20260817 had to fix.
        // A client can never address a pool.
        create_table(
            m,
            "pools",
            &[
                ("id", ColType::PkAuto),
                ("pid", ColType::UuidUniq),
                ("name", ColType::StringUniq),
                ("provider", ColType::StringWithDefault("aws".to_string())),
                ("region", ColType::StringNull),
                ("api_endpoint", ColType::StringNull),
                ("physical_bucket", ColType::String),
                ("access_id", ColType::StringNull),
                // Same AES-GCM envelope as access_keys.secret_encrypted (models/crypto.rs).
                // Reversible on purpose: the gateway signs upstream requests with it.
                ("access_secret_encrypted", ColType::BlobNull),
            ],
            &[],
        )
        .await?;
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        drop_table(m, "pools").await?;
        Ok(())
    }
}
```

Đăng ký trong `migration/src/lib.rs`: thêm `mod m20260818_000001_pools;` sau `mod m20260817_000006_quota_checks;`, và `Box::new(m20260818_000001_pools::Migration),` ngay **trên** dòng `// inject-above (do not remove this comment)`.

- [ ] **Step 4: Áp migration và sinh entity**

```bash
DB_TYPE=postgres cargo loco db reset
DB_TYPE=postgres cargo loco db entities
```

Ràng buộc CLAUDE.md: `db entities` phải chạy đối với Postgres. Chạy trên MySQL hoặc SQLite ra kiểu cột khác và làm hỏng model.

Sau lệnh này có `src/models/_entities/pools.rs`.

- [ ] **Step 5: Viết model**

Tạo `src/models/pools.rs`:

```rust
use loco_rs::prelude::*;
use uuid::Uuid;

use super::crypto;

pub use super::_entities::pools::{ActiveModel, Column, Entity, Model};

pub const PROVIDER_AWS: &str = "aws";
pub const PROVIDER_R2: &str = "r2";
pub const PROVIDER_B2: &str = "b2";
pub const PROVIDER_SPACES: &str = "spaces";
pub const PROVIDER_MINIO: &str = "minio";
pub const PROVIDER_CEPH: &str = "ceph";
pub const PROVIDER_CUSTOM: &str = "custom";

pub const PROVIDERS: &[&str] = &[
    PROVIDER_AWS,
    PROVIDER_R2,
    PROVIDER_B2,
    PROVIDER_SPACES,
    PROVIDER_MINIO,
    PROVIDER_CEPH,
    PROVIDER_CUSTOM,
];

/// Validates a provider string against the list the gateway knows how to sign for.
///
/// # Errors
///
/// Returns a message error for anything else.
pub fn validate_provider(provider: &str) -> ModelResult<()> {
    if PROVIDERS.contains(&provider) {
        return Ok(());
    }
    Err(ModelError::msg(
        "unknown provider; expected one of aws, r2, b2, spaces, minio, ceph, custom",
    ))
}

#[derive(Debug, Clone, Default)]
pub struct CreateParams {
    pub name: String,
    pub provider: String,
    pub region: Option<String>,
    pub api_endpoint: Option<String>,
    pub physical_bucket: String,
    pub access_id: Option<String>,
    /// Plaintext on the way in, never stored that way.
    pub access_secret: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateParams {
    pub region: Option<String>,
    pub api_endpoint: Option<String>,
    pub physical_bucket: Option<String>,
    pub access_id: Option<String>,
    /// `None` means keep the stored secret. The admin form never echoes it back, so an empty field must mean unchanged, never erase.
    pub access_secret: Option<String>,
}

#[async_trait::async_trait]
impl ActiveModelBehavior for super::_entities::pools::ActiveModel {
    async fn before_save<C>(self, _db: &C, insert: bool) -> Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        if insert {
            let mut this = self;
            this.pid = ActiveValue::Set(Uuid::new_v4());
            Ok(this)
        } else {
            Ok(self)
        }
    }
}

impl Model {
    /// Creates a pool.
    ///
    /// # Errors
    /// Returns an error on an unknown provider, an empty physical bucket, or DB failure (incl. duplicate name).
    pub async fn create(db: &DatabaseConnection, params: &CreateParams) -> ModelResult<Self> {
        validate_provider(&params.provider)?;
        if params.physical_bucket.trim().is_empty() {
            return Err(ModelError::msg("physical_bucket must not be empty"));
        }

        Ok(ActiveModel {
            name: ActiveValue::set(params.name.clone()),
            provider: ActiveValue::set(params.provider.clone()),
            region: ActiveValue::set(params.region.clone()),
            api_endpoint: ActiveValue::set(params.api_endpoint.clone()),
            physical_bucket: ActiveValue::set(params.physical_bucket.clone()),
            access_id: ActiveValue::set(params.access_id.clone()),
            access_secret_encrypted: ActiveValue::set(
                params.access_secret.as_deref().map(crypto::encrypt),
            ),
            ..Default::default()
        }
        .insert(db)
        .await?)
    }

    /// # Errors
    /// Returns an error when no pool has that pid, or on DB failure.
    pub async fn find_by_pid(db: &DatabaseConnection, pid: &str) -> ModelResult<Self> {
        let parsed = Uuid::parse_str(pid).map_err(|e| ModelError::Any(e.into()))?;
        Entity::find()
            .filter(Column::Pid.eq(parsed))
            .one(db)
            .await?
            .ok_or(ModelError::EntityNotFound)
    }

    /// # Errors
    /// Returns an error when no pool has that id, or on DB failure.
    pub async fn find_by_id(db: &DatabaseConnection, id: i32) -> ModelResult<Self> {
        Entity::find_by_id(id)
            .one(db)
            .await?
            .ok_or(ModelError::EntityNotFound)
    }

    /// # Errors
    /// Returns an error on DB failure.
    pub async fn list_all(db: &DatabaseConnection) -> ModelResult<Vec<Self>> {
        Ok(Entity::find().order_by_asc(Column::Name).all(db).await?)
    }

    /// Replaces the fields the admin form submits.
    /// A `None` leaves the stored value alone; that is what an untouched field means.
    ///
    /// # Errors
    /// Returns an error on an empty physical bucket or DB failure.
    pub async fn update_config(
        self,
        db: &DatabaseConnection,
        params: &UpdateParams,
    ) -> ModelResult<Self> {
        if let Some(bucket) = &params.physical_bucket {
            if bucket.trim().is_empty() {
                return Err(ModelError::msg("physical_bucket must not be empty"));
            }
        }

        let mut am: ActiveModel = self.into();
        if let Some(region) = &params.region {
            am.region = ActiveValue::set(Some(region.clone()));
        }
        if let Some(endpoint) = &params.api_endpoint {
            am.api_endpoint = ActiveValue::set(Some(endpoint.clone()));
        }
        if let Some(bucket) = &params.physical_bucket {
            am.physical_bucket = ActiveValue::set(bucket.clone());
        }
        if let Some(access_id) = &params.access_id {
            am.access_id = ActiveValue::set(Some(access_id.clone()));
        }
        if let Some(secret) = &params.access_secret {
            am.access_secret_encrypted = ActiveValue::set(Some(crypto::encrypt(secret)));
        }
        Ok(am.update(db).await?)
    }

    /// Whether this pool can actually be used to sign an upstream request.
    /// The backfill migration creates a pool with no credentials so existing buckets have something to point at; every S3 request against it must fail loudly rather than silently.
    #[must_use]
    pub fn is_configured(&self) -> bool {
        self.access_id.is_some() && self.access_secret_encrypted.is_some()
    }

    /// Plaintext upstream secret, for signing requests to the object store.
    /// Never return this over the API.
    ///
    /// # Errors
    /// Returns an error when no secret is stored or decryption fails.
    pub fn decrypt_secret(&self) -> ModelResult<String> {
        let blob = self
            .access_secret_encrypted
            .as_deref()
            .ok_or_else(|| ModelError::msg("pool has no upstream secret configured"))?;
        crypto::decrypt(blob).map_err(|e| ModelError::Message(e.to_string()))
    }
}
```

Thêm `use sea_orm::QueryOrder;` nếu `order_by_asc` không có trong prelude.

Thêm `pub mod pools;` vào `src/models/mod.rs`.

- [ ] **Step 6: Chạy test ba backend**

```bash
cargo test --test mod models::pools 2>&1 | tail -10
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test --test mod models::pools 2>&1 | tail -5
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test --test mod models::pools 2>&1 | tail -5
```

Expected: PASS 7 test trên cả ba.

- [ ] **Step 7: Clippy và commit**

```bash
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add migration/ src/ tests/
git commit -m "feat(pools): add the pools table and model

A pool is an upstream store plus the physical bucket inside it. Deliberately
not a buckets row: user_id IS NULL as a sentinel for system pool is what turned
a deleted owner's private bucket into a shared one, and a client can never
address a pool anyway. An empty secret on update means unchanged, never erase,
because the admin form does not echo it back."
```

---

## Task 2: `buckets.pool_id` với backfill

**Files:**
- Create: `migration/src/m20260818_000002_bucket_pool.rs`
- Modify: `migration/src/lib.rs`, `src/models/buckets.rs`, `src/controllers/buckets.rs`, `src/views/buckets.rs`, `src/fixtures/`, mọi test gọi `buckets::Model::create`
- Test: `tests/models/buckets.rs`

**Interfaces:**
- Consumes: `pools::Model` (task 1).
- Produces: `buckets::Model::create(db, user_id, pool_id, name, max_bytes)` — thêm tham số `pool_id`. `buckets::Model` mất `provider`, `region`, `api_endpoint`, `access_id`, `access_secret_encrypted`, `set_store`, `decrypt_store_secret`, `create_system`, `find_system_by_name`, `is_system`.

- [ ] **Step 1: Viết test**

Thêm vào `tests/models/buckets.rs`:

```rust
/// A bucket cannot exist without a pool: the gateway would have nowhere to proxy it.
#[tokio::test]
#[serial]
async fn a_bucket_belongs_to_a_pool() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(db, "user1@example.com")
        .await
        .unwrap();
    let pool = pools::Model::create(
        db,
        &pools::CreateParams {
            name: "main".to_string(),
            provider: pools::PROVIDER_MINIO.to_string(),
            physical_bucket: "osg-main".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let bucket = buckets::Model::create(db, user.id, pool.id, "media-cdn", 0)
        .await
        .unwrap();

    assert_eq!(bucket.pool_id, pool.id);
}

/// ON DELETE RESTRICT, not SET NULL: a silently orphaned bucket is the bug m20260817 fixed.
#[tokio::test]
#[serial]
async fn a_pool_with_buckets_cannot_be_deleted() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(db, "user1@example.com")
        .await
        .unwrap();
    let pool = pools::Model::create(
        db,
        &pools::CreateParams {
            name: "main".to_string(),
            provider: pools::PROVIDER_MINIO.to_string(),
            physical_bucket: "osg-main".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    buckets::Model::create(db, user.id, pool.id, "media-cdn", 0)
        .await
        .unwrap();

    let am: pools::ActiveModel = pool.into();
    assert!(am.delete(db).await.is_err());
}
```

Thêm import `pools` vào đầu file.

- [ ] **Step 2: Chạy để chắc nó fail**

Run: `cargo test --test mod models::buckets 2>&1 | tail -10`
Expected: FAIL biên dịch — `create` nhận 4 tham số, không phải 5.

- [ ] **Step 3: Viết migration với backfill**

Tạo `migration/src/m20260818_000002_bucket_pool.rs`:

```rust
use loco_rs::schema::*;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// Placeholder physical bucket for the backfill pool.
/// Deliberately not a plausible name: every S3 request against this pool must fail until an admin replaces it, and the failure should read as unconfigured rather than as a typo.
const BACKFILL_PHYSICAL_BUCKET: &str = "CHANGE-ME";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        let conn = m.get_connection();
        let backend = m.get_database_backend();

        // Count existing buckets before adding a NOT NULL column with no default.
        let existing: i64 = {
            let row = conn
                .query_one(Statement::from_string(
                    backend,
                    "SELECT COUNT(*) AS c FROM buckets".to_string(),
                ))
                .await?;
            row.map_or(Ok(0), |r| r.try_get::<i64>("", "c"))?
        };

        // Nullable first, backfill, then tighten — the only order that works with rows present.
        add_column(m, "buckets", "pool_id", ColType::IntegerNull).await?;

        if existing > 0 {
            conn.execute(Statement::from_string(
                backend,
                format!(
                    "INSERT INTO pools (pid, name, provider, physical_bucket) \
                     VALUES ('{}', 'default', 'custom', '{BACKFILL_PHYSICAL_BUCKET}')",
                    uuid::Uuid::new_v4()
                ),
            ))
            .await?;
            conn.execute(Statement::from_string(
                backend,
                "UPDATE buckets SET pool_id = (SELECT id FROM pools WHERE name = 'default')"
                    .to_string(),
            ))
            .await?;
        }

        // SQLite cannot modify a column or add a foreign key after the fact.
        // The application layer enforces both, and SQLite is single-node dev/test only.
        if !matches!(backend, DatabaseBackend::Sqlite) {
            m.alter_table(
                Table::alter()
                    .table(Alias::new("buckets"))
                    .modify_column(
                        ColumnDef::new(Alias::new("pool_id"))
                            .integer()
                            .not_null()
                            .to_owned(),
                    )
                    .to_owned(),
            )
            .await?;

            m.alter_table(
                Table::alter()
                    .table(Alias::new("buckets"))
                    .add_foreign_key(
                        TableForeignKey::new()
                            .name("fk_buckets_pool")
                            .from_tbl(Alias::new("buckets"))
                            .from_col(Alias::new("pool_id"))
                            .to_tbl(Alias::new("pools"))
                            .to_col(Alias::new("id"))
                            // RESTRICT, not SET NULL: a silently orphaned bucket is exactly the
                            // bug m20260817 had to fix on the users side.
                            .on_delete(ForeignKeyAction::Restrict)
                            .to_owned(),
                    )
                    .to_owned(),
            )
            .await?;
        }

        // The six store columns move to pools.
        for col in [
            "provider",
            "region",
            "api_endpoint",
            "access_id",
            "access_secret_encrypted",
        ] {
            remove_column(m, "buckets", col).await?;
        }

        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        add_column(
            m,
            "buckets",
            "provider",
            ColType::StringWithDefault("internal".to_string()),
        )
        .await?;
        add_column(m, "buckets", "region", ColType::StringNull).await?;
        add_column(m, "buckets", "api_endpoint", ColType::StringNull).await?;
        add_column(m, "buckets", "access_id", ColType::StringNull).await?;
        add_column(m, "buckets", "access_secret_encrypted", ColType::BlobNull).await?;
        remove_column(m, "buckets", "pool_id").await?;
        Ok(())
    }
}
```

Thêm `uuid = { version = "1.6", features = ["v4"] }` vào `migration/Cargo.toml` nếu chưa có.

Đăng ký trong `migration/src/lib.rs` phía trên marker.

Ghi chú về SQLite: cột giữ nguyên nullable và không có FK. `buckets::Model::create` luôn truyền `pool_id`, và không đường nào tạo bucket mà thiếu nó — nên hành vi giống hệt ba backend. Ghi lại trong doc-comment của migration.

- [ ] **Step 4: Sinh lại entity và sửa model**

```bash
DB_TYPE=postgres cargo loco db reset
DB_TYPE=postgres cargo loco db entities
```

Trong `src/models/buckets.rs`:

- `create` thêm tham số `pool_id: i32` sau `user_id`, set `pool_id: ActiveValue::set(pool_id)`.
- Xoá `create_system`, `find_system_by_name`, `is_system`, `StoreParams`, `set_store`, `decrypt_store_secret`, `PROVIDER_*`, `PROVIDERS` — provider giờ thuộc `pools`.
- Giữ `is_public`, `is_unlimited`, `list_for_user`, `find_by_user_and_name`, `find_by_pid_for_user`, `validate_name`.

Bỏ `use super::crypto;` nếu không còn dùng.

- [ ] **Step 5: Sửa controller và view**

`src/controllers/buckets.rs`:

```rust
#[derive(Debug, Deserialize, Serialize)]
pub struct CreateParams {
    pub name: String,
    /// Required: a bucket with no pool has nowhere to proxy to.
    pub pool_id: String,
    /// Required on purpose: `0` means unlimited, and unlimited must be a decision, never a default.
    pub max_bytes: i64,
}
```

`pool_id` là `pid` của pool (chuỗi), không phải id nội bộ — API không bao giờ để lộ id tăng dần. Handler `create` giải nó:

```rust
    let pool = pools::Model::find_by_pid(&ctx.db, &params.pool_id)
        .await
        .map_err(|_| Error::BadRequest("unknown pool".to_string()))?;
    let bucket = buckets::Model::create(&ctx.db, caller.user.id, pool.id, &params.name, params.max_bytes)
```

`src/views/buckets.rs` — `BucketDetail` thêm `pool_id: String` (pid của pool) và `pool_name: String`. Nghĩa là `index`/`show` phải load pool. `list_for_user` trả N bucket → tránh N+1 bằng một truy vấn pool duy nhất:

```rust
    let rows = buckets::Model::list_for_user(&ctx.db, caller.user.id).await?;
    let pools = pools::Model::list_all(&ctx.db).await?;   // vài dòng, không phải N truy vấn
    let by_id: HashMap<i32, &pools::Model> = pools.iter().map(|p| (p.id, p)).collect();
```

- [ ] **Step 6: Sửa fixture và mọi test gọi `create`**

Tạo `src/fixtures/pools.yaml`:

```yaml
---
- id: 1
  pid: 33333333-3333-3333-3333-333333333333
  name: main
  provider: minio
  region: ap-southeast-1
  api_endpoint: "http://localhost:9000"
  physical_bucket: osg-main
  access_id: FIXTUREUPSTREAMID
  # AES-GCM of "fixture-upstream-secret" is process-specific, so the fixture leaves it null.
  # Tests that need a working pool call pools::Model::create instead of relying on the fixture.
  access_secret_encrypted: null
  created_at: "2023-11-12T12:34:56.789Z"
  updated_at: "2023-11-12T12:34:56.789Z"
```

`src/app.rs` — `seed` phải nạp `pools` **trước** `buckets`, và `truncate` phải xoá `buckets` trước `pools`:

```rust
    async fn truncate(ctx: &AppContext) -> Result<()> {
        truncate_table(&ctx.db, objects::Entity).await?;
        truncate_table(&ctx.db, access_key_permissions::Entity).await?;
        truncate_table(&ctx.db, access_key_prefixes::Entity).await?;
        truncate_table(&ctx.db, access_keys::Entity).await?;
        truncate_table(&ctx.db, buckets::Entity).await?;
        truncate_table(&ctx.db, pools::Entity).await?;
        truncate_table(&ctx.db, users::Entity).await?;
        Ok(())
    }
```

`seed` nạp `pools.yaml` theo cùng pattern `users.yaml`, giữ nguyên khối `match` swallow lỗi `reset_autoincrement` trên MySQL.

Rồi sửa mọi lời gọi `buckets::Model::create(db, user.id, "name", 0)` thành `buckets::Model::create(db, user.id, pool_id, "name", 0)`. Tìm hết:

```bash
grep -rn "buckets::Model::create" tests/ src/
```

Thêm một helper vào `tests/models/mod.rs` để khỏi lặp:

```rust
/// The seeded pool every model test hangs its buckets off.
pub async fn seeded_pool(db: &sea_orm::DatabaseConnection) -> i32 {
    object_storage_gate::models::pools::Model::find_by_name(db, "main")
        .await
        .expect("fixture pool 'main'")
        .id
}
```

Cần thêm `find_by_name` vào `pools::Model` — một truy vấn `eq(Column::Name, name)`.

- [ ] **Step 7: Chạy test ba backend**

```bash
cargo test 2>&1 | tail -5
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test 2>&1 | tail -5
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test 2>&1 | tail -5
```

Snapshot của `tests/models/snapshots/*buckets*` sẽ lệch vì bảng mất 5 cột và thêm 1. Kiểm nội dung mới rồi áp:

```bash
for f in tests/models/snapshots/*.snap.new tests/requests/snapshots/*.snap.new; do
  [ -e "$f" ] && echo "=== $f ===" && cat "$f"
done
```

- [ ] **Step 8: Commit**

```bash
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add -A migration/ src/ tests/
git commit -m "feat(db): bind every bucket to a pool

Moves the six upstream-store columns from buckets to pools and adds
buckets.pool_id NOT NULL with ON DELETE RESTRICT. The migration adds the column
nullable, backfills a 'default' pool when rows exist, then tightens it — the
only order that works on a populated table. A pool created by the backfill has
no credentials, so every S3 request against it fails loudly until an admin
fills it in."
```

---

## Task 3: `/api/admin/pools` CRUD

**Files:**
- Create: `src/controllers/admin_pools.rs`, `src/views/pools.rs`, `tests/requests/admin_pools.rs`
- Modify: `src/controllers/mod.rs`, `src/views/mod.rs`, `src/app.rs`, `tests/requests/mod.rs`

**Interfaces:**
- Consumes: `AdminCaller` (P1), `pools::Model` (task 1).
- Produces: `GET|POST /api/admin/pools`, `GET|PATCH|DELETE /api/admin/pools/{pid}`. `PoolResponse` — không bao giờ chứa secret.

- [ ] **Step 1: Viết test**

Tạo `tests/requests/admin_pools.rs`:

```rust
use loco_rs::testing::prelude::*;
use object_storage_gate::{app::App, models::pools};
use serial_test::serial;

use super::prepare_data;

fn body(name: &str) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "provider": "minio",
        "region": "ap-southeast-1",
        "api_endpoint": "https://minio.internal:9000",
        "physical_bucket": "osg-main",
        "access_id": "UPSTREAMKEYID",
        "access_secret": "upstream-secret-value"
    })
}

#[tokio::test]
#[serial]
async fn admin_can_create_and_list_a_pool() {
    request::<App, _, _>(|request, ctx| async move {
        let admin = prepare_data::init_admin_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&admin.token);
        let created = request
            .post("/api/admin/pools")
            .add_header(k, v)
            .json(&body("main"))
            .await;
        assert_eq!(created.status_code(), 200);

        // The secret must never come back out.
        let text = created.text();
        assert!(!text.contains("upstream-secret-value"));
        assert!(!text.contains("access_secret"));
        assert!(text.contains("UPSTREAMKEYID"));
        assert!(text.contains("osg-main"));

        let (k, v) = prepare_data::auth_header(&admin.token);
        let listed = request.get("/api/admin/pools").add_header(k, v).await;
        assert_eq!(listed.json::<Vec<serde_json::Value>>().len(), 1);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_plain_user_is_refused() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&user.token);
        let listed = request.get("/api/admin/pools").add_header(k, v).await;
        assert_eq!(listed.status_code(), 403);
        assert!(listed.text().contains("admin_required"));

        let (k, v) = prepare_data::auth_header(&user.token);
        let created = request
            .post("/api/admin/pools")
            .add_header(k, v)
            .json(&body("sneaky"))
            .await;
        assert_eq!(created.status_code(), 403);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn unknown_provider_and_empty_physical_bucket_are_rejected() {
    request::<App, _, _>(|request, ctx| async move {
        let admin = prepare_data::init_admin_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&admin.token);
        let mut bad = body("weird");
        bad["provider"] = serde_json::json!("dropbox");
        assert_eq!(
            request.post("/api/admin/pools").add_header(k, v).json(&bad).await.status_code(),
            400
        );

        let (k, v) = prepare_data::auth_header(&admin.token);
        let mut bad = body("empty");
        bad["physical_bucket"] = serde_json::json!("");
        assert_eq!(
            request.post("/api/admin/pools").add_header(k, v).json(&bad).await.status_code(),
            400
        );
    })
    .await;
}

/// An untouched secret field means unchanged, never erase.
#[tokio::test]
#[serial]
async fn patch_without_a_secret_keeps_the_stored_one() {
    request::<App, _, _>(|request, ctx| async move {
        let admin = prepare_data::init_admin_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&admin.token);
        let created = request
            .post("/api/admin/pools")
            .add_header(k, v)
            .json(&body("main"))
            .await;
        let pid = created.json::<serde_json::Value>()["pid"]
            .as_str()
            .unwrap()
            .to_string();

        let (k, v) = prepare_data::auth_header(&admin.token);
        let patched = request
            .patch(&format!("/api/admin/pools/{pid}"))
            .add_header(k, v)
            .json(&serde_json::json!({ "region": "us-east-1" }))
            .await;
        assert_eq!(patched.status_code(), 200);

        let pool = pools::Model::find_by_pid(&ctx.db, &pid).await.unwrap();
        assert_eq!(pool.region.as_deref(), Some("us-east-1"));
        assert_eq!(pool.decrypt_secret().unwrap(), "upstream-secret-value");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_pool_with_buckets_cannot_be_deleted() {
    request::<App, _, _>(|request, ctx| async move {
        let admin = prepare_data::init_admin_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&admin.token);
        let created = request
            .post("/api/admin/pools")
            .add_header(k, v)
            .json(&body("main"))
            .await;
        let pid = created.json::<serde_json::Value>()["pid"].as_str().unwrap().to_string();
        let pool = pools::Model::find_by_pid(&ctx.db, &pid).await.unwrap();

        object_storage_gate::models::buckets::Model::create(
            &ctx.db, admin.user.id, pool.id, "media-cdn", 0,
        )
        .await
        .unwrap();

        let (k, v) = prepare_data::auth_header(&admin.token);
        let deleted = request
            .delete(&format!("/api/admin/pools/{pid}"))
            .add_header(k, v)
            .await;
        assert_eq!(deleted.status_code(), 400);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn an_empty_pool_can_be_deleted() {
    request::<App, _, _>(|request, ctx| async move {
        let admin = prepare_data::init_admin_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&admin.token);
        let created = request
            .post("/api/admin/pools")
            .add_header(k, v)
            .json(&body("spare"))
            .await;
        let pid = created.json::<serde_json::Value>()["pid"].as_str().unwrap().to_string();

        let (k, v) = prepare_data::auth_header(&admin.token);
        assert_eq!(
            request.delete(&format!("/api/admin/pools/{pid}")).add_header(k, v).await.status_code(),
            200
        );
    })
    .await;
}
```

Thêm `mod admin_pools;` vào `tests/requests/mod.rs`.

- [ ] **Step 2: Chạy để chắc nó fail**

Run: `cargo test --test mod requests::admin_pools 2>&1 | tail -10`
Expected: FAIL — route trả 404 (POST) hoặc 200 kèm HTML của SPA (GET).

- [ ] **Step 3: Viết view**

Tạo `src/views/pools.rs`:

```rust
use serde::{Deserialize, Serialize};

use crate::models::_entities::pools;

/// The admin-facing shape of a pool.
/// Lists fields by hand and has no secret field at all: `access_secret_encrypted` exists to be signed with, never to be read back.
#[derive(Debug, Deserialize, Serialize)]
pub struct PoolResponse {
    pub pid: String,
    pub name: String,
    pub provider: String,
    pub region: Option<String>,
    pub api_endpoint: Option<String>,
    pub physical_bucket: String,
    pub access_id: Option<String>,
    /// Whether a credential is stored at all — enough for the console to warn, without revealing it.
    pub is_configured: bool,
    pub created_at: String,
}

impl PoolResponse {
    #[must_use]
    pub fn new(pool: &pools::Model) -> Self {
        Self {
            pid: pool.pid.to_string(),
            name: pool.name.clone(),
            provider: pool.provider.clone(),
            region: pool.region.clone(),
            api_endpoint: pool.api_endpoint.clone(),
            physical_bucket: pool.physical_bucket.clone(),
            access_id: pool.access_id.clone(),
            is_configured: pool.access_id.is_some() && pool.access_secret_encrypted.is_some(),
            created_at: pool.created_at.to_rfc3339(),
        }
    }
}
```

Thêm `pub mod pools;` vào `src/views/mod.rs`.

- [ ] **Step 4: Viết controller**

Tạo `src/controllers/admin_pools.rs`:

```rust
//! Admin-only pool management.
//!
//! A pool is the upstream store a bucket proxies to. Without at least one configured pool the gateway cannot serve a single S3 request, so this tree is a prerequisite for the whole data plane.
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    controllers::api::AdminCaller,
    models::{buckets, pools},
    views::pools::PoolResponse,
};

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateBody {
    pub name: String,
    pub provider: String,
    pub region: Option<String>,
    pub api_endpoint: Option<String>,
    pub physical_bucket: String,
    pub access_id: Option<String>,
    /// Plaintext on the way in, stored AES-GCM encrypted, never returned.
    pub access_secret: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateBody {
    pub region: Option<String>,
    pub api_endpoint: Option<String>,
    pub physical_bucket: Option<String>,
    pub access_id: Option<String>,
    /// Absent means keep the stored secret. The form never echoes it back, so absent cannot mean erase.
    pub access_secret: Option<String>,
}

fn bad_request(e: &ModelError) -> Error {
    Error::BadRequest(e.to_string())
}

async fn load(db: &DatabaseConnection, pid: &str) -> Result<pools::Model> {
    pools::Model::find_by_pid(db, pid)
        .await
        .map_err(|_| Error::NotFound)
}

#[debug_handler]
async fn index(_admin: AdminCaller, State(ctx): State<AppContext>) -> Result<Response> {
    let rows = pools::Model::list_all(&ctx.db).await?;
    format::json(rows.iter().map(PoolResponse::new).collect::<Vec<_>>())
}

#[debug_handler]
async fn create(
    _admin: AdminCaller,
    State(ctx): State<AppContext>,
    Json(body): Json<CreateBody>,
) -> Result<Response> {
    let pool = pools::Model::create(
        &ctx.db,
        &pools::CreateParams {
            name: body.name,
            provider: body.provider,
            region: body.region,
            api_endpoint: body.api_endpoint,
            physical_bucket: body.physical_bucket,
            access_id: body.access_id,
            access_secret: body.access_secret,
        },
    )
    .await
    .map_err(|e| bad_request(&e))?;

    format::json(PoolResponse::new(&pool))
}

#[debug_handler]
async fn show(
    _admin: AdminCaller,
    Path(pid): Path<String>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    let pool = load(&ctx.db, &pid).await?;
    format::json(PoolResponse::new(&pool))
}

#[debug_handler]
async fn update(
    _admin: AdminCaller,
    Path(pid): Path<String>,
    State(ctx): State<AppContext>,
    Json(body): Json<UpdateBody>,
) -> Result<Response> {
    let pool = load(&ctx.db, &pid).await?;
    let updated = pool
        .update_config(
            &ctx.db,
            &pools::UpdateParams {
                region: body.region,
                api_endpoint: body.api_endpoint,
                physical_bucket: body.physical_bucket,
                access_id: body.access_id,
                access_secret: body.access_secret,
            },
        )
        .await
        .map_err(|e| bad_request(&e))?;

    format::json(PoolResponse::new(&updated))
}

#[debug_handler]
async fn destroy(
    _admin: AdminCaller,
    Path(pid): Path<String>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    let db = &ctx.db;
    let pool = load(db, &pid).await?;

    // The foreign key is RESTRICT, so the DB would refuse anyway — but a 400 with a sentence
    // beats a 500 with a constraint name.
    let count = buckets::Model::count_for_pool(db, pool.id).await?;
    if count > 0 {
        return Err(Error::BadRequest(format!(
            "{count} bucket(s) still use this pool; move or delete them first"
        )));
    }

    let am: pools::ActiveModel = pool.into();
    am.delete(db).await?;

    format::json(())
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/admin/pools")
        .add("/", get(index).post(create))
        .add("/{pid}", get(show).patch(update).delete(destroy))
}
```

Thêm `buckets::Model::count_for_pool(db, pool_id) -> ModelResult<u64>` — một `Entity::find().filter(Column::PoolId.eq(pool_id)).count(db)`.

Đăng ký trong `src/controllers/mod.rs` và `src/app.rs`.

- [ ] **Step 5: Chạy test và kiểm route**

```bash
cargo test --test mod requests::admin_pools 2>&1 | tail -10
cargo loco routes 2>/dev/null | grep pools
```

Expected: PASS 6 test; `cargo loco routes` liệt kê 5 route.

- [ ] **Step 6: Ba backend, clippy, commit**

```bash
cargo test 2>&1 | tail -3
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test 2>&1 | tail -3
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test 2>&1 | tail -3
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add src/ tests/
git commit -m "feat(admin): pool management API

Five routes under /api/admin/pools, gated by AdminCaller. PoolResponse has no
secret field at all: access_secret_encrypted exists to be signed with, never to
be read back. An absent access_secret on PATCH keeps the stored one, because the
form does not echo it and absent cannot mean erase."
```

---

## Task 4: Console — màn Pool thật, và bucket chọn pool

**Files:**
- Create: `frontend/src/lib/pools.ts`
- Modify: `frontend/src/routes/_app/admin/buckets.tsx`, `frontend/src/routes/_app/buckets/index.tsx`, `frontend/src/lib/buckets.ts`
- Test: `frontend/src/lib/pools.test.ts`

**Interfaces:**
- Consumes: `/api/admin/pools` (task 3), `run()` (P4).
- Produces: `lib/pools.ts` xuất `listPools`, `createPool`, `getPool`, `updatePool`, `deletePool`.

- [ ] **Step 1: Viết test cho lib**

Tạo `frontend/src/lib/pools.test.ts`:

```ts
import { describe, expect, it, vi } from "vitest";
import { setToken } from "./auth";
import { createPool, updatePool } from "./pools";

describe("pools api", () => {
  it("posts a pool with its credentials", async () => {
    setToken("tok");
    const fetchMock = vi.fn(
      async () => new Response("{}", { status: 200 }),
    );
    vi.stubGlobal("fetch", fetchMock);

    await createPool({
      name: "main",
      provider: "minio",
      region: "ap-southeast-1",
      api_endpoint: "https://minio.internal:9000",
      physical_bucket: "osg-main",
      access_id: "ID",
      access_secret: "SECRET",
    });

    const [path, init] = fetchMock.mock.calls[0];
    expect(path).toBe("/api/admin/pools");
    expect(JSON.parse(init.body as string).access_secret).toBe("SECRET");
  });

  it("omits access_secret when the field was left blank", async () => {
    setToken("tok");
    const fetchMock = vi.fn(
      async () => new Response("{}", { status: 200 }),
    );
    vi.stubGlobal("fetch", fetchMock);

    await updatePool("some-pid", { region: "us-east-1", access_secret: "" });

    const [, init] = fetchMock.mock.calls[0];
    const body = JSON.parse(init.body as string);
    expect(body.region).toBe("us-east-1");
    expect("access_secret" in body).toBe(false);
  });
});
```

Test thứ hai là cái quan trọng: ô secret để trống phải **không gửi** field đó, chứ không gửi chuỗi rỗng — gửi `""` là xoá credential của pool đang chạy.

- [ ] **Step 2: Chạy để chắc nó fail**

Run: `cd frontend && corepack pnpm vitest run src/lib/pools.test.ts`
Expected: FAIL — module không tồn tại.

- [ ] **Step 3: Viết client**

Tạo `frontend/src/lib/pools.ts`:

```ts
import { api } from "./auth";

export const PROVIDERS = [
  "aws",
  "r2",
  "b2",
  "spaces",
  "minio",
  "ceph",
  "custom",
] as const;

export type Provider = (typeof PROVIDERS)[number];

export type Pool = {
  pid: string;
  name: string;
  provider: Provider;
  region: string | null;
  api_endpoint: string | null;
  physical_bucket: string;
  access_id: string | null;
  is_configured: boolean;
  created_at: string;
};

export type PoolInput = {
  name: string;
  provider: Provider;
  region?: string;
  api_endpoint?: string;
  physical_bucket: string;
  access_id?: string;
  access_secret?: string;
};

export const listPools = () => api<Pool[]>("/api/admin/pools");

export const createPool = (input: PoolInput) =>
  api<Pool>("/api/admin/pools", {
    method: "POST",
    body: JSON.stringify(input),
  });

export const getPool = (pid: string) => api<Pool>(`/api/admin/pools/${pid}`);

/**
 * Blank fields are dropped, not sent.
 *
 * The server treats an absent `access_secret` as "keep the stored one"; sending an empty
 * string would erase the credential of a pool that is serving traffic.
 */
export const updatePool = (
  pid: string,
  patch: Partial<Omit<PoolInput, "name" | "provider">>,
) => {
  const body: Record<string, string> = {};
  for (const [k, v] of Object.entries(patch)) {
    if (v !== undefined && v !== "") body[k] = v;
  }
  return api<Pool>(`/api/admin/pools/${pid}`, {
    method: "PATCH",
    body: JSON.stringify(body),
  });
};

export const deletePool = (pid: string) =>
  api<void>(`/api/admin/pools/${pid}`, { method: "DELETE" });
```

- [ ] **Step 4: Viết lại màn Pool**

`frontend/src/routes/_app/admin/buckets.tsx` — bỏ `ComingSoon`, dựng bảng pool thật:

```tsx
export const Route = createFileRoute("/_app/admin/buckets")({
  beforeLoad: ({ context }) => {
    if (context.user.role !== "admin") throw redirect({ to: "/" });
  },
  loader: () => listPools(),
  component: AdminPools,
});
```

Bảng: `NAME` · `PROVIDER` · `PHYSICAL BUCKET` · `ENDPOINT` · `CREDENTIAL` · menu.

Cột `CREDENTIAL` đọc `is_configured`: `"đã cấu hình"` màu thường, `"CHƯA CÓ CREDENTIAL"` màu `var(--dgr)`. Pool sinh ra từ backfill nằm ở trạng thái thứ hai, và mọi request S3 vào nó sẽ fail — nên nó phải đập vào mắt.

Form tạo/sửa: `name` (chỉ lúc tạo), `provider` (select), `region`, `api_endpoint`, `physical_bucket`, `access_id`, `access_secret`.

Ô `access_secret` khi sửa: `placeholder="Để trống nếu không đổi"`, và **không** prefill — server không trả nó về. Có một dòng giải thích dưới ô:

```tsx
<div style={{ fontSize: 12, color: "var(--faint)", marginTop: 5, lineHeight: 1.5 }}>
  Để trống nếu không đổi. Máy chủ không trả secret về, nên không có gì để prefill.
</div>
```

Mọi mutation bọc trong `run()` với `onError: (m) => toast(m, "danger")`, rồi `router.invalidate()`.

- [ ] **Step 5: Form tạo bucket chọn pool**

`frontend/src/lib/buckets.ts` — `createBucket(name, max_bytes, pool_id)`.

`frontend/src/routes/_app/buckets/index.tsx`:

- Loader trả cả bucket và pool: `Promise.all([listBuckets(), listPools()])`.
  Nhưng `listPools` là route admin — user thường sẽ nhận 403. Nên cần một endpoint user đọc được.
  **Quyết định:** thêm `GET /api/pools` (dùng `Caller`, không phải `AdminCaller`) trả về danh sách rút gọn `{ pid, name, provider }` — đủ để chọn, không có credential, không có `physical_bucket`. Thêm route đó vào task 3 trước khi làm bước này.
- Form tạo bucket thêm select pool. Chỉ có một pool thì chọn sẵn và ẩn select đi.
- Không có pool nào: form đổi thành một dòng `"Chưa có pool nào. Liên hệ quản trị viên."` và nút tạo bị vô hiệu — đúng sự thật, không phải nút giả.

- [ ] **Step 6: Kiểm frontend**

```bash
cd frontend
corepack pnpm vitest run
corepack pnpm biome check
corepack pnpm exec tsc --noEmit
corepack pnpm build
```

`routeTree.gen.ts` được sinh lại trong bước build.

- [ ] **Step 7: Kiểm bằng tay**

```bash
# một cửa sổ
LOCO_ENV=development cargo loco start
# cửa sổ khác
cd frontend && corepack pnpm dev
```

Đi hết: đăng nhập admin → tạo pool → thấy `is_configured` đúng → sửa region mà không nhập secret → xác nhận secret **không** bị xoá (kiểm bằng `psql`: `SELECT access_secret_encrypted IS NOT NULL FROM pools`) → tạo bucket gắn pool → xoá pool còn bucket phải bị từ chối.

Bước xác nhận secret không bị xoá là bước quan trọng nhất của cả task này: một pool mất credential thì mọi request S3 của mọi tenant dùng nó đều chết.

- [ ] **Step 8: Commit**

```bash
git add -A frontend/ src/ tests/
git commit -m "feat(console): real pool management, and buckets pick a pool

P4 made this screen a status page because the form collected provider
credentials into React state that a reload threw away. It has a backend now.
A blank secret field is dropped from the PATCH body rather than sent empty:
the server reads absent as unchanged, and an empty string would erase the
credential of a pool that is serving traffic. Adds GET /api/pools so a
non-admin can pick a pool without seeing any credentials."
```

---

## Task 5: Tài liệu

**Files:**
- Modify: `docs/docker.md`, `README.md`, `CLAUDE.md`

- [ ] **Step 1: `docs/docker.md` — bước vận hành bắt buộc**

```markdown
## Pool phải có credential trước khi gateway phục vụ được

Migration `m20260818_000002_bucket_pool` tạo một pool tên `default` với
`physical_bucket = 'CHANGE-ME'` và **không có credential** khi cài đặt đã có
bucket từ trước. Mọi request S3 vào pool đó trả `InternalError` cho tới khi admin
điền vào.

Sau khi migrate:

1. Đăng nhập console bằng tài khoản admin.
2. Vào Admin → Pool. Pool `default` hiện `CHƯA CÓ CREDENTIAL`.
3. Điền `physical_bucket` thật, `access_id`, `access_secret`, và `api_endpoint`
   nếu không dùng AWS.

Cài mới thì không có bucket nào nên không có pool `default`; tạo pool đầu tiên
bằng tay ở cùng màn hình đó.
```

- [ ] **Step 2: `README.md` — bảng route và mô tả schema**

Thêm vào bảng route admin:

```markdown
| GET | `/api/admin/pools` | every pool |
| POST | `/api/admin/pools` | create a pool |
| GET | `/api/admin/pools/{pid}` | one pool |
| PATCH | `/api/admin/pools/{pid}` | config; a blank secret keeps the stored one |
| DELETE | `/api/admin/pools/{pid}` | refused while any bucket uses it |
| GET | `/api/pools` | name and provider only, so a user can pick one |
```

Sửa mục "What is in the schema": thêm dòng `pools`, và sửa dòng `buckets` — bỏ phần store columns, thêm `pool_id`.

- [ ] **Step 3: `CLAUDE.md` — ràng buộc mới**

```markdown
- **Pool giữ credential upstream, không phải bucket.** Sáu cột store đã dời từ `buckets` sang `pools`; `buckets.pool_id` là NOT NULL với `ON DELETE RESTRICT`. Không có `user_id IS NULL` sentinel nữa — cái đó là nguồn của lỗi rò dữ liệu mà `m20260817` phải sửa.
- **`pools.access_secret_encrypted` không bao giờ ra API.** `PoolResponse` không có field cho nó; `is_configured` là tất cả những gì console cần biết.
- **SQLite không có FK `pool_id`.** `MODIFY COLUMN` và `ADD FOREIGN KEY` không tồn tại trên SQLite, nên cột ở đó vẫn nullable và không có ràng buộc. Tầng ứng dụng enforce cả hai; SQLite chỉ dùng cho dev/test một node.
```

- [ ] **Step 4: Commit**

```bash
git add docs/ README.md CLAUDE.md
git commit -m "docs: record pools and the post-migration credential step"
```

---

## Self-review

**Phủ spec.** Mục 3.1 (`pools`) → task 1. Mục 3.2 (`buckets.pool_id` + backfill) → task 2. Mục 18.2 (`/api/admin/pools` + console) → task 3, 4. Mục 18.3 (bước vận hành) → task 5.

**Chưa phủ, cố ý.** Không có route S3 nào — plan này dựng nền. `pools.max_bytes` không tồn tại: pool là store, không phải đơn vị quota; quota nằm ở bucket và user như P5 đã làm.

**Nhất quán kiểu.** `pools::CreateParams`/`UpdateParams` khai task 1, dùng ở task 1 và 3. `buckets::Model::create(db, user_id, pool_id, name, max_bytes)` đổi ở task 2, mọi caller sửa cùng task. `count_for_pool` thêm ở task 3, dùng cùng task. `PoolResponse` khai task 3, `Pool` type ở TypeScript (task 4) khớp từng field.

**Rủi ro đã biết.**

1. **Task 2 chạy `ALTER TABLE` trên bảng có dữ liệu.** Trên MySQL đây là rebuild bảng, khoá ghi. `buckets` nhỏ nên nhanh, nhưng vẫn nên lên lịch.
2. **`GET /api/pools` phát hiện muộn** — bước 5 của task 4 mới thấy là cần nó. Thêm route đó vào task 3 trước khi làm task 4, đừng để nó thành một commit riêng lẻ.
3. **Backfill dùng raw SQL nội suy chuỗi.** `uuid` và hằng `CHANGE-ME` đều do code sinh, không phải input người dùng, nên không có bề mặt injection. Nhưng nếu ai sửa sau này thành nhận tham số thì phải chuyển sang prepared statement.
