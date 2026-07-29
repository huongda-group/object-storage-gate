"""Configuration and fixtures for the S3 conformance suite.

Nothing here knows whether it is talking to a real object store or to Object
Storage Gate. `OSG_S3_TARGET` only gates markers; assertions are the same at
both ends, which is the point of the suite.
"""

from __future__ import annotations

import json
import os
import pathlib
import re
import uuid
import warnings
from dataclasses import dataclass

import boto3
import pytest
from botocore.config import Config
from botocore.exceptions import ClientError

HERE = pathlib.Path(__file__).parent

# Everything the suite writes lives under this one root. The real guard is IAM:
# neither key is granted object access outside it, so a buggy test cannot reach
# real data.
TEST_ROOT = "osg-conformance"
ALLOW_ROOT = f"{TEST_ROOT}/allow"  # the scoped key's only permitted folder
DENY_ROOT = f"{TEST_ROOT}/deny"  # its neighbour, which it must never reach

GOLDEN_PATH = HERE / "golden" / "upstream.json"

REQUIRED_ENV = (
    "OSG_S3_ENDPOINT",
    "OSG_S3_REGION",
    "OSG_S3_BUCKET",
    "OSG_S3_KEY_FULL_ID",
    "OSG_S3_KEY_FULL_SECRET",
    "OSG_S3_KEY_SCOPED_ID",
    "OSG_S3_KEY_SCOPED_SECRET",
)


def _load_dotenv() -> None:
    """Read tests/s3/.env if present. Real environment variables win."""
    path = HERE / ".env"
    if not path.exists():
        return
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        name, value = line.split("=", 1)
        os.environ.setdefault(name.strip(), value.strip().strip("\"'"))


def pytest_addoption(parser):
    parser.addoption(
        "--record-golden",
        action="store_true",
        help="rewrite golden/upstream.json from this run (OSG_S3_TARGET=upstream only)",
    )


def pytest_configure(config):
    _load_dotenv()
    if config.getoption("--record-golden") and os.environ.get("OSG_S3_TARGET", "upstream") != "upstream":
        raise pytest.UsageError("--record-golden requires OSG_S3_TARGET=upstream")


def pytest_collection_modifyitems(config, items):
    target = os.environ.get("OSG_S3_TARGET", "upstream")
    for item in items:
        if "gateway_only" in item.keywords and target != "gateway":
            item.add_marker(pytest.mark.skip(reason=f"gateway_only, target is {target}"))
        if "upstream_only" in item.keywords and target != "upstream":
            item.add_marker(pytest.mark.skip(reason=f"upstream_only, target is {target}"))


@dataclass(frozen=True)
class S3Config:
    target: str
    endpoint: str
    region: str
    bucket: str
    addressing: str
    full: tuple[str, str]
    scoped: tuple[str, str]


@pytest.fixture(scope="session")
def cfg() -> S3Config:
    missing = [name for name in REQUIRED_ENV if not os.environ.get(name)]
    if missing:
        pytest.skip(
            "S3 conformance suite not configured — missing "
            + ", ".join(missing)
            + ". See tests/s3/README.md."
        )
    return S3Config(
        target=os.environ.get("OSG_S3_TARGET", "upstream"),
        endpoint=os.environ["OSG_S3_ENDPOINT"],
        region=os.environ["OSG_S3_REGION"],
        bucket=os.environ["OSG_S3_BUCKET"],
        addressing=os.environ.get("OSG_S3_ADDRESSING", "auto"),
        full=(os.environ["OSG_S3_KEY_FULL_ID"], os.environ["OSG_S3_KEY_FULL_SECRET"]),
        scoped=(os.environ["OSG_S3_KEY_SCOPED_ID"], os.environ["OSG_S3_KEY_SCOPED_SECRET"]),
    )


def _client(cfg: S3Config, creds: tuple[str, str]):
    return boto3.client(
        "s3",
        endpoint_url=cfg.endpoint,
        region_name=cfg.region,
        aws_access_key_id=creds[0],
        aws_secret_access_key=creds[1],
        config=Config(
            signature_version="s3v4",
            s3={"addressing_style": cfg.addressing},
            retries={"max_attempts": 2, "mode": "standard"},
        ),
    )


