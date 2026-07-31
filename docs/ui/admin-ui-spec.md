# Object Storage Gate — Đặc tả UI (Admin/Console)

**Ngày:** 2026-07-28
**Trạng thái backend:** slice #1 (data foundation) đã xong — bảng `users`, `buckets`, `access_keys`, `access_key_permissions`, `access_key_prefixes`, `objects`. Chỉ có auth API (JWT) là chạy được thật; API quản trị (buckets/keys/objects) **chưa có**, sẽ làm ở slice #7.
**Mục tiêu tài liệu:** đủ chi tiết để designer/frontend dev dựng UI mà không cần đọc code Rust.

---

## 1. Sản phẩm là gì

Object Storage Gate là **cổng S3-compatible** đứng trước một object store thật (S3/R2/MinIO/Wasabi/B2/Ceph). Một bucket vật lý phục vụ nhiều user; mỗi user bị cô lập bằng prefix + access key + quota riêng.

Console này **không phải** để upload/download hằng ngày (client dùng aws-cli/rclone/SDK nói S3 trực tiếp). Console là nơi:

- User tự tạo bucket, xem dung lượng đã dùng, tạo/thu hồi access key, giới hạn quyền của từng key.
- Admin quản lý user, cấp quota, xem toàn hệ thống.

Tinh thần thiết kế: **giống trang quản lý hạ tầng (Cloudflare R2 / Backblaze / DigitalOcean Spaces)** — dày đặc dữ liệu, bảng là nhân vật chính, ít hình minh hoạ, số liệu đọc được ngay.

---

## 2. Hai vai trò

| Vai trò | Cột `users.role` | Thấy gì |
|---|---|---|
| **User** (chủ storage) | `user` | Chỉ dữ liệu của chính mình: dashboard, buckets, objects, access keys, profile |
| **Admin** | `admin` | Toàn bộ mục của user **cộng** khu `/admin`: danh sách user, cấp quota, đổi role, thống kê hệ thống |

Admin **không** tự động thấy nút "xoá object của user khác" — khu admin là quản trị tài khoản/quota, không phải file browser của người khác. (Xem object của user khác: chỉ đọc, ẩn sau nút "Xem chi tiết".)

---

## 3. Sitemap

```
/login                      Đăng nhập
/register                   Đăng ký
/forgot                     Quên mật khẩu
/reset?token=               Đặt lại mật khẩu
/verify/:token              Xác thực email (trang kết quả, không có form)
/magic-link                 Đăng nhập bằng link email (tuỳ chọn, backend đã có)

/                           Dashboard (user)
/buckets                    Danh sách bucket
/buckets/new                Tạo bucket (modal hoặc trang)
/buckets/:name              Object browser trong bucket
/buckets/:name/settings     Quota + xoá bucket
/keys                       Danh sách access key
/keys/:pid                  Chi tiết key: quyền + prefix
/api                        Token + danh sách endpoint
/settings                   Hồ sơ, đổi mật khẩu

/admin                      Dashboard hệ thống          (chỉ admin)
/admin/users                Danh sách user              (chỉ admin)
/admin/users/:pid           Chi tiết user               (chỉ admin)
```

---

## 4. Khung layout chung

- **Sidebar trái cố định** (thu gọn được, ~240px): logo, nhóm *Storage* (Dashboard, Buckets, Access Keys), nhóm *Admin* (chỉ hiện khi `role = admin`), dưới cùng là user menu (email, Settings, Logout).
- **Topbar**: breadcrumb + ô search theo ngữ cảnh (trong bucket = search prefix), avatar/menu.
- **Content**: max-width ~1400px, padding 24px.
- **Toast** góc phải trên cho kết quả hành động; **inline error** trong form; **banner đỏ** cho lỗi cấp trang.
- **Responsive**: ≥1280 full; 768–1279 sidebar thu thành icon; <768 sidebar thành drawer, bảng chuyển sang dạng card xếp dọc.
- **Dark mode** bắt buộc (dev-tool, dev xài đêm nhiều). Dùng CSS variable cho màu, cả hai theme.

---

## 5. Component dùng lại (đặc tả kỹ)

