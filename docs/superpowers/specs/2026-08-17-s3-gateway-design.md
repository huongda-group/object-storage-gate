# S3 Gateway — Design Spec

**Ngày:** 2026-08-17
**Phạm vi:** Toàn bộ tầng dữ liệu S3 — slice #2 đến #6. SigV4, cây route S3, cách ly theo prefix, proxy tới object store, quota trên đường ghi, multipart, copy, presigned, audit log, background jobs.
**Nghiệm thu:** 61/61 test trong `tests/s3/` chạy được với `OSG_S3_TARGET=gateway`.
**Tiền đề:** Giai đoạn P1–P5 (`docs/superpowers/plans/2026-08-17-go-live-roadmap.md`) đã xong. Spec này sửa một chỗ trong P5, ghi rõ ở mục 8.

**Về kích thước:** spec này phủ năm slice. Plan sinh ra từ nó sẽ dài, và nên tách thành nhiều file plan theo ranh giới mục — schema+pool, SigV4, biên giới cách ly, verb đơn lẻ, listing, multipart+copy, audit+jobs. Mỗi file kết thúc bằng một deliverable test được. Đó là việc của `writing-plans`, không phải của spec này.

---

## 1. Bối cảnh

`src/controllers/` hiện có `auth.rs`, `api.rs`, `admin.rs`, `buckets.rs` — toàn bộ là management API. Không một route S3 nào, không SigV4, không HTTP client trong `Cargo.toml`. Sản phẩm là một S3 gateway; phần đó chưa được viết.

Cái đã có và spec này dựa lên:

- `access_keys` + `access_key_permissions` + `access_key_prefixes`: định danh và policy per-key, có rotate/disable/revoke/expire.
- `objects`: metadata một dòng một `(bucket_id, object_key)`, `varchar(1024)`, collation binary trên MySQL.
- `quota`: `reserve`/`commit`/`release`/`settle`/`reconcile`, mỗi cái một `UPDATE ... WHERE <guard>` cộng kiểm `rows_affected` — atomic trên cả ba backend, không lock.
- `crypto`: AES-256-GCM có byte version và đường đọc key cũ.
- `tests/s3/`: 61 test conformance viết bằng boto3, hiện chạy với S3 thật. Đây là **spec thực thi được** cho bề mặt wire; mọi mục dưới đây tham chiếu về test cụ thể mà nó phải làm xanh.

---

## 2. Tám quyết định đã chốt

| # | Quyết định | Lý do |
|---|---|---|
| 1 | Một spec cho cả G1 core + G2 multipart/copy/presigned + G3 audit | Chốt một lần, plan sinh ra dài nhưng không phải quay lại chốt giữa đường |
| 2 | `pools` là bảng riêng; `buckets.pool_id` FK | `user_id IS NULL` làm sentinel cho "system pool" đã sinh ra lỗi rò dữ liệu mà P3 sửa; không chồng thêm self-FK lên đúng chỗ đã cắn |
| 3 | `reqwest` + tự ký SigV4 | Stream body thật cho mọi verb; signer dùng chung `canonical_request()` với verifier mà gateway bắt buộc phải có |
| 4 | ETag lấy nguyên của upstream | Client thấy đúng cái S3 thấy, kể cả dạng multipart `-N`; không đọc lại body, không tính MD5 |
| 5 | ListObjectsV2 đọc từ bảng `objects` | Đúng ràng buộc "quota is DB-driven, never bucket-scanned"; prefix scoping thành một điều kiện `WHERE`; SQL làm phân trang |
| 6 | Multipart proxy thẳng sang upstream | Gateway stateless như FUTURE.md cam kết; không byte nào ở lại, không cần disk |
| 7 | Redis bắt buộc — `BackgroundQueue` cho audit | Audit không nằm trên đường request |
| 8 | Versioning: chỉ để chỗ trống | `objects` giữ một dòng một key, PUT đè; 0/61 test conformance liên quan |

---

## 3. Schema

Bốn migration mới, hai migration sửa bảng cũ.

### 3.1 `pools` — `m20260818_000001_pools`

```
id            PkAuto
pid           UuidUniq
name          string, unique
provider      string, default "aws"     -- aws | r2 | b2 | spaces | minio | ceph | custom
region        string null
api_endpoint  string null               -- null nghĩa là endpoint mặc định của AWS theo region
physical_bucket  string NOT NULL        -- tên bucket thật trên upstream
access_id     string null
access_secret_encrypted  blob null      -- cùng envelope AES-GCM với access_keys
created_at / updated_at
```

Sáu cột store dời từ `buckets` sang đây. `public_enabled` **không** dời — cờ đó thuộc bucket, không thuộc pool.

Model `src/models/pools.rs`: `create`, `find_by_id`, `find_by_name`, `list_all`, `set_credentials`, `decrypt_secret`. `decrypt_secret` không bao giờ được gọi từ tầng view.

### 3.2 `buckets` — `m20260818_000002_bucket_pool`

```
+ pool_id   i32 NOT NULL, FK -> pools, ON DELETE RESTRICT
- provider, region, api_endpoint, access_id, access_secret_encrypted
```

`ON DELETE RESTRICT` là cố ý: xoá một pool đang có bucket phải fail, không được im lặng làm bucket mồ côi — đúng bài học từ `ON DELETE SET NULL` mà P3 phải sửa.

**Backfill.** `pool_id` NOT NULL nên migration phải xử lý bucket đã tồn tại:

1. Nếu bảng `buckets` rỗng: thêm cột NOT NULL, xong.
2. Nếu có dòng: tạo một pool `default` với `physical_bucket = 'CHANGE-ME'` và credential rỗng, trỏ mọi bucket hiện có vào đó.

