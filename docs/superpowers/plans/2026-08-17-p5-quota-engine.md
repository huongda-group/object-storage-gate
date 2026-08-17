# P5 — Máy quota — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `max_bytes` được thực thi thật, và `used_bytes` / `object_count` phản ánh đúng nội dung bucket, trên cả ba backend, không dùng lock nào.

**Architecture:** Ba thao tác, mỗi thao tác là đúng một câu `UPDATE ... WHERE <guard>` cộng kiểm `rows_affected`. `reserve` tăng `reserved_bytes` và chỉ thành công nếu còn chỗ; `commit` chuyển từ `reserved_bytes` sang `used_bytes`; `release` trả lại phần đã giữ khi upload hỏng. Quota tồn tại ở hai tầng — bucket và tài khoản — nên mỗi thao tác chạm hai bảng và phải hoàn tác được nếu tầng thứ hai từ chối. Một task CLI đối chiếu lại từ bảng `objects` để sửa trôi.

**Tech Stack:** Rust, SeaORM 1.1, loco task, serial_test, tokio.

**Spec:** `docs/superpowers/specs/2026-08-17-go-live-hardening-design.md`

**Phụ thuộc:** P3 phải xong — task 3 của P3 thêm index cho `buckets.user_id`, và task 5 sửa `put_object` thành hội tụ, mà quota móc thẳng vào đó.

## Global Constraints

- Ba backend hạng nhất: Postgres, MySQL >= 8.0.13, SQLite. Cấm `ILIKE`, `RETURNING`, `ON CONFLICT`, `jsonb`, cột array, `pg_advisory_lock`, `FOR UPDATE SKIP LOCKED`.
- **Quota mutation không lấy lock.** `reserve`/`commit`/`release` là một `UPDATE ... WHERE <guard>` cộng kiểm `rows_affected`. Advisory lock chỉ có ở Postgres và nằm ngoài phạm vi.
- Migration dùng `ColType` + `SchemaManager`; raw SQL branch theo `m.get_database_backend()`.
- `src/models/_entities/` generated từ Postgres.
- SQLite một writer; đường ghi phải chịu `SQLITE_BUSY`.
- Comment tiếng Anh, một câu một dòng.
- Không tự commit/push ngoài bước commit trong plan. Không AI attribution.
- Mọi task phải chạy test trên cả ba backend trước khi commit.

---

## File Structure

**Tạo mới:**
- `src/models/quota.rs` — reserve / commit / release / reconcile, toàn bộ số học quota
- `src/tasks/reconcile_quota.rs` — task CLI đối chiếu
- `tests/models/quota.rs` — test số học và test đua

**Sửa:**
- `src/models/mod.rs` — khai báo `quota`
- `src/models/objects.rs` — `put_object` và `delete` gọi vào quota
- `src/tasks/mod.rs` — khai báo task
- `src/app.rs` — `register_tasks`
- `migration/src/m20260817_000006_quota_checks.rs` — CHECK constraint không âm
- `tests/models/mod.rs`

---

## Task 1: Số học quota — reserve, commit, release

**Files:**
- Create: `src/models/quota.rs`, `tests/models/quota.rs`
- Modify: `src/models/mod.rs`, `tests/models/mod.rs`

**Interfaces:**
- Consumes: cột `users.max_bytes/used_bytes/reserved_bytes`, `buckets.max_bytes/used_bytes/reserved_bytes/object_count`.
- Produces:
  - `quota::reserve(db, bucket_id, bytes) -> ModelResult<Reservation>` — giữ chỗ ở cả bucket lẫn tài khoản, hoàn tác bucket nếu tài khoản từ chối.
  - `quota::commit(db, &Reservation, delta_objects: i64) -> ModelResult<()>` — chuyển giữ chỗ thành đã dùng.
  - `quota::release(db, &Reservation) -> ModelResult<()>` — trả lại phần giữ chỗ.
  - `quota::account_for_delete(db, bucket_id, bytes) -> ModelResult<()>` — trừ `used_bytes` và `object_count` khi xoá.
  - `pub struct Reservation { pub bucket_id: i32, pub user_id: Option<i32>, pub bytes: i64 }`
  - `ModelError` với thông điệp `"quota exceeded"` khi hết chỗ.

- [x] **Step 1: Viết test**

Tạo `tests/models/quota.rs`:

```rust
use loco_rs::testing::prelude::*;
use object_storage_gate::{
    app::App,
    models::{buckets, quota, users},
};
use serial_test::serial;

/// Sets up an owner with `user_max` and one bucket with `bucket_max`.
async fn setup(
    db: &sea_orm::DatabaseConnection,
    user_max: i64,
    bucket_max: i64,
) -> (users::Model, buckets::Model) {
    let user = users::Model::find_by_email(db, "user1@example.com").await.unwrap();
    let mut am: users::ActiveModel = user.into();
    am.max_bytes = sea_orm::ActiveValue::set(user_max);
    let user = am.update(db).await.unwrap();

    let bucket = buckets::Model::create_for_user(db, user.id, "quota-test").await.unwrap();
    let mut am: buckets::ActiveModel = bucket.into();
    am.max_bytes = sea_orm::ActiveValue::set(bucket_max);
    let bucket = am.update(db).await.unwrap();

    (user, bucket)
}

#[tokio::test]
#[serial]
async fn reserve_then_commit_moves_bytes_from_reserved_to_used() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (user, bucket) = setup(db, 1000, 500).await;

    let reservation = quota::reserve(db, bucket.id, 100).await.unwrap();

    let b = buckets::Model::find_by_user_and_name(db, user.id, "quota-test").await.unwrap().unwrap();
    assert_eq!(b.reserved_bytes, 100);
    assert_eq!(b.used_bytes, 0);

    quota::commit(db, &reservation, 1).await.unwrap();

    let b = buckets::Model::find_by_user_and_name(db, user.id, "quota-test").await.unwrap().unwrap();
    assert_eq!(b.reserved_bytes, 0);
    assert_eq!(b.used_bytes, 100);
    assert_eq!(b.object_count, 1);

    let u = users::Model::find_by_email(db, "user1@example.com").await.unwrap();
    assert_eq!(u.reserved_bytes, 0);
    assert_eq!(u.used_bytes, 100);
}

#[tokio::test]
#[serial]
async fn release_gives_the_reservation_back() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (user, bucket) = setup(db, 1000, 500).await;

    let reservation = quota::reserve(db, bucket.id, 200).await.unwrap();
    quota::release(db, &reservation).await.unwrap();

    let b = buckets::Model::find_by_user_and_name(db, user.id, "quota-test").await.unwrap().unwrap();
    assert_eq!(b.reserved_bytes, 0);
    assert_eq!(b.used_bytes, 0);

    let u = users::Model::find_by_email(db, "user1@example.com").await.unwrap();
    assert_eq!(u.reserved_bytes, 0);
}

#[tokio::test]
#[serial]
async fn reserve_refuses_past_the_bucket_quota() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (_user, bucket) = setup(db, 10_000, 500).await;

    quota::reserve(db, bucket.id, 400).await.unwrap();
    let refused = quota::reserve(db, bucket.id, 200).await;

    assert!(refused.is_err());
    assert!(refused.unwrap_err().to_string().contains("quota exceeded"));
}

/// The account quota is the outer bound; a bucket with room must still be refused when the
/// account has none.
#[tokio::test]
#[serial]
async fn reserve_refuses_past_the_account_quota_and_rolls_the_bucket_back() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (user, bucket) = setup(db, 300, 10_000).await;

    let refused = quota::reserve(db, bucket.id, 400).await;
    assert!(refused.is_err());

    // The bucket-level reservation must not be left behind.
    let b = buckets::Model::find_by_user_and_name(db, user.id, "quota-test").await.unwrap().unwrap();
    assert_eq!(b.reserved_bytes, 0, "a failed account reserve leaked a bucket reservation");
}

/// `max_bytes == 0` means unlimited, which is what `is_unlimited()` already documents.
#[tokio::test]
#[serial]
async fn zero_max_bytes_means_unlimited() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (_user, bucket) = setup(db, 0, 0).await;

    let r = quota::reserve(db, bucket.id, 999_999_999_999).await.unwrap();
    quota::commit(db, &r, 1).await.unwrap();
}

/// Two reservations racing for the last slot: exactly one must win.
/// This is the whole reason the guard lives in the UPDATE rather than in a read beforehand.
#[tokio::test]
#[serial]
async fn concurrent_reserves_cannot_both_win_the_last_slot() {
    let boot = boot_test::<App>().await.unwrap();
    let db = boot.app_context.db.clone();
    seed::<App>(&boot.app_context).await.unwrap();
    let (_user, bucket) = setup(&db, 10_000, 100).await;

    let a = {
        let db = db.clone();
        tokio::spawn(async move { quota::reserve(&db, bucket.id, 60).await })
    };
    let b = {
        let db = db.clone();
        tokio::spawn(async move { quota::reserve(&db, bucket.id, 60).await })
    };

    let (ra, rb) = tokio::join!(a, b);
    let wins = [ra.unwrap().is_ok(), rb.unwrap().is_ok()]
        .iter()
        .filter(|ok| **ok)
        .count();

    assert_eq!(wins, 1, "both reservations fit into a 100-byte bucket");
}

/// A bucket with no owner is a system pool: outside every account quota.
#[tokio::test]
#[serial]
async fn a_system_pool_has_no_account_quota() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let pool = buckets::Model::create_system(db, "archive").await.unwrap();
    let r = quota::reserve(db, pool.id, 5000).await.unwrap();
    assert_eq!(r.user_id, None);
    quota::commit(db, &r, 1).await.unwrap();
}
```

Thêm `mod quota;` vào `tests/models/mod.rs`.

Ghi chú: `buckets::Model::create_system` phải tồn tại — kiểm bằng
`grep -n "pub async fn create" src/models/buckets.rs` và sửa lời gọi cho khớp API
thật, hoặc tạo bucket rồi set `user_id = None` bằng tay.

- [x] **Step 2: Chạy để chắc nó fail**

Run: `cargo test --test mod models::quota 2>&1 | tail -20`
Expected: FAIL biên dịch — `unresolved import 'quota'`.

- [x] **Step 3: Viết module quota**

Tạo `src/models/quota.rs`:

```rust
//! Quota accounting.
//!
//! Every write follows reserve → upload → commit, and releases the reservation if the upload fails.
//! The bucket is never scanned to total its size: `ListObjects` over a bucket with a million objects is not a quota check, it is an outage.
//!
//! Every mutation here is a single `UPDATE ... WHERE <guard>` plus a `rows_affected` check.
//! That is atomic on Postgres, MySQL and SQLite alike, and it is the reason no lock appears anywhere in this file — advisory locks are Postgres-only and out of bounds.
use loco_rs::prelude::*;
use sea_orm::sea_query::Expr;

use super::_entities::{buckets, users};

/// A held reservation, returned by [`reserve`] and consumed by [`commit`] or [`release`].
///
/// Carries the owner it also charged, so the release path does not have to re-read the bucket to find out whether there was one.
#[derive(Debug, Clone, Copy)]
pub struct Reservation {
    pub bucket_id: i32,
    pub user_id: Option<i32>,
    pub bytes: i64,
}

fn exceeded() -> ModelError {
    ModelError::msg("quota exceeded")
}

/// Adds `bytes` to a bucket's reservation, refusing when it would cross `max_bytes`.
///
/// The guard is `max_bytes = 0 OR used + reserved + bytes <= max_bytes`, evaluated by the database inside the same statement that performs the increment, so two concurrent callers cannot both read room that only one of them can have.
async fn reserve_bucket(
    db: &DatabaseConnection,
    bucket_id: i32,
    bytes: i64,
) -> ModelResult<bool> {
    let res = buckets::Entity::update_many()
        .col_expr(
            buckets::Column::ReservedBytes,
            Expr::col(buckets::Column::ReservedBytes).add(bytes),
        )
        .filter(buckets::Column::Id.eq(bucket_id))
        .filter(
            Expr::col(buckets::Column::MaxBytes).eq(0).or(Expr::col(
                buckets::Column::UsedBytes,
            )
            .add(Expr::col(buckets::Column::ReservedBytes))
            .add(bytes)
            .lte(Expr::col(buckets::Column::MaxBytes))),
        )
        .exec(db)
        .await?;

    Ok(res.rows_affected > 0)
}

/// The account-level twin of [`reserve_bucket`].
async fn reserve_user(db: &DatabaseConnection, user_id: i32, bytes: i64) -> ModelResult<bool> {
    let res = users::Entity::update_many()
        .col_expr(
            users::Column::ReservedBytes,
            Expr::col(users::Column::ReservedBytes).add(bytes),
        )
        .filter(users::Column::Id.eq(user_id))
        .filter(
            Expr::col(users::Column::MaxBytes).eq(0).or(Expr::col(
                users::Column::UsedBytes,
            )
            .add(Expr::col(users::Column::ReservedBytes))
            .add(bytes)
            .lte(Expr::col(users::Column::MaxBytes))),
        )
        .exec(db)
        .await?;

    Ok(res.rows_affected > 0)
}

/// Holds `bytes` against a bucket and, when the bucket has an owner, against that owner's account.
///
/// Both levels must succeed. When the account refuses, the bucket-level hold is given straight back, because a reservation nobody can commit is a slow leak that only the reconcile task would ever notice.
///
/// # Errors
///
/// Returns a `quota exceeded` error when either level has no room, or a DB error.
pub async fn reserve(
    db: &DatabaseConnection,
    bucket_id: i32,
    bytes: i64,
) -> ModelResult<Reservation> {
    if bytes < 0 {
        return Err(ModelError::msg("cannot reserve a negative size"));
    }

    let bucket = buckets::Entity::find_by_id(bucket_id)
        .one(db)
        .await?
        .ok_or(ModelError::EntityNotFound)?;

    if !reserve_bucket(db, bucket_id, bytes).await? {
        return Err(exceeded());
    }

    if let Some(user_id) = bucket.user_id {
        if !reserve_user(db, user_id, bytes).await? {
            // Give the bucket-level hold back before reporting the account-level refusal.
            release_bucket(db, bucket_id, bytes).await?;
            return Err(exceeded());
        }
    }

    Ok(Reservation {
        bucket_id,
        user_id: bucket.user_id,
        bytes,
    })
}

async fn release_bucket(db: &DatabaseConnection, bucket_id: i32, bytes: i64) -> ModelResult<()> {
    buckets::Entity::update_many()
        .col_expr(
            buckets::Column::ReservedBytes,
            Expr::col(buckets::Column::ReservedBytes).sub(bytes),
        )
        .filter(buckets::Column::Id.eq(bucket_id))
        .filter(Expr::col(buckets::Column::ReservedBytes).gte(bytes))
        .exec(db)
        .await?;
    Ok(())
}

/// Gives a reservation back after a failed upload.
///
/// The `reserved >= bytes` guard means a double release cannot drive the counter negative; the second call simply updates nothing.
///
/// # Errors
///
/// Returns a DB error.
pub async fn release(db: &DatabaseConnection, reservation: &Reservation) -> ModelResult<()> {
    release_bucket(db, reservation.bucket_id, reservation.bytes).await?;

    if let Some(user_id) = reservation.user_id {
        users::Entity::update_many()
            .col_expr(
                users::Column::ReservedBytes,
                Expr::col(users::Column::ReservedBytes).sub(reservation.bytes),
            )
            .filter(users::Column::Id.eq(user_id))
            .filter(Expr::col(users::Column::ReservedBytes).gte(reservation.bytes))
            .exec(db)
            .await?;
    }

    Ok(())
}

/// Turns a reservation into stored bytes once the upload has landed.
///
/// `delta_objects` is `1` for a new object, `0` for an overwrite, and the caller is the only one who knows which.
///
/// # Errors
///
/// Returns a DB error.
pub async fn commit(
    db: &DatabaseConnection,
    reservation: &Reservation,
    delta_objects: i64,
) -> ModelResult<()> {
    buckets::Entity::update_many()
        .col_expr(
            buckets::Column::ReservedBytes,
            Expr::col(buckets::Column::ReservedBytes).sub(reservation.bytes),
        )
        .col_expr(
            buckets::Column::UsedBytes,
            Expr::col(buckets::Column::UsedBytes).add(reservation.bytes),
        )
        .col_expr(
            buckets::Column::ObjectCount,
            Expr::col(buckets::Column::ObjectCount).add(delta_objects),
        )
        .filter(buckets::Column::Id.eq(reservation.bucket_id))
        .filter(Expr::col(buckets::Column::ReservedBytes).gte(reservation.bytes))
        .exec(db)
        .await?;

    if let Some(user_id) = reservation.user_id {
        users::Entity::update_many()
            .col_expr(
                users::Column::ReservedBytes,
                Expr::col(users::Column::ReservedBytes).sub(reservation.bytes),
            )
            .col_expr(
                users::Column::UsedBytes,
                Expr::col(users::Column::UsedBytes).add(reservation.bytes),
            )
            .filter(users::Column::Id.eq(user_id))
            .filter(Expr::col(users::Column::ReservedBytes).gte(reservation.bytes))
            .exec(db)
            .await?;
    }

    Ok(())
}

/// Subtracts a deleted object's bytes from the stored totals.
///
/// Clamped by a `used >= bytes` guard: a double delete cannot drive the counter negative, and the reconcile task fixes anything that still drifts.
///
/// # Errors
///
/// Returns a DB error.
pub async fn account_for_delete(
    db: &DatabaseConnection,
    bucket_id: i32,
    bytes: i64,
) -> ModelResult<()> {
    let bucket = buckets::Entity::find_by_id(bucket_id)
        .one(db)
        .await?
        .ok_or(ModelError::EntityNotFound)?;

    buckets::Entity::update_many()
        .col_expr(
            buckets::Column::UsedBytes,
            Expr::col(buckets::Column::UsedBytes).sub(bytes),
        )
        .col_expr(
            buckets::Column::ObjectCount,
            Expr::col(buckets::Column::ObjectCount).sub(1),
        )
        .filter(buckets::Column::Id.eq(bucket_id))
        .filter(Expr::col(buckets::Column::UsedBytes).gte(bytes))
        .filter(Expr::col(buckets::Column::ObjectCount).gte(1))
        .exec(db)
        .await?;

    if let Some(user_id) = bucket.user_id {
        users::Entity::update_many()
            .col_expr(
                users::Column::UsedBytes,
                Expr::col(users::Column::UsedBytes).sub(bytes),
            )
            .filter(users::Column::Id.eq(user_id))
            .filter(Expr::col(users::Column::UsedBytes).gte(bytes))
            .exec(db)
            .await?;
    }

    Ok(())
}
```

