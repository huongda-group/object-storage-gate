# Go-live hardening — design

Ngày: 2026-08-17. Nguồn: kiểm định toàn source ngày 2026-08-17 (74 phát hiện, 9 blocker).

Tài liệu này chốt các quyết định thiết kế cho nhánh "siết phần đã có". Mọi plan
trong `docs/superpowers/plans/2026-08-17-*.md` đều tham chiếu về đây.

---

## 1. Bối cảnh

Rà toàn bộ source cho câu hỏi "go live được chưa" trả lời: chưa. Hai nhóm lý do
tách bạch nhau.

**Nhóm A — thứ định bán chưa được viết.** `src/controllers/` chỉ có `auth.rs` và
`api.rs`. Không route S3 nào, không SigV4, không HTTP client trong `Cargo.toml`,
không quota, không audit log, không prefix rewrite. `README.md:47` đã ghi thẳng
điều này. Slice #2–#6 chưa có cả spec lẫn plan.

**Nhóm B — phần đã có còn 9 blocker.** Nặng nhất: đăng ký mở tự do cộng với
`login` không kiểm email đã verify cộng với `max_bytes = 0` nghĩa là không giới
hạn — ba thứ ghép lại thành một chuỗi ba request để người lạ có credential S3
quota vô hạn.

Spec này giải quyết nhóm B. Nhóm A cần spec riêng cho gateway, chưa viết.

---

## 2. Quyết định đã chốt

### 2.1 Xoá hoàn toàn chức năng tự đăng ký

Không invite công khai, không allowlist domain, không "đăng ký rồi chờ duyệt".
Endpoint `POST /api/auth/register` biến mất khỏi bảng route.

**Đường duy nhất để có user:**

1. **User đầu tiên** — `POST /api/auth/setup`, chỉ chạy được khi DB chưa có user
   nào. Giữ nguyên cơ chế hiện tại.
2. **Mọi user sau đó** — admin tạo qua `POST /api/admin/users`, hoặc qua màn
   admin trên console (gọi đúng endpoint đó).

**Vì sao:** với một cổng lưu trữ bán theo tenant, tài khoản là đơn vị tính tiền
và đơn vị cách ly. Tài khoản chỉ nên xuất hiện khi có người chịu trách nhiệm tạo
ra nó. Bỏ đăng ký cũng xoá luôn ba bề mặt tấn công cùng lúc: brute-force tạo tài
khoản, spam mail, và enumeration qua thông báo trùng email.

### 2.2 Admin đặt mật khẩu tạm, user đổi ở lần đăng nhập đầu

Admin nhập `email`, `name`, `password`, `max_bytes`, `role`. Hệ thống tạo user
với cờ `must_change_password = true`.

Lần đăng nhập đầu tiên vẫn trả JWT bình thường, kèm cờ. Console thấy cờ thì ép
qua màn đổi mật khẩu. Phía server, extractor `Caller` từ chối mọi endpoint khác
với `403 password_change_required` cho tới khi user đổi xong.

**Vì sao chọn cách này thay vì invite token qua mail:** SMTP đang chết trên
production (`SMTP_ENABLE` default `false`, và block mailer production không có
mục `auth:` nên không cấu hình được SES/SendGrid/Mailgun). Một luồng tạo user
phụ thuộc mail là một luồng tạo user không chạy được. Mật khẩu tạm đi qua kênh
mà admin đang dùng để nói chuyện với user rồi.

### 2.3 Xoá hết luồng mail

Bỏ: `POST /api/auth/register`, `GET /api/auth/verify/{token}`,
`POST /api/auth/resend-verification-mail`, `POST /api/auth/magic-link`,
`GET /api/auth/magic-link/{token}`, `POST /api/auth/forgot`,
`POST /api/auth/reset`.

Bỏ luôn `src/mailers/` và toàn bộ template `.t`.

**Thay thế:** user quên mật khẩu thì admin đặt lại qua
`POST /api/admin/users/{pid}/password`, đúng cơ chế mật khẩu tạm ở 2.2.

**Vì sao:** ba lý do cộng lại. Mail flows là nguồn của ba finding riêng biệt
(reset token không hết hạn, verification token không bao giờ bị xoá, ba endpoint
gửi mail không rate limit thành máy phát spam). Chúng phụ thuộc SMTP đang chết.
Và với mô hình admin-tạo-user, verify email là thừa — admin đã xác nhận địa chỉ
khi gõ nó vào.

**Đánh đổi được chấp nhận:** admin gánh việc reset mật khẩu. Chấp nhận được ở
quy mô hiện tại; nếu số user lên hàng nghìn thì dựng lại luồng self-service reset
là một slice riêng, và lúc đó SMTP phải được sửa trước.

### 2.4 `max_bytes` phải khai báo tường minh

Giữ nguyên semantics `0 = không giới hạn` (`users.rs:24`) — nó đúng cho admin và
cho system pool. Nhưng `POST /api/admin/users` **bắt buộc** truyền `max_bytes`,
không có serde default. Admin muốn cho ai unlimited thì phải gõ `0` bằng tay.

**Vì sao:** rủi ro thật không nằm ở giá trị `0`, nó nằm ở chỗ `0` là thứ bạn
nhận được khi không ai nghĩ tới. Bắt khai báo tường minh giết đúng cái đó mà
không phải đổi semantics đang dùng ở ba chỗ khác.

---

## 3. Thay đổi bề mặt API

### Biến mất

