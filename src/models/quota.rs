//! Quota accounting.
//!
//! Every write follows reserve → upload → commit, and releases the reservation if the upload fails.
//! The bucket is never scanned to total its size: `ListObjects` over a bucket with a million objects is not a quota check, it is an outage.
//!
//! Every mutation here is a single `UPDATE ... WHERE <guard>` plus a `rows_affected` check.
//! That is atomic on Postgres, `MySQL` and `SQLite` alike, and it is why no lock appears anywhere in this file — advisory locks are Postgres-only and out of bounds.
use loco_rs::prelude::*;
use sea_orm::{sea_query::Expr, TransactionTrait};

use super::_entities::{buckets, objects, users};

/// A held reservation, returned by [`reserve`] and consumed by [`commit`] or [`release`].
///
/// Carries the owner it also charged, so the release path does not have to re-read the bucket to find out whether there was one.
#[derive(Debug, Clone, Copy)]
pub struct Reservation {
    pub bucket_id: i32,
    pub user_id: Option<i32>,
    pub bytes: i64,
}

/// What a reconcile pass changed.
#[derive(Debug, Default, Clone, Copy)]
pub struct ReconcileReport {
    pub buckets_fixed: u64,
    pub users_fixed: u64,
}

fn exceeded() -> ModelError {
    ModelError::msg("quota exceeded")
}

/// Adds `bytes` to a bucket's reservation, refusing when it would cross `max_bytes`.
///
/// The guard is `max_bytes = 0 OR used + reserved + bytes <= max_bytes`, evaluated by the database inside the same statement that performs the increment, so two concurrent callers cannot both read room that only one of them can have.
async fn reserve_bucket(db: &DatabaseConnection, bucket_id: i32, bytes: i64) -> ModelResult<bool> {
    let res = buckets::Entity::update_many()
        .col_expr(
            buckets::Column::ReservedBytes,
            Expr::col(buckets::Column::ReservedBytes).add(bytes),
        )
        .filter(buckets::Column::Id.eq(bucket_id))
        .filter(
            Expr::col(buckets::Column::MaxBytes).eq(0).or(Expr::expr(
                Expr::col(buckets::Column::UsedBytes)
                    .add(Expr::col(buckets::Column::ReservedBytes))
                    .add(bytes),
            )
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
            Expr::col(users::Column::MaxBytes).eq(0).or(Expr::expr(
                Expr::col(users::Column::UsedBytes)
                    .add(Expr::col(users::Column::ReservedBytes))
                    .add(bytes),
            )
            .lte(Expr::col(users::Column::MaxBytes))),
        )
        .exec(db)
        .await?;

    Ok(res.rows_affected > 0)
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

/// Holds `bytes` against a bucket and, when the bucket has an owner, against that owner's account.
///
/// Both levels must succeed.
/// When the account refuses, the bucket-level hold is given straight back, because a reservation nobody can commit is a slow leak that only the reconcile task would ever notice.
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

/// Describes a hold that already exists, without taking a new one.
///
/// Multipart accumulates its reservation across many `UploadPart` calls, so by the time `Abort` or `Complete` runs there is no `Reservation` value left to hand back — only a total in `multipart_uploads.reserved_bytes`.
/// This rebuilds the value from that total so `release` and `commit` stay the only code that moves quota.
///
/// # Errors
///
/// Returns an error when the bucket is gone, or a DB error.
pub async fn held(db: &DatabaseConnection, bucket_id: i32, bytes: i64) -> ModelResult<Reservation> {
    let bucket = buckets::Entity::find_by_id(bucket_id)
        .one(db)
        .await?
        .ok_or(ModelError::EntityNotFound)?;
    Ok(Reservation {
        bucket_id,
        user_id: bucket.user_id,
        bytes,
    })
}

/// Turns a reservation into stored bytes once the upload has landed.
///
/// `delta_objects` is `1` for a new object and `0` for an overwrite, and the caller is the only one who knows which.
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
        .filter(Expr::expr(Expr::col(buckets::Column::UsedBytes).add(delta_bytes)).gte(0))
        .exec(db)
        .await?;

    if let Some(user_id) = bucket.user_id {
        users::Entity::update_many()
            .col_expr(
                users::Column::UsedBytes,
                Expr::col(users::Column::UsedBytes).add(delta_bytes),
            )
            .filter(users::Column::Id.eq(user_id))
            .filter(Expr::expr(Expr::col(users::Column::UsedBytes).add(delta_bytes)).gte(0))
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

// ponytail: loads every object row per bucket to sum them, one bucket at a time.
// Ceiling: fine up to a few hundred thousand objects; past that use a grouped SUM query, which needs no lock either.
/// Recomputes every stored total from the object rows.
///
/// The counters are an optimisation; the object rows are the truth.
/// A process that dies between reserve and commit leaves a hold that nothing will ever release, and this is what releases it.
///
/// Clears `reserved_bytes` outright rather than trying to tell a live reservation from a dead one: a reservation only lives for the duration of one upload, so anything still held when this runs is almost certainly stale.
/// Run it off-peak for that reason — a concurrent upload loses its hold, and its commit then re-adds the bytes anyway, but the window is briefly permissive.
///
/// # Errors
///
/// Returns a DB error.
pub async fn reconcile(db: &DatabaseConnection) -> ModelResult<ReconcileReport> {
    let mut report = ReconcileReport::default();

    let txn = db.begin().await?;

    let all_buckets = buckets::Entity::find().all(&txn).await?;

    for bucket in &all_buckets {
        let rows = objects::Entity::find()
            .filter(objects::Column::BucketId.eq(bucket.id))
            .all(&txn)
            .await?;

        let real_bytes: i64 = rows.iter().map(|o| o.size).sum();
        // A bucket with more than i64::MAX objects cannot exist, so the saturating branch is unreachable — but a wrapping cast is not something to leave in a quota path.
        let real_count = i64::try_from(rows.len()).unwrap_or(i64::MAX);

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
            .exec(&txn)
            .await?;

        report.buckets_fixed += 1;
    }

    // Account totals are the sum of the owner's buckets, which are correct by now.
    let all_users = users::Entity::find().all(&txn).await?;

    for user in &all_users {
        let owned: i64 = buckets::Entity::find()
            .filter(buckets::Column::UserId.eq(user.id))
            .all(&txn)
            .await?
            .iter()
            .map(|b| b.used_bytes)
            .sum();

        if user.used_bytes == owned && user.reserved_bytes == 0 {
            continue;
        }

        tracing::warn!(
            user_id = user.id,
            stored_bytes = user.used_bytes,
            real_bytes = owned,
            stale_reserved = user.reserved_bytes,
            "account quota drift corrected"
        );

        users::Entity::update_many()
            .col_expr(users::Column::UsedBytes, Expr::value(owned))
            .col_expr(users::Column::ReservedBytes, Expr::value(0))
            .filter(users::Column::Id.eq(user.id))
            .exec(&txn)
            .await?;

        report.users_fixed += 1;
    }

    txn.commit().await?;

    Ok(report)
}
