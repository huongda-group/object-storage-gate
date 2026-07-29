"""CopyObject. Both ends of a copy are an access decision, which is why this
file exists separately from the CRUD one."""

from __future__ import annotations

from _helpers import s3_error, single_part_etag, status

BODY = b"payload to be copied"


def _source(s3, bucket: str, sandbox: str) -> str:
    key = f"{sandbox}/source.bin"
    s3.put_object(
        Bucket=bucket,
        Key=key,
        Body=BODY,
        ContentType="text/plain",
        Metadata={"origin": "source"},
    )
    return key


def test_copy_in_bucket_keeps_bytes_and_etag(s3_full, bucket, sandbox):
    src = _source(s3_full, bucket, sandbox)
    dest = f"{sandbox}/copy.bin"

    result = s3_full.copy_object(
        Bucket=bucket, Key=dest, CopySource={"Bucket": bucket, "Key": src}
    )

    assert status(result) == 200
    # The ETag lives inside CopyObjectResult, not in the headers like PutObject.
    assert result["CopyObjectResult"]["ETag"] == single_part_etag(BODY)
    assert s3_full.get_object(Bucket=bucket, Key=dest)["Body"].read() == BODY


def test_copy_defaults_to_carrying_metadata_over(s3_full, bucket, sandbox):
    src = _source(s3_full, bucket, sandbox)
    dest = f"{sandbox}/copy-metadata.bin"

    s3_full.copy_object(Bucket=bucket, Key=dest, CopySource={"Bucket": bucket, "Key": src})

    head = s3_full.head_object(Bucket=bucket, Key=dest)
    assert head["Metadata"] == {"origin": "source"}
    assert head["ContentType"] == "text/plain"


def test_copy_with_replace_directive_swaps_metadata(s3_full, bucket, sandbox):
    src = _source(s3_full, bucket, sandbox)
    dest = f"{sandbox}/copy-replaced.bin"

    s3_full.copy_object(
        Bucket=bucket,
        Key=dest,
        CopySource={"Bucket": bucket, "Key": src},
        MetadataDirective="REPLACE",
        Metadata={"origin": "rewritten"},
        ContentType="application/octet-stream",
    )

    head = s3_full.head_object(Bucket=bucket, Key=dest)
    assert head["Metadata"] == {"origin": "rewritten"}
    assert head["ContentType"] == "application/octet-stream"
    # Metadata changed, bytes did not.
    assert s3_full.get_object(Bucket=bucket, Key=dest)["Body"].read() == BODY


def test_copy_onto_itself_without_replace_is_rejected(s3_full, bucket, sandbox):
    src = _source(s3_full, bucket, sandbox)

    with s3_error("InvalidRequest") as err:
        s3_full.copy_object(Bucket=bucket, Key=src, CopySource={"Bucket": bucket, "Key": src})
    assert err["status"] == 400

    # With REPLACE it becomes a legal metadata-only update.
    s3_full.copy_object(
        Bucket=bucket,
        Key=src,
        CopySource={"Bucket": bucket, "Key": src},
        MetadataDirective="REPLACE",
        Metadata={"origin": "self-updated"},
    )
    assert s3_full.head_object(Bucket=bucket, Key=src)["Metadata"] == {"origin": "self-updated"}


def test_copy_from_missing_source_is_no_such_key(s3_full, bucket, sandbox):
    with s3_error("NoSuchKey") as err:
        s3_full.copy_object(
            Bucket=bucket,
            Key=f"{sandbox}/copy-of-nothing.bin",
            CopySource={"Bucket": bucket, "Key": f"{sandbox}/absent.bin"},
        )
    assert err["status"] == 404


def test_copy_does_not_remove_the_source(s3_full, bucket, sandbox):
    src = _source(s3_full, bucket, sandbox)
    s3_full.copy_object(
        Bucket=bucket, Key=f"{sandbox}/kept.bin", CopySource={"Bucket": bucket, "Key": src}
    )

    assert s3_full.get_object(Bucket=bucket, Key=src)["Body"].read() == BODY
