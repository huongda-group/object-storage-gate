# G6 — Multipart, Copy, Presigned — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Upload nhiều phần, copy trong và giữa các bucket của cùng tài khoản, và URL đã ký sẵn — cả ba tôn trọng đúng biên giới cách ly và đúng kế toán quota.

**Architecture:** Part đi thẳng sang multipart của upstream; gateway chỉ giữ một bảng ánh xạ `UploadId` của mình sang của upstream, cộng tổng đang giữ để Abort trả lại đúng. Copy giải đầu nguồn bằng **cùng đoạn code** với đầu đích, vì hai đầu của một copy là chỗ kinh điển để một bên được kiểm và bên kia không. Presigned dùng chung `canonical_request()` với dạng header.

**Tech Stack:** Rust, axum 0.8, SeaORM 1.1, `quick-xml`.

**Spec:** `docs/superpowers/specs/2026-08-17-s3-gateway-design.md` mục 10, 11, 5.3

**Phụ thuộc:** G4 xong — `PendingPut`, `record_put`, `xml`, `TestGateway`.

**Deliverable:** `aws s3 cp` một file 100 MB (aws-cli tự chuyển sang multipart), `aws s3 cp s3://b/a s3://b/c`, và một presigned URL mở được bằng `curl` không credential. 20/61 test conformance còn lại.

## Global Constraints

- Ba backend hạng nhất: Postgres, MySQL >= 8.0.13, SQLite.
- **Quota mutation không lấy lock.** Multipart cộng dồn reservation qua nhiều request, mỗi lần vẫn là một `UPDATE ... WHERE <guard>`.
- Comment trong code: tiếng Anh, một câu một dòng.
- Không tự commit/push ngoài các bước commit trong plan. Không AI attribution.
- Sau mỗi task: clippy CI-strict sạch, test xanh trên cả ba backend.

---

## File Structure

**Tạo mới:**
- `src/models/multipart_uploads.rs`
- `src/controllers/s3/multipart.rs`
- `src/controllers/s3/copy.rs`
- `tests/models/multipart_uploads.rs`
- `tests/requests/s3/multipart.rs`
- `tests/requests/s3/copy.rs`
- `tests/requests/s3/presigned.rs`

**Sửa:**
- `src/models/mod.rs`, `src/s3/xml.rs`, `src/controllers/s3/mod.rs`, `src/app.rs` (truncate)
- `src/s3/request.rs` — `resolve_copy_source` có caller đầu tiên
- `tests/support/signer.rs` — `presign`

---

## Task 1: Model `multipart_uploads`

**Files:**
- Create: `src/models/multipart_uploads.rs`, `tests/models/multipart_uploads.rs`
- Modify: `src/models/mod.rs`, `src/app.rs`, `tests/models/mod.rs`

**Interfaces:**
- Consumes: bảng `multipart_uploads` (G1 migration `m20260818_000003`).
- Produces:
  - `multipart_uploads::Model::create(db, bucket_id, key, upstream_upload_id) -> ModelResult<Model>`
  - `multipart_uploads::Model::find_for(db, pid, bucket_id, key) -> ModelResult<Model>`
  - `multipart_uploads::Model::add_reserved(db, id, bytes) -> ModelResult<()>`
  - `multipart_uploads::Model::list_for_bucket(db, bucket_id, prefix) -> ModelResult<Vec<Model>>`
  - `multipart_uploads::Model::older_than(db, days) -> ModelResult<Vec<Model>>`

- [ ] **Step 1: Viết test**

```rust
/// The UploadId a client sees is our pid, and the lookup must pin it to the bucket and key from
/// the path — otherwise an UploadId from one bucket can be used to write into another.
#[tokio::test]
#[serial]
async fn find_for_pins_the_upload_to_its_bucket_and_key() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let a = a_bucket(db, "bucket-a").await;
    let b = a_bucket(db, "bucket-b").await;
    let up = multipart_uploads::Model::create(db, a, "img/big.bin", "upstream-1")
        .await
        .unwrap();
    let pid = up.pid.to_string();

    // Right bucket, right key.
    assert!(multipart_uploads::Model::find_for(db, &pid, a, "img/big.bin").await.is_ok());

    // Right pid, wrong bucket.
    assert!(multipart_uploads::Model::find_for(db, &pid, b, "img/big.bin").await.is_err());

    // Right pid, right bucket, wrong key.
    assert!(multipart_uploads::Model::find_for(db, &pid, a, "img/other.bin").await.is_err());
}

/// add_reserved is a guarded UPDATE, so two concurrent UploadPart calls both count.
#[tokio::test]
#[serial]
async fn add_reserved_accumulates_across_concurrent_parts() {
    let boot = boot_test::<App>().await.unwrap();
    let db = boot.app_context.db.clone();
    seed::<App>(&boot.app_context).await.unwrap();

    let bucket_id = a_bucket(&db, "concurrent").await;
    let up = multipart_uploads::Model::create(&db, bucket_id, "big.bin", "upstream-1")
        .await
        .unwrap();
    let id = up.id;

    let a = { let db = db.clone(); tokio::spawn(async move {
        multipart_uploads::Model::add_reserved(&db, id, 500).await }) };
    let b = { let db = db.clone(); tokio::spawn(async move {
        multipart_uploads::Model::add_reserved(&db, id, 300).await }) };
    let (ra, rb) = tokio::join!(a, b);
    ra.unwrap().unwrap();
    rb.unwrap().unwrap();

    let fresh = multipart_uploads::Model::find_by_id(&db, id).await.unwrap();
    assert_eq!(fresh.reserved_bytes, 800, "a lost update means Abort releases too little");
}

/// Deleting a bucket takes its open uploads with it; a row pointing at a gone bucket is a row the
/// cleanup task can never resolve.
#[tokio::test]
#[serial]
async fn uploads_cascade_with_their_bucket() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let bucket_id = a_bucket(db, "doomed").await;
    multipart_uploads::Model::create(db, bucket_id, "big.bin", "upstream-1")
        .await
        .unwrap();

    let bucket = buckets::Entity::find_by_id(bucket_id).one(db).await.unwrap().unwrap();
    let am: buckets::ActiveModel = bucket.into();
    am.delete(db).await.unwrap();

    assert!(
        multipart_uploads::Model::list_for_bucket(db, bucket_id, "").await.unwrap().is_empty()
    );
}

#[tokio::test]
#[serial]
async fn older_than_finds_stale_uploads_only() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let bucket_id = a_bucket(db, "stale").await;
    let fresh = multipart_uploads::Model::create(db, bucket_id, "new.bin", "u1").await.unwrap();
    let old = multipart_uploads::Model::create(db, bucket_id, "old.bin", "u2").await.unwrap();

    // Backdate one row directly; created_at is set by the DB on insert.
    backdate(db, old.id, 30).await;

    let stale = multipart_uploads::Model::older_than(db, 7).await.unwrap();
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].id, old.id);
    assert!(stale.iter().all(|u| u.id != fresh.id));
}
```

