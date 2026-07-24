# Data Foundation — Design Spec

**Date:** 2026-07-24
**Slice:** #1 of the Object Storage Gate build (see `FUTURE.md`).
**Scope:** SeaORM migrations + models for `tenants`, `access_keys` (+ policy child tables), and versioned `objects` metadata, plus the AES-GCM secret-crypto helper and app wiring. No S3 API surface, no backend proxy, no SigV4, no quota method bodies, no Redis — those are later slices. This slice produces a compiling, migrated schema with model-level business logic and unit tests.

## Context

The repo is an unmodified loco.rs 0.16 SaaS starter (`User` + JWT + mailers + React frontend). Only the `users` table exists. This slice lays the data layer every later slice depends on. It deliberately builds structure (columns, tables, indexes, version-chain helpers) but leaves runtime enforcement (SigV4 verify, quota reserve/commit, proxying) to slices #2–#4.

## Conventions (all new tables)

- Internal `id` (i32, PK, auto-increment) for FKs/joins **plus** public `pid` (UUID) for paths and API — the loco `users` pattern.
- `created_at` / `updated_at` timestamptz (loco default columns).
- Postgres in dev/prod, SQLite in tests. Column types chosen to work on both (avoid PG-only types except where noted; enums modeled as strings for SQLite compatibility).
- Business logic lives in sibling modules (`models/tenants.rs`, etc.), never in generated `_entities/`. Regenerate entities with `cargo loco db entities` after migrations.

## Secret crypto (`src/models/crypto.rs`)

- Algorithm: AES-256-GCM.
- Master key sourced from config via `get_env` (`config/*.yaml`), 32 bytes (base64 or hex in env). Fail fast at startup if missing/wrong length.
- Stored form in `bytea`: `nonce (12 bytes) || ciphertext || tag`. Random nonce per encryption.
- Two functions: `encrypt(plaintext: &str) -> Vec<u8>` and `decrypt(bytes: &[u8]) -> Result<String>`.
- Rationale: SigV4 recomputes HMAC from the secret at verify time, so the secret must be recoverable — password-style hashing is impossible. Encryption-at-rest means a DB dump alone leaks no usable secrets.

## Table: `tenants`

