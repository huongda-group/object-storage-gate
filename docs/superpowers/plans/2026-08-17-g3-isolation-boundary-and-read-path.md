# G3 — Biên giới cách ly và đường đọc — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `S3Request::resolve` giải xong auth → authorize → prefix rewrite tại đúng một chỗ; cây route S3 và dispatch theo query đứng được; `GetObject` và `HeadObject` chạy end-to-end qua upstream giả.

**Architecture:** `resolve` là constructor duy nhất của `S3Request`, và mọi hàm `upstream.rs` đòi `&S3Request` — nên không có rewrite thì không có gì để gọi upstream. `dispatch()` gọi `resolve` một lần rồi phân nhánh theo query; nó là chỗ duy nhất thấy được cả auth-fail lẫn kết quả, nên G7 cắm audit vào đúng đây.

**Tech Stack:** Rust, axum 0.8, loco-rs 0.16, `quick-xml`.

**Spec:** `docs/superpowers/specs/2026-08-17-s3-gateway-design.md` mục 6, 7, 8.2, 12

**Deliverable:** `aws s3 cp s3://bucket/key -` chạy được qua gateway với một upstream thật, và 13 test `test_scoping.py` phần read có bản Rust tương đương dùng upstream giả.

## Global Constraints

- Ba backend hạng nhất: Postgres, MySQL >= 8.0.13, SQLite.
- Comment trong code: tiếng Anh, một câu một dòng.
- Không tự commit/push ngoài các bước commit trong plan. Không AI attribution.
- Sau mỗi task: `cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms` sạch, test xanh trên cả ba backend.
- **Không handler nào được tự dựng physical key.** Chỉ `S3Request::resolve` làm việc đó. Một handler nối chuỗi lấy path là một lỗ rò chéo tenant.

---

## File Structure

**Tạo mới:**
- `src/s3/request.rs` — `S3Request`, `resolve`, `resolve_bucket_only`, `resolve_copy_source`
- `src/s3/xml.rs` — error XML, và khung dựng response
- `src/controllers/s3/mod.rs` — cây route + dispatch
- `src/controllers/s3/object.rs` — `GetObject`, `HeadObject`
- `tests/requests/s3/mod.rs`, `tests/requests/s3/scoping.rs`, `tests/requests/s3/read.rs`
- `tests/support/signer.rs` — ký request trong test như một client S3

**Sửa:**
- `src/s3/mod.rs`, `src/controllers/mod.rs`, `src/app.rs`
- `src/models/access_keys.rs` — thêm `find_by_access_key_id`, sửa luật khớp prefix
- `tests/requests/mod.rs`

---

## Task 1: Luật khớp prefix và tra key

**Files:**
- Modify: `src/models/access_keys.rs`
- Test: `tests/models/access_keys.rs`

**Interfaces:**
- Consumes: —
- Produces:
  - `access_keys::prefix_allows(prefix: &str, key: &str) -> bool`
  - `access_keys::Model::find_by_access_key_id(db, &str) -> ModelResult<Model>`
  - `access_keys::Model::allows_key(&self, db, key: &str) -> ModelResult<bool>`

Làm trước vì `resolve` dựa vào nó, và vì nó sửa một finding của P3 chưa đóng.

- [ ] **Step 1: Viết test**

Thêm vào `tests/models/access_keys.rs`:

```rust
/// P3 flagged this and left it open: prefix `team` also authorised `teamsecret/`, so a key
/// handed to one team could read another team's folder.
#[test]
fn prefix_matching_respects_the_separator() {
    use object_storage_gate::models::access_keys::prefix_allows;

    // Inside.
    assert!(prefix_allows("img/", "img/a.png"));
    assert!(prefix_allows("img/", "img/nested/a.png"));
    assert!(prefix_allows("img", "img"));
    assert!(prefix_allows("img", "img/a.png"));

    // The bug.
    assert!(!prefix_allows("team", "teamsecret/x"));
    assert!(!prefix_allows("img", "imgsecret/a.png"));

    // Not a prefix at all.
    assert!(!prefix_allows("img/", "docs/a.png"));
    assert!(!prefix_allows("img/", "im"));
}

#[tokio::test]
#[serial]
async fn a_key_with_no_prefixes_allows_everything() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(db, "user1@example.com").await.unwrap();
    let (key, _) = access_keys::Model::create_key(
        db,
        user.id,
        &access_keys::CreateKeyParams {
            label: "primary".to_string(),
            expires_at: None,
            permissions: vec![access_keys::ACTION_READ.to_string()],
            prefixes: vec![],
        },
    )
    .await
    .unwrap();

    assert!(key.allows_key(db, "anything/at/all").await.unwrap());
}

#[tokio::test]
#[serial]
async fn a_scoped_key_allows_only_its_folders()  {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(db, "user1@example.com").await.unwrap();
    let (key, _) = access_keys::Model::create_key(
        db,
        user.id,
        &access_keys::CreateKeyParams {
            label: "readonly".to_string(),
            expires_at: None,
            permissions: vec![access_keys::ACTION_READ.to_string()],
            prefixes: vec!["img/".to_string(), "docs/".to_string()],
        },
    )
    .await
    .unwrap();

    assert!(key.allows_key(db, "img/a.png").await.unwrap());
    assert!(key.allows_key(db, "docs/b.pdf").await.unwrap());
    assert!(!key.allows_key(db, "backup/c.tar").await.unwrap());
}

#[tokio::test]
#[serial]
async fn find_by_access_key_id_ignores_revoked_and_expired() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(db, "user1@example.com").await.unwrap();
    let (key, _) = access_keys::Model::create_key(
        db,
        user.id,
        &access_keys::CreateKeyParams {
            label: "primary".to_string(),
            expires_at: None,
            permissions: vec![access_keys::ACTION_READ.to_string()],
            prefixes: vec![],
        },
    )
    .await
    .unwrap();
    let id = key.access_key_id.clone();

    // Found while active.
    assert!(access_keys::Model::find_by_access_key_id(db, &id).await.is_ok());

    key.revoke(db).await.unwrap();

    // Still a row, but the lookup must not hand it back: a revoked credential does not exist.
    assert!(access_keys::Model::find_by_access_key_id(db, &id).await.is_err());
}
```