@pytest.fixture(scope="session")
def bucket(cfg) -> str:
    return cfg.bucket


@pytest.fixture(scope="session")
def s3_full(cfg):
    """Key confined to `osg-conformance/*` — runs the verb suite."""
    return _client(cfg, cfg.full)


@pytest.fixture(scope="session")
def s3_scoped(cfg):
    """Key confined to `osg-conformance/allow/*` — the one-key-one-folder case."""
    return _client(cfg, cfg.scoped)


@pytest.fixture(scope="session")
def make_client(cfg):
    """Build an extra client — used for deliberately wrong credentials."""

    def _make(key_id: str, secret: str):
        return _client(cfg, (key_id, secret))

    return _make


@pytest.fixture(scope="session")
def run_id() -> str:
    return uuid.uuid4().hex[:12]


@pytest.fixture(scope="session")
def full_root(run_id) -> str:
    return f"{TEST_ROOT}/run-{run_id}"


@pytest.fixture(scope="session")
def allow_root(run_id) -> str:
    return f"{ALLOW_ROOT}/run-{run_id}"


@pytest.fixture(scope="session")
def deny_root(run_id) -> str:
    return f"{DENY_ROOT}/run-{run_id}"


@pytest.fixture
def sandbox(full_root, request) -> str:
    """A prefix nobody else in this run writes to, named after the test."""
    return f"{full_root}/{re.sub(r'[^A-Za-z0-9._-]+', '-', request.node.name)}"


def _purge(client, bucket: str, prefix: str) -> None:
    try:
        pages = client.get_paginator("list_multipart_uploads").paginate(Bucket=bucket, Prefix=prefix)
        for page in pages:
            for upload in page.get("Uploads", []):
                client.abort_multipart_upload(
                    Bucket=bucket, Key=upload["Key"], UploadId=upload["UploadId"]
                )
        pages = client.get_paginator("list_objects_v2").paginate(Bucket=bucket, Prefix=prefix)
        for page in pages:
            keys = [{"Key": obj["Key"]} for obj in page.get("Contents", [])]
            if keys:
                client.delete_objects(Bucket=bucket, Delete={"Objects": keys, "Quiet": True})
    except ClientError as exc:
        # Teardown must not mask a test failure, but silent leftovers cost money.
        warnings.warn(f"could not purge {prefix}: {exc}", stacklevel=1)


@pytest.fixture(scope="session", autouse=True)
def _cleanup(s3_full, bucket, full_root, allow_root, deny_root):
    yield
    for root in (full_root, allow_root, deny_root):
        _purge(s3_full, bucket, root)


class Golden:
    """Values that legitimately differ between object stores.

    Most assertions hard-code the correct AWS value; only genuinely
    provider-variable facts come through here, recorded once from an upstream
    run and then asserted against when the target is the gateway.
    """

    def __init__(self, path: pathlib.Path, record: bool):
        self.path = path
        self.record = record
        self.data = json.loads(path.read_text()) if path.exists() else {}
        self.dirty = False

    def check(self, name: str, value):
        if self.record:
            if self.data.get(name) != value:
                self.data[name] = value
                self.dirty = True
            return value
        if name not in self.data:
            pytest.fail(
                f"golden value {name!r} has never been recorded — run the suite "
                f"against upstream with --record-golden first"
            )
        assert value == self.data[name], (
            f"golden {name}: upstream recorded {self.data[name]!r}, this target returned {value!r}"
        )
        return value

    def flush(self) -> None:
        if not self.dirty:
            return
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.path.write_text(json.dumps(self.data, indent=2, sort_keys=True) + "\n")


@pytest.fixture(scope="session")
def golden(pytestconfig):
    box = Golden(GOLDEN_PATH, record=pytestconfig.getoption("--record-golden"))
    yield box
    box.flush()


@pytest.fixture(scope="session")
def raw(cfg):
    """Issue requests below the SDK: raw XML, exact status, unsigned, skewed clock."""
    from _helpers import signed_request

    def _raw(method: str, key: str = "", **kwargs):
        return signed_request(cfg, method, key, **kwargs)

    return _raw
