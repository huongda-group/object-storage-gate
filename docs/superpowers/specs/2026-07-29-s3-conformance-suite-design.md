# S3 conformance suite — design

Date: 2026-07-29
Status: approved

## Problem

`FUTURE.md` promises an S3 API "fully compatible with the S3 standard", usable
from boto3, aws-cli, rclone and Cyberduck without client changes. Nothing
verifies that claim, and slices #2/#3 (SigV4 auth, prefix rewrite, backend
proxy, verb surface) have not been written yet. Two risks follow:

1. The gateway gets built against a guess of what S3 returns. Error codes, ETag
   formats and status codes are where S3 clients break, and they are exactly the
   details nobody remembers correctly.
2. The upstream store may not support everything the gateway needs. Finding that
   out after the proxy is written is the expensive order.

## Approach

One black-box pytest + boto3 suite in `tests/s3/`, configured entirely from the
environment. It knows nothing about which server answers.

- **Today** it points at a real S3 endpoint through the credentials that a pool
  row holds (`buckets.provider/region/api_endpoint/access_id/access_secret_encrypted`).
  Passing means the assertions encode real S3 behaviour.
- **After slice #3** the same files point at the gateway. A difference is a
  gateway bug, measured rather than argued.

Tests that only make sense at one end carry a marker (`gateway_only`,
`upstream_only`); the files are never forked.

### Why not an existing suite

`ceph/s3-tests` is the industry conformance suite but is dense with features
this product will not implement (ACL, bucket policy, website, lifecycle, SSE-C,
versioning, object lock), so a run against real S3 fails broadly and needs
per-test triage. `minio/mint` is turnkey but cannot express the property that
matters most here — a prefix-scoped key must be unable to reach another
prefix, on every verb including both ends of CopyObject. Both are recorded in
`tests/s3/README.md` as optional manual gates, not as the foundation.

### Why boto3 rather than Rust + aws-sdk-s3

botocore raises `ClientError` carrying the S3 error code, which makes
error-surface assertions short; it is also the strictest common client on XML
and headers, and it is the client `CLAUDE.md` names. Testing the gateway with
the same SDK family it may be implemented against would hide wire bugs. Cost:
one Python toolchain (`uv`) beside the Rust one, isolated in `tests/s3/`.

## Configuration

```
OSG_S3_TARGET       upstream | gateway     # gates markers only, never assertions
OSG_S3_ENDPOINT     https://s3.ap-southeast-1.amazonaws.com
OSG_S3_REGION       ap-southeast-1
OSG_S3_BUCKET       physical bucket today; logical bucket on the gateway later
OSG_S3_ADDRESSING   auto | path            # gateway will be path-style
OSG_S3_KEY_FULL_ID     / OSG_S3_KEY_FULL_SECRET
OSG_S3_KEY_SCOPED_ID   / OSG_S3_KEY_SCOPED_SECRET
```

Read from the process environment, or from `tests/s3/.env` (gitignored).
Credentials never enter the repository, a chat transcript, or a test file.

## Safety

Every object the suite writes lives under one root, `osg-conformance/`. The
guard is IAM, not a flag in the code: neither key is granted anything outside
that root, so a buggy test cannot reach real data. Each run also allocates a
`run_id` and teardown deletes only its own subtree.

Two IAM users are required. **FULL** — runs the verb suite:

```json
{ "Version": "2012-10-17", "Statement": [
  { "Effect": "Allow",
    "Action": ["s3:GetObject","s3:PutObject","s3:DeleteObject",
               "s3:AbortMultipartUpload","s3:ListMultipartUploadParts"],
    "Resource": "arn:aws:s3:::BUCKET/osg-conformance/*" },
  { "Effect": "Allow",
    "Action": ["s3:ListBucket","s3:ListBucketMultipartUploads"],
    "Resource": "arn:aws:s3:::BUCKET" }
]}
```