Test `add_reserved_accumulates_across_concurrent_parts` là cái đáng nhất: một client upload nhiều part song song là chuyện bình thường (aws-cli mặc định 10 luồng), và một lost update ở đây làm Abort release thiếu — quota bị giữ vĩnh viễn mà không ai thấy.

Thêm `mod multipart_uploads;` vào `tests/models/mod.rs`. Thêm `truncate_table(&ctx.db, multipart_uploads::Entity)` vào `App::truncate` **trước** `buckets`.

- [ ] **Step 2: Chạy để chắc nó fail, viết, chạy lại**

`add_reserved` phải là `UPDATE ... SET reserved_bytes = reserved_bytes + ? WHERE id = ?`, không phải read-modify-write:

```rust
    /// Adds to the running hold with a single guarded UPDATE.
    ///
    /// Read-modify-write loses one of two concurrent UploadPart calls, and a lost update here
    /// means Abort releases less than was held — quota held forever with nothing to show it.
    ///
    /// # Errors
    /// Returns an error on DB failure.
    pub async fn add_reserved(db: &DatabaseConnection, id: i32, bytes: i64) -> ModelResult<()> {
        Entity::update_many()
            .col_expr(
                Column::ReservedBytes,
                Expr::col(Column::ReservedBytes).add(bytes),
            )
            .filter(Column::Id.eq(id))
            .exec(db)
            .await?;
        Ok(())
    }
```

- [ ] **Step 3: Ba backend và commit**

```bash
cargo test --test mod models::multipart_uploads 2>&1 | tail -5
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test 2>&1 | tail -3
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test 2>&1 | tail -3
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add src/ tests/
git commit -m "feat(multipart): the upload-mapping model

find_for pins an UploadId to the bucket and key from the path, so an UploadId
issued for one bucket cannot be used to write into another. add_reserved is a
guarded UPDATE rather than read-modify-write: aws-cli uploads ten parts at once
by default, and a lost update means Abort releases less than was held — quota
held forever with nothing to show for it."
```

---

## Task 2: Create, UploadPart, Abort

**Files:**
- Create: `src/controllers/s3/multipart.rs`
- Modify: `src/controllers/s3/mod.rs`, `src/s3/xml.rs`
- Test: `tests/requests/s3/multipart.rs`

**Interfaces:**
- Consumes: `S3Request`, `upstream::Client`, `multipart_uploads::Model`, `quota`.
- Produces: `multipart::create`, `multipart::upload_part`, `multipart::abort`; `xml::initiate_multipart_result`.

- [ ] **Step 1: Viết test**