Thêm `pub mod quota;` vào `src/models/mod.rs`.

- [x] **Step 4: Chạy test ba backend**

```bash
cargo test --test mod models::quota 2>&1 | tail -20
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test --test mod models::quota 2>&1 | tail -10
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test --test mod models::quota 2>&1 | tail -10
```

Expected: PASS cả bảy test trên cả ba backend. Test đua là cái quan trọng nhất —
nếu nó fail trên một backend thì guard chưa nằm trong `UPDATE`.

- [x] **Step 5: Clippy và commit**

```bash
cargo clippy --all-targets 2>&1 | tail -10
git add src/ tests/
git commit -m "feat(quota): add reserve, commit, release and delete accounting

used_bytes, reserved_bytes and object_count existed as columns that no code
ever wrote to, so GET /api/usage always returned zero and max_bytes was never
enforced. Every mutation is a single guarded UPDATE with a rows_affected check,
which is atomic on all three backends and needs no lock."
```

---

## Task 2: Nối quota vào đường ghi object

**Files:**
- Modify: `src/models/objects.rs`
- Modify: `tests/models/quota.rs`

**Interfaces:**
- Consumes: `quota::reserve`, `quota::commit`, `quota::release`, `quota::account_for_delete` (task 1).
- Produces: `objects::Model::put_object` giữ chỗ trước và commit sau; `objects::Model::delete` trừ counter.

- [x] **Step 1: Viết test**

Thêm vào `tests/models/quota.rs`:

```rust
#[tokio::test]
#[serial]
async fn put_object_charges_the_quota() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (user, bucket) = setup(db, 10_000, 1000).await;

    objects::Model::put_object(db, bucket.id, "a.bin", 300, "e1", "application/octet-stream")
        .await
        .unwrap();

    let b = buckets::Model::find_by_user_and_name(db, user.id, "quota-test").await.unwrap().unwrap();
    assert_eq!(b.used_bytes, 300);
    assert_eq!(b.object_count, 1);
    assert_eq!(b.reserved_bytes, 0);
}

/// Overwriting must charge the difference, not the whole new size again.
#[tokio::test]
#[serial]
async fn overwriting_charges_only_the_delta() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (user, bucket) = setup(db, 10_000, 1000).await;

    objects::Model::put_object(db, bucket.id, "a.bin", 300, "e1", "application/octet-stream")
        .await
        .unwrap();
    objects::Model::put_object(db, bucket.id, "a.bin", 500, "e2", "application/octet-stream")
        .await
        .unwrap();

    let b = buckets::Model::find_by_user_and_name(db, user.id, "quota-test").await.unwrap().unwrap();
    assert_eq!(b.used_bytes, 500, "overwrite double-charged");
    assert_eq!(b.object_count, 1, "overwrite counted a second object");
}

/// Shrinking an object must give the difference back.
#[tokio::test]
#[serial]
async fn overwriting_smaller_returns_the_difference() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (user, bucket) = setup(db, 10_000, 1000).await;

    objects::Model::put_object(db, bucket.id, "a.bin", 800, "e1", "application/octet-stream").await.unwrap();
    objects::Model::put_object(db, bucket.id, "a.bin", 100, "e2", "application/octet-stream").await.unwrap();

    let b = buckets::Model::find_by_user_and_name(db, user.id, "quota-test").await.unwrap().unwrap();
    assert_eq!(b.used_bytes, 100);
}

#[tokio::test]
#[serial]
async fn put_object_is_refused_past_the_quota_and_stores_nothing() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (user, bucket) = setup(db, 10_000, 500).await;

    let refused = objects::Model::put_object(db, bucket.id, "big.bin", 900, "e", "application/octet-stream").await;
    assert!(refused.is_err());

    assert!(objects::Model::get(db, bucket.id, "big.bin").await.unwrap().is_none());

    let b = buckets::Model::find_by_user_and_name(db, user.id, "quota-test").await.unwrap().unwrap();
    assert_eq!(b.used_bytes, 0);
    assert_eq!(b.reserved_bytes, 0, "a refused write leaked a reservation");
}

#[tokio::test]
#[serial]
async fn deleting_returns_the_bytes() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (user, bucket) = setup(db, 10_000, 1000).await;

    objects::Model::put_object(db, bucket.id, "a.bin", 300, "e1", "application/octet-stream").await.unwrap();
    objects::Model::delete(db, bucket.id, "a.bin").await.unwrap();

    let b = buckets::Model::find_by_user_and_name(db, user.id, "quota-test").await.unwrap().unwrap();
    assert_eq!(b.used_bytes, 0);
    assert_eq!(b.object_count, 0);

    let u = users::Model::find_by_email(db, "user1@example.com").await.unwrap();
    assert_eq!(u.used_bytes, 0);
}

/// Deleting something that is not there must not drive the counters negative.
#[tokio::test]
#[serial]
async fn deleting_a_missing_object_changes_nothing() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (user, bucket) = setup(db, 10_000, 1000).await;

    objects::Model::delete(db, bucket.id, "never-existed").await.unwrap();

    let b = buckets::Model::find_by_user_and_name(db, user.id, "quota-test").await.unwrap().unwrap();
    assert_eq!(b.used_bytes, 0);
    assert_eq!(b.object_count, 0);
}
```

