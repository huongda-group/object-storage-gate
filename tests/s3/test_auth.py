"""SigV4 rejection surface. The gateway verifies signatures itself, so these
codes are a contract it has to match exactly — clients branch on them."""

from __future__ import annotations

import datetime

from _helpers import s3_error

BODY = b"guarded object"


def test_wrong_secret_is_signature_does_not_match(cfg, make_client, bucket, sandbox, s3_full):
    key = f"{sandbox}/guarded.bin"
    s3_full.put_object(Bucket=bucket, Key=key, Body=BODY)

    impostor = make_client(cfg.full[0], "definitely-not-the-secret")
    with s3_error("SignatureDoesNotMatch") as err:
        impostor.get_object(Bucket=bucket, Key=key)
    assert err["status"] == 403


def test_unknown_access_key_id_is_invalid_access_key_id(make_client, bucket, sandbox):
    stranger = make_client("OSGNOSUCHKEYID00000", "irrelevant-secret")

    with s3_error("InvalidAccessKeyId") as err:
        stranger.get_object(Bucket=bucket, Key=f"{sandbox}/anything.bin")
    assert err["status"] == 403


def test_unsigned_request_is_access_denied(raw, s3_full, bucket, sandbox):
    key = f"{sandbox}/unsigned.bin"
    s3_full.put_object(Bucket=bucket, Key=key, Body=BODY)

    response = raw("GET", key, sign=False)

    assert response.status == 403
    assert b"<Code>AccessDenied</Code>" in response.body


def test_signature_within_the_clock_window_still_works(raw, s3_full, bucket, sandbox):
    key = f"{sandbox}/skewed-a-little.bin"
    s3_full.put_object(Bucket=bucket, Key=key, Body=BODY)

    response = raw("GET", key, skew=datetime.timedelta(minutes=5))

    assert response.status == 200
    assert response.body == BODY


def test_signature_far_outside_the_clock_window_is_refused(raw, s3_full, bucket, sandbox, golden):
    key = f"{sandbox}/skewed-a-lot.bin"
    s3_full.put_object(Bucket=bucket, Key=key, Body=BODY)

    response = raw("GET", key, skew=datetime.timedelta(hours=25))

    assert response.status == 403
    # AWS answers RequestTimeTooSkewed once the signing time is more than 15
    # minutes out. Stores that skip the check answer SignatureDoesNotMatch, so
    # the code is recorded rather than hard-coded.
    code = response.body.split(b"<Code>")[1].split(b"</Code>")[0].decode()
    golden.check("clock_skew_code", code)
