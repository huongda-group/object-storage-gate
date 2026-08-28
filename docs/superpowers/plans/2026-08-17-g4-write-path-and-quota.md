# G4 — Đường ghi và quota — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `PutObject`, `DeleteObject`, `DeleteObjects` chạy qua upstream, và quota được giữ **trước** khi byte nào rời gateway, commit sau khi upstream nhận xong, release khi hỏng.

**Architecture:** Tách `objects::Model::put_object` mà P5 ship thành `begin_put` → `PendingPut` → `commit`/`abort`, để upload upstream nằm được giữa reserve và commit. Một cơ chế quota, hai entry point, không đường nào tính tiền hai lần.

**Tech Stack:** Rust, axum 0.8, SeaORM 1.1, `quick-xml`.

**Spec:** `docs/superpowers/specs/2026-08-17-s3-gateway-design.md` mục 8

**Phụ thuộc:** G3 xong — `S3Request`, dispatch, `xml`, `TestGateway`.

**Deliverable:** `aws s3 cp file s3://bucket/key` chạy được, và một upload vượt quota bị từ chối **trước khi** upstream nhận byte nào.

## Global Constraints

- Ba backend hạng nhất: Postgres, MySQL >= 8.0.13, SQLite.
- **Quota mutation không lấy lock.** `reserve`/`commit`/`release` là một `UPDATE ... WHERE <guard>` cộng kiểm `rows_affected`. Advisory lock chỉ có ở Postgres và nằm ngoài phạm vi.
- Comment trong code: tiếng Anh, một câu một dòng.
- Không tự commit/push ngoài các bước commit trong plan. Không AI attribution.
- Sau mỗi task: clippy CI-strict sạch, test xanh trên cả ba backend.

---

## File Structure

**Sửa:**
- `src/models/objects.rs` — thêm `begin_put`, `PendingPut`, `record_put`; `put_object` thành wrapper
- `src/controllers/s3/object.rs` — thêm `put`, `delete`, `delete_objects`
- `src/controllers/s3/mod.rs` — nối nhánh dispatch
- `src/s3/xml.rs` — parse body `DeleteObjects`, dựng response `DeleteResult`
- `tests/models/quota.rs` — test cho `begin_put`
- `tests/requests/s3/write.rs` (mới)

---

## Task 1: Tách `begin_put` khỏi `put_object`

**Files:**
- Modify: `src/models/objects.rs`
- Test: `tests/models/quota.rs`

**Interfaces:**
- Consumes: `quota::reserve`/`commit`/`release`/`settle` (P5).
- Produces:
  - `objects::PendingPut { bucket_id, object_key, size, reservation: Option<quota::Reservation>, delta_bytes, delta_objects }`
  - `objects::Model::begin_put(db, bucket_id, key, size) -> ModelResult<PendingPut>`
  - `PendingPut::commit(self, db, etag, content_type) -> ModelResult<Model>`
  - `PendingPut::abort(self, db) -> ModelResult<()>`
  - `objects::Model::record_put(db, bucket_id, key, size, etag, content_type) -> ModelResult<Model>` — metadata thuần, không quota
  - `objects::Model::put_object` giữ chữ ký cũ, thành `begin_put` + `commit`

- [ ] **Step 1: Viết test**

Thêm vào `tests/models/quota.rs`:

