use loco_rs::prelude::*;
use sea_orm::QueryOrder;
use uuid::Uuid;

use super::crypto;

pub use super::_entities::buckets::{ActiveModel, Column, Entity, Model};

// Note on the generated entity: `name` carries `#[sea_orm(unique)]` even though names are only unique per owner.
// sea-orm-codegen sees the unique index `idx_buckets_owner_name ON buckets (COALESCE(user_id, 0), name)` and can only resolve its one real column.
// The DB constraint is the correct one; the attribute is codegen metadata (used when generating schema *from* entities, which this project never does — migrations own the schema).

/// Backend store a bucket proxies to — the Pool form's provider dropdown (`console-object-storage-gate/project/Admin Buckets.dc.html` `PROVIDERS()`).
pub const PROVIDER_INTERNAL: &str = "internal";
pub const PROVIDER_AWS: &str = "aws";
pub const PROVIDER_R2: &str = "r2";
pub const PROVIDER_B2: &str = "b2";
pub const PROVIDER_SPACES: &str = "spaces";
pub const PROVIDER_MINIO: &str = "minio";
pub const PROVIDER_CUSTOM: &str = "custom";

pub const PROVIDERS: &[&str] = &[
    PROVIDER_INTERNAL,
    PROVIDER_AWS,
    PROVIDER_R2,
    PROVIDER_B2,
    PROVIDER_SPACES,
    PROVIDER_MINIO,
    PROVIDER_CUSTOM,
];

/// Store config as the admin form submits it.
/// `access_secret` is plaintext on the way in and never stored that way.
#[derive(Debug, Clone, Default)]
pub struct StoreParams {
    pub provider: String,
    pub region: Option<String>,
    pub api_endpoint: Option<String>,
    pub access_id: Option<String>,
    pub access_secret: Option<String>,
    pub public_enabled: bool,
}

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
        validate_name(name)?;
        if max_bytes < 0 {
            return Err(ModelError::msg("max_bytes must not be negative"));
        }
        Ok(ActiveModel {
            user_id: ActiveValue::set(Some(user_id)),
            name: ActiveValue::set(name.to_string()),
            max_bytes: ActiveValue::set(max_bytes),
            ..Default::default()
        }
        .insert(db)
        .await?)
    }

    /// Create a gateway-wide pool with no owner — the admin Pool screen's "hệ thống" rows.
    /// Its bytes count against no user's quota.
    ///
    /// # Errors
    /// Returns an error on DB failure (incl. duplicate system pool name).
    pub async fn create_system(
        db: &DatabaseConnection,
        name: &str,
        max_bytes: i64,
    ) -> ModelResult<Self> {
        Ok(ActiveModel {
            user_id: ActiveValue::set(None),
            name: ActiveValue::set(name.to_string()),
            max_bytes: ActiveValue::set(max_bytes),
            ..Default::default()
        }
        .insert(db)
        .await?)
    }

    /// Buckets owned by a user.
    /// System pool buckets (`user_id IS NULL`) are not part of anyone's account listing.
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

    /// # Errors
    /// Returns an error on DB failure.
    pub async fn find_system_by_name(
        db: &DatabaseConnection,
        name: &str,
    ) -> ModelResult<Option<Self>> {
        Ok(Entity::find()
            .filter(Column::UserId.is_null())
            .filter(Column::Name.eq(name))
            .one(db)
            .await?)
    }

    /// Replace this bucket's backend-store config.
    /// The secret is encrypted here; pass `None` to keep the stored one.
    ///
    /// # Errors
    /// Returns an error on an unknown provider or DB failure.
    pub async fn set_store(
        &self,
        db: &DatabaseConnection,
        params: &StoreParams,
    ) -> ModelResult<Self> {
        if !PROVIDERS.contains(&params.provider.as_str()) {
            return Err(ModelError::Message(format!(
                "unknown provider {}",
                params.provider
            )));
        }
        let mut active: ActiveModel = self.clone().into();
        active.provider = ActiveValue::set(params.provider.clone());
        active.region = ActiveValue::set(params.region.clone());
        active.api_endpoint = ActiveValue::set(params.api_endpoint.clone());
        active.access_id = ActiveValue::set(params.access_id.clone());
        if let Some(secret) = params.access_secret.as_deref() {
            active.access_secret_encrypted = ActiveValue::set(Some(crypto::encrypt(secret)));
        }
        active.public_enabled = ActiveValue::set(params.public_enabled);
        Ok(active.update(db).await?)
    }

    /// Plaintext upstream secret, for signing requests to the backend store.
    /// Never return this over the API.
    ///
    /// # Errors
    /// Returns an error if no secret is stored or decryption fails.
    pub fn decrypt_store_secret(&self) -> ModelResult<String> {
        let blob = self
            .access_secret_encrypted
            .as_deref()
            .ok_or_else(|| ModelError::Message("bucket has no store secret".to_string()))?;
        crypto::decrypt(blob).map_err(|e| ModelError::Message(e.to_string()))
    }

    #[must_use]
    pub const fn is_unlimited(&self) -> bool {
        self.max_bytes == 0
    }

    /// A pool with no owner: gateway-wide, outside every user's quota.
    #[must_use]
    pub const fn is_system(&self) -> bool {
        self.user_id.is_none()
    }

    /// Objects are reachable over a public URL without a signed request.
    #[must_use]
    pub const fn is_public(&self) -> bool {
        self.public_enabled
    }
}