Trường hợp 2 để lại một hệ quả phải nói thẳng: **mọi request S3 sẽ trả `InternalError` cho tới khi admin điền credential cho pool `default`.** Ghi trong `docs/docker.md` như một bước vận hành bắt buộc.

### 3.3 `multipart_uploads` — `m20260818_000003_multipart_uploads`

```
id            PkAuto
pid           UuidUniq          -- chính là UploadId trả cho client
bucket_id     i32 FK -> buckets, ON DELETE CASCADE
object_key    varchar(1024)
upstream_upload_id  string
reserved_bytes  bigint default 0
created_at / updated_at
```

Index `(bucket_id, object_key)` không unique — S3 cho phép nhiều upload đang mở trên cùng một key.

**Không có bảng `multipart_parts`.** Upstream giữ part. ETag từng part do client gửi lại trong body của `CompleteMultipartUpload`. Cái duy nhất gateway cần nhớ là tổng đang giữ, nằm ở `reserved_bytes`.

### 3.4 `audit_logs` — `m20260818_000004_audit_logs`

```
id            PkAuto
pid           UuidUniq
occurred_at   TIMESTAMP(6) trên MySQL     -- cột timestamp mới, phải khai precision
user_id       i32 null                    -- auth fail chưa giải được user
access_key_id string null                 -- chuỗi client gửi, kể cả khi không tồn tại
bucket_id     i32 null
object_key    varchar(1024) null
action        string        -- read | write | delete | list | multipart | presigned | auth
outcome       string        -- ok | denied | quota_exceeded | not_found | error
status_code   i32
bytes         bigint default 0
duration_ms   i32
request_id    string
ip            string
user_agent    string null
```

Index `(occurred_at)` cho cleanup, `(user_id, occurred_at)` cho truy vấn theo tài khoản.

`occurred_at` là cột `TIMESTAMP` mới nên **phải khai `TIMESTAMP(6)` trên MySQL** — `m20260815_000001_mysql_timestamp_precision` chỉ widen những cột tồn tại lúc nó chạy. Bỏ qua là thời gian bị làm tròn tới giây và audit của hai request cách nhau 100ms trông như cùng lúc.

Không FK từ `audit_logs` sang `users`/`buckets`: audit phải sống sót khi user hoặc bucket bị xoá. Đó là mục đích của nó.

### 3.5 `objects` — chỗ trống cho versioning

```
+ version_id  varchar(64) CHARACTER SET ascii COLLATE ascii_bin NOT NULL DEFAULT ''
+ is_latest   boolean default true
unique index đổi (bucket_id, object_key) -> (bucket_id, object_key, version_id)
```

Hai chi tiết bắt buộc, cả hai đều là bẫy:

**`version_id` phải là `''`, không phải `NULL`.** Postgres và MySQL coi `NULL` là khác nhau trong unique index, nên `NULL` sẽ cho phép trùng `(bucket_id, object_key)` — đúng cái unique index đang bảo vệ. Với tất cả `''` thì index mới hành xử y hệt index cũ.

**Charset `ascii` là để index vừa trần InnoDB.** Version ID của S3 là base64url, thuần ASCII. Tính lại:

```
index (bucket_id, object_key(700), version_id)
  bucket_id            4 byte
  object_key(700)      700 × 4 = 2800 byte   (utf8mb4)
  version_id(64) ascii 64 byte
                       ------
                       2868 byte  ≤  3072    ✓
```

Nếu để `version_id` là utf8mb4 thì `64 × 4 = 256` byte, tổng `3060` — vẫn vừa nhưng sát trần, và bất kỳ thay đổi nhỏ nào sau này cũng vỡ. Dùng `ascii` giữ được prefix `object_key(700)` hiện tại **không phải đổi**, còn dư 204 byte.

Đổi unique index trên MySQL lại vướng đúng cái P3 đã gặp: InnoDB không cho drop index mà foreign key đang dựa vào. Dùng lại nguyên pattern của `m20260817_000003_column_lengths` — tạo index tạm trên `bucket_id`, drop index cũ, tạo index mới, drop index tạm.

`is_latest` chưa có code nào đọc; `list_by_prefix` sẽ thêm `AND is_latest = true` để sau bật versioning không phải sửa query.

---

## 4. Bố cục module

```
src/s3/
  mod.rs
  sigv4.rs      canonical request; verify (header + query-string); sign (upstream)
  request.rs    S3Request::resolve — auth -> authorize -> rewrite
  upstream.rs   reqwest client, streaming, map lỗi upstream
  xml.rs        dựng response XML
  error.rs      S3Error -> (status, Code, Message) -> XML

src/controllers/s3/
  mod.rs        cây route + dispatch theo query + audit
  object.rs     Get / Put / Head / Delete / DeleteObjects
  listing.rs    ListObjectsV2 / ListBuckets / HeadBucket
  multipart.rs  Create / UploadPart / UploadPartCopy / Complete / Abort / ListParts / ListMultipartUploads
  copy.rs       CopyObject

src/controllers/admin_pools.rs    CRUD pool cho admin
src/models/pools.rs
src/models/multipart_uploads.rs
src/models/audit_logs.rs
src/workers/audit.rs
src/tasks/cleanup_multipart.rs
src/tasks/cleanup_audit.rs
src/initializers/rate_limit.rs    sửa: loại trừ data plane
tests/support/mock_upstream.rs
```

`sigv4.rs` dùng chung `canonical_request()` cho cả verify (request của client) và sign (request lên upstream): cùng một thuật toán, chạy hai chiều. Đây là lý do chọn tự ký thay vì `aws-sdk-s3` — verifier là thứ bắt buộc phải có, signer thành phần dôi ra gần như miễn phí.

---

## 5. SigV4

### 5.1 Dạng header

