use sea_orm_migration::prelude::*;

/// `MySQL`-only: widen every `TIMESTAMP` column to `TIMESTAMP(6)`.
///
/// `MySQL`'s `TIMESTAMP` defaults to precision 0 — it *rounds* to the second, including rounding up.
/// Postgres `timestamptz` and `SQLite` (which stores ISO strings) both keep the fractional part, so the same line of code drifts by up to half a second on `MySQL`: `expires_at` reads later than it was written, `days_until_expiry()` jumps a day, magic links expire later than the computed ceiling.
///
/// Only columns that exist when this migration runs can be patched.
/// Migrations generated later sit below the `inject-above` marker in `lib.rs`, meaning they run *after* it, so new `TIMESTAMP` columns come back at precision 0 and the rounding bug returns.
/// A migration that adds a table or a timestamp column calls [`crate::mysql_timestamps::widen_all`] at the end of its own `up` — that is the same scan this runs, and it is what keeps `create_table`'s implicit `created_at`/`updated_at` from reintroducing the drift.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        crate::mysql_timestamps::widen_all(m).await
    }

    async fn down(&self, _m: &SchemaManager) -> Result<(), DbErr> {
        // Lowering the precision back only loses data without restoring anything — no-op.
        Ok(())
    }
}