- [ ] **Step 2: Chạy để chắc nó fail**

Run: `cargo test --test mod models::access_keys 2>&1 | tail -10`
Expected: FAIL — `prefix_allows`, `allows_key`, `find_by_access_key_id` chưa có.

- [ ] **Step 3: Viết**

Trong `src/models/access_keys.rs`, phạm vi module:

```rust
/// Whether `prefix` authorises `key`.
///
/// A prefix must land on a path boundary. Without that rule a key scoped to `team` also
/// authorises `teamsecret/`, which is a different tenant's folder as far as the person who
/// issued the key is concerned.
#[must_use]
pub fn prefix_allows(prefix: &str, key: &str) -> bool {
    key.starts_with(prefix)
        && (prefix.ends_with('/')
            || key.len() == prefix.len()
            || key.as_bytes()[prefix.len()] == b'/')
}
```

Trong `impl Model`:

```rust
    /// Finds an access key by the public id a client presents, but only while it is usable.
    ///
    /// A revoked, disabled or expired key must read as absent: that is a credential-validity
    /// question, not an authorisation one, and one answer for all three does not confirm to the
    /// caller whether the key exists.
    ///
    /// # Errors
    /// Returns an error when no usable key has that id, or on DB failure.
    pub async fn find_by_access_key_id(
        db: &DatabaseConnection,
        access_key_id: &str,
    ) -> ModelResult<Self> {
        let key = Entity::find()
            .filter(Column::AccessKeyId.eq(access_key_id))
            .one(db)
            .await?
            .ok_or(ModelError::EntityNotFound)?;

        if key.effective_status() == KEY_ACTIVE {
            Ok(key)
        } else {
            Err(ModelError::EntityNotFound)
        }
    }

    /// Whether this key's prefix policy authorises `key`.
    /// A key with no prefixes is scoped to the whole bucket.
    ///
    /// # Errors
    /// Returns an error on DB failure.
    pub async fn allows_key(&self, db: &DatabaseConnection, key: &str) -> ModelResult<bool> {
        let prefixes = self.prefixes(db).await?;
        if prefixes.is_empty() {
            return Ok(true);
        }
        Ok(prefixes.iter().any(|p| prefix_allows(p, key)))
    }
```

Kiểm `effective_status()` đã tồn tại và trả `KEY_ACTIVE`/`KEY_DISABLED`/`KEY_REVOKED`/`KEY_EXPIRED`:

```bash
grep -n "effective_status" -A12 src/models/access_keys.rs
```

- [ ] **Step 4: Chạy test và commit**

```bash
cargo test --test mod models::access_keys 2>&1 | tail -5
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test --test mod models::access_keys 2>&1 | tail -3
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test --test mod models::access_keys 2>&1 | tail -3
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add src/ tests/
git commit -m "fix(keys): prefix matching must land on a path boundary

P3 flagged this and left it open: prefix 'team' also authorised 'teamsecret/',
which is a different tenant's folder as far as whoever issued the key is
concerned. Adds find_by_access_key_id, which treats a revoked, disabled or
expired key as absent — one answer for all three, so the caller cannot learn
whether the key exists."
```

---

## Task 2: `S3Request::resolve`

**Files:**
- Create: `src/s3/request.rs`
- Modify: `src/s3/mod.rs`
- Test: `tests/requests/s3/scoping.rs`, `tests/support/signer.rs`

**Interfaces:**
- Consumes: `sigv4` (G2), `S3Error` (G2), `access_keys::find_by_access_key_id` + `allows_key` (task 1), `buckets::find_by_user_and_name`, `pools::find_by_id` (G1).
- Produces:
  - `S3Request { key, user, bucket, pool, logical_key, physical_key }`
  - `S3Request::resolve(ctx, parts, action) -> Result<Self, S3Error>`
  - `S3Request::resolve_bucket_only(ctx, parts) -> Result<Self, S3Error>`
  - `S3Request::resolve_copy_source(&self, ctx, header) -> Result<PhysicalRef, S3Error>`
  - `PhysicalRef { bucket: buckets::Model, logical_key: String, physical_key: String }`

