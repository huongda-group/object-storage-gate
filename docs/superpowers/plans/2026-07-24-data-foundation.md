# Data Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the data layer for the Object Storage Gate under the **user-owns-buckets** model — extend `users` into the account/owner (role + total quota), add `buckets` (per-user, per-bucket quota), `access_keys` (+ per-key policy child tables), and `objects` metadata (no versioning) — plus an AES-256-GCM secret-crypto helper and app wiring, with model logic and unit tests. No S3 API, proxy, SigV4, quota-mutation bodies, or Redis (later slices).

**Architecture:** loco.rs 0.16 (Axum + SeaORM). Migrations use `loco_rs::schema` helpers; entities generated into `src/models/_entities/`; logic in sibling `src/models/*.rs`. Secrets encrypted at rest (SigV4 must recover plaintext to recompute HMAC). A user is the tenant; users own many buckets; objects belong to buckets; quota is two-tier (user + bucket) with `0` = unlimited.

**Tech Stack:** Rust 2021, loco-rs 0.16.4, sea-orm 1.1, Postgres (dev/prod) + SQLite (tests), `aes-gcm` 0.10, `base64` 0.22, `uuid` 1.6, `chrono`, `insta`, `serial_test`.

## Global Constraints

- Never hand-edit `src/models/_entities/*` — generate via `cargo loco db entities`. Each table task gives the exact generated content as a fallback when no live Postgres is available; SQLite drives tests either way.
- Logic in `src/models/<name>.rs`, re-exporting its `_entities` entity (mirror `src/models/users.rs`).
- Register every migration in `migration/src/lib.rs` (above `inject-above`); wire tables into `app.rs` `truncate()`.
- `create_table(m, name, cols, refs)` auto-adds `created_at`/`updated_at` — never list them in `cols`.
- FK refs `&[("target","")]` → NOT NULL `Integer` `{singular}_id` with `ON DELETE CASCADE ON UPDATE CASCADE`.
- **Unlimited sentinel is `0`** — quota columns are `BigInteger` NOT NULL default 0, never NULL.
- Column types must work on Postgres AND SQLite. Status/role/action are `String`, validated in Rust.
- **Never commit or push unless the user asks** (CLAUDE.md). Steps say "stage"; the user commits.
- `#[serial]` on tests touching shared DB/seed state; boot with `boot_test::<App>()`.

---

### Task 1: AES-GCM secret-crypto helper

Self-contained, no DB.

**Files:**
- Modify: `Cargo.toml`
- Create: `src/models/crypto.rs`
- Modify: `src/models/mod.rs`

**Interfaces:**
- Produces: `encrypt(&str) -> Vec<u8>` (`nonce||ct||tag`), `decrypt(&[u8]) -> loco_rs::Result<String>`, `pub const NONCE_LEN: usize = 12`.

- [ ] **Step 1: Add deps to `Cargo.toml`** under `[dependencies]`:

```toml
aes-gcm = { version = "0.10", features = ["std"] }
base64 = { version = "0.22" }
```

- [ ] **Step 2: Write `src/models/crypto.rs`**

```rust
use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use loco_rs::prelude::*;
use std::sync::OnceLock;

pub const NONCE_LEN: usize = 12;

// ponytail: dev/test fallback key. Production MUST set OSG_MASTER_KEY to a
// base64-encoded 32-byte key. Upgrade path: KMS-backed key if rotation needed.
const DEV_KEY_B64: &str = "ZGV2LW9ubHktMzJieXRlLW1hc3Rlci1rZXktMDEyMzQ=";

fn master_key() -> &'static Key<Aes256Gcm> {
    static KEY: OnceLock<Key<Aes256Gcm>> = OnceLock::new();
    KEY.get_or_init(|| {
        let b64 = std::env::var("OSG_MASTER_KEY").unwrap_or_else(|_| DEV_KEY_B64.to_string());
        let bytes = STANDARD.decode(b64.trim()).expect("OSG_MASTER_KEY must be valid base64");
        assert_eq!(bytes.len(), 32, "OSG_MASTER_KEY must decode to 32 bytes");
        *Key::<Aes256Gcm>::from_slice(&bytes)
    })
}

/// Encrypt a secret for storage. Layout: `nonce || ciphertext || tag`.
#[must_use]
pub fn encrypt(plaintext: &str) -> Vec<u8> {
    let cipher = Aes256Gcm::new(master_key());
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let mut ct = cipher.encrypt(&nonce, plaintext.as_bytes()).expect("encrypt");
    let mut out = nonce.to_vec();
    out.append(&mut ct);
    out
}

/// Decrypt a stored secret. Fails on truncated/tampered input.
///
/// # Errors
/// Returns an error if input is too short or authentication fails.
pub fn decrypt(data: &[u8]) -> Result<String> {
    if data.len() <= NONCE_LEN {
        return Err(Error::string("ciphertext too short"));
    }
    let (nonce_bytes, ct) = data.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new(master_key());
    let pt = cipher
        .decrypt(Nonce::from_slice(nonce_bytes), ct)
        .map_err(|_| Error::string("decrypt failed"))?;
    String::from_utf8(pt).map_err(|e| Error::string(&e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let s = "s3cr3t-access-key-value";
        let blob = encrypt(s);
        assert_ne!(blob, s.as_bytes());
        assert_eq!(decrypt(&blob).unwrap(), s);
    }
    #[test]
    fn nonce_is_random() {
        assert_ne!(encrypt("same"), encrypt("same"));
    }
    #[test]
    fn tampered_fails() {
        let mut blob = encrypt("secret");
        let last = blob.len() - 1;
        blob[last] ^= 0xFF;
        assert!(decrypt(&blob).is_err());
    }
    #[test]
    fn too_short_fails() {
        assert!(decrypt(b"short").is_err());
    }
}
```

