# G7 — Audit, background jobs, và bộ conformance — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Mọi request S3 để lại một dòng audit ghi qua queue; multipart bỏ dở và audit cũ được dọn tự động; rate limit không còn chặn data plane; và 61 test conformance chạy được với `OSG_S3_TARGET=gateway`.

**Architecture:** Audit ghi qua Redis queue nên không thêm độ trễ vào đường request. `dispatch()` là chỗ duy nhất thấy cả auth-fail lẫn kết quả, nên entry dựng ở đó — đó là lý do G3 đổi `S3Request` từ extractor thành constructor. Rate limit bọc governor trong một layer chỉ áp cho `/api`.

**Tech Stack:** Rust, loco worker + Redis, `tower`, pytest + boto3.

**Spec:** `docs/superpowers/specs/2026-08-17-s3-gateway-design.md` mục 13, 14, 15, 17.3

**Phụ thuộc:** G6 xong — toàn bộ bề mặt verb.

**Deliverable:** `cargo loco task cleanup_multipart` và `cleanup_audit` chạy được; 61/61 conformance xanh; `docker-compose` có Valkey và code đọc `REDIS_URL`.

## Global Constraints

- Ba backend hạng nhất: Postgres, MySQL >= 8.0.13, SQLite.
- Cột `TIMESTAMP` mới phải khai `TIMESTAMP(6)` trên MySQL — `audit_logs.occurred_at` là cột như vậy, và G1 đã tạo nó; task 1 chỉ kiểm.
- **Audit không được làm fail một request.** Một lỗi khi enqueue phải log rồi đi tiếp; một request S3 thành công không được biến thành 500 vì Redis rớt.
- Comment trong code: tiếng Anh, một câu một dòng.
- Không tự commit/push ngoài các bước commit trong plan. Không AI attribution.

---

## File Structure

**Tạo mới:**
- `src/models/audit_logs.rs`
- `src/workers/audit.rs`
- `src/tasks/cleanup_multipart.rs`
- `src/tasks/cleanup_audit.rs`
- `src/controllers/admin_audit.rs` — đọc audit cho admin
- `src/views/audit.rs`
- `tests/models/audit_logs.rs`
- `tests/requests/s3/audit.rs`
- `tests/tasks/cleanup.rs`

**Sửa:**
- `src/models/mod.rs`, `src/workers/mod.rs`, `src/tasks/mod.rs`, `src/app.rs`
- `src/controllers/s3/mod.rs` — dựng entry trong `dispatch`
- `src/initializers/rate_limit.rs` — loại trừ data plane
- `config/production.yaml`, `config/development.yaml`, `config/test.yaml`
- `docker-compose.yml`
- `tests/s3/README.md`, `tests/s3/conftest.py`
- `docs/docker.md`, `README.md`, `CLAUDE.md`

---

## Task 1: Model và worker audit

**Files:**
- Create: `src/models/audit_logs.rs`, `src/workers/audit.rs`, `tests/models/audit_logs.rs`
- Modify: `src/models/mod.rs`, `src/workers/mod.rs`, `src/app.rs`, `tests/models/mod.rs`

**Interfaces:**
- Consumes: bảng `audit_logs` (G1 migration `m20260818_000004`).
- Produces:
  - `audit_logs::AuditEntry` — payload serialize được, đi qua queue
  - `audit_logs::Model::record(db, &AuditEntry) -> ModelResult<Model>`
  - `audit_logs::Model::list_for_user(db, user_id, limit) -> ModelResult<Vec<Model>>`
  - `audit_logs::Model::delete_older_than(db, days) -> ModelResult<u64>`
  - `workers::audit::AuditWorker` — `BackgroundWorker<AuditEntry>`

- [ ] **Step 1: Viết test**

```rust
/// An auth failure has no user and no bucket, and that is the case worth recording — it is how
/// key probing shows up.
#[tokio::test]
#[serial]
async fn an_entry_without_a_user_is_recordable() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    let entry = audit_logs::AuditEntry {
        user_id: None,
        access_key_id: Some("OSGDOESNOTEXIST".to_string()),
        bucket_id: None,
        object_key: None,
        action: audit_logs::ACTION_AUTH.to_string(),
        outcome: audit_logs::OUTCOME_DENIED.to_string(),
        status_code: 403,
        bytes: 0,
        duration_ms: 3,
        request_id: "req-1".to_string(),
        ip: "203.0.113.7".to_string(),
        user_agent: Some("aws-cli/2.15".to_string()),
    };

    let row = audit_logs::Model::record(db, &entry).await.unwrap();

    assert!(row.user_id.is_none());
    assert_eq!(row.access_key_id.as_deref(), Some("OSGDOESNOTEXIST"));
    assert_eq!(row.outcome, audit_logs::OUTCOME_DENIED);
}

/// Audit must outlive the user it describes: deleting an account cannot erase the record of what
/// its keys did. That means no foreign key.
#[tokio::test]
#[serial]
async fn an_entry_survives_the_user_being_deleted() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();

    let user = users::Model::find_by_email(db, "user1@example.com").await.unwrap();
    audit_logs::Model::record(db, &entry_for(user.id)).await.unwrap();

    user.delete_with_owned_data(db).await.unwrap();

    let all = audit_logs::Model::list_recent(db, 100).await.unwrap();
    assert_eq!(all.len(), 1, "audit was deleted with the user");
}

/// occurred_at must keep sub-second precision, or two requests 100ms apart look simultaneous and
/// the order of events in an incident is lost. On MySQL that needs TIMESTAMP(6).
#[tokio::test]
#[serial]
async fn occurred_at_keeps_sub_second_precision() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    let a = audit_logs::Model::record(db, &entry_for_none("req-a")).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let b = audit_logs::Model::record(db, &entry_for_none("req-b")).await.unwrap();

    assert_ne!(
        a.occurred_at, b.occurred_at,
        "two entries 50ms apart share a timestamp; MySQL needs TIMESTAMP(6)"
    );
    assert!(b.occurred_at > a.occurred_at);
}

#[tokio::test]
#[serial]
async fn delete_older_than_keeps_recent_rows() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    let fresh = audit_logs::Model::record(db, &entry_for_none("fresh")).await.unwrap();
    let old = audit_logs::Model::record(db, &entry_for_none("old")).await.unwrap();
    backdate_audit(db, old.id, 120).await;

    let removed = audit_logs::Model::delete_older_than(db, 90).await.unwrap();

    assert_eq!(removed, 1);
    let all = audit_logs::Model::list_recent(db, 100).await.unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].id, fresh.id);
}

/// The worker is registered and its payload round-trips through the queue's serialisation.
#[tokio::test]
#[serial]
async fn the_audit_entry_round_trips_as_json() {
    let entry = entry_for_none("req-1");
    let json = serde_json::to_string(&entry).unwrap();
    let back: audit_logs::AuditEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(back.request_id, entry.request_id);
    assert_eq!(back.outcome, entry.outcome);
}
```

