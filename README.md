# Object Storage Gate

An S3-compatible gateway that puts many tenants inside one physical bucket.

Clients speak plain S3 — AWS SDK, boto3, aws-cli, rclone, Cyberduck — against
their own logical bucket with their own access keys. The gateway authenticates
the request, checks the key's policy, rewrites the bucket and key into a private
prefix of the real object store, enforces quota from the database, proxies the
call, and records what happened. The client never sees the physical layout.

Backend store is any S3-compatible service: Amazon S3, Cloudflare R2, MinIO,
Wasabi, Backblaze B2, Ceph RGW, DigitalOcean Spaces.

```
S3 client ──► Object Storage Gate ──► object store
              SigV4 verify              physical-bucket/
              key policy + prefix ACL      {user_pid}/{bucket_name}/{object_key}
              quota reserve/commit
              metadata + audit
```

`FUTURE.md` is the original product spec. The shipped data model differs from it
in one deliberate way: buckets belong to **users**, not to a separate tenant
entity, and the physical prefix is `{user_pid}/{bucket_name}/`. See
`docs/superpowers/specs/2026-07-24-data-foundation-design.md`.

## Status

Built on loco.rs 0.16 (Axum + SeaORM). Delivered in slices; this is what exists
today.

| | State |
|---|---|
| Data foundation — schema, models, encrypted secrets | **done** (slice #1) |
| JWT user auth — register, verify, login, forgot/reset, magic link | **done** (loco starter, extended) |
| Console SPA — every screen; auth, access keys and the API page wired to the real API | **done**; buckets/objects/admin screens still on mocks |
| S3 conformance test suite | **done**, runs against a real store today |
| SigV4 verify + user/bucket resolution | slice #2 |
| Prefix rewrite + backend proxy + S3 verbs | slice #3 |
| Quota reserve/commit/release + reconcile + Redis locks | slice #4 |
| Versioning, CopyObject, multipart | slice #5 |
| Audit log + background jobs | slice #6 |
| Access key REST API + PAT-authenticated account API | **done** (slice #7) |
| Bucket/object/admin REST API for the console | slice #7, remainder |

No S3 endpoint is served yet. The gateway exposes the account API under
`/api/*` and serves `frontend/dist` as static files.

## Quick start

```sh
# Postgres for dev; the server auto-migrates on boot
createdb object-storage-gate_development

cd frontend && pnpm install && pnpm build && cd ..
cargo loco start                 # http://localhost:5150
```

Console dev server with hot reload, proxying `/api` to the Rust server:

```sh
cd frontend && pnpm dev          # http://localhost:3000
```

## Database

Three backends, all first-class:

| Backend  | Minimum      | Notes                                                        |
|----------|--------------|--------------------------------------------------------------|
| Postgres | 14           | dev default                                                   |
| MySQL    | **8.0.13**   | needs functional indexes for `idx_buckets_owner_name`         |
| SQLite   | 3.35         | single writer — fits a one-node deploy                        |

Pick one with `DB_TYPE` (`postgres` | `mysql` | `sqlite`); `config/*.yaml` builds the
default URI from it. `DATABASE_URL` always wins, and production takes a full
`DATABASE_URL` with no `DB_TYPE`. Nothing loads `.env` at runtime, so export the
variable or prefix the command:

```sh
DB_TYPE=mysql cargo loco start
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test
```

Docker Compose keeps the stack (app + valkey) in `docker-compose.yml` and one overlay
per database:

```sh
docker compose -f docker-compose.yml -f docker-compose/postgres.yml up -d
docker compose -f docker-compose.yml -f docker-compose/mysql.yml    up -d
docker compose -f docker-compose.yml -f docker-compose/sqlite.yml   up -d
```

Known ceiling: `objects.object_key` is `varchar(255)` while S3 allows keys up to 1024
bytes. Widening it to 1024 would push the unique index `(bucket_id, object_key)` past
InnoDB's 3072-byte limit under utf8mb4, so that change has to move the unique key onto
a hash column (sha256 hex, 64 chars).

## API

Every endpoint under `/api/*` accepts either the console's JWT or a personal
access token (PAT), so a service reaches the same routes the console does
without logging in. There is no version prefix. The PAT is the account's
`users.api_key`: **one token per account**, and rotating it invalidates the old
one immediately — every service using it starts getting `401` until its config
is updated.

Get the token from the console at `/api`, then:

```sh
export OSG_HOST=http://localhost:5150
export OSG_TOKEN=osg_pat_…              # console → API → Hiện → Copy

curl "$OSG_HOST/api/whoami" -H "Authorization: Bearer $OSG_TOKEN"

curl -X POST "$OSG_HOST/api/keys" \
  -H "Authorization: Bearer $OSG_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"label":"ci","permissions":["read","list"],"prefixes":["ci/"]}'
```

| Method | Path | Purpose |
|---|---|---|
| GET | `/api/whoami` | verify the token; returns pid, email, name, role |
| GET | `/api/keys` | list access keys with their policy |
| POST | `/api/keys` | create a key — the response carries `secret` **once** |
| GET | `/api/keys/{pid}` | one key |
| PATCH | `/api/keys/{pid}` | label, status, `expires_at`, permissions, prefixes |
| POST | `/api/keys/{pid}/rotate` | new key with the same policy; old one goes `disabled` |
| DELETE | `/api/keys/{pid}` | revoke permanently (terminal — no way back) |
| GET | `/api/buckets` | buckets owned by the account |
| GET | `/api/usage` | used / reserved / max bytes, object and bucket counts |

A key belonging to another account returns `404`, not `403`. Permissions are limited
to `read`, `write`, `delete`, `list`, `multipart`, `presigned`; prefixes may not
be absolute or contain `..`.

## What is in the schema

| Table | Holds |
|---|---|
| `users` | account + `role` (`user` \| `admin`) + total quota (`max_bytes`, `used_bytes`, `reserved_bytes`); `0` bytes means unlimited |
| `buckets` | a logical bucket. `user_id` NULL = **system pool**, gateway-wide and outside every user's quota. Per-bucket quota counters and `object_count`. Name unique per owner (`UNIQUE (COALESCE(user_id,0), name)`) |
| `buckets` (store columns) | which object store this bucket proxies to: `provider`, `region`, `api_endpoint`, `access_id`, `access_secret_encrypted`, `public_enabled` |
| `access_keys` | client credentials — `OSG…` id, AES-GCM encrypted secret, `label` (primary / backup / temp / CI / read-only), `status` (`active` \| `disabled` \| `revoked`), optional `expires_at`. A user holds many; rotate one without touching the others |
| `access_key_permissions` | per-key actions: `read`, `write`, `delete`, `list`, `multipart`, `presigned` |
| `access_key_prefixes` | per-key prefix confinement — the "one key, one folder" rule |
| `objects` | metadata per `(bucket_id, object_key)`: `size`, `etag`, `content_type`, timestamps. Quota reads from here, never from scanning the bucket |

Two credential systems, deliberately separate:

- **JWT** authenticates humans in the console (`/api/auth/*`).
- **SigV4 access keys** authenticate S3 clients. The gateway verifies the
  client's `OSG…` key, then re-signs upstream with the bucket's own store
  credentials. Client keys never reach the object store.

Both secret kinds are stored AES-256-GCM encrypted (`src/models/crypto.rs`),
reversible because the gateway has to sign with them.

**Production must set `OSG_MASTER_KEY`** to a base64-encoded 32-byte key.
Without it the code falls back to a hard-coded development key.

## Layout

```
src/
  app.rs                  Hooks impl — the wiring hub. Routes, workers, tasks,
                          truncate and seed register here or they do not load.
  controllers/            Axum handlers. auth.rs today; the S3 verb surface lands here.
  models/                 SeaORM logic. _entities/ is generated — never hand-edit.
                          Quota mutations belong here, not in controllers.
  workers/  tasks/        background jobs (reconcile, cleanup) and CLI tasks
migration/                one file per migration, each listed in migration/src/lib.rs
config/                   development / test / production YAML, Tera-templated
frontend/                 React 19 + TanStack Router console → frontend/dist
tests/                    Rust suites (models, requests, tasks, workers) + tests/s3/
docs/                     design specs, implementation plans, admin UI contract
console-object-storage-gate/  design prototypes each console screen was ported from
```

## Commands

```sh
cargo loco start                      # serve (:5150, auto-migrate in dev)
cargo loco routes                     # list registered routes
cargo loco db migrate                 # apply migrations
cargo loco db entities                # regenerate src/models/_entities from the DB
cargo loco db reset                   # drop + recreate + migrate (dev/test only)
cargo loco generate model <name>      # model + migration
cargo loco task <name>                # run a CLI task

cargo test                            # all Rust tests
cargo test --test mod                 # the integration suite
cargo clippy --all-targets && cargo fmt
```

## Testing

- **Rust** — `insta` snapshots (`cargo insta review` after intended output
  changes), `serial_test` for anything touching shared DB or quota state,
  `rstest` for parametrized cases. Request tests boot a full app. SQLite
  in-memory by default; set `DATABASE_URL` to run the same suite on Postgres or
  MySQL — CI runs all three.
- **Console** — `cd frontend && pnpm test` (Vitest over `src/lib/`), `pnpm lint`.
- **S3 conformance** — `tests/s3/`, pytest + boto3 run with `uv`. One black-box
  suite, two targets: point it at a real object store today to record how S3
  actually behaves, and at the gateway after slice #3, where a difference is a
  gateway bug. It includes the prefix-boundary tests — a scoped key must be
  unable to reach a neighbouring prefix on every verb, including both ends of
  CopyObject. See `tests/s3/README.md`; with no credentials configured it skips
  cleanly.

## Documentation

| File | Contents |
|---|---|
| `FUTURE.md` | product vision (Vietnamese) |
| `CLAUDE.md` | working notes and constraints for contributors |
| `docs/superpowers/specs/` | design specs, one per slice |
| `docs/superpowers/plans/` | step-by-step implementation plans |
| `docs/ui/admin-ui-spec.md` | console behaviour, copy and data shapes |
| `tests/s3/README.md` | running the conformance suite, IAM policies, safety |

## Constraints worth knowing before changing anything

- **Quota is database-driven, never bucket-scanned.** Every write reserves,
  uploads, then commits — releasing on failure. No `ListObjects` to total a
  size. A periodic reconcile task fixes drift.
- **Tenant isolation is a hard boundary.** The prefix rewrite has to make
  cross-tenant read, list and write impossible on *every* verb. Copy validates
  both source and destination.
- **S3 wire compatibility is the product.** XML bodies, ETags, headers and error
  codes must match what real clients expect, which is why they are tested
  against real clients and not only unit asserts.
- **`src/models/_entities/` is generated.** Business logic goes in the sibling
  module (`models/buckets.rs` extends `_entities/buckets.rs`).
