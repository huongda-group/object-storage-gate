use chrono::Utc;
use loco_rs::prelude::*;
use sea_orm::{prelude::DateTimeWithTimeZone, sea_query::Expr, QueryOrder, TransactionTrait};
use uuid::Uuid;

pub use super::_entities::access_keys::{ActiveModel, Column, Entity, Model};
use super::_entities::{access_key_permissions, access_key_prefixes};
use super::crypto;

pub const KEY_ACTIVE: &str = "active";
pub const KEY_DISABLED: &str = "disabled";
pub const KEY_REVOKED: &str = "revoked";
/// Never stored — derived from `expires_at`.
/// The console's fourth status pill ("Hết hạn") comes from `effective_status()`, so the UI never re-derives it.
pub const KEY_EXPIRED: &str = "expired";

pub const ACTION_READ: &str = "read";
pub const ACTION_WRITE: &str = "write";
pub const ACTION_DELETE: &str = "delete";
pub const ACTION_LIST: &str = "list";
pub const ACTION_MULTIPART: &str = "multipart";
pub const ACTION_PRESIGNED: &str = "presigned";

/// Whether `prefix` authorises `key`.
///
/// A prefix must land on a path boundary.
/// Without that rule a key scoped to `team` also authorises `teamsecret/`, which is a different tenant's folder as far as the person who issued the key is concerned.
#[must_use]
pub fn prefix_allows(prefix: &str, key: &str) -> bool {
    key.starts_with(prefix)
        && (prefix.ends_with('/')
            || key.len() == prefix.len()
            || key.as_bytes()[prefix.len()] == b'/')
}

pub const LABELS: &[&str] = &["primary", "backup", "temporary", "ci", "readonly"];
pub const ACTIONS: &[&str] = &[
    ACTION_READ,
    ACTION_WRITE,
    ACTION_DELETE,
    ACTION_LIST,
    ACTION_MULTIPART,
    ACTION_PRESIGNED,
];

pub const MAX_PREFIXES: usize = 20;
pub const MAX_PREFIX_LEN: usize = 512;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CreateKeyParams {
    pub label: String,
    pub expires_at: Option<DateTimeWithTimeZone>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub prefixes: Vec<String>,
}

/// A key plus its policy, loaded together for listings.
#[derive(Debug, Clone)]
pub struct KeyWithPolicy {
    pub key: Model,
    pub permissions: Vec<String>,
    pub prefixes: Vec<String>,
}

fn invalid(msg: &str) -> ModelError {
    ModelError::Message(msg.to_string())
}

/// # Errors
/// Returns an error when the label is not one of `LABELS`.
pub fn validate_label(label: &str) -> ModelResult<()> {
    if LABELS.contains(&label) {
        Ok(())
    } else {
        Err(invalid(
            "label must be one of: primary, backup, temporary, ci, readonly",
        ))
    }
}

/// # Errors
/// Returns an error when any action is outside `ACTIONS`.
pub fn validate_actions(actions: &[String]) -> ModelResult<()> {
    for a in actions {
        if !ACTIONS.contains(&a.as_str()) {
            return Err(invalid(&format!("unknown permission: {a}")));
        }
    }
    Ok(())
}

