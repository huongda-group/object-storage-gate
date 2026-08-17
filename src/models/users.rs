use async_trait::async_trait;
use loco_rs::{auth::jwt, hash, prelude::*};
use sea_orm::{PaginatorTrait, QueryOrder};
use serde::{Deserialize, Serialize};
use serde_json::Map;
use uuid::Uuid;

pub use super::_entities::users::{self, ActiveModel, Entity, Model};

/// Prefix length of a personal access token, stored in the clear so the hash can be looked up.
const PAT_PREFIX_LEN: usize = 12;

/// Builds a fresh personal access token and its stored representation.
///
/// Returns `(plaintext, prefix, hash)`. The plaintext leaves the process exactly once, at rotation; the column holds only the hash.
///
/// # Errors
///
/// When hashing fails.
fn mint_api_token() -> ModelResult<(String, String, String)> {
    let prefix = Uuid::new_v4().simple().to_string()[..PAT_PREFIX_LEN].to_string();
    let secret = Uuid::new_v4().simple().to_string();
    let token = format!("osg_pat_{prefix}_{secret}");
    let hashed = hash::hash_password(&token).map_err(|e| ModelError::Any(e.into()))?;
    Ok((token, prefix, hashed))
}

/// Extracts the lookup prefix from a presented token.
fn token_prefix(token: &str) -> Option<&str> {
    token.strip_prefix("osg_pat_")?.get(..PAT_PREFIX_LEN)
}

pub const ROLE_ADMIN: &str = "admin";
pub const ROLE_USER: &str = "user";

impl Model {
    #[must_use]
    pub fn is_admin(&self) -> bool {
        self.role == ROLE_ADMIN
    }

    /// Account-wide quota unlimited when `max_bytes == 0`.
    #[must_use]
    pub const fn is_unlimited(&self) -> bool {
        self.max_bytes == 0
    }
}

/// Shortest password the API will accept.
/// The starter allowed four characters, which is not a password.
pub const MIN_PASSWORD_LEN: usize = 8;

/// Longest password the API will accept, so a multi-megabyte body cannot reach Argon2.
pub const MAX_PASSWORD_LEN: usize = 256;

/// Validates a password before it is hashed.
///
/// # Errors
///
/// Returns a message error when the password is too short or too long.
pub fn validate_password(password: &str) -> ModelResult<()> {
    if password.len() < MIN_PASSWORD_LEN {
        return Err(ModelError::msg("password must be at least 8 characters"));
    }
    if password.len() > MAX_PASSWORD_LEN {
        return Err(ModelError::msg("password must be at most 256 characters"));
    }
    Ok(())
}