- [ ] **Step 3: Register** — add `pub mod crypto;` to `src/models/mod.rs`.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib crypto`
Expected: 4 passed.

- [ ] **Step 5: Lint + stage**

```bash
cargo clippy --all-targets 2>&1 | tail -5
cargo fmt
git add Cargo.toml Cargo.lock src/models/crypto.rs src/models/mod.rs
```

---

### Task 2: `users` — add role + total quota

**Files:**
- Create: `migration/src/m20260724_000001_users_account.rs`
- Modify: `migration/src/lib.rs`
- Modify: `src/models/_entities/users.rs` (via `db entities`, or fallback below)
- Modify: `src/models/users.rs`
- Test: `tests/models/users_account.rs`, `tests/models/mod.rs`

**Interfaces:**
- Produces: `users::Model` gains `role: String`, `max_bytes: i64`, `used_bytes: i64`, `reserved_bytes: i64`. `users::{ROLE_ADMIN, ROLE_USER}`, `Model::is_admin()`, `Model::is_unlimited()`.

- [ ] **Step 1: Write migration `migration/src/m20260724_000001_users_account.rs`**

```rust
use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        add_column(m, "users", "role", ColType::StringWithDefault("user".to_string())).await?;
        add_column(m, "users", "max_bytes", ColType::BigIntegerWithDefault(0)).await?;
        add_column(m, "users", "used_bytes", ColType::BigIntegerWithDefault(0)).await?;
        add_column(m, "users", "reserved_bytes", ColType::BigIntegerWithDefault(0)).await?;
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        remove_column(m, "users", "reserved_bytes").await?;
        remove_column(m, "users", "used_bytes").await?;
        remove_column(m, "users", "max_bytes").await?;
        remove_column(m, "users", "role").await?;
        Ok(())
    }
}
```

- [ ] **Step 2: Register** in `migration/src/lib.rs`: add `mod m20260724_000001_users_account;` and `Box::new(m20260724_000001_users_account::Migration),` above the inject marker.

- [ ] **Step 3: Migrate + regen entity**

Run:
```bash
cargo loco db migrate
cargo loco db entities
```

**Fallback (no live Postgres):** in `src/models/_entities/users.rs`, add these fields to the `Model` struct (after `pub name: String,`):

```rust
    pub role: String,
    pub max_bytes: i64,
    pub used_bytes: i64,
    pub reserved_bytes: i64,
```

- [ ] **Step 4: Extend `src/models/users.rs`** — add near the top (after the existing `pub const MAGIC_LINK_*` consts):

```rust
pub const ROLE_ADMIN: &str = "admin";
pub const ROLE_USER: &str = "user";
```

And add a new `impl Model` block (place after the existing `impl super::_entities::users::Model` / model methods, anywhere at module scope):

```rust
impl Model {
    #[must_use]
    pub fn is_admin(&self) -> bool {
        self.role == ROLE_ADMIN
    }

