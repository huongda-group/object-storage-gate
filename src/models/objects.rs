use loco_rs::prelude::*;
use sea_orm::{QueryOrder, QuerySelect};
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
