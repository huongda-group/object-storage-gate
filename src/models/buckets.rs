use loco_rs::prelude::*;
use sea_orm::{PaginatorTrait, QueryOrder};
use uuid::Uuid;

use super::pools;

pub use super::_entities::buckets::{ActiveModel, Column, Entity, Model};

// Note on the generated entity: `name` carries `#[sea_orm(unique)]` even though names are only unique per owner.
// sea-orm-codegen sees the unique index `idx_buckets_owner_name ON buckets (COALESCE(user_id, 0), name)` and can only resolve its one real column.
// The DB constraint is the correct one; the attribute is codegen metadata (used when generating schema *from* entities, which this project never does — migrations own the schema).

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

/// Longest bucket name the API accepts, matching the S3 bucket-name rules the console mirrors.
pub const MAX_BUCKET_NAME_LEN: usize = 63;

/// Shortest bucket name the API accepts.
pub const MIN_BUCKET_NAME_LEN: usize = 3;

/// Validates a bucket name against the S3 naming rules.
///
/// Lowercase letters, digits, hyphens and dots; must start and end alphanumeric.
/// The gateway rewrites this into a path segment of the physical key, so a name that S3 would reject is a name that cannot round-trip.
///
/// # Errors
///
/// Returns a message error describing the first rule the name breaks.
pub fn validate_name(name: &str) -> ModelResult<()> {
    if name.len() < MIN_BUCKET_NAME_LEN || name.len() > MAX_BUCKET_NAME_LEN {
        return Err(ModelError::msg(
            "bucket name must be between 3 and 63 characters",
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
    {
        return Err(ModelError::msg(
            "bucket name may contain only lowercase letters, digits, hyphens and dots",
        ));
    }
    let first_last_ok = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    if !name.starts_with(first_last_ok) || !name.ends_with(first_last_ok) {
        return Err(ModelError::msg(
            "bucket name must start and end with a letter or digit",
        ));
    }
    if name.contains("..") {
        return Err(ModelError::msg("bucket name may not contain '..'"));
    }
    Ok(())
}

impl Model {
    /// A bucket by its public id, scoped to its owner.
    ///
    /// The ownership condition lives in the query, so a bucket belonging to someone else reads as absent rather than as forbidden — the same shape `access_keys::Model::find_by_pid_for_user` already uses.
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

    /// Create a bucket for a user, bound to the pool it proxies to. `max_bytes == 0` means unlimited.
    ///
    /// The pool lookup is not redundant with the foreign key: `SQLite` has no foreign key on `pool_id` (it cannot add one after the fact), so this check is what makes the three backends behave alike.
    ///
    /// # Errors
    /// Returns an error on an invalid name, a negative quota, an unknown pool, or DB failure (incl. duplicate name for the user).
    pub async fn create(
        db: &DatabaseConnection,
        user_id: i32,
        pool_id: i32,
        name: &str,
        max_bytes: i64,
    ) -> ModelResult<Self> {
        validate_name(name)?;
        if max_bytes < 0 {
            return Err(ModelError::msg("max_bytes must not be negative"));
        }
        pools::Model::find_by_id(db, pool_id)
            .await
            .map_err(|_| ModelError::msg("unknown pool"))?;

        Ok(ActiveModel {
            user_id: ActiveValue::set(Some(user_id)),
            pool_id: ActiveValue::set(pool_id),
            name: ActiveValue::set(name.to_string()),
            max_bytes: ActiveValue::set(max_bytes),
            ..Default::default()
        }
        .insert(db)
        .await?)
    }

    /// Buckets owned by a user.
    ///
    /// # Errors
    /// Returns an error on DB failure.
    pub async fn list_for_user(db: &DatabaseConnection, user_id: i32) -> ModelResult<Vec<Self>> {
        Ok(Entity::find()
            .filter(Column::UserId.eq(user_id))
            .order_by_asc(Column::Name)
            .all(db)
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

    /// How many buckets across every owner still point at this pool.
    /// The delete handler reads this so a pool still in use fails with a sentence rather than a constraint name.
    ///
    /// # Errors
    /// Returns an error on DB failure.
    pub async fn count_for_pool(db: &DatabaseConnection, pool_id: i32) -> ModelResult<u64> {
        Ok(Entity::find()
            .filter(Column::PoolId.eq(pool_id))
            .count(db)
            .await?)
    }

    #[must_use]
    pub const fn is_unlimited(&self) -> bool {
        self.max_bytes == 0
    }

    /// Objects are reachable over a public URL without a signed request.
    #[must_use]
    pub const fn is_public(&self) -> bool {
        self.public_enabled
    }
}