`occurred_at_keeps_sub_second_precision` là test canh đúng ràng buộc CLAUDE.md về `TIMESTAMP(6)`. Nó fail trên MySQL nếu G1 quên khai precision, và fail theo cách rất khó chẩn nếu không có test: audit vẫn có đủ dòng, chỉ mất thứ tự trong một sự cố.

- [ ] **Step 2: Chạy để chắc nó fail, viết model**

```rust
pub const ACTION_READ: &str = "read";
pub const ACTION_WRITE: &str = "write";
pub const ACTION_DELETE: &str = "delete";
pub const ACTION_LIST: &str = "list";
pub const ACTION_MULTIPART: &str = "multipart";
pub const ACTION_PRESIGNED: &str = "presigned";
pub const ACTION_AUTH: &str = "auth";

pub const OUTCOME_OK: &str = "ok";
pub const OUTCOME_DENIED: &str = "denied";
pub const OUTCOME_QUOTA: &str = "quota_exceeded";
pub const OUTCOME_NOT_FOUND: &str = "not_found";
pub const OUTCOME_ERROR: &str = "error";

/// What the request path hands to the queue.
///
/// `outcome` is separate from `status_code` so "which key gets denied most" is a GROUP BY rather
/// than a status-code parse — the same 403 covers a wrong signature, a missing permission and a
/// full bucket, and those are three different operational problems.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry { /* … */ }
```

Worker:

```rust
//! Writes audit entries off the request path.
//!
//! One INSERT is only about a millisecond, but it is a millisecond on every S3 request including
//! the ones that are already slow, and a DB hiccup would turn successful uploads into 500s.
pub struct AuditWorker {
    pub ctx: AppContext,
}

#[async_trait]
impl BackgroundWorker<AuditEntry> for AuditWorker {
    fn build(ctx: &AppContext) -> Self {
        Self { ctx: ctx.clone() }
    }

    async fn perform(&self, entry: AuditEntry) -> Result<()> {
        audit_logs::Model::record(&self.ctx.db, &entry).await?;
        Ok(())
    }
}
```

Đăng ký trong `App::connect_workers`. Thêm `truncate_table(&ctx.db, audit_logs::Entity)` vào `App::truncate`.

- [ ] **Step 3: Ba backend và commit**

```bash
cargo test --test mod models::audit_logs 2>&1 | tail -5
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test 2>&1 | tail -3
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test 2>&1 | tail -3
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add src/ tests/
git commit -m "feat(audit): the audit model and its worker

outcome is stored separately from status_code: the same 403 covers a wrong
signature, a missing permission and a full bucket, and those are three different
operational problems. No foreign key to users — audit must outlive the account it
describes, or deleting a user erases the record of what its keys did.

A test asserts sub-second precision on occurred_at, which is what TIMESTAMP(6)
buys on MySQL; without it two requests 100ms apart share a timestamp and the order
of events in an incident is lost."
```

---

## Task 2: Ghi audit trong `dispatch`

**Files:**
- Modify: `src/controllers/s3/mod.rs`
- Test: `tests/requests/s3/audit.rs`

**Interfaces:**
- Consumes: `AuditEntry`, `AuditWorker` (task 1).
- Produces: mọi nhánh dispatch ghi đúng một dòng audit.

- [ ] **Step 1: Viết test**