```rust
/// The gateway needs the upstream upload to sit between reserve and commit. begin_put holds the
/// reservation without writing metadata, so a failed upload leaves nothing behind.
#[tokio::test]
#[serial]
async fn begin_put_reserves_without_writing_metadata() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (_user_id, bucket_id) = setup(db, 10_000, 1000).await;

    let pending = objects::Model::begin_put(db, bucket_id, "a.bin", 300)
        .await
        .unwrap();

    let b = bucket_of(db, bucket_id).await;
    assert_eq!(b.reserved_bytes, 300, "reservation must be held");
    assert_eq!(b.used_bytes, 0, "nothing committed yet");
    assert_eq!(b.object_count, 0);
    assert!(
        objects::Model::get(db, bucket_id, "a.bin").await.unwrap().is_none(),
        "no metadata row before the upload lands"
    );

    pending.commit(db, "etag-1", "application/octet-stream").await.unwrap();

    let b = bucket_of(db, bucket_id).await;
    assert_eq!(b.reserved_bytes, 0);
    assert_eq!(b.used_bytes, 300);
    assert_eq!(b.object_count, 1);
}

#[tokio::test]
#[serial]
async fn abort_releases_and_writes_nothing() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (_user_id, bucket_id) = setup(db, 10_000, 1000).await;

    let pending = objects::Model::begin_put(db, bucket_id, "a.bin", 300).await.unwrap();
    pending.abort(db).await.unwrap();

    let b = bucket_of(db, bucket_id).await;
    assert_eq!(b.reserved_bytes, 0);
    assert_eq!(b.used_bytes, 0);
    assert!(objects::Model::get(db, bucket_id, "a.bin").await.unwrap().is_none());
    assert_eq!(user_of(db).await.reserved_bytes, 0);
}

/// Over quota is refused at begin, before the caller has moved a byte.
#[tokio::test]
#[serial]
async fn begin_put_refuses_over_quota() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (_user_id, bucket_id) = setup(db, 10_000, 500).await;

    let refused = objects::Model::begin_put(db, bucket_id, "big.bin", 900).await;

    assert!(refused.is_err());
    assert!(refused.unwrap_err().to_string().contains("quota exceeded"));
    let b = bucket_of(db, bucket_id).await;
    assert_eq!(b.reserved_bytes, 0, "a refused begin must not leak a hold");
}

/// An overwrite charges the difference, and a shrink needs no reservation at all.
#[tokio::test]
#[serial]
async fn begin_put_charges_only_the_delta() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (_user_id, bucket_id) = setup(db, 10_000, 1000).await;

    objects::Model::put_object(db, bucket_id, "a.bin", 300, "e1", "text/plain")
        .await
        .unwrap();

    // Grow: reserve 200, not 500.
    let pending = objects::Model::begin_put(db, bucket_id, "a.bin", 500).await.unwrap();
    assert_eq!(bucket_of(db, bucket_id).await.reserved_bytes, 200);
    pending.commit(db, "e2", "text/plain").await.unwrap();
    assert_eq!(bucket_of(db, bucket_id).await.used_bytes, 500);

    // Shrink: no reservation, settled at commit.
    let pending = objects::Model::begin_put(db, bucket_id, "a.bin", 100).await.unwrap();
    assert_eq!(bucket_of(db, bucket_id).await.reserved_bytes, 0);
    pending.commit(db, "e3", "text/plain").await.unwrap();
    assert_eq!(bucket_of(db, bucket_id).await.used_bytes, 100);
    assert_eq!(bucket_of(db, bucket_id).await.object_count, 1);
}

/// put_object is now begin_put + commit; the behaviour P5 shipped must not change.
#[tokio::test]
#[serial]
async fn put_object_still_charges_exactly_once() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (_user_id, bucket_id) = setup(db, 10_000, 1000).await;

    objects::Model::put_object(db, bucket_id, "a.bin", 300, "e1", "text/plain")
        .await
        .unwrap();

    let b = bucket_of(db, bucket_id).await;
    assert_eq!(b.used_bytes, 300, "double-charged");
    assert_eq!(b.reserved_bytes, 0);
    assert_eq!(b.object_count, 1);
}

/// record_put is the multipart escape hatch: metadata only, quota untouched.
#[tokio::test]
#[serial]
async fn record_put_writes_metadata_and_leaves_quota_alone() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let (_user_id, bucket_id) = setup(db, 10_000, 1000).await;

    objects::Model::record_put(db, bucket_id, "a.bin", 300, "e1", "text/plain")
        .await
        .unwrap();

    assert!(objects::Model::get(db, bucket_id, "a.bin").await.unwrap().is_some());
    let b = bucket_of(db, bucket_id).await;
    assert_eq!(b.used_bytes, 0, "record_put must not touch quota; multipart owns it");
    assert_eq!(b.object_count, 0);
}
```

