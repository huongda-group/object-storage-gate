# Multi-DB: Postgres / MySQL / SQLite — thiết kế

**Ngày:** 2026-08-15
**Phạm vi:** cho phép chạy production trên cả ba backend, ngang hàng nhau. Đụng `Cargo.toml`, `config/*.yaml`, `.env`, `migration/`, `src/app.rs`, CI, `docker-compose.yml`, tài liệu. Không đụng logic domain, không đổi schema, không sinh lại `_entities/`.

---

## 1. Vấn đề

Repo hiện chỉ compile hai driver: `sqlx-postgres` + `sqlx-sqlite` (`Cargo.toml`). Postgres là DB dev/prod, SQLite chỉ đóng vai in-memory cho test (`config/test.yaml` → `sqlite::memory:`). MySQL chưa connect được — thiếu driver hẳn.

Mục tiêu: **cả ba đều là tier 1** — chạy được production, migration chạy sạch, toàn bộ test suite xanh trên cả ba trong CI.

## 2. Những chỗ gãy đã xác minh trong code

| # | Chỗ | Gãy trên | Chi tiết |
|---|-----|----------|----------|
| B1 | `Cargo.toml` sea-orm features | MySQL | thiếu `sqlx-mysql` → không connect được |
| B2 | `migration/src/m20260724_000002_buckets.rs:35`, `m20260724_000006_objects.rs:29`, `m20260724_000004_access_key_permissions.rs:19` | MySQL | `CREATE UNIQUE INDEX IF NOT EXISTS` — MySQL không có `IF NOT EXISTS` cho index |
| B3 | `buckets.rs:35` index `COALESCE(user_id,0), name` | MySQL | functional index cần MySQL ≥ 8.0.13 và cú pháp bọc ngoặc kép `((COALESCE(user_id,0)), name)` |
| B4 | `loco_rs::db::seed` → `reset_autoincrement` (`db.rs:410`, helper `db.rs:356/398`) | MySQL | trả `Error::Message("Unsupported database backend: MySQL")`. `src/app.rs::seed` và **mọi test gọi `seed::<App>`** đều đi qua đây → CI MySQL đỏ hết |
| B5 | `loco_rs::db::get_tables` (`db.rs:766`) → `dump_tables`/`dump_schema` | MySQL | lệnh CLI dump schema không chạy. Không ảnh hưởng runtime |
| B6 | `ColType::Uuid*` (cột `pid`) | MySQL | map ra `binary(16)` thay vì kiểu `uuid` native |
| B7 | `timestamptz` của loco (`created_at`/`updated_at`) | MySQL | `TIMESTAMP` không mang timezone, trong khi `_entities` khai `DateTimeWithTimeZone` |

Đã kiểm và **không** phải vấn đề:

- SQLite pragma: `loco_rs::db::connect` (`db.rs:145`) tự chạy `WAL` + `busy_timeout=5000` + `foreign_keys=ON` + `synchronous=NORMAL`. Không phải viết gì.
- `truncate_table` (`db.rs:702`) = `delete_many()`, portable.
- `db::reset` = `Migrator::fresh`, portable.
- Phát hiện trùng email/user: `src/models/users.rs:254,299` là check-then-insert ở tầng app, không đọc mã lỗi DB → portable. Snapshot `handle_create_with_password_with_duplicate@users.snap` (`Err(EntityAlreadyExists)`) không đổi theo backend.
- `src/models/crypto.rs` dùng `ColType::BlobNull` → `bytea`/`blob`/`BLOB`, portable.

## 3. Thiết kế

### 3.1 Driver

`Cargo.toml`:

```toml
sea-orm = { version = "1.1", features = [
  "sqlx-sqlite", "sqlx-postgres", "sqlx-mysql",
  "runtime-tokio-rustls", "macros",
] }
```

`migration/Cargo.toml` giữ nguyên — driver unify từ crate app qua feature resolution.

**Ràng buộc kéo theo:** loco có `bg_pg` và `bg_sqlt`, **không có `bg_mysql`**. Hiện `workers.mode: BackgroundAsync` (in-process) nên không sao. Ngày nào đổi sang `BackgroundQueue` thì MySQL bắt buộc đi Redis queue — ghi vô CLAUDE.md.

### 3.2 Chọn DB bằng `DB_TYPE` trong `.env`