Thêm import `objects` vào đầu file test.

- [x] **Step 2: Chạy để chắc nó fail**

Run: `cargo test --test mod models::quota::put_object 2>&1 | tail -20`
Expected: FAIL — `used_bytes` bằng 0, không ai ghi vào.

- [x] **Step 3: Sửa `put_object`**

Trong `src/models/objects.rs`, bọc phần ghi bằng quota. Kích thước tính chênh
lệch, không tính tuyệt đối:

```rust
    /// Insert a new object or overwrite the existing `(bucket_id, key)` row (`PutObject` semantics, versioning off).
    ///
    /// Charges the quota by the difference: an overwrite that grows an object reserves only the extra bytes, and one that shrinks it gives bytes back.
    /// Reserves before writing and releases on failure, so a refused or failed write never leaves a hold behind.
    ///
    /// # Errors
    /// Returns a `quota exceeded` error when there is no room, or a DB error.
    pub async fn put_object(
        db: &DatabaseConnection,
        bucket_id: i32,
        key: &str,
        size: i64,
        etag: &str,
        content_type: &str,
    ) -> ModelResult<Self> {
        let existing = Self::get(db, bucket_id, key).await?;
        let previous_size = existing.as_ref().map_or(0, |o| o.size);
        let delta = size - previous_size;
        let delta_objects = i64::from(existing.is_none());

        // Only a growing write needs a reservation; a shrinking one gives bytes back at commit time.
        let reservation = if delta > 0 {
            Some(quota::reserve(db, bucket_id, delta).await?)
        } else {
            None
        };

        let write = Self::write_row(db, bucket_id, key, size, etag, content_type).await;

        match write {
            Ok(row) => {
                if let Some(reservation) = reservation {
                    quota::commit(db, &reservation, delta_objects).await?;
                } else {
                    // A shrink or a same-size overwrite: settle the difference directly.
                    quota::settle(db, bucket_id, delta, delta_objects).await?;
                }
                Ok(row)
            }
            Err(e) => {
                if let Some(reservation) = reservation {
                    quota::release(db, &reservation).await?;
                }
                Err(e)
            }
        }
    }
```

Tách phần ghi thuần khỏi phần quota — đổi tên thân hàm cũ (bản hội tụ từ P3 task 5)
thành `write_row`, private:

```rust
    /// The bare upsert, without any quota accounting.
    /// Tries the update first and only inserts when nothing was updated, then retries once if the insert lost a race.
    async fn write_row(
        db: &DatabaseConnection,
        bucket_id: i32,
        key: &str,
        size: i64,
        etag: &str,
        content_type: &str,
    ) -> ModelResult<Self> {
        // ... thân hàm từ P3 task 5, không đổi ...
    }
```

Thêm `quota::settle` vào `src/models/quota.rs`:

```rust
/// Applies a byte delta that needed no reservation, i.e. zero or negative.
///
/// Used by an overwrite that shrinks an object: there was never anything to hold, only something to give back.
///
/// # Errors
///
/// Returns a DB error.
pub async fn settle(
    db: &DatabaseConnection,
    bucket_id: i32,
    delta_bytes: i64,
    delta_objects: i64,
) -> ModelResult<()> {
    if delta_bytes == 0 && delta_objects == 0 {
        return Ok(());
    }

    let bucket = buckets::Entity::find_by_id(bucket_id)
        .one(db)
        .await?
        .ok_or(ModelError::EntityNotFound)?;

    buckets::Entity::update_many()
        .col_expr(
            buckets::Column::UsedBytes,
            Expr::col(buckets::Column::UsedBytes).add(delta_bytes),
        )
        .col_expr(
            buckets::Column::ObjectCount,
            Expr::col(buckets::Column::ObjectCount).add(delta_objects),
        )
        .filter(buckets::Column::Id.eq(bucket_id))
        .filter(Expr::col(buckets::Column::UsedBytes).add(delta_bytes).gte(0))
        .exec(db)
        .await?;

    if let Some(user_id) = bucket.user_id {
        users::Entity::update_many()
            .col_expr(
                users::Column::UsedBytes,
                Expr::col(users::Column::UsedBytes).add(delta_bytes),
            )
            .filter(users::Column::Id.eq(user_id))
            .filter(Expr::col(users::Column::UsedBytes).add(delta_bytes).gte(0))
            .exec(db)
            .await?;
    }

    Ok(())
}
```

- [x] **Step 4: Sửa `delete`**

```rust
    /// Removes an object and returns its bytes to the quota.
    /// Deleting something that is not there is a no-op, not an error — that is `DeleteObject` semantics, and it also keeps a retried delete from double-crediting.
    ///
    /// # Errors
    /// Returns an error on DB failure.
    pub async fn delete(db: &DatabaseConnection, bucket_id: i32, key: &str) -> ModelResult<()> {
        let Some(existing) = Self::get(db, bucket_id, key).await? else {
            return Ok(());
        };

        let removed = Entity::delete_many()
            .filter(Column::BucketId.eq(bucket_id))
            .filter(Column::ObjectKey.eq(key))
            .exec(db)
            .await?;

        // Another caller deleted it between our read and our delete; they credited the quota, not us.
        if removed.rows_affected == 0 {
            return Ok(());
        }

        quota::account_for_delete(db, bucket_id, existing.size).await
    }
```

Thêm `use super::quota;` vào đầu `src/models/objects.rs`.

- [x] **Step 5: Chạy test ba backend**

```bash
cargo test --test mod models 2>&1 | tail -10
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test --test mod models 2>&1 | tail -10
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test --test mod models 2>&1 | tail -10
```