Test cuối là chỗ ghi lại bằng code cái đánh đổi mà spec mục 8 nói tới: `record_put` là một đường ghi không an toàn về quota, và test khẳng định nó **cố ý** như vậy chứ không phải quên.

- [ ] **Step 2: Chạy để chắc nó fail**

Run: `cargo test --test mod models::quota 2>&1 | tail -10`
Expected: FAIL biên dịch — `begin_put`, `record_put` chưa có.

- [ ] **Step 3: Viết**

```rust
/// A quota hold taken before an upload, waiting to become stored bytes.
///
/// The gateway needs the upstream upload between reserve and commit; a single `put_object` that
/// owns the whole sequence cannot express that. Holding the reservation in a value means a caller
/// that drops it without committing has still made an explicit choice, because `commit` and
/// `abort` both consume `self`.
pub struct PendingPut {
    bucket_id: i32,
    object_key: String,
    size: i64,
    reservation: Option<quota::Reservation>,
    delta_bytes: i64,
    delta_objects: i64,
}

impl Model {
    /// Holds quota for a write without touching metadata.
    ///
    /// Only a growing write needs a reservation; a shrink or a same-size overwrite settles at
    /// commit time.
    ///
    /// # Errors
    /// Returns a `quota exceeded` error when there is no room, or a DB error.
    pub async fn begin_put(
        db: &DatabaseConnection,
        bucket_id: i32,
        key: &str,
        size: i64,
    ) -> ModelResult<PendingPut> {
        let existing = Self::get(db, bucket_id, key).await?;
        let previous_size = existing.as_ref().map_or(0, |o| o.size);
        let delta_bytes = size - previous_size;
        let delta_objects = i64::from(existing.is_none());

        let reservation = if delta_bytes > 0 {
            Some(quota::reserve(db, bucket_id, delta_bytes).await?)
        } else {
            None
        };

        Ok(PendingPut {
            bucket_id,
            object_key: key.to_string(),
            size,
            reservation,
            delta_bytes,
            delta_objects,
        })
    }

    /// Writes metadata without touching quota.
    ///
    /// Only the multipart path may use this. Multipart accumulates its reservation across many
    /// `UploadPart` requests, so no `PendingPut` can hold it and `CompleteMultipartUpload` owns
    /// the accounting itself (spec §10).
    ///
    /// # Errors
    /// Returns an error on DB failure.
    pub async fn record_put(
        db: &DatabaseConnection,
        bucket_id: i32,
        key: &str,
        size: i64,
        etag: &str,
        content_type: &str,
    ) -> ModelResult<Self> {
        Self::write_row(db, bucket_id, key, size, etag, content_type).await
    }
}

impl PendingPut {
    /// Turns the hold into stored bytes once the upload has landed.
    ///
    /// # Errors
    /// Returns an error on DB failure.
    pub async fn commit(
        self,
        db: &DatabaseConnection,
        etag: &str,
        content_type: &str,
    ) -> ModelResult<Model> {
        let row = Model::write_row(
            db,
            self.bucket_id,
            &self.object_key,
            self.size,
            etag,
            content_type,
        )
        .await?;

        match self.reservation {
            Some(reservation) => quota::commit(db, &reservation, self.delta_objects).await?,
            None => {
                quota::settle(db, self.bucket_id, self.delta_bytes, self.delta_objects).await?;
            }
        }

        Ok(row)
    }

    /// Gives the hold back. Nothing was written, so there is nothing to undo.
    ///
    /// # Errors
    /// Returns an error on DB failure.
    pub async fn abort(self, db: &DatabaseConnection) -> ModelResult<()> {
        if let Some(reservation) = self.reservation {
            quota::release(db, &reservation).await?;
        }
        Ok(())
    }
}
```

Rồi `put_object` thành:

