"""Multipart upload: create, upload, list, complete, abort, part-copy.

Every test here moves megabytes, so they carry the `slow` marker and the byte
buffers are module-level and reused. Deselect with `-m "not slow"`.
"""

from __future__ import annotations

import pytest

from _helpers import MIN_PART, multipart_etag, s3_error, status

pytestmark = pytest.mark.slow

BIG = b"A" * MIN_PART  # exactly the smallest legal non-final part
SMALL = b"z" * (1024 * 1024)


def test_multipart_round_trip(s3_full, bucket, sandbox):
    key = f"{sandbox}/multipart.bin"
    upload_id = s3_full.create_multipart_upload(Bucket=bucket, Key=key)["UploadId"]

    parts = []
    for number, chunk in enumerate([BIG, SMALL], start=1):
        uploaded = s3_full.upload_part(
            Bucket=bucket, Key=key, UploadId=upload_id, PartNumber=number, Body=chunk
        )
        parts.append({"ETag": uploaded["ETag"], "PartNumber": number})

    listed = s3_full.list_parts(Bucket=bucket, Key=key, UploadId=upload_id)
    assert [p["PartNumber"] for p in listed["Parts"]] == [1, 2]
    assert [p["Size"] for p in listed["Parts"]] == [len(BIG), len(SMALL)]

    done = s3_full.complete_multipart_upload(
        Bucket=bucket, Key=key, UploadId=upload_id, MultipartUpload={"Parts": parts}
    )
    assert status(done) == 200
    # A multipart ETag is the md5 of the concatenated part digests, then `-N`.
    # Clients use the `-N` suffix to know the object was not uploaded in one go.
    assert done["ETag"] == multipart_etag([BIG, SMALL])
    assert done["ETag"].endswith('-2"')

    head = s3_full.head_object(Bucket=bucket, Key=key)
    assert head["ContentLength"] == len(BIG) + len(SMALL)
    assert head["ETag"] == done["ETag"]
    assert s3_full.get_object(Bucket=bucket, Key=key)["Body"].read() == BIG + SMALL


def test_list_multipart_uploads_filters_by_prefix(s3_full, bucket, sandbox, full_root):
    """Prefix has to filter, not decorate.

    An upload lives outside the object namespace until it completes, so this
    listing is the only way a tenant can see its own pending uploads — and the
    only place the gateway can leak somebody else's. MinIO ignores Prefix here
    and returns nothing, so a failure on this test is worth reading before
    assuming the gateway is at fault.
    """
    mine = f"{sandbox}/pending.bin"
    other = f"{full_root}/elsewhere-{sandbox.rsplit('/', 1)[-1]}/pending.bin"
    ids = {}
    for key in (mine, other):
        ids[key] = s3_full.create_multipart_upload(Bucket=bucket, Key=key)["UploadId"]
    try:
        listing = s3_full.list_multipart_uploads(Bucket=bucket, Prefix=f"{sandbox}/")
        pending = {(u["Key"], u["UploadId"]) for u in listing.get("Uploads", [])}

        assert (mine, ids[mine]) in pending
        assert (other, ids[other]) not in pending
    finally:
        for key, upload_id in ids.items():
            s3_full.abort_multipart_upload(Bucket=bucket, Key=key, UploadId=upload_id)


def test_abort_discards_the_upload_and_writes_nothing(s3_full, bucket, sandbox):
    key = f"{sandbox}/aborted.bin"
    upload_id = s3_full.create_multipart_upload(Bucket=bucket, Key=key)["UploadId"]
    s3_full.upload_part(
        Bucket=bucket, Key=key, UploadId=upload_id, PartNumber=1, Body=SMALL
    )

    assert status(s3_full.abort_multipart_upload(Bucket=bucket, Key=key, UploadId=upload_id)) == 204

    with s3_error("NoSuchUpload"):
        s3_full.list_parts(Bucket=bucket, Key=key, UploadId=upload_id)
    # An aborted upload must leave no object behind, not a truncated one.
    with s3_error("NoSuchKey"):
        s3_full.get_object(Bucket=bucket, Key=key)


def test_non_final_part_below_the_minimum_is_rejected_at_complete(s3_full, bucket, sandbox):
    key = f"{sandbox}/too-small.bin"
    upload_id = s3_full.create_multipart_upload(Bucket=bucket, Key=key)["UploadId"]

    parts = []
    for number in (1, 2):
        uploaded = s3_full.upload_part(
            Bucket=bucket, Key=key, UploadId=upload_id, PartNumber=number, Body=SMALL
        )
        parts.append({"ETag": uploaded["ETag"], "PartNumber": number})

    # UploadPart itself accepts the small part; the size rule is only enforced
    # on completion, which is where the gateway has to enforce it too.
    with s3_error("EntityTooSmall") as err:
        s3_full.complete_multipart_upload(
            Bucket=bucket, Key=key, UploadId=upload_id, MultipartUpload={"Parts": parts}
        )
    assert err["status"] == 400
    s3_full.abort_multipart_upload(Bucket=bucket, Key=key, UploadId=upload_id)


def test_complete_with_a_wrong_part_etag_is_invalid_part(s3_full, bucket, sandbox):
    key = f"{sandbox}/bad-etag.bin"
    upload_id = s3_full.create_multipart_upload(Bucket=bucket, Key=key)["UploadId"]
    s3_full.upload_part(
        Bucket=bucket, Key=key, UploadId=upload_id, PartNumber=1, Body=SMALL
    )

    with s3_error("InvalidPart") as err:
        s3_full.complete_multipart_upload(
            Bucket=bucket,
            Key=key,
            UploadId=upload_id,
            MultipartUpload={
                "Parts": [{"ETag": '"00000000000000000000000000000000"', "PartNumber": 1}]
            },
        )
    assert err["status"] == 400
    s3_full.abort_multipart_upload(Bucket=bucket, Key=key, UploadId=upload_id)


def test_upload_part_copy_takes_a_part_from_an_existing_object(s3_full, bucket, sandbox):
    source = f"{sandbox}/copy-source.bin"
    s3_full.put_object(Bucket=bucket, Key=source, Body=BIG)

    key = f"{sandbox}/assembled.bin"
    upload_id = s3_full.create_multipart_upload(Bucket=bucket, Key=key)["UploadId"]

    copied = s3_full.upload_part_copy(
        Bucket=bucket,
        Key=key,
        UploadId=upload_id,
        PartNumber=1,
        CopySource={"Bucket": bucket, "Key": source},
    )
    # UploadPartCopy nests the part ETag, unlike UploadPart.
    parts = [{"ETag": copied["CopyPartResult"]["ETag"], "PartNumber": 1}]

    tail = s3_full.upload_part(
        Bucket=bucket, Key=key, UploadId=upload_id, PartNumber=2, Body=SMALL
    )
    parts.append({"ETag": tail["ETag"], "PartNumber": 2})

    done = s3_full.complete_multipart_upload(
        Bucket=bucket, Key=key, UploadId=upload_id, MultipartUpload={"Parts": parts}
    )
    assert done["ETag"] == multipart_etag([BIG, SMALL])
    assert s3_full.get_object(Bucket=bucket, Key=key)["Body"].read() == BIG + SMALL