```rust
/// The UploadId handed to the client is ours, never upstream's — leaking upstream identifiers
/// leaks that there is an upstream and how it names things.
#[tokio::test]
#[serial]
async fn create_returns_our_upload_id_not_upstreams() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(canned(
            200,
            br#"<InitiateMultipartUploadResult><UploadId>UPSTREAM-XYZ</UploadId>
                </InitiateMultipartUploadResult>"#,
        ));

        let res = g.post(&signer, "/media-cdn/img/big.bin?uploads", b"").await;

        assert_eq!(res.status_code(), 200);
        let body = res.text();
        assert!(body.contains("<InitiateMultipartUploadResult"));
        assert!(body.contains("<Bucket>media-cdn</Bucket>"));
        assert!(body.contains("<Key>img/big.bin</Key>"));
        assert!(!body.contains("UPSTREAM-XYZ"), "upstream UploadId leaked: {body}");
        g.mock.assert_key(0, "osg-main/{user_pid}/media-cdn/img/big.bin");
    })
    .await;
}

/// test_scoping.py::test_scoped_key_cannot_start_a_multipart_upload_outside
#[tokio::test]
#[serial]
async fn a_scoped_key_cannot_start_an_upload_outside() {
    with_gateway(|g| async move {
        let signer = g.scoped_key("img/").await;

        let res = g.post(&signer, "/media-cdn/docs/big.bin?uploads", b"").await;

        assert_eq!(res.status_code(), 403);
        g.mock.assert_untouched();
    })
    .await;
}

/// Each part reserves before it moves, so a client cannot fill a 1 MiB bucket with 10 GiB of
/// parts and only find out at Complete.
#[tokio::test]
#[serial]
async fn each_part_reserves_before_it_uploads() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.set_bucket_quota("media-cdn", 10 * 1024 * 1024).await;
        let upload = g.start_upload(&signer, "img/big.bin").await;

        g.mock.push(etag_ok("\"p1\""));
        g.upload_part(&signer, "img/big.bin", &upload, 1, &vec![0u8; 6 * 1024 * 1024]).await;
        assert_eq!(g.bucket_row("media-cdn").await.reserved_bytes, 6 * 1024 * 1024);

        // Second part would exceed the bucket quota: refused, and upstream never sees it.
        let before = g.mock.requests().len();
        let res = g
            .upload_part_raw(&signer, "img/big.bin", &upload, 2, &vec![0u8; 6 * 1024 * 1024])
            .await;
        assert_eq!(res.status_code(), 403);
        assert!(res.text().contains("QuotaExceeded"));
        assert_eq!(g.mock.requests().len(), before, "the refused part reached upstream");
    })
    .await;
}

/// A part upload that fails upstream releases its own hold, not the whole accumulated one.
#[tokio::test]
#[serial]
async fn a_failed_part_releases_only_its_own_hold() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        let upload = g.start_upload(&signer, "img/big.bin").await;

        g.mock.push(etag_ok("\"p1\""));
        g.upload_part(&signer, "img/big.bin", &upload, 1, &vec![0u8; 300]).await;

        g.mock.push(canned(503, b"nope"));
        let res = g.upload_part_raw(&signer, "img/big.bin", &upload, 2, &vec![0u8; 500]).await;
        assert_eq!(res.status_code(), 500);

        assert_eq!(
            g.bucket_row("media-cdn").await.reserved_bytes, 300,
            "only the failed part's hold should be released"
        );
    })
    .await;
}

/// An UploadId from another bucket must not work, even with a valid signature.
#[tokio::test]
#[serial]
async fn an_upload_id_from_another_bucket_is_no_such_upload() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.extra_bucket("archive").await;
        let upload = g.start_upload(&signer, "img/big.bin").await;

        let res = g
            .upload_part_raw_in(&signer, "archive", "img/big.bin", &upload, 1, b"x")
            .await;

        assert_eq!(res.status_code(), 404);
        assert!(res.text().contains("NoSuchUpload"));
    })
    .await;
}

/// test_multipart.py::test_abort_discards_the_upload_and_writes_nothing
#[tokio::test]
#[serial]
async fn abort_releases_everything_and_writes_no_object() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        let upload = g.start_upload(&signer, "img/big.bin").await;

        g.mock.push(etag_ok("\"p1\""));
        g.upload_part(&signer, "img/big.bin", &upload, 1, &vec![0u8; 300]).await;
        g.mock.push(etag_ok("\"p2\""));
        g.upload_part(&signer, "img/big.bin", &upload, 2, &vec![0u8; 500]).await;
        assert_eq!(g.bucket_row("media-cdn").await.reserved_bytes, 800);

        g.mock.push(canned(204, b""));
        let res = g.delete(&signer, &format!("/media-cdn/img/big.bin?uploadId={upload}")).await;

        assert_eq!(res.status_code(), 204);
        let b = g.bucket_row("media-cdn").await;
        assert_eq!(b.reserved_bytes, 0);
        assert_eq!(b.used_bytes, 0);
        assert!(g.object_row("media-cdn", "img/big.bin").await.is_none());
        assert!(g.upload_row(&upload).await.is_none(), "the mapping row must be gone");
    })
    .await;
}

/// Abort twice: the second is still 204 and does not release again.
#[tokio::test]
#[serial]
async fn abort_is_idempotent() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        let upload = g.start_upload(&signer, "img/big.bin").await;
        g.mock.push(etag_ok("\"p1\""));
        g.upload_part(&signer, "img/big.bin", &upload, 1, &vec![0u8; 300]).await;

        g.mock.push(canned(204, b""));
        g.delete(&signer, &format!("/media-cdn/img/big.bin?uploadId={upload}")).await;

        let res = g.delete(&signer, &format!("/media-cdn/img/big.bin?uploadId={upload}")).await;
        assert_eq!(res.status_code(), 404, "the mapping is gone, so the upload is unknown");
        assert_eq!(g.bucket_row("media-cdn").await.reserved_bytes, 0);
    })
    .await;
}
```

`a_failed_part_releases_only_its_own_hold` bắt một lỗi dễ viết: release `reserved_bytes` của cả upload thay vì phần của part vừa hỏng, làm mất luôn phần đã giữ của những part đã thành công.

- [ ] **Step 2: Chạy để chắc nó fail, viết**

Thứ tự trong `upload_part`:

```
1. resolve(action = ACTION_MULTIPART)
2. find_for(pid, bucket.id, logical_key)  -> NoSuchUpload nếu lệch
3. Content-Length thiếu -> MissingContentLength
4. quota::reserve(bucket.id, len)         -> QuotaExceeded
5. upstream UploadPart(upstream_upload_id, partNumber) với body stream
6. Ok  -> add_reserved(upload.id, len); trả ETag của upstream
   Err -> quota::release(reservation của riêng part này); trả lỗi
```

Bước 6 nhánh `Err` release **`reservation` của lời gọi này**, không phải `upload.reserved_bytes`.

`abort`:

```
1. resolve(action = ACTION_MULTIPART)
2. find_for -> NoSuchUpload (nên abort lần hai trả 404, đúng như test)
3. upstream Abort
4. quota::release(upload.reserved_bytes)
5. xoá row
```

Bước 3 trước 4–5 là cố ý: upstream lỗi thì reservation còn nguyên và `cleanup_multipart` xử lý sau; đảo lại thì quota trả rồi mà part vẫn nằm trên store.

- [ ] **Step 3: Ba backend và commit**

```bash
cargo test --test mod requests::s3::multipart 2>&1 | tail -5
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test 2>&1 | tail -3
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test 2>&1 | tail -3
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add src/ tests/
git commit -m "feat(s3): CreateMultipartUpload, UploadPart, Abort

Every part reserves before it moves, so a client cannot fill a 1 MiB bucket with
10 GiB of parts and only discover it at Complete. A failed part releases its own
hold, not the accumulated one — releasing everything would discard the holds of
the parts that succeeded.

The UploadId handed to the client is ours, never upstream's: leaking the upstream
identifier leaks that there is an upstream and how it names things. Abort proxies
before releasing, so an upstream failure leaves the hold for the cleanup task
rather than crediting quota for parts still sitting on the store."
```

---

## Task 3: CompleteMultipartUpload

**Files:**
- Modify: `src/controllers/s3/multipart.rs`, `src/s3/xml.rs`
- Test: `tests/requests/s3/multipart.rs`

**Interfaces:**
- Consumes: task 1, 2; `record_put` (G4).
- Produces: `multipart::complete`; `xml::parse_complete_request`, `xml::complete_multipart_result`.

- [ ] **Step 1: Viết test**

