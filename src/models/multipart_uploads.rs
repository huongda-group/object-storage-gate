//! Open multipart uploads.
//!
//! The store keeps the parts; this table keeps the mapping from the `UploadId` a client holds to the one the store issued, plus the quota currently held for the upload.
//! Those two facts are the whole reason the table exists.
use loco_rs::prelude::*;
use sea_orm::{sea_query::Expr, QueryOrder};
use uuid::Uuid;

pub use super::_entities::multipart_uploads::{ActiveModel, Column, Entity, Model};

#[async_trait::async_trait]
impl ActiveModelBehavior for super::_entities::multipart_uploads::ActiveModel {
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
    /// # Errors
    /// Returns an error on DB failure.
    pub async fn create(
        db: &DatabaseConnection,
        bucket_id: i32,
        key: &str,
        upstream_upload_id: &str,
    ) -> ModelResult<Self> {
        Ok(ActiveModel {
            bucket_id: ActiveValue::set(bucket_id),
            object_key: ActiveValue::set(key.to_string()),
            upstream_upload_id: ActiveValue::set(upstream_upload_id.to_string()),
            reserved_bytes: ActiveValue::set(0),
            ..Default::default()
        }
        .insert(db)
        .await?)
    }

    /// Looks up an upload by the `UploadId` a client presented, pinned to the bucket and key from the request path.
    ///
    /// Pinning is the point: without it an `UploadId` issued for one bucket could be replayed against another bucket's path, and every part would land wherever the path said.
    ///
    /// # Errors
    /// Returns an error when no open upload matches all three, or on DB failure.
    pub async fn find_for(
        db: &DatabaseConnection,
        pid: &str,
        bucket_id: i32,
        key: &str,
    ) -> ModelResult<Self> {
        let parsed = Uuid::parse_str(pid).map_err(|_| ModelError::EntityNotFound)?;
        Entity::find()
            .filter(Column::Pid.eq(parsed))
            .filter(Column::BucketId.eq(bucket_id))
            .filter(Column::ObjectKey.eq(key))
            .one(db)
            .await?
            .ok_or(ModelError::EntityNotFound)
    }

    /// # Errors
    /// Returns an error when no upload has that id, or on DB failure.
    pub async fn find_by_id(db: &DatabaseConnection, id: i32) -> ModelResult<Self> {
        Entity::find_by_id(id)
            .one(db)
            .await?
            .ok_or(ModelError::EntityNotFound)
    }

    /// Adds to the running hold.
    ///
    /// A read-modify-write here would lose an update between two concurrent `UploadPart` calls, and the loss shows up much later as an `Abort` that releases too little — which is a slow quota leak nobody can trace back.
    ///
    /// # Errors
    /// Returns an error on DB failure, or when the row is gone.
    pub async fn add_reserved(db: &DatabaseConnection, id: i32, bytes: i64) -> ModelResult<()> {
        let updated = Entity::update_many()
            .col_expr(
                Column::ReservedBytes,
                Expr::col(Column::ReservedBytes).add(bytes),
            )
            .filter(Column::Id.eq(id))
            .exec(db)
            .await?;
        if updated.rows_affected == 0 {
            return Err(ModelError::EntityNotFound);
        }
        Ok(())
    }

    /// Open uploads in a bucket, for `ListMultipartUploads`.
    ///
    /// # Errors
    /// Returns an error on DB failure.
    pub async fn list_for_bucket(
        db: &DatabaseConnection,
        bucket_id: i32,
        prefix: &str,
    ) -> ModelResult<Vec<Self>> {
        let mut q = Entity::find()
            .filter(Column::BucketId.eq(bucket_id))
            .order_by_asc(Column::ObjectKey);
        if !prefix.is_empty() {
            q = q.filter(Column::ObjectKey.gte(prefix));
            if let Some(upper) = super::objects::prefix_upper_bound(prefix) {
                q = q.filter(Column::ObjectKey.lt(upper));
            }
        }
        Ok(q.all(db).await?)
    }

    /// Uploads left open longer than `days`, for the cleanup task.
    ///
    /// # Errors
    /// Returns an error on DB failure.
    pub async fn older_than(db: &DatabaseConnection, days: i64) -> ModelResult<Vec<Self>> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(days);
        Ok(Entity::find()
            .filter(Column::CreatedAt.lt(cutoff))
            .all(db)
            .await?)
    }

    /// # Errors
    /// Returns an error on DB failure.
    pub async fn remove(self, db: &DatabaseConnection) -> ModelResult<()> {
        let am: ActiveModel = self.into();
        am.delete(db).await?;
        Ok(())
    }
}
