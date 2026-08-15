use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

/// `MySQL`-only: nâng mọi cột `TIMESTAMP` lên `TIMESTAMP(6)`.
///
/// `TIMESTAMP` của `MySQL` mặc định precision 0 — nó *làm tròn* tới giây, kể cả
/// làm tròn lên. Postgres `timestamptz` và `SQLite` (lưu chuỗi ISO) đều giữ phần
/// thập phân, nên cùng một dòng code sẽ cho kết quả lệch tới nửa giây trên
/// `MySQL`: `expires_at` đọc ra xa hơn lúc ghi, `days_until_expiry()` nhảy một
/// ngày, magic link hết hạn trễ hơn trần đã tính.
///
/// Duyệt `information_schema` thay vì liệt kê tay từng cột: cột timestamp nằm
/// rải khắp `users`, `access_keys`, `buckets`, `objects`... và migration sau này
/// thêm cột mới cũng vẫn đi qua đây khi chạy trên DB trắng.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        if m.get_database_backend() != DatabaseBackend::MySql {
            return Ok(());
        }
        let conn = m.get_connection();
        let rows = conn
            .query_all(Statement::from_string(
                DatabaseBackend::MySql,
                "SELECT TABLE_NAME, COLUMN_NAME, IS_NULLABLE, COLUMN_DEFAULT, EXTRA \
                 FROM information_schema.COLUMNS \
                 WHERE TABLE_SCHEMA = DATABASE() AND DATA_TYPE = 'timestamp' \
                   AND DATETIME_PRECISION = 0",
            ))
            .await?;

        for row in rows {
            let table: String = row.try_get("", "TABLE_NAME")?;
            let column: String = row.try_get("", "COLUMN_NAME")?;
            let nullable: String = row.try_get("", "IS_NULLABLE")?;
            let default: Option<String> = row.try_get("", "COLUMN_DEFAULT")?;
            let extra: String = row.try_get("", "EXTRA")?;

            let mut sql = format!("ALTER TABLE `{table}` MODIFY COLUMN `{column}` TIMESTAMP(6)");
            sql.push_str(if nullable == "YES" {
                " NULL"
            } else {
                " NOT NULL"
            });
            if let Some(default) = default {
                // CURRENT_TIMESTAMP phải khớp precision của cột, không thì MySQL
                // từ chối; giá trị hằng thì bọc nháy.
                if default.to_uppercase().starts_with("CURRENT_TIMESTAMP") {
                    sql.push_str(" DEFAULT CURRENT_TIMESTAMP(6)");
                } else {
                    sql.push_str(" DEFAULT '");
                    sql.push_str(&default.replace('\'', "''"));
                    sql.push('\'');
                }
            }
            // EXTRA gộp nhiều thứ, phần lớn không phải DDL hợp lệ (MySQL 8 nhét
            // cả "DEFAULT_GENERATED" vô đây). Chỉ mệnh đề ON UPDATE là cần giữ,
            // và nó phải khớp precision mới của cột.
            if extra.to_lowercase().contains("on update") {
                sql.push_str(" ON UPDATE CURRENT_TIMESTAMP(6)");
            }
            conn.execute_unprepared(&sql).await?;
        }
        Ok(())
    }

    async fn down(&self, _m: &SchemaManager) -> Result<(), DbErr> {
        // Hạ lại precision chỉ làm mất dữ liệu chứ không phục hồi gì — no-op.
        Ok(())
    }
}