```
DB_TYPE=postgres        # postgres | mysql | sqlite
# DATABASE_URL=...      # optional; đặt vô là thắng DB_TYPE
```

`config/{development,test,production}.yaml` chọn URI mặc định bằng Tera, `DATABASE_URL` vẫn override:

```yaml
database:
{%- set db_type = get_env(name="DB_TYPE", default="postgres") %}
{%- if db_type == "sqlite" %}
  uri: {{ get_env(name="DATABASE_URL", default="sqlite://data/osg_development.sqlite?mode=rwc") }}
{%- elif db_type == "mysql" %}
  uri: {{ get_env(name="DATABASE_URL", default="mysql://loco:loco@localhost:3306/object_storage_gate_development") }}
{%- else %}
  uri: {{ get_env(name="DATABASE_URL", default="postgres://loco:loco@localhost:5432/object-storage-gate_development") }}
{%- endif %}
```

- `test.yaml`: nhánh sqlite giữ `sqlite::memory:` (không phải file).
- `production.yaml`: cả ba nhánh **không có default** — `get_env(name="DATABASE_URL")` bắt buộc, y như hiện tại. `DB_TYPE` ở prod chỉ để tài liệu hoá; URI vẫn là nguồn sự thật.
- Không thêm code Rust nào. Backend thật do scheme trong URI quyết định (sea-orm parse), `DB_TYPE` chỉ là bàn đạp chọn default cho dev — chấp nhận việc hai biến có thể lệch nhau, URI luôn thắng.

### 3.3 Migration portable

**B2 — hai index thường** (`objects`, `access_key_permissions`): bỏ SQL thô, dùng `SchemaManager`:

```rust
if !m.has_index("objects", "idx_objects_bucket_key").await? {
    m.create_index(
        Index::create()
            .name("idx_objects_bucket_key")
            .table(Objects::Table)
            .col(Objects::BucketId)
            .col(Objects::ObjectKey)
            .unique()
            .to_owned(),
    ).await?;
}
```

`has_index` thay cho `IF NOT EXISTS` — chạy đúng cả ba.

**B3 — index functional trên `buckets`**: branch theo backend, giữ nguyên schema logic (`COALESCE(user_id,0)` để hai system pool không trùng tên):

```rust
let sql = match m.get_database_backend() {
    DatabaseBackend::MySql =>
        "CREATE UNIQUE INDEX idx_buckets_owner_name ON buckets ((COALESCE(user_id, 0)), name)",
    _ =>
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_buckets_owner_name ON buckets (COALESCE(user_id, 0), name)",
};
```

Nhánh MySQL không có `IF NOT EXISTS` → phải guard bằng `m.has_index("buckets", "idx_buckets_owner_name")` trước.

**Yêu cầu tối thiểu: MySQL ≥ 8.0.13** (functional index). Ghi vô README + `docker-compose.yml` pin `mysql:8`.

`down()` không đổi — `drop_table` kéo index theo.

### 3.4 Gỡ B4 (seed trên MySQL)

`loco_rs::db::seed` insert xong hết rồi mới gọi `reset_autoincrement` và chết ở đó. Mà MySQL/InnoDB **tự** đẩy bộ đếm `AUTO_INCREMENT` lên khi insert id tường minh (fixture `src/fixtures/users.yaml` có `id: 1`...), nên bước reset đó với MySQL là thừa. Nuốt đúng lỗi đó trong `src/app.rs`:

```rust
async fn seed(ctx: &AppContext, base: &Path) -> Result<()> {
    // ponytail: loco's db::seed ends with reset_autoincrement, which hard-errors on
    // MySQL — but InnoDB advances the AUTO_INCREMENT counter on explicit-id inserts
    // by itself, so the rows are already correct when this fires. Swallow that one
    // error; upgrade path is patching loco upstream to no-op there.
    match db::seed::<users::ActiveModel>(&ctx.db, &path).await {
        Err(Error::Message(m)) if m.contains("Unsupported database backend: MySQL") => Ok(()),
        other => other,
    }
}
```

Verify bằng test: seed trên MySQL xong, tạo user mới phải nhận `id = 3` (fixture có 2 dòng), không đụng primary key.

### 3.5 `_entities/` giữ nguyên, không regen theo backend

`src/models/_entities/` là output của `db entities` chạy trên Postgres — coi đó là canonical, **không** chạy lại trên MySQL/SQLite (output sẽ khác về timestamp/uuid và làm hỏng model). Rủi ro B6/B7 để test suite chạy thật trên MySQL bắt:

- `pid: Uuid` ↔ `binary(16)`: sea-orm/sqlx mysql có encode/decode Uuid sang binary — test `can_find_by_pid` phủ.
- `DateTimeWithTimeZone` ↔ `TIMESTAMP`: đây là rủi ro lớn nhất của cả plan.

**Fallback nếu B7 vỡ:** đổi migration từ `timestamps_tz` sang timestamp naive UTC cho cả ba backend, regen `_entities` trên Postgres, sửa chỗ nào dùng `DateTimeWithTimeZone` (`src/controllers/api.rs`, `src/models/access_keys.rs` expiry). Đây là migration mới, không sửa migration cũ. Quyết định sau khi chạy thử — không làm trước.

### 3.6 Luật cho code viết sau này

Thêm mục vô `CLAUDE.md`:

- Cấm: `ILIKE`, `RETURNING`, `ON CONFLICT`/`ON DUPLICATE KEY`, `jsonb`, array column, `pg_advisory_lock`, `SELECT ... FOR UPDATE SKIP LOCKED`.
- Quota `reserve`/`commit`/`release`: một câu `UPDATE ... WHERE <guard>` rồi đọc `rows_affected`, **không** dùng lock — atomic trên cả ba backend. (Slice quota chưa viết; luật này chặn trước.)
- SQLite chỉ một writer: đường ghi phải chịu được `SQLITE_BUSY` retry; `busy_timeout=5000` đã có sẵn từ loco.
- Migration mới: `ColType` + `SchemaManager` trước; SQL thô chỉ khi bắt buộc, và phải branch đủ ba backend.

### 3.7 Test + CI

`ci.yaml` job `test` → matrix:

```yaml
strategy:
  matrix:
    db: [sqlite, postgres, mysql]
```

- services: giữ `redis` + `postgres`, thêm `mysql:8` (`MYSQL_DATABASE: mysql_test`, healthcheck `mysqladmin ping`).
- `DATABASE_URL` set theo `matrix.db`; nhánh sqlite để `sqlite::memory:`.
- Job `clippy`/`rustfmt` giữ nguyên, chạy một lần.

Test hiện đã chạy PG trong CI và SQLite ở local, nên đây là nới matrix chứ không viết test mới. Ngoại lệ: thêm một test cho §3.4 (seed + insert id kế tiếp).

Snapshot `insta`: kiểm lại sau lần chạy MySQL đầu tiên. Snapshot nào lộ kiểu dữ liệu backend (timestamp format) thì thêm filter trong `configure_insta!`, không tách snapshot theo backend.

### 3.8 Vận hành

- `docker-compose.yml`: thêm service `mysql` (profile `mysql`), Postgres giữ mặc định. SQLite không cần service — chỉ volume cho file DB.
- README: bảng ba backend, ghi rõ MySQL ≥ 8.0.13, SQLite một writer (hợp deploy 1 node), lệnh loco nào không chạy trên MySQL (B5).

## 4. Trần đã biết, không sửa trong slice này

- `object_key` là `ColType::String` = `varchar(255)`; S3 cho phép key tới 1024 byte. Hạn chế có sẵn từ trước, cả PG lẫn MySQL đều dính. Khi nào nới lên 1024: unique index `(bucket_id, object_key)` sẽ vượt trần 3072 byte của InnoDB utf8mb4 → lúc đó phải đổi sang cột hash (sha256 hex, 64 ký tự) làm khoá unique. Ghi ledger, không làm bây giờ.
- B5 (`dump_schema`/`dump_tables` không chạy trên MySQL): chỉ là lệnh CLI phụ, ghi tài liệu, không vá.
- `db entities` trên MySQL/SQLite: không hỗ trợ, `_entities` chỉ sinh từ Postgres.

## 5. Định nghĩa "xong"

1. `cargo test --all-features --all` xanh trên cả ba backend, local và CI.
2. `cargo loco db migrate` rồi `db down` (toàn bộ) chạy sạch trên cả ba.
3. Đổi `DB_TYPE` trong `.env` là boot được, không phải sửa file nào khác.
4. `cargo clippy --all-targets` sạch, `cargo fmt` sạch.
5. README + CLAUDE.md ghi đủ ràng buộc §3.6 và §4.