### 5.1 QuotaBar
Hiển thị `used_bytes` / `max_bytes` (và `reserved_bytes` nếu >0).

- `max_bytes = 0` nghĩa là **KHÔNG GIỚI HẠN** — không vẽ thanh, hiện chữ `12.4 GiB đã dùng · Không giới hạn` kèm icon ∞.
- Có giới hạn: thanh ngang, phần `used` màu chủ đạo, phần `reserved` màu nhạt hơn có sọc (đang upload dở), nền xám.
- Ngưỡng màu: <75% xanh, 75–89% vàng, ≥90% đỏ. ≥90% thêm nhãn "Sắp đầy".
- Text dưới thanh: `12.4 GiB / 50 GiB (24.8%)`.

### 5.2 Định dạng byte
Dùng **IEC** (KiB/MiB/GiB/TiB), 1 chữ số thập phân, `0 B` khi rỗng. Tooltip hiện số byte thô kèm dấu phân cách nghìn.

### 5.3 StatusPill
| Giá trị | Màu | Nhãn |
|---|---|---|
| `active` | xanh lá | Đang hoạt động |
| `disabled` | xám | Tạm khoá |
| `revoked` | đỏ | Đã thu hồi |
| hết hạn (`expires_at` < now dù status active) | cam | Hết hạn |

### 5.4 SecretRevealModal — **quan trọng nhất**
Secret của access key **chỉ hiện đúng một lần** sau khi tạo. Modal phải:

- Tiêu đề: "Lưu secret key ngay bây giờ".
- Banner cảnh báo vàng: "Đây là lần duy nhất secret hiện ra. Đóng cửa sổ này là không lấy lại được."
- Hai ô đọc-chỉ, mỗi ô có nút Copy: `Access Key ID` (dạng `OSG3f7a…`), `Secret Access Key`.
- Secret mặc định che (`••••`), có nút con mắt để hiện.
- Nút **Tải file** → xuất `.csv` (cột `Access key ID,Secret access key`) và tuỳ chọn snippet `~/.aws/credentials`.
- Nút đóng bị **disable cho tới khi** user tick checkbox "Tôi đã lưu secret".
- Không đóng được bằng phím Esc / click nền.

### 5.5 PermissionMatrix
6 quyền, mỗi quyền một checkbox + mô tả ngắn (bảng `access_key_permissions`, có dòng = được cấp):

| action | Nhãn | Mô tả cho user |
|---|---|---|
| `read` | Đọc | GetObject, HeadObject |
| `write` | Ghi | PutObject (ghi đè object cùng key) |
| `delete` | Xoá | DeleteObject |
| `list` | Liệt kê | ListObjectsV2, HeadBucket |
| `multipart` | Upload nhiều phần | File lớn (>5 GiB) |
| `presigned` | Link ký sẵn | Tạo URL tạm cho bên thứ ba |

Preset nhanh ở đầu: **Read-only** (read+list), **Read/Write** (read+write+list+multipart), **Full** (tất cả), **Tuỳ chỉnh**. Chọn preset chỉ tick sẵn checkbox, user vẫn sửa được.

Cảnh báo mềm: tick `write` mà không tick `read` → hiện dòng chữ xám "Key này ghi được nhưng không đọc lại được — đúng ý bạn chứ?" (không chặn).

### 5.6 PrefixEditor
Danh sách chuỗi prefix (bảng `access_key_prefixes`). **Rỗng = key truy cập được toàn bộ bucket của user** — phải nói rõ bằng empty state: "Chưa giới hạn prefix — key này chạm được mọi object trong tài khoản. Thêm prefix để thu hẹp."

- Mỗi dòng: input text + nút xoá. Nút "Thêm prefix".
- Placeholder: `images/*`, `logs/2026/*`.
- Validate: không rỗng, không bắt đầu bằng `/`, ≤1024 ký tự.
- Dưới danh sách hiện preview: "Key được phép trên: `images/*`, `logs/2026/*`".

### 5.7 ConfirmDangerDialog
Dùng cho: xoá bucket, thu hồi key, xoá user. Bắt gõ đúng tên đối tượng (tên bucket / access key id / email) mới bật nút đỏ. Nêu rõ hậu quả: xoá bucket = **xoá cascade toàn bộ metadata object** trong bucket đó.