```rust
    /// Insert or overwrite, charging quota, in one call.
    ///
    /// For callers that have nothing to do between the reservation and the write. The gateway uses
    /// `begin_put` instead, because the upstream upload goes in that gap.
    ///
    /// # Errors
    /// Returns a `quota exceeded` error when there is no room, or a DB error.
    pub async fn put_object(
        db: &DatabaseConnection,
        bucket_id: i32,
        key: &str,
        size: i64,
        etag: &str,
        content_type: &str,
    ) -> ModelResult<Self> {
        Self::begin_put(db, bucket_id, key, size)
            .await?
            .commit(db, etag, content_type)
            .await
    }
```

`write_row` (bản hội tụ từ P3) đổi từ `async fn` private thành `pub(crate)` hoặc giữ private và để `record_put` gọi — giữ private là đúng, `record_put` ở cùng `impl`.

- [ ] **Step 4: Chạy test ba backend và commit**

```bash
cargo test --test mod models::quota 2>&1 | tail -5
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test 2>&1 | tail -3
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test 2>&1 | tail -3
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add src/ tests/
git commit -m "refactor(quota): split begin_put out of put_object

The gateway needs the upstream upload between reserve and commit, which a single
put_object owning the whole sequence cannot express. commit and abort both
consume PendingPut, so dropping a hold without deciding is not something that
happens by accident. put_object becomes begin_put + commit and keeps its
behaviour.

record_put is the one metadata writer that does not touch quota, because
multipart accumulates its reservation across many UploadPart requests and owns
the accounting itself. A test asserts that is deliberate rather than forgotten."
```

---

## Task 2: PutObject

**Files:**
- Modify: `src/controllers/s3/object.rs`, `src/controllers/s3/mod.rs`
- Test: `tests/requests/s3/write.rs`

**Interfaces:**
- Consumes: `S3Request` (G3), `upstream::Client` (G2), `begin_put` (task 1).
- Produces: `object::put`.

- [ ] **Step 1: Viết test**

Tạo `tests/requests/s3/write.rs`:

```rust
/// The happy path, and the physical key under test.
#[tokio::test]
#[serial]
async fn put_uploads_and_records_metadata() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(Canned {
            status: 200,
            headers: vec![("etag".into(), "\"abc123\"".into())],
            body: vec![],
        });

        let res = g.put(&signer, "/media-cdn/img/a.png", b"png bytes").await;

        assert_eq!(res.status_code(), 200);
        assert_eq!(header(&res, "etag"), "\"abc123\"");
        g.mock.assert_key(0, "osg-main/{user_pid}/media-cdn/img/a.png");
        assert_eq!(g.mock.requests()[0].body, b"png bytes");

        // Metadata records the upstream ETag verbatim, not a recomputed one.
        let row = g.object_row("media-cdn", "img/a.png").await.unwrap();
        assert_eq!(row.etag, "\"abc123\"");
        assert_eq!(row.size, 9);
    })
    .await;
}

/// The whole point of reserve-before-upload: an over-quota write must not move a byte.
#[tokio::test]
#[serial]
async fn an_over_quota_put_never_reaches_upstream() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.set_bucket_quota("media-cdn", 100).await;

        let res = g.put(&signer, "/media-cdn/big.bin", &vec![0u8; 500]).await;

        assert_eq!(res.status_code(), 403);
        assert!(res.text().contains("QuotaExceeded"));
        g.mock.assert_untouched();
        assert_eq!(g.bucket_row("media-cdn").await.reserved_bytes, 0);
    })
    .await;
}

/// A failed upload releases the hold; a leaked hold is a bucket that slowly refuses writes.
#[tokio::test]
#[serial]
async fn a_failed_upload_releases_the_reservation() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(canned(503, b"upstream unavailable"));

        let res = g.put(&signer, "/media-cdn/a.bin", b"bytes").await;

        assert_eq!(res.status_code(), 500);
        let b = g.bucket_row("media-cdn").await;
        assert_eq!(b.reserved_bytes, 0, "the hold leaked");
        assert_eq!(b.used_bytes, 0);
        assert!(g.object_row("media-cdn", "a.bin").await.is_none());
    })
    .await;
}

/// Content-Length is required: the reservation needs a size, and aws-chunked is 501 anyway.
#[tokio::test]
#[serial]
async fn a_put_without_content_length_is_411() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;

        let res = g.put_without_content_length(&signer, "/media-cdn/a.bin").await;

        assert_eq!(res.status_code(), 411);
        assert!(res.text().contains("MissingContentLength"));
        g.mock.assert_untouched();
    })
    .await;
}

/// test_object_crud.py::test_put_preserves_content_type_and_user_metadata
#[tokio::test]
#[serial]
async fn content_type_and_user_metadata_reach_upstream() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(Canned {
            status: 200,
            headers: vec![("etag".into(), "\"e\"".into())],
            body: vec![],
        });

        g.put_with(
            &signer,
            "/media-cdn/a.png",
            b"bytes",
            &[
                ("content-type", "image/png"),
                ("x-amz-meta-owner", "team-a"),
                ("cache-control", "max-age=3600"),
            ],
        )
        .await;

        let seen = &g.mock.requests()[0];
        assert_eq!(header_of(seen, "content-type"), "image/png");
        assert_eq!(header_of(seen, "x-amz-meta-owner"), "team-a");
        assert_eq!(header_of(seen, "cache-control"), "max-age=3600");
    })
    .await;
}

/// test_object_crud.py::test_put_overwrites_in_place
#[tokio::test]
#[serial]
async fn an_overwrite_keeps_one_row_and_charges_the_delta() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(etag_ok("\"e1\""));
        g.mock.push(etag_ok("\"e2\""));

        g.put(&signer, "/media-cdn/a.bin", &vec![0u8; 300]).await;
        g.put(&signer, "/media-cdn/a.bin", &vec![0u8; 500]).await;

        let b = g.bucket_row("media-cdn").await;
        assert_eq!(b.used_bytes, 500);
        assert_eq!(b.object_count, 1);
        assert_eq!(g.object_row("media-cdn", "a.bin").await.unwrap().etag, "\"e2\"");
    })
    .await;
}
```