```rust
/// test_multipart.py::test_multipart_round_trip
#[tokio::test]
#[serial]
async fn complete_writes_metadata_and_settles_the_quota_exactly() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        let upload = g.start_upload(&signer, "img/big.bin").await;

        g.mock.push(etag_ok("\"p1\""));
        g.upload_part(&signer, "img/big.bin", &upload, 1, &vec![0u8; 6 * 1024 * 1024]).await;
        g.mock.push(etag_ok("\"p2\""));
        g.upload_part(&signer, "img/big.bin", &upload, 2, &vec![0u8; 1024]).await;

        let held = 6 * 1024 * 1024 + 1024;
        assert_eq!(g.bucket_row("media-cdn").await.reserved_bytes, held);

        // Complete, then the HEAD the gateway does to learn the final size.
        g.mock.push(canned(
            200,
            br#"<CompleteMultipartUploadResult><ETag>"final-2"</ETag>
                </CompleteMultipartUploadResult>"#,
        ));
        g.mock.push(Canned {
            status: 200,
            headers: vec![("content-length".into(), held.to_string())],
            body: vec![],
        });

        let res = g
            .complete(&signer, "img/big.bin", &upload, &[(1, "\"p1\""), (2, "\"p2\"")])
            .await;

        assert_eq!(res.status_code(), 200);
        let body = res.text();
        assert!(body.contains("<CompleteMultipartUploadResult"));
        assert!(body.contains("<ETag>&quot;final-2&quot;</ETag>") || body.contains("final-2"));
        assert!(body.contains("<Bucket>media-cdn</Bucket>"));
        assert!(body.contains("<Key>img/big.bin</Key>"));
        assert!(!body.contains("osg-main"));

        let b = g.bucket_row("media-cdn").await;
        assert_eq!(b.reserved_bytes, 0, "the whole hold must be settled");
        assert_eq!(b.used_bytes, held);
        assert_eq!(b.object_count, 1);

        let row = g.object_row("media-cdn", "img/big.bin").await.unwrap();
        assert_eq!(row.size, held);
        assert_eq!(row.etag, "\"final-2\"", "the upstream ETag verbatim, -N form included");

        assert!(g.upload_row(&upload).await.is_none());
    })
    .await;
}

/// A part re-uploaded twice was reserved twice; Complete gives the excess back rather than
/// leaving the bucket permanently short.
#[tokio::test]
#[serial]
async fn complete_returns_the_excess_from_a_re_uploaded_part() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        let upload = g.start_upload(&signer, "img/big.bin").await;

        // Part 1 uploaded twice: 300 reserved each time.
        g.mock.push(etag_ok("\"p1a\""));
        g.upload_part(&signer, "img/big.bin", &upload, 1, &vec![0u8; 300]).await;
        g.mock.push(etag_ok("\"p1b\""));
        g.upload_part(&signer, "img/big.bin", &upload, 1, &vec![0u8; 300]).await;
        assert_eq!(g.bucket_row("media-cdn").await.reserved_bytes, 600);

        g.mock.push(canned(200, br#"<CompleteMultipartUploadResult><ETag>"f"</ETag></CompleteMultipartUploadResult>"#));
        g.mock.push(Canned {
            status: 200,
            headers: vec![("content-length".into(), "300".into())],
            body: vec![],
        });

        g.complete(&signer, "img/big.bin", &upload, &[(1, "\"p1b\"")]).await;

        let b = g.bucket_row("media-cdn").await;
        assert_eq!(b.used_bytes, 300, "charge the object, not the sum of attempts");
        assert_eq!(b.reserved_bytes, 0, "the 300 excess must be released");
    })
    .await;
}

/// test_multipart.py::test_non_final_part_below_the_minimum_is_rejected_at_complete
#[tokio::test]
#[serial]
async fn an_upstream_entity_too_small_keeps_its_code() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        let upload = g.start_upload(&signer, "img/big.bin").await;
        g.mock.push(etag_ok("\"p1\""));
        g.upload_part(&signer, "img/big.bin", &upload, 1, &vec![0u8; 100]).await;
        g.mock.push(etag_ok("\"p2\""));
        g.upload_part(&signer, "img/big.bin", &upload, 2, &vec![0u8; 100]).await;

        g.mock.push(canned(
            400,
            br#"<Error><Code>EntityTooSmall</Code>
                <Message>Your proposed upload is smaller than the minimum allowed size</Message>
                <Key>osg-main/1111/media-cdn/img/big.bin</Key></Error>"#,
        ));

        let res = g
            .complete(&signer, "img/big.bin", &upload, &[(1, "\"p1\""), (2, "\"p2\"")])
            .await;

        assert_eq!(res.status_code(), 400);
        let body = res.text();
        assert!(body.contains("EntityTooSmall"));
        assert!(!body.contains("osg-main"), "physical path leaked: {body}");

        // A failed Complete leaves the upload open so the client can retry.
        assert!(g.upload_row(&upload).await.is_some());
        assert_eq!(g.bucket_row("media-cdn").await.reserved_bytes, 200);
    })
    .await;
}

/// test_multipart.py::test_complete_with_a_wrong_part_etag_is_invalid_part
#[tokio::test]
#[serial]
async fn a_wrong_part_etag_is_invalid_part() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        let upload = g.start_upload(&signer, "img/big.bin").await;
        g.mock.push(etag_ok("\"p1\""));
        g.upload_part(&signer, "img/big.bin", &upload, 1, &vec![0u8; 300]).await;

        g.mock.push(canned(400, br#"<Error><Code>InvalidPart</Code></Error>"#));
        let res = g.complete(&signer, "img/big.bin", &upload, &[(1, "\"wrong\"")]).await;

        assert_eq!(res.status_code(), 400);
        assert!(res.text().contains("InvalidPart"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn complete_with_an_empty_part_list_is_malformed_xml() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        let upload = g.start_upload(&signer, "img/big.bin").await;

        let res = g.complete(&signer, "img/big.bin", &upload, &[]).await;

        assert_eq!(res.status_code(), 400);
        assert!(res.text().contains("MalformedXML"));
        // Only the create call; Complete never went out.
        assert_eq!(g.mock.requests().len(), 1);
    })
    .await;
}
```

`an_upstream_entity_too_small_keeps_its_code` khẳng định thêm một điều quan trọng: Complete thất bại thì **giữ** upload mở và giữ reservation, để client sửa rồi thử lại. Xoá row lúc đó là làm client mất đường retry và bỏ part trên store thành rác.

- [ ] **Step 2: Chạy để chắc nó fail, viết**

