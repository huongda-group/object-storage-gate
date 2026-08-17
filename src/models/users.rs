use async_trait::async_trait;
use loco_rs::{auth::jwt, hash, prelude::*};
use serde::{Deserialize, Serialize};
use serde_json::Map;
use uuid::Uuid;

pub use super::_entities::users::{self, ActiveModel, Entity, Model};

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
            this.api_key = ActiveValue::Set(format!("lo-{}", Uuid::new_v4()));
            Ok(this)
        } else {
            Ok(self)
        }
    }
}

#[async_trait]
impl Authenticable for Model {
    async fn find_by_api_key(db: &DatabaseConnection, api_key: &str) -> ModelResult<Self> {
        let user = users::Entity::find()
            .filter(
                model::query::condition()
                    .eq(users::Column::ApiKey, api_key)
                    .build(),
            )
            .one(db)
            .await?;
        user.ok_or_else(|| ModelError::EntityNotFound)
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

    /// finds a user by the provided api key
    ///
    /// # Errors
    ///
    /// When could not find user by the given token or DB query error
    pub async fn find_by_api_key(db: &DatabaseConnection, api_key: &str) -> ModelResult<Self> {
        let user = users::Entity::find()
            .filter(
                model::query::condition()
                    .eq(users::Column::ApiKey, api_key)
                    .build(),
            )
            .one(db)
            .await?;
        user.ok_or_else(|| ModelError::EntityNotFound)
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