```rust
/// One row per request, on the happy path.
#[tokio::test]
#[serial]
async fn a_successful_get_is_audited() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(ok_body(b"png bytes"));

        g.get(&signer, "/media-cdn/img/a.png").await;
        g.drain_queue().await;

        let rows = g.audit_rows().await;
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(r.action, "read");
        assert_eq!(r.outcome, "ok");
        assert_eq!(r.status_code, 200);
        assert_eq!(r.object_key.as_deref(), Some("img/a.png"));
        assert_eq!(r.bytes, 9);
        assert!(r.user_id.is_some());
        assert!(!r.request_id.is_empty());
        // The logical key, never the physical one.
        assert!(!r.object_key.as_deref().unwrap().contains("osg-main"));
    })
    .await;
}

/// The case a status-code-only audit would lose: three different 403s.
#[tokio::test]
#[serial]
async fn the_three_kinds_of_403_are_distinguishable() {
    with_gateway(|g| async move {
        // Wrong signature.
        let bad = g.full_key().await.with_secret("nope");
        g.get(&bad, "/media-cdn/a.png").await;

        // Missing permission.
        let readonly = g.key_with(&["read"], &[]).await;
        g.put(&readonly, "/media-cdn/a.png", b"x").await;

        // Full bucket.
        let full = g.full_key().await;
        g.set_bucket_quota("media-cdn", 1).await;
        g.put(&full, "/media-cdn/b.png", &vec![0u8; 100]).await;

        g.drain_queue().await;

        let rows = g.audit_rows().await;
        let outcomes: Vec<&str> = rows.iter().map(|r| r.outcome.as_str()).collect();
        assert!(outcomes.contains(&"denied"));
        assert!(outcomes.contains(&"quota_exceeded"));
        assert!(rows.iter().all(|r| r.status_code == 403));
    })
    .await;
}

/// An unauthenticated request has no user, but the key id the client presented is the thing
/// worth keeping: it is how key probing is spotted.
#[tokio::test]
#[serial]
async fn an_unknown_key_id_is_recorded_with_the_id_it_presented() {
    with_gateway(|g| async move {
        let unknown = g.full_key().await.with_id("OSGPROBE0000000001");

        g.get(&unknown, "/media-cdn/a.png").await;
        g.drain_queue().await;

        let rows = g.audit_rows().await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].action, "auth");
        assert_eq!(rows[0].outcome, "denied");
        assert!(rows[0].user_id.is_none());
        assert_eq!(rows[0].access_key_id.as_deref(), Some("OSGPROBE0000000001"));
    })
    .await;
}

/// Exactly one row per request, including multi-step verbs.
#[tokio::test]
#[serial]
async fn a_multipart_upload_leaves_one_row_per_request_not_per_part() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        let upload = g.start_upload(&signer, "big.bin").await;
        g.mock.push(etag_ok("\"p1\""));
        g.upload_part(&signer, "big.bin", &upload, 1, &vec![0u8; 300]).await;
        g.mock.push(canned(200, br#"<CompleteMultipartUploadResult><ETag>"f"</ETag></CompleteMultipartUploadResult>"#));
        g.mock.push(Canned { status: 200, headers: vec![("content-length".into(), "300".into())], body: vec![] });
        g.complete(&signer, "big.bin", &upload, &[(1, "\"p1\"")]).await;

        g.drain_queue().await;

        let rows = g.audit_rows().await;
        assert_eq!(rows.len(), 3, "create, part, complete — one each");
        assert!(rows.iter().all(|r| r.action == "multipart"));
        assert!(rows.iter().all(|r| r.outcome == "ok"));
    })
    .await;
}

/// Audit must never break a request. If the queue is unreachable, the request still succeeds.
#[tokio::test]
#[serial]
async fn a_broken_queue_does_not_fail_the_request() {
    with_gateway_no_queue(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(ok_body(b"png bytes"));

        let res = g.get(&signer, "/media-cdn/img/a.png").await;

        assert_eq!(res.status_code(), 200, "a queue failure turned a good request into an error");
        assert_eq!(res.text(), "png bytes");
    })
    .await;
}

/// The management API is not audited here: it has its own surface, and mixing them makes the S3
/// audit useless for the thing it exists for.
#[tokio::test]
#[serial]
async fn the_management_api_is_not_in_the_s3_audit() {
    with_gateway(|g| async move {
        let user = g.console_login().await;
        g.api_get("/api/keys", &user).await;
        g.drain_queue().await;

        assert!(g.audit_rows().await.is_empty());
    })
    .await;
}
```

`a_broken_queue_does_not_fail_the_request` là test quan trọng nhất của task. Audit là quan sát, không phải chức năng; để nó làm chết một upload 5 GiB là đổi một tính năng lấy một dòng log.

- [ ] **Step 2: Chạy để chắc nó fail, viết**

```rust
/// Wraps a verb handler with timing and audit.
///
/// This is the only place that sees both an authentication failure and a result — the reason
/// `S3Request::resolve` is a constructor rather than an axum extractor (spec §13).
async fn audited<F, Fut>(
    ctx: &AppContext,
    parts: &Parts,
    action: &'static str,
    handler: F,
) -> Response
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<Response, S3Error>>,
{
    let started = Instant::now();
    let result = handler().await;

    let entry = build_entry(parts, action, &result, started.elapsed());

    // Deliberately ignored: audit is observation, not function. Letting a queue outage turn a
    // successful 5 GiB upload into a 500 trades a feature for a log line.
    if let Err(e) = AuditWorker::perform_later(ctx, entry).await {
        tracing::error!(error = %e, "could not enqueue an audit entry");
    }

    match result {
        Ok(res) => res,
        Err(err) => xml::error_response(&err, parts.uri.path(), request_id(parts)),
    }
}
```

`build_entry` cần biết `user_id`/`bucket_id` — mà những cái đó nằm trong `S3Request` mà handler tạo bên trong. Giải: handler trả `(Response, AuditFacts)` hoặc `S3Error`, với `AuditFacts { user_id, bucket_id, object_key, bytes }`. Chọn cách đó: nó buộc mỗi handler nói ra nó đã làm gì, thay vì `audited` phải đoán.

- [ ] **Step 3: Ba backend và commit**

```bash
cargo test --test mod requests::s3::audit 2>&1 | tail -5
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test 2>&1 | tail -3
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test 2>&1 | tail -3
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add src/ tests/
git commit -m "feat(s3): audit every request from dispatch

dispatch is the only place that sees both an auth failure and a result, which is
why S3Request::resolve is a constructor rather than an extractor. A handler
returns what it did alongside its response, so the wrapper records facts rather
than guessing them.

An enqueue failure is logged and ignored: audit is observation, not function, and
letting a queue outage turn a successful 5 GiB upload into a 500 trades a feature
for a log line. A test asserts exactly that."
```

---

## Task 3: Redis queue và config

**Files:**
- Modify: `config/production.yaml`, `config/development.yaml`, `config/test.yaml`, `docker-compose.yml`, `docs/docker.md`, `CLAUDE.md`

**Interfaces:**
- Consumes: `AuditWorker` (task 1).
- Produces: `workers.mode: BackgroundQueue` với Redis ở production.

- [ ] **Step 1: Config**

`config/production.yaml`:

```yaml
workers:
  # BackgroundQueue, not BackgroundAsync: an in-process queue loses every audit entry queued at
  # the moment of a restart, and a missing audit trail on a multi-tenant gateway is a compliance
  # problem rather than a performance one.
  mode: BackgroundQueue

queue:
  kind: Redis
  uri: "{{ get_env(name='REDIS_URL') }}"
  dangerously_flush: false
```