- [ ] **Step 2: Chạy để chắc nó fail**

Run: `cargo test --test mod requests::s3::write 2>&1 | tail -10`
Expected: FAIL — `PUT` trả 501.

- [ ] **Step 3: Viết handler**

```rust
/// Headers forwarded from the client up to the store on a write.
/// A whitelist, not a blacklist: an unknown header can change how the store treats the object.
const FORWARD_ON_WRITE: &[&str] = &[
    "content-type",
    "content-encoding",
    "content-disposition",
    "content-language",
    "cache-control",
    "expires",
];
```

`x-amz-meta-*` khớp theo tiền tố. `x-amz-` khác thì **không** chuyển tiếp: `x-amz-acl`, `x-amz-server-side-encryption`, `x-amz-storage-class` đều thay đổi hành vi của store theo cách gateway không kiểm soát, và spec mục 19 đã ghi ACL/SSE nằm ngoài phạm vi. Chuyển tiếp mù một cái trong đó là để client đặt ACL public trên object của họ trong physical bucket dùng chung — tức là mở dữ liệu của họ ra Internet qua một đường gateway không biết tới.

Thứ tự trong handler:

```
1. resolve(action = ACTION_WRITE)
2. content_length từ header, thiếu -> MissingContentLength
3. x-amz-content-sha256 == STREAMING-... -> NotImplemented
4. begin_put(bucket.id, logical_key, len)   -> QuotaExceeded
5. upstream PUT với body stream
6. Ok(resp)  -> etag từ resp; pending.commit(db, etag, content_type)
   Err(e)    -> pending.abort(db); trả e
```

Bước 3 phải nằm **trước** bước 4: không giữ quota cho một request mình sắp từ chối.

- [ ] **Step 4: Chạy test và commit**

```bash
cargo test --test mod requests::s3::write 2>&1 | tail -5
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add src/ tests/
git commit -m "feat(s3): PutObject with quota held before the upload

The reservation is taken before any byte leaves the gateway and released if the
upload fails, so an over-quota write never reaches the store and a failure never
leaves a hold behind — a leaked hold is a bucket that slowly stops accepting
writes for no visible reason.

x-amz-acl, x-amz-server-side-encryption and x-amz-storage-class are not
forwarded. Passing one through would let a client set a public ACL on an object
inside the shared physical bucket, opening their data over a path the gateway
knows nothing about."
```