```
Authorization: AWS4-HMAC-SHA256
  Credential=OSG…/20260817/ap-southeast-1/s3/aws4_request,
  SignedHeaders=host;x-amz-content-sha256;x-amz-date,
  Signature=…
x-amz-date: 20260817T081500Z
x-amz-content-sha256: <hex> | UNSIGNED-PAYLOAD | STREAMING-AWS4-HMAC-SHA256-PAYLOAD
```

Trình tự:

```
1. parse Authorization -> access_key_id, scope, SignedHeaders, Signature
   thiếu header và không có query creds -> AccessDenied
2. tra access_key_id     -> không thấy: InvalidAccessKeyId
3. effective_status() != active -> InvalidAccessKeyId
4. |now - x-amz-date| > 15 phút -> RequestTimeTooSkewed
5. dựng canonical request TỪ REQUEST NHƯ NHẬN ĐƯỢC — bucket/key logic, chưa rewrite
6. signing key = HMAC chain(decrypt(secret_encrypted), date, region, service)
7. so sánh constant-time -> lệch: SignatureDoesNotMatch
```

Bước 3 gộp `revoked`, `disabled`, `expired` vào một mã. Cố ý: đó là vấn đề hiệu lực của credential, không phải vấn đề phân quyền, và một mã duy nhất không xác nhận cho người gọi biết key có tồn tại hay không.

Bước 5 dùng request **như nhận được**, vì client ký cái đó. Rewrite xảy ra sau, ở mục 6.

**Region không pin.** Client ký với region nào thì recompute với chính region đó — signature đã bao nó. Pin thêm chỉ tạo một cách fail nữa mà không tăng an toàn. Test `test_signature_within_the_clock_window_still_works` không phụ thuộc region.

### 5.2 `x-amz-content-sha256`

| Giá trị | Xử lý |
|---|---|
| hex digest | Đưa vào canonical request. Không đọc lại body để đối chiếu. |
| `UNSIGNED-PAYLOAD` | Đưa nguyên chuỗi vào canonical request. |
| `STREAMING-AWS4-HMAC-SHA256-PAYLOAD` | `NotImplemented` (501) |

Hai chỗ cắt có ý thức, phải ghi `ponytail:` kèm trần trong `sigv4.rs`:

**Không đối chiếu body với hash đã khai.** Signature đã ràng cái hash, nên người ngoài không sửa được cả hai mà không có secret. Client tự khai sai hash thì chỉ tự làm hỏng object của mình.
*Trần:* MITM có secret thì phát hiện được, ta thì không.
*Đường nâng cấp:* hash khi stream rồi so ở cuối — nhưng lúc đó byte đã ở upstream, nên phải đổi sang staging trước, tức là bỏ streaming. Không đáng.

**`STREAMING-…` (aws-chunked) trả 501.** Nó là định dạng wire khác: body chia frame, mỗi chunk một signature. Nó xuất hiện khi body là stdin hoặc client dùng AWS CRT. `aws s3 cp <file> s3://…` bình thường **không** dùng nó — botocore gửi hex digest thật.
*Trần:* có client thật đạp vào thì đây là việc đầu tiên phải làm tiếp.

### 5.3 Dạng query-string (presigned)

```
X-Amz-Algorithm=AWS4-HMAC-SHA256
X-Amz-Credential=KEY/20260817/region/s3/aws4_request
X-Amz-Date=20260817T081500Z
X-Amz-Expires=3600
X-Amz-SignedHeaders=host
X-Amz-Signature=…
```

Dùng chung `canonical_request()`, khác bốn điểm:

1. Payload hash luôn là `UNSIGNED-PAYLOAD`.
2. Canonical query string bỏ `X-Amz-Signature`.
3. Hết hạn khi `X-Amz-Date + X-Amz-Expires < now` → `AccessDenied`.
4. Key phải có quyền `presigned`.

Test: `test_presigned_get_serves_the_object_without_credentials`, `test_presigned_put_accepts_an_upload`, `test_presigned_url_for_one_key_does_not_open_another`, `test_tampered_signature_is_rejected`, `test_expired_presigned_url_is_refused`.

---

## 6. `S3Request` — biên giới cách ly

Đây là mục quan trọng nhất của spec. 13 test `test_scoping.py` tồn tại chỉ để bắt lỗi ở đây.

```rust
pub struct S3Request {
    pub key: access_keys::Model,
    pub user: users::Model,
    pub bucket: buckets::Model,
    pub pool: pools::Model,
    pub logical_key: String,    // client hỏi cái gì
    pub physical_key: String,   // {user_pid}/{bucket_name}/{logical_key}
}

impl S3Request {
    /// Verb có key: chạy đủ bước 1–7.
    pub async fn resolve(ctx: &AppContext, parts: &Parts, action: &str)
        -> Result<Self, S3Error>;

    /// Verb không có key (ListBuckets, HeadBucket): bước 1–3, bỏ 4–7.
    /// `logical_key` và `physical_key` là chuỗi rỗng — dùng chúng là lỗi lập trình.
    pub async fn resolve_bucket_only(ctx: &AppContext, parts: &Parts)
        -> Result<Self, S3Error>;
}
```

`resolve_bucket_only` trả cùng kiểu `S3Request` với hai trường key rỗng. Đó là một chỗ nhập nhằng có ý thức: tách thành hai kiểu riêng thì mọi hàm nhận `&S3Request` phải nhân đôi. Đánh đổi được ghi trong doc-comment, và `ListBuckets`/`HeadBucket` là hai chỗ duy nhất gọi nó.

`resolve` là constructor **duy nhất**. Nó không phải axum extractor: audit phải ghi cả lần auth thất bại, mà extractor reject trước khi vào handler nên không chỗ nào thấy đủ để ghi. `resolve` được gọi đúng một lần ở đầu `dispatch()` — nhờ vậy dispatch thấy hết: auth fail, verb, kết quả, thời lượng.

