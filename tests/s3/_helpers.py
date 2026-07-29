"""Plumbing shared by the conformance tests. Fixtures live in conftest.py."""

from __future__ import annotations

import hashlib
import urllib.error
import urllib.parse
import urllib.request
from contextlib import contextmanager
from dataclasses import dataclass

import botocore.auth
import pytest
from botocore.auth import S3SigV4Auth
from botocore.awsrequest import AWSRequest
from botocore.credentials import Credentials
from botocore.exceptions import ClientError

# S3 rejects any part but the last below this size.
MIN_PART = 5 * 1024 * 1024


def err_code(exc: ClientError) -> str:
    return exc.response["Error"]["Code"]


def err_status(exc: ClientError) -> int:
    return exc.response["ResponseMetadata"]["HTTPStatusCode"]


def status(resp: dict) -> int:
    return resp["ResponseMetadata"]["HTTPStatusCode"]


def headers_of(resp: dict) -> dict:
    return resp["ResponseMetadata"]["HTTPHeaders"]


@contextmanager
def s3_error(*codes: str):
    """Assert the block raises `ClientError`, optionally with one of `codes`.

    Yields a dict that is filled in once the block has raised, so a test can
    assert further on the code or status:

        with s3_error("AccessDenied") as e: ...
        assert e["status"] == 403
    """
    box: dict = {}
    with pytest.raises(ClientError) as excinfo:
        yield box
    box["exc"] = excinfo.value
    box["code"] = err_code(excinfo.value)
    box["status"] = err_status(excinfo.value)
    if codes:
        assert box["code"] in codes, f"expected one of {codes}, got {box['code']}"


def md5_hex(data: bytes) -> str:
    return hashlib.md5(data).hexdigest()


def single_part_etag(data: bytes) -> str:
    """What S3 reports for an object uploaded in one PUT: the quoted body md5."""
    return f'"{md5_hex(data)}"'


def multipart_etag(parts: list[bytes]) -> str:
    """md5 of the concatenated part digests, then `-<part count>`."""
    joined = b"".join(hashlib.md5(p).digest() for p in parts)
    return f'"{md5_hex(joined)}-{len(parts)}"'


@dataclass(frozen=True)
class Raw:
    """A response read off the wire, before any SDK modelling."""

    status: int
    headers: dict
    body: bytes

    @property
    def text(self) -> str:
        return self.body.decode("utf-8", "replace")


def object_url(cfg, key: str) -> str:
    base = cfg.endpoint.rstrip("/")
    if cfg.addressing == "path":
        root = f"{base}/{cfg.bucket}"
    else:
        scheme, host = base.split("://", 1)
        root = f"{scheme}://{cfg.bucket}.{host}"
    if not key:
        return root + "/"
    return f"{root}/{urllib.parse.quote(key, safe='/')}"


def signed_request(
    cfg,
    method: str,
    key: str,
    *,
    creds: tuple[str, str] | None = None,
    body: bytes = b"",
    headers: dict | None = None,
    query: dict | None = None,
    skew=None,
    sign: bool = True,
) -> Raw:
    """Issue a request without boto3's client layer.

    Needed for the cases the SDK models away: raw error XML, exact status codes,
    an unsigned request, and a signature carrying a skewed timestamp.
    """
    url = object_url(cfg, key)
    if query:
        url += "?" + urllib.parse.urlencode(query)
    request = AWSRequest(method=method, url=url, data=body, headers=dict(headers or {}))
    if sign:
        key_id, secret = creds or cfg.full
        auth = S3SigV4Auth(Credentials(key_id, secret), "s3", cfg.region)
        if skew is None:
            auth.add_auth(request)
        else:
            # botocore stamps the signing time inside add_auth, so the only way to
            # sign for a different clock is to move the clock it reads.
            real = botocore.auth.get_current_datetime
            botocore.auth.get_current_datetime = lambda: real() + skew
            try:
                auth.add_auth(request)
            finally:
                botocore.auth.get_current_datetime = real
    prepared = request.prepare()
    outbound = urllib.request.Request(
        prepared.url,
        data=body or None,
        method=method,
        headers=dict(prepared.headers),
    )
    try:
        with urllib.request.urlopen(outbound) as response:
            return Raw(response.status, dict(response.headers), response.read())
    except urllib.error.HTTPError as exc:
        return Raw(exc.code, dict(exc.headers), exc.read())