`config/development.yaml` và `config/test.yaml`: giữ `BackgroundAsync`. Test không nên cần Redis — `perform_later` với `BackgroundAsync` chạy ngay trong process, nên `drain_queue()` trong test chỉ là `tokio::task::yield_now()` vài lần.

Ghi lại quyết định đó ngay trong `config/test.yaml`:

```yaml
workers:
  # BackgroundAsync in test on purpose: the queue is not what these tests exercise, and requiring
  # Redis to run `cargo test` would make the suite un-runnable on a fresh checkout.
  mode: BackgroundAsync
```

- [ ] **Step 2: Compose lấy lại Valkey**

```yaml
  # P2 removed this because nothing read REDIS_URL. The audit worker does now.
  valkey:
    image: valkey/valkey:8-alpine
    restart: unless-stopped
    command: ["valkey-server", "--appendonly", "yes"]
    volumes:
      - valkeydata:/data
    healthcheck:
      test: ["CMD", "valkey-cli", "ping"]
      interval: 5s
      retries: 10

  app:
    environment:
      REDIS_URL: redis://valkey:6379
    depends_on:
      valkey:
        condition: service_healthy
```

`--appendonly yes` là cố ý: một audit entry đang trong queue lúc Valkey restart mà không có AOF thì mất, và cả điểm của việc dùng queue là để không mất.

- [ ] **Step 3: Kiểm bằng tay với Redis thật**

```bash
JWT_SECRET=$(openssl rand -base64 32) OSG_MASTER_KEY=$(openssl rand -base64 32) \
SERVER_HOST=http://localhost:5150 POSTGRES_USER=osg POSTGRES_PASSWORD=$(openssl rand -hex 16) \
docker compose -f docker-compose.yml -f docker-compose/postgres.yml up -d

docker compose run --rm app object_storage_gate-cli db migrate
# rồi đi một vòng S3, sau đó:
docker compose exec valkey valkey-cli LLEN queue:default
docker compose exec -T db psql -U osg -c "SELECT action, outcome, count(*) FROM audit_logs GROUP BY 1,2;"
```

Kiểm cả trường hợp Valkey rớt:

```bash
docker compose stop valkey
# một request S3 nữa — phải vẫn 200
docker compose start valkey
```

- [ ] **Step 4: CLAUDE.md**

```markdown
- **Redis bắt buộc ở production.** `workers.mode: BackgroundQueue` cho audit worker, nên deploy MySQL từ giờ cũng cần Redis — loco không có `bg_mysql`. Dev và test giữ `BackgroundAsync`: bắt `cargo test` cần Redis là làm suite không chạy được trên một checkout mới.
- **Audit không được làm fail request.** Lỗi enqueue thì log rồi đi tiếp. Audit là quan sát, không phải chức năng.
```

- [ ] **Step 5: Commit**

```bash
git add config/ docker-compose.yml docs/ CLAUDE.md
git commit -m "feat(ops): Redis-backed queue for audit

BackgroundQueue in production, because an in-process queue loses every entry
queued at the moment of a restart, and a missing audit trail on a multi-tenant
gateway is a compliance problem rather than a performance one. Valkey comes back
to compose with appendonly on — an entry in flight during a restart is exactly
what the queue exists to not lose.

Dev and test stay BackgroundAsync: requiring Redis to run cargo test would make
the suite un-runnable on a fresh checkout."
```

---

## Task 4: Rate limit loại trừ data plane

**Files:**
- Modify: `src/initializers/rate_limit.rs`
- Test: `tests/requests/rate_limit.rs`

**Interfaces:**
- Consumes: `GovernorLayer` (P2).
- Produces: layer chỉ áp cho path bắt đầu bằng `/api`.

Đây là trần `ponytail:` mà P2 ghi lại, giờ đến hạn.

- [ ] **Step 1: Viết test**

```rust
/// The ceiling P2 recorded: the governor layer covered the whole router, and the S3 data plane is
/// /{bucket}/{key} — that is everything. A legitimate multipart upload sends far more than 60
/// requests a minute.
#[tokio::test]
#[serial]
async fn the_s3_data_plane_is_not_rate_limited() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;

        for i in 0..200 {
            g.mock.push(ok_body(b"x"));
            let res = g.get(&signer, &format!("/media-cdn/img/{i}.png")).await;
            assert_ne!(
                res.status_code(), 429,
                "request {i} was throttled; a 200-part upload would fail"
            );
        }
    })
    .await;
}

/// Login is still limited: that is the endpoint the limiter exists for.
#[tokio::test]
#[serial]
async fn the_management_api_is_still_rate_limited() {
    request::<App, _, _>(|request, _ctx| async move {
        let mut saw_429 = false;
        for _ in 0..80 {
            let res = request
                .post("/api/auth/login")
                .json(&serde_json::json!({ "email": "nobody@x.vn", "password": "guess" }))
                .await;
            if res.status_code() == 429 {
                saw_429 = true;
                break;
            }
        }
        assert!(saw_429, "login accepted 80 attempts without throttling");
    })
    .await;
}

/// The health endpoints are not limited either: a probe every second must not exhaust a bucket.
#[tokio::test]
#[serial]
async fn health_endpoints_are_not_rate_limited() {
    request::<App, _, _>(|request, _ctx| async move {
        for _ in 0..200 {
            assert_ne!(request.get("/_ping").await.status_code(), 429);
        }
    })
    .await;
}
```

- [ ] **Step 2: Chạy để chắc nó fail, viết**

```rust
/// Applies the governor to the management API only.
///
/// The S3 data plane lives at `/{bucket}/{key}`, which is everything, so a router-wide limiter
/// throttles a legitimate multipart upload — aws-cli sends ten parts at a time and a 5 GiB file is
/// hundreds of requests. Rate limiting for the data plane is quota (already enforced) and the
/// reverse proxy (documented in docs/docker.md).
///
/// Health endpoints are excluded too: a probe every second must not consume a client's bucket.
#[derive(Clone)]
struct ApiOnly<S> {
    limited: S,
    passthrough: S,
}
```