Expected: PASS. Test cũ trong `tests/models/objects.rs` có thể fail vì bucket
mặc định có `max_bytes` nhỏ — kiểm và sửa fixture nếu cần, đừng nới guard.

- [x] **Step 6: Commit**

```bash
git add src/ tests/
git commit -m "feat(quota): charge and credit the quota on every object write

put_object and delete wrote object rows without touching used_bytes or
object_count. Overwrites now charge the difference rather than the full size,
and a refused or failed write releases its reservation."
```

---

## Task 3: Task đối chiếu

**Files:**
- Create: `src/tasks/reconcile_quota.rs`
- Modify: `src/tasks/mod.rs`, `src/app.rs:78-80`
- Create: `tests/tasks/reconcile_quota.rs`
- Modify: `tests/tasks/mod.rs`

**Interfaces:**
- Consumes: `quota` (task 1).
- Produces: task CLI `reconcile_quota`, chạy bằng `cargo loco task reconcile_quota`. `quota::reconcile(db) -> ModelResult<ReconcileReport>` với `ReconcileReport { buckets_fixed: u64, users_fixed: u64 }`.

Bối cảnh: guard chống được đua, nhưng không chống được tiến trình chết giữa
reserve và commit. Một reservation mồ côi khoá chỗ mãi mãi. `src/tasks/mod.rs`
hiện là file rỗng và `register_tasks` chỉ có marker.

- [x] **Step 1: Viết test**

Tạo `tests/tasks/reconcile_quota.rs`:

```rust
use loco_rs::{task, testing::prelude::*};
use object_storage_gate::{
    app::App,
    models::{buckets, objects, quota, users},
};
use serial_test::serial;

/// Drift is the normal state after a crash between reserve and commit.
/// Reconcile must recompute both levels from the object rows, which are the only source of truth.
#[tokio::test]
#[serial]
async fn reconcile_recomputes_totals_from_the_object_rows() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(db, "user1@example.com").await.unwrap();
    let bucket = buckets::Model::create_for_user(db, user.id, "drifted").await.unwrap();

    objects::Model::put_object(db, bucket.id, "a", 100, "e", "text/plain").await.unwrap();
    objects::Model::put_object(db, bucket.id, "b", 250, "e", "text/plain").await.unwrap();

    // Simulate a crash between reserve and commit, plus a bogus used_bytes.
    let mut am: buckets::ActiveModel = bucket.clone().into();
    am.reserved_bytes = sea_orm::ActiveValue::set(9_999);
    am.used_bytes = sea_orm::ActiveValue::set(7);
    am.object_count = sea_orm::ActiveValue::set(42);
    am.update(db).await.unwrap();

    let report = quota::reconcile(db).await.unwrap();
    assert!(report.buckets_fixed >= 1);

    let b = buckets::Model::find_by_user_and_name(db, user.id, "drifted").await.unwrap().unwrap();
    assert_eq!(b.used_bytes, 350);
    assert_eq!(b.object_count, 2);
    assert_eq!(b.reserved_bytes, 0, "reconcile must clear stale reservations");

    let u = users::Model::find_by_email(db, "user1@example.com").await.unwrap();
    assert_eq!(u.used_bytes, 350);
    assert_eq!(u.reserved_bytes, 0);
}

/// A bucket that is already correct must not be reported as fixed.
#[tokio::test]
#[serial]
async fn reconcile_is_idempotent() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(db, "user1@example.com").await.unwrap();
    let bucket = buckets::Model::create_for_user(db, user.id, "correct").await.unwrap();
    objects::Model::put_object(db, bucket.id, "a", 100, "e", "text/plain").await.unwrap();

    quota::reconcile(db).await.unwrap();
    let second = quota::reconcile(db).await.unwrap();

    assert_eq!(second.buckets_fixed, 0);
    assert_eq!(second.users_fixed, 0);
}

#[tokio::test]
#[serial]
async fn the_task_is_registered_and_runs() {
    let boot = boot_test::<App>().await.unwrap();

    assert!(task::<App>(
        &boot.app_context,
        "reconcile_quota",
        &task::Vars::default()
    )
    .await
    .is_ok());
}
```

Thêm `mod reconcile_quota;` vào `tests/tasks/mod.rs`.

Ghi chú: chữ ký của `loco_rs::testing::prelude::task` khác nhau giữa các phiên
bản — kiểm bằng `grep -rn "pub async fn task" ~/.cargo/registry/src/*/loco-rs-0.16.4/src/testing/`
và sửa lời gọi cho khớp.

- [x] **Step 2: Chạy để chắc nó fail**

Run: `cargo test --test mod tasks::reconcile_quota 2>&1 | tail -20`
Expected: FAIL — `quota::reconcile` chưa tồn tại.

- [x] **Step 3: Viết `reconcile`**

Thêm vào `src/models/quota.rs`:

```rust
/// What a reconcile pass changed.
#[derive(Debug, Default, Clone, Copy)]
pub struct ReconcileReport {
    pub buckets_fixed: u64,
    pub users_fixed: u64,
}

/// Recomputes every stored total from the object rows.
///
/// The counters are an optimisation; the object rows are the truth. A process that dies between reserve and commit leaves a hold that nothing will ever release, and this is what releases it.
///
/// Clears `reserved_bytes` outright rather than trying to tell a live reservation from a dead one: a reservation only lives for the duration of one upload, so anything still held when this runs is almost certainly stale.
/// Run it off-peak for that reason — a concurrent upload loses its hold and its commit then re-adds the bytes anyway, but the window is briefly permissive.
///
/// # Errors
///
/// Returns a DB error.
pub async fn reconcile(db: &DatabaseConnection) -> ModelResult<ReconcileReport> {
    use sea_orm::{QuerySelect, QueryTrait};

    let mut report = ReconcileReport::default();

    let all_buckets = buckets::Entity::find().all(db).await?;

    for bucket in &all_buckets {
        let rows = super::_entities::objects::Entity::find()
            .filter(super::_entities::objects::Column::BucketId.eq(bucket.id))
            .all(db)
            .await?;

        let real_bytes: i64 = rows.iter().map(|o| o.size).sum();
        let real_count = rows.len() as i64;

        if bucket.used_bytes == real_bytes
            && bucket.object_count == real_count
            && bucket.reserved_bytes == 0
        {
            continue;
        }

        tracing::warn!(
            bucket_id = bucket.id,
            bucket = %bucket.name,
            stored_bytes = bucket.used_bytes,
            real_bytes,
            stored_count = bucket.object_count,
            real_count,
            stale_reserved = bucket.reserved_bytes,
            "quota drift corrected"
        );

        buckets::Entity::update_many()
            .col_expr(buckets::Column::UsedBytes, Expr::value(real_bytes))
            .col_expr(buckets::Column::ObjectCount, Expr::value(real_count))
            .col_expr(buckets::Column::ReservedBytes, Expr::value(0))
            .filter(buckets::Column::Id.eq(bucket.id))
            .exec(db)
            .await?;

        report.buckets_fixed += 1;
    }

    // Account totals are the sum of the owner's buckets, which are now correct.
    let all_users = users::Entity::find().all(db).await?;

    for user in &all_users {
        let owned: i64 = all_buckets
            .iter()
            .filter(|b| b.user_id == Some(user.id))
            .map(|b| {
                // Recomputed above, so read the corrected value rather than the stale one.
                b.used_bytes
            })
            .sum();

        // The in-memory copies still hold pre-correction values, so re-read.
        let fresh: i64 = buckets::Entity::find()
            .filter(buckets::Column::UserId.eq(user.id))
            .all(db)
            .await?
            .iter()
            .map(|b| b.used_bytes)
            .sum();
        let _ = owned;

        if user.used_bytes == fresh && user.reserved_bytes == 0 {
            continue;
        }

        tracing::warn!(
            user_id = user.id,
            stored_bytes = user.used_bytes,
            real_bytes = fresh,
            stale_reserved = user.reserved_bytes,
            "account quota drift corrected"
        );

        users::Entity::update_many()
            .col_expr(users::Column::UsedBytes, Expr::value(fresh))
            .col_expr(users::Column::ReservedBytes, Expr::value(0))
            .filter(users::Column::Id.eq(user.id))
            .exec(db)
            .await?;

        report.users_fixed += 1;
    }

    Ok(report)
}
```

Dọn: bỏ biến `owned` và `let _ = owned;` — chúng là tàn dư của một cách tính sai.
Chỉ giữ `fresh`. Bỏ luôn `use sea_orm::{QuerySelect, QueryTrait};` nếu clippy báo
thừa.

Ghi chú `ponytail:` đặt trên `reconcile`:

```rust
// ponytail: loads every object row per bucket to sum them, one bucket at a time.
// Ceiling: fine up to a few hundred thousand objects; past that use a grouped SUM query, which needs no lock either.
```

- [x] **Step 4: Viết task**

Tạo `src/tasks/reconcile_quota.rs`:

```rust
//! Recomputes quota counters from the object rows.
//!
//! Run it on a schedule. The guarded UPDATEs in `models::quota` survive concurrency but not a process that dies between reserve and commit, and this is what cleans up after that.
use loco_rs::prelude::*;

use crate::models::quota;

pub struct ReconcileQuota;

#[async_trait]
impl Task for ReconcileQuota {
    fn task(&self) -> TaskInfo {
        TaskInfo {
            name: "reconcile_quota".to_string(),
            detail: "recompute bucket and account quota counters from the object rows".to_string(),
        }
    }

    async fn run(&self, app_context: &AppContext, _vars: &task::Vars) -> Result<()> {
        let report = quota::reconcile(&app_context.db).await?;
        tracing::info!(
            buckets_fixed = report.buckets_fixed,
            users_fixed = report.users_fixed,
            "quota reconcile finished"
        );
        println!(
            "reconciled: {} buckets, {} accounts",
            report.buckets_fixed, report.users_fixed
        );
        Ok(())
    }
}
```

`src/tasks/mod.rs`:

```rust
pub mod reconcile_quota;
```

`src/app.rs`:

```rust
    fn register_tasks(tasks: &mut Tasks) {
        tasks.register(crate::tasks::reconcile_quota::ReconcileQuota);
        // tasks-inject (do not remove)
    }
```

Bỏ `#[allow(unused_variables)]` phía trên nếu clippy không còn cần.

- [x] **Step 5: Chạy test và task thật**

```bash
cargo test --test mod tasks 2>&1 | tail -10
cargo loco task
cargo loco task reconcile_quota
```

Expected: `cargo loco task` liệt kê `reconcile_quota`; chạy nó in ra dòng tổng kết.

- [x] **Step 6: Ghi tài liệu**

Thêm vào `README.md`:

```markdown
### Đối chiếu quota

`used_bytes` và `object_count` là bộ đếm; bảng `objects` mới là sự thật. Một
tiến trình chết giữa reserve và commit để lại phần giữ chỗ không ai trả.

```bash
cargo loco task reconcile_quota
```

Chạy định kỳ, giờ thấp điểm — nó xoá sạch `reserved_bytes`, nên một upload đang
chạy sẽ mất phần giữ chỗ (commit của nó cộng lại ngay, nhưng có một khoảng ngắn
quota nới lỏng hơn thực tế).
```

- [x] **Step 7: Commit**

```bash
git add src/ tests/ README.md
git commit -m "feat(quota): add the reconcile task

The guarded UPDATEs survive concurrency but not a process that dies between
reserve and commit, which leaves a hold nothing ever releases. src/tasks was an
empty file and register_tasks had no task in it."
```

---

## Task 4: CHECK constraint không âm

**Files:**
- Create: `migration/src/m20260817_000006_quota_checks.rs`
- Modify: `migration/src/lib.rs`

**Interfaces:**
- Consumes: task 1, 2 (counter đã có người ghi).
- Produces: `CHECK (used_bytes >= 0)`, `CHECK (reserved_bytes >= 0)`, `CHECK (object_count >= 0)` trên `buckets` và `users`.

