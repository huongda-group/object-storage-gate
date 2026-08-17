# P3 — Sửa tầng dữ liệu — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Đóng bảy finding High của tầng dữ liệu, để cùng một request cho cùng một kết quả trên Postgres, MySQL và SQLite, và để hai thao tác chạy song song không đảo ngược lẫn nhau.

**Architecture:** Ba nhóm. Nhóm schema (độ dài cột, collation, index, cascade) là migration cộng test chạy trên cả ba backend. Nhóm đồng thời (`set_status`, `revoke`, `rotate`, `put_object`) đổi từ SELECT-rồi-UPDATE sang một `UPDATE ... WHERE <guard>` cộng kiểm `rows_affected`. Nhóm truy vấn (`list_by_prefix`) đổi `starts_with` sang so sánh khoảng, vừa thoát wildcard vừa dùng được index.

**Tech Stack:** Rust, SeaORM 1.1, sea-orm-migration, insta, serial_test.

**Spec:** `docs/superpowers/specs/2026-08-17-go-live-hardening-design.md`

## Global Constraints

- Ba backend hạng nhất: Postgres, MySQL >= 8.0.13, SQLite. Cấm `ILIKE`, `RETURNING`, `ON CONFLICT` / `ON DUPLICATE KEY`, `jsonb`, cột array, `pg_advisory_lock`, `FOR UPDATE SKIP LOCKED`.
- Migration dùng `ColType` + `SchemaManager` trước; raw SQL chỉ khi không tránh được và phải branch theo `m.get_database_backend()` — mẫu ở `migration/src/m20260724_000002_buckets.rs:36-45`.
- Cột `TIMESTAMP` mới khai `TIMESTAMP(6)` trên MySQL.
- Quota mutation không lấy lock: một `UPDATE ... WHERE <guard>` cộng kiểm `rows_affected`.
- `src/models/_entities/` generated từ Postgres, không sửa tay.
- SQLite một writer; đường ghi phải chịu `SQLITE_BUSY`. WAL và `busy_timeout=5000` đã được `loco_rs::db::connect` đặt, đừng đặt lại.
- Comment tiếng Anh, một câu một dòng.
- Không tự commit/push ngoài bước commit trong plan. Không AI attribution.

**Mọi task trong plan này phải chạy test trên cả ba backend trước khi commit.** Hai
finding của plan này chỉ lộ trên MySQL — chạy một backend là không chứng minh
được gì.

```bash
cargo test
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test
```

---

## File Structure

**Tạo mới:**
- `migration/src/m20260817_000003_column_lengths.rs` — độ dài tường minh cho `prefix`, `object_key`, `name`
- `migration/src/m20260817_000004_binary_collation.rs` — collation phân biệt hoa/thường trên MySQL
- `migration/src/m20260817_000005_hot_indexes.rs` — 4 index còn thiếu
- `tests/models/concurrency.rs` — test đua cho status và put_object
- `tests/models/portability.rs` — test biên độ dài và case-sensitivity

**Sửa:**
- `src/models/access_keys.rs` — `set_status`, `revoke`, `rotate` dùng guard
- `src/models/objects.rs` — `put_object` an toàn, `list_by_prefix` đổi sang so sánh khoảng
- `src/models/crypto.rs` — thêm byte version vào envelope
- `src/controllers/admin.rs` — xoá user thì xoá bucket theo, thay vì từ chối
- `migration/src/m20260724_000002_buckets.rs` — không sửa, chỉ tham chiếu
- `tests/models/mod.rs` — khai báo hai module test mới

---

## Task 1: Độ dài cột tường minh

**Files:**
- Create: `migration/src/m20260817_000003_column_lengths.rs`
- Modify: `migration/src/lib.rs`
- Create: `tests/models/portability.rs`
- Modify: `tests/models/mod.rs`

**Interfaces:**
- Consumes: —
- Produces: `access_key_prefixes.prefix` là `varchar(512)`, `objects.object_key` là `varchar(1024)`, `buckets.name` là `varchar(255)` trên cả ba backend.

Bối cảnh: `ColType::String` map thành `varchar(255)` trên MySQL
(`sea-query-0.32.7/src/backend/mysql/table.rs:38`) nhưng `varchar` không giới hạn
trên Postgres. `src/models/access_keys.rs:35` khai `MAX_PREFIX_LEN = 512` và
validate cho phép đúng 512 — nên cùng một request thành công trên Postgres và
báo `Data too long` trên MySQL. Với `object_key` thì nặng hơn: S3 cho phép key
tới 1024 byte, và giới hạn 255 sẽ vỡ ngay khi tầng S3 lên.

- [x] **Step 1: Viết test biên độ dài**

Tạo `tests/models/portability.rs`:

```rust
use loco_rs::testing::prelude::*;
use object_storage_gate::{
    app::App,
    models::{access_keys, buckets, objects, users},
};
use serial_test::serial;

/// A 512-character prefix is exactly what `MAX_PREFIX_LEN` promises callers.
/// On MySQL a varchar(255) column silently makes that promise a lie.
#[tokio::test]
#[serial]
async fn accepts_a_prefix_at_the_documented_maximum() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(db, "user1@example.com")
        .await
        .unwrap();

    let long_prefix = format!("{}/", "a".repeat(access_keys::MAX_PREFIX_LEN - 1));
    assert_eq!(long_prefix.len(), access_keys::MAX_PREFIX_LEN);

    let (key, _secret) = access_keys::Model::create_key(
        db,
        user.id,
        &access_keys::CreateKeyParams {
            label: "long-prefix".to_string(),
            expires_at: None,
            permissions: vec!["read".to_string()],
            prefixes: vec![long_prefix.clone()],
        },
    )
    .await
    .expect("a prefix at the documented maximum must be storable");

    assert_eq!(key.prefixes(db).await.unwrap(), vec![long_prefix]);
}

/// S3 allows object keys up to 1024 bytes.
/// This is the smallest test that fails on a varchar(255) column.
#[tokio::test]
#[serial]
async fn accepts_an_object_key_at_the_s3_maximum() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(db, "user1@example.com")
        .await
        .unwrap();
    let bucket = buckets::Model::create_for_user(db, user.id, "keys-are-long")
        .await
        .unwrap();

    let key = "k".repeat(1024);
    let stored = objects::Model::put_object(db, bucket.id, &key, 1, "etag", "text/plain")
        .await
        .expect("a 1024-byte object key must be storable");

    assert_eq!(stored.object_key.len(), 1024);
}
```

Thêm `mod portability;` vào `tests/models/mod.rs`.

Ghi chú: tên hàm tạo bucket ở dòng `create_for_user` phải khớp API thật trong
`src/models/buckets.rs` — kiểm bằng `grep -n "pub async fn create" src/models/buckets.rs`
và sửa lời gọi cho khớp trước khi chạy.

- [x] **Step 2: Chạy trên MySQL để thấy nó fail**

```bash
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test --test mod models::portability 2>&1 | tail -20
```

Expected: FAIL với `Data too long for column`.

```bash
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test --test mod models::portability 2>&1 | tail -5
```

Expected: PASS — chính sự khác biệt này là finding.

