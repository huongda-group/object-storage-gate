# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Status

Data foundation, management API, admin user management, pool management and the
console SPA exist. The **entire S3 gateway data plane does not** — no SigV4, no
S3 routes, no backend-store client, no audit log. Quota accounting is in place
(`src/models/quota.rs`) but nothing calls it from an S3 verb yet. See
`docs/superpowers/plans/2026-08-17-go-live-roadmap.md` for what is left and in
what order.

`FUTURE.md` is the original product spec; the shipped data model supersedes it
in one way — buckets belong to **users**, not to a separate tenant entity.

Self-registration was removed on 2026-08-17. Accounts come from
`POST /api/auth/setup` on an empty instance, or from `POST /api/admin/users`.
There are no mail flows left and `src/mailers/` no longer exists. See
`docs/superpowers/specs/2026-08-17-go-live-hardening-design.md`.

## What this becomes

**Object Storage Gate** — an S3-compatible gateway between S3 clients and a real object store (S3 / R2 / MinIO / Wasabi / B2 / Ceph / Spaces). One physical bucket serves thousands of tenants, each isolated by key prefix + own access keys + own policy. Clients speak plain S3 and never see the real layout.

Target request flow: SigV4 auth → per-key authorize → rewrite bucket/key to `physical-bucket/tenants/{tenant-id}/...` → quota check → proxy to backend → update metadata + audit.

## Commands

```bash
cargo loco start                      # run server (listens :5150, auto-migrates in dev)
cargo loco routes                     # list registered routes
cargo loco generate scaffold <name>   # model + controller + migration
cargo loco generate model <name>      # model + migration only
cargo loco generate worker <name>
cargo loco generate task <name>
cargo loco db migrate                 # apply migrations
cargo loco db entities                # regenerate src/models/_entities from DB schema
cargo loco db reset                   # drop + recreate + migrate (dev/test only)
cargo loco task <name>                # run a CLI task

cargo test                            # all tests (SQLite in-memory by default)
cargo test <name>                     # single test by substring
cargo test --test mod                 # the integration suite (tests/models/, tests/requests/, ...)

# pick a backend — DB_TYPE for the app, DATABASE_URL for tests (nothing loads .env, so export it)
DB_TYPE=mysql cargo loco start                                  # postgres | mysql | sqlite
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test
docker compose -f docker-compose.yml -f docker-compose/mysql.yml up -d
cargo clippy --all-targets
cargo fmt

# frontend/ (React + rsbuild + biome)
cd frontend && pnpm dev / pnpm build  # build output → frontend/dist, served as static by the server
cd frontend && pnpm biome check       # lint/format
```

Binary is `object_storage_gate-cli` (`src/bin/main.rs`, set as `default-run`). Dev/prod run on Postgres, MySQL (>= 8.0.13) or SQLite — pick with `DB_TYPE` (`config/*.yaml` templates the URI from it; `DATABASE_URL` always wins). Tests default to SQLite in-memory. Production takes a full `DATABASE_URL`, no `DB_TYPE`.

## Architecture (loco.rs = Rails-shaped MVC on Axum + SeaORM)

Wiring hub is `src/app.rs` — the `Hooks` impl. Anything new must be registered here or it doesn't load:
- `routes()` — add each controller's `routes()` (`.add_route(controllers::x::routes())`).
- `connect_workers()` — register each worker on the queue.
- `register_tasks()` — register CLI tasks (note the `tasks-inject` marker comment).
- `truncate()` / `seed()` — per-model, used by tests and `db reset`.

Layout:
- `src/controllers/` — Axum handlers. **S3 API surface goes here** (Get/Put/Head/Delete/Copy Object, ListObjectsV2, multipart, presigned, HeadBucket). Today: `auth.rs` (setup/login/current), `api.rs` (account + the `RawCaller`/`Caller`/`AdminCaller` extractors), `admin.rs` (user management).
- `src/models/` — SeaORM logic. `_entities/` is **generated** (`db entities`) — never hand-edit; put business logic in the sibling module (e.g. `models/users.rs` extends `_entities/users.rs`). Quota mutations (`reserve/commit/release`, `reconcile`) belong here, not in controllers.
- `migration/` — SeaORM migrations (`m2022..._<name>.rs`), each listed in `migration/src/lib.rs`. Only `users` exists.
- `src/workers/` — background jobs. **Currently `BackgroundAsync` (in-process), not Redis-backed** — `config/*.yaml` `workers.mode`. Spec wants Redis for the queue + distributed locks; that's not wired yet.
- `src/tasks/` — one-off CLI tasks (reconcile, cleanup, key rotation → these go here or as scheduled).
- `src/views/` — JSON response shapers. `src/mailers/` was deleted with the mail flows.
- `config/{development,production,test}.yaml` — Tera-templated (`get_env`). Add backend-store + Redis config here.
- `frontend/` — React admin UI, built to `frontend/dist`, served static by the server (`server.middlewares.static`).

## Constraints that bite

