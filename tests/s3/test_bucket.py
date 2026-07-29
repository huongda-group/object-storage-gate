"""Bucket-level verbs: HeadBucket, ListBuckets, ListObjectsV2."""

from __future__ import annotations

import pytest

from _helpers import status


def _seed(s3, bucket: str, sandbox: str) -> list[str]:
    keys = [
        f"{sandbox}/a.txt",
        f"{sandbox}/b.txt",
        f"{sandbox}/sub/c.txt",
        f"{sandbox}/sub/d.txt",
    ]
    for key in keys:
        s3.put_object(Bucket=bucket, Key=key, Body=key.encode())
    return keys


def test_head_bucket(s3_full, bucket):
    assert status(s3_full.head_bucket(Bucket=bucket)) == 200


@pytest.mark.gateway_only
def test_list_buckets_returns_logical_buckets(s3_full, bucket):
    """The gateway must list the caller's own buckets, never the physical one.

    Upstream cannot answer this: ListBuckets there is account-wide and the
    conformance keys are deliberately not granted s3:ListAllMyBuckets.
    """
    names = [b["Name"] for b in s3_full.list_buckets()["Buckets"]]
    assert bucket in names


def test_list_objects_v2_by_prefix(s3_full, bucket, sandbox):
    keys = _seed(s3_full, bucket, sandbox)
    listing = s3_full.list_objects_v2(Bucket=bucket, Prefix=f"{sandbox}/")

    assert listing["KeyCount"] == len(keys)
    assert listing["IsTruncated"] is False
    assert sorted(obj["Key"] for obj in listing["Contents"]) == sorted(keys)
    # Sizes and ETags come back on the listing, not only on HeadObject.
    by_key = {obj["Key"]: obj for obj in listing["Contents"]}
    assert by_key[keys[0]]["Size"] == len(keys[0].encode())
    assert by_key[keys[0]]["ETag"].startswith('"')


def test_list_objects_v2_delimiter_rolls_up_common_prefixes(s3_full, bucket, sandbox):
    _seed(s3_full, bucket, sandbox)
    listing = s3_full.list_objects_v2(Bucket=bucket, Prefix=f"{sandbox}/", Delimiter="/")

    assert sorted(obj["Key"] for obj in listing["Contents"]) == [
        f"{sandbox}/a.txt",
        f"{sandbox}/b.txt",
    ]
    assert [cp["Prefix"] for cp in listing["CommonPrefixes"]] == [f"{sandbox}/sub/"]
    assert listing["Delimiter"] == "/"


def test_list_objects_v2_paginates_with_continuation_token(s3_full, bucket, sandbox):
    keys = _seed(s3_full, bucket, sandbox)

    first = s3_full.list_objects_v2(Bucket=bucket, Prefix=f"{sandbox}/", MaxKeys=2)
    assert first["IsTruncated"] is True
    assert first["KeyCount"] == 2
    assert first["MaxKeys"] == 2
    token = first["NextContinuationToken"]

    second = s3_full.list_objects_v2(
        Bucket=bucket, Prefix=f"{sandbox}/", MaxKeys=2, ContinuationToken=token
    )
    assert second["IsTruncated"] is False
    assert second["ContinuationToken"] == token

    seen = [obj["Key"] for obj in first["Contents"]] + [obj["Key"] for obj in second["Contents"]]
    assert sorted(seen) == sorted(keys)
    # Keys arrive in lexicographic order, which is what clients page on.
    assert seen == sorted(keys)


def test_list_objects_v2_start_after_excludes_the_marker(s3_full, bucket, sandbox):
    keys = _seed(s3_full, bucket, sandbox)
    listing = s3_full.list_objects_v2(
        Bucket=bucket, Prefix=f"{sandbox}/", StartAfter=f"{sandbox}/a.txt"
    )
    returned = [obj["Key"] for obj in listing["Contents"]]

    assert f"{sandbox}/a.txt" not in returned
    assert sorted(returned) == sorted(keys[1:])


def test_list_objects_v2_empty_prefix_has_no_contents_key(s3_full, bucket, sandbox):
    listing = s3_full.list_objects_v2(Bucket=bucket, Prefix=f"{sandbox}/nothing-here/")

    assert listing["KeyCount"] == 0
    assert listing["IsTruncated"] is False
    # S3 omits Contents entirely rather than sending an empty list; clients that
    # index it blindly break, so the gateway must omit it too.
    assert "Contents" not in listing