    /// Account-wide quota unlimited when `max_bytes == 0`.
    #[must_use]
    pub fn is_unlimited(&self) -> bool {
        self.max_bytes == 0
    }
}
```

> If `users.rs` already has an `impl Model { ... }`, add the two methods inside it instead of a second block.

- [ ] **Step 5: Write test `tests/models/users_account.rs`**

```rust
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
        name: ActiveValue::set("A".to_string()),
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
```

- [ ] **Step 6: Register test** — add `mod users_account;` to `tests/models/mod.rs`.

- [ ] **Step 7: Run tests**

Run: `cargo test --test models users_account`
Expected: 2 passed.

- [ ] **Step 8: Lint + stage**

```bash
cargo clippy --all-targets 2>&1 | tail -5
cargo fmt
git add migration/ src/models/ tests/models/
```

---

### Task 3: `buckets` table + model

**Files:**
- Create: `migration/src/m20260724_000002_buckets.rs`, `src/models/buckets.rs`, `tests/models/buckets.rs`
- Create (gen/fallback): `src/models/_entities/buckets.rs` + `_entities/mod.rs`
- Modify: `migration/src/lib.rs`, `src/models/mod.rs`, `tests/models/mod.rs`

**Interfaces:**
- Consumes: `users` entity (Task 2).
- Produces:
  - `buckets` entity: `id`, `pid: Uuid`, `user_id: i32`, `name: String`, `max_bytes/used_bytes/reserved_bytes/object_count: i64`, timestamps.
  - `buckets::Model::create(db, user_id, name, max_bytes) -> ModelResult<Model>`
  - `buckets::Model::find_by_user_and_name(db, user_id, name) -> ModelResult<Option<Model>>`
  - `buckets::Model::is_unlimited(&self) -> bool`

- [ ] **Step 1: Write migration `migration/src/m20260724_000002_buckets.rs`**

```rust
use loco_rs::schema::*;
use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::ConnectionTrait;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "buckets",
            &[
                ("id", ColType::PkAuto),
                ("pid", ColType::UuidUniq),
                ("name", ColType::String),
                ("max_bytes", ColType::BigIntegerWithDefault(0)),
                ("used_bytes", ColType::BigIntegerWithDefault(0)),
                ("reserved_bytes", ColType::BigIntegerWithDefault(0)),
                ("object_count", ColType::BigIntegerWithDefault(0)),
            ],
            &[("users", "")],
        )
        .await?;
        m.get_connection()
            .execute_unprepared(
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_buckets_user_name \
                 ON buckets (user_id, name)",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        drop_table(m, "buckets").await?;
        Ok(())
    }
}
```

- [ ] **Step 2: Register migration** in `migration/src/lib.rs` (mod + Box, order 000002).

- [ ] **Step 3: Migrate + regen**

Run:
```bash
cargo loco db migrate
cargo loco db entities
```

**Fallback:** create `src/models/_entities/buckets.rs` and add `pub mod buckets;` to `_entities/mod.rs`:

```rust
//! `SeaORM` Entity, @generated by sea-orm-codegen 1.0.0

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "buckets")]
pub struct Model {
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
    #[sea_orm(primary_key)]
    pub id: i32,
    pub pid: Uuid,
    pub name: String,
    pub max_bytes: i64,
    pub used_bytes: i64,
    pub reserved_bytes: i64,
    pub object_count: i64,
    pub user_id: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
```

- [ ] **Step 4: Write `src/models/buckets.rs`**

```rust
use loco_rs::prelude::*;
use uuid::Uuid;

pub use super::_entities::buckets::{ActiveModel, Column, Entity, Model};

#[async_trait::async_trait]
impl ActiveModelBehavior for super::_entities::buckets::ActiveModel {
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
    /// Create a bucket for a user. `max_bytes == 0` means unlimited.
    ///
    /// # Errors
    /// Returns an error on DB failure (incl. duplicate name for the user).
    pub async fn create(
        db: &DatabaseConnection,
        user_id: i32,
        name: &str,
        max_bytes: i64,
    ) -> ModelResult<Self> {
        Ok(ActiveModel {
            user_id: ActiveValue::set(user_id),
            name: ActiveValue::set(name.to_string()),
            max_bytes: ActiveValue::set(max_bytes),
            ..Default::default()
        }
        .insert(db)
        .await?)
    }

    /// # Errors
    /// Returns an error on DB failure.
    pub async fn find_by_user_and_name(
        db: &DatabaseConnection,
        user_id: i32,
        name: &str,
    ) -> ModelResult<Option<Self>> {
        Ok(Entity::find()
            .filter(Column::UserId.eq(user_id))
            .filter(Column::Name.eq(name))
            .one(db)
            .await?)
    }