Trình tự:

```
1. verify signature (mục 5) -> access_keys::Model (active) -> users::Model
2. bucket từ path -> buckets::find_by_user_and_name(db, user.id, name)
                     không thấy: NoSuchBucket
                     ^ bucket của user khác cũng rơi vào đây — đúng ý,
                       không xác nhận nó tồn tại
3. pool -> pools::find_by_id(db, bucket.pool_id)
4. validate logical_key:
     chứa segment `..` hoặc bắt đầu bằng `/` -> InvalidArgument
     dài > 1024 byte                        -> KeyTooLongError
5. authorize action: action ∈ key.permissions(db)  else AccessDenied
6. authorize prefix: khớp một prefix của key      else AccessDenied
7. physical_key = format!("{}/{}/{}", user.pid, bucket.name, logical_key)
```

### 6.1 Luật khớp prefix

Sửa đúng finding của P3 — prefix `team` hiện cho phép luôn `teamsecret/`:

```rust
fn prefix_allows(prefix: &str, key: &str) -> bool {
    key.starts_with(prefix)
        && (prefix.ends_with('/')
            || key.len() == prefix.len()
            || key.as_bytes()[prefix.len()] == b'/')
}
```

Key không có prefix nào = toàn bucket. Có ít nhất một = phải khớp một trong đó.

`validate_prefixes` không cần chặn `%` và `_` nữa: P3 đã đổi `list_by_prefix` sang so sánh khoảng, không còn `LIKE` để escape.

### 6.2 Tính chất cấu trúc

Mọi hàm trong `upstream.rs` nhận `&S3Request`. Không có `S3Request` thì không có `physical_key`, không có credential pool, không gọi được upstream. Prefix rewrite và authorize không phải một bước ai đó phải nhớ gọi — nó là điều kiện để có thứ cần dùng.

### 6.3 CopyObject — hai đầu

```rust
fn resolve_copy_source(dest: &S3Request, header: &str)
    -> Result<PhysicalRef, S3Error>;
```

Header `x-amz-copy-source: /bucket/key` hoặc `bucket/key`, url-encoded. Hàm này chạy **cùng** bước 2–7, với **cùng access key** — nên `test_scoped_key_cannot_copy_from_outside` và `test_scoped_key_cannot_copy_to_outside` bị chặn bởi cùng một đoạn code, không phải hai đoạn song song dễ lệch. Nguồn ở bucket khác của cùng user thì được; khác user thì `NoSuchBucket`.

### 6.4 Verb không có key

`ListBuckets` và `HeadBucket` bỏ bước 4–7. `HeadBucket` với key scoped vẫn trả 200 (`test_head_bucket_with_a_scoped_key`): scope giới hạn object, không giới hạn sự tồn tại của bucket.

---

## 7. Cây route và dispatch

axum không route theo query param, mà S3 chồng verb lên cùng path bằng query. Nên có một tầng dispatch trong handler — không tránh được.

```
/                     GET    ListBuckets
/{bucket}             GET    ?list-type=2 -> ListObjectsV2
                             ?uploads     -> ListMultipartUploads
                             (trống)      -> NotImplemented (ListObjects V1)
                      HEAD   HeadBucket
                      POST   ?delete      -> DeleteObjects
                      PUT    NotImplemented
                      DELETE NotImplemented
/{bucket}/{*key}      GET    ?uploadId    -> ListParts
                             (trống)      -> GetObject
                      HEAD   HeadObject
                      PUT    ?uploadId&partNumber + x-amz-copy-source -> UploadPartCopy
                             ?uploadId&partNumber                     -> UploadPart
                             x-amz-copy-source                        -> CopyObject
                             (trống)                                  -> PutObject
                      POST   ?uploads     -> CreateMultipartUpload
                             ?uploadId    -> CompleteMultipartUpload
                      DELETE ?uploadId    -> AbortMultipartUpload
                             (trống)      -> DeleteObject
```

`CreateBucket` / `DeleteBucket` trả 501 với thông điệp chỉ về console: bucket là đơn vị tính tiền, không để client tự tạo.

`ListObjects` V1 trả 501: aws-cli, boto3, rclone đều dùng V2. V1 chỉ khác tên token phân trang nên thêm được, nhưng chưa ai cần.

**Cây route S3 phải đăng ký sau cùng** trong `App::routes()`. `/{bucket}/{*key}` khớp gần như mọi thứ, nên nó phải nằm sau `/api/*`, sau static middleware của `frontend/dist`, và sau `/_health`. Một lỗi thứ tự ở đây làm console không load được.

---

## 8. Quota trên đường ghi — và một chỗ sửa P5

`objects::Model::put_object` mà P5 ship **tự ôm quota**: reserve → ghi row → commit. Gateway cần upload upstream **nằm giữa** reserve và commit. Gọi cả hai là tính tiền hai lần.

Tách entry point, giữ đúng một cơ chế:

```rust
// src/models/objects.rs
pub async fn put_object(...) -> ModelResult<Model>;      // giữ nguyên, cho caller cục bộ và test
pub async fn begin_put(db, bucket_id, key, size) -> ModelResult<PendingPut>;

pub struct PendingPut {
    bucket_id: i32,
    object_key: String,
    size: i64,
    reservation: Option<quota::Reservation>,   // None khi delta <= 0
    delta_bytes: i64,
    delta_objects: i64,
}

impl PendingPut {
    pub async fn commit(self, db, etag: &str, content_type: &str) -> ModelResult<Model>;
    pub async fn abort(self, db) -> ModelResult<()>;
}

/// Ghi metadata thuần, KHÔNG đụng quota. Chỉ đường multipart được dùng.
pub async fn record_put(db, bucket_id, key, size, etag, content_type) -> ModelResult<Model>;
```

