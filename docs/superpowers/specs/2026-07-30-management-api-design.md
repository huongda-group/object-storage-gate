# Management API + Access Key API — thiết kế

**Ngày:** 2026-07-30
**Slice:** #7 của Object Storage Gate.
**Phạm vi:** một cây route `/api/*` (không version prefix) nhận cả JWT console lẫn PAT của service ngoài, API access-key đầy đủ, và trang `/api` trong console. Không có SigV4, không có proxy S3, không có bucket create/delete qua API — các slice khác.

---

## 1. Vấn đề

Frontend `/keys` và `/keys/:pid` đã dựng xong nhưng chạy trên mock (`frontend/src/lib/mock.ts`, các marker `TODO(slice#7)`). Backend chỉ có `src/controllers/auth.rs`. Service ngoài chưa có đường nào để tự lấy access key hay đọc quota.

Cần:
1. API access-key thật để console thay mock.
2. Đường cho service ngoài (CI, backend nội bộ, hệ monitoring) gọi bằng token dài hạn, không cần đăng nhập bằng mật khẩu.
3. Trang trong console để lấy token và tra endpoint.

## 2. Ba tầng auth — không lẫn nhau

Hệ này có ba loại credential, mỗi loại một mục đích:

| Tầng | Credential | Extractor | Dùng cho |
|---|---|---|---|
| Console | JWT (`osg_token` trong localStorage) | `auth::JWT` | Frontend, hết hạn theo config |
| Service ngoài | PAT = `users.api_key`, header `Authorization: Bearer <token>` | `auth::ApiToken<users::Model>` | CI, backend nội bộ, monitoring |
| Data plane (S3) | `access_keys.access_key_id` + secret, SigV4 | *chưa có — slice #3* | Client S3 (aws-cli, boto3, rclone) |

Hai loại đầu **dùng chung** một cây route `/api/*`. Extractor `Caller` thử JWT trước (verify bằng chữ ký, không đụng DB), không được thì tra PAT trong DB.

**Vì sao gộp, không tách:** ban đầu định để `/api/v1/*` chỉ nhận PAT. Bỏ version prefix thì `/api/v1/keys` đụng thẳng `/api/keys` của console, buộc phải chọn. Gộp không mất gì: console vốn đã tạo / xoay / thu hồi key được bằng JWT ở `/api/keys`, nên một cây riêng chặn JWT chưa từng chặn được năng lực nào — chỉ là hai đường tới cùng một quyền. Data plane S3 vẫn tách hoàn toàn (SigV4, không dùng `/api/*`).

### 2.1 PAT tái dùng `users.api_key` — quyết định và đánh đổi

Starter loco đã có cột `users.api_key` (sinh lúc insert, dạng `lo-<uuid>`) và `impl Authenticable for users::Model` với `find_by_api_key`. Extractor `auth::ApiToken<users::Model>` của loco chạy được ngay, không cần bảng mới, không cần migration, không cần middleware tự viết.

Đánh đổi đã cân nhắc và **chấp nhận** (phương án thay thế là bảng `api_tokens` riêng, đã loại):

- **1 token / account.** Mọi service dùng chung một token. Rotate là đổi cho tất cả cùng lúc → phải hẹn giờ deploy đồng loạt. Trang `/api` phải nói rõ điều này ngay cạnh nút Rotate.
- **Token lưu plaintext trong DB.** Đây là mặc định của loco, không phải điều slice này thêm vào, nhưng slice này biến nó thành credential toàn quyền account. Rò DB = mất account. Không hash được vì `find_by_api_key` tra bằng so sánh trực tiếp.
- **Không có `expires_at`, không có `last_used_at`.** Không phát hiện được token bị lộ; chỉ rotate mù.

Đường nâng cấp khi cần (không phá schema hiện tại): thêm bảng `api_tokens` (`pid, user_id, name, token_prefix, token_hash, expires_at, last_used_at, revoked_at`), extractor tra bằng `token_prefix` rồi so SHA-256, `users.api_key` thành legacy đọc-chỉ.

### 2.2 Quyền của PAT

Không có cột scope. PAT mang đúng quyền của user sở hữu: `users.role` (`user` / `admin`). Endpoint trong slice này đều là phạm vi account của chính chủ token, nên `role` chưa dùng tới ngoài việc trả về trong `whoami`. Endpoint admin toàn hệ thống (`/api/admin/*`) không nằm trong slice này.

### 2.3 Định dạng token mới

`POST /api/token/rotate` sinh `osg_pat_<uuid simple>` (thay vì `lo-<uuid>` của starter). Tra cứu là so khớp chính xác nên token cũ dạng `lo-…` vẫn dùng được, không cần backfill.

## 3. Endpoint