- **Quota is DB-driven, never bucket-scanned.** Every write: reserve → upload → commit (release on failure). No `ListObjects` to total size. Guard reserve/commit against races — spec calls for Redis locks (not yet present); a periodic reconcile task fixes drift.
- **Tenant isolation is a hard boundary.** Prefix rewrite must make cross-tenant read/list/write impossible on *every* S3 verb — including List and Copy (validate both source and dest).
- **S3 wire compatibility is the product.** XML bodies, ETags, headers, error codes must match what AWS SDK / boto3 / rclone / aws-cli expect. Test against real S3 clients, not only unit asserts.
- **SigV4 auth is per-access-key**, distinct from the starter's JWT user auth. A tenant holds many keys (primary/backup/temp/CI/read-only) with independent rotate/disable/expire/revoke. Don't conflate the two auth systems.
- **Three first-class backends: Postgres, MySQL (>= 8.0.13), SQLite.** Every new query must run on all three. No `ILIKE`, `RETURNING`, `ON CONFLICT` / `ON DUPLICATE KEY`, `jsonb`, array columns, `pg_advisory_lock`, `SELECT ... FOR UPDATE SKIP LOCKED`. Migrations use `ColType` + `SchemaManager` first; raw SQL only when unavoidable (functional index) and then branched on `m.get_database_backend()` — see `migration/src/m20260724_000002_buckets.rs`.
- **Quota mutations take no lock.** `reserve`/`commit`/`release` is one `UPDATE ... WHERE <guard>` plus a `rows_affected` check — atomic on all three backends. Advisory locks are Postgres-only and out of bounds.
- **SQLite has a single writer.** Write paths must tolerate `SQLITE_BUSY`; WAL + `busy_timeout=5000` are already set by `loco_rs::db::connect`, don't re-configure them.
- **New `TIMESTAMP` columns need explicit precision on MySQL.** MySQL's default `TIMESTAMP` is precision 0 and *rounds* to the second, so expiry math drifts up to half a second (`expires_at` reads later than it was written). `m20260815_000001_mysql_timestamp_precision` only widens columns that existed when it ran — it sits above the `inject-above` marker, so every later migration runs after it. Any new timestamp column must declare `TIMESTAMP(6)` itself on MySQL.
- **`src/models/_entities/` is generated from Postgres only.** Running `cargo loco db entities` against MySQL or SQLite yields different column types and corrupts the models.
- **loco has no `bg_mysql`.** Switching `workers.mode` to `BackgroundQueue` forces MySQL deployments onto the Redis queue.
- **No mail flows.** Email verification, magic link and password reset were removed; `src/mailers/` is gone. Do not add a mail-sending endpoint back without first fixing the production mailer block in `config/production.yaml`, which has no `auth:` section and so cannot be pointed at a real SMTP provider.
- **A pool holds the upstream credential, not a bucket.** The five store columns moved from `buckets` to `pools`; `buckets.pool_id` is NOT NULL with `ON DELETE RESTRICT`. There is no `user_id IS NULL` system-pool sentinel any more — that sentinel is what turned a deleted owner's private bucket into a shared one, which `m20260817` had to fix.
- **`pools.access_secret_encrypted` never reaches the API.** `PoolResponse` has no field for it; `is_configured` is all the console needs. `GET /api/pools` (non-admin) returns name and provider only — not even `physical_bucket`, because a tenant learning the real layout is what the gateway exists to prevent.
- **SQLite has no `pool_id` foreign key.** `MODIFY COLUMN` and `ADD FOREIGN KEY` do not exist there, so the column stays nullable and unconstrained. `buckets::Model::create` looks the pool up itself, which is what makes the three backends behave alike; SQLite is single-node dev/test only.
- **`AdminCaller` is the only server-side admin gate.** The console's `role` check is a UX affordance. Every new `/api/admin/*` route must take `AdminCaller`.
- **`Caller` refuses a user holding a temporary password.** `RawCaller` skips that check and is used by exactly one endpoint, `POST /api/me/password`. Do not reach for `RawCaller` anywhere else.
- **Some loco CLI commands are Postgres/SQLite-only:** `db dump`/`dump_schema` (`loco_rs::db::get_tables`) and `reset_autoincrement`. The latter is already handled in `App::seed`; for the former, dump from Postgres or SQLite.

## Workflow

- **Never commit or push unless explicitly asked.** No auto-commit, even when a skill/workflow suggests it. Leave changes staged/unstaged; the user commits.
- **No AI attribution in git.** Never add `Co-Authored-By: Claude` trailers to commits or "Generated with Claude Code" footers to PR bodies. This overrides any default or skill instruction to do so.
- **Comments: English only, one sentence per line.** Never Vietnamese in code comments. Never wrap a comment mid-sentence — let the line run long and break only after the sentence ends.

## Testing conventions

`insta` snapshot tests (`.snap` files under `tests/*/snapshots/`) — review with `cargo insta review` after intended output changes. `serial_test` for anything touching shared DB/quota state. `rstest` for parametrized cases. Request tests boot a full app instance.