`put_object` thành `begin_put` rồi commit ngay. Gateway thì `begin_put` → upstream → `commit(etag)` hoặc `abort()`. Một cơ chế quota, hai entry point, không đường nào tính hai lần.

`record_put` là ngoại lệ, và nó là một sự đánh đổi phải nói thẳng: **nó là một đường ghi không an toàn về quota.** Multipart bắt buộc cần nó, vì reservation của multipart được cộng dồn qua nhiều request `UploadPart` — không có `PendingPut` nào ôm được nó. Nên `CompleteMultipartUpload` tự sở hữu việc kế toán: nó gọi `record_put` rồi tự `quota::commit` và `quota::release` phần dư (mục 10).

Giảm thiểu: doc-comment của `record_put` ghi rõ chỉ multipart được gọi, và request test khẳng định `objects.used_bytes` sau một vòng multipart đúng bằng size cuối. Không có cách nào ép bằng kiểu dữ liệu ở đây mà không dựng thêm một guard type cho riêng multipart — không đáng.

### 8.1 PutObject

```
1. Content-Length thiếu -> MissingContentLength (411)
   (cần size để reserve; aws-chunked đã 501 ở mục 5.2)
2. begin_put(bucket_id, logical_key, len)  -> hết chỗ: QuotaExceeded (403)
3. stream body -> upstream PUT(pool, physical_key), signed
   chuyển tiếp: Content-Type, Cache-Control, Content-Disposition,
                Content-Encoding, x-amz-meta-*
4. upstream 2xx -> pending.commit(db, upstream_etag, content_type)
   upstream lỗi  -> pending.abort(db) rồi map lỗi
```

`reqwest::Body::wrap_stream` trên body của axum: không byte nào vào RAM. Một PUT 5 GiB đi qua với bộ nhớ hằng số.

### 8.2 GetObject

Proxy thuần. Chuyển tiếp lên upstream: `Range`, `If-None-Match`, `If-Match`, `If-Modified-Since`, `If-Unmodified-Since`. Trả nguyên status (200 / 206 / 304 / 412) và header `Content-Length`, `Content-Type`, `ETag`, `Last-Modified`, `Content-Range`, `Accept-Ranges`, `x-amz-meta-*`.

**Không đọc `objects` cho GET.** Body và header đến từ upstream; row `objects` là metadata cho listing và quota, không phải nguồn sự thật cho nội dung.

Test: `test_get_range_returns_partial_content`, `test_get_range_suffix_reads_the_tail`, `test_get_if_none_match_on_current_etag_is_304`, `test_get_if_modified_since_in_the_future_is_304`.

### 8.3 DeleteObject và DeleteObjects

`DeleteObject`: proxy DELETE (idempotent, 204) rồi `objects::delete` — hàm đó đã trả byte về quota từ P5. Test `test_delete_object_is_idempotent`.

`DeleteObjects` (`POST ?delete`):

```
parse XML: danh sách <Object><Key>, cờ <Quiet>
> 1000 key -> MalformedXML
authorize TỪNG key theo prefix policy
  key ngoài phạm vi -> một entry <Error><Code>AccessDenied</Code> trong response,
                       KHÔNG phải 403 cho cả request (đó là semantics batch của S3)
một lệnh DeleteObjects lên upstream với các key đã rewrite
cập nhật metadata + quota từng key đã xoá
<Quiet>true</Quiet> -> bỏ danh sách <Deleted>
```

Test: `test_delete_objects_verbose_reports_every_key`, `test_delete_objects_quiet_omits_the_deleted_list`.

### 8.4 Quota vượt → mã lỗi

`403` với `<Code>QuotaExceeded</Code>`. S3 không có mã chuẩn cho việc này. Đây là **mã phi chuẩn duy nhất** gateway phát ra; đánh dấu `gateway_only` trong `tests/s3/` và ghi trong `tests/s3/README.md`.

---

## 9. ListObjectsV2 — đọc từ DB

```sql
SELECT object_key, size, etag, updated_at
FROM objects
WHERE bucket_id = :bucket_id
  AND object_key >= :prefix
  AND object_key <  :prefix_upper      -- prefix_upper_bound() của P3
  AND object_key >  :after             -- continuation-token | start-after
  AND is_latest = true
ORDER BY object_key ASC
LIMIT :max_keys + 1
```

`prefix` trống thì bỏ cả hai điều kiện biên — `prefix_upper_bound("")` trả `None`, đúng như P3 đã xử lý. `:after` trống thì bỏ điều kiện `>`; `object_key > ''` đúng với mọi key nên để lại cũng không sai, nhưng bỏ đi thì query planner dùng index gọn hơn.

- `max-keys`: default 1000, cap 1000.
- `IsTruncated`: đúng khi lấy được `max_keys + 1` dòng.
- `NextContinuationToken`: base64 của key cuối đã emit. Trông opaque như S3, và giải mã được thành `:after`.
- `KeyCount` = số `Contents` + số `CommonPrefixes`.
- Không có gì khớp thì **bỏ hẳn** thẻ `Contents` — `test_list_objects_v2_empty_prefix_has_no_contents_key`.
- `encoding-type=url`: url-encode key và prefix trong XML.

`delimiter` roll-up làm trong Rust: cắt prefix khỏi key, tìm delimiter đầu tiên; có thì emit một `CommonPrefixes` (dedup) và không emit key đó vào `Contents`.

**Prefix scoping cho list.** Key có prefix mà `prefix` yêu cầu không nằm trong một prefix nào được phép → `AccessDenied`. List với prefix trống trên key scoped bị từ chối — đúng như IAM key scoped trên S3 thật. Test: `test_scoped_key_cannot_list_the_whole_bucket`, `test_scoped_key_cannot_list_another_folder`, `test_scoped_key_can_list_its_own_folder`.