Cách gọn nhất trong tower: `tower::steer::Steer`, hoặc một `Service` tự viết chọn giữa hai inner service theo `req.uri().path()`. Tự viết ~30 dòng và không thêm dependency:

```rust
impl<S, B> Service<Request<B>> for ApiOnly<S>
where
    S: Service<Request<B>> + Clone,
{
    fn call(&mut self, req: Request<B>) -> Self::Future {
        if req.uri().path().starts_with("/api") {
            self.limited.call(req)
        } else {
            self.passthrough.call(req)
        }
    }
}
```

Cả hai nhánh phải `poll_ready` — dùng `futures::future::Either` cho `Future` type.

- [ ] **Step 3: Ba backend và commit**

```bash
cargo test --test mod requests::rate_limit 2>&1 | tail -5
cargo test 2>&1 | tail -3
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add src/ tests/
git commit -m "fix(security): rate limit the management API only

Closing the ceiling P2 recorded. The governor layer covered the whole router and
the S3 data plane is /{bucket}/{key} — that is everything — so a legitimate
multipart upload would have been throttled: aws-cli sends ten parts at a time and
a 5 GiB file is hundreds of requests. Rate limiting for the data plane is quota,
already enforced, and the reverse proxy. Health probes are excluded too."
```

---

## Task 5: Background jobs

**Files:**
- Create: `src/tasks/cleanup_multipart.rs`, `src/tasks/cleanup_audit.rs`, `tests/tasks/cleanup.rs`
- Modify: `src/tasks/mod.rs`, `src/app.rs`, `README.md`

**Interfaces:**
- Consumes: `multipart_uploads::Model::older_than` (G6), `audit_logs::Model::delete_older_than` (task 1), `upstream::Client`.
- Produces: task `cleanup_multipart`, `cleanup_audit`.

- [ ] **Step 1: Viết test**

```rust
/// A stale upload must be aborted upstream *and* released, in that order — releasing first would
/// credit quota for parts still occupying the store.
#[tokio::test]
#[serial]
async fn cleanup_multipart_aborts_upstream_then_releases() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        let upload = g.start_upload(&signer, "big.bin").await;
        g.mock.push(etag_ok("\"p1\""));
        g.upload_part(&signer, "big.bin", &upload, 1, &vec![0u8; 500]).await;
        assert_eq!(g.bucket_row("media-cdn").await.reserved_bytes, 500);

        g.backdate_upload(&upload, 30).await;
        g.mock.push(canned(204, b""));

        g.run_task("cleanup_multipart").await;

        // Upstream was told to abort.
        let last = g.mock.requests().last().unwrap().clone();
        assert_eq!(last.method, "DELETE");
        assert!(last.query.contains("uploadId="));

        assert_eq!(g.bucket_row("media-cdn").await.reserved_bytes, 0);
        assert!(g.upload_row(&upload).await.is_none());
    })
    .await;
}

/// A fresh upload is left alone: a client mid-upload must not have it pulled out from under them.
#[tokio::test]
#[serial]
async fn cleanup_multipart_leaves_fresh_uploads_alone() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        let upload = g.start_upload(&signer, "big.bin").await;
        let before = g.mock.requests().len();

        g.run_task("cleanup_multipart").await;

        assert_eq!(g.mock.requests().len(), before, "a fresh upload was aborted");
        assert!(g.upload_row(&upload).await.is_some());
    })
    .await;
}

/// If upstream refuses the abort, keep the row so the next run tries again — dropping it would
/// leak both the parts and the reservation with nothing left pointing at them.
#[tokio::test]
#[serial]
async fn an_upstream_abort_failure_keeps_the_row_for_the_next_run() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        let upload = g.start_upload(&signer, "big.bin").await;
        g.mock.push(etag_ok("\"p1\""));
        g.upload_part(&signer, "big.bin", &upload, 1, &vec![0u8; 500]).await;
        g.backdate_upload(&upload, 30).await;

        g.mock.push(canned(503, b"nope"));
        g.run_task("cleanup_multipart").await;

        assert!(g.upload_row(&upload).await.is_some(), "the row was dropped after a failure");
        assert_eq!(g.bucket_row("media-cdn").await.reserved_bytes, 500);
    })
    .await;
}

/// One stale upload failing must not stop the others from being cleaned.
#[tokio::test]
#[serial]
async fn one_failure_does_not_stop_the_sweep() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        let a = g.start_upload(&signer, "a.bin").await;
        let b = g.start_upload(&signer, "b.bin").await;
        g.backdate_upload(&a, 30).await;
        g.backdate_upload(&b, 30).await;

        g.mock.push(canned(503, b"nope"));   // a fails
        g.mock.push(canned(204, b""));       // b succeeds

        g.run_task("cleanup_multipart").await;

        let remaining = g.upload_rows().await.len();
        assert_eq!(remaining, 1, "the sweep stopped at the first failure");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn cleanup_audit_deletes_only_what_is_past_retention() {
    with_gateway(|g| async move {
        g.seed_audit("fresh", 1).await;
        g.seed_audit("old", 200).await;

        g.run_task("cleanup_audit").await;

        let rows = g.audit_rows().await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].request_id, "fresh");
    })
    .await;
}
```

`one_failure_does_not_stop_the_sweep` bắt lỗi kinh điển của một task quét: `?` trong vòng lặp làm cả lượt dừng ở phần tử đầu hỏng, và những phần tử sau không bao giờ được dọn — mà log thì vẫn có một dòng lỗi trông như đã xử lý.

- [ ] **Step 2: Chạy để chắc nó fail, viết**