---

## Task 3: DeleteObject và DeleteObjects

**Files:**
- Modify: `src/controllers/s3/object.rs`, `src/controllers/s3/mod.rs`, `src/s3/xml.rs`
- Test: `tests/requests/s3/write.rs`

**Interfaces:**
- Consumes: `S3Request`, `objects::Model::delete` (P5), `upstream::Client`.
- Produces: `object::delete`, `object::delete_objects`; `xml::parse_delete_request`, `xml::delete_result`.

- [ ] **Step 1: Viết test**

```rust
/// test_object_crud.py::test_delete_object_is_idempotent
#[tokio::test]
#[serial]
async fn delete_is_idempotent_and_credits_the_quota() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(etag_ok("\"e1\""));
        g.put(&signer, "/media-cdn/a.bin", &vec![0u8; 300]).await;
        assert_eq!(g.bucket_row("media-cdn").await.used_bytes, 300);

        g.mock.push(canned(204, b""));
        let res = g.delete(&signer, "/media-cdn/a.bin").await;
        assert_eq!(res.status_code(), 204);
        assert!(res.text().is_empty());

        let b = g.bucket_row("media-cdn").await;
        assert_eq!(b.used_bytes, 0);
        assert_eq!(b.object_count, 0);

        // Again: still 204, and the counters do not go negative.
        g.mock.push(canned(204, b""));
        let res = g.delete(&signer, "/media-cdn/a.bin").await;
        assert_eq!(res.status_code(), 204);
        let b = g.bucket_row("media-cdn").await;
        assert_eq!(b.used_bytes, 0);
        assert_eq!(b.object_count, 0);
    })
    .await;
}

/// test_object_crud.py::test_delete_objects_verbose_reports_every_key
#[tokio::test]
#[serial]
async fn delete_objects_reports_every_key() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        for _ in 0..3 {
            g.mock.push(etag_ok("\"e\""));
        }
        for k in ["a.bin", "b.bin", "c.bin"] {
            g.put(&signer, &format!("/media-cdn/{k}"), b"x").await;
        }

        g.mock.push(canned(200, br#"<DeleteResult/>"#));
        let res = g
            .post_delete(&signer, "/media-cdn", &["a.bin", "b.bin", "c.bin"], false)
            .await;

        assert_eq!(res.status_code(), 200);
        let body = res.text();
        for k in ["a.bin", "b.bin", "c.bin"] {
            assert!(body.contains(&format!("<Key>{k}</Key>")), "missing {k}");
        }
        assert!(body.contains("<Deleted>"));
        assert_eq!(g.bucket_row("media-cdn").await.object_count, 0);
    })
    .await;
}

/// test_object_crud.py::test_delete_objects_quiet_omits_the_deleted_list
#[tokio::test]
#[serial]
async fn delete_objects_quiet_omits_the_deleted_list() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(etag_ok("\"e\""));
        g.put(&signer, "/media-cdn/a.bin", b"x").await;

        g.mock.push(canned(200, br#"<DeleteResult/>"#));
        let res = g.post_delete(&signer, "/media-cdn", &["a.bin"], true).await;

        assert_eq!(res.status_code(), 200);
        assert!(!res.text().contains("<Deleted>"));
    })
    .await;
}

/// A key outside the prefix becomes one <Error> entry, not a 403 for the whole batch — that is
/// S3's batch semantics, and a whole-request refusal would make one bad key undo 999 good ones.
#[tokio::test]
#[serial]
async fn a_key_outside_the_prefix_becomes_an_error_entry() {
    with_gateway(|g| async move {
        let signer = g.scoped_key("img/").await;
        g.mock.push(etag_ok("\"e\""));
        g.put(&signer, "/media-cdn/img/a.png", b"x").await;

        g.mock.push(canned(200, br#"<DeleteResult/>"#));
        let res = g
            .post_delete(&signer, "/media-cdn", &["img/a.png", "docs/b.pdf"], false)
            .await;

        assert_eq!(res.status_code(), 200);
        let body = res.text();
        assert!(body.contains("<Deleted>"));
        assert!(body.contains("<Key>img/a.png</Key>"));
        assert!(body.contains("<Error>"));
        assert!(body.contains("<Code>AccessDenied</Code>"));
        assert!(body.contains("<Key>docs/b.pdf</Key>"));

        // The denied key must not appear in the upstream request at all.
        let sent = String::from_utf8_lossy(&g.mock.requests().last().unwrap().body).to_string();
        assert!(!sent.contains("docs/b.pdf"), "denied key was sent upstream: {sent}");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn more_than_a_thousand_keys_is_malformed_xml() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        let keys: Vec<String> = (0..1001).map(|i| format!("k{i}")).collect();
        let refs: Vec<&str> = keys.iter().map(String::as_str).collect();

        let res = g.post_delete(&signer, "/media-cdn", &refs, false).await;

        assert_eq!(res.status_code(), 400);
        assert!(res.text().contains("MalformedXML"));
        g.mock.assert_untouched();
    })
    .await;
}
```

