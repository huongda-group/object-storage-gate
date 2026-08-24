# Go-live Roadmap

> **For agentic workers:** đây là tài liệu điều phối, không phải plan thực thi.
> Mỗi giai đoạn có plan riêng. Đọc plan của giai đoạn rồi dùng
> superpowers:subagent-driven-development hoặc superpowers:executing-plans.

**Goal:** Đưa Object Storage Gate từ "management app có 9 blocker" tới "S3 gateway phục vụ được tenant thật".

**Spec:** `docs/superpowers/specs/2026-08-17-go-live-hardening-design.md` (giai đoạn 1–5). Giai đoạn 6–7 chưa có spec.

---

## Bản đồ

| # | Giai đoạn | Plan | Chặn cái gì | Phụ thuộc |
|---|---|---|---|---|
| 1 | Xoá đăng ký + admin quản lý user | `2026-08-17-p1-auth-teardown-admin-users.md` | Blocker 1, 9 | **XONG** |
| 2 | Siết config, deploy, CI | `2026-08-17-p2-hardening-config-ops.md` | Blocker 2, 3, 4, 5 | **XONG** |
| 3 | Sửa tầng dữ liệu | `2026-08-17-p3-data-layer-correctness.md` | 7 High | **XONG** |
| 4 | Console bỏ mock, nối API thật | `2026-08-17-p4-console-real-api.md` | Blocker 7, 8 | **XONG** |
| 5 | Máy quota | `2026-08-17-p5-quota-engine.md` | Blocker 6 | **XONG** |
| G1 | Pools + ràng buộc bucket → pool | `2026-08-17-g1-pools-and-bucket-binding.md` | tiền đề cho cả gateway | 1 — **XONG** |
| G2 | SigV4 + client upstream | `2026-08-17-g2-sigv4-and-upstream-client.md` | slice #2 | G1 |
| G3 | Biên giới cách ly + đường đọc | `2026-08-17-g3-isolation-boundary-and-read-path.md` | slice #2, #3 | G2 |
| G4 | Đường ghi + quota | `2026-08-17-g4-write-path-and-quota.md` | slice #3, #4 | G3, 5 |
| G5 | Listing đọc từ DB | `2026-08-17-g5-listing-from-db.md` | slice #3 | G3 |
| G6 | Multipart, copy, presigned | `2026-08-17-g6-multipart-copy-presigned.md` | slice #5 | G4 |
| G7 | Audit, jobs, conformance | `2026-08-17-g7-audit-jobs-and-conformance.md` | slice #6 | G6 |

Giai đoạn 1, 2, 3 độc lập nhau — chạy song song được nếu có nhiều người.
Giai đoạn 4 cần API admin của giai đoạn 1. Giai đoạn 5 cần index và guard của
giai đoạn 3.

---

## Cổng nghiệm thu

**Cổng A — mở console cho người dùng nội bộ. ĐÃ ĐẠT** (giai đoạn 1–5 xong,
104 test xanh trên cả ba backend, clippy pedantic+nursery sạch).
Lúc này: không ai tự tạo được tài khoản, admin có đường quản lý user, console
không còn hiển thị số bịa, và deploy không còn năm lỗ hổng cấu hình.
Chưa phục vụ S3 — console là thứ duy nhất chạy thật.

**Cổng B — mở cho tenant thứ nhất.** Thêm giai đoạn 3, 5, 6.
Lúc này một lệnh `aws s3 cp` chạy được, quota được enforce, và cách ly prefix
có code chứ không chỉ có schema.

**Cổng C — mở cho tenant thứ hai.** Thêm giai đoạn 7.
Audit log có, multipart có, và `tests/s3/` chạy được với `OSG_S3_TARGET=gateway`
trong CI.

Không nhảy cóc Cổng B sang C. Nhiều tenant mà chưa có audit log nghĩa là khi có
sự cố cách ly thì không truy được chuyện gì đã xảy ra.

---

## Gateway: G1–G7

Spec: `docs/superpowers/specs/2026-08-17-s3-gateway-design.md`.

Tám quyết định đã chốt ở đó, kèm lý do: pool là bảng riêng · `reqwest` tự ký ·
ETag lấy của upstream · listing đọc từ DB · multipart proxy thẳng · Redis bắt buộc
cho queue audit · versioning chỉ để chỗ trống · một spec cho cả năm slice.

Thứ tự bắt buộc: **G1 → G2 → G3** rồi nhánh. G4 và G5 độc lập nhau sau G3, chạy
song song được. G6 cần G4. G7 cần hết.

G1 kéo theo một việc P4 để lại: màn Pool trên console mà P4 cho thành
`ComingSoon` giờ có backend thật, cộng `/api/admin/pools`. Không có pool thì
không tạo được bucket, không có bucket thì gateway vô dụng — nên nó nằm trong G1
chứ không để mồ côi.

### Khối lượng đã biết (để ước lượng, không phải để thực thi)

Từ kiểm định, đây là thứ phải tồn tại trước khi một client S3 thật nói chuyện được:

1. Thêm HTTP client (`reqwest` hoặc `aws-sdk-s3`) — hiện tại `Cargo.toml` có 0.
2. Xác thực SigV4: canonical request, signed headers, `x-amz-content-sha256` kể
   cả `UNSIGNED-PAYLOAD` và `STREAMING-…`, cửa sổ lệch giờ, tra key qua
   `find_by_access_key_id` → `decrypt_secret`, cộng biến thể query-string cho
   presigned.