    #[must_use]
    pub fn is_unlimited(&self) -> bool {
        self.max_bytes == 0
    }
}
```

- [ ] **Step 5: Register** — add `pub mod buckets;` to `src/models/mod.rs`.

- [ ] **Step 6: Write test `tests/models/buckets.rs`**

```rust
use loco_rs::testing::prelude::*;
use object_storage_gate::{app::App, models::buckets};
use sea_orm::{ActiveModelTrait, ActiveValue};
use serial_test::serial;

async fn user(db: &sea_orm::DatabaseConnection, email: &str) -> i32 {
    use object_storage_gate::models::users;
    users::ActiveModel {
        email: ActiveValue::set(email.to_string()),
        password: ActiveValue::set("x".to_string()),
        name: ActiveValue::set("U".to_string()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
    .id
}

#[tokio::test]
#[serial]
async fn create_and_find_bucket() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let uid = user(db, "u1@ex.com").await;

    let b = buckets::Model::create(db, uid, "photos", 0).await.unwrap();
    assert!(!b.pid.is_nil());
    assert!(b.is_unlimited());
    assert_eq!(b.object_count, 0);

    let found = buckets::Model::find_by_user_and_name(db, uid, "photos")
        .await
        .unwrap();
    assert_eq!(found.unwrap().id, b.id);
    assert!(buckets::Model::find_by_user_and_name(db, uid, "nope")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
#[serial]
async fn bucket_name_unique_per_user() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let u1 = user(db, "a@ex.com").await;
    let u2 = user(db, "b@ex.com").await;

    buckets::Model::create(db, u1, "photos", 0).await.unwrap();
    // Same name, different user → OK.
    buckets::Model::create(db, u2, "photos", 0).await.unwrap();
    // Same name, same user → unique-index violation.
    assert!(buckets::Model::create(db, u1, "photos", 0).await.is_err());
}
```

- [ ] **Step 7: Register test** — add `mod buckets;` to `tests/models/mod.rs`.

- [ ] **Step 8: Run tests**

Run: `cargo test --test models buckets`
Expected: 2 passed.

- [ ] **Step 9: Lint + stage**

```bash
cargo clippy --all-targets 2>&1 | tail -5
cargo fmt
git add migration/ src/models/ tests/models/
```

---

### Task 4: `access_keys` + policy child tables + model

**Files:**
- Create: `migration/src/m20260724_000003_access_keys.rs`, `..._000004_access_key_permissions.rs`, `..._000005_access_key_prefixes.rs`
- Create: `src/models/access_keys.rs`, `src/models/access_key_permissions.rs`, `src/models/access_key_prefixes.rs`
- Create (gen/fallback): `_entities/{access_keys,access_key_permissions,access_key_prefixes}.rs` + `_entities/mod.rs`
- Modify: `migration/src/lib.rs`, `src/models/mod.rs`, `tests/models/mod.rs`
- Test: `tests/models/access_keys.rs`

**Interfaces:**
- Consumes: `crypto` (Task 1), `users` entity (Task 2).
- Produces:
  - `access_keys` entity: `id`, `pid`, `user_id: i32`, `access_key_id: String`, `secret_encrypted: Vec<u8>`, `label`, `status`, `expires_at: Option<DateTimeWithTimeZone>`, timestamps.
  - `Model::create_key(db, user_id, label) -> ModelResult<(Model, String)>`
  - `Model::find_by_access_key_id(db, &str)`, `Model::decrypt_secret(&self)`, `Model::is_usable(&self)`, `Model::permissions(db)`, `Model::prefixes(db)`
  - Consts: `KEY_ACTIVE/DISABLED/REVOKED`, `ACTION_READ/WRITE/DELETE/LIST/MULTIPART/PRESIGNED`.

- [ ] **Step 1: Write `migration/src/m20260724_000003_access_keys.rs`**

```rust
use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "access_keys",
            &[
                ("id", ColType::PkAuto),
                ("pid", ColType::UuidUniq),
                ("access_key_id", ColType::StringUniq),
                ("secret_encrypted", ColType::Blob),
                ("label", ColType::StringWithDefault("primary".to_string())),
                ("status", ColType::StringWithDefault("active".to_string())),
                ("expires_at", ColType::TimestampWithTimeZoneNull),
            ],
            &[("users", "")],
        )
        .await?;
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        drop_table(m, "access_keys").await?;
        Ok(())
    }
}
```

- [ ] **Step 2: Write `migration/src/m20260724_000004_access_key_permissions.rs`**

```rust
use loco_rs::schema::*;
use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::ConnectionTrait;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "access_key_permissions",
            &[("id", ColType::PkAuto), ("action", ColType::String)],
            &[("access_keys", "")],
        )
        .await?;
        m.get_connection()
            .execute_unprepared(
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_akp_key_action \
                 ON access_key_permissions (access_key_id, action)",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        drop_table(m, "access_key_permissions").await?;
        Ok(())
    }
}
```

- [ ] **Step 3: Write `migration/src/m20260724_000005_access_key_prefixes.rs`**

```rust
use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "access_key_prefixes",
            &[("id", ColType::PkAuto), ("prefix", ColType::String)],
            &[("access_keys", "")],
        )
        .await?;
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        drop_table(m, "access_key_prefixes").await?;
        Ok(())
    }
}
```

- [ ] **Step 4: Register all three** in `migration/src/lib.rs` (mod + Box, order 000003–000005).

- [ ] **Step 5: Migrate + regen**

Run:
```bash
cargo loco db migrate
cargo loco db entities
```

**Fallback** — create these and add each to `_entities/mod.rs`:

`src/models/_entities/access_keys.rs`:
```rust
//! `SeaORM` Entity, @generated by sea-orm-codegen 1.0.0

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "access_keys")]
pub struct Model {
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
    #[sea_orm(primary_key)]
    pub id: i32,
    pub pid: Uuid,
    #[sea_orm(unique)]
    pub access_key_id: String,
    pub secret_encrypted: Vec<u8>,
    pub label: String,
    pub status: String,
    pub expires_at: Option<DateTimeWithTimeZone>,
    pub user_id: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
```

`src/models/_entities/access_key_permissions.rs`:
```rust
//! `SeaORM` Entity, @generated by sea-orm-codegen 1.0.0

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "access_key_permissions")]
pub struct Model {
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
    #[sea_orm(primary_key)]
    pub id: i32,
    pub action: String,
    pub access_key_id: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
```

`src/models/_entities/access_key_prefixes.rs`:
```rust
//! `SeaORM` Entity, @generated by sea-orm-codegen 1.0.0

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "access_key_prefixes")]
pub struct Model {
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
    #[sea_orm(primary_key)]
    pub id: i32,
    pub prefix: String,
    pub access_key_id: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
```

> `access_key_id` here is the FK i32 (`{singular}_id` of `access_keys`); on the `access_keys` table itself it's the public `String` identity. Different tables — no collision.

- [ ] **Step 6: Write thin child modules**

`src/models/access_key_permissions.rs`:
```rust
pub use super::_entities::access_key_permissions::{ActiveModel, Column, Entity, Model};
```

`src/models/access_key_prefixes.rs`:
```rust
pub use super::_entities::access_key_prefixes::{ActiveModel, Column, Entity, Model};
```

- [ ] **Step 7: Write `src/models/access_keys.rs`**

```rust
use chrono::Utc;
use loco_rs::prelude::*;
use uuid::Uuid;

use super::_entities::{access_key_permissions, access_key_prefixes};
use super::crypto;
pub use super::_entities::access_keys::{ActiveModel, Column, Entity, Model};

pub const KEY_ACTIVE: &str = "active";
pub const KEY_DISABLED: &str = "disabled";
pub const KEY_REVOKED: &str = "revoked";

pub const ACTION_READ: &str = "read";
pub const ACTION_WRITE: &str = "write";
pub const ACTION_DELETE: &str = "delete";
pub const ACTION_LIST: &str = "list";
pub const ACTION_MULTIPART: &str = "multipart";
pub const ACTION_PRESIGNED: &str = "presigned";

#[async_trait::async_trait]
impl ActiveModelBehavior for super::_entities::access_keys::ActiveModel {
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
    /// Create an access key for a user. Returns the model plus the plaintext
    /// secret ONCE (stored only encrypted; decryptable internally for SigV4).
    ///
    /// # Errors
    /// Returns an error on DB failure.
    pub async fn create_key(
        db: &DatabaseConnection,
        user_id: i32,
        label: &str,
    ) -> ModelResult<(Self, String)> {
        let access_key_id = format!("OSG{}", Uuid::new_v4().simple());
        let secret = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let model = ActiveModel {
            user_id: ActiveValue::set(user_id),
            access_key_id: ActiveValue::set(access_key_id),
            secret_encrypted: ActiveValue::set(crypto::encrypt(&secret)),
            label: ActiveValue::set(label.to_string()),
            status: ActiveValue::set(KEY_ACTIVE.to_string()),
            ..Default::default()
        }
        .insert(db)
        .await?;
        Ok((model, secret))
    }