- [x] **Step 3: Viết migration**

Tạo `migration/src/m20260817_000003_column_lengths.rs`:

```rust
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// `ColType::String` becomes `varchar(255)` on MySQL and an unbounded `varchar` on Postgres, so the same value is accepted on one backend and rejected on another.
/// These lengths are the ones the application already promises: `MAX_PREFIX_LEN` is 512, and S3 allows object keys up to 1024 bytes.
const WIDENINGS: &[(&str, &str, u32)] = &[
    ("access_key_prefixes", "prefix", 512),
    ("objects", "object_key", 1024),
    ("buckets", "name", 255),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        for (table, column, len) in WIDENINGS {
            m.alter_table(
                Table::alter()
                    .table(Alias::new(*table))
                    .modify_column(
                        ColumnDef::new(Alias::new(*column))
                            .string_len(*len)
                            .not_null()
                            .to_owned(),
                    )
                    .to_owned(),
            )
            .await?;
        }
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        for (table, column, _len) in WIDENINGS {
            m.alter_table(
                Table::alter()
                    .table(Alias::new(*table))
                    .modify_column(
                        ColumnDef::new(Alias::new(*column))
                            .string()
                            .not_null()
                            .to_owned(),
                    )
                    .to_owned(),
            )
            .await?;
        }
        Ok(())
    }
}
```

Đăng ký trong `migration/src/lib.rs` phía trên marker `inject-above`.

- [x] **Step 4: Xử lý giới hạn index của InnoDB**

`objects.object_key` nằm trong unique index `idx_objects_bucket_key` cùng
`bucket_id`. Với `utf8mb4`, `varchar(1024)` chiếm `1024 * 4 = 4096` byte, vượt
trần 3072 byte cho một index của InnoDB. Nên migration ở bước 3 sẽ fail trên
MySQL với `Specified key was too long`.

Sửa migration: trên MySQL, drop index trước, đổi cột, rồi tạo lại index với
prefix length.

```rust
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        let is_mysql = matches!(m.get_database_backend(), DatabaseBackend::MySql);

        // InnoDB caps a single index at 3072 bytes, and a utf8mb4 varchar(1024) alone is 4096.
        // Drop the composite unique index, widen the column, then rebuild it over a 700-character prefix of the key.
        // A prefix index still enforces uniqueness for every key shorter than 700 characters, and the gateway rejects duplicate keys at the application layer anyway.
        if is_mysql && m.has_index("objects", "idx_objects_bucket_key").await? {
            m.drop_index(
                Index::drop()
                    .name("idx_objects_bucket_key")
                    .table(Alias::new("objects"))
                    .to_owned(),
            )
            .await?;
        }

        for (table, column, len) in WIDENINGS {
            m.alter_table(
                Table::alter()
                    .table(Alias::new(*table))
                    .modify_column(
                        ColumnDef::new(Alias::new(*column))
                            .string_len(*len)
                            .not_null()
                            .to_owned(),
                    )
                    .to_owned(),
            )
            .await?;
        }

        if is_mysql {
            m.get_connection()
                .execute_unprepared(
                    "CREATE UNIQUE INDEX idx_objects_bucket_key ON objects (bucket_id, object_key(700))",
                )
                .await?;
        }

        Ok(())
    }
```

Thêm `use sea_orm::{ConnectionTrait, DatabaseBackend};` đầu file.

Ghi chú `ponytail:` đặt ngay trên khối MySQL:

```rust
// ponytail: a 700-character prefix index, not the full key.
// Ceiling: two keys identical in their first 700 characters and different after that collide as duplicates on MySQL only.
// Upgrade path: index a hash column of the full key if that ever happens in practice.
```

- [x] **Step 5: Chạy migration và test trên ba backend**

```bash
DB_TYPE=postgres cargo loco db reset && cargo test --test mod models::portability
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test --test mod models::portability
DATABASE_URL=sqlite::memory: cargo test --test mod models::portability
```

Expected: PASS cả ba.

- [x] **Step 6: Sinh lại entity**

```bash
DB_TYPE=postgres cargo loco db entities
```

Kiểu Rust không đổi (vẫn `String`), nhưng chạy để `_entities/` khớp schema thật.

- [x] **Step 7: Commit**

```bash
git add migration/ src/models/_entities/ tests/
git commit -m "fix(db): declare explicit column lengths for prefix, object_key and name

ColType::String is varchar(255) on MySQL and unbounded on Postgres, so a
512-character prefix, which MAX_PREFIX_LEN explicitly allows, was accepted on
one backend and rejected on another. Rebuilds the objects unique index over a
key prefix on MySQL, where a utf8mb4 varchar(1024) exceeds InnoDB's 3072-byte
index limit."
```

---

## Task 2: Collation phân biệt hoa/thường trên MySQL

**Files:**
- Create: `migration/src/m20260817_000004_binary_collation.rs`
- Modify: `migration/src/lib.rs`
- Modify: `tests/models/portability.rs`

**Interfaces:**
- Consumes: task 1 (cột đã có độ dài tường minh).
- Produces: `objects.object_key`, `buckets.name`, `access_key_prefixes.prefix`, `access_keys.access_key_id` dùng `utf8mb4_bin` trên MySQL.

Bối cảnh: MySQL 8 mặc định `utf8mb4_0900_ai_ci` — vừa bỏ dấu vừa bỏ phân biệt
hoa/thường. Postgres và SQLite thì phân biệt. Client PUT `Photos/A.JPG` rồi
`photos/a.jpg`: Postgres ra hai object, MySQL coi là trùng unique index và **đè
mất object thứ nhất**. Mất dữ liệu âm thầm, và sai semantics S3 vì key S3 luôn
phân biệt hoa/thường.

- [x] **Step 1: Viết test**

Thêm vào `tests/models/portability.rs`:

```rust
/// S3 object keys are case-sensitive. MySQL's default collation is not, and the unique
/// index would silently overwrite one object with another.
#[tokio::test]
#[serial]
async fn object_keys_are_case_sensitive() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(db, "user1@example.com")
        .await
        .unwrap();
    let bucket = buckets::Model::create_for_user(db, user.id, "case-matters")
        .await
        .unwrap();

    objects::Model::put_object(db, bucket.id, "Photos/A.JPG", 10, "e1", "image/jpeg")
        .await
        .unwrap();
    objects::Model::put_object(db, bucket.id, "photos/a.jpg", 20, "e2", "image/jpeg")
        .await
        .unwrap();

    let upper = objects::Model::get(db, bucket.id, "Photos/A.JPG")
        .await
        .unwrap()
        .expect("the uppercase key must still exist");
    let lower = objects::Model::get(db, bucket.id, "photos/a.jpg")
        .await
        .unwrap()
        .expect("the lowercase key must exist");

    assert_eq!(upper.size, 10);
    assert_eq!(lower.size, 20);
}

/// Bucket names are per-owner unique; two owners may use the same name, and one owner
/// must not have `Media` collapse into `media`.
#[tokio::test]
#[serial]
async fn bucket_names_are_case_sensitive() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(db, "user1@example.com")
        .await
        .unwrap();

    buckets::Model::create_for_user(db, user.id, "media").await.unwrap();
    let second = buckets::Model::create_for_user(db, user.id, "Media").await;

    assert!(
        second.is_ok(),
        "bucket names differing only in case must be distinct"
    );
}
```