- [ ] **Step 1: Viết signer cho test**

Tạo `tests/support/signer.rs` — không có nó thì không viết được test nào cho `resolve`.

```rust
//! Signs a request the way a real S3 client does, so tests exercise the verifier rather than a
//! shortcut around it.
//!
//! Deliberately not built on `sigv4::canonical_request`: a test signer that shares the
//! implementation under test agrees with it even when both are wrong. This one follows the AWS
//! documentation directly.
pub struct TestSigner {
    pub access_key_id: String,
    pub secret: String,
    pub region: String,
}

impl TestSigner {
    /// Returns the headers a signed request carries: `authorization`, `x-amz-date`,
    /// `x-amz-content-sha256`, `host`.
    pub fn sign(
        &self,
        method: &str,
        path: &str,
        query: &[(&str, &str)],
        host: &str,
        payload_hash: &str,
    ) -> Vec<(String, String)> { /* ... */ }

    /// Same, with a deliberately wrong signature — for the SignatureDoesNotMatch path.
    pub fn sign_tampered(&self, /* … */) -> Vec<(String, String)> { /* ... */ }

    /// Signs at a given instant, for the clock-skew tests.
    pub fn sign_at(&self, at: chrono::DateTime<chrono::Utc>, /* … */) -> Vec<(String, String)> { /* ... */ }

    /// Builds a presigned query string.
    pub fn presign(&self, method: &str, path: &str, expires_secs: u64, host: &str) -> String { /* ... */ }
}
```

Lý do không dùng chung code với `sigv4.rs` được ghi ngay trong doc-comment: một test signer dùng chung implementation với thứ đang được test thì đồng ý với nó cả khi cả hai đều sai. Bộ vector AWS ở G2 canh `canonical_request`; signer này canh việc verify lắp đúng vào request thật.

- [ ] **Step 2: Viết test cách ly**

Tạo `tests/requests/s3/scoping.rs`. Đây là bộ test quan trọng nhất của cả G3–G7.

```rust
//! The isolation boundary. Mirrors tests/s3/test_scoping.py, but with a mock upstream — so each
//! test can assert the stronger property: the store was never touched.

/// A scoped key reading inside its folder reaches upstream with the rewritten key.
#[tokio::test]
#[serial]
async fn a_scoped_key_reads_inside_its_folder() {
    with_gateway(|g| async move {
        let signer = g.scoped_key("img/").await;
        g.mock.push(ok_body(b"png bytes"));

        let res = g.get(&signer, "/media-cdn/img/a.png").await;

        assert_eq!(res.status_code(), 200);
        g.mock.assert_key(0, "osg-main/{user_pid}/media-cdn/img/a.png");
    })
    .await;
}

/// Eight verbs, all outside the prefix, none of which may reach the store.
///
/// The assertion that matters is `assert_untouched`, not the 403: a gateway that calls upstream
/// and only then refuses has already let the request cross the boundary, and a status-only
/// assertion cannot tell the two apart.
#[tokio::test]
#[serial]
async fn a_scoped_key_cannot_touch_anything_outside() {
    with_gateway(|g| async move {
        let signer = g.scoped_key("img/").await;

        for (method, path) in [
            ("GET",    "/media-cdn/docs/a.pdf"),
            ("HEAD",   "/media-cdn/docs/a.pdf"),
            ("PUT",    "/media-cdn/docs/a.pdf"),
            ("DELETE", "/media-cdn/docs/a.pdf"),
        ] {
            let res = g.request(&signer, method, path).await;
            assert_eq!(res.status_code(), 403, "{method} {path}");
            assert!(res.text().contains("AccessDenied") || method == "HEAD");
        }

        g.mock.assert_untouched();
    })
    .await;
}

/// The separator rule from task 1, end to end.
#[tokio::test]
#[serial]
async fn a_key_scoped_to_img_cannot_read_imgsecret() {
    with_gateway(|g| async move {
        let signer = g.scoped_key("img").await;

        let res = g.get(&signer, "/media-cdn/imgsecret/a.png").await;

        assert_eq!(res.status_code(), 403);
        g.mock.assert_untouched();
    })
    .await;
}

/// Another user's bucket reads as absent, not as forbidden — the same posture the management
/// API takes, and it does not confirm the bucket exists.
#[tokio::test]
#[serial]
async fn another_users_bucket_is_no_such_bucket() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.other_user_bucket("their-private").await;

        let res = g.get(&signer, "/their-private/a.png").await;

        assert_eq!(res.status_code(), 404);
        assert!(res.text().contains("NoSuchBucket"));
        g.mock.assert_untouched();
    })
    .await;
}

/// Path traversal must be refused before the rewrite, not normalised into it.
#[tokio::test]
#[serial]
async fn a_key_containing_dotdot_is_refused() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;

        for key in ["../other/a.png", "img/../../escape", "a/../../../etc/passwd"] {
            let res = g.get(&signer, &format!("/media-cdn/{key}")).await;
            assert_eq!(res.status_code(), 400, "key {key}");
            assert!(res.text().contains("InvalidArgument"));
        }

        g.mock.assert_untouched();
    })
    .await;
}

/// A key missing the action is refused even inside its prefix.
#[tokio::test]
#[serial]
async fn a_read_only_key_cannot_write_inside_its_prefix() {
    with_gateway(|g| async move {
        let signer = g.key_with(&["read"], &["img/"]).await;

        let res = g.put(&signer, "/media-cdn/img/a.png", b"bytes").await;

        assert_eq!(res.status_code(), 403);
        g.mock.assert_untouched();
    })
    .await;
}

/// Auth failures, all four shapes. tests/s3/test_auth.py has the same five.
#[tokio::test]
#[serial]
async fn auth_failures_map_to_the_right_codes() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;

        // Unsigned.
        let res = g.raw_get("/media-cdn/a.png", &[]).await;
        assert_eq!(res.status_code(), 403);
        assert!(res.text().contains("AccessDenied"));

        // Unknown key id.
        let unknown = signer.with_id("OSGDOESNOTEXIST0000");
        let res = g.get(&unknown, "/media-cdn/a.png").await;
        assert!(res.text().contains("InvalidAccessKeyId"));

        // Wrong secret.
        let wrong = signer.with_secret("not-the-secret");
        let res = g.get(&wrong, "/media-cdn/a.png").await;
        assert!(res.text().contains("SignatureDoesNotMatch"));

        // Clock far outside the window.
        let res = g.get_at(&signer, "/media-cdn/a.png", hours_ago(3)).await;
        assert!(res.text().contains("RequestTimeTooSkewed"));

        // A revoked key reads as unknown, not as denied.
        g.revoke_key(&signer).await;
        let res = g.get(&signer, "/media-cdn/a.png").await;
        assert!(res.text().contains("InvalidAccessKeyId"));

        g.mock.assert_untouched();
    })
    .await;
}
```

