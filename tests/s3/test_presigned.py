"""Presigned URLs — the one path where a browser reaches the store with no
Authorization header at all."""

from __future__ import annotations

import time
import urllib.error
import urllib.request

import pytest

BODY = b"presigned payload"


def _fetch(url: str, *, method: str = "GET", data: bytes | None = None):
    request = urllib.request.Request(url, data=data, method=method)
    try:
        with urllib.request.urlopen(request) as response:
            return response.status, response.read()
    except urllib.error.HTTPError as exc:
        return exc.code, exc.read()


def test_presigned_get_serves_the_object_without_credentials(s3_full, bucket, sandbox):
    key = f"{sandbox}/download.bin"
    s3_full.put_object(Bucket=bucket, Key=key, Body=BODY)

    url = s3_full.generate_presigned_url(
        "get_object", Params={"Bucket": bucket, "Key": key}, ExpiresIn=300
    )
    status, body = _fetch(url)

    assert status == 200
    assert body == BODY
    # The signature travels in the query string, never in a header.
    assert "X-Amz-Signature=" in url
    assert "X-Amz-Expires=300" in url


def test_presigned_put_accepts_an_upload(s3_full, bucket, sandbox):
    key = f"{sandbox}/upload.bin"
    url = s3_full.generate_presigned_url(
        "put_object", Params={"Bucket": bucket, "Key": key}, ExpiresIn=300
    )

    status, _ = _fetch(url, method="PUT", data=BODY)

    assert status == 200
    assert s3_full.get_object(Bucket=bucket, Key=key)["Body"].read() == BODY


def test_presigned_url_for_one_key_does_not_open_another(s3_full, bucket, sandbox):
    signed = f"{sandbox}/signed.bin"
    other = f"{sandbox}/other.bin"
    s3_full.put_object(Bucket=bucket, Key=signed, Body=BODY)
    s3_full.put_object(Bucket=bucket, Key=other, Body=b"not yours")

    url = s3_full.generate_presigned_url(
        "get_object", Params={"Bucket": bucket, "Key": signed}, ExpiresIn=300
    )
    status, body = _fetch(url.replace(signed, other))

    assert status == 403
    assert b"SignatureDoesNotMatch" in body


def test_tampered_signature_is_rejected(s3_full, bucket, sandbox):
    key = f"{sandbox}/tampered.bin"
    s3_full.put_object(Bucket=bucket, Key=key, Body=BODY)

    url = s3_full.generate_presigned_url(
        "get_object", Params={"Bucket": bucket, "Key": key}, ExpiresIn=300
    )
    head, _, signature = url.rpartition("X-Amz-Signature=")
    forged = head + "X-Amz-Signature=" + ("0" * len(signature))

    status, body = _fetch(forged)

    assert status == 403
    assert b"<Code>SignatureDoesNotMatch</Code>" in body


@pytest.mark.slow
def test_expired_presigned_url_is_refused(s3_full, bucket, sandbox, golden):
    key = f"{sandbox}/expiring.bin"
    s3_full.put_object(Bucket=bucket, Key=key, Body=BODY)

    url = s3_full.generate_presigned_url(
        "get_object", Params={"Bucket": bucket, "Key": key}, ExpiresIn=1
    )
    time.sleep(2)
    status, body = _fetch(url)

    assert status == 403
    # AWS answers AccessDenied with "Request has expired"; other stores pick
    # different codes, so the exact one is recorded rather than asserted.
    code = body.split(b"<Code>")[1].split(b"</Code>")[0].decode()
    golden.check("expired_presign_code", code)