### 5.8 Bảng (Table)
Chuẩn chung: header dính (sticky), sort theo cột, phân trang server-side (mặc định 25/trang, chọn 25/50/100), skeleton 5 dòng khi loading, empty state có icon + 1 câu + nút hành động chính, row hover đổi nền, cột hành động cuối dòng dạng menu `⋯`.

---

## 6. Chi tiết từng màn hình

### 6.1 Auth (đã có backend thật)

**Login** — form giữa trang, card ~400px: email, password, nút "Đăng nhập", link "Quên mật khẩu?" và "Chưa có tài khoản? Đăng ký". Lỗi sai thông tin: banner đỏ trên form, không nói rõ sai email hay sai mật khẩu.
**Register** — name, email, password, xác nhận password. Thành công → trang "Kiểm tra email của bạn" + nút "Gửi lại email xác thực".
**Forgot / Reset** — một ô email; trang reset lấy token từ query string, hai ô password.
**Verify** — trang kết quả, hai trạng thái: thành công (tick xanh, nút "Đăng nhập"), token hỏng/hết hạn (chữ đỏ, nút gửi lại).

Ràng buộc mật khẩu (hiện tối thiểu, hiển thị checklist realtime): ≥8 ký tự.

### 6.2 Dashboard user (`/`)

Bốn stat card trên cùng: **Dung lượng dùng** (QuotaBar to, số lớn), **Số bucket**, **Tổng object**, **Access key đang hoạt động**.

Dưới đó hai cột:
- Trái: **Bucket dùng nhiều nhất** — top 5, mỗi dòng tên bucket + mini QuotaBar + số object.
- Phải: **Access key** — 3 key gần nhất, status pill, nút "Xem tất cả".

Cuối trang: khối **Kết nối nhanh** — tab `aws-cli` / `rclone` / `boto3`, mỗi tab một snippet code có nút copy, điền sẵn endpoint gateway và `region`. Chỗ access key hiện `<ACCESS_KEY_ID>` placeholder (không có secret ở đây).

Empty state (user mới, 0 bucket): thay toàn bộ bằng khối onboarding 3 bước — 1) Tạo bucket 2) Tạo access key 3) Copy lệnh kết nối.

### 6.3 Buckets (`/buckets`)

Bảng, cột:

| Cột | Nguồn | Ghi chú hiển thị |
|---|---|---|
| Tên | `buckets.name` | link vào object browser, font mono |
| Dung lượng | `used_bytes` / `max_bytes` | mini QuotaBar, `0` = "Không giới hạn" |
| Object | `object_count` | số nguyên có phân cách nghìn |
| Tạo lúc | `created_at` | ngày tương đối ("3 ngày trước"), tooltip ngày đầy đủ |
| ⋯ | | Mở, Sửa quota, Xoá |

Nút chính góc phải: **Tạo bucket**.

**Form tạo bucket** (modal):
- `name` — validate theo luật S3, kiểm ngay khi gõ: 3–63 ký tự, chỉ chữ thường/số/dấu `-` và `.`, bắt đầu & kết thúc bằng chữ hoặc số, không hai dấu chấm liền, không giống địa chỉ IP. Thêm luật của hệ thống: **trùng tên trong cùng một user là lỗi**, khác user thì không sao (server trả 409, hiện lỗi ngay dưới ô input).
- `max_bytes` — input số + dropdown đơn vị (MiB/GiB/TiB) + checkbox "Không giới hạn" (tick = gửi `0`, khoá ô số). Nếu tài khoản có `max_bytes` ≠ 0, hiện gợi ý: "Quota tài khoản còn trống: 32 GiB".

### 6.4 Object browser (`/buckets/:name`)

Trọng tâm: duyệt object theo prefix như thư mục.

- **Breadcrumb prefix**: `photos / 2026 / 07 /` — mỗi mảnh click được, mảnh cuối là hiện tại. Có nút "về gốc".
- **Ô lọc prefix** ở topbar: gõ prefix → lọc server-side (`list_by_prefix`), không phải filter phía client.
- **Bảng object**:

| Cột | Nguồn |
|---|---|
| Key | `objects.object_key` — hiện phần sau prefix hiện tại, font mono; hàng "thư mục" (prefix chung) hiện icon folder và click để đi sâu |
| Kích thước | `size` |
| Loại | `content_type` (badge nhỏ, ví dụ `image/png`) |
| ETag | `etag` — cắt 8 ký tự đầu + copy |
| Sửa lúc | `updated_at` |
| ⋯ | Copy key, Tải xuống, Xoá |

- Chọn nhiều dòng bằng checkbox → thanh hành động nổi: "Xoá N object".
- Empty state: "Bucket này chưa có object" + snippet `aws s3 cp ./file s3://<bucket>/ --endpoint-url …`.
- **Nút Upload**: đưa vào thiết kế nhưng **disable + tooltip "Sắp có"** cho tới slice #3 (chưa có S3 API để đẩy file thật).

### 6.5 Access Keys (`/keys`)

Bảng:

| Cột | Nguồn |
|---|---|
| Access Key ID | `access_key_id` — font mono, cắt giữa `OSG3f7a…9b2c`, nút copy |
| Nhãn | `label` — badge: primary / backup / temporary / ci / readonly |
| Quyền | tóm tắt từ `access_key_permissions`: hiện tối đa 3 chip + "+2" |
| Phạm vi | `access_key_prefixes`: "Toàn tài khoản" nếu rỗng, ngược lại chip prefix đầu + "+n" |
| Trạng thái | StatusPill |
| Hết hạn | `expires_at` — "—" nếu null; nếu <7 ngày hiện cam "Còn 3 ngày" |
| Tạo lúc | `created_at` |
| ⋯ | Sửa quyền, Tạm khoá / Mở lại, Xoay khoá, Thu hồi |

Nút chính: **Tạo access key** → form: `label` (dropdown 5 giá trị), preset quyền, prefix (tuỳ chọn), `expires_at` (date picker, tuỳ chọn, có preset 7/30/90 ngày). Submit → **SecretRevealModal**.

Ngữ nghĩa hành động — phải viết rõ trong dialog xác nhận:
- **Tạm khoá** (`disabled`): dừng tạm, mở lại được.
- **Thu hồi** (`revoked`): vĩnh viễn, không mở lại. Dùng ConfirmDangerDialog.
- **Xoay khoá**: tạo key mới cùng quyền/prefix, key cũ chuyển `disabled` (không xoá ngay để app đang chạy có thời gian đổi). Kết quả cũng mở SecretRevealModal, kèm nhắc: "Sau khi đổi xong config, nhớ thu hồi key cũ."

### 6.6 Chi tiết access key (`/keys/:pid`)

Trang (không modal, vì có 2 khối sửa): header là access key id + status pill + nút hành động; thân gồm PermissionMatrix và PrefixEditor, mỗi khối có nút Lưu riêng và trạng thái "có thay đổi chưa lưu". Cuối trang khối *Danger zone* viền đỏ: Thu hồi key.

Secret **không bao giờ** hiện lại ở đây — chỉ ghi dòng chữ xám: "Secret chỉ hiện một lần lúc tạo. Mất thì xoay khoá."

### 6.7 API (`/api`)

Ba khối:

1. **Token** — ô mono che sẵn, nút *Hiện* / *Copy* / *Đổi token*. Đổi token mở ConfirmDangerDialog: "Token cũ mất hiệu lực ngay. Mọi service đang dùng token này sẽ nhận 401 cho tới khi cập nhật config." Dưới ô ghi rõ: mỗi tài khoản chỉ có **một** token, đổi là đổi cho mọi service cùng lúc.
2. **Bảng endpoint** — method / path / mô tả cho toàn bộ `/api/*`; mỗi dòng bung được snippet `curl` copy-được (token chỉ chèn vào snippet khi đang ở trạng thái *Hiện*).
3. **Kiểm tra kết nối** — gọi `GET /api/whoami` bằng chính PAT (không phải JWT session), in `HTTP <code>` + body thô.