`with_gateway` là một harness dựng: app test + `MockUpstream` + một pool trỏ vào mock + một user + một bucket. Đặt nó trong `tests/requests/s3/mod.rs` cùng `TestGateway` với các helper `full_key`, `scoped_key`, `key_with`, `get`, `put`, `request`, `raw_get`, `get_at`, `revoke_key`, `other_user_bucket`.

Bỏ công dựng harness này ở đây là đúng chỗ: G4–G7 mỗi plan thêm hàng chục test dùng nó.

- [ ] **Step 3: Chạy để chắc nó fail**

Run: `cargo test --test mod requests::s3 2>&1 | tail -10`
Expected: FAIL biên dịch.

- [ ] **Step 4: Viết `S3Request`**

```rust
//! The isolation boundary.
//!
//! `resolve` is the only constructor, and every function in `upstream` takes `&S3Request` — so
//! there is no physical key and no pool credential without having gone through authorisation and
//! rewrite. That is a property of the types, not a step someone has to remember.
use crate::{
    models::{access_keys, buckets, pools, users},
    s3::{error::S3Error, sigv4},
};

pub struct S3Request {
    pub key: access_keys::Model,
    pub user: users::Model,
    pub bucket: buckets::Model,
    pub pool: pools::Model,
    /// What the client asked for.
    pub logical_key: String,
    /// `{user_pid}/{bucket_name}/{logical_key}`, inside `pool.physical_bucket`.
    pub physical_key: String,
}

/// Longest object key S3 accepts, in bytes.
pub const MAX_KEY_LEN: usize = 1024;

impl S3Request {
    /// Verbs that address an object: runs the whole chain.
    ///
    /// # Errors
    /// See spec §6 step by step; every failure is an `S3Error` with the code a client can act on.
    pub async fn resolve(
        ctx: &AppContext,
        parts: &Parts,
        action: &str,
    ) -> Result<Self, S3Error> {
        let (bucket_name, logical_key) = split_path(parts.uri.path())?;

        // 1-2. Authenticate. The canonical request is built from the URI as received: the client
        // signed the logical path, and rewriting before verifying would break every signature.
        let key = authenticate(ctx, parts).await?;
        let user = users::Model::find_by_pid(&ctx.db, &key.user_id.to_string())
            .await
            .map_err(|_| S3Error::InternalError)?;

        // 3. Bucket, scoped to this key's owner. Another user's bucket is absent, not forbidden.
        let bucket = buckets::Model::find_by_user_and_name(&ctx.db, user.id, &bucket_name)
            .await
            .map_err(|_| S3Error::InternalError)?
            .ok_or(S3Error::NoSuchBucket)?;

        let pool = pools::Model::find_by_id(&ctx.db, bucket.pool_id)
            .await
            .map_err(|_| S3Error::InternalError)?;

        // 4. Validate the key before it becomes part of a path.
        validate_logical_key(&logical_key)?;

        // 5-6. Authorise. Both checks before any rewrite, so a refusal cannot have produced a
        // physical key.
        if !key.permissions(&ctx.db).await
            .map_err(|_| S3Error::InternalError)?
            .iter()
            .any(|p| p == action)
        {
            return Err(S3Error::AccessDenied);
        }
        if !key.allows_key(&ctx.db, &logical_key).await
            .map_err(|_| S3Error::InternalError)?
        {
            return Err(S3Error::AccessDenied);
        }

        // 7. Rewrite.
        let physical_key = format!("{}/{}/{}", user.pid, bucket.name, logical_key);

        Ok(Self { key, user, bucket, pool, logical_key, physical_key })
    }
}

/// Rejects a key that cannot safely become a path segment.
///
/// `..` is refused rather than normalised: normalising would turn `img/../../escape` into a key
/// the prefix check already approved under a different name.
fn validate_logical_key(key: &str) -> Result<(), S3Error> {
    if key.len() > MAX_KEY_LEN {
        return Err(S3Error::KeyTooLong);
    }
    if key.starts_with('/') {
        return Err(S3Error::InvalidArgument(
            "Object key must not start with a slash.".to_string(),
        ));
    }
    if key.split('/').any(|seg| seg == "..") {
        return Err(S3Error::InvalidArgument(
            "Object key must not contain a '..' path segment.".to_string(),
        ));
    }
    Ok(())
}
```