```
1. resolve(action = ACTION_MULTIPART)
2. find_for -> NoSuchUpload
3. parse XML part list; rỗng hoặc sai -> MalformedXml (chưa gọi upstream)
4. upstream Complete(upstream_upload_id, parts)
     Err -> trả lỗi, GIỮ row và GIỮ reservation
5. upstream HEAD(physical_key) -> size thật
     Complete không trả size, và cộng size các part thì sai khi có part upload lại
6. record_put(size, upstream_etag, content_type)
7. quota::commit đúng `size`; quota::release(upload.reserved_bytes - size)
8. xoá row
```

Bước 5 tốn một round trip. Đổi lại là con số đúng: cộng size các part đếm cả lần upload lại, và spec mục 10 đã chốt chấp nhận round trip này thay vì đoán.

Bước 7 dùng `quota::commit` với một `Reservation` dựng tay từ `(bucket_id, user_id, size)` — hoặc thêm `quota::commit_amount(db, bucket_id, bytes, delta_objects)` cho rõ. Chọn cái sau: dựng `Reservation` tay ở ngoài module quota là mở đường cho người khác dựng sai.

- [ ] **Step 3: Ba backend và commit**

```bash
cargo test --test mod requests::s3::multipart 2>&1 | tail -5
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test 2>&1 | tail -3
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test 2>&1 | tail -3
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add src/ tests/
git commit -m "feat(s3): CompleteMultipartUpload

Complete HEADs the object afterwards to learn its real size: the Complete
response does not carry one, and summing the part sizes counts a re-uploaded part
twice. The accumulated hold is then settled to exactly that size and the excess
released, so a client that retried a part does not leave the bucket permanently
short.

A failed Complete keeps the upload open and the hold in place, so the client can
fix the part list and retry — deleting the row would strand the parts on the
store with no way to reference them."
```

---

## Task 4: CopyObject và UploadPartCopy

**Files:**
- Create: `src/controllers/s3/copy.rs`
- Modify: `src/controllers/s3/mod.rs`, `src/controllers/s3/multipart.rs`, `src/s3/xml.rs`
- Test: `tests/requests/s3/copy.rs`

**Interfaces:**
- Consumes: `S3Request::resolve_copy_source` (G3), `PendingPut` (G4).
- Produces: `copy::copy_object`, `multipart::upload_part_copy`; `xml::copy_object_result`, `xml::copy_part_result`.

- [ ] **Step 1: Viết test**

```rust
/// test_copy.py::test_copy_in_bucket_keeps_bytes_and_etag
#[tokio::test]
#[serial]
async fn copy_within_a_bucket_rewrites_both_ends() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.seed_object("media-cdn", "img/a.png", 300, "\"e1\"").await;
        g.mock.push(canned(
            200,
            br#"<CopyObjectResult><ETag>"e1"</ETag>
                <LastModified>2026-08-17T08:00:00.000Z</LastModified></CopyObjectResult>"#,
        ));

        let res = g
            .copy(&signer, "/media-cdn/img/b.png", "/media-cdn/img/a.png")
            .await;

        assert_eq!(res.status_code(), 200);
        assert!(res.text().contains("<CopyObjectResult"));

        // Both ends rewritten: the source header and the destination path.
        let seen = &g.mock.requests()[0];
        assert_eq!(seen.path.trim_start_matches('/'), "osg-main/{user_pid}/media-cdn/img/b.png");
        assert_eq!(
            header_of(seen, "x-amz-copy-source"),
            "/osg-main/{user_pid}/media-cdn/img/a.png"
        );

        assert_eq!(g.bucket_row("media-cdn").await.used_bytes, 600);
        assert_eq!(g.bucket_row("media-cdn").await.object_count, 2);
    })
    .await;
}

/// test_scoping.py::test_scoped_key_cannot_copy_from_outside
#[tokio::test]
#[serial]
async fn a_scoped_key_cannot_copy_from_outside() {
    with_gateway(|g| async move {
        let signer = g.scoped_key("img/").await;
        g.seed_object("media-cdn", "docs/secret.pdf", 300, "\"e\"").await;

        let res = g
            .copy(&signer, "/media-cdn/img/stolen.pdf", "/media-cdn/docs/secret.pdf")
            .await;

        assert_eq!(res.status_code(), 403);
        g.mock.assert_untouched();
    })
    .await;
}

/// test_scoping.py::test_scoped_key_cannot_copy_to_outside
#[tokio::test]
#[serial]
async fn a_scoped_key_cannot_copy_to_outside() {
    with_gateway(|g| async move {
        let signer = g.scoped_key("img/").await;
        g.seed_object("media-cdn", "img/a.png", 300, "\"e\"").await;

        let res = g
            .copy(&signer, "/media-cdn/docs/leaked.png", "/media-cdn/img/a.png")
            .await;

        assert_eq!(res.status_code(), 403);
        g.mock.assert_untouched();
    })
    .await;
}

/// Another user's bucket as a source reads as absent, not forbidden.
#[tokio::test]
#[serial]
async fn copying_from_another_users_bucket_is_no_such_bucket() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.other_user_bucket("theirs").await;

        let res = g.copy(&signer, "/media-cdn/a.png", "/theirs/a.png").await;

        assert_eq!(res.status_code(), 404);
        assert!(res.text().contains("NoSuchBucket"));
        g.mock.assert_untouched();
    })
    .await;
}

/// test_copy.py::test_copy_onto_itself_without_replace_is_rejected
#[tokio::test]
#[serial]
async fn self_copy_without_replace_is_invalid_request() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.seed_object("media-cdn", "img/a.png", 300, "\"e\"").await;

        let res = g.copy(&signer, "/media-cdn/img/a.png", "/media-cdn/img/a.png").await;
        assert_eq!(res.status_code(), 400);
        assert!(res.text().contains("InvalidRequest"));
        g.mock.assert_untouched();

        // With REPLACE it is allowed: that is how a client changes metadata in place.
        g.mock.push(canned(200, br#"<CopyObjectResult><ETag>"e"</ETag></CopyObjectResult>"#));
        let res = g
            .copy_with(
                &signer,
                "/media-cdn/img/a.png",
                "/media-cdn/img/a.png",
                &[("x-amz-metadata-directive", "REPLACE")],
            )
            .await;
        assert_eq!(res.status_code(), 200);
    })
    .await;
}

/// test_copy.py::test_copy_from_missing_source_is_no_such_key
#[tokio::test]
#[serial]
async fn copying_a_missing_source_is_no_such_key() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;

        let res = g.copy(&signer, "/media-cdn/b.png", "/media-cdn/gone.png").await;

        assert_eq!(res.status_code(), 404);
        assert!(res.text().contains("NoSuchKey"));
        g.mock.assert_untouched();
    })
    .await;
}

/// test_copy.py::test_copy_does_not_remove_the_source
#[tokio::test]
#[serial]
async fn copy_leaves_the_source_alone() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.seed_object("media-cdn", "img/a.png", 300, "\"e\"").await;
        g.mock.push(canned(200, br#"<CopyObjectResult><ETag>"e"</ETag></CopyObjectResult>"#));

        g.copy(&signer, "/media-cdn/img/b.png", "/media-cdn/img/a.png").await;

        assert!(g.object_row("media-cdn", "img/a.png").await.is_some());
        assert!(g.object_row("media-cdn", "img/b.png").await.is_some());
    })
    .await;
}

/// Copying into a bucket with no room is refused before upstream sees it.
#[tokio::test]
#[serial]
async fn copy_respects_the_destination_quota() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.seed_object("media-cdn", "img/a.png", 900, "\"e\"").await;
        g.set_bucket_quota("media-cdn", 1000).await;

        let res = g.copy(&signer, "/media-cdn/img/b.png", "/media-cdn/img/a.png").await;

        assert_eq!(res.status_code(), 403);
        assert!(res.text().contains("QuotaExceeded"));
        g.mock.assert_untouched();
    })
    .await;
}

/// test_multipart.py::test_upload_part_copy_takes_a_part_from_an_existing_object
#[tokio::test]
#[serial]
async fn upload_part_copy_authorises_the_source() {
    with_gateway(|g| async move {
        let signer = g.scoped_key("img/").await;
        g.seed_object("media-cdn", "docs/secret.pdf", 300, "\"e\"").await;
        let upload = g.start_upload(&signer, "img/big.bin").await;

        let res = g
            .upload_part_copy(&signer, "img/big.bin", &upload, 1, "/media-cdn/docs/secret.pdf")
            .await;

        assert_eq!(res.status_code(), 403);
        // Only the create call went out.
        assert_eq!(g.mock.requests().len(), 1);
    })
    .await;
}
```