**Trôi.** Đọc từ DB nghĩa là nếu có ai ghi trực tiếp lên physical bucket, listing không thấy. `reconcile_quota` sửa counter nhưng không sinh row `objects` mới. Ghi lại như một giới hạn đã biết: gateway là đường ghi duy nhất được hỗ trợ; pool credential không nên chia sẻ cho công cụ khác.

`ListBuckets`: từ DB, bucket của user, `CreationDate` từ `created_at`. Test `test_list_buckets_returns_logical_buckets`.

`HeadBucket`: 200 nếu bucket resolve được, 404 `NoSuchBucket` nếu không. Không body.

---

## 10. Multipart

`UploadId` trả cho client là `multipart_uploads.pid` của mình, **không** phải id của upstream — không rò định danh upstream.

```
CreateMultipartUpload   POST /{b}/{k}?uploads
  authorize multipart
  upstream Create(physical_key) -> upstream_upload_id
  insert multipart_uploads
  trả <UploadId>{our pid}</UploadId>

UploadPart              PUT /{b}/{k}?uploadId=OUR&partNumber=N
  load multipart_uploads theo pid
    row.bucket_id phải bằng bucket resolve được từ path
    row.object_key phải bằng logical_key của path
    lệch một trong hai -> NoSuchUpload
    ^ đây là chỗ chặn việc dùng UploadId của bucket khác để ghi vào key khác
  Content-Length bắt buộc -> quota::reserve(len)
  proxy upstream UploadPart(upstream_upload_id, N)
  ok:  reserved_bytes += len   (UPDATE có guard)
  lỗi: quota::release(len)
  trả ETag của upstream

UploadPartCopy          PUT …?uploadId&partNumber + x-amz-copy-source
  resolve_copy_source (cùng policy)
  reserve theo size của source (từ objects)
  proxy upstream UploadPartCopy
  trả <CopyPartResult>

CompleteMultipartUpload POST /{b}/{k}?uploadId=OUR
  parse XML part list (PartNumber + ETag)
  proxy upstream Complete(upstream_upload_id, parts)
  HEAD physical object -> lấy size thật
  objects: record_put(size, upstream_etag)
  commit đúng `size`; release `reserved_bytes - size`
  xoá row multipart_uploads

AbortMultipartUpload    DELETE …?uploadId=OUR
  proxy upstream Abort
  quota::release(reserved_bytes)
  xoá row

ListParts               GET /{b}/{k}?uploadId=OUR   proxy thuần
ListMultipartUploads    GET /{b}?uploads            từ bảng, lọc prefix + policy
```

Complete cần một `HEAD` thêm lên upstream vì response của Complete không mang size. Một round trip nữa — chấp nhận, và nói rõ thay vì đoán size từ tổng part.

Client upload lại part 3 hai lần thì cả hai đều reserve, nên `reserved_bytes` đếm dư. Complete trả phần dư về. Ghi trong doc-comment.

`test_list_multipart_uploads_filters_by_prefix` là test đã biết fail trên MinIO (`tests/s3/README.md:149`) — không phải lỗi gateway.

Test còn lại: `test_multipart_round_trip`, `test_abort_discards_the_upload_and_writes_nothing`, `test_non_final_part_below_the_minimum_is_rejected_at_complete`, `test_complete_with_a_wrong_part_etag_is_invalid_part`, `test_upload_part_copy_takes_a_part_from_an_existing_object`, `test_scoped_key_cannot_start_a_multipart_upload_outside`.

---

## 11. CopyObject

```
resolve source (cùng key policy, cùng đoạn code với dest — mục 6.3)
self-copy mà không có x-amz-metadata-directive: REPLACE -> InvalidRequest
source không có -> NoSuchKey
begin_put ở dest (size source từ objects, trừ size dest hiện có)
proxy upstream CopyObject, chuyển x-amz-metadata-directive
commit
response: <CopyObjectResult><ETag/><LastModified/></CopyObjectResult>
```

Test: `test_copy_in_bucket_keeps_bytes_and_etag`, `test_copy_defaults_to_carrying_metadata_over`, `test_copy_with_replace_directive_swaps_metadata`, `test_copy_onto_itself_without_replace_is_rejected`, `test_copy_from_missing_source_is_no_such_key`, `test_copy_does_not_remove_the_source`, `test_scoped_key_can_copy_within_its_folder`.

---

## 12. Wire và bề mặt lỗi

```xml
<?xml version="1.0" encoding="UTF-8"?>
<Error>
  <Code>NoSuchKey</Code>
  <Message>The specified key does not exist.</Message>
  <Resource>/media-cdn/photos/a.jpg</Resource>
  <RequestId>e48cf880-…</RequestId>
</Error>
```

HEAD không bao giờ có body — `test_head_missing_key_is_bare_404`, `test_head_reports_length_and_etag_without_a_body`.

`x-amz-request-id` trên mọi response, cùng giá trị với `x-request-id` mà loco sinh.

| Code | Status | Khi nào |
|---|---|---|
| `AccessDenied` | 403 | không ký, thiếu quyền, ngoài prefix, presigned hết hạn |
| `InvalidAccessKeyId` | 403 | key không tồn tại hoặc không active |
| `SignatureDoesNotMatch` | 403 | signature lệch |
| `RequestTimeTooSkewed` | 403 | lệch giờ quá ±15 phút |
| `QuotaExceeded` | 403 | phi chuẩn, chỉ gateway phát |
| `NoSuchBucket` | 404 | bucket không thuộc user của key |
| `NoSuchKey` | 404 | object không có |
| `NoSuchUpload` | 404 | uploadId lạ |
| `KeyTooLongError` | 400 | key > 1024 byte |
| `InvalidArgument` | 400 | param sai, key chứa `..` |
| `InvalidRequest` | 400 | self-copy không có REPLACE |
| `MalformedXML` | 400 | body XML sai, hoặc > 1000 key |
| `MissingContentLength` | 411 | PUT không có Content-Length |
| `EntityTooSmall` / `InvalidPart` / `InvalidPartOrder` | 400 | từ upstream lúc Complete |
| `PreconditionFailed` | 412 | If-Match không thoả |
| `NotImplemented` | 501 | ListObjects V1, CreateBucket, aws-chunked |
| `InternalError` | 500 | upstream 5xx, lỗi DB, pool thiếu credential |