Thứ tự bước 4 trước 5–6 là cố ý: một key chứa `..` bị từ chối trước khi luật prefix chạy, nên không có cách nào một key hợp lệ về prefix mà độc hại về path lọt qua.

`resolve_bucket_only` bỏ bước 4–7, `logical_key` và `physical_key` là chuỗi rỗng. Doc-comment ghi rõ dùng chúng là lỗi lập trình, và chỉ `ListBuckets`/`HeadBucket` gọi nó.

`resolve_copy_source` chạy lại bước 3–7 với **cùng** `self.key`:

```rust
    /// Resolves `x-amz-copy-source` under the same key's policy as the destination.
    ///
    /// Deliberately the same code path as `resolve` steps 3-7, not a parallel one: the two ends of
    /// a copy are the classic place where one side gets checked and the other does not.
    pub async fn resolve_copy_source(
        &self,
        ctx: &AppContext,
        header: &str,
    ) -> Result<PhysicalRef, S3Error> { /* ... */ }
```

- [ ] **Step 5: Chạy test cách ly**

Run: `cargo test --test mod requests::s3::scoping 2>&1 | tail -15`
Expected: PASS cả 7 test. `assert_untouched` không nổ ở test nào.

- [ ] **Step 6: Commit**

```bash
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add src/ tests/
git commit -m "feat(s3): the isolation boundary

S3Request::resolve is the only constructor, and every upstream function takes
&S3Request — so there is no physical key and no pool credential without having
passed authorisation and rewrite. That is a property of the types rather than a
step someone must remember.

The key is validated before the prefix check, so a key containing '..' is
refused rather than normalised into one the prefix check already approved under
a different name. Tests assert the mock upstream was never touched, which is a
stronger claim than a 403: it catches a gateway that calls the store and only
then refuses."
```

---

## Task 3: Error XML và dispatch

**Files:**
- Create: `src/s3/xml.rs`, `src/controllers/s3/mod.rs`
- Modify: `src/controllers/mod.rs`, `src/app.rs`
- Test: `tests/requests/s3/wire.rs`

**Interfaces:**
- Consumes: `S3Error` (G2), `S3Request` (task 2).
- Produces:
  - `xml::error_response(&S3Error, resource: &str, request_id: &str) -> Response`
  - `controllers::s3::routes() -> Routes`
  - `dispatch_*` một hàm cho mỗi `(method, path-shape)`

- [ ] **Step 1: Viết test wire**

Tạo `tests/requests/s3/wire.rs`:

```rust
/// tests/s3/test_wire.py::test_error_response_is_s3_shaped_xml
#[tokio::test]
#[serial]
async fn an_error_is_s3_shaped_xml() {
    with_gateway(|g| async move {
        let res = g.raw_get("/media-cdn/a.png", &[]).await;

        assert_eq!(res.status_code(), 403);
        assert_eq!(header(&res, "content-type"), "application/xml");

        let body = res.text();
        assert!(body.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(body.contains("<Error>"));
        assert!(body.contains("<Code>AccessDenied</Code>"));
        assert!(body.contains("<Message>"));
        assert!(body.contains("<Resource>/media-cdn/a.png</Resource>"));
        assert!(body.contains("<RequestId>"));
    })
    .await;
}

/// tests/s3/test_wire.py::test_responses_carry_a_request_id_header
#[tokio::test]
#[serial]
async fn every_response_carries_an_amz_request_id() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(ok_body(b"bytes"));

        let ok = g.get(&signer, "/media-cdn/a.png").await;
        assert!(!header(&ok, "x-amz-request-id").is_empty());

        let err = g.raw_get("/media-cdn/a.png", &[]).await;
        assert!(!header(&err, "x-amz-request-id").is_empty());
    })
    .await;
}

/// HEAD never has a body — botocore reads Content-Length and would hang or mis-parse otherwise.
#[tokio::test]
#[serial]
async fn a_head_error_has_no_body() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(canned(404, b"<Error><Code>NoSuchKey</Code></Error>"));

        let res = g.head(&signer, "/media-cdn/missing.png").await;

        assert_eq!(res.status_code(), 404);
        assert!(res.text().is_empty(), "HEAD must not carry a body");
    })
    .await;
}

/// Query dispatch. A missing branch here is a verb that silently does the wrong thing.
#[tokio::test]
#[serial]
async fn unimplemented_shapes_say_so() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;

        // ListObjects V1 — no list-type param.
        let res = g.get(&signer, "/media-cdn").await;
        assert_eq!(res.status_code(), 501);
        assert!(res.text().contains("NotImplemented"));

        // CreateBucket over S3.
        let res = g.put(&signer, "/new-bucket", b"").await;
        assert_eq!(res.status_code(), 501);

        g.mock.assert_untouched();
    })
    .await;
}

/// aws-chunked signing is out of scope; it must say so rather than mis-parse the body.
#[tokio::test]
#[serial]
async fn aws_chunked_payload_is_not_implemented() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;

        let res = g
            .put_with_payload_hash(
                &signer,
                "/media-cdn/a.bin",
                "STREAMING-AWS4-HMAC-SHA256-PAYLOAD",
                b"chunked framing",
            )
            .await;

        assert_eq!(res.status_code(), 501);
        assert!(res.text().contains("aws-chunked"));
        g.mock.assert_untouched();
    })
    .await;
}
```