3. Cây route path-style `/{bucket}` và `/{bucket}/{*key}` trên
   GET/PUT/HEAD/DELETE/POST, dispatch theo query (`?uploads`, `?uploadId=`,
   `?list-type=2`, `?delete`). Đây là mảng lớn nhất.
4. Authorize theo key: map mỗi verb sang `ACTION_*`, enforce
   `access_key_prefixes` lên key của *request*, kiểm cả hai đầu CopyObject.
5. Prefix rewrite tại một chokepoint duy nhất, chặn path traversal.
6. Ký lại lên upstream bằng credential của bucket, stream body.
7. Đúng wire S3: `<Error><Code>` XML, status code, format ETag, `x-amz-request-id`.
8. Audit log: bảng, migration, một lần ghi mỗi request.
9. Ghi `tests/s3/golden/upstream.json` với store thật, rồi chạy suite với
   `OSG_S3_TARGET=gateway` và đưa vào CI.

Bộ conformance Python đã sẵn sàng làm cổng nghiệm thu: 61 test qua 8 file, phủ
mọi verb, phân trang, multipart, presigned, và 12 test riêng cho biên giới prefix
(`tests/s3/test_scoping.py`). Nó chưa chạy trong CI vì cần credential trong
repository secrets — `docs/superpowers/specs/2026-07-29-s3-conformance-suite-design.md:150`
ghi rõ đó là quyết định của chủ repo.

---

## Ghi chú cho người thực thi

**Checkbox trong plan phản ánh tiến độ thật.** Bốn plan cũ có 231 bước, 0 bước
được tick, kể cả những phần đã ship. Đừng lặp lại — tick khi xong.

**Mỗi task kết thúc bằng một deliverable test được.** Không gộp task, không nhảy
task. Nếu một task hoá ra sai, sửa plan trước rồi mới sửa code.

**Chạy trên cả ba backend trước khi đóng một giai đoạn.**

```bash
cargo test                                                        # sqlite in-memory
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test
```

Giai đoạn 3 tồn tại chính vì hai lỗi chỉ lộ trên MySQL (độ dài cột, collation).

---

## Phát hiện thêm khi thực thi (không có trong báo cáo kiểm định)

Những cái này chỉ lộ ra khi chạy thật, không lộ khi đọc code:

1. **`JWT_SECRET` phải là base64.** loco ký bằng `EncodingKey::from_base64_secret`,
   nên một giá trị không phải base64 vẫn cho app boot rồi làm **mọi** login thất
   bại với đúng một dòng `unauthorized!` — không phân biệt được với sai mật khẩu.
   Giờ boot production từ chối.
2. **`uri:` trong `production.yaml` không được quote** — một `DATABASE_URL` kết
   thúc bằng dấu hai chấm (`sqlite::memory:`) phá YAML.
3. **`secure_headers.preset` một mình vô tác dụng** — `is_enabled()` chỉ đọc
   `enable`, nên preset không có `enable: true` là header không ra.
4. **`mailer.smtp` bắt buộc có `host`** kể cả khi `enable: false`; phải gỡ hẳn
   khối `mailer:`.
5. **MySQL không cho drop index mà FK đang dựa vào** — migration đổi độ dài
   `object_key` phải tạo index tạm cho khoá ngoại bám vào trước.
6. **SQLite không `MODIFY COLUMN`** và cũng không ép độ dài varchar, nên
   migration độ dài cột phải bỏ qua SQLite hoàn toàn.
7. **`tower_governor` mặc định kéo cả `tonic`** (gRPC); phải tắt default features.
8. **Rate limit theo peer IP sau reverse proxy thành giới hạn toàn cục** — thêm
   `RATE_LIMIT_TRUST_PROXY`, mặc định tắt vì header do client gửi.
9. **`--all-targets` cho clippy lộ 4 lint `future_not_send`** — `TestServer` cố ý
   không `Send`; allow ở crate root của test.
10. **Route đã gỡ không trả 404** — static SPA fallback trả 405 cho POST và
    200 + `index.html` cho GET. Test phải assert vào body, không vào status.
11. **`cargo loco db entities` tự sửa `src/models/mod.rs`** và tự sinh file model
    rỗng. Thêm `pub mod x;` bằng tay sau lệnh đó là khai báo trùng.
12. **Clippy sạch với cache ấm không có nghĩa là sạch.** Crate nào không biên dịch
    lại thì lint của nó không in ra. Đổi `migration/src/lib.rs` làm lộ 12 lint có
    sẵn từ P3/P5, kể cả một phép ép `usize as i64` trong đường quota.
13. **Backfill viết bằng nội suy chuỗi hỏng trên MySQL.** `pools.pid` là `uuid`
    trên Postgres nhưng `binary(16)` trên MySQL — chỉ query builder mã hoá đúng cả
    hai. `db reset` không bao giờ có sẵn dòng để migrate, nên chỉ dựng tay một
    schema có dữ liệu mới thấy.
14. **MySQL không rollback được DDL.** Migration hỏng giữa chừng để lại bảng dở
    dang (`pool_id` đã thêm), lần chạy sau báo `Duplicate column name`.
