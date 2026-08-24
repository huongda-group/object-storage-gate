use loco_rs::prelude::*;
use sea_orm::{sea_query::Expr, QueryOrder, QuerySelect};
use uuid::Uuid;

use super::quota;

pub use super::_entities::objects::{ActiveModel, Column, Entity, Model};

/// The smallest string strictly greater than every string starting with `prefix`.
///
/// Increments the last code point that can be incremented, dropping trailing ones that cannot.
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

/// A quota hold taken before an upload, waiting to become stored bytes.
///
/// The gateway needs the upstream upload to sit between reserve and commit; a single `put_object` that owns the whole sequence cannot express that.
/// `commit` and `abort` both consume `self`, so dropping a hold without deciding is not something that happens by accident.
#[derive(Debug)]
pub struct PendingPut {
    bucket_id: i32,
    object_key: String,
    size: i64,
    reservation: Option<quota::Reservation>,
    delta_bytes: i64,
    delta_objects: i64,
}

impl PendingPut {
    /// The bytes this write will add, negative when it shrinks the object.
    #[must_use]
    pub const fn delta_bytes(&self) -> i64 {
        self.delta_bytes
    }

    /// Turns the hold into stored bytes once the upload has landed.
    ///
    /// # Errors
    /// Returns an error on DB failure.
    pub async fn commit(
        self,
        db: &DatabaseConnection,
        etag: &str,
        content_type: &str,
    ) -> ModelResult<Model> {
        // The hold must come back if the metadata write fails: `commit` consumes `self`, so a bare `?` here would drop the reservation without releasing it, and a leaked hold is a bucket that slowly stops accepting writes for no visible reason.
        let row = match Model::write_row(
            db,
            self.bucket_id,
            &self.object_key,
            self.size,
            etag,
            content_type,
        )
        .await
        {
            Ok(row) => row,
            Err(e) => {
                if let Some(reservation) = self.reservation {
                    quota::release(db, &reservation).await?;
                }
                return Err(e);
            }
        };

        if let Some(reservation) = self.reservation {
            quota::commit(db, &reservation, self.delta_objects).await?;
        } else {
            quota::settle(db, self.bucket_id, self.delta_bytes, self.delta_objects).await?;
        }

        Ok(row)
    }

    /// Gives the hold back. Nothing was written, so there is nothing to undo.
    ///
    /// # Errors
    /// Returns an error on DB failure.
    pub async fn abort(self, db: &DatabaseConnection) -> ModelResult<()> {
        if let Some(reservation) = self.reservation {
            quota::release(db, &reservation).await?;
        }
        Ok(())
    }
}

impl Model {
    /// Holds quota for a write without touching metadata.
    ///
    /// Only a growing write needs a reservation; a shrink or a same-size overwrite settles at commit time.
    ///
    /// # Errors
    /// Returns a `quota exceeded` error when there is no room, or a DB error.
    pub async fn begin_put(
        db: &DatabaseConnection,
        bucket_id: i32,
        key: &str,
        size: i64,
    ) -> ModelResult<PendingPut> {
        let existing = Self::get(db, bucket_id, key).await?;
        let previous_size = existing.as_ref().map_or(0, |o| o.size);
        let delta_bytes = size - previous_size;
        let delta_objects = i64::from(existing.is_none());

        let reservation = if delta_bytes > 0 {
            Some(quota::reserve(db, bucket_id, delta_bytes).await?)
        } else {
            None
        };

        Ok(PendingPut {
            bucket_id,
            object_key: key.to_string(),
            size,
            reservation,
            delta_bytes,
            delta_objects,
        })
    }

    /// Insert a new object or overwrite the existing `(bucket_id, key)` row (`PutObject` semantics, versioning off).
    ///
    /// For callers with nothing to do between the reservation and the write. The gateway uses `begin_put` instead, because the upstream upload goes in that gap.
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
        let pending = Self::begin_put(db, bucket_id, key, size).await?;
        pending.commit(db, etag, content_type).await
    }

    /// Writes metadata without touching quota.
    ///
    /// Only the multipart path may use this. Multipart accumulates its reservation across many `UploadPart` requests, so no `PendingPut` can hold it and `CompleteMultipartUpload` owns the accounting itself (spec §10).
    /// Nothing else may call it: every other caller would silently store bytes nobody is charged for.
    ///
    /// # Errors
    /// Returns an error on DB failure.
    pub async fn record_put(
        db: &DatabaseConnection,
        bucket_id: i32,
        key: &str,
        size: i64,
        etag: &str,
        content_type: &str,
    ) -> ModelResult<Self> {
        Self::write_row(db, bucket_id, key, size, etag, content_type).await
    }

    /// The bare upsert, without any quota accounting.
    ///
    /// Tries the update first and only inserts when nothing was updated, then retries the update once if the insert lost a race.
    /// The previous read-then-insert let two concurrent writes both see no row, both insert, and one hit the unique index with a 500 — which S3 clients trigger routinely, because retrying is what they do.
    async fn write_row(
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
                // Loop once more; the update finds the row this time.
                Err(_) if attempt == 0 => (),
                Err(e) => return Err(e.into()),
            }
        }

        Err(ModelError::msg("put_object could not converge"))
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

    /// Removes an object and returns its bytes to the quota.
    ///
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

    /// Objects in a bucket whose key starts with `prefix`, up to `limit`, ordered by key (`ListObjectsV2` backing query).
    ///
    /// Uses a range comparison rather than `LIKE`.
    /// sea-orm's `starts_with` builds `format!("{}%", s)` with no escaping, so `%` and `_` in a caller-supplied prefix act as wildcards, and `SQLite`'s `LIKE` is case-insensitive for ASCII while Postgres's is not.
    /// A range is literal on all three backends and, unlike a `LIKE`, can use the `(bucket_id, object_key)` index.
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
}

#[cfg(test)]
mod tests {
    use super::prefix_upper_bound;

    #[test]
    fn upper_bound_increments_the_last_character() {
        // '/' is U+002F, so the range for "a/" is ["a/", "a0").
        assert_eq!(prefix_upper_bound("a/").as_deref(), Some("a0"));
        assert_eq!(
            prefix_upper_bound("tenants/a").as_deref(),
            Some("tenants/b")
        );
    }

    #[test]
    fn upper_bound_skips_the_surrogate_gap() {
        // char::from_u32 returns None for D800..DFFF, so the loop has to fall back a character.
        let s = format!("x{}", '\u{D7FF}');
        let bound = prefix_upper_bound(&s).unwrap();
        assert!(bound > s);
    }

    #[test]
    fn upper_bound_of_an_all_max_prefix_is_none() {
        let s = format!("{}", char::MAX);
        assert_eq!(prefix_upper_bound(&s), None);
    }
}