    /// # Errors
    /// Returns `EntityNotFound` if no key matches.
    pub async fn find_by_access_key_id(
        db: &DatabaseConnection,
        access_key_id: &str,
    ) -> ModelResult<Self> {
        Entity::find()
            .filter(Column::AccessKeyId.eq(access_key_id))
            .one(db)
            .await?
            .ok_or_else(|| ModelError::EntityNotFound)
    }

    /// Decrypt the stored secret for SigV4 verification.
    ///
    /// # Errors
    /// Returns an error if decryption fails.
    pub fn decrypt_secret(&self) -> ModelResult<String> {
        crypto::decrypt(&self.secret_encrypted).map_err(|e| ModelError::Any(e.into()))
    }

    /// Usable if active and not expired.
    #[must_use]
    pub fn is_usable(&self) -> bool {
        self.status == KEY_ACTIVE && self.expires_at.is_none_or(|exp| exp > Utc::now())
    }

    /// # Errors
    /// Returns an error on DB failure.
    pub async fn permissions(&self, db: &DatabaseConnection) -> ModelResult<Vec<String>> {
        Ok(access_key_permissions::Entity::find()
            .filter(access_key_permissions::Column::AccessKeyId.eq(self.id))
            .all(db)
            .await?
            .into_iter()
            .map(|r| r.action)
            .collect())
    }