| Route | Thay bằng |
|---|---|
| `POST /api/auth/register` | `POST /api/admin/users` |
| `GET /api/auth/verify/{token}` | — (admin đã xác nhận email) |
| `POST /api/auth/resend-verification-mail` | — |
| `POST /api/auth/magic-link` | — |
| `GET /api/auth/magic-link/{token}` | — |
| `POST /api/auth/forgot` | `POST /api/admin/users/{pid}/password` |
| `POST /api/auth/reset` | `POST /api/me/password` |

### Giữ nguyên

`GET /api/auth/setup`, `POST /api/auth/setup`, `POST /api/auth/login`,
`GET /api/auth/current`, và toàn bộ `/api/keys*`, `/api/buckets`, `/api/usage`,
`/api/token*`.

### Mới

| Route | Quyền | Thân |
|---|---|---|
| `GET /api/admin/users` | admin | — |
| `POST /api/admin/users` | admin | `{email, name, password, role, max_bytes}` |
| `GET /api/admin/users/{pid}` | admin | — |
| `PATCH /api/admin/users/{pid}` | admin | `{name?, role?, max_bytes?}` |
| `POST /api/admin/users/{pid}/password` | admin | `{password}` |
| `DELETE /api/admin/users/{pid}` | admin | — |
| `POST /api/me/password` | user | `{current_password, new_password}` |

---

## 4. Thay đổi schema

Migration `m20260817_000001_auth_teardown`:

**Xoá cột** khỏi `users`: `email_verification_token`,
`email_verification_sent_at`, `email_verified_at`, `magic_link_token`,
`magic_link_expiration`, `reset_token`, `reset_sent_at`.

**Thêm cột** vào `users`: `must_change_password` — boolean, NOT NULL,
default `false`.

Sau migration phải chạy `cargo loco db entities` **đối với Postgres** (ràng buộc
CLAUDE.md: `_entities/` sinh từ Postgres, chạy trên MySQL/SQLite sẽ ra kiểu cột
khác và làm hỏng model).

`LoginResponse.is_verified` biến mất, thay bằng `must_change_password`.
`CurrentResponse` cũng gánh thêm `must_change_password`.

---

## 5. Ràng buộc toàn dự án (áp cho mọi plan)

Chép nguyên từ `CLAUDE.md`, mọi task đều chịu:

- **Ba backend hạng nhất: Postgres, MySQL >= 8.0.13, SQLite.** Mọi query mới
  phải chạy được trên cả ba. Cấm `ILIKE`, `RETURNING`, `ON CONFLICT` /
  `ON DUPLICATE KEY`, `jsonb`, cột array, `pg_advisory_lock`,
  `SELECT ... FOR UPDATE SKIP LOCKED`.
- **Migration dùng `ColType` + `SchemaManager` trước.** Raw SQL chỉ khi không
  tránh được, và phải branch theo `m.get_database_backend()` — xem
  `migration/src/m20260724_000002_buckets.rs`.
- **Cột `TIMESTAMP` mới phải khai precision tường minh trên MySQL** (`TIMESTAMP(6)`).
  `m20260815_000001` chỉ widen những cột đã tồn tại lúc nó chạy.
- **Quota mutation không lấy lock.** `reserve`/`commit`/`release` là một
  `UPDATE ... WHERE <guard>` cộng kiểm `rows_affected`.
- **`src/models/_entities/` là generated.** Không sửa tay. Sinh từ Postgres.
- **SQLite một writer duy nhất.** Đường ghi phải chịu được `SQLITE_BUSY`.
  WAL + `busy_timeout=5000` đã do `loco_rs::db::connect` đặt, đừng đặt lại.
- **Comment trong code: tiếng Anh, một câu một dòng.** Không xuống dòng giữa câu.
- **Không tự commit hay push.** Plan có bước commit; người thực thi bấm, không
  phải agent tự bấm ngoài phạm vi task.
- **Không có AI attribution trong git.** Không trailer `Co-Authored-By: Claude`,
  không footer "Generated with Claude Code".

---

## 6. Cái nằm ngoài spec này

Slice #2–#6 (SigV4, route S3, proxy tới backend store, prefix rewrite, multipart,
audit log) **chưa có spec**. Chúng cần một tài liệu thiết kế riêng, và tài liệu
đó phải chốt ít nhất các câu hỏi sau trước khi viết được plan TDD:

1. **Layout key vật lý.** `FUTURE.md:44` ghi `main-bucket/tenants/{tenant-id}/...`,
   `README.md:17` ghi `physical-bucket/{user_pid}/{bucket_name}/{object_key}`, và
   `docs/superpowers/specs/2026-07-24-data-foundation-design.md:14` xác nhận cái
   sau thay thế cái trước. Cần chốt dứt điểm ở một chỗ.
2. **Streaming hay buffer.** Một PUT 5 GiB không được vào RAM. Chọn client
   (`reqwest` streaming body vs `aws-sdk-s3`) là hệ quả của quyết định này.
3. **Nguồn sự thật cho ETag.** Trả ETag của upstream hay tự tính? Ảnh hưởng tới
   multipart và tới `tests/s3/test_wire.py`.
4. **Redis: bắt buộc hay tuỳ chọn.** CLAUDE.md ghi rõ chuyển `workers.mode` sang
   `BackgroundQueue` ép các deploy MySQL phải dùng Redis queue. Cần quyết định
   trước khi audit log và reconcile task lên.
5. **Versioning.** `objects.put_object` hiện là overwrite. Bật versioning sau này
   là đổi schema, nên cần biết bây giờ.

Xem `docs/superpowers/plans/2026-08-17-go-live-roadmap.md` mục "Giai đoạn 6–7".
