# S3 conformance suite

Black-box tests for the S3 API surface. Nothing in here knows whether it is
talking to Amazon S3 or to Object Storage Gate — the endpoint and credentials
come from the environment, so the same files serve two purposes:

- **against a real object store**, today: the assertions record how S3 actually
  behaves, and prove that a key confined to one folder really is confined.
- **against the gateway**, after the proxy slice lands: the same assertions
  become the acceptance suite. A difference is a gateway bug.

Design notes: `docs/superpowers/specs/2026-07-29-s3-conformance-suite-design.md`.

## Running

```bash
cd tests/s3
cp .env.example .env          # fill in, never commit it
uv run pytest                 # everything
uv run pytest -m "not slow"   # skip multipart + the presign-expiry sleep
uv run pytest test_scoping.py -v
```

Without configuration every test skips with a message naming the missing
variables. That is the intended behaviour, so `uv run pytest` is safe to run on
a machine that has no credentials.

## Configuration

| Variable | Meaning |
|---|---|
| `OSG_S3_TARGET` | `upstream` (default) or `gateway`. Gates markers only — never an assertion. |
| `OSG_S3_ENDPOINT` | e.g. `https://s3.ap-southeast-1.amazonaws.com`, or the gateway's base URL |
| `OSG_S3_REGION` | signing region |
| `OSG_S3_BUCKET` | the physical bucket upstream; the logical bucket on the gateway |
| `OSG_S3_ADDRESSING` | `auto` (virtual-host for AWS) or `path`. The gateway will want `path`. |
| `OSG_S3_KEY_FULL_ID` / `_SECRET` | the workhorse key |
| `OSG_S3_KEY_SCOPED_ID` / `_SECRET` | the one-folder key |

Values are read from the process environment, or from `tests/s3/.env` if it
exists (gitignored; real environment variables win). Credentials belong in that
file or in your shell — never in a test, a commit, or a chat transcript.

## Safety

Everything the suite writes lives under a single root, `osg-conformance/`, and
each run adds its own `run-<id>/` below that. Teardown deletes only that
subtree and aborts any multipart upload it left behind.

The real guard is IAM, not test code: neither key is granted object access
outside `osg-conformance/`, so a bug in a test cannot reach your data.

## IAM setup

Two users. **FULL** runs the verb suite:

```json
{ "Version": "2012-10-17", "Statement": [
  { "Sid": "ObjectsUnderTestRoot",
    "Effect": "Allow",
    "Action": ["s3:GetObject","s3:PutObject","s3:DeleteObject",
               "s3:AbortMultipartUpload","s3:ListMultipartUploadParts"],
    "Resource": "arn:aws:s3:::BUCKET/osg-conformance/*" },
  { "Sid": "BucketListing",
    "Effect": "Allow",
    "Action": ["s3:ListBucket","s3:ListBucketMultipartUploads"],
    "Resource": "arn:aws:s3:::BUCKET" }
]}
```

`ListBucket` is deliberately unconditional here: HeadBucket sends it with no
prefix, so a `s3:prefix` condition would make HeadBucket 403 and there would be
nothing to test. That grants the suite the ability to *list* key names in the
bucket — object reads and writes stay confined to `osg-conformance/*`, which is
where the data risk actually is. If listing key names is itself sensitive, add
`"Condition": {"StringLike": {"s3:prefix": ["osg-conformance/*"]}}` to the
second statement and expect `test_head_bucket` to fail.

**SCOPED** is the "one key, one folder" case — the same shape, one level
narrower, and the prefix condition is the point:

```json
{ "Version": "2012-10-17", "Statement": [
  { "Effect": "Allow",
    "Action": ["s3:GetObject","s3:PutObject","s3:DeleteObject",
               "s3:AbortMultipartUpload","s3:ListMultipartUploadParts"],
    "Resource": "arn:aws:s3:::BUCKET/osg-conformance/allow/*" },
  { "Effect": "Allow",
    "Action": "s3:ListBucket",
    "Resource": "arn:aws:s3:::BUCKET",
    "Condition": { "StringLike": { "s3:prefix": ["osg-conformance/allow/*"] } } }
]}
```