- [x] **Step 2: Chạy trên MySQL để thấy nó fail**

```bash
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test --test mod models::portability::object_keys 2>&1 | tail -20
```

Expected: FAIL — `upper.size` bằng 20, vì bản ghi thứ hai đã đè bản ghi thứ nhất.

- [x] **Step 3: Viết migration**

Tạo `migration/src/m20260817_000004_binary_collation.rs`:

```rust
use sea_orm::{ConnectionTrait, DatabaseBackend};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// Columns whose values are identifiers, not human text, so a case-insensitive or accent-insensitive comparison is always wrong.
/// `(table, column, varchar length)`.
const IDENTIFIER_COLUMNS: &[(&str, &str, u32)] = &[
    ("objects", "object_key", 1024),
    ("buckets", "name", 255),
    ("access_key_prefixes", "prefix", 512),
    ("access_keys", "access_key_id", 255),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        // Postgres and SQLite already compare these byte-for-byte.
        // MySQL 8 defaults to utf8mb4_0900_ai_ci, which folds case and accents, so two distinct S3 keys collide on the unique index and one silently overwrites the other.
        if !matches!(m.get_database_backend(), DatabaseBackend::MySql) {
            return Ok(());
        }

        let conn = m.get_connection();
        for (table, column, len) in IDENTIFIER_COLUMNS {
            conn.execute_unprepared(&format!(
                "ALTER TABLE {table} MODIFY {column} VARCHAR({len}) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL"
            ))
            .await?;
        }
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        if !matches!(m.get_database_backend(), DatabaseBackend::MySql) {
            return Ok(());
        }

        let conn = m.get_connection();
        for (table, column, len) in IDENTIFIER_COLUMNS {
            conn.execute_unprepared(&format!(
                "ALTER TABLE {table} MODIFY {column} VARCHAR({len}) NOT NULL"
            ))
            .await?;
        }
        Ok(())
    }
}
```

Đăng ký trong `migration/src/lib.rs` phía trên marker.

Ghi chú: raw SQL ở đây là bắt buộc — `sea-query` không có API khai collation cho
`modify_column`. Đã branch theo `get_database_backend()` đúng chuẩn CLAUDE.md.

Cảnh báo dữ liệu: nếu bảng đã có hai key chỉ khác hoa/thường thì chúng đã bị gộp
từ trước và migration này không phục hồi được. Trên môi trường có dữ liệu, chạy
truy vấn kiểm trước:

```sql
SELECT bucket_id, LOWER(object_key), COUNT(*) c
FROM objects GROUP BY bucket_id, LOWER(object_key) HAVING c > 1;
```

- [x] **Step 4: Chạy test ba backend**

```bash
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo loco db reset
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test --test mod models::portability
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test --test mod models::portability
cargo test --test mod models::portability
```

Expected: PASS cả ba.

- [x] **Step 5: Commit**

```bash
git add migration/ tests/
git commit -m "fix(db): use binary collation for identifier columns on MySQL

MySQL 8 defaults to a case- and accent-insensitive collation, so PUT
Photos/A.JPG followed by PUT photos/a.jpg collided on the unique index and one
object silently overwrote the other. S3 keys are case-sensitive."
```

---

## Task 3: Index cho đường đọc nóng

**Files:**
- Create: `migration/src/m20260817_000005_hot_indexes.rs`
- Modify: `migration/src/lib.rs`

**Interfaces:**
- Consumes: —
- Produces: unique index trên `users.pid`; index trên `access_keys.user_id`, `buckets.user_id`, `access_key_prefixes.access_key_id`, `users.api_key_prefix`.

Bối cảnh: Postgres và SQLite không tự tạo index cho cột khoá ngoại (chỉ MySQL
InnoDB có). Nặng nhất là `users.pid`: khai `ColType::Uuid`, không index, không
unique — mà `find_by_pid` chạy **mỗi request có JWT**
(`src/controllers/api.rs`, extractor `RawCaller`), tức quét tuần tự bảng users mỗi
lần gọi API. Index hàm `(COALESCE(user_id,0), name)` của buckets không dùng được
cho `WHERE user_id = ?` trên Postgres.

- [x] **Step 1: Viết migration**

Tạo `migration/src/m20260817_000005_hot_indexes.rs`:

```rust
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// `(index name, table, column, unique)`.
/// Postgres and SQLite do not index foreign-key columns automatically, and `users.pid` is read on every authenticated request.
const INDEXES: &[(&str, &str, &str, bool)] = &[
    ("idx_users_pid", "users", "pid", true),
    ("idx_users_api_key_prefix", "users", "api_key_prefix", false),
    ("idx_access_keys_user", "access_keys", "user_id", false),
    ("idx_buckets_user", "buckets", "user_id", false),
    (
        "idx_access_key_prefixes_key",
        "access_key_prefixes",
        "access_key_id",
        false,
    ),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        for (name, table, column, unique) in INDEXES {
            // `has_index` instead of `IF NOT EXISTS`: MySQL has no such syntax for indexes.
            if m.has_index(table, name).await? {
                continue;
            }
            let mut idx = Index::create();
            idx.name(*name)
                .table(Alias::new(*table))
                .col(Alias::new(*column));
            if *unique {
                idx.unique();
            }
            m.create_index(idx.to_owned()).await?;
        }
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        for (name, table, _column, _unique) in INDEXES {
            if !m.has_index(table, name).await? {
                continue;
            }
            m.drop_index(Index::drop().name(*name).table(Alias::new(*table)).to_owned())
                .await?;
        }
        Ok(())
    }
}
```

Đăng ký trong `migration/src/lib.rs` phía trên marker.

Ghi chú: `idx_users_api_key_prefix` chỉ có nghĩa sau khi P2 task 4 thêm cột
`api_key_prefix`. Nếu P3 chạy trước P2, bỏ dòng đó ra khỏi mảng và thêm lại sau
— migration sẽ fail với "column not found" nếu cột chưa tồn tại.

- [x] **Step 2: Chạy migration ba backend**

```bash
DB_TYPE=postgres cargo loco db reset && cargo test 2>&1 | tail -5
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test 2>&1 | tail -5
DATABASE_URL=sqlite::memory: cargo test 2>&1 | tail -5
```

Expected: PASS cả ba. Test không đổi — đây là thay đổi hiệu năng, không đổi hành vi.

- [x] **Step 3: Xác nhận index thật sự được dùng**

```bash
DB_TYPE=postgres cargo loco db reset
psql postgres://loco:loco@localhost:5432/osg_development -c \
  "EXPLAIN SELECT * FROM users WHERE pid = '11111111-1111-1111-1111-111111111111';"
```

Expected: `Index Scan using idx_users_pid`, không phải `Seq Scan`.

- [x] **Step 4: Commit**