/// Validates a role string against the two roles the system knows.
///
/// # Errors
///
/// Returns a message error for anything else.
pub fn validate_role(role: &str) -> ModelResult<()> {
    if role == ROLE_ADMIN || role == ROLE_USER {
        return Ok(());
    }
    Err(ModelError::msg("role must be 'admin' or 'user'"))
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateUserParams {
    pub email: String,
    pub name: String,
    pub password: String,
    pub role: String,
    /// Required on purpose: `0` means unlimited, and unlimited must be a decision, never a default.
    pub max_bytes: i64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LoginParams {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RegisterParams {
    pub email: String,
    pub password: String,
    pub name: String,
}

#[derive(Debug, Validate, Deserialize)]
pub struct Validator {
    #[validate(length(min = 2, message = "Name must be at least 2 characters long."))]
    pub name: String,
    #[validate(email(message = "invalid email"))]
    pub email: String,
}

impl Validatable for ActiveModel {
    fn validator(&self) -> Box<dyn Validate> {
        Box::new(Validator {
            name: self.name.as_ref().to_owned(),
            email: self.email.as_ref().to_owned(),
        })
    }
}

#[async_trait::async_trait]
impl ActiveModelBehavior for super::_entities::users::ActiveModel {
    async fn before_save<C>(self, _db: &C, insert: bool) -> Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        self.validate()?;
        if insert {
            let mut this = self;
            this.pid = ActiveValue::Set(Uuid::new_v4());
            // The token minted here is intentionally discarded: a user who wants a PAT rotates one, and rotation is the only path that ever reveals it.
            let (_plaintext, prefix, hashed) =
                mint_api_token().map_err(|e| DbErr::Custom(e.to_string()))?;
            this.api_key = ActiveValue::Set(hashed);
            this.api_key_prefix = ActiveValue::Set(Some(prefix));
            Ok(this)
        } else {
            Ok(self)
        }
    }
}

#[async_trait]
impl Authenticable for Model {
    async fn find_by_api_key(db: &DatabaseConnection, api_key: &str) -> ModelResult<Self> {
        Self::find_by_api_key(db, api_key).await
    }

    async fn find_by_claims_key(db: &DatabaseConnection, claims_key: &str) -> ModelResult<Self> {
        Self::find_by_pid(db, claims_key).await
    }
}

impl Model {
    /// finds a user by the provided email
    ///
    /// # Errors
    ///
    /// When could not find user by the given token or DB query error
    pub async fn find_by_email(db: &DatabaseConnection, email: &str) -> ModelResult<Self> {
        let user = users::Entity::find()
            .filter(
                model::query::condition()
                    .eq(users::Column::Email, email)
                    .build(),
            )
            .one(db)
            .await?;
        user.ok_or_else(|| ModelError::EntityNotFound)
    }

    /// finds a user by the provided pid
    ///
    /// # Errors
    ///
    /// When could not find user  or DB query error
    pub async fn find_by_pid(db: &DatabaseConnection, pid: &str) -> ModelResult<Self> {
        let parse_uuid = Uuid::parse_str(pid).map_err(|e| ModelError::Any(e.into()))?;
        let user = users::Entity::find()
            .filter(
                model::query::condition()
                    .eq(users::Column::Pid, parse_uuid)
                    .build(),
            )
            .one(db)
            .await?;
        user.ok_or_else(|| ModelError::EntityNotFound)
    }

    /// Finds a user by a presented personal access token.
    ///
    /// Looks up by the token's plaintext prefix, then verifies the full token against the stored Argon2 hash.
    /// A prefix collision costs one extra hash verification and nothing else.
    ///
    /// # Errors
    ///
    /// When no user matches, or on a DB query error
    pub async fn find_by_api_key(db: &DatabaseConnection, api_key: &str) -> ModelResult<Self> {
        let Some(prefix) = token_prefix(api_key) else {
            return Err(ModelError::EntityNotFound);
        };
        let candidates = users::Entity::find()
            .filter(
                model::query::condition()
                    .eq(users::Column::ApiKeyPrefix, prefix)
                    .build(),
            )
            .all(db)
            .await?;

        candidates
            .into_iter()
            .find(|u| hash::verify_password(api_key, &u.api_key))
            .ok_or(ModelError::EntityNotFound)
    }

    /// Issues a fresh personal access token, invalidating the previous one.
    /// Returns the plaintext exactly once; it is not recoverable afterwards.
    ///
    /// # Errors
    ///
    /// When hashing or the DB write fails
    pub async fn rotate_api_token(self, db: &DatabaseConnection) -> ModelResult<(Self, String)> {
        let (token, prefix, hashed) = mint_api_token()?;
        let mut am: ActiveModel = self.into();
        am.api_key = ActiveValue::set(hashed);
        am.api_key_prefix = ActiveValue::set(Some(prefix));
        let user = am.update(db).await?;
        Ok((user, token))
    }

    /// Verifies whether the provided plain password matches the hashed password
    ///
    /// # Errors
    ///
    /// when could not verify password
    #[must_use]
    pub fn verify_password(&self, password: &str) -> bool {
        hash::verify_password(password, &self.password)
    }

    /// Returns whether the instance has no user at all, i.e. it still needs its first-run admin setup.
    ///
    /// # Errors
    ///
    /// When the query fails
    pub async fn any_exists(db: &DatabaseConnection) -> ModelResult<bool> {
        Ok(users::Entity::find().one(db).await?.is_some())
    }

    /// Creates the first-run admin: an admin-role user of a brand-new instance.
    /// Refused once any user exists.
    ///
    /// # Errors
    ///
    /// When a user already exists, or the user could not be saved into the DB
    pub async fn create_first_admin(
        db: &DatabaseConnection,
        params: &RegisterParams,
    ) -> ModelResult<Self> {
        let txn = db.begin().await?;

        // ponytail: read-committed lets two concurrent setup calls on a brand-new empty DB both see zero users and both become admin.
        // Take a lock (advisory / sentinel row) if that window ever matters.
        if users::Entity::find().one(&txn).await?.is_some() {
            return Err(ModelError::EntityAlreadyExists {});
        }

        let password_hash =
            hash::hash_password(&params.password).map_err(|e| ModelError::Any(e.into()))?;
        let user = users::ActiveModel {
            email: ActiveValue::set(params.email.clone()),
            password: ActiveValue::set(password_hash),
            name: ActiveValue::set(params.name.clone()),
            role: ActiveValue::set(ROLE_ADMIN.to_string()),
            ..Default::default()
        }
        .insert(&txn)
        .await?;

        txn.commit().await?;

        Ok(user)
    }

    /// Creates a user on an admin's behalf, with a temporary password the user must replace at first login.
    ///
    /// # Errors
    ///
    /// When the email is taken, the role or password is invalid, or the DB write fails
    pub async fn create_by_admin(
        db: &DatabaseConnection,
        params: &CreateUserParams,
    ) -> ModelResult<Self> {
        validate_role(&params.role)?;
        validate_password(&params.password)?;
        if params.max_bytes < 0 {
            return Err(ModelError::msg("max_bytes must not be negative"));
        }

        let txn = db.begin().await?;

        if users::Entity::find()
            .filter(
                model::query::condition()
                    .eq(users::Column::Email, &params.email)
                    .build(),
            )
            .one(&txn)
            .await?
            .is_some()
        {
            return Err(ModelError::EntityAlreadyExists {});
        }

        let password_hash =
            hash::hash_password(&params.password).map_err(|e| ModelError::Any(e.into()))?;
        let user = users::ActiveModel {
            email: ActiveValue::set(params.email.clone()),
            password: ActiveValue::set(password_hash),
            name: ActiveValue::set(params.name.clone()),
            role: ActiveValue::set(params.role.clone()),
            max_bytes: ActiveValue::set(params.max_bytes),
            must_change_password: ActiveValue::set(true),
            ..Default::default()
        }
        .insert(&txn)
        .await?;

        txn.commit().await?;

        Ok(user)
    }

    /// Lists every user, newest first.
    ///
    /// # Errors
    ///
    /// When the query fails
    pub async fn list_all(db: &DatabaseConnection) -> ModelResult<Vec<Self>> {
        Ok(users::Entity::find()
            .order_by_desc(users::Column::Id)
            .all(db)
            .await?)
    }

    /// Counts admins, so the last one cannot be demoted or deleted.
    ///
    /// # Errors
    ///
    /// When the query fails
    pub async fn admin_count(db: &DatabaseConnection) -> ModelResult<u64> {
        Ok(users::Entity::find()
            .filter(
                model::query::condition()
                    .eq(users::Column::Role, ROLE_ADMIN)
                    .build(),
            )
            .count(db)
            .await?)
    }

    /// Creates a JWT
    ///
    /// # Errors
    ///
    /// when could not convert user claims to jwt token
    pub fn generate_jwt(&self, secret: &str, expiration: u64) -> ModelResult<String> {
        jwt::JWT::new(secret)
            .generate_token(expiration, self.pid.to_string(), Map::new())
            .map_err(ModelError::from)
    }
}

impl ActiveModel {
    /// Replaces the user's password hash.
    /// Used by the admin reset endpoint and by the self-service change-password endpoint.
    ///
    /// # Errors
    ///
    /// when has DB query error or could not hash the given password
    pub async fn reset_password(
        self,
        db: &DatabaseConnection,
        password: &str,
    ) -> ModelResult<Model> {
        self.set_password(db, password, false).await
    }

    /// Replaces the password hash and sets whether the user must change it at next login.
    /// An admin-issued temporary password passes `must_change = true`; a self-service change passes `false`.
    ///
    /// # Errors
    ///
    /// when has DB query error or could not hash the given password
    pub async fn set_password(
        mut self,
        db: &DatabaseConnection,
        password: &str,
        must_change: bool,
    ) -> ModelResult<Model> {
        self.password =
            ActiveValue::set(hash::hash_password(password).map_err(|e| ModelError::Any(e.into()))?);
        self.must_change_password = ActiveValue::set(must_change);
        self.update(db).await.map_err(ModelError::from)
    }
}