`ListBucket` is unconditional on the FULL key on purpose. HeadBucket sends it
with no prefix, so an `s3:prefix` condition denies HeadBucket and there is
nothing left to test. The grant exposes key *names* in the bucket; object reads
and writes stay confined to `osg-conformance/*`, which is where the data risk
is. Adding the condition back is a one-line change documented in
`tests/s3/README.md`, at the cost of `test_head_bucket`.

**SCOPED** — the "one key, one folder" case: identical but one level narrower,
`arn:aws:s3:::BUCKET/osg-conformance/allow/*` and
`s3:prefix: ["osg-conformance/allow/*"]`. `osg-conformance/deny/` is the
neighbour it must not reach; the FULL key seeds data there.

This measures one behaviour the gateway would otherwise have to guess:
HeadBucket with the scoped key sends `ListBucket` with no prefix, so AWS denies
it. The gateway has to make the same choice deliberately.

## Test surface

| File | Covers |
|---|---|
| `test_bucket.py` | HeadBucket, ListBuckets, ListObjectsV2: prefix, `delimiter=/` → CommonPrefixes, `max-keys` + continuation token, `start-after`, empty listing |
| `test_object_crud.py` | PutObject (content type, `x-amz-meta-*`, single-part ETag = md5), GetObject (whole, `Range`, `If-None-Match`/`If-Modified-Since` → 304), HeadObject, DeleteObject idempotent 204 on a missing key, DeleteObjects batch quiet + verbose, missing key → `NoSuchKey` 404 |
| `test_copy.py` | CopyObject in-bucket, `MetadataDirective` COPY vs REPLACE, self-copy without REPLACE must fail, missing source → `NoSuchKey` |
| `test_multipart.py` | Create/UploadPart/ListParts/ListMultipartUploads/Complete/Abort, UploadPartCopy, multipart ETag shape `<md5>-N`, non-final part < 5 MiB → `EntityTooSmall`, wrong part ETag → `InvalidPart` |
| `test_presigned.py` | presigned GET and PUT, expired presign → 403, tampered signature → `SignatureDoesNotMatch` |
| `test_auth.py` | wrong secret → `SignatureDoesNotMatch` 403, unknown key id → `InvalidAccessKeyId` 403, unsigned → `AccessDenied`, skewed `x-amz-date` → `RequestTimeTooSkewed` |
| `test_scoping.py` | scoped key: full CRUD inside `allow/`; Get/Put/Delete/Head outside → 403; list with no prefix → 403; `prefix=deny/` → 403; CopyObject blocked with an outside source *and* with an outside destination |
| `test_wire.py` | what the boto3 model hides: `<Error><Code>` body, `x-amz-request-id` present, exact HTTP status, `Content-Length` on HEAD |

Deferred to slice #3, written as `gateway_only`: cross-tenant access with a
second user's key, quota exhaustion → 507, and the requirement that no response
leaks the physical layout (no physical bucket name, no `user_pid`).

## Golden values

Most assertions hard-code the correct AWS value, so a passing upstream run *is*
the record. Only provider-variable values — the error code on a prefix denial,
the multipart ETag, presigned expiry behaviour — go through a `golden` fixture
(~30 lines) backed by `tests/s3/golden/upstream.json`, written with
`--record-golden` and asserted otherwise. The gateway run asserts against the
same file.

No general recorder, no cassettes, no diff report. Add them if the fixture
stops being enough.

## Layout

```
tests/s3/
  README.md              # how to run, env vars, IAM policies, safety notes
  pyproject.toml         # pytest + boto3 only, run with uv
  .env.example
  conftest.py            # config, both clients, sandbox run_id, teardown, golden, markers
  golden/upstream.json
  test_{bucket,object_crud,copy,multipart,presigned,auth,scoping,wire}.py
```

`.gitignore` gains `tests/s3/.env` and `tests/s3/.venv`.

## Out of scope

Versioning, ACL, bucket policy, lifecycle, website, SSE-C/KMS, tagging, CORS,
replication and object lock are not in `FUTURE.md` and are not tested. CI
wiring is deferred: it needs credentials in repository secrets, which is the
owner's call.

Cost: multipart requires parts ≥ 5 MiB, roughly 50 MiB of traffic per run
against real S3.