| column | type | notes |
|---|---|---|
| id | i32 PK | internal |
| pid | uuid unique | public id; used in physical prefix `tenants/{pid}/…` |
| name | string | display name |
| status | string | `active` \| `suspended` |
| versioning_enabled | bool, default false | S3 per-namespace versioning toggle |
| max_bytes | bigint null | quota limit; null = unlimited |
| max_objects | bigint null | quota limit; null = unlimited |
| max_multipart | bigint null | max open multipart uploads; null = unlimited |
| used_bytes | bigint, default 0 | live usage (mutated by slice #4 reserve/commit/release under row lock) |
| reserved_bytes | bigint, default 0 | in-flight reservations |
| object_count | bigint, default 0 | live count of stored versions |
| created_at / updated_at | timestamptz | |

Quota columns live on `tenants` (not a separate 1:1 table) per decision. Contention on the hot usage columns is accepted for now; Redis distributed locks (slice #4) mitigate it. This slice only adds the columns — no mutation logic yet.

## Table: `access_keys`

| column | type | notes |
|---|---|---|
| id | i32 PK | internal |
| pid | uuid unique | public id |
| tenant_id | i32 FK → tenants.id | on delete cascade |
| access_key_id | string unique | the public S3 identity clients send (AKIA-style) |
| secret_encrypted | bytea | AES-GCM(secret) via crypto helper |
| label | string | Secret Management: `primary` \| `backup` \| `temporary` \| `ci` \| `readonly` (free string, not enforced enum) |
| status | string | `active` \| `disabled` \| `revoked` |
| expires_at | timestamptz null | for temporary keys; null = no expiry |
| created_at / updated_at | timestamptz | |

Index: `access_key_id` unique. FK index on `tenant_id`.

### Policy child tables (normalized)

**`access_key_permissions`**
| column | type | notes |
|---|---|---|
| id | i32 PK | |
| access_key_id | i32 FK → access_keys.id | on delete cascade |
| action | string | `read` \| `write` \| `delete` \| `list` \| `multipart` \| `presigned` |

Presence of a row = action granted. Unique `(access_key_id, action)`.

**`access_key_prefixes`**
| column | type | notes |
|---|---|---|
| id | i32 PK | |
| access_key_id | i32 FK → access_keys.id | on delete cascade |
| prefix | string | e.g. `images/*`, `documents/*`. Empty set for a key = full tenant namespace |

Both are read together on every auth check (slice #2); callers cache the loaded policy per request rather than re-querying.

## Table: `objects` (versioned)

| column | type | notes |
|---|---|---|
| id | i32 PK | internal |
| pid | uuid unique | public id |
| tenant_id | i32 FK → tenants.id | on delete cascade |
| object_key | string | logical key the client sees (unrewritten) |
| version_id | uuid | S3 versionId; a fixed sentinel value marks the `null`-version row used when versioning is off |
| is_latest | bool | exactly one latest per (tenant, key) |
| is_delete_marker | bool | delete-without-versionId inserts one |
| size | bigint | bytes; 0 for delete markers |
| etag | string | S3 ETag |
| content_type | string | |
| created_at / updated_at | timestamptz | |

Indexes:
- Partial unique: `(tenant_id, object_key) WHERE is_latest = true` — guarantees one current version per key. (Postgres partial index; on SQLite tests, emulate with a partial index or a plain unique on a computed latest key — migration handles both, tests run SQLite.)
- `(tenant_id, object_key, created_at)` — version-chain listing + ListObjectsV2 prefix scans.

### Versioning behavior (encoded in `models/objects.rs`, consumed by later slices)

- **GET** (no versionId): return latest where `is_latest AND NOT is_delete_marker`; if the latest is a delete marker → 404.
- **PUT**: insert a new version row, set prior latest `is_latest=false`. If tenant `versioning_enabled=false`: overwrite the single sentinel `null`-version row instead of chaining.
- **DELETE** (no versionId): insert a delete-marker version, becomes latest.
- **DELETE** (with versionId): hard-remove that specific version row.
- **Quota accounting** (defined here, mutated in #4): every stored version consumes its `size` in `used_bytes`; delete markers count 0; `object_count` counts stored version rows.

These transitions ship as pure model helpers with unit tests now; the HTTP verbs that call them arrive in slices #3/#5.

## App wiring (`src/app.rs`)

- Register each migration in `migration/src/lib.rs` (above the `inject-above` marker).
- `truncate()`: add each new table.
- `seed()`: seed one demo tenant + one active access key (with a known secret, encrypted) + a read/write/list permission set — used by tests and `db reset`.

## Models / files delivered

- `src/models/crypto.rs` — AES-GCM encrypt/decrypt.
- `src/models/tenants.rs` — create, lookup by pid, status helpers.
- `src/models/access_keys.rs` — `create_key` (generates access_key_id + secret, encrypts), `find_by_access_key_id`, `decrypt_secret`, status/expiry checks, policy loaders (`permissions()`, `prefixes()`).
- `src/models/objects.rs` — version-chain helpers: `put_version`, `latest`, `insert_delete_marker`, `remove_version`, list-by-prefix.
- `src/models/access_key_permissions.rs`, `src/models/access_key_prefixes.rs` — thin, mostly generated.
- Migrations: `m2026…_tenants.rs`, `_access_keys.rs`, `_access_key_permissions.rs`, `_access_key_prefixes.rs`, `_objects.rs`.

## Testing

- `models/crypto.rs`: encrypt→decrypt round-trip; tampered ciphertext fails; wrong key fails.
- `access_keys`: create generates unique id + recoverable secret; expired/disabled/revoked status checks.
- `objects`: PUT chains versions and flips `is_latest`; versioning-off overwrites sentinel; delete marker hides latest from GET; DELETE-with-versionId removes only that row; partial-unique enforces single latest.
- Use `serial_test` for anything touching shared seed/DB state; `insta` snapshots only where output shape is asserted.
- No S3 client / backend tests in this slice.

## Explicitly out of scope (later slices)

- SigV4 verification + tenant resolution middleware (#2).
- Prefix rewrite + backend-store proxy + S3 verbs (#3).
- Quota reserve/commit/release bodies, reconcile task, Redis locks (#4).
- ListObjectsV2 / Copy / multipart tables + logic (#5).
- Audit log table + background jobs (#6).
- Admin API + React UI (#7).