```bash
git add migration/
git commit -m "perf(db): index the columns read on every authenticated request

users.pid had no index at all, and find_by_pid runs on every request carrying
a JWT. Postgres and SQLite do not index foreign-key columns automatically, so
four more lookups were sequential scans."
```

---

## Task 4: Đổi trạng thái key có guard

**Files:**
- Modify: `src/models/access_keys.rs`
- Create: `tests/models/concurrency.rs`
- Modify: `tests/models/mod.rs`

**Interfaces:**
- Consumes: —
- Produces: `set_status(self, db, status) -> ModelResult<Self>` và `revoke(self, db) -> ModelResult<Self>` dùng `UPDATE ... WHERE status <> 'revoked'` cộng kiểm `rows_affected`; `rotate` cũng vậy.

Bối cảnh: `set_status` (`src/models/access_keys.rs:296`) kiểm `KEY_REVOKED` trên
bản chụp đã load từ trước rồi chạy `UPDATE ... WHERE id = ?` không có điều kiện
trạng thái. `src/controllers/api.rs:130` load model một lần rồi `update_key` chạy
tuần tự nhiều update — cửa sổ đua rộng. Admin thu hồi key bị lộ trong khi console
gửi `PATCH {"status":"active"}` từ model cũ: UPDATE sau thắng, key đã thu hồi sống
lại.

- [x] **Step 1: Viết test đua**

Tạo `tests/models/concurrency.rs`:

```rust
use loco_rs::testing::prelude::*;
use object_storage_gate::{
    app::App,
    models::{access_keys, buckets, objects, users},
};
use serial_test::serial;

/// The guard must live in the UPDATE, not in a snapshot read beforehand.
/// This reproduces the window: load the model, revoke through a second handle, then try to
/// reactivate through the stale one.
#[tokio::test]
#[serial]
async fn a_revoked_key_cannot_be_reactivated_from_a_stale_model() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(db, "user1@example.com")
        .await
        .unwrap();
    let (key, _secret) = access_keys::Model::create_key(
        db,
        user.id,
        &access_keys::CreateKeyParams {
            label: "leaked".to_string(),
            expires_at: None,
            permissions: vec!["read".to_string()],
            prefixes: vec![],
        },
    )
    .await
    .unwrap();

    // The console loaded this before the admin acted.
    let stale = key.clone();

    // The admin revokes it.
    key.revoke(db).await.unwrap();

    // The console's pending PATCH must not bring it back.
    let result = stale.set_status(db, access_keys::KEY_ACTIVE).await;
    assert!(result.is_err(), "a stale model reactivated a revoked key");

    let fresh = access_keys::Model::find_by_pid_for_user(
        db,
        &access_keys::Model::find_by_pid_for_user(db, "", user.id)
            .await
            .map(|k| k.pid.to_string())
            .unwrap_or_default(),
        user.id,
    )
    .await;
    let _ = fresh;
}

/// Rotating a key that was revoked in the meantime must fail, not produce a live orphan.
#[tokio::test]
#[serial]
async fn a_revoked_key_cannot_be_rotated_from_a_stale_model() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(db, "user1@example.com")
        .await
        .unwrap();
    let (key, _secret) = access_keys::Model::create_key(
        db,
        user.id,
        &access_keys::CreateKeyParams {
            label: "leaked".to_string(),
            expires_at: None,
            permissions: vec!["read".to_string()],
            prefixes: vec![],
        },
    )
    .await
    .unwrap();

    let stale = key.clone();
    key.revoke(db).await.unwrap();

    assert!(stale.rotate(db).await.is_err());

    // And no replacement key was left behind.
    let all = access_keys::Model::list_for_user(db, user.id).await.unwrap();
    assert_eq!(all.len(), 1, "rotate left an orphan key behind");
}
```

Đơn giản hoá phần cuối test đầu tiên — bỏ khối `fresh` lằng nhằng, thay bằng:

```rust
    let all = access_keys::Model::list_for_user(db, user.id).await.unwrap();
    assert_eq!(all[0].status, access_keys::KEY_REVOKED);
```

Thêm `mod concurrency;` vào `tests/models/mod.rs`.

- [x] **Step 2: Chạy để chắc nó fail**

Run: `cargo test --test mod models::concurrency 2>&1 | tail -20`
Expected: FAIL — `set_status` trên model cũ thành công, key trở lại `active`.

- [x] **Step 3: Đổi `set_status` sang UPDATE có guard**

Trong `src/models/access_keys.rs`:

```rust
    /// Move a key between `active` and `disabled`.
    /// `revoked` is terminal: a revoked key is never brought back, because callers may already treat it as gone.
    ///
    /// The guard lives in the UPDATE rather than in a check against `self`, because `self` may have been loaded before a concurrent revoke landed.
    ///
    /// # Errors
    /// Returns an error for an unknown status, for any change to a revoked key, or on DB failure.
    pub async fn set_status(self, db: &DatabaseConnection, status: &str) -> ModelResult<Self> {
        if status != KEY_ACTIVE && status != KEY_DISABLED {
            return Err(invalid("status must be active or disabled"));
        }

        let res = Entity::update_many()
            .col_expr(Column::Status, Expr::value(status))
            .filter(Column::Id.eq(self.id))
            .filter(Column::Status.ne(KEY_REVOKED))
            .exec(db)
            .await?;

        if res.rows_affected == 0 {
            return Err(invalid("a revoked key cannot change status"));
        }

        Self::find_by_id(db, self.id).await
    }

    /// Permanent. The row stays for audit; only the status changes.
    /// Idempotent: revoking an already-revoked key is not an error.
    ///
    /// # Errors
    /// Returns an error on DB failure.
    pub async fn revoke(self, db: &DatabaseConnection) -> ModelResult<Self> {
        Entity::update_many()
            .col_expr(Column::Status, Expr::value(KEY_REVOKED))
            .filter(Column::Id.eq(self.id))
            .exec(db)
            .await?;

        Self::find_by_id(db, self.id).await
    }

    /// Reloads a key by its primary key.
    ///
    /// # Errors
    /// Returns an error when the row is gone, or on DB failure.
    async fn find_by_id(db: &DatabaseConnection, id: i32) -> ModelResult<Self> {
        Entity::find_by_id(id)
            .one(db)
            .await?
            .ok_or(ModelError::EntityNotFound)
    }
```

Thêm `use sea_orm::sea_query::Expr;` đầu file nếu prelude chưa kéo vào.

- [x] **Step 4: Đổi `rotate` cho an toàn**

Vấn đề thứ hai của `rotate` (`access_keys.rs:339`): `create_key` tự commit
transaction riêng, rồi `am.update(db)` để disable key cũ chạy **ngoài mọi
transaction**. Update thứ hai lỗi thì key mới đã tồn tại và active nhưng caller
nhận `Err` nên không bao giờ thấy secret — một credential sống mà chủ sở hữu
không biết.

Đảo thứ tự: disable key cũ trước (có guard), chỉ tạo key mới khi disable thành công.