### 3.1 `/api/*` — extractor `Caller` (JWT hoặc PAT)

| Method | Path | Body | Trả về |
|---|---|---|---|
| GET | `/api/whoami` | — | `{pid, email, name, role}` — để service test kết nối |
| GET | `/api/keys` | — | `[KeyResponse]` |
| POST | `/api/keys` | `{label, permissions[], prefixes[], expires_at?}` | `CreateKeyResponse` (có `secret`) |
| GET | `/api/keys/:pid` | — | `KeyResponse` |
| PATCH | `/api/keys/:pid` | `{label?, status?, expires_at?, permissions?, prefixes?}` | `KeyResponse` |
| POST | `/api/keys/:pid/rotate` | — | `CreateKeyResponse` (key mới) |
| DELETE | `/api/keys/:pid` | — | `KeyResponse` với `status: revoked` |
| GET | `/api/buckets` | — | `[BucketResponse]` — read-only |
| GET | `/api/usage` | — | `UsageResponse` |
| GET | `/api/token` | — | `{token}` |
| POST | `/api/token/rotate` | — | `{token}` |

`GET /api/token` trả token thật cho chủ sở hữu đang đăng nhập. Không làm kiểu "reveal once": token nằm plaintext trong DB nên che một lần chỉ là hình thức, che mà vẫn tra lại được thì gây hiểu sai về mức bảo vệ. Trang `/api` hiện token sau khi bấm "Hiện".

### 3.2 Không có `/api/v1`

Không version bằng URL. Khi cần đổi breaking, xử theo thứ tự: thêm field mới (không phá client cũ) → thêm endpoint mới → cuối cùng mới tính tới version, và lúc đó là header chứ không phải path.

`POST /api/buckets`, `DELETE /api/buckets/:name`: **không** trong slice này. Bucket tạo qua console. Thêm sau nếu có nhu cầu thật.

### 3.3 Một bộ handler duy nhất

`src/controllers/api.rs` chứa `Caller` + toàn bộ handler + `routes()` (prefix `/api`). Không có tầng wrapper, không có bản sao logic — trước đây phải có vì hai cây route dùng hai extractor khác nhau.

```rust
pub struct Caller { pub user: users::Model }
// impl FromRequestParts: thử auth::JWT → fallback auth::ApiToken<users::Model>
```

Đăng ký trong `src/app.rs::routes()`.

### 3.4 Mã lỗi

| Tình huống | Mã |
|---|---|
| Không có token, JWT hỏng/hết hạn, PAT không khớp | `401` |
| Key `pid` không tồn tại **hoặc** thuộc user khác | `404` |
| Body sai (label lạ, action lạ, prefix bậy, `expires_at` quá khứ) | `400` kèm thông báo |
| `revoked` → `active` | `400` |

Key của user khác trả `404` chứ không `403`: `403` xác nhận key đó tồn tại, làm rò thông tin qua việc thăm dò pid.

## 4. Response shape

```rust
// src/views/keys.rs
pub struct KeyResponse {
    pub pid: String,
    pub access_key_id: String,
    pub label: String,
    pub status: String,              // effective_status(): active|disabled|revoked|expired
    pub expires_at: Option<DateTimeWithTimeZone>,
    pub days_until_expiry: Option<i64>,
    pub permissions: Vec<String>,
    pub prefixes: Vec<String>,
    pub created_at: DateTimeWithTimeZone,
}

pub struct CreateKeyResponse {
    #[serde(flatten)] pub key: KeyResponse,
    pub secret: String,              // duy nhất một lần, không lưu lại được
}

pub struct BucketResponse {
    pub name: String, pub max_bytes: i64, pub used_bytes: i64,
    pub object_count: i64, pub public_enabled: bool,
}

pub struct UsageResponse {
    pub used_bytes: i64, pub reserved_bytes: i64, pub max_bytes: i64,
    pub object_count: i64, pub bucket_count: i64,
}
```

`secret_encrypted` không bao giờ xuất hiện trong response — view dựng bằng tay từng field, không `#[serde(skip)]` trên entity (skip dễ bị xoá lúc chạy `db entities`).

## 5. Model — nơi chứa logic

Bổ sung `src/models/access_keys.rs`. Controller không query trực tiếp.