```rust
async fn run(&self, ctx: &AppContext, _vars: &task::Vars) -> Result<()> {
    let days = env_days("OSG_MULTIPART_TTL_DAYS", 7);
    let stale = multipart_uploads::Model::older_than(&ctx.db, days).await?;

    let mut aborted = 0;
    let mut failed = 0;
    for upload in stale {
        // Per-upload error handling on purpose: `?` here would stop the sweep at the first
        // failure and leave every later upload uncleaned, with one log line that looks like the
        // run completed.
        match abort_one(ctx, &upload).await {
            Ok(()) => aborted += 1,
            Err(e) => {
                failed += 1;
                tracing::warn!(
                    upload = %upload.pid, error = %e,
                    "could not abort a stale multipart upload; keeping the row for the next run"
                );
            }
        }
    }

    tracing::info!(aborted, failed, ttl_days = days, "multipart cleanup finished");
    println!("aborted {aborted} stale upload(s), {failed} failed");
    Ok(())
}
```

`abort_one` theo đúng thứ tự của G6 task 2: upstream Abort → `quota::release` → xoá row. Upstream lỗi thì `return Err` và **không** làm hai bước sau.

- [ ] **Step 3: Chạy task thật, ba backend, commit**

```bash
cargo loco task
DB_TYPE=postgres cargo loco task cleanup_multipart
DB_TYPE=postgres cargo loco task cleanup_audit
cargo test --test mod tasks 2>&1 | tail -5
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test 2>&1 | tail -3
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test 2>&1 | tail -3
```

`README.md` thêm bảng task và env:

```markdown
| Task | Việc | Env |
|---|---|---|
| `reconcile_quota` | đối chiếu counter với bảng `objects` | — |
| `cleanup_multipart` | abort upload upstream bỏ dở, trả phần đã giữ | `OSG_MULTIPART_TTL_DAYS`, default 7 |
| `cleanup_audit` | xoá `audit_logs` quá hạn lưu | `OSG_AUDIT_RETENTION_DAYS`, default 90 |

Cả ba nên chạy theo lịch, giờ thấp điểm. `reconcile_quota` xoá sạch
`reserved_bytes`, nên một upload đang bay mất phần giữ — commit của nó cộng lại
ngay, nhưng có một khoảng ngắn quota nới lỏng hơn thực tế.
```

```bash
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add src/ tests/ README.md
git commit -m "feat(tasks): cleanup_multipart and cleanup_audit

Each stale upload is aborted upstream before its hold is released: releasing
first would credit quota for parts still occupying the store. A failure keeps the
row for the next run rather than dropping it, which would leak both the parts and
the reservation with nothing pointing at them.

Errors are handled per upload, not with ? — that would stop the sweep at the
first failure and leave every later upload uncleaned, behind one log line that
looks like the run completed."
```

---

## Task 6: Đọc audit cho admin

**Files:**
- Create: `src/controllers/admin_audit.rs`, `src/views/audit.rs`
- Modify: `src/controllers/mod.rs`, `src/views/mod.rs`, `src/app.rs`
- Test: `tests/requests/admin_audit.rs`

**Interfaces:**
- Consumes: `AdminCaller`, `audit_logs::Model`.
- Produces: `GET /api/admin/audit` với lọc theo `user`, `outcome`, `action`, `since`, `limit`.

Audit ghi vào bảng mà không có đường đọc là một bảng chỉ lớn lên. Task này nhỏ nhưng nó là lý do tồn tại của cả G7.

- [ ] **Step 1: Viết test**

```rust
#[tokio::test]
#[serial]
async fn an_admin_can_filter_the_audit_log() {
    request::<App, _, _>(|request, ctx| async move {
        let admin = prepare_data::init_admin_login(&request, &ctx).await;
        seed_audit_rows(&ctx).await;   // ok / denied / quota_exceeded

        let (k, v) = prepare_data::auth_header(&admin.token);
        let all = request.get("/api/admin/audit").add_header(k, v).await;
        assert_eq!(all.json::<Vec<serde_json::Value>>().len(), 3);

        let (k, v) = prepare_data::auth_header(&admin.token);
        let denied = request
            .get("/api/admin/audit?outcome=denied")
            .add_header(k, v)
            .await;
        assert_eq!(denied.json::<Vec<serde_json::Value>>().len(), 1);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_plain_user_cannot_read_the_audit_log() {
    request::<App, _, _>(|request, ctx| async move {
        let user = prepare_data::init_user_login(&request, &ctx).await;
        let (k, v) = prepare_data::auth_header(&user.token);

        let res = request.get("/api/admin/audit").add_header(k, v).await;

        assert_eq!(res.status_code(), 403);
        assert!(res.text().contains("admin_required"));
    })
    .await;
}

/// The default limit exists so one query cannot pull a hundred million rows into memory.
#[tokio::test]
#[serial]
async fn the_limit_is_capped() {
    request::<App, _, _>(|request, ctx| async move {
        let admin = prepare_data::init_admin_login(&request, &ctx).await;

        let (k, v) = prepare_data::auth_header(&admin.token);
        let res = request
            .get("/api/admin/audit?limit=999999")
            .add_header(k, v)
            .await;

        assert_eq!(res.status_code(), 200);
        // Cap is 1000; with no rows this just proves the parameter did not blow up.
        assert!(res.json::<Vec<serde_json::Value>>().len() <= 1000);
    })
    .await;
}
```

- [ ] **Step 2: Viết, chạy, commit**

`AuditResponse` không chứa gì mới ngoài các cột — nhưng `ip` và `user_agent` là dữ liệu cá nhân, nên chỉ admin đọc được, và `README.md` phải nói rõ chúng được lưu bao lâu (`OSG_AUDIT_RETENTION_DAYS`).

```bash
cargo test --test mod requests::admin_audit 2>&1 | tail -5
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add src/ tests/
git commit -m "feat(admin): read the audit log

An audit table with no way to read it is a table that only grows. Filters on
outcome, action, user and time, capped at 1000 rows so one query cannot pull the
whole history into memory. ip and user_agent are personal data, so admin-only,
and the retention window is documented."
```

---

## Task 7: Cắm bộ conformance

