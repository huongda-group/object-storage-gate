"""One key, one folder.

The scoped credential may touch `osg-conformance/allow/` and nothing else.
Every verb has to hold that line — including both ends of CopyObject, which is
the boundary most gateways leak through. On the upstream target the line is
drawn by an IAM policy; on the gateway it will be drawn by
`access_key_prefixes`, and the answers must match.
"""

from __future__ import annotations

import re

import pytest

from _helpers import s3_error, status

BODY = b"scoped payload"


def _box(root: str, request) -> str:
    return f"{root}/{re.sub(r'[^A-Za-z0-9._-]+', '-', request.node.name)}"


@pytest.fixture
def allow_box(allow_root, request) -> str:
    return _box(allow_root, request)


@pytest.fixture
def deny_box(deny_root, request, s3_full, bucket) -> str:
    """A neighbouring folder, seeded by the full key so there is something to steal."""
    box = _box(deny_root, request)
    s3_full.put_object(Bucket=bucket, Key=f"{box}/secret.bin", Body=b"not for the scoped key")
    return box


def test_scoped_key_has_full_crud_inside_its_folder(s3_scoped, bucket, allow_box):
    key = f"{allow_box}/mine.bin"

    assert status(s3_scoped.put_object(Bucket=bucket, Key=key, Body=BODY)) == 200
    assert s3_scoped.get_object(Bucket=bucket, Key=key)["Body"].read() == BODY
    assert s3_scoped.head_object(Bucket=bucket, Key=key)["ContentLength"] == len(BODY)

    listing = s3_scoped.list_objects_v2(Bucket=bucket, Prefix=f"{allow_box}/")
    assert [obj["Key"] for obj in listing["Contents"]] == [key]

    assert status(s3_scoped.delete_object(Bucket=bucket, Key=key)) == 204


def test_scoped_key_cannot_read_outside(s3_scoped, bucket, deny_box):
    with s3_error("AccessDenied") as err:
        s3_scoped.get_object(Bucket=bucket, Key=f"{deny_box}/secret.bin")
    assert err["status"] == 403


def test_scoped_key_cannot_head_outside(s3_scoped, bucket, deny_box):
    # HEAD has no body to carry an error code, so the status is the whole answer.
    # A gateway that answers 404 here leaks nothing but breaks clients that
    # distinguish "gone" from "not yours".
    with s3_error("403") as err:
        s3_scoped.head_object(Bucket=bucket, Key=f"{deny_box}/secret.bin")
    assert err["status"] == 403


def test_scoped_key_cannot_write_outside(s3_scoped, bucket, deny_box):
    with s3_error("AccessDenied") as err:
        s3_scoped.put_object(Bucket=bucket, Key=f"{deny_box}/planted.bin", Body=BODY)
    assert err["status"] == 403


def test_scoped_key_cannot_delete_outside(s3_scoped, s3_full, bucket, deny_box):
    key = f"{deny_box}/secret.bin"

    with s3_error("AccessDenied") as err:
        s3_scoped.delete_object(Bucket=bucket, Key=key)
    assert err["status"] == 403
    # The refusal must be real, not merely reported.
    assert s3_full.head_object(Bucket=bucket, Key=key)["ContentLength"] > 0


def test_scoped_key_cannot_start_a_multipart_upload_outside(s3_scoped, bucket, deny_box):
    with s3_error("AccessDenied") as err:
        s3_scoped.create_multipart_upload(Bucket=bucket, Key=f"{deny_box}/big.bin")
    assert err["status"] == 403


def test_scoped_key_cannot_list_the_whole_bucket(s3_scoped, bucket):
    # No prefix means "show me everything", which is exactly what scoping forbids.
    with s3_error("AccessDenied") as err:
        s3_scoped.list_objects_v2(Bucket=bucket)
    assert err["status"] == 403


def test_scoped_key_cannot_list_another_folder(s3_scoped, bucket, deny_box):
    with s3_error("AccessDenied") as err:
        s3_scoped.list_objects_v2(Bucket=bucket, Prefix=f"{deny_box}/")
    assert err["status"] == 403


def test_scoped_key_can_list_its_own_folder(s3_scoped, bucket, allow_box):
    s3_scoped.put_object(Bucket=bucket, Key=f"{allow_box}/listed.bin", Body=BODY)

    listing = s3_scoped.list_objects_v2(Bucket=bucket, Prefix=f"{allow_box}/")

    assert listing["KeyCount"] == 1


def test_scoped_key_cannot_copy_from_outside(s3_scoped, bucket, allow_box, deny_box):
    with s3_error("AccessDenied") as err:
        s3_scoped.copy_object(
            Bucket=bucket,
            Key=f"{allow_box}/stolen.bin",
            CopySource={"Bucket": bucket, "Key": f"{deny_box}/secret.bin"},
        )
    assert err["status"] == 403


def test_scoped_key_cannot_copy_to_outside(s3_scoped, s3_full, bucket, allow_box, deny_box):
    source = f"{allow_box}/exportable.bin"
    s3_scoped.put_object(Bucket=bucket, Key=source, Body=BODY)
    destination = f"{deny_box}/exfiltrated.bin"

    with s3_error("AccessDenied") as err:
        s3_scoped.copy_object(
            Bucket=bucket, Key=destination, CopySource={"Bucket": bucket, "Key": source}
        )
    assert err["status"] == 403
    with s3_error("404"):
        s3_full.head_object(Bucket=bucket, Key=destination)


def test_scoped_key_can_copy_within_its_folder(s3_scoped, bucket, allow_box):
    source = f"{allow_box}/original.bin"
    s3_scoped.put_object(Bucket=bucket, Key=source, Body=BODY)

    s3_scoped.copy_object(
        Bucket=bucket,
        Key=f"{allow_box}/duplicate.bin",
        CopySource={"Bucket": bucket, "Key": source},
    )

    assert s3_scoped.get_object(Bucket=bucket, Key=f"{allow_box}/duplicate.bin")["Body"].read() == BODY


def test_head_bucket_with_a_scoped_key(s3_scoped, bucket, golden):
    """A decision the gateway would otherwise have to guess at.

    HeadBucket sends ListBucket with no prefix, so a prefix-conditioned IAM
    policy refuses it. Whatever upstream answers, the gateway answers the same.
    """
    try:
        result = status(s3_scoped.head_bucket(Bucket=bucket))
    except Exception as exc:  # noqa: BLE001 — the status is the finding
        result = getattr(exc, "response", {}).get("ResponseMetadata", {}).get("HTTPStatusCode")
    golden.check("head_bucket_scoped_status", result)