Khẳng định trong test đầu về `x-amz-copy-source` là chỗ dễ sai nhất của cả task: header gửi lên upstream phải là **physical** path kèm physical bucket, còn header nhận từ client là **logical**. Quên rewrite nó thì upstream báo `NoSuchKey` và triệu chứng trông như object không tồn tại.

- [ ] **Step 2: Chạy để chắc nó fail, viết**

```
1. resolve(action = ACTION_WRITE)            -> dest
2. header x-amz-copy-source, thiếu -> không phải copy
3. resolve_copy_source(dest, header)         -> source (cùng key policy)
4. self-copy && directive != REPLACE -> InvalidRequest
5. objects::get(source) -> None: NoSuchKey; Some: size
6. begin_put(dest.bucket.id, dest.logical_key, source_size)  -> QuotaExceeded
7. upstream CopyObject:
     path   = physical_key của dest
     header = x-amz-copy-source: /{pool.physical_bucket}/{physical_key của source}
     chuyển x-amz-metadata-directive; khi REPLACE thì chuyển cả content-type và x-amz-meta-*
8. Ok  -> pending.commit(db, etag, content_type)
   Err -> pending.abort(db)
```

Bước 5 lấy size từ row `objects` chứ không HEAD upstream: row là nguồn cho quota, và một HEAD nữa chỉ để biết size là round trip không cần.

`upload_part_copy` giống nhưng ghi vào reservation của upload thay vì `PendingPut`, và trả `<CopyPartResult>`.

- [ ] **Step 3: Ba backend và commit**

```bash
cargo test --test mod requests::s3::copy 2>&1 | tail -5
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test 2>&1 | tail -3
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test 2>&1 | tail -3
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add src/ tests/
git commit -m "feat(s3): CopyObject and UploadPartCopy

Both ends go through resolve_copy_source, which is the same code path as the
destination rather than a parallel one — the two ends of a copy are the classic
place where one side gets checked and the other does not, and test_scoping has
three cases for exactly that.

The x-amz-copy-source header sent upstream carries the physical bucket and key;
the one received from the client carries the logical pair. Forgetting to rewrite
it produces an upstream NoSuchKey whose symptom looks like a missing object."
```

---

## Task 5: Presigned URL

**Files:**
- Modify: `src/s3/request.rs`, `src/controllers/s3/mod.rs`, `tests/support/signer.rs`
- Test: `tests/requests/s3/presigned.rs`

**Interfaces:**
- Consumes: `sigv4::parse_query` (G2).
- Produces: `S3Request::resolve` nhận được cả dạng query khi không có `Authorization`.

- [ ] **Step 1: Viết test**

