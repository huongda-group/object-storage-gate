//! What every S3 request left behind.
//!
//! `outcome` is stored separately from `status_code` because the same 403 covers a wrong
//! signature, a missing permission and a full bucket — three different operational problems that
//! a status-code parse cannot tell apart.
use loco_rs::prelude::*;
use sea_orm::{QueryOrder, QuerySelect};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use super::_entities::audit_logs::{ActiveModel, Column, Entity, Model};

pub const ACTION_READ: &str = "read";
pub const ACTION_WRITE: &str = "write";
pub const ACTION_DELETE: &str = "delete";
pub const ACTION_LIST: &str = "list";
pub const ACTION_MULTIPART: &str = "multipart";
pub const ACTION_PRESIGNED: &str = "presigned";
/// A request that never got far enough to name an action.
pub const ACTION_AUTH: &str = "auth";

pub const OUTCOME_OK: &str = "ok";
pub const OUTCOME_DENIED: &str = "denied";
pub const OUTCOME_QUOTA: &str = "quota_exceeded";
pub const OUTCOME_NOT_FOUND: &str = "not_found";
pub const OUTCOME_ERROR: &str = "error";

/// What the request path hands to the queue.
///
/// Everything is owned and serialisable, because it crosses a queue boundary and the request it
/// describes is long gone by the time it is written.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub user_id: Option<i32>,
    /// The string the client sent, kept even when no such key exists — that is how key probing shows up.
    pub access_key_id: Option<String>,
    pub bucket_id: Option<i32>,
    pub object_key: Option<String>,
    pub action: String,
    pub outcome: String,
    pub status_code: i32,
    pub bytes: i64,
    pub duration_ms: i32,
    pub request_id: String,
    pub ip: String,
    pub user_agent: Option<String>,
}

/// Which outcome an HTTP status means.
#[must_use]
pub const fn outcome_for(status: u16) -> &'static str {
    match status {
        200..=399 => OUTCOME_OK,
        404 => OUTCOME_NOT_FOUND,
        // Every other 4xx is a refusal. The specific S3 code is what separates a wrong signature from a full bucket, and the caller supplies that.
        400..=499 => OUTCOME_DENIED,
        _ => OUTCOME_ERROR,
    }
}

#[async_trait::async_trait]
impl ActiveModelBehavior for super::_entities::audit_logs::ActiveModel {
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
    pub async fn record(db: &DatabaseConnection, entry: &AuditEntry) -> ModelResult<Self> {
        Ok(ActiveModel {
            occurred_at: ActiveValue::set(chrono::Utc::now().into()),
            user_id: ActiveValue::set(entry.user_id),
            access_key_id: ActiveValue::set(entry.access_key_id.clone()),
            bucket_id: ActiveValue::set(entry.bucket_id),
            object_key: ActiveValue::set(entry.object_key.clone()),
            action: ActiveValue::set(entry.action.clone()),
            outcome: ActiveValue::set(entry.outcome.clone()),
            status_code: ActiveValue::set(entry.status_code),
            bytes: ActiveValue::set(entry.bytes),
            duration_ms: ActiveValue::set(entry.duration_ms),
            request_id: ActiveValue::set(entry.request_id.clone()),
            ip: ActiveValue::set(entry.ip.clone()),
            user_agent: ActiveValue::set(entry.user_agent.clone()),
            ..Default::default()
        }
        .insert(db)
        .await?)
    }

    /// # Errors
    /// Returns an error on DB failure.
    pub async fn list_recent(db: &DatabaseConnection, limit: u64) -> ModelResult<Vec<Self>> {
        Ok(Entity::find()
            .order_by_desc(Column::OccurredAt)
            .order_by_desc(Column::Id)
            .limit(limit)
            .all(db)
            .await?)
    }

    /// # Errors
    /// Returns an error on DB failure.
    pub async fn list_for_user(
        db: &DatabaseConnection,
        user_id: i32,
        limit: u64,
    ) -> ModelResult<Vec<Self>> {
        Ok(Entity::find()
            .filter(Column::UserId.eq(user_id))
            .order_by_desc(Column::OccurredAt)
            .order_by_desc(Column::Id)
            .limit(limit)
            .all(db)
            .await?)
    }

    /// Drops entries older than `days`, returning how many went.
    ///
    /// # Errors
    /// Returns an error on DB failure.
    pub async fn delete_older_than(db: &DatabaseConnection, days: i64) -> ModelResult<u64> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(days);
        let res = Entity::delete_many()
            .filter(Column::OccurredAt.lt(cutoff))
            .exec(db)
            .await?;
        Ok(res.rows_affected)
    }
}
