# Data Foundation — Design Spec

**Date:** 2026-07-24 (revised — user-owned buckets model)
**Slice:** #1 of the Object Storage Gate build.
**Scope:** Data layer for the gateway under the **user-owns-buckets** model: extend the starter `users` table into the account/owner, add `buckets` (first-class, per-user, per-bucket quota), `access_keys` (+ per-key policy child tables), and `objects` metadata (no versioning), plus the AES-256-GCM secret-crypto helper and app wiring. No S3 API, proxy, SigV4 verify, quota-mutation bodies, or Redis — later slices.

## Model (supersedes FUTURE.md's single-virtual-bucket framing)

- **A `user` is the account/tenant.** Reuse the starter `users` table + JWT auth; add a `role` column (`admin` | `user`). Admins manage the system; users own storage.
- **Per-user total quota** on `users`: `max_bytes` where **`0` = unlimited**.
- **Buckets are first-class.** One user owns many buckets; each bucket has its own quota (`max_bytes`, `0` = unlimited). Bucket names are unique **per user** (two users may both own `photos`); the authenticated access key resolves which user, hence which bucket.
- **Objects belong to a bucket.** One row per `(bucket_id, object_key)` — PutObject overwrites (versioning deferred to a later slice).
- **Quota is two-tier:** a write reserves against both the bucket and the owning user; either at limit rejects. (Reserve/commit/release bodies are slice #4; this slice only lays the columns.)
- Physical backend mapping (slice #3): `physical-bucket/{user_pid}/{bucket_name}/{object_key}`.

## Conventions (new tables)

- Internal `id` (i32 PK) + public `pid` (UUID) for paths/API — the loco `users` pattern. `pid` set in `before_save` on insert.
- `created_at`/`updated_at` auto-added by loco `create_table`.
- Postgres (dev/prod) + SQLite (tests). Status/role/action fields are `String` (validated in Rust), not native enums.
- **Unlimited sentinel is `0`, not NULL.** Quota columns are `BigInteger` NOT NULL default `0`.
- Business logic in `src/models/<name>.rs`; never hand-edit `_entities/`.

## Secret crypto (`src/models/crypto.rs`)

AES-256-GCM. Master key from `OSG_MASTER_KEY` env (base64, 32 bytes) via `OnceLock`, with a documented dev/test fallback. Stored form: `nonce(12) || ciphertext || tag` in a `Blob` column. `encrypt(&str) -> Vec<u8>`, `decrypt(&[u8]) -> Result<String>`. Rationale: SigV4 recomputes HMAC from the plaintext secret at verify time — password hashing is impossible, so encrypt-at-rest.

## `users` (ALTER existing)

Add columns:
| column | type | notes |
|---|---|---|
| role | string, default `user` | `admin` \| `user` |
| max_bytes | bigint, default 0 | total account quota; `0` = unlimited |
| used_bytes | bigint, default 0 | live usage across all the user's buckets |
| reserved_bytes | bigint, default 0 | in-flight reservations |

Model (`src/models/users.rs`, extend): `ROLE_ADMIN`/`ROLE_USER` consts, `is_admin()`, `is_unlimited()` (`max_bytes == 0`).

## `buckets`

| column | type | notes |
|---|---|---|
| id / pid | i32 / uuid | |
| user_id | i32 FK → users | ON DELETE CASCADE |
| name | string | S3 bucket name the client uses |
| max_bytes | bigint, default 0 | per-bucket quota; `0` = unlimited |
| used_bytes / reserved_bytes / object_count | bigint, default 0 | live usage (mutated in slice #4) |

Unique index `(user_id, name)`. Model: `create`, `find_by_user_and_name`, `is_unlimited()`.

## `access_keys`

| column | type | notes |
|---|---|---|
| id / pid | i32 / uuid | |
| user_id | i32 FK → users | ON DELETE CASCADE |
| access_key_id | string unique | public S3 identity clients send |
| secret_encrypted | blob | AES-GCM(secret) |
| label | string, default `primary` | primary/backup/temporary/ci/readonly |
| status | string, default `active` | active/disabled/revoked |
| expires_at | timestamptz null | temporary keys |

Model: `create_key(db, user_id, label) -> (Model, plaintext_secret)`, `find_by_access_key_id`, `decrypt_secret`, `is_usable`, `permissions(db)`, `prefixes(db)`.

### Per-key policy (kept, normalized)

- **`access_key_permissions`** — `(access_key_id FK, action)`; action ∈ read/write/delete/list/multipart/presigned. Presence = granted. Unique `(access_key_id, action)`.
- **`access_key_prefixes`** — `(access_key_id FK, prefix)`, e.g. `images/*`. Empty set = full access to the user's buckets.

## `objects` (no versioning)

| column | type | notes |
|---|---|---|
| id / pid | i32 / uuid | |
| bucket_id | i32 FK → buckets | ON DELETE CASCADE |
| object_key | string | logical key within the bucket |
| size | bigint, default 0 | |
| etag | string | |
| content_type | string, default `application/octet-stream` | |

Unique index `(bucket_id, object_key)` — one row per key, PutObject overwrites. Index `(bucket_id, object_key)` also serves ListObjectsV2 prefix scans (covered by the unique index). Model: `put_object` (upsert), `get`, `delete`, `list_by_prefix`.

## App wiring (`src/app.rs`)

- `truncate()`: add objects → access_key_permissions/prefixes → access_keys → buckets → users (FK order).
- `seed()`: existing users seed keeps working; add a demo bucket fixture owned by a seeded user (optional, for tests).

## Testing

- crypto: round-trip, random nonce, tamper fails, too-short fails.
- users: role/quota defaults, `is_admin`, `is_unlimited`.
- buckets: create, per-user name uniqueness (same name different user OK; duplicate for same user fails), `is_unlimited`.
- access_keys: create generates recoverable secret; status/expiry `is_usable`; policy children load.
- objects: put inserts; put again overwrites same row (unique key); get/delete; list_by_prefix filters by prefix within bucket.
- `serial_test` for shared DB state; SQLite backend.

## Out of scope (later slices)

SigV4 verify + user/bucket resolution (#2); prefix rewrite + backend proxy + S3 verbs (#3); quota reserve/commit/release + reconcile + Redis locks (#4); object versioning, Copy, multipart (#5); audit log + background jobs (#6); admin API + React UI (#7).

---

## Addendum, 2026-07-29 — what the console UI needs on top of slice #1

Comparing `frontend/` (the ported console) against the shipped schema left one
screen unsupported and one status underspecified. Both are covered now.

### `buckets` — owner is nullable, and carries backend-store config

The admin Pool screen (`console-object-storage-gate/project/Admin Buckets.dc.html`)
lists gateway-wide pools with no owner, and edits which object store each bucket
proxies to. Slice #1 had neither.

- `user_id` became a **nullable** reference (`create_table(..., &[("users?", "")])`).
  `NULL` = system pool: gateway-wide, outside every user's quota
  (`Model::create_system`, `is_system()`, `find_system_by_name`).
  FK is `ON DELETE SET NULL`, so the delete-user API (#7) must drop the user's
  buckets first or they silently become system pools.
- Name uniqueness moved to `UNIQUE (COALESCE(user_id, 0), name)`
  (`idx_buckets_owner_name`). A plain `(user_id, name)` index would let two system
  pools share a name, because NULLs compare distinct. Side effect: sea-orm-codegen
  can only resolve the one real column in that index and stamps
  `#[sea_orm(unique)]` on `name` — metadata only, the DB constraint is correct.
- New columns (`m20260724_000007_bucket_backend_store`): `provider` (default
  `internal`, validated against `buckets::PROVIDERS`), `region`, `api_endpoint`,
  `access_id`, `access_secret_encrypted` (AES-GCM, same envelope as
  `access_keys.secret_encrypted`), `public_enabled` (default false).
  `Model::set_store` / `decrypt_store_secret` own the encryption; passing
  `access_secret: None` keeps the stored one so editing a pool doesn't wipe it.

### `access_keys` — `expired` is derived, never stored

The console shows four status pills; the DB stores three
(`active`/`disabled`/`revoked`). Expiry is a function of `expires_at`, so the
backend derives it and the UI never recomputes: `is_expired()`,
`effective_status()` (a revoked key stays revoked after lapsing), and
`days_until_expiry()` for the "Còn 3 ngày" column.

### Still not in the DB, on purpose

- **Physical capacity** behind "trên 4 TiB vật lý" / "oversubscribe 127%" is
  deployment config, not a row. It belongs in `config/*.yaml` and the
  `/api/admin/summary` response (#7).
- **Per-user counters** the admin table shows (bucket count, `4/5` keys) are
  `COUNT` queries the summary endpoints compute (#7); nothing is denormalised.
- **Route identifiers**: the console currently routes by `name` / `access_key_id`
  / `email`. `pid` exists on every table and the API contract (`docs/ui/admin-ui-spec.md` §7)
  uses it; the Pool screen in particular must switch, since bucket names are only
  unique per owner.