    /// # Errors
    /// Returns an error on DB failure.
    pub async fn prefixes(&self, db: &DatabaseConnection) -> ModelResult<Vec<String>> {
        Ok(access_key_prefixes::Entity::find()
            .filter(access_key_prefixes::Column::AccessKeyId.eq(self.id))
            .all(db)
            .await?
            .into_iter()
            .map(|r| r.prefix)
            .collect())
    }
}
```

> `Option::is_none_or` is stable since Rust 1.82. If the toolchain is older, replace `is_usable` body with: `if self.status != KEY_ACTIVE { return false; } match self.expires_at { Some(e) => e > Utc::now(), None => true }`.

- [ ] **Step 8: Register modules** — add to `src/models/mod.rs`:

```rust
pub mod access_key_permissions;
pub mod access_key_prefixes;
pub mod access_keys;
```

- [ ] **Step 9: Write test `tests/models/access_keys.rs`**

```rust
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
        name: ActiveValue::set("U".to_string()),
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

    let (key, secret) = access_keys::Model::create_key(db, uid, "primary").await.unwrap();
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
    let (key, _) = access_keys::Model::create_key(db, uid, "primary").await.unwrap();

    let mut am: access_keys::ActiveModel = key.clone().into();
    am.status = ActiveValue::set(access_keys::KEY_DISABLED.to_string());
    assert!(!am.update(db).await.unwrap().is_usable());

    let mut am2: access_keys::ActiveModel = key.into();
    am2.expires_at = ActiveValue::set(Some((Utc::now() - Duration::hours(1)).into()));
    assert!(!am2.update(db).await.unwrap().is_usable());
}

#[tokio::test]
#[serial]
async fn policy_children_load() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let uid = user(db).await;
    let (key, _) = access_keys::Model::create_key(db, uid, "primary").await.unwrap();

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
```

- [ ] **Step 10: Register test** — add `mod access_keys;` to `tests/models/mod.rs`.

- [ ] **Step 11: Run tests**

Run: `cargo test --test models access_keys`
Expected: 3 passed.

- [ ] **Step 12: Lint + stage**

```bash
cargo clippy --all-targets 2>&1 | tail -5
cargo fmt
git add migration/ src/models/ tests/models/
```

---

### Task 5: `objects` table + metadata model (no versioning)

**Files:**
- Create: `migration/src/m20260724_000006_objects.rs`, `src/models/objects.rs`, `tests/models/objects.rs`
- Create (gen/fallback): `src/models/_entities/objects.rs` + `_entities/mod.rs`
- Modify: `migration/src/lib.rs`, `src/models/mod.rs`, `tests/models/mod.rs`

**Interfaces:**
- Consumes: `buckets` entity (Task 3).
- Produces:
  - `objects` entity: `id`, `pid`, `bucket_id: i32`, `object_key: String`, `size: i64`, `etag: String`, `content_type: String`, timestamps.
  - `Model::put_object(db, bucket_id, key, size, etag, content_type) -> ModelResult<Model>` (insert or overwrite existing `(bucket_id,key)` row)
  - `Model::get(db, bucket_id, key) -> ModelResult<Option<Model>>`
  - `Model::delete(db, bucket_id, key) -> ModelResult<()>`
  - `Model::list_by_prefix(db, bucket_id, prefix, limit) -> ModelResult<Vec<Model>>`

- [ ] **Step 1: Write `migration/src/m20260724_000006_objects.rs`**

```rust
use loco_rs::schema::*;
use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::ConnectionTrait;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "objects",
            &[
                ("id", ColType::PkAuto),
                ("pid", ColType::UuidUniq),
                ("object_key", ColType::String),
                ("size", ColType::BigIntegerWithDefault(0)),
                ("etag", ColType::StringWithDefault(String::new())),
                ("content_type", ColType::StringWithDefault("application/octet-stream".to_string())),
            ],
            &[("buckets", "")],
        )
        .await?;
        // One row per key per bucket; also serves prefix listing.
        m.get_connection()
            .execute_unprepared(
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_objects_bucket_key \
                 ON objects (bucket_id, object_key)",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        drop_table(m, "objects").await?;
        Ok(())
    }
}
```

- [ ] **Step 2: Register migration** in `migration/src/lib.rs` (mod + Box, order 000006).

- [ ] **Step 3: Migrate + regen**

Run:
```bash
cargo loco db migrate
cargo loco db entities
```

**Fallback:** create `src/models/_entities/objects.rs` and add `pub mod objects;` to `_entities/mod.rs`:

```rust
//! `SeaORM` Entity, @generated by sea-orm-codegen 1.0.0

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "objects")]
pub struct Model {
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
    #[sea_orm(primary_key)]
    pub id: i32,
    pub pid: Uuid,
    pub object_key: String,
    pub size: i64,
    pub etag: String,
    pub content_type: String,
    pub bucket_id: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
```

- [ ] **Step 4: Write `src/models/objects.rs`**

```rust
use loco_rs::prelude::*;
use uuid::Uuid;

pub use super::_entities::objects::{ActiveModel, Column, Entity, Model};

#[async_trait::async_trait]
impl ActiveModelBehavior for super::_entities::objects::ActiveModel {
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
    /// Insert a new object or overwrite the existing `(bucket_id, key)` row
    /// (PutObject semantics, versioning off).
    ///
    /// # Errors
    /// Returns an error on DB failure.
    pub async fn put_object(
        db: &DatabaseConnection,
        bucket_id: i32,
        key: &str,
        size: i64,
        etag: &str,
        content_type: &str,
    ) -> ModelResult<Self> {
        if let Some(existing) = Self::get(db, bucket_id, key).await? {
            let mut am: ActiveModel = existing.into();
            am.size = ActiveValue::set(size);
            am.etag = ActiveValue::set(etag.to_string());
            am.content_type = ActiveValue::set(content_type.to_string());
            return Ok(am.update(db).await?);
        }
        Ok(ActiveModel {
            bucket_id: ActiveValue::set(bucket_id),
            object_key: ActiveValue::set(key.to_string()),
            size: ActiveValue::set(size),
            etag: ActiveValue::set(etag.to_string()),
            content_type: ActiveValue::set(content_type.to_string()),
            ..Default::default()
        }
        .insert(db)
        .await?)
    }

