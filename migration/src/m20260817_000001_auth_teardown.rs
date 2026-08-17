use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// Columns that only ever served the mail-based auth flows removed in P1.
const MAIL_COLUMNS: &[&str] = &[
    "email_verification_token",
    "email_verification_sent_at",
    "email_verified_at",
    "magic_link_token",
    "magic_link_expiration",
    "reset_token",
    "reset_sent_at",
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        for col in MAIL_COLUMNS {
            remove_column(m, "users", col).await?;
        }
        add_column(
            m,
            "users",
            "must_change_password",
            ColType::BooleanWithDefault(false),
        )
        .await?;
        Ok(())
    }

    // The restored columns come back as timestamps with MySQL's default precision 0, because
    // m20260815_000001 only widened the columns that existed when it ran.
    // Acceptable: down() is an emergency exit, and nothing reads these columns any more.
    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        remove_column(m, "users", "must_change_password").await?;
        add_column(m, "users", "reset_token", ColType::StringNull).await?;
        add_column(
            m,
            "users",
            "reset_sent_at",
            ColType::TimestampWithTimeZoneNull,
        )
        .await?;
        add_column(m, "users", "email_verification_token", ColType::StringNull).await?;
        add_column(
            m,
            "users",
            "email_verification_sent_at",
            ColType::TimestampWithTimeZoneNull,
        )
        .await?;
        add_column(
            m,
            "users",
            "email_verified_at",
            ColType::TimestampWithTimeZoneNull,
        )
        .await?;
        add_column(m, "users", "magic_link_token", ColType::StringNull).await?;
        add_column(
            m,
            "users",
            "magic_link_expiration",
            ColType::TimestampWithTimeZoneNull,
        )
        .await?;
        Ok(())
    }
}