Guard trong `UPDATE` đã chặn phần lớn đường xuống âm, nhưng CHECK là lưới cuối:
nó biến một bug kế toán tương lai thành lỗi ghi ngay tại chỗ thay vì một con số
âm âm thầm trôi qua API.

- [x] **Step 1: Viết migration**

Tạo `migration/src/m20260817_000006_quota_checks.rs`:

```rust
use sea_orm::{ConnectionTrait, DatabaseBackend};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// `(table, column)` pairs that must never go negative.
const NON_NEGATIVE: &[(&str, &str)] = &[
    ("buckets", "used_bytes"),
    ("buckets", "reserved_bytes"),
    ("buckets", "object_count"),
    ("users", "used_bytes"),
    ("users", "reserved_bytes"),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        let conn = m.get_connection();
        for (table, column) in NON_NEGATIVE {
            let name = format!("chk_{table}_{column}_non_negative");
            match m.get_database_backend() {
                // SQLite cannot add a CHECK constraint to an existing table; doing it would mean
                // rebuilding the table, which is not worth it for a backstop that the guarded
                // UPDATEs already enforce.
                DatabaseBackend::Sqlite => {}
                _ => {
                    conn.execute_unprepared(&format!(
                        "ALTER TABLE {table} ADD CONSTRAINT {name} CHECK ({column} >= 0)"
                    ))
                    .await?;
                }
            }
        }
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        let conn = m.get_connection();
        for (table, column) in NON_NEGATIVE {
            let name = format!("chk_{table}_{column}_non_negative");
            match m.get_database_backend() {
                DatabaseBackend::Sqlite => {}
                DatabaseBackend::MySql => {
                    conn.execute_unprepared(&format!("ALTER TABLE {table} DROP CHECK {name}"))
                        .await?;
                }
                _ => {
                    conn.execute_unprepared(&format!(
                        "ALTER TABLE {table} DROP CONSTRAINT {name}"
                    ))
                    .await?;
                }
            }
        }
        Ok(())
    }
}
```

Đăng ký trong `migration/src/lib.rs` phía trên marker.

Ghi chú: MySQL 8.0.16 trở lên mới thực thi CHECK; dưới đó nó phân tích cú pháp
rồi bỏ qua. Ràng buộc dự án là >= 8.0.13, nên trên 8.0.13–8.0.15 constraint tồn
tại mà không có tác dụng. Không sao — nó là lưới cuối, không phải cơ chế chính.
Ghi lại điều này ngay trong migration bằng một comment.

- [x] **Step 2: Chạy migration ba backend**

```bash
DB_TYPE=postgres cargo loco db reset && cargo test 2>&1 | tail -5
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test 2>&1 | tail -5
DATABASE_URL=sqlite::memory: cargo test 2>&1 | tail -5
```

- [x] **Step 3: Kiểm CHECK thật sự chặn**

```bash
psql postgres://loco:loco@localhost:5432/osg_development -c \
  "UPDATE buckets SET used_bytes = -1 WHERE id = 1;"
```

Expected: lỗi vi phạm constraint.

- [x] **Step 4: Commit**

```bash
git add migration/
git commit -m "feat(db): add non-negative check constraints on the quota counters

A backstop, not the mechanism: the guarded UPDATEs already refuse to go
negative. This turns a future accounting bug into a write error instead of a
negative number quietly served over the API."
```

---

## Self-review

**Phủ blocker.** Blocker 6 (quota chưa cài) → task 1, 2, 3. Bốn cột từng không có
ai ghi vào giờ có đường ghi có guard, có đường trả, và có đường sửa trôi.

**Phủ ràng buộc CLAUDE.md.** "Quota is DB-driven, never bucket-scanned" — không
đường nào trong `put_object`/`delete` quét bucket; chỉ `reconcile` mới đọc bảng
`objects`, và nó là task chạy ngoài giờ, đúng như spec mô tả. "Quota mutations
take no lock" — mọi hàm trong `models/quota.rs` là một `UPDATE ... WHERE` cộng
`rows_affected`, không `begin()`, không advisory lock. "Guard reserve/commit
against races" — test `concurrent_reserves_cannot_both_win_the_last_slot` chứng
minh trên cả ba backend.

**Chưa phủ, cố ý.** Redis distributed lock mà spec gốc nhắc tới: không cần. Guard
trong `UPDATE` đã atomic trên cả ba backend, và CLAUDE.md đã tự chốt điều đó
("Quota mutations take no lock"). Thêm Redis vào đây là thêm một phụ thuộc để
giải một bài toán đã giải xong. Nó vẫn cần cho queue bền ở giai đoạn 6, chỉ là
không cần cho quota. Quota theo prefix, quota theo số lượng object, cảnh báo khi
gần đầy: chưa có ai yêu cầu.

**Nhất quán kiểu.** `Reservation { bucket_id, user_id: Option<i32>, bytes }` khai
task 1, dùng bởi `commit`, `release` (task 1) và `put_object` (task 2).
`settle(db, bucket_id, delta_bytes, delta_objects)` khai task 2 trong cùng
module, gọi từ `put_object` cùng task. `ReconcileReport { buckets_fixed,
users_fixed }` khai task 3, trả bởi `reconcile` và đọc bởi task CLI cùng task.
`write_row` là private, tách ra ở task 2 từ thân `put_object` mà P3 task 5 đã sửa.

**Rủi ro đã biết.** `reconcile` xoá sạch `reserved_bytes`, nên chạy giữa lúc có
upload đang bay sẽ nới quota trong một khoảng ngắn. Đã ghi trong doc-comment và
trong README. Task 2 đổi `put_object` từ "luôn thành công" sang "có thể trả
`quota exceeded`" — mọi caller phải xử lý; hiện chỉ có test gọi nó, nhưng khi
tầng S3 lên thì lỗi này phải map sang `QuotaExceeded` trong XML lỗi S3.