- [ ] **Step 2: Chạy để chắc nó fail**

Run: `cargo test --test mod requests::s3::wire 2>&1 | tail -10`
Expected: FAIL — route chưa tồn tại.

- [ ] **Step 3: Viết `xml::error_response`**

```rust
/// Renders an S3 error.
///
/// `Resource` is the logical path the client asked for, never the physical one — the whole point
/// of the gateway is that the client never learns the physical layout, and an error body is the
/// easiest place to leak it.
#[must_use]
pub fn error_response(err: &S3Error, resource: &str, request_id: &str) -> Response { /* ... */ }
```

Escape XML cho `resource` và `message`: một key chứa `&` hoặc `<` mà không escape thì tạo ra XML không parse được, và botocore sẽ báo một lỗi hoàn toàn khác.

HEAD: `dispatch` biết method, nên nó dựng response lỗi rỗng body cho HEAD. Không để `error_response` tự đoán.

- [ ] **Step 4: Viết cây route và dispatch**

```rust
//! The S3 route tree.
//!
//! axum cannot route on query parameters, and S3 overloads verbs onto the same path with them —
//! `?uploads`, `?uploadId`, `?list-type=2`, `?delete`. So each (method, path-shape) gets one
//! handler that reads the query and dispatches. That layer is forced by the protocol.
//!
//! It is also where audit belongs (G7): it is the only place that sees both an auth failure and
//! a result.
pub fn routes() -> Routes {
    Routes::new()
        .add("/", get(list_buckets))
        .add("/{bucket}", get(bucket_get).head(bucket_head).post(bucket_post)
                          .put(not_implemented).delete(not_implemented))
        .add("/{bucket}/{*key}", get(object_get).head(object_head)
                                 .put(object_put).post(object_post).delete(object_delete))
}
```

**Thứ tự đăng ký trong `App::routes()` là quan trọng.** `/{bucket}/{*key}` khớp gần như mọi thứ, nên cây S3 phải nằm **sau cùng**:

```rust
    fn routes(_ctx: &AppContext) -> AppRoutes {
        AppRoutes::with_default_routes()
            .add_route(controllers::auth::routes())
            .add_route(controllers::api::routes())
            .add_route(controllers::admin::routes())
            .add_route(controllers::admin_pools::routes())
            .add_route(controllers::buckets::routes())
            // Last: /{bucket}/{*key} matches nearly everything.
            .add_route(controllers::s3::routes())
    }
```

Thêm một test khẳng định thứ tự đó, vì hỏng nó làm console trắng và triệu chứng không chỉ về route:

```rust
/// The S3 catch-all must not shadow the management API or the console.
#[tokio::test]
#[serial]
async fn the_s3_catch_all_does_not_shadow_the_management_api() {
    with_gateway(|g| async move {
        // Unauthenticated /api/keys is 401 from the management API, not an S3 error.
        let res = g.raw_get("/api/keys", &[]).await;
        assert_eq!(res.status_code(), 401);
        assert!(!res.text().contains("<Error>"));

        // The SPA still serves.
        let res = g.raw_get("/", &[]).await;
        assert!(res.text().contains("<!DOCTYPE html>") || res.status_code() == 200);

        // Health still answers.
        let res = g.raw_get("/_ping", &[]).await;
        assert_eq!(res.status_code(), 200);
    })
    .await;
}
```

Bảng dispatch theo spec mục 7. Mỗi nhánh chưa làm gọi `not_implemented(reason)`.

- [ ] **Step 5: Chạy test và commit**

```bash
cargo test --test mod requests::s3 2>&1 | tail -10
cargo loco routes 2>/dev/null | tail -20
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add src/ tests/
git commit -m "feat(s3): error XML and the query-dispatch route tree

axum cannot route on query parameters and S3 overloads verbs onto the same path
with them, so the dispatch layer is forced by the protocol. The tree registers
last: /{bucket}/{*key} matches nearly everything, and a test asserts it does not
shadow the management API, the console or the health endpoints — that failure
shows up as a blank console, which does not point at routing.

Resource in an error body is the logical path, never the physical one: an error
body is the easiest place to leak the layout the gateway exists to hide."
```