Khẳng định cuối của test thứ tư là cái quan trọng: key bị từ chối **không** được lọt vào request gửi upstream. Một implementation authorize xong rồi vẫn gửi cả danh sách sẽ pass mọi assert về response mà vẫn xoá mất dữ liệu ngoài phạm vi.

- [ ] **Step 2: Chạy để chắc nó fail**

Run: `cargo test --test mod requests::s3::write 2>&1 | tail -10`

- [ ] **Step 3: Viết**

`xml::parse_delete_request(body: &[u8]) -> Result<(Vec<String>, bool), S3Error>` — trả `(keys, quiet)`, và `MalformedXml` khi > 1000 key hoặc XML sai.

`xml::delete_result(deleted: &[String], errors: &[(String, S3Error)], quiet: bool) -> String`.

Handler:

```
1. resolve_bucket_only  -> bucket + pool  (không có key ở path)
2. parse body           -> keys, quiet
3. authorize từng key:
     validate_logical_key + key.allows_key + có ACTION_DELETE
     fail -> vào danh sách errors, KHÔNG vào danh sách gửi upstream
4. rewrite các key được phép -> upstream DeleteObjects
5. objects::delete cho từng key đã xoá (credit quota)
6. dựng DeleteResult
```

Bước 1 dùng `resolve_bucket_only` nhưng vẫn cần kiểm `ACTION_DELETE` trên key — nên thêm một tham số hoặc kiểm tay ngay sau. Kiểm tay và ghi comment tại sao: `DeleteObjects` không có một key duy nhất để `resolve` kiểm prefix, nên authorize xảy ra theo từng phần tử ở bước 3.

- [ ] **Step 4: Chạy ba backend và commit**

```bash
cargo test 2>&1 | tail -3
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test 2>&1 | tail -3
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test 2>&1 | tail -3
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add src/ tests/
git commit -m "feat(s3): DeleteObject and DeleteObjects

Delete is idempotent and credits the quota. In a batch delete, a key outside the
key's prefix becomes one <Error> entry rather than a 403 for the whole request —
that is S3's batch semantics, and refusing the request would let one bad key undo
999 good ones. A test asserts the denied key never appears in the upstream body:
an implementation that authorises and then sends the whole list passes every
response assertion while still deleting data out of scope."
```

---

## Task 4: Nghiệm thu với client thật

**Files:** không sửa code; đây là bước kiểm.

- [ ] **Step 1: Dựng upstream MinIO**

```bash
docker run -d --name osg-upstream -p 9000:9000 \
  -e MINIO_ROOT_USER=upstream -e MINIO_ROOT_PASSWORD=upstream-secret \
  quay.io/minio/minio server /data
docker exec osg-upstream mc alias set local http://localhost:9000 upstream upstream-secret
docker exec osg-upstream mc mb local/osg-main
```

