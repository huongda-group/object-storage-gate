"""PutObject, GetObject, HeadObject, DeleteObject, DeleteObjects."""

from __future__ import annotations

import datetime

from _helpers import headers_of, s3_error, single_part_etag, status

BODY = b"hello object storage gate"


def test_put_then_get_round_trips_body_and_etag(s3_full, bucket, sandbox):
    key = f"{sandbox}/plain.bin"
    put = s3_full.put_object(Bucket=bucket, Key=key, Body=BODY)

    assert status(put) == 200
    # A single-part upload always ETags as the quoted md5 of the body. Clients
    # rely on this to verify uploads without re-downloading.
    assert put["ETag"] == single_part_etag(BODY)

    got = s3_full.get_object(Bucket=bucket, Key=key)
    assert status(got) == 200
    assert got["Body"].read() == BODY
    assert got["ContentLength"] == len(BODY)
    assert got["ETag"] == put["ETag"]


def test_put_preserves_content_type_and_user_metadata(s3_full, bucket, sandbox):
    key = f"{sandbox}/typed.json"
    s3_full.put_object(
        Bucket=bucket,
        Key=key,
        Body=b'{"ok":true}',
        ContentType="application/json",
        Metadata={"Tenant": "acme", "Batch": "17"},
    )

    head = s3_full.head_object(Bucket=bucket, Key=key)
    assert head["ContentType"] == "application/json"
    # S3 lowercases user metadata keys on the way out.
    assert head["Metadata"] == {"tenant": "acme", "batch": "17"}
    assert "x-amz-meta-tenant" in headers_of(head)


def test_default_content_type_matches_upstream(s3_full, bucket, sandbox, golden):
    """Providers disagree here, so the value is recorded rather than asserted."""
    key = f"{sandbox}/untyped.bin"
    s3_full.put_object(Bucket=bucket, Key=key, Body=BODY)

    head = s3_full.head_object(Bucket=bucket, Key=key)
    golden.check("default_content_type", head["ContentType"])


def test_put_overwrites_in_place(s3_full, bucket, sandbox):
    key = f"{sandbox}/overwritten.bin"
    s3_full.put_object(Bucket=bucket, Key=key, Body=b"first")
    second = s3_full.put_object(Bucket=bucket, Key=key, Body=b"second version")

    got = s3_full.get_object(Bucket=bucket, Key=key)
    assert got["Body"].read() == b"second version"
    assert got["ETag"] == second["ETag"] == single_part_etag(b"second version")

    # One key, one object — overwriting must not leave a second listing entry.
    listing = s3_full.list_objects_v2(Bucket=bucket, Prefix=key)
    assert listing["KeyCount"] == 1


def test_head_object_reports_size_and_last_modified(s3_full, bucket, sandbox):
    key = f"{sandbox}/headed.bin"
    s3_full.put_object(Bucket=bucket, Key=key, Body=BODY)

    head = s3_full.head_object(Bucket=bucket, Key=key)
    assert status(head) == 200
    assert head["ContentLength"] == len(BODY)
    assert head["ETag"] == single_part_etag(BODY)
    assert isinstance(head["LastModified"], datetime.datetime)
    assert head["LastModified"].tzinfo is not None


def test_get_range_returns_partial_content(s3_full, bucket, sandbox):
    key = f"{sandbox}/ranged.bin"
    s3_full.put_object(Bucket=bucket, Key=key, Body=BODY)

    got = s3_full.get_object(Bucket=bucket, Key=key, Range="bytes=0-4")
    assert status(got) == 206
    assert got["Body"].read() == BODY[:5]
    assert got["ContentLength"] == 5
    assert got["ContentRange"] == f"bytes 0-4/{len(BODY)}"
    assert got["AcceptRanges"] == "bytes"


def test_get_range_suffix_reads_the_tail(s3_full, bucket, sandbox):
    key = f"{sandbox}/tail.bin"
    s3_full.put_object(Bucket=bucket, Key=key, Body=BODY)

    got = s3_full.get_object(Bucket=bucket, Key=key, Range="bytes=-4")
    assert got["Body"].read() == BODY[-4:]


def test_get_if_none_match_on_current_etag_is_304(s3_full, bucket, sandbox):
    key = f"{sandbox}/conditional.bin"
    etag = s3_full.put_object(Bucket=bucket, Key=key, Body=BODY)["ETag"]

    with s3_error() as err:
        s3_full.get_object(Bucket=bucket, Key=key, IfNoneMatch=etag)
    assert err["status"] == 304

    # A stale ETag must still serve the body.
    stale = '"00000000000000000000000000000000"'
    got = s3_full.get_object(Bucket=bucket, Key=key, IfNoneMatch=stale)
    assert got["Body"].read() == BODY


def test_get_if_modified_since_in_the_future_is_304(s3_full, bucket, sandbox):
    key = f"{sandbox}/fresh.bin"
    s3_full.put_object(Bucket=bucket, Key=key, Body=BODY)
    future = datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(hours=1)

    with s3_error() as err:
        s3_full.get_object(Bucket=bucket, Key=key, IfModifiedSince=future)
    assert err["status"] == 304


def test_get_missing_key_is_no_such_key(s3_full, bucket, sandbox):
    with s3_error("NoSuchKey") as err:
        s3_full.get_object(Bucket=bucket, Key=f"{sandbox}/absent.bin")
    assert err["status"] == 404


def test_head_missing_key_is_bare_404(s3_full, bucket, sandbox):
    # HEAD carries no body, so there is no error code to parse — botocore
    # surfaces the status as the code. Clients branch on this.
    with s3_error("404") as err:
        s3_full.head_object(Bucket=bucket, Key=f"{sandbox}/absent.bin")
    assert err["status"] == 404


def test_delete_object_is_idempotent(s3_full, bucket, sandbox):
    key = f"{sandbox}/deletable.bin"
    s3_full.put_object(Bucket=bucket, Key=key, Body=BODY)

    assert status(s3_full.delete_object(Bucket=bucket, Key=key)) == 204
    # Deleting what is not there is a success, not a 404.
    assert status(s3_full.delete_object(Bucket=bucket, Key=key)) == 204
    with s3_error("NoSuchKey"):
        s3_full.get_object(Bucket=bucket, Key=key)


def test_delete_objects_verbose_reports_every_key(s3_full, bucket, sandbox):
    keys = [f"{sandbox}/batch-{i}.bin" for i in range(3)]
    for key in keys:
        s3_full.put_object(Bucket=bucket, Key=key, Body=BODY)
    absent = f"{sandbox}/batch-absent.bin"

    result = s3_full.delete_objects(
        Bucket=bucket,
        Delete={"Objects": [{"Key": k} for k in keys + [absent]], "Quiet": False},
    )

    assert "Errors" not in result
    # A key that was already gone is still reported as deleted.
    assert sorted(d["Key"] for d in result["Deleted"]) == sorted(keys + [absent])


def test_delete_objects_quiet_omits_the_deleted_list(s3_full, bucket, sandbox):
    key = f"{sandbox}/quiet.bin"
    s3_full.put_object(Bucket=bucket, Key=key, Body=BODY)

    result = s3_full.delete_objects(
        Bucket=bucket, Delete={"Objects": [{"Key": key}], "Quiet": True}
    )

    assert "Deleted" not in result
    assert "Errors" not in result
    assert s3_full.list_objects_v2(Bucket=bucket, Prefix=key)["KeyCount"] == 0