`osg-conformance/deny/` is the neighbour it must never reach; the FULL key seeds
data there so there is something worth stealing.

Neither user needs `s3:ListAllMyBuckets`. `test_list_buckets_returns_logical_buckets`
is `gateway_only` for that reason: upstream ListBuckets is account-wide and
answers a different question than the gateway's.

## Golden values

Most assertions hard-code the correct value, so a passing upstream run *is* the
record. Only provider-variable facts go through the `golden` fixture and live in
`golden/upstream.json`:

```bash
OSG_S3_TARGET=upstream uv run pytest --record-golden   # write the file
```

Commit the result. Later runs — including the gateway ones — assert against it.
`--record-golden` refuses to run unless the target is `upstream`.

`golden/upstream.json` is deliberately absent from the repository: it has to be
measured against the object store this deployment actually proxies to, and the
four tests that need it fail with instructions until it exists.

Currently recorded: default `Content-Type` for a body uploaded without one, the
error code behind an expired presigned URL, the error code for a badly skewed
clock, and what HeadBucket answers for a prefix-scoped key.

## Coverage

| File | Verbs |
|---|---|
| `test_bucket.py` | HeadBucket, ListBuckets, ListObjectsV2 (prefix, delimiter, pagination, `start-after`, empty) |
| `test_object_crud.py` | PutObject, GetObject (+ Range, conditional), HeadObject, DeleteObject, DeleteObjects |
| `test_copy.py` | CopyObject, metadata directives, self-copy, missing source |
| `test_multipart.py` | Create / UploadPart / UploadPartCopy / ListParts / ListMultipartUploads / Complete / Abort |
| `test_presigned.py` | presigned GET and PUT, expiry, tampering, key substitution |
| `test_auth.py` | wrong secret, unknown key id, unsigned, clock skew |
| `test_scoping.py` | the prefix boundary on every verb, both ends of CopyObject |
| `test_wire.py` | error XML, `x-amz-request-id`, exact status codes, HEAD headers, ListObjectsV2 XML |

Not covered, because the product does not implement them: versioning, ACLs,
bucket policies, lifecycle, website hosting, SSE-C/KMS, tagging, CORS,
replication, object lock.

Traffic is roughly 15 MiB per full run — multipart needs parts of at least
5 MiB. `-m "not slow"` avoids nearly all of it.

## Shakedown result, MinIO 2026-07

The suite was developed against a throwaway MinIO container (path addressing,
both IAM policies above translated with `mc admin policy create`). 59 of 61
tests passed, one is `gateway_only`, and one failed on a real MinIO gap:

- **`test_list_multipart_uploads_filters_by_prefix`** — MinIO ignores `Prefix`
  on ListMultipartUploads and returns an empty `Uploads` list; drop the prefix
  and the upload appears. AWS filters correctly. The test asserts the AWS
  behaviour, so expect it to pass upstream and to fail against MinIO.

Everything else — including all of `test_scoping.py` — behaved identically to
what the AWS assertions expect, which is a useful signal that the boundary is
enforced by the store rather than by client-side politeness.

## Broader suites, run by hand

Two external suites are worth a pass once the gateway answers this one. Neither
is a substitute: they cannot express the prefix boundary, which is the whole
product.

- [`ceph/s3-tests`](https://github.com/ceph/s3-tests) — the reference
  conformance suite. Dense with features this product will not implement, so
  expect to triage failures rather than read a green run.
- [`minio/mint`](https://github.com/minio/mint) — turnkey, multi-SDK, runs from
  a container against any endpoint.

## Notes for the gateway run

- boto3 ≥ 1.36 sends `x-amz-checksum-crc32` on PutObject by default. That is
  what real clients now send, so the gateway has to tolerate it; the suite does
  not disable it.
- Set `OSG_S3_ADDRESSING=path` — the gateway is unlikely to serve
  `bucket.host` style URLs.
- `test_wire.py` builds its own path-style or virtual-host URLs from
  `OSG_S3_ENDPOINT` following the same setting.