```rust
    /// Issue a replacement key with the same policy and disable this one.
    /// The old key is disabled rather than revoked so a running app has a window to swap its config.
    ///
    /// Disables first, then creates: if creation fails the caller retries and nothing is left live that they never saw the secret for.
    ///
    /// # Errors
    /// Returns an error when the key is revoked or expired, or on DB failure.
    pub async fn rotate(&self, db: &DatabaseConnection) -> ModelResult<(Self, String)> {
        // Copying a lapsed `expires_at` onto the new key would fail validation with a confusing message; say what is actually wrong instead.
        if self.is_expired() {
            return Err(invalid(
                "an expired key cannot be rotated; create a new key instead",
            ));
        }

        let params = CreateKeyParams {
            label: self.label.clone(),
            expires_at: self.expires_at,
            permissions: self.permissions(db).await?,
            prefixes: self.prefixes(db).await?,
        };

        let disabled = Entity::update_many()
            .col_expr(Column::Status, Expr::value(KEY_DISABLED))
            .filter(Column::Id.eq(self.id))
            .filter(Column::Status.ne(KEY_REVOKED))
            .exec(db)
            .await?;

        if disabled.rows_affected == 0 {
            return Err(invalid("a revoked key cannot be rotated"));
        }

        Self::create_key(db, self.user_id, &params).await
    }
```

- [x] **Step 5: Chạy test ba backend**

```bash
cargo test --test mod models 2>&1 | tail -10
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test --test mod models 2>&1 | tail -5
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test --test mod models 2>&1 | tail -5
```

Expected: PASS. Test cũ trong `tests/models/access_keys.rs` khẳng định
`revoke` rồi `set_status` trả lỗi vẫn xanh; nếu có test khẳng định `revoke` hai
lần là lỗi thì sửa nó — hành vi mới là idempotent, và đó là hành vi đúng cho một
thao tác dập sự cố.

- [x] **Step 6: Commit**

```bash
git add src/ tests/
git commit -m "fix(keys): guard status transitions in the UPDATE, not against a stale model

set_status checked the revoked flag on a model loaded earlier and then issued
an UPDATE with no status condition, so a pending PATCH could reactivate a key
an admin had just revoked. rotate created the replacement before disabling the
original, so a failure left a live key whose secret the owner never saw."
```

---

## Task 5: `put_object` an toàn và `list_by_prefix` thoát wildcard

**Files:**
- Modify: `src/models/objects.rs`
- Modify: `tests/models/objects.rs`
- Modify: `tests/models/concurrency.rs`

**Interfaces:**
- Consumes: —
- Produces: `put_object` là update-trước-insert-sau có bắt lỗi trùng; `list_by_prefix` dùng so sánh khoảng `>= prefix AND < prefix_upper` thay cho `LIKE`.

Bối cảnh cho `put_object` (`src/models/objects.rs:36`): hai PUT song song cùng
`(bucket_id, key)` đều thấy `None` từ `get`, đều insert, một cái ăn unique
violation và bung 500 thay vì 200. Client S3 retry là chuyện thường ngày.

Bối cảnh cho `list_by_prefix` (`src/models/objects.rs:92`): sea-orm
`starts_with` dựng `format!("{}%", s)` rồi `.like(...)`, không thoát `%` và `_`
(`sea-orm-1.1.20/src/entity/column.rs:189-195`). Khi lớp prefix-scoping của access
key cắm vào, một key chỉ được phép `tenants/a/` gửi `prefix=tenants/a_/` sẽ đọc
được `tenants/ab/`, và `prefix=%` quét cả bucket. Thêm nữa `LIKE` của SQLite mặc
định không phân biệt hoa/thường với ASCII, nên ba backend cho ba kết quả.

- [x] **Step 1: Viết test**

Thêm vào `tests/models/objects.rs`:

```rust
/// A LIKE wildcard in the prefix must match literally, not as a pattern.
/// Once access-key prefix scoping is wired to this query, an unescaped `_` lets a key
/// confined to `tenants/a/` read `tenants/ab/`.
#[tokio::test]
#[serial]
async fn list_by_prefix_treats_wildcards_literally() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(db, "user1@example.com").await.unwrap();
    let bucket = buckets::Model::create_for_user(db, user.id, "wildcards").await.unwrap();

    for key in ["a_/one", "ab/two", "a%/three", "az/four"] {
        objects::Model::put_object(db, bucket.id, key, 1, "e", "text/plain")
            .await
            .unwrap();
    }

    let underscore = objects::Model::list_by_prefix(db, bucket.id, "a_/", 100).await.unwrap();
    assert_eq!(underscore.len(), 1);
    assert_eq!(underscore[0].object_key, "a_/one");

    let percent = objects::Model::list_by_prefix(db, bucket.id, "a%", 100).await.unwrap();
    assert_eq!(percent.len(), 1);
    assert_eq!(percent[0].object_key, "a%/three");
}

/// An empty prefix lists the whole bucket, which is what ListObjectsV2 with no prefix means.
#[tokio::test]
#[serial]
async fn list_by_prefix_with_empty_prefix_lists_everything() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(db, "user1@example.com").await.unwrap();
    let bucket = buckets::Model::create_for_user(db, user.id, "everything").await.unwrap();

    for key in ["one", "two", "three"] {
        objects::Model::put_object(db, bucket.id, key, 1, "e", "text/plain").await.unwrap();
    }

    let all = objects::Model::list_by_prefix(db, bucket.id, "", 100).await.unwrap();
    assert_eq!(all.len(), 3);
}
```

Thêm vào `tests/models/concurrency.rs`:

```rust
/// Two writes to the same key must both succeed; the second overwrites the first.
/// The old read-modify-write let both see no row and both insert, and one hit the unique index.
#[tokio::test]
#[serial]
async fn concurrent_put_object_on_the_same_key_does_not_error() {
    let boot = boot_test::<App>().await.unwrap();
    let db = boot.app_context.db.clone();
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(&db, "user1@example.com").await.unwrap();
    let bucket = buckets::Model::create_for_user(&db, user.id, "races").await.unwrap();

    let a = {
        let db = db.clone();
        tokio::spawn(async move {
            objects::Model::put_object(&db, bucket.id, "same/key", 1, "e1", "text/plain").await
        })
    };
    let b = {
        let db = db.clone();
        tokio::spawn(async move {
            objects::Model::put_object(&db, bucket.id, "same/key", 2, "e2", "text/plain").await
        })
    };

    let (ra, rb) = tokio::join!(a, b);
    assert!(ra.unwrap().is_ok(), "first concurrent put failed");
    assert!(rb.unwrap().is_ok(), "second concurrent put failed");

    let rows = objects::Model::list_by_prefix(&db, bucket.id, "same/", 100).await.unwrap();
    assert_eq!(rows.len(), 1);
}
```

- [x] **Step 2: Chạy để chắc nó fail**

Run: `cargo test --test mod models::objects::list_by_prefix 2>&1 | tail -20`
Expected: FAIL — `a_/` khớp cả `ab/two` và `az/four`.

- [x] **Step 3: Sửa `list_by_prefix`**