```rust
/// test_presigned.py::test_presigned_get_serves_the_object_without_credentials
#[tokio::test]
#[serial]
async fn a_presigned_get_needs_no_credentials_on_the_request() {
    with_gateway(|g| async move {
        let signer = g.key_with(&["read", "presigned"], &[]).await;
        g.mock.push(ok_body(b"png bytes"));

        let url = signer.presign("GET", "/media-cdn/img/a.png", 3600, &g.host);
        // No Authorization header at all.
        let res = g.raw_get(&url, &[]).await;

        assert_eq!(res.status_code(), 200);
        assert_eq!(res.text(), "png bytes");
        g.mock.assert_key(0, "osg-main/{user_pid}/media-cdn/img/a.png");
    })
    .await;
}

/// test_presigned.py::test_presigned_put_accepts_an_upload
#[tokio::test]
#[serial]
async fn a_presigned_put_accepts_an_upload() {
    with_gateway(|g| async move {
        let signer = g.key_with(&["write", "presigned"], &[]).await;
        g.mock.push(etag_ok("\"e1\""));

        let url = signer.presign("PUT", "/media-cdn/img/a.png", 3600, &g.host);
        let res = g.raw_put(&url, b"bytes", &[]).await;

        assert_eq!(res.status_code(), 200);
        assert_eq!(g.object_row("media-cdn", "img/a.png").await.unwrap().size, 5);
    })
    .await;
}

/// The presigned permission is separate: a key that can read must not be able to mint URLs
/// unless it was given that too.
#[tokio::test]
#[serial]
async fn a_key_without_the_presigned_permission_is_refused() {
    with_gateway(|g| async move {
        let signer = g.key_with(&["read"], &[]).await;

        let url = signer.presign("GET", "/media-cdn/img/a.png", 3600, &g.host);
        let res = g.raw_get(&url, &[]).await;

        assert_eq!(res.status_code(), 403);
        assert!(res.text().contains("AccessDenied"));
        g.mock.assert_untouched();
    })
    .await;
}

/// test_presigned.py::test_presigned_url_for_one_key_does_not_open_another
#[tokio::test]
#[serial]
async fn a_url_for_one_key_does_not_open_another() {
    with_gateway(|g| async move {
        let signer = g.key_with(&["read", "presigned"], &[]).await;

        let url = signer.presign("GET", "/media-cdn/img/a.png", 3600, &g.host);
        let swapped = url.replace("/img/a.png", "/img/b.png");

        let res = g.raw_get(&swapped, &[]).await;

        assert_eq!(res.status_code(), 403);
        assert!(res.text().contains("SignatureDoesNotMatch"));
        g.mock.assert_untouched();
    })
    .await;
}

/// test_presigned.py::test_tampered_signature_is_rejected
#[tokio::test]
#[serial]
async fn a_tampered_signature_is_rejected() {
    with_gateway(|g| async move {
        let signer = g.key_with(&["read", "presigned"], &[]).await;

        let url = signer.presign("GET", "/media-cdn/img/a.png", 3600, &g.host);
        let tampered = tamper_last_hex_char(&url);

        let res = g.raw_get(&tampered, &[]).await;

        assert_eq!(res.status_code(), 403);
        g.mock.assert_untouched();
    })
    .await;
}

/// test_presigned.py::test_expired_presigned_url_is_refused
#[tokio::test]
#[serial]
async fn an_expired_url_is_refused() {
    with_gateway(|g| async move {
        let signer = g.key_with(&["read", "presigned"], &[]).await;

        let url = signer.presign_at("GET", "/media-cdn/img/a.png", 60, &g.host, hours_ago(2));
        let res = g.raw_get(&url, &[]).await;

        assert_eq!(res.status_code(), 403);
        g.mock.assert_untouched();
    })
    .await;
}

/// The prefix scope still applies: a presigned URL cannot be minted for a key outside it.
#[tokio::test]
#[serial]
async fn the_prefix_scope_still_applies_to_a_presigned_url() {
    with_gateway(|g| async move {
        let signer = g.key_with(&["read", "presigned"], &["img/"]).await;

        let url = signer.presign("GET", "/media-cdn/docs/secret.pdf", 3600, &g.host);
        let res = g.raw_get(&url, &[]).await;

        assert_eq!(res.status_code(), 403);
        g.mock.assert_untouched();
    })
    .await;
}

/// A revoked key's outstanding URLs stop working immediately: the signature is still valid, but
/// the key lookup is what refuses.
#[tokio::test]
#[serial]
async fn revoking_a_key_kills_its_outstanding_urls() {
    with_gateway(|g| async move {
        let signer = g.key_with(&["read", "presigned"], &[]).await;
        let url = signer.presign("GET", "/media-cdn/img/a.png", 3600, &g.host);

        g.revoke_key(&signer).await;

        let res = g.raw_get(&url, &[]).await;
        assert_eq!(res.status_code(), 403);
        assert!(res.text().contains("InvalidAccessKeyId"));
        g.mock.assert_untouched();
    })
    .await;
}
```

`revoking_a_key_kills_its_outstanding_urls` là tính chất mà một gateway phải có mà S3 thuần thì không: thu hồi key làm mọi URL đã phát chết ngay, vì gateway tra key mỗi lần. Không có test này thì dễ vô tình cache key và mất tính chất đó.

- [ ] **Step 2: Chạy để chắc nó fail, viết**

`S3Request::resolve` sửa bước 1:

```rust
/// Authenticates either form.
///
/// Header first: a request carrying both is signed by a client that meant the header, and the
/// query form exists for URLs handed to someone with no credentials at all.
async fn authenticate(ctx: &AppContext, parts: &Parts) -> Result<access_keys::Model, S3Error> {
    let presented = match sigv4::parse_authorization(&parts.headers) {
        Ok(p) => p,
        Err(_) => sigv4::parse_query(&query_pairs(parts))?,
    };
    // ...
    if presented.expires.is_some() {
        // Query form: the presigned permission is required on top of the verb's own action.
        if !key.permissions(&ctx.db).await?.iter().any(|p| p == ACTION_PRESIGNED) {
            return Err(S3Error::AccessDenied);
        }
        sigv4::check_expiry(&presented, Utc::now())?;
    }
    // ...
}
```

Quyền `presigned` kiểm **cộng thêm** action của verb: một URL GET đã ký cần cả `read` và `presigned`.

- [ ] **Step 3: Ba backend, nghiệm thu client thật, commit**

```bash
cargo test --test mod requests::s3::presigned 2>&1 | tail -5
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test 2>&1 | tail -3
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test 2>&1 | tail -3
```

Nghiệm thu bằng client thật — cả ba tính năng của G6:

```bash
export AWS_ACCESS_KEY_ID=OSG… AWS_SECRET_ACCESS_KEY=… H=http://localhost:5150

# multipart: aws-cli tự chuyển sang multipart khi file > 8 MB
head -c 104857600 /dev/urandom > /tmp/100mb.bin
aws s3 cp /tmp/100mb.bin s3://media-cdn/big/100mb.bin --endpoint-url $H
aws s3 cp s3://media-cdn/big/100mb.bin /tmp/back.bin --endpoint-url $H
cmp /tmp/100mb.bin /tmp/back.bin && echo "multipart round trip byte-identical"

# copy
aws s3 cp s3://media-cdn/big/100mb.bin s3://media-cdn/big/copy.bin --endpoint-url $H
aws s3api head-object --bucket media-cdn --key big/copy.bin --endpoint-url $H

# presigned: mở bằng curl, không credential
URL=$(aws s3 presign s3://media-cdn/big/copy.bin --expires-in 300 --endpoint-url $H)
curl -s -o /tmp/presigned.bin -w "%{http_code}\n" "$URL"
cmp /tmp/100mb.bin /tmp/presigned.bin && echo "presigned byte-identical"

# abort dở dang: cắt giữa upload rồi kiểm quota trả lại
```