**Files:**
- Modify: `tests/s3/conftest.py`, `tests/s3/README.md`, `.github/workflows/ci.yaml`, `docs/docker.md`

**Interfaces:**
- Consumes: toàn bộ G1–G6.
- Produces: 61/61 test xanh với `OSG_S3_TARGET=gateway`.

- [ ] **Step 1: Ghi golden file**

`tests/s3/golden/upstream.json` hiện rỗng, và 4 test dựa vào nó. Ghi nó với upstream thật trước:

```bash
cd tests/s3
cp .env.example .env    # điền credential S3 thật, chỉ có quyền dưới osg-conformance/
uv run pytest --record-golden
```

Nếu chưa có cờ `--record-golden` thì thêm vào `conftest.py`. Không đoán nội dung golden — cả điểm của nó là ghi lại **hành vi thật của S3**.

- [ ] **Step 2: Đánh dấu `gateway_only` và `upstream_only`**

```python
# tests/s3/conftest.py
def pytest_collection_modifyitems(config, items):
    target = os.environ.get("OSG_S3_TARGET", "upstream")
    for item in items:
        if "gateway_only" in item.keywords and target != "gateway":
            item.add_marker(pytest.mark.skip(reason="gateway only"))
        if "upstream_only" in item.keywords and target != "upstream":
            item.add_marker(pytest.mark.skip(reason="upstream only"))
```

Thêm test `gateway_only` cho hành vi chỉ gateway có:

```python
@pytest.mark.gateway_only
def test_over_quota_put_is_quota_exceeded(gateway_client, small_bucket):
    """QuotaExceeded is not an S3 code. It is the one non-standard code this gateway emits,
    because S3 has no equivalent and a client that cannot read a code cannot act."""
    with pytest.raises(ClientError) as e:
        gateway_client.put_object(Bucket=small_bucket, Key="big", Body=b"x" * 10_000_000)
    assert e.value.response["Error"]["Code"] == "QuotaExceeded"

@pytest.mark.gateway_only
def test_revoking_a_key_invalidates_its_presigned_urls(gateway_client, revocable_key):
    """A property a plain S3 bucket does not have: the gateway looks the key up on every
    request, so revocation is immediate."""
    url = presign_with(revocable_key, "GET", "some-key")
    revoke(revocable_key)
    assert requests.get(url).status_code == 403

@pytest.mark.gateway_only
def test_list_objects_v1_is_not_implemented(gateway_client, bucket):
    with pytest.raises(ClientError) as e:
        gateway_client.list_objects(Bucket=bucket)      # V1
    assert e.value.response["Error"]["Code"] == "NotImplemented"
```

- [ ] **Step 3: Chạy đủ 61 test với gateway**

```bash
# gateway trỏ vào MinIO cục bộ, theo docs/docker.md
cd tests/s3
OSG_S3_TARGET=gateway \
OSG_S3_ENDPOINT=http://localhost:5150 \
OSG_S3_BUCKET=media-cdn \
OSG_S3_ADDRESSING=path \
OSG_S3_KEY_FULL_ID=OSG… OSG_S3_KEY_FULL_SECRET=… \
OSG_S3_KEY_SCOPED_ID=OSG… OSG_S3_KEY_SCOPED_SECRET=… \
  uv run pytest -v
```

Expected: 61 passed (trừ những cái skip vì `upstream_only`, và `test_list_multipart_uploads_filters_by_prefix` nếu upstream là MinIO — `tests/s3/README.md:149` đã ghi).

Mỗi test fail ở đây là một khác biệt **đo được** giữa gateway và S3 thật. Sửa gateway, không sửa test — trừ khi test mã hoá một hành vi mà spec mục 19 đã tuyên bố nằm ngoài phạm vi, và lúc đó nó phải thành `upstream_only` kèm một dòng lý do.

- [ ] **Step 4: `tests/s3/README.md`**

```markdown
## Chạy với gateway

Bộ này giờ có hai đầu. `OSG_S3_TARGET=upstream` chạy với S3 thật và ghi lại S3
hành xử thế nào; `OSG_S3_TARGET=gateway` chạy với gateway, và một khác biệt là
một bug của gateway.

### Marker

- `gateway_only` — hành vi chỉ gateway có: `QuotaExceeded`, thu hồi key làm chết
  presigned URL, `NotImplemented` cho ListObjects V1 và aws-chunked.
- `upstream_only` — hành vi S3 có mà gateway tuyên bố không làm (spec mục 19).

### Đã biết fail

- `test_list_multipart_uploads_filters_by_prefix` fail với MinIO làm upstream.
  Đây là hạn chế của MinIO, không phải của gateway; chạy với S3 thật thì xanh.
```

- [ ] **Step 5: CI (tuỳ chủ repo)**

Thêm một job **không bắt buộc**, chỉ chạy khi có secret:

```yaml
  conformance:
    name: S3 conformance
    runs-on: ubuntu-latest
    if: ${{ github.event_name == 'push' && github.ref == 'refs/heads/main' }}
    continue-on-error: true
    steps:
      - uses: actions/checkout@v4
      # Requires OSG_S3_* in repository secrets. Absent secrets skip the job rather than fail it:
      # a fork must still be able to run CI.
      - name: Skip without credentials
        id: gate
        run: |
          if [ -z "${{ secrets.OSG_S3_KEY_FULL_ID }}" ]; then
            echo "have=false" >> "$GITHUB_OUTPUT"
          else
            echo "have=true" >> "$GITHUB_OUTPUT"
          fi
      # … dựng MinIO, boot gateway, chạy pytest, chỉ khi have=true
```

`continue-on-error: true` và cổng theo secret là cố ý: `docs/superpowers/specs/2026-07-29-s3-conformance-suite-design.md:150` ghi rõ việc bỏ credential vào repo secret là quyết định của chủ repo, không phải của spec.

- [ ] **Step 6: Commit**