- `list_for_user(db, user_id) -> Vec<(Model, Vec<String>, Vec<String>)>` — 3 query: keys, toàn bộ permissions của các key id đó, toàn bộ prefixes; rồi group trong bộ nhớ. Không N+1.
- `find_by_pid_for_user(db, pid, user_id)` — điều kiện sở hữu nằm **trong** query, không kiểm tra sau khi load. Trả `EntityNotFound` cho cả hai trường hợp không có và không thuộc.
- `set_permissions(db, &[String])` — whitelist đúng 6 hằng `ACTION_*`; xoá hết rồi insert lại trong transaction.
- `set_prefixes(db, &[String])` — validate: không rỗng, không chứa `..`, không bắt đầu bằng `/`, ≤512 ký tự, tối đa 20 prefix/key. Replace trong transaction.
- `set_status(db, status)` — chỉ nhận `active` / `disabled`. Key đang `revoked` từ chối mọi thay đổi (terminal).
- `revoke(db)` — set `revoked`, không xoá row (audit sau này cần). Trả về model đã cập nhật để controller render lại, console không phải gọi thêm một vòng.
- `rotate(db)` — một transaction: tạo key mới copy `label` / `expires_at` / permissions / prefixes, key cũ set `disabled`. Trả `(Model, secret)`.
- `create_key` mở rộng chữ ký: nhận `expires_at: Option<...>`, `permissions: &[String]`, `prefixes: &[String]`; whitelist label ∈ {`primary`, `backup`, `temporary`, `ci`, `readonly`}.

Validate prefix là biên tin cậy (prefix quyết định phạm vi đọc/ghi của key), nên chạy ở model — mọi đường vào đều đi qua đây, không riêng controller.

## 6. Trang `/api` trong console

Route mới `frontend/src/routes/_app/api.tsx`. Ba khối:

1. **Token** — ô mono che sẵn, nút "Hiện" / "Copy", nút "Rotate" mở `ConfirmDangerModal` với nội dung: *"Token cũ mất hiệu lực ngay. Mọi service đang dùng token này sẽ nhận 401 cho tới khi cập nhật config."* Kèm dòng xám: chỉ có một token cho cả account.
2. **Endpoint** — bảng method / path / mô tả cho toàn bộ `/api/*`, mỗi dòng mở được snippet `curl` copy-được, token đã chèn sẵn nếu đang hiện.
3. **Kiểm tra kết nối** — nút gọi `GET /api/whoami` bằng chính PAT (không phải JWT session của trang), hiện kết quả thô. Chứng minh token dùng được thật, không chỉ nhìn thấy chuỗi.

Không có tab snippet S3 (aws-cli / boto3 / rclone) — chờ SigV4 ở slice #3, viết bây giờ là hướng dẫn người ta cấu hình một endpoint chưa tồn tại.

Sidebar: nhóm `STORAGE` thêm dòng "API" sau "Access Keys" (`frontend/src/components/Sidebar.tsx`).

## 7. Frontend nối dây

- `frontend/src/lib/keys.ts` — client mỏng trên `api<T>()` sẵn có ở `lib/auth.ts`: `listKeys`, `createKey`, `getKey`, `updateKey`, `rotateKey`, `revokeKey`, `getToken`, `rotateToken`, `whoami`.
- `/keys`: bỏ `KEYS` / `NEW_KEY` mock, load thật, `createKey` → `SecretRevealModal` với secret thật. Xoá các marker `TODO(slice#7)`.
- `/keys/$pid`: `PermissionMatrix` và `PrefixEditor` lưu qua `PATCH`, giữ trạng thái "có thay đổi chưa lưu" đang có.
- `frontend/src/lib/mock.ts`: xoá phần `KEYS` / `NEW_KEY` nếu không màn nào còn dùng; các mock bucket giữ nguyên (slice khác).

## 8. Test

`tests/models/access_keys.rs`:
- `rotate` giữ đúng permissions + prefixes, key cũ thành `disabled`, secret mới khác secret cũ
- `revoked` là terminal: `set_status(active)` lỗi
- `set_permissions` từ chối action lạ; `set_prefixes` từ chối `..`, từ chối đầu `/`
- `create_key` từ chối label lạ

`tests/requests/keys.rs`:
- user B gọi `GET /api/keys/:pid` của user A → `404`
- không header → `401`; PAT sai → `401`; PAT đúng → `200`
- response không chứa chuỗi `secret_encrypted`, và `GET /api/keys` không chứa field `secret`
- `POST /api/keys` rồi `GET` lại: `secret` chỉ có ở lần tạo

Dùng `serial_test` cho test đụng state dùng chung, `insta` cho snapshot response. Test DB là SQLite in-memory, seed user qua fixture sẵn có trong `tests/`; snapshot mới cần `cargo insta review`.

## 9. Ngoài phạm vi

- SigV4 verify + proxy S3 (slice #3)
- `POST/DELETE /api/buckets`
- `/api/admin/*` toàn hệ thống
- Bảng `api_tokens` (nhiều token, hash, hạn dùng, last_used_at) — xem §2.1
- Rate limit trên `/api/*`
- Audit log cho hành động qua PAT