---

## Task 4: GetObject và HeadObject

**Files:**
- Create: `src/controllers/s3/object.rs`
- Modify: `src/controllers/s3/mod.rs`
- Test: `tests/requests/s3/read.rs`

**Interfaces:**
- Consumes: `S3Request` (task 2), `upstream::Client` (G2).
- Produces: `object::get`, `object::head`.

- [ ] **Step 1: Viết test**

Tạo `tests/requests/s3/read.rs`:

```rust
/// Body and headers come from upstream; the objects row is metadata for listing and quota, not
/// the source of truth for content.
#[tokio::test]
#[serial]
async fn get_streams_the_upstream_body_and_headers() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(Canned {
            status: 200,
            headers: vec![
                ("content-type".into(), "image/png".into()),
                ("etag".into(), "\"abc123\"".into()),
                ("content-length".into(), "9".into()),
                ("last-modified".into(), "Mon, 17 Aug 2026 08:00:00 GMT".into()),
                ("x-amz-meta-owner".into(), "team-a".into()),
            ],
            body: b"png bytes".to_vec(),
        });

        let res = g.get(&signer, "/media-cdn/img/a.png").await;

        assert_eq!(res.status_code(), 200);
        assert_eq!(res.text(), "png bytes");
        assert_eq!(header(&res, "content-type"), "image/png");
        assert_eq!(header(&res, "etag"), "\"abc123\"");
        assert_eq!(header(&res, "x-amz-meta-owner"), "team-a");
        g.mock.assert_key(0, "osg-main/{user_pid}/media-cdn/img/a.png");
    })
    .await;
}

/// test_object_crud.py::test_get_range_returns_partial_content
#[tokio::test]
#[serial]
async fn a_range_request_is_forwarded_and_206_comes_back() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(Canned {
            status: 206,
            headers: vec![
                ("content-range".into(), "bytes 0-3/9".into()),
                ("content-length".into(), "4".into()),
            ],
            body: b"png ".to_vec(),
        });

        let res = g.get_with(&signer, "/media-cdn/img/a.png", &[("range", "bytes=0-3")]).await;

        assert_eq!(res.status_code(), 206);
        assert_eq!(header(&res, "content-range"), "bytes 0-3/9");
        assert_eq!(header_of(&g.mock.requests()[0], "range"), "bytes=0-3");
    })
    .await;
}

/// test_object_crud.py::test_get_if_none_match_on_current_etag_is_304
#[tokio::test]
#[serial]
async fn conditional_headers_are_forwarded_and_304_comes_back() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(Canned { status: 304, headers: vec![], body: vec![] });

        let res = g
            .get_with(&signer, "/media-cdn/img/a.png", &[("if-none-match", "\"abc123\"")])
            .await;

        assert_eq!(res.status_code(), 304);
        assert!(res.text().is_empty());
    })
    .await;
}

/// test_object_crud.py::test_get_missing_key_is_no_such_key
#[tokio::test]
#[serial]
async fn a_missing_key_is_no_such_key_without_leaking_the_physical_path() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(canned(
            404,
            br#"<Error><Code>NoSuchKey</Code><Key>osg-main/1111/media-cdn/gone.png</Key></Error>"#,
        ));

        let res = g.get(&signer, "/media-cdn/gone.png").await;

        assert_eq!(res.status_code(), 404);
        let body = res.text();
        assert!(body.contains("NoSuchKey"));
        assert!(!body.contains("osg-main"), "physical bucket leaked: {body}");
        assert!(body.contains("<Resource>/media-cdn/gone.png</Resource>"));
    })
    .await;
}

/// test_object_crud.py::test_head_missing_key_is_bare_404
#[tokio::test]
#[serial]
async fn head_reports_metadata_without_a_body() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(Canned {
            status: 200,
            headers: vec![
                ("content-length".into(), "9".into()),
                ("etag".into(), "\"abc123\"".into()),
            ],
            body: vec![],
        });

        let res = g.head(&signer, "/media-cdn/img/a.png").await;

        assert_eq!(res.status_code(), 200);
        assert_eq!(header(&res, "content-length"), "9");
        assert_eq!(header(&res, "etag"), "\"abc123\"");
        assert!(res.text().is_empty());
    })
    .await;
}
```

- [ ] **Step 2: Chạy để chắc nó fail**

Run: `cargo test --test mod requests::s3::read 2>&1 | tail -10`
Expected: FAIL — handler chưa có.

- [ ] **Step 3: Viết handler**

```rust
/// Headers forwarded from the client to upstream on a read.
/// Anything not on this list is dropped: forwarding an unknown header can change upstream
/// behaviour in ways the gateway did not intend.
const FORWARD_TO_UPSTREAM: &[&str] = &[
    "range",
    "if-match",
    "if-none-match",
    "if-modified-since",
    "if-unmodified-since",
];

/// Headers forwarded from upstream back to the client.
/// `x-amz-meta-*` is matched by prefix, not listed.
const FORWARD_TO_CLIENT: &[&str] = &[
    "content-type",
    "content-length",
    "content-range",
    "content-encoding",
    "content-disposition",
    "cache-control",
    "etag",
    "last-modified",
    "accept-ranges",
];
```