```bash
git add tests/s3/ .github/ docs/
git commit -m "test(s3): run the conformance suite against the gateway

61 tests that recorded how real S3 behaves now point at the gateway, where a
difference is a measured bug rather than an argued one. Adds gateway_only cases
for the three behaviours only this gateway has: QuotaExceeded, revocation killing
outstanding presigned URLs, and NotImplemented for ListObjects V1.

The CI job is gated on repository secrets and continue-on-error, because putting
S3 credentials in secrets is the repo owner's call and a fork must still be able
to run CI."
```

---

## Task 8: Đóng sổ

**Files:** `README.md`, `CLAUDE.md`, `docs/superpowers/plans/2026-08-17-go-live-roadmap.md`

- [ ] **Step 1: `README.md` — bảng trạng thái**

```markdown
| SigV4 verify + user/bucket resolution | **done** (G2, G3) |
| Prefix rewrite + backend proxy + S3 verbs | **done** (G3, G4, G5) |
| Quota reserve/commit/release + reconcile | **done** (P5, G4) |
| CopyObject, multipart, presigned | **done** (G6) |
| Audit log + background jobs | **done** (G7) |
```

Và bỏ dòng "No S3 endpoint is served yet."

Thêm mục "Bề mặt S3" liệt kê verb đã hỗ trợ và verb trả 501, để không ai phải đọc code mới biết.

- [ ] **Step 2: `CLAUDE.md`**

Sửa mục Status: tầng dữ liệu S3 giờ tồn tại. Thêm ràng buộc:

```markdown
- **Mọi verb S3 mới phải đi qua `S3Request::resolve`.** Không handler nào được tự dựng physical key. `resolve` là chỗ duy nhất authorize và rewrite; nối chuỗi lấy path trong handler là một lỗ rò chéo tenant.
- **Cây route S3 đăng ký sau cùng.** `/{bucket}/{*key}` khớp gần như mọi thứ. Có test canh, nhưng thứ tự trong `App::routes()` là thứ phải giữ.
- **Lỗi upstream parse rồi phát lại, không forward body.** Body lỗi của upstream chứa physical bucket và physical key.
- **Thêm cột timestamp thì khai `TIMESTAMP(6)` trên MySQL.** `audit_logs.occurred_at` là ví dụ, và có test canh precision.
```

- [ ] **Step 3: Roadmap**

Đánh dấu giai đoạn 6–7 xong, và cập nhật cổng nghiệm thu:

```markdown
**Cổng B — mở cho tenant thứ nhất. ĐÃ ĐẠT.** Một lệnh `aws s3 cp` chạy được,
quota được enforce, cách ly prefix có code chứ không chỉ có schema.

**Cổng C — mở cho tenant thứ hai. ĐÃ ĐẠT.** Audit log có, multipart có, và
`tests/s3/` chạy với `OSG_S3_TARGET=gateway`.
```

Rồi thêm một mục "Còn lại" cho những gì spec mục 19 tuyên bố nằm ngoài phạm vi, để lần sau không phải đi tìm: versioning, aws-chunked, đối chiếu body hash, ListObjects V1, public read, quota theo số object, task xoay master key.

- [ ] **Step 4: Kiểm toàn bộ lần cuối**

```bash
cargo test 2>&1 | tail -3
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test 2>&1 | tail -3
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test 2>&1 | tail -3
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all -- --check
cd frontend && corepack pnpm vitest run && corepack pnpm biome check && corepack pnpm exec tsc --noEmit
```

- [ ] **Step 5: Commit**

```bash
git add README.md CLAUDE.md docs/
git commit -m "docs: the S3 data plane exists

Removes 'No S3 endpoint is served yet' and records the four constraints the data
plane adds: every verb goes through S3Request::resolve, the S3 route tree
registers last, upstream errors are re-emitted rather than forwarded, and a new
timestamp column needs TIMESTAMP(6) on MySQL."
```

---

## Self-review

**Phủ spec.** Mục 13 (audit) → task 1, 2, 3, 6. Mục 14 (rate limit) → task 4. Mục 15 (jobs) → task 5. Mục 17.3 (conformance) → task 7. Mục 18 (tài liệu) → task 8.

**Chưa phủ, cố ý.** Toàn bộ mục 19 của spec — versioning, aws-chunked, đối chiếu body hash, ListObjects V1, public read, quota theo số object, task xoay master key. Task 8 bước 3 ghi chúng vào roadmap để không mồ côi.

**Nhất quán kiểu.** `AuditEntry` khai task 1, dùng task 2. `AuditFacts` khai task 2. `older_than` từ G6 task 1 có caller đầu tiên ở task 5. `ApiOnly` layer khai task 4.

**Rủi ro đã biết.**

1. **`AuditFacts` buộc mỗi handler phải khai nó đã làm gì.** Đó là điểm mạnh, nhưng cũng nghĩa là một handler mới có thể khai sai và audit ghi sai một cách âm thầm. Không có cách ép bằng kiểu; test ở task 2 phủ từng verb, và verb mới phải thêm test.
2. **Redis thành phụ thuộc bắt buộc ở production.** CLAUDE.md từng ghi loco không có `bg_mysql`; giờ điều đó thành một ràng buộc vận hành thật. Deploy MySQL không có Redis sẽ fail lúc boot — tốt hơn là fail âm thầm, nhưng phải nằm trong `docs/docker.md` trước khi ai deploy.
3. **`drain_queue()` trong test dựa vào `BackgroundAsync`.** Nó không kiểm đường Redis thật. Task 3 bước 3 (kiểm bằng tay với Valkey, kể cả lúc Valkey rớt) là chỗ duy nhất kiểm đường đó.
4. **Golden file phải ghi bằng credential thật.** Nếu ghi bằng MinIO thì nó mã hoá hành vi của MinIO, không phải của S3 — và cả điểm của bộ conformance mất. Task 7 bước 1 phải chạy với S3 thật.
5. **Bộ conformance có thể lộ ra khác biệt mà spec chưa quyết.** Lúc đó dừng lại, ghi vào spec, rồi mới sửa — đừng đổi test cho khớp code.