/// Prefix decides what a key can read and write, so this is a trust boundary: every write path goes through here, not through the controller.
///
/// # Errors
/// Returns an error when a prefix is empty, absolute, contains `..`, is too long, or when there are too many of them.
pub fn validate_prefixes(prefixes: &[String]) -> ModelResult<()> {
    if prefixes.len() > MAX_PREFIXES {
        return Err(invalid("at most 20 prefixes per key"));
    }
    for p in prefixes {
        if p.is_empty() {
            return Err(invalid("prefix must not be empty"));
        }
        if p.len() > MAX_PREFIX_LEN {
            return Err(invalid("prefix must be at most 512 characters"));
        }
        if p.starts_with('/') {
            return Err(invalid("prefix must not start with /"));
        }
        if p.contains("..") {
            return Err(invalid("prefix must not contain .."));
        }
    }
    Ok(())
}

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
    /// Create an access key for a user with its policy.
    /// Returns the model plus the plaintext secret ONCE (stored only encrypted; decryptable internally for `SigV4`).
    ///
    /// # Errors
    /// Returns an error on validation failure or DB failure.
    pub async fn create_key(
        db: &DatabaseConnection,
        user_id: i32,
        params: &CreateKeyParams,
    ) -> ModelResult<(Self, String)> {
        validate_label(&params.label)?;
        validate_actions(&params.permissions)?;
        validate_prefixes(&params.prefixes)?;
        if params.expires_at.is_some_and(|e| e <= Utc::now()) {
            return Err(invalid("expires_at must be in the future"));
        }

        let access_key_id = format!("OSG{}", Uuid::new_v4().simple());
        let secret = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());

        let txn = db.begin().await?;
        let model = ActiveModel {
            user_id: ActiveValue::set(user_id),
            access_key_id: ActiveValue::set(access_key_id),
            secret_encrypted: ActiveValue::set(crypto::encrypt(&secret)),
            label: ActiveValue::set(params.label.clone()),
            status: ActiveValue::set(KEY_ACTIVE.to_string()),
            expires_at: ActiveValue::set(params.expires_at),
            ..Default::default()
        }
        .insert(&txn)
        .await?;

        for action in &params.permissions {
            access_key_permissions::ActiveModel {
                access_key_id: ActiveValue::set(model.id),
                action: ActiveValue::set(action.clone()),
                ..Default::default()
            }
            .insert(&txn)
            .await?;
        }
        for prefix in &params.prefixes {
            access_key_prefixes::ActiveModel {
                access_key_id: ActiveValue::set(model.id),
                prefix: ActiveValue::set(prefix.clone()),
                ..Default::default()
            }
            .insert(&txn)
            .await?;
        }
        txn.commit().await?;

        Ok((model, secret))
    }

    /// All keys of a user with their policy, in 3 queries — never one query per key.
    ///
    /// # Errors
    /// Returns an error on DB failure.
    pub async fn list_for_user(
        db: &DatabaseConnection,
        user_id: i32,
    ) -> ModelResult<Vec<KeyWithPolicy>> {
        let keys = Entity::find()
            .filter(Column::UserId.eq(user_id))
            .order_by_desc(Column::CreatedAt)
            .all(db)
            .await?;
        let ids: Vec<i32> = keys.iter().map(|k| k.id).collect();

        let perms = access_key_permissions::Entity::find()
            .filter(access_key_permissions::Column::AccessKeyId.is_in(ids.clone()))
            .all(db)
            .await?;
        let prefixes = access_key_prefixes::Entity::find()
            .filter(access_key_prefixes::Column::AccessKeyId.is_in(ids))
            .all(db)
            .await?;

        Ok(keys
            .into_iter()
            .map(|key| KeyWithPolicy {
                permissions: perms
                    .iter()
                    .filter(|p| p.access_key_id == key.id)
                    .map(|p| p.action.clone())
                    .collect(),
                prefixes: prefixes
                    .iter()
                    .filter(|p| p.access_key_id == key.id)
                    .map(|p| p.prefix.clone())
                    .collect(),
                key,
            })
            .collect())
    }

    /// Ownership is part of the query, not a check after loading: a key of another user is indistinguishable from a key that does not exist.
    ///
    /// # Errors
    /// Returns `EntityNotFound` when the pid is malformed, missing, or owned by someone else.
    pub async fn find_by_pid_for_user(
        db: &DatabaseConnection,
        pid: &str,
        user_id: i32,
    ) -> ModelResult<Self> {
        let uuid = Uuid::parse_str(pid).map_err(|_| ModelError::EntityNotFound)?;
        Entity::find()
            .filter(Column::Pid.eq(uuid))
            .filter(Column::UserId.eq(user_id))
            .one(db)
            .await?
            .ok_or(ModelError::EntityNotFound)
    }

    /// Finds an access key by the public id a client presents, but only while it is usable.
    ///
    /// A revoked, disabled or expired key must read as absent: that is a credential-validity question, not an authorisation one, and one answer for all three does not confirm to the caller whether the key exists.
    ///
    /// # Errors
    /// Returns an error when no usable key has that id, or on DB failure.
    pub async fn find_by_access_key_id(
        db: &DatabaseConnection,
        access_key_id: &str,
    ) -> ModelResult<Self> {
        let key = Entity::find()
            .filter(Column::AccessKeyId.eq(access_key_id))
            .one(db)
            .await?
            .ok_or(ModelError::EntityNotFound)?;

        if key.is_usable() {
            Ok(key)
        } else {
            Err(ModelError::EntityNotFound)
        }
    }

    /// Whether this key's prefix policy authorises `key`.
    /// A key with no prefixes is scoped to the whole bucket.
    ///
    /// # Errors
    /// Returns an error on DB failure.
    pub async fn allows_key(&self, db: &DatabaseConnection, key: &str) -> ModelResult<bool> {
        let prefixes = self.prefixes(db).await?;
        if prefixes.is_empty() {
            return Ok(true);
        }
        Ok(prefixes.iter().any(|p| prefix_allows(p, key)))
    }

    /// Whether this key carries `action`.
    ///
    /// # Errors
    /// Returns an error on DB failure.
    pub async fn allows_action(&self, db: &DatabaseConnection, action: &str) -> ModelResult<bool> {
        Ok(self.permissions(db).await?.iter().any(|p| p == action))
    }

    /// Decrypt the stored secret for `SigV4` verification.
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

    /// Status for the API/console: the stored one, unless the key lapsed while still marked active.
    /// Revoked and disabled keep their own status — a revoked key is not "merely expired".
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

    /// Move a key between `active` and `disabled`.
    /// `revoked` is terminal: a revoked key is never brought back, because callers may already treat it as gone.
    ///
    /// The guard lives in the UPDATE rather than in a check against `self`, because `self` may have been loaded before a concurrent revoke landed — which is exactly the window an admin hits when they revoke a leaked key while the console has a PATCH in flight.
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

        Self::reload(db, self.id).await
    }

    /// Permanent.
    /// The row stays for audit; only the status changes.
    /// Idempotent: revoking an already-revoked key is not an error, because containing an incident is exactly when someone clicks twice.
    ///
    /// # Errors
    /// Returns an error on DB failure.
    pub async fn revoke(self, db: &DatabaseConnection) -> ModelResult<Self> {
        Entity::update_many()
            .col_expr(Column::Status, Expr::value(KEY_REVOKED))
            .filter(Column::Id.eq(self.id))
            .exec(db)
            .await?;

        Self::reload(db, self.id).await
    }

    /// Reloads a key by its primary key, after a guarded UPDATE has changed it.
    ///
    /// # Errors
    /// Returns an error when the row is gone, or on DB failure.
    async fn reload(db: &DatabaseConnection, id: i32) -> ModelResult<Self> {
        Entity::find_by_id(id)
            .one(db)
            .await?
            .ok_or(ModelError::EntityNotFound)
    }

    /// Issue a replacement key with the same policy and disable this one.
    /// The old key is disabled rather than revoked so a running app has a window to swap its config.
    ///
    /// Disables first, then creates.
    /// The other order left a live replacement behind whenever the second write failed, and the caller got an `Err` so nobody ever saw its secret.
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

    /// Replace the key's permissions.
    /// Validation runs before the transaction opens, so a rejected update never deletes the current rows.
    ///
    /// # Errors
    /// Returns an error on unknown action or DB failure.
    pub async fn set_permissions(
        &self,
        db: &DatabaseConnection,
        actions: &[String],
    ) -> ModelResult<()> {
        validate_actions(actions)?;
        let txn = db.begin().await?;
        access_key_permissions::Entity::delete_many()
            .filter(access_key_permissions::Column::AccessKeyId.eq(self.id))
            .exec(&txn)
            .await?;
        for action in actions {
            access_key_permissions::ActiveModel {
                access_key_id: ActiveValue::set(self.id),
                action: ActiveValue::set(action.clone()),
                ..Default::default()
            }
            .insert(&txn)
            .await?;
        }
        txn.commit().await?;
        Ok(())
    }

    /// Replace the key's prefixes.
    /// Same ordering rule as `set_permissions`.
    ///
    /// # Errors
    /// Returns an error on an invalid prefix or DB failure.
    pub async fn set_prefixes(
        &self,
        db: &DatabaseConnection,
        prefixes: &[String],
    ) -> ModelResult<()> {
        validate_prefixes(prefixes)?;
        let txn = db.begin().await?;
        access_key_prefixes::Entity::delete_many()
            .filter(access_key_prefixes::Column::AccessKeyId.eq(self.id))
            .exec(&txn)
            .await?;
        for prefix in prefixes {
            access_key_prefixes::ActiveModel {
                access_key_id: ActiveValue::set(self.id),
                prefix: ActiveValue::set(prefix.clone()),
                ..Default::default()
            }
            .insert(&txn)
            .await?;
        }
        txn.commit().await?;
        Ok(())
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