**Lỗi upstream phải parse rồi phát lại, không forward nguyên body.** Body lỗi của upstream chứa tên physical bucket và physical key — forward nguyên là rò layout thật cho client, đúng cái sản phẩm cam kết không bao giờ để lộ. Nên: parse `<Code>` của upstream, phát lại `<Error>` của mình với `Resource` logic và `RequestId` của mình.

Status codes chính xác (`test_verb_status_codes_are_exact`): PUT 200 · GET 200 · HEAD 200 · DELETE 204 · `POST ?delete` 200 · Create 200 · UploadPart 200 · Complete 200 · Abort 204.

Test còn lại: `test_error_response_is_s3_shaped_xml`, `test_responses_carry_a_request_id_header`, `test_list_objects_v2_xml_is_s3_shaped`.

---

## 13. Audit

Ghi qua queue, không nằm trên đường request:

```
dispatch()
  t0 = Instant::now()
  result = S3Request::resolve(...) -> verb handler
  entry = AuditEntry { action, outcome, status_code, bytes, duration_ms,
                       user_id?, access_key_id?, bucket_id?, object_key?,
                       request_id, ip, user_agent }
  AuditWorker::perform_later(&ctx, entry)     // Redis queue, không chờ DB
  return result
```

`outcome` tách khỏi `status_code` để trả lời được "key nào bị từ chối nhiều nhất" mà không phải parse status: `ok` / `denied` / `quota_exceeded` / `not_found` / `error`.

Auth thất bại cũng ghi, với `user_id = NULL` và `access_key_id` là chuỗi client gửi — kể cả khi key đó không tồn tại. Đó là dữ liệu cần để phát hiện dò key.

`ip` lấy từ `X-Forwarded-For` khi `RATE_LIMIT_TRUST_PROXY=true`, ngược lại từ socket. Dùng lại đúng quyết định của P2: header do client gửi nên chỉ tin khi có proxy mình kiểm soát.

Config production:

```yaml
workers:
  mode: BackgroundQueue
queue:
  kind: Redis
  uri: "{{ get_env(name='REDIS_URL') }}"
```

Kéo theo:

- `CLAUDE.md`: dòng "loco has no `bg_mysql`" thành ràng buộc thật — deploy MySQL từ giờ **bắt buộc** có Redis.
- `docker-compose.yml`: lấy lại service Valkey mà P2 gỡ. P2 gỡ đúng ở thời điểm đó (không code nào đọc `REDIS_URL`); giờ có code đọc.

---

## 14. Rate limit phải loại trừ data plane

Đây là trần `ponytail:` ghi trong `src/initializers/rate_limit.rs` từ P2, giờ đến hạn.

Layer governor hiện áp toàn router. Path S3 là `/{bucket}/{key}`, tức là *mọi thứ* — nên một multipart upload hợp lệ sẽ đạp vào giới hạn 60 request/phút.

Sửa: một layer mỏng bọc governor, chỉ gọi nó khi `path.starts_with("/api")`, còn lại pass through. Khoảng 30 dòng.

Data plane S3 **không** rate limit ở tầng app. Chỗ đó thuộc quota (đã có) và thuộc reverse proxy (đã ghi trong `docs/docker.md`).

---

## 15. Background jobs

| Task | Việc | Env |
|---|---|---|
| `reconcile_quota` | đã có từ P5 | — |
| `cleanup_multipart` | abort upload upstream cũ hơn N ngày, release phần giữ, xoá row | `OSG_MULTIPART_TTL_DAYS`, default 7 |
| `cleanup_audit` | xoá `audit_logs` cũ hơn N ngày | `OSG_AUDIT_RETENTION_DAYS`, default 90 |

Không có task expire access key: `is_expired()` đã suy ra lúc đọc, một task ghi thêm cột chỉ là dữ liệu trùng. FUTURE.md liệt kê nó, nhưng nó không cần tồn tại.

---

## 16. Config và biến môi trường

Mới:

| Biến | Default | Việc |
|---|---|---|
| `REDIS_URL` | — bắt buộc ở production | queue cho audit |
| `OSG_UPSTREAM_TIMEOUT_MS` | `30000` | timeout cho request điều khiển lên upstream (không áp cho body stream) |
| `OSG_MULTIPART_TTL_DAYS` | `7` | ngưỡng của `cleanup_multipart` |
| `OSG_AUDIT_RETENTION_DAYS` | `90` | ngưỡng của `cleanup_audit` |

Dependency mới trong `Cargo.toml`:

```toml
reqwest = { version = "0.12", default-features = false, features = ["stream", "rustls-tls"] }
hmac = "0.12"
sha2 = "0.10"
quick-xml = { version = "0.37", features = ["serialize"] }
percent-encoding = "2"
```

`default-features = false` cho `reqwest`: tránh kéo `openssl` khi `rustls` đã có sẵn qua `sea-orm`. Cùng lý do đã tắt default features của `tower_governor` ở P2.

---

## 17. Test

Ba tầng, mỗi tầng bắt một loại lỗi khác nhau.

### 17.1 Unit — SigV4 với test vector của AWS