    /// # Errors
    /// Returns an error on DB failure.
    pub async fn get(
        db: &DatabaseConnection,
        bucket_id: i32,
        key: &str,
    ) -> ModelResult<Option<Self>> {
        Ok(Entity::find()
            .filter(Column::BucketId.eq(bucket_id))
            .filter(Column::ObjectKey.eq(key))
            .one(db)
            .await?)
    }

    /// # Errors
    /// Returns an error on DB failure.
    pub async fn delete(db: &DatabaseConnection, bucket_id: i32, key: &str) -> ModelResult<()> {
        Entity::delete_many()
            .filter(Column::BucketId.eq(bucket_id))
            .filter(Column::ObjectKey.eq(key))
            .exec(db)
            .await?;
        Ok(())
    }

    /// Objects in a bucket whose key starts with `prefix`, up to `limit`,
    /// ordered by key (ListObjectsV2 backing query).
    ///
    /// # Errors
    /// Returns an error on DB failure.
    pub async fn list_by_prefix(
        db: &DatabaseConnection,
        bucket_id: i32,
        prefix: &str,
        limit: u64,
    ) -> ModelResult<Vec<Self>> {
        Ok(Entity::find()
            .filter(Column::BucketId.eq(bucket_id))
            .filter(Column::ObjectKey.starts_with(prefix))
            .order_by_asc(Column::ObjectKey)
            .limit(limit)
            .all(db)
            .await?)
    }
}
```

- [ ] **Step 5: Register** — add `pub mod objects;` to `src/models/mod.rs`.

- [ ] **Step 6: Write test `tests/models/objects.rs`**

```rust
use loco_rs::testing::prelude::*;
use object_storage_gate::{
    app::App,
    models::{buckets, objects, users},
};
use sea_orm::{ActiveModelTrait, ActiveValue};
use serial_test::serial;

async fn bucket(db: &sea_orm::DatabaseConnection) -> i32 {
    let uid = users::ActiveModel {
        email: ActiveValue::set("o@ex.com".to_string()),
        password: ActiveValue::set("x".to_string()),
        name: ActiveValue::set("U".to_string()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
    .id;
    buckets::Model::create(db, uid, "b", 0).await.unwrap().id
}

#[tokio::test]
#[serial]
async fn put_then_overwrite_same_row() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let bid = bucket(db).await;

    let v1 = objects::Model::put_object(db, bid, "a.txt", 10, "e1", "text/plain").await.unwrap();
    let v2 = objects::Model::put_object(db, bid, "a.txt", 20, "e2", "text/plain").await.unwrap();

    assert_eq!(v1.id, v2.id, "same row overwritten");
    let got = objects::Model::get(db, bid, "a.txt").await.unwrap().unwrap();
    assert_eq!(got.size, 20);
    assert_eq!(got.etag, "e2");
}

#[tokio::test]
#[serial]
async fn delete_removes_object() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let bid = bucket(db).await;

    objects::Model::put_object(db, bid, "a.txt", 10, "e", "text/plain").await.unwrap();
    objects::Model::delete(db, bid, "a.txt").await.unwrap();
    assert!(objects::Model::get(db, bid, "a.txt").await.unwrap().is_none());
}

#[tokio::test]
#[serial]
async fn list_by_prefix_filters() {
    let boot = boot_test::<App>().await.expect("boot");
    let db = &boot.app_context.db;
    let bid = bucket(db).await;

    objects::Model::put_object(db, bid, "images/1.png", 1, "e", "image/png").await.unwrap();
    objects::Model::put_object(db, bid, "images/2.png", 1, "e", "image/png").await.unwrap();
    objects::Model::put_object(db, bid, "docs/1.txt", 1, "e", "text/plain").await.unwrap();

    let listed = objects::Model::list_by_prefix(db, bid, "images/", 100).await.unwrap();
    let keys: Vec<_> = listed.iter().map(|o| o.object_key.as_str()).collect();
    assert_eq!(keys, vec!["images/1.png", "images/2.png"]);
}
```

- [ ] **Step 7: Register test** — add `mod objects;` to `tests/models/mod.rs`.

- [ ] **Step 8: Run tests**

Run: `cargo test --test models objects`
Expected: 3 passed.

- [ ] **Step 9: Lint + stage**

```bash
cargo clippy --all-targets 2>&1 | tail -5
cargo fmt
git add migration/ src/models/ tests/models/
```

---

### Task 6: App wiring — truncate + full suite

**Files:**
- Modify: `src/app.rs`
- Test: full `cargo test`

- [ ] **Step 1: Extend imports + `truncate()` in `src/app.rs`**

Add to the `use crate::{...}` block:
```rust
use crate::models::_entities::{
    access_key_permissions, access_key_prefixes, access_keys, buckets, objects, users,
};
```
Replace the `truncate` body (children → parents for FK order):
```rust
    async fn truncate(ctx: &AppContext) -> Result<()> {
        truncate_table(&ctx.db, objects::Entity).await?;
        truncate_table(&ctx.db, access_key_permissions::Entity).await?;
        truncate_table(&ctx.db, access_key_prefixes::Entity).await?;
        truncate_table(&ctx.db, access_keys::Entity).await?;
        truncate_table(&ctx.db, buckets::Entity).await?;
        truncate_table(&ctx.db, users::Entity).await?;
        Ok(())
    }
```

> `seed()` is unchanged — the existing users seed keeps working; new tables need no seed for this slice (tests create their own rows).

- [ ] **Step 2: Run the full suite**

Run: `cargo test`
Expected: all tests pass — existing users/auth tests plus new users_account, buckets, access_keys, objects.

- [ ] **Step 3: Clean reset (if Postgres available)**

Run: `cargo loco db reset`
Expected: drop + recreate + migrate + seed with no FK errors.

- [ ] **Step 4: Final lint + stage**

```bash
cargo clippy --all-targets 2>&1 | tail -10
cargo fmt --check
git add src/app.rs
```

---

## Self-Review

**Spec coverage:**
- `users` role + total quota (0=unlimited) → Task 2. ✓
- AES-GCM crypto → Task 1. ✓
- `buckets` per-user, per-bucket quota, `(user_id,name)` unique → Task 3. ✓
- `access_keys` (FK user) + normalized per-key `access_key_permissions` + `access_key_prefixes` → Task 4. ✓
- `objects` (FK bucket, one row per key, no versioning) + put/get/delete/list → Task 5. ✓
- Wiring (truncate FK order) → Task 6. ✓
- Out-of-scope (SigV4, proxy, quota bodies, Redis, versioning, multipart, audit, admin UI) excluded. ✓

**Placeholder scan:** No TBD/TODO; every code step shows complete code. ✓

**Type consistency:** `create_key -> (Model, String)`; `create(db, user_id, name, max_bytes)`; `put_object` insert/overwrite matches test asserting same `id`; `get`/`find_by_user_and_name` return `Option`; FK cols `user_id`/`bucket_id`/`access_key_id: i32` consistent across entities and queries; `0`=unlimited used in `is_unlimited` on both users and buckets. ✓

**Env risk:** `db entities` needs live Postgres; every table task provides exact generated `_entities` fallback. `Option::is_none_or` needs Rust ≥1.82 with a documented fallback in Task 4.