Danh sách trắng cả hai chiều, không phải danh sách đen. Một header của upstream mà ta chuyển tiếp mù có thể mang tên bucket thật (`x-amz-bucket-region` là vô hại, nhưng `x-amz-id-2` và các header debug của một số provider thì không).

`get` stream response body về mà không đọc vào bộ nhớ: `Body::from_stream(upstream_response.body)`.

`head` gọi cùng đường nhưng bỏ body — và **không** được dựng response lỗi có body.

- [ ] **Step 4: Chạy test ba backend**

```bash
cargo test 2>&1 | tail -3
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test 2>&1 | tail -3
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test 2>&1 | tail -3
```

- [ ] **Step 5: Kiểm với client S3 thật**

Đây là bước nghiệm thu của cả G3. Cần một MinIO cục bộ làm upstream:

```bash
docker run -d --name osg-upstream -p 9000:9000 \
  -e MINIO_ROOT_USER=upstream -e MINIO_ROOT_PASSWORD=upstream-secret \
  quay.io/minio/minio server /data
docker exec osg-upstream mc alias set local http://localhost:9000 upstream upstream-secret
docker exec osg-upstream mc mb local/osg-main
```

Rồi qua console: tạo pool trỏ vào `http://localhost:9000` với `physical_bucket = osg-main`, tạo bucket `media-cdn`, tạo access key có `read`.

Đặt object vào upstream đúng chỗ gateway sẽ tìm, rồi đọc **qua gateway**:

```bash
docker exec osg-upstream sh -c 'echo hello > /tmp/a.txt && \
  mc cp /tmp/a.txt local/osg-main/{user_pid}/media-cdn/img/a.txt'

AWS_ACCESS_KEY_ID=OSG… AWS_SECRET_ACCESS_KEY=… \
  aws s3 cp s3://media-cdn/img/a.txt - --endpoint-url http://localhost:5150
```

Expected: in ra `hello`.

Đây là lần đầu một client S3 thật nói chuyện được với gateway. Nếu bước này fail mà mọi test đều xanh thì chỗ sai gần như chắc chắn là `canonical_request` — signature của botocore khác signer trong test, và chỉ bước này bắt được.

- [ ] **Step 6: Commit**

```bash
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add src/ tests/
git commit -m "feat(s3): GetObject and HeadObject

Body and headers come from upstream; the objects row is metadata for listing and
quota, not the source of truth for content. Headers are whitelisted in both
directions rather than blacklisted: forwarding an unknown upstream header can
carry the physical bucket name, and forwarding an unknown client header can
change upstream behaviour the gateway did not intend.

Verified with aws-cli against a MinIO upstream — botocore's signature differs
from the test signer, so that is the only check that catches a canonical-request
bug."
```

---

## Self-review

**Phủ spec.** Mục 6 (biên giới cách ly) → task 1, 2. Mục 6.1 (luật prefix) → task 1. Mục 7 (route + dispatch) → task 3. Mục 8.2 (GetObject) → task 4. Mục 12 (wire, error XML) → task 3.

**Chưa phủ, cố ý.** `resolve_copy_source` viết ở task 2 nhưng chưa có caller — G6 dùng. PutObject/Delete → G4. Listing → G5. Audit → G7. `x-amz-content-sha256` dạng hex chỉ được đưa vào canonical request, chưa đối chiếu body — đã chốt ở spec mục 5.2.

**Nhất quán kiểu.** `S3Request` khai task 2, dùng task 4 và G4–G7. `PhysicalRef` khai task 2, dùng G6. `xml::error_response` khai task 3, dùng task 3, 4 và mọi plan sau. `TestGateway`/`with_gateway` khai task 2, dùng task 3, 4 và G4–G7 — nó là harness chung, đầu tư một lần.

**Rủi ro đã biết.**

1. **Thứ tự đăng ký route.** `/{bucket}/{*key}` khớp gần hết. Sai thứ tự thì console trắng và triệu chứng không chỉ về route. Có test canh, nhưng test đó phải chạy **trước** khi ai đó thêm route mới.
2. **Test signer không dùng chung code với `sigv4.rs`** là cố ý, nhưng nó có nghĩa signer trong test cũng có thể sai. Bước 5 của task 4 (aws-cli thật) là cái duy nhất bắt được cả hai cùng sai.
3. **`users::Model::find_by_pid(&key.user_id.to_string())`** trong đoạn `resolve` ở bước 4 là sai — `user_id` là `i32`, `find_by_pid` cần UUID. Phải thêm `users::Model::find_by_id(db, i32)` hoặc load qua quan hệ. Sửa khi viết, và đây là lý do bước 3 của task 2 phải chạy compiler trước khi tin đoạn mẫu.
4. **Danh sách trắng header có thể thiếu.** Một client dùng header S3 mà ta không chuyển tiếp sẽ thấy hành vi khác S3 thật, và triệu chứng là "nó không work" chứ không phải một lỗi. Bộ conformance ở G7 bắt phần lớn; phần còn lại chỉ lộ với client thật.