Kiểm luôn quota sau khi xong: `used_bytes` phải bằng đúng 200 MB (hai object), `reserved_bytes` bằng 0. Nếu `reserved_bytes` khác 0 thì có hold rò — và nó chỉ lộ ra ở bước này, vì aws-cli upload 10 part song song còn test thì tuần tự.

```bash
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add src/ tests/ docs/
git commit -m "feat(s3): presigned URLs

The presigned permission is required on top of the verb's own action, so a key
that can read cannot mint URLs unless it was given that too. Revoking a key kills
its outstanding URLs immediately, because the key is looked up on every request —
a property a plain S3 bucket does not have, and one a cache would quietly remove.

Verified with aws s3 presign and curl: aws-cli uploads ten parts concurrently
where the tests are sequential, so it is the only check that catches a leaked
reservation."
```

---

## Task 6: ListParts và ListMultipartUploads

**Files:**
- Modify: `src/controllers/s3/multipart.rs`, `src/s3/xml.rs`
- Test: `tests/requests/s3/multipart.rs`

**Interfaces:**
- Consumes: task 1, 2.
- Produces: `multipart::list_parts`, `multipart::list_uploads`; `xml::list_multipart_uploads_result`.

- [ ] **Step 1: Viết test**

```rust
/// test_multipart.py::test_list_multipart_uploads_filters_by_prefix
/// Known to fail against MinIO (tests/s3/README.md:149); that is upstream, not the gateway.
#[tokio::test]
#[serial]
async fn list_uploads_filters_by_prefix_and_hides_upstream_ids() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        let a = g.start_upload(&signer, "img/one.bin").await;
        let b = g.start_upload(&signer, "docs/two.bin").await;

        let body = g.get(&signer, "/media-cdn?uploads&prefix=img/").await.text();

        assert!(body.contains("<ListMultipartUploadsResult"));
        assert!(body.contains("<Key>img/one.bin</Key>"));
        assert!(!body.contains("docs/two.bin"));
        assert!(body.contains(&a));
        assert!(!body.contains(&b));
        assert!(!body.contains("UPSTREAM"), "upstream UploadId leaked");
    })
    .await;
}

/// A scoped key sees only its own uploads.
#[tokio::test]
#[serial]
async fn a_scoped_key_lists_only_its_own_uploads() {
    with_gateway(|g| async move {
        let full = g.full_key().await;
        g.start_upload(&full, "docs/secret.bin").await;
        let scoped = g.scoped_key("img/").await;
        g.start_upload(&scoped, "img/mine.bin").await;

        let body = g.get(&scoped, "/media-cdn?uploads&prefix=img/").await.text();

        assert!(body.contains("img/mine.bin"));
        assert!(!body.contains("docs/secret.bin"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn list_parts_proxies_upstream() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        let upload = g.start_upload(&signer, "img/big.bin").await;
        g.mock.push(canned(
            200,
            br#"<ListPartsResult><Part><PartNumber>1</PartNumber><ETag>"p1"</ETag>
                <Size>300</Size></Part></ListPartsResult>"#,
        ));

        let body = g
            .get(&signer, &format!("/media-cdn/img/big.bin?uploadId={upload}"))
            .await
            .text();

        assert!(body.contains("<PartNumber>1</PartNumber>"));
        assert!(!body.contains("osg-main"));
    })
    .await;
}
```

- [ ] **Step 2: Viết, chạy ba backend, commit**

`list_uploads` đọc từ bảng, lọc prefix bằng cùng luật `may_list` của G5. `list_parts` proxy thuần nhưng phải **viết lại XML** để bỏ `<Bucket>` và `<Key>` vật lý nếu upstream trả — không forward mù.

```bash
cargo test 2>&1 | tail -3
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test 2>&1 | tail -3
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test 2>&1 | tail -3
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add src/ tests/
git commit -m "feat(s3): ListParts and ListMultipartUploads

ListMultipartUploads reads the mapping table and applies the same prefix rule as
listing, so a scoped key sees only its own uploads. ListParts proxies upstream but
rewrites the XML rather than forwarding it: an upstream ListPartsResult names the
physical bucket and key."
```

---

## Self-review

**Phủ spec.** Mục 10 (multipart) → task 1, 2, 3, 6. Mục 11 (copy) → task 4. Mục 5.3 (presigned) → task 5. Bảng mục 20: `test_multipart.py` (6) → task 2, 3, 4, 6; `test_copy.py` (6) → task 4; `test_presigned.py` (5) → task 5; ba case copy của `test_scoping.py` → task 4.

**Chưa phủ, cố ý.** Audit và background jobs → G7. `cleanup_multipart` cần `older_than` mà task 1 đã viết, nhưng task thì ở G7.

**Nhất quán kiểu.** `multipart_uploads::Model` khai task 1, dùng task 2, 3, 6 và G7. `quota::commit_amount` thêm ở task 3. `resolve_copy_source` từ G3 có caller đầu tiên ở task 4.

**Rủi ro đã biết.**

1. **`x-amz-copy-source` phải rewrite.** Quên thì triệu chứng là `NoSuchKey`, trông như object không tồn tại chứ không như một bug rewrite. Test đầu của task 4 khẳng định header đã gửi, không chỉ khẳng định status.
2. **Complete cần một HEAD thêm.** Nếu upstream Complete và HEAD không nhất quán ngay (một số store có độ trễ đọc-sau-ghi cho multipart) thì size sai. S3 và MinIO nhất quán ngay cho Complete; provider khác thì phải kiểm. Ghi vào `docs/docker.md` khi thêm provider.
3. **`upload_part_copy` reserve theo size của source, nhưng UploadPartCopy có thể lấy một range.** Header `x-amz-copy-source-range` giới hạn phần được copy. Nếu có range thì reserve theo độ dài range, không phải toàn object — bỏ sót là tính quota dư. Bộ conformance không kiểm, nên phải tự thêm test.
4. **Test presigned dựa vào `signer.presign` tự viết.** Bước nghiệm thu `aws s3 presign` là cái duy nhất bắt được cả signer test và verifier cùng sai.