- [ ] **Step 2: Cấu hình qua console**

Pool → `provider=minio`, `api_endpoint=http://localhost:9000`, `physical_bucket=osg-main`, credential của MinIO. Bucket `media-cdn` quota 1 GiB. Access key có `read`, `write`, `delete`, `list`.

- [ ] **Step 3: Vòng ghi–đọc–xoá bằng aws-cli**

```bash
export AWS_ACCESS_KEY_ID=OSG…
export AWS_SECRET_ACCESS_KEY=…
export H=http://localhost:5150

head -c 1048576 /dev/urandom > /tmp/1mb.bin
aws s3 cp /tmp/1mb.bin s3://media-cdn/img/1mb.bin --endpoint-url $H
aws s3 cp s3://media-cdn/img/1mb.bin /tmp/back.bin  --endpoint-url $H
cmp /tmp/1mb.bin /tmp/back.bin && echo "round trip byte-identical"
aws s3 rm s3://media-cdn/img/1mb.bin --endpoint-url $H
```

Rồi kiểm physical layout đúng như spec:

```bash
docker exec osg-upstream mc ls --recursive local/osg-main
```

Expected: đường dẫn dạng `{user_pid}/media-cdn/img/1mb.bin`. Nếu nó là `media-cdn/img/1mb.bin` (thiếu `user_pid`) thì hai user cùng đặt tên bucket sẽ ghi đè nhau — bug nặng nhất mà bước này bắt được.

- [ ] **Step 4: Quota thật**

```bash
# đặt quota bucket còn 1 MiB qua console, rồi:
head -c 5242880 /dev/urandom > /tmp/5mb.bin
aws s3 cp /tmp/5mb.bin s3://media-cdn/big.bin --endpoint-url $H
```

Expected: từ chối. Và `mc ls` phải **không** thấy `big.bin` — nếu thấy thì reserve chạy sau upload.

- [ ] **Step 5: Dọn và ghi lại**

```bash
docker rm -f osg-upstream
```

Thêm vào `docs/docker.md` một mục ngắn "Kiểm gateway cục bộ bằng MinIO" với đúng các lệnh trên, để lần sau không phải dựng lại từ đầu.

- [ ] **Step 6: Commit**

```bash
git add docs/
git commit -m "docs: how to exercise the gateway against a local MinIO"
```

---

## Self-review

**Phủ spec.** Mục 8 (chỗ sửa P5) → task 1. Mục 8.1 (PutObject) → task 2. Mục 8.3 (Delete, DeleteObjects) → task 3. Mục 8.4 (QuotaExceeded) → task 2.

**Chưa phủ, cố ý.** Listing → G5. Multipart, Copy, Presigned → G6. Audit → G7.

**Nhất quán kiểu.** `PendingPut` khai task 1, dùng task 2 và G6. `record_put` khai task 1, dùng G6. `xml::parse_delete_request`/`delete_result` khai task 3.

**Rủi ro đã biết.**

1. **Danh sách trắng `x-amz-*` khi ghi.** Không chuyển tiếp `x-amz-acl` là quyết định bảo mật, nhưng nó cũng nghĩa là một client đặt `x-amz-acl: private` sẽ không thấy lỗi, chỉ là header bị bỏ. S3 thật thì nhận. Đây là một khác biệt hành vi, và bộ conformance không có test cho nó — ghi vào spec mục 19 nếu chưa có.
2. **`objects::delete` chạy sau upstream DELETE.** Nếu upstream xoá xong rồi gateway chết trước khi cập nhật metadata thì row còn lại và quota tính dư. `reconcile_quota` sửa. Không đổi thứ tự: xoá metadata trước rồi upstream fail thì mất dấu object vẫn tồn tại — tệ hơn.
3. **`DeleteObjects` gửi một request upstream cho cả lô.** Nếu upstream trả lỗi từng phần thì phải parse `<Error>` của nó và không credit quota cho những key đó. Test hiện chưa phủ trường hợp upstream xoá được một phần — thêm một test nữa nếu bộ conformance ở G7 lộ ra.