Đối chiếu `canonical_request`, `string_to_sign`, `signing_key`, `signature` với **bộ test vector chính thức của AWS** cho SigV4.

Đây là chỗ duy nhất trong toàn dự án có đáp án đúng do người khác công bố. Không dùng là bỏ không. Một `canonical_request` sai một dấu newline thì mọi test tự viết cũng sai giống nhau và cùng xanh.

### 17.2 Request test với upstream giả

`tests/support/mock_upstream.rs` — một axum server nhỏ dựng trong test, **ghi lại nó nhận được gì** và trả response đặt trước. Không mạng, không credential.

Đây là tầng bắt hầu hết lỗi, và điểm mạnh nhất của nó là khẳng định được điều này:

```
key scoped 'img/' gọi GET /media-cdn/docs/x
  -> mock upstream KHÔNG nhận request nào
  -> client nhận AccessDenied
```

Test "upstream không nhận gì" mạnh hơn test "client nhận 403": nó bắt được cả trường hợp gateway đã gọi upstream rồi mới từ chối — tức là dữ liệu đã rời khỏi biên giới trước khi policy chạy.

Phải phủ, tối thiểu:

- physical key mock nhận đúng bằng `{user_pid}/{bucket_name}/{logical_key}` cho mọi verb
- key scoped: 8 verb bên ngoài prefix đều không tới upstream
- CopyObject: cả hai đầu, cả hai chiều
- quota: `begin_put` từ chối trước khi upstream nhận byte nào
- quota: upstream lỗi thì reservation được release
- multipart: Abort release đúng `reserved_bytes`
- lỗi upstream chứa physical key thì response ra client **không** chứa nó
- `ListObjectsV2`: delimiter, phân trang, `start-after`, prefix scoping — không gọi upstream lần nào

### 17.3 Conformance Python

61 test với `OSG_S3_TARGET=gateway`, upstream thật.

Hai việc phải làm trước:

1. Ghi `tests/s3/golden/upstream.json` với store thật — file đó hiện rỗng, và 4 test dựa vào nó.
2. Đánh dấu `gateway_only` cho test về `QuotaExceeded` (mã phi chuẩn) và `NotImplemented` (V1 list, aws-chunked).

CI chạy được khi chủ repo bỏ credential vào repository secrets — `docs/superpowers/specs/2026-07-29-s3-conformance-suite-design.md:150` ghi rõ đó là quyết định của chủ repo, không phải của spec này.

---

## 18. Triển khai

### 18.1 Thứ tự

Bốn migration mới, hai sửa bảng cũ. Chạy như một bước riêng trước rollout — `auto_migrate` đã tắt từ P2.

```
m20260818_000001_pools
m20260818_000002_bucket_pool         <- backfill, xem 3.2
m20260818_000003_multipart_uploads
m20260818_000004_audit_logs
m20260818_000005_object_versioning   <- đổi unique index, cần scratch index trên MySQL
```

### 18.2 Kéo theo — không để mồ côi

Spec này **phải** bật lại màn Pool trên console mà P4 cho thành `ComingSoon`, cộng `/api/admin/pools` CRUD:

```
GET    /api/admin/pools
POST   /api/admin/pools
GET    /api/admin/pools/{pid}
PATCH  /api/admin/pools/{pid}
DELETE /api/admin/pools/{pid}      -> RESTRICT nếu còn bucket
```

Lý do: không có pool thì không tạo được bucket; không có bucket thì gateway vô dụng. `POST /api/buckets` cũng phải nhận `pool_id`.

P4 cho màn Pool thành `ComingSoon` là đúng ở thời điểm đó — form đó thu credential provider rồi vứt đi khi tải lại trang. Giờ nó có backend thật.

### 18.3 Bước vận hành bắt buộc

Sau khi migrate trên một cài đặt đã có bucket: **admin phải điền credential cho pool `default`**, nếu không mọi request S3 trả `InternalError`. Ghi trong `docs/docker.md`.

---

## 19. Nằm ngoài phạm vi

Ghi ra để không ai tưởng là bỏ sót:

- **Versioning** — chỉ để chỗ trống (`version_id`, `is_latest`). Không `versionId` param, không delete marker.
- **aws-chunked (`STREAMING-AWS4-HMAC-SHA256-PAYLOAD`)** — 501. Xem 5.2.
- **Đối chiếu body với `x-amz-content-sha256`** — không làm. Xem 5.2.
- **ListObjects V1** — 501.
- **CreateBucket / DeleteBucket qua S3** — 501; bucket tạo ở console.
- **ACL, bucket policy, website, lifecycle, SSE-C, object lock, tagging** — không có trong FUTURE.md, không có trong bộ conformance.
- **Public read qua HTTP không ký** — `buckets.public_enabled` đã có cột nhưng chưa có đường phục vụ. Cần một route riêng không qua SigV4, và một quyết định về CDN. Slice sau.
- **Quota theo số lượng object và theo số multipart đang mở** — FUTURE.md liệt kê; `objects.object_count` đã đếm, nhưng chưa có cột trần. Slice sau.
- **Rate limit / bandwidth theo tenant** — FUTURE.md ghi "optional".
- **Task ghi lại secret bằng master key mới** — đường đọc hai key đã có từ P3, task chưa viết.

---

## 20. Bảng đối chiếu test

| File | Test | Mục spec |
|---|---|---|
| `test_auth.py` | 5 | 5.1 |
| `test_bucket.py` | 7 | 9 |
| `test_object_crud.py` | 14 | 8.1, 8.2, 8.3 |
| `test_wire.py` | 5 | 12 |
| `test_scoping.py` | 13 | 6 |
| `test_multipart.py` | 6 | 10 |
| `test_copy.py` | 6 | 11 |
| `test_presigned.py` | 5 | 5.3 |
| | **61** | |
