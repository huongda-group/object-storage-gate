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
| 1 | Xoá đăng ký + admin quản lý user | `2026-08-17-p1-auth-teardown-admin-users.md` | Blocker 1, 9 | — |
| 2 | Siết config, deploy, CI | `2026-08-17-p2-hardening-config-ops.md` | Blocker 2, 3, 4, 5 | — |
| 3 | Sửa tầng dữ liệu | `2026-08-17-p3-data-layer-correctness.md` | — (7 High) | — |
| 4 | Console bỏ mock, nối API thật | `2026-08-17-p4-console-real-api.md` | Blocker 7, 8 | 1 |
| 5 | Máy quota | `2026-08-17-p5-quota-engine.md` | Blocker 6 | 3 |
| 6 | Gateway: SigV4 + route S3 + proxy | **chưa có plan — cần spec** | slice #2, #3 | 2, 3, 5 |
| 7 | Gateway: multipart, copy, presigned, audit | **chưa có plan — cần spec** | slice #5, #6 | 6 |

Giai đoạn 1, 2, 3 độc lập nhau — chạy song song được nếu có nhiều người.
Giai đoạn 4 cần API admin của giai đoạn 1. Giai đoạn 5 cần index và guard của
giai đoạn 3.

---

## Cổng nghiệm thu

**Cổng A — mở console cho người dùng nội bộ.** Xong giai đoạn 1, 2, 4.
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

## Giai đoạn 6–7: vì sao chưa có plan

Viết plan TDD cho SigV4 mà không có spec là bịa. `docs/superpowers/plans/` hiện
có 4 file, không file nào cho gateway; `docs/superpowers/specs/` cũng vậy. Trước
khi viết được plan, phải chốt năm câu hỏi trong
`docs/superpowers/specs/2026-08-17-go-live-hardening-design.md` mục 6:

1. Layout key vật lý — `FUTURE.md` và `README.md` đang mâu thuẫn.
2. Streaming hay buffer — quyết định này chọn luôn HTTP client.
3. Nguồn sự thật cho ETag.
4. Redis bắt buộc hay tuỳ chọn.
5. Versioning bây giờ hay sau.

Bước kế tiếp cho nhánh gateway là một phiên brainstorm ra
`docs/superpowers/specs/2026-08-XX-s3-gateway-design.md`, không phải viết code.

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
