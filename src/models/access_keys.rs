use chrono::Utc;
use loco_rs::prelude::*;
use uuid::Uuid;

pub use super::_entities::access_keys::{ActiveModel, Column, Entity, Model};
use super::_entities::{access_key_permissions, access_key_prefixes};
use super::crypto;

pub const KEY_ACTIVE: &str = "active";
pub const KEY_DISABLED: &str = "disabled";
pub const KEY_REVOKED: &str = "revoked";
/// Never stored — derived from `expires_at`. The console's fourth status pill
/// ("Hết hạn") comes from `effective_status()`, so the UI never re-derives it.
pub const KEY_EXPIRED: &str = "expired";

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
        self.status == KEY_ACTIVE && !self.is_expired()
    }

    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.expires_at.is_some_and(|exp| exp <= Utc::now())
    }

    /// Status for the API/console: the stored one, unless the key lapsed while
    /// still marked active. Revoked and disabled keep their own status — a
    /// revoked key is not "merely expired".
    #[must_use]
    pub fn effective_status(&self) -> &str {
        if self.status == KEY_ACTIVE && self.is_expired() {
            KEY_EXPIRED
        } else {
            &self.status
        }
    }

    /// Days left before `expires_at`, for the console's "Còn 3 ngày" column.
    /// `None` when the key never expires; `Some(0)` on the last day.
    #[must_use]
    pub fn days_until_expiry(&self) -> Option<i64> {
        self.expires_at
            .map(|exp| (exp.with_timezone(&Utc) - Utc::now()).num_days().max(0))
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