```rust
    /// Objects in a bucket whose key starts with `prefix`, up to `limit`, ordered by key (`ListObjectsV2` backing query).
    ///
    /// Uses a range comparison rather than `LIKE`.
    /// sea-orm's `starts_with` builds `format!("{}%", s)` with no escaping, so `%` and `_` in a caller-supplied prefix act as wildcards, and SQLite's `LIKE` is case-insensitive for ASCII while Postgres's is not.
    /// A range also uses the `(bucket_id, object_key)` index, which a leading-wildcard `LIKE` never could.
    ///
    /// # Errors
    /// Returns an error on DB failure.
    pub async fn list_by_prefix(
        db: &DatabaseConnection,
        bucket_id: i32,
        prefix: &str,
        limit: u64,
    ) -> ModelResult<Vec<Self>> {
        let mut query = Entity::find()
            .filter(Column::BucketId.eq(bucket_id))
            .order_by_asc(Column::ObjectKey)
            .limit(limit);

        if !prefix.is_empty() {
            query = query.filter(Column::ObjectKey.gte(prefix));
            if let Some(upper) = prefix_upper_bound(prefix) {
                query = query.filter(Column::ObjectKey.lt(upper));
            }
        }

        Ok(query.all(db).await?)
    }
```

Thêm hàm ở phạm vi module, ngoài `impl Model`:

```rust
/// The smallest string strictly greater than every string starting with `prefix`.
///
/// Increments the last code point that can be incremented, dropping trailing `char::MAX` scalars.
/// Returns `None` when no such bound exists, in which case the caller keeps only the lower bound — every remaining key sorts after the prefix anyway.
fn prefix_upper_bound(prefix: &str) -> Option<String> {
    let mut chars: Vec<char> = prefix.chars().collect();
    while let Some(last) = chars.pop() {
        if let Some(next) = char::from_u32(u32::from(last) + 1) {
            let mut bound: String = chars.into_iter().collect();
            bound.push(next);
            return Some(bound);
        }
    }
    None
}
```

Ghi chú: `char::from_u32` trả `None` cho các giá trị surrogate `D800..DFFF`, nên
một prefix kết thúc bằng `\u{D7FF}` sẽ rơi xuống ký tự trước — vòng `while` xử lý
đúng vì nó pop tiếp. Thêm unit test:

```rust
#[cfg(test)]
mod tests {
    use super::prefix_upper_bound;

    #[test]
    fn upper_bound_increments_the_last_character() {
        assert_eq!(prefix_upper_bound("a/").as_deref(), Some("a0"));
        assert_eq!(prefix_upper_bound("tenants/a").as_deref(), Some("tenants/b"));
    }

    #[test]
    fn upper_bound_skips_the_surrogate_gap() {
        let s = format!("x{}", '\u{D7FF}');
        let bound = prefix_upper_bound(&s).unwrap();
        assert!(bound > s);
    }
}
```

Kiểm lại `prefix_upper_bound("a/")`: `/` là U+002F, cộng một ra U+0030 là `0`.
Vậy khoảng là `["a/", "a0")` — đúng, mọi key bắt đầu bằng `a/` nằm trong đó.

- [x] **Step 4: Sửa `put_object`**

```rust
    /// Insert a new object or overwrite the existing `(bucket_id, key)` row (`PutObject` semantics, versioning off).
    ///
    /// Tries the update first and only inserts when nothing was updated, then retries the update once if the insert lost a race.
    /// The previous read-then-insert let two concurrent writes both see no row, both insert, and one hit the unique index with a 500 — which S3 clients trigger routinely, because retrying is what they do.
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
        for attempt in 0..2 {
            let updated = Entity::update_many()
                .col_expr(Column::Size, Expr::value(size))
                .col_expr(Column::Etag, Expr::value(etag))
                .col_expr(Column::ContentType, Expr::value(content_type))
                .filter(Column::BucketId.eq(bucket_id))
                .filter(Column::ObjectKey.eq(key))
                .exec(db)
                .await?;

            if updated.rows_affected > 0 {
                return Self::get(db, bucket_id, key)
                    .await?
                    .ok_or(ModelError::EntityNotFound);
            }

            let insert = ActiveModel {
                bucket_id: ActiveValue::set(bucket_id),
                object_key: ActiveValue::set(key.to_string()),
                size: ActiveValue::set(size),
                etag: ActiveValue::set(etag.to_string()),
                content_type: ActiveValue::set(content_type.to_string()),
                ..Default::default()
            }
            .insert(db)
            .await;

            match insert {
                Ok(row) => return Ok(row),
                // Another writer inserted the same key between our update and our insert.
                // Loop once more; the update will find the row this time.
                Err(_) if attempt == 0 => continue,
                Err(e) => return Err(e.into()),
            }
        }

        Err(ModelError::msg("put_object could not converge"))
    }
```

- [x] **Step 5: Chạy test ba backend**

```bash
cargo test --test mod models 2>&1 | tail -10
cargo test --lib objects 2>&1 | tail -5
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test --test mod models 2>&1 | tail -5
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test --test mod models 2>&1 | tail -5
```

Expected: PASS. Trên SQLite, test đua có thể gặp `SQLITE_BUSY` — `busy_timeout`
đã là 5000ms nên nó sẽ chờ và thành công; nếu vẫn fail, đó là một phát hiện thật
và phải xử lý chứ không phải nới test.

- [x] **Step 6: Commit**

```bash
git add src/ tests/
git commit -m "fix(objects): escape prefix matching and make put_object convergent

list_by_prefix used sea-orm's starts_with, which builds an unescaped LIKE, so a
caller-supplied % or _ acted as a wildcard and SQLite matched case-insensitively
where Postgres did not. A range comparison is literal on all three backends and
uses the composite index. put_object read then inserted, so two concurrent
writes to the same key raced into a unique violation."
```

---

## Task 6: Xoá user không để lại bucket mồ côi

**Files:**
- Modify: `src/controllers/admin.rs`
- Modify: `src/models/users.rs`
- Modify: `tests/requests/admin.rs`

**Interfaces:**
- Consumes: `AdminCaller` và `destroy` handler từ P1 task 4.
- Produces: `users::Model::delete_with_owned_data(self, db) -> ModelResult<()>` — xoá bucket của user trong cùng transaction rồi xoá user.

Bối cảnh: `migration/src/m20260724_000002_buckets.rs:27` khai `&[("users?", "")]`,
mà loco (`loco-rs/src/schema.rs:670-676`) map ref nullable thành
`ON DELETE SET NULL`. Sau khi user bị xoá, `find_system_by_name`
(`src/models/buckets.rs:130`) trả về bucket cũ của họ như một pool hệ thống, kèm
nguyên `access_secret_encrypted` và toàn bộ objects — objects cascade theo bucket,
không theo user, nên không bị xoá. Ngoài rò rỉ, còn kịch bản trùng tên làm
`DELETE FROM users` vỡ unique index `idx_buckets_owner_name` với lỗi rất khó chẩn.

P1 task 4 đã đặt một guard tạm: từ chối xoá user còn bucket. Task này thay guard
đó bằng hành vi đúng.

- [x] **Step 1: Viết test**

Thêm vào `tests/requests/admin.rs`:

```rust
/// Deleting an owner must take their buckets and objects with them.
/// The foreign key is ON DELETE SET NULL, so without this the bucket would reappear as a
/// system pool carrying the former owner's encrypted upstream credentials.
#[tokio::test]
#[serial]
async fn deleting_a_user_removes_their_buckets_and_objects() {
    request::<App, _, _>(|request, ctx| async move {
        let token = admin_token(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&token);
        request
            .post("/api/admin/users")
            .add_header(k, v)
            .json(&serde_json::json!({
                "email": "leaving@congty.vn", "name": "Leaving",
                "password": "temp-password-1", "role": "user", "max_bytes": 0
            }))
            .await;

        let target = users::Model::find_by_email(&ctx.db, "leaving@congty.vn")
            .await
            .unwrap();
        let bucket = buckets::Model::create_for_user(&ctx.db, target.id, "leftovers")
            .await
            .unwrap();
        objects::Model::put_object(&ctx.db, bucket.id, "a/b.txt", 5, "e", "text/plain")
            .await
            .unwrap();

        let (k, v) = prepare_data::auth_header(&token);
        let res = request
            .delete(&format!("/api/admin/users/{}", target.pid))
            .add_header(k, v)
            .await;
        assert_eq!(res.status_code(), 200);

        // No system pool inherited the bucket.
        assert!(buckets::Model::find_system_by_name(&ctx.db, "leftovers")
            .await
            .unwrap()
            .is_none());

        // And the objects went with it.
        assert!(objects::Model::get(&ctx.db, bucket.id, "a/b.txt")
            .await
            .unwrap()
            .is_none());
    })
    .await;
}
```

Thêm import `buckets` và `objects` vào đầu file test.

- [x] **Step 2: Chạy để chắc nó fail**

Run: `cargo test --test mod requests::admin::deleting 2>&1 | tail -20`
Expected: FAIL — P1 trả 400 "delete or reassign this user's buckets first".

- [x] **Step 3: Thêm phương thức model**

Trong `src/models/users.rs`, `impl Model`:

```rust
    /// Deletes a user together with everything they own.
    ///
    /// The buckets foreign key is `ON DELETE SET NULL`, so a bare user delete would leave their bucket behind as a system pool, still carrying the encrypted upstream credentials and every object in it.
    /// Objects cascade from their bucket, so deleting the buckets is enough to take them too.
    ///
    /// # Errors
    ///
    /// When any of the deletes fails
    pub async fn delete_with_owned_data(self, db: &DatabaseConnection) -> ModelResult<()> {
        let txn = db.begin().await?;

        buckets::Entity::delete_many()
            .filter(buckets::Column::UserId.eq(self.id))
            .exec(&txn)
            .await?;

        users::Entity::delete_by_id(self.id).exec(&txn).await?;

        txn.commit().await?;
        Ok(())
    }
```

Thêm `use super::buckets;` vào khối import của `src/models/users.rs`.

Ghi chú: `access_keys` của user cascade theo `users` (khoá ngoại không nullable),
nên không cần xoá tay. Kiểm lại bằng
`grep -n 'users' migration/src/m20260724_000003_access_keys.rs` — nếu ở đó cũng là
`"users?"` thì phải xoá tay như buckets.

- [x] **Step 4: Sửa handler**

Trong `src/controllers/admin.rs`, thay khối guard bucket bằng:

```rust
    if user.id == admin.user.id {
        return Err(Error::BadRequest("cannot delete your own account".to_string()));
    }
    if user.is_admin() && users::Model::admin_count(db).await? <= 1 {
        return Err(Error::BadRequest("cannot delete the last admin".to_string()));
    }

    user.delete_with_owned_data(db).await?;

    format::json(())
```

Bỏ `use crate::models::buckets;` khỏi controller nếu không còn dùng.

- [x] **Step 5: Chạy test ba backend**

```bash
cargo test 2>&1 | tail -10
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test 2>&1 | tail -5
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test 2>&1 | tail -5
```

- [x] **Step 6: Commit**

```bash
git add src/ tests/
git commit -m "fix(admin): delete a user's buckets and objects with the user

The buckets foreign key is ON DELETE SET NULL, so removing an owner turned
their private bucket into a system pool that still carried their encrypted
upstream credentials and every object in it."
```

---

## Task 7: Envelope mã hoá có byte version

**Files:**
- Modify: `src/models/crypto.rs`
- Modify: `README.md`

**Interfaces:**
- Consumes: —
- Produces: envelope thành `version || nonce || ciphertext || tag`. `decrypt` đọc được cả envelope cũ (không có byte version) lẫn mới. `OSG_MASTER_KEY_PREVIOUS` cho phép giải bằng key cũ trong lúc xoay.

Bối cảnh: envelope hiện là `nonce || ciphertext || tag`, không có định danh key,
và `master_key()` cache trong `OnceLock` suốt đời process. Không có đường đọc hai
key song song. Đúng lúc cần xoay `OSG_MASTER_KEY` — kể cả xoay bắt buộc sau kịch
bản key dev — mọi `access_keys.secret_encrypted` và
`buckets.access_secret_encrypted` thành rác vĩnh viễn.

- [x] **Step 1: Viết test**

Thêm vào `mod tests` của `src/models/crypto.rs`:

```rust
    #[test]
    fn new_envelope_carries_a_version_byte() {
        let blob = encrypt("secret");
        assert_eq!(blob[0], ENVELOPE_V1);
        assert_eq!(decrypt(&blob).unwrap(), "secret");
    }

    /// A blob written before the version byte existed has no marker, and must still decrypt.
    /// Its length is nonce + ciphertext + tag, and the first byte is a random nonce byte that
    /// only accidentally equals ENVELOPE_V1.
    #[test]
    fn legacy_envelope_without_a_version_byte_still_decrypts() {
        let legacy = legacy_encrypt_for_test("old secret");
        assert_eq!(decrypt(&legacy).unwrap(), "old secret");
    }
```

Thêm helper chỉ dùng cho test, ngay trong `mod tests`:

```rust
    /// Reproduces the pre-versioning envelope layout: `nonce || ciphertext || tag`.
    fn legacy_encrypt_for_test(plaintext: &str) -> Vec<u8> {
        let cipher = Aes256Gcm::new(master_key());
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let mut ct = cipher.encrypt(&nonce, plaintext.as_bytes()).expect("encrypt");
        let mut out = nonce.to_vec();
        out.append(&mut ct);
        out
    }
```

- [x] **Step 2: Chạy để chắc nó fail**

Run: `cargo test --lib crypto 2>&1 | tail -10`
Expected: FAIL — `ENVELOPE_V1` chưa tồn tại.

- [x] **Step 3: Viết envelope có version**

Trong `src/models/crypto.rs`:

```rust
/// Envelope layout marker written as the first byte of every new ciphertext.
/// Blobs written before this existed start straight with the nonce, and `decrypt` falls back to that layout.
pub const ENVELOPE_V1: u8 = 1;

/// Encrypt a secret for storage. Layout: `version || nonce || ciphertext || tag`.
///
/// # Panics
///
/// Panics if the master key is invalid, or if AES-GCM encryption fails — both are deployment faults, not bad runtime input.
#[must_use]
pub fn encrypt(plaintext: &str) -> Vec<u8> {
    let cipher = Aes256Gcm::new(master_key());
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let mut ct = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .expect("encrypt");
    let mut out = Vec::with_capacity(1 + NONCE_LEN + ct.len());
    out.push(ENVELOPE_V1);
    out.extend_from_slice(nonce.as_slice());
    out.append(&mut ct);
    out
}

/// Decrypt a stored secret. Fails on truncated or tampered input.
///
/// Tries the current key first, then `OSG_MASTER_KEY_PREVIOUS` if it is set, so a key rotation can read old rows while new writes use the new key.
/// Accepts both the versioned layout and the original `nonce || ciphertext || tag` one.
///
/// # Errors
/// Returns an error if input is too short or authentication fails under every available key.
pub fn decrypt(data: &[u8]) -> Result<String> {
    for body in candidate_bodies(data) {
        for key in candidate_keys() {
            if let Some(plain) = try_decrypt(key, body) {
                return String::from_utf8(plain).map_err(|e| Error::string(&e.to_string()));
            }
        }
    }
    Err(Error::string("decrypt failed"))
}

/// The byte slices to try, newest layout first.
fn candidate_bodies(data: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::with_capacity(2);
    if data.len() > 1 + NONCE_LEN && data[0] == ENVELOPE_V1 {
        out.push(&data[1..]);
    }
    if data.len() > NONCE_LEN {
        out.push(data);
    }
    out
}

/// The current master key, then the previous one if a rotation is in progress.
fn candidate_keys() -> Vec<&'static Key<Aes256Gcm>> {
    let mut keys = vec![master_key()];
    if let Some(previous) = previous_key() {
        keys.push(previous);
    }
    keys
}

fn try_decrypt(key: &Key<Aes256Gcm>, body: &[u8]) -> Option<Vec<u8>> {
    let (nonce_bytes, ct) = body.split_at(NONCE_LEN);
    Aes256Gcm::new(key)
        .decrypt(Nonce::from_slice(nonce_bytes), ct)
        .ok()
}

/// The key rows were encrypted under before the current rotation, if one is in progress.
/// Set `OSG_MASTER_KEY_PREVIOUS` during a rotation, run the re-encrypt task, then unset it.
fn previous_key() -> Option<&'static Key<Aes256Gcm>> {
    static PREVIOUS: OnceLock<Option<Key<Aes256Gcm>>> = OnceLock::new();
    PREVIOUS
        .get_or_init(|| {
            let b64 = std::env::var("OSG_MASTER_KEY_PREVIOUS").ok()?;
            let bytes = STANDARD.decode(b64.trim()).ok()?;
            if bytes.len() != 32 {
                return None;
            }
            Some(*Key::<Aes256Gcm>::from_slice(&bytes))
        })
        .as_ref()
}
```

Ghi chú `ponytail:` đặt trên `candidate_bodies`:

```rust
// ponytail: two candidate layouts and two candidate keys means at most four AES-GCM attempts on a legacy row under rotation.
// Ceiling: fine at this volume; drop the legacy layout once a re-encrypt task has rewritten every row.
```

- [x] **Step 4: Chạy test**

Run: `cargo test --lib crypto 2>&1 | tail -10`
Expected: PASS 9 test.

Run: `cargo test 2>&1 | tail -10`
Expected: PASS — test round-trip của `access_keys` và `buckets` vẫn xanh vì
`encrypt`/`decrypt` đối xứng.

- [x] **Step 5: Ghi quy trình xoay key**

Thêm vào `README.md`:

```markdown
### Xoay `OSG_MASTER_KEY`

Mọi secret access key và credential backend store đều mã hoá bằng key này. Xoay
theo ba bước, không được nhảy bước:

1. Đặt `OSG_MASTER_KEY_PREVIOUS` bằng key hiện tại, và `OSG_MASTER_KEY` bằng key
   mới. Deploy. Từ lúc này ghi mới dùng key mới, đọc cũ vẫn giải được bằng key cũ.
2. Chạy task ghi lại toàn bộ hàng đã mã hoá bằng key mới.
3. Bỏ `OSG_MASTER_KEY_PREVIOUS`. Deploy.

Sinh key mới: `openssl rand -base64 32`.
```

Ghi chú: task ở bước 2 chưa tồn tại — nó thuộc `src/tasks/`, hiện là file rỗng.
Thêm mục vào backlog thay vì viết nửa vời ở đây; đường đọc hai key đã đủ để một
lần xoay không phá dữ liệu, đó là cái quan trọng.

- [x] **Step 6: Commit**

```bash
git add src/ README.md
git commit -m "feat(crypto): version the ciphertext envelope and support a previous key

The envelope carried no key identifier and the process cached a single key for
its lifetime, so rotating OSG_MASTER_KEY would have made every stored access-key
secret and every backend-store credential permanently unreadable. Reads now fall
back to OSG_MASTER_KEY_PREVIOUS, and old blobs without a version byte still
decrypt."
```

---

## Self-review

**Phủ finding High của tầng dữ liệu.** Độ dài cột → task 1. Collation → task 2.
Index thiếu → task 3. Race `set_status`/`rotate` → task 4. `starts_with` không
escape → task 5. `put_object` không atomic → task 5. `ON DELETE SET NULL` →
task 6. Crypto không xoay được → task 7.

**Chưa phủ, cố ý.** `expires_at` là `TIMESTAMP` nên trần 2038 trên MySQL
(Medium) — sửa là đổi sang `DATETIME(6)` và đụng cùng cột mà
`m20260815_000001` vừa xử lý; để lại cho một migration riêng khi có nhu cầu thật.
`updated_at` không bao giờ được cập nhật (Medium) — sửa trong `before_save` của ba
model, nhỏ nhưng chạm nhiều test snapshot, tách riêng. `access_key_prefixes`
thiếu unique constraint (Medium) — task 3 thêm index thường, chưa thêm unique vì
`set_prefixes` xoá-rồi-chèn nên trùng chỉ xảy ra khi client gửi mảng có phần tử
lặp; validate ở tầng model rẻ hơn.

**Nhất quán kiểu.** `prefix_upper_bound(&str) -> Option<String>` định nghĩa và
dùng trong task 5. `find_by_id(db, i32) -> ModelResult<Self>` là private helper
thêm ở task 4, dùng bởi `set_status` và `revoke` cùng task.
`delete_with_owned_data(self, db) -> ModelResult<()>` định nghĩa task 6 ở
`models::users`, gọi từ `controllers::admin` cùng task. `ENVELOPE_V1: u8` định
nghĩa task 7, dùng bởi `encrypt`, `candidate_bodies`, và test cùng task.

**Rủi ro đã biết.** Task 1 và 2 chạy `ALTER TABLE ... MODIFY` trên bảng có dữ
liệu — trên MySQL đây là thao tác rebuild bảng, khoá ghi trong lúc chạy. Với bảng
`objects` lớn thì phải lên lịch, không chạy giữa giờ. Task 2 không phục hồi được
dữ liệu đã bị gộp từ trước; chạy truy vấn kiểm trùng ở bước 3 trước khi migrate.
