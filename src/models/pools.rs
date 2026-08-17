use loco_rs::prelude::*;
use sea_orm::QueryOrder;
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
    /// Returns an error when no pool has that name, or on DB failure.
    pub async fn find_by_name(db: &DatabaseConnection, name: &str) -> ModelResult<Self> {
        Entity::find()
            .filter(Column::Name.eq(name))
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
    pub const fn is_configured(&self) -> bool {
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