Không có snippet client S3 (aws-cli / boto3 / rclone) — chờ SigV4 (slice #3); viết trước là hướng dẫn cấu hình một endpoint chưa tồn tại.

### 6.8 Settings (`/settings`)
Hồ sơ (name, email — email đọc-chỉ), đổi mật khẩu (mật khẩu cũ + mới + xác nhận), và khối chỉ-đọc "Quota tài khoản" với QuotaBar + dòng "Cần thêm dung lượng? Liên hệ admin."

### 6.9 Admin dashboard (`/admin`)
Stat card: tổng user, tổng bucket, tổng object, tổng dung lượng dùng, tổng quota đã cấp (và tỉ lệ oversubscribe nếu tổng quota > dung lượng vật lý). Bảng "Top 10 user theo dung lượng" và bảng "User sắp đầy quota (≥90%)".

### 6.10 Admin — Users (`/admin/users`)

| Cột | Nguồn |
|---|---|
| Email | `users.email` |
| Tên | `users.name` |
| Role | badge admin/user |
| Dung lượng | `used_bytes` / `max_bytes` — mini QuotaBar |
| Bucket | đếm |
| Key | đếm (active/tổng) |
| Xác thực email | tick xanh / dấu chấm than xám (`email_verified_at`) |
| Tạo lúc | `created_at` |
| ⋯ | Xem chi tiết, Sửa quota, Đổi role |

Có ô search theo email/tên, filter theo role và theo "quota ≥90%".

**Sửa quota** (modal): input số + đơn vị + checkbox Không giới hạn. Cảnh báo nếu đặt `max_bytes` **nhỏ hơn** `used_bytes` hiện tại: "User đang dùng 40 GiB, đặt quota 20 GiB sẽ chặn mọi lần ghi mới cho tới khi họ xoá bớt." — cho phép nhưng bắt xác nhận.

**Đổi role**: dropdown; hạ chính mình từ admin xuống user thì chặn ("Không thể tự hạ quyền chính mình").

### 6.11 Admin — Chi tiết user (`/admin/users/:pid`)
Header: email, role, QuotaBar tài khoản, nút Sửa quota / Đổi role. Ba tab: **Buckets** (bảng chỉ-đọc + sửa quota từng bucket), **Access keys** (chỉ-đọc + nút tạm khoá/thu hồi khẩn cấp), **Hoạt động** (placeholder "Sắp có" — audit log là slice #6).

---

## 7. Hợp đồng API (frontend code theo cái này)

Đã có thật (loco auth):

```
POST /api/auth/register           {name,email,password}
POST /api/auth/login              {email,password} -> {token, pid, name, is_verified}
GET  /api/auth/current            Bearer -> user hiện tại
POST /api/auth/forgot             {email}
POST /api/auth/reset              {token,password}
GET  /api/auth/verify/{token}
POST /api/auth/resend-verification-mail
POST /api/auth/magic-link
```

**Chưa có — cần backend slice #7. Frontend cứ code theo shape này, tạm mock:**

```
GET    /api/me/summary                     -> {used_bytes,max_bytes,reserved_bytes,bucket_count,object_count,active_key_count}
GET    /api/buckets?page=&per_page=        -> {items:[Bucket], total}
POST   /api/buckets                        {name, max_bytes}          -> Bucket   (409 nếu trùng tên trong user)
PATCH  /api/buckets/{pid}                  {max_bytes}                -> Bucket
DELETE /api/buckets/{pid}
GET    /api/buckets/{pid}/objects?prefix=&page=&per_page=  -> {items:[Object], common_prefixes:[string], total}
DELETE /api/buckets/{pid}/objects          {keys:[string]}

GET    /api/keys                           -> {items:[AccessKey]}
POST   /api/keys                           {label, permissions:[string], prefixes:[string], expires_at?} 
                                           -> {key:AccessKey, secret:"<chỉ trả lần này>"}
GET    /api/keys/{pid}                     -> AccessKey (kèm permissions, prefixes)
PATCH  /api/keys/{pid}                     {label?, status?, permissions?, prefixes?, expires_at?}
POST   /api/keys/{pid}/rotate              -> {key:AccessKey, secret:"..."}
DELETE /api/keys/{pid}                     (= revoke)

GET    /api/admin/summary
GET    /api/admin/users?q=&role=&page=
GET    /api/admin/users/{pid}
PATCH  /api/admin/users/{pid}              {max_bytes?, role?}
```

Kiểu dữ liệu:

```ts
type Bucket = {
  pid: string; name: string;
  max_bytes: number; used_bytes: number; reserved_bytes: number; object_count: number;
  created_at: string; updated_at: string;
};
type S3Object = {
  pid: string; object_key: string; size: number; etag: string;
  content_type: string; created_at: string; updated_at: string;
};
type AccessKey = {
  pid: string; access_key_id: string; label: string;
  status: "active" | "disabled" | "revoked";
  expires_at: string | null;
  permissions: ("read"|"write"|"delete"|"list"|"multipart"|"presigned")[];
  prefixes: string[];
  created_at: string;
};
type User = {
  pid: string; name: string; email: string; role: "admin" | "user";
  max_bytes: number; used_bytes: number; reserved_bytes: number;
  email_verified_at: string | null; created_at: string;
};
```

Lưu ý dùng chung: **luôn định danh bằng `pid` (UUID) trên URL/API, không bao giờ dùng `id` số.** `*_bytes` là số nguyên byte (có thể vượt 2^53 về lý thuyết — parse an toàn, hiển thị bằng string nếu backend trả string).

Lỗi: HTTP status + body `{error: string}`. 401 → đá về `/login` và xoá token. 403 → trang "Không có quyền". 409 → lỗi inline trên field liên quan.

---

## 8. Trạng thái & chi tiết dễ bị bỏ sót

- **Loading**: skeleton, không spinner toàn trang (trừ lần boot đầu).
- **Optimistic** cho toggle status key; rollback + toast đỏ nếu server lỗi.
- **Số liệu quota là ảnh chụp**, không realtime — có nút refresh nhỏ cạnh stat card, hiện "Cập nhật lúc HH:mm".
- `reserved_bytes > 0` nghĩa là đang có upload dở → tooltip giải thích, đừng để user tưởng bug.
- **Copy**: mọi nút copy đổi icon thành tick 1.5 giây + toast nhỏ.
- **a11y**: modal có focus trap, Esc đóng (trừ SecretRevealModal), mọi icon-button có `aria-label`, tương phản ≥ 4.5:1, bảng có `<th scope>`.
- **Ngôn ngữ**: tiếng Việt là mặc định, nhưng để chuỗi trong file i18n ngay từ đầu (khách nước ngoài là chuyện sớm muộn). Tên kỹ thuật (bucket, access key, prefix, object, ETag) **giữ nguyên tiếng Anh**, đừng dịch.
- **Định dạng ngày**: tương đối cho <7 ngày, `DD/MM/YYYY HH:mm` cho cũ hơn, tooltip luôn hiện tuyệt đối kèm timezone.

---

## 9. Cái gì làm được ngay, cái gì chờ backend

| Màn hình | Làm được bây giờ | Chờ |
|---|---|---|
| Auth (login/register/forgot/reset/verify) | ✅ API thật, ráp được luôn | — |
| Layout, sidebar, theme, component chung | ✅ | — |
| Dashboard, Buckets, Keys, Admin | ✅ dựng UI + mock data theo §7 | API slice #7 |
| Object browser | ✅ dựng UI + mock | API slice #7 |
| Upload / Download object | ❌ | S3 API slice #3 |
| Audit log / Hoạt động | ❌ | slice #6 |

Đề nghị: dựng lớp `api/` với mock có thể bật/tắt bằng env (`VITE_/RSBUILD_MOCK=1`) để đổi sang API thật chỉ bằng một chỗ.

## 10. Nền kỹ thuật hiện có

`frontend/` là React 18 + rsbuild + biome, mới có mỗi trang splash (`src/LocoSplash.tsx`). Chưa có router, chưa có UI library, chưa có state manager. Build ra `frontend/dist`, server Rust serve static.

Gợi ý tối thiểu (không bắt buộc): `react-router` cho routing, TanStack Query cho fetch/cache, Tailwind cho style. Đừng thêm nhiều hơn mức cần.
