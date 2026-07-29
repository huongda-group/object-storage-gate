"""What the SDK models away.

boto3 parses XML into dicts and raises typed errors, which hides the bytes an
S3 client actually reads. rclone, Cyberduck and older SDKs are less forgiving,
so these tests go over the wire directly.
"""

from __future__ import annotations

BODY = b"wire-level payload"


def test_error_response_is_s3_shaped_xml(raw, bucket, sandbox):
    missing = f"{sandbox}/absent.bin"

    response = raw("GET", missing)

    assert response.status == 404
    body = response.text
    assert body.startswith("<?xml")
    assert "<Error>" in body
    assert "<Code>NoSuchKey</Code>" in body
    assert "<Message>" in body
    # Clients and support tickets both key off these two.
    assert f"<Key>{missing}</Key>" in body
    assert "<RequestId>" in body


def test_responses_carry_a_request_id_header(raw, s3_full, bucket, sandbox):
    key = f"{sandbox}/traced.bin"
    s3_full.put_object(Bucket=bucket, Key=key, Body=BODY)

    response = raw("GET", key)

    assert response.status == 200
    lowered = {name.lower() for name in response.headers}
    assert "x-amz-request-id" in lowered


def test_verb_status_codes_are_exact(raw, bucket, sandbox):
    key = f"{sandbox}/statuses.bin"

    assert raw("PUT", key, body=BODY).status == 200
    assert raw("GET", key).status == 200
    assert raw("HEAD", key).status == 200
    # DELETE is 204 with no body, not 200.
    deleted = raw("DELETE", key)
    assert deleted.status == 204
    assert deleted.body == b""


def test_head_reports_length_and_etag_without_a_body(raw, s3_full, bucket, sandbox):
    key = f"{sandbox}/headers.bin"
    etag = s3_full.put_object(Bucket=bucket, Key=key, Body=BODY)["ETag"]

    response = raw("HEAD", key)
    headers = {name.lower(): value for name, value in response.headers.items()}

    assert response.body == b""
    assert headers["content-length"] == str(len(BODY))
    # The ETag stays quoted on the wire; stripping the quotes breaks conditional
    # requests that echo it back.
    assert headers["etag"] == etag
    assert etag.startswith('"') and etag.endswith('"')


def test_list_objects_v2_xml_is_s3_shaped(raw, s3_full, bucket, sandbox):
    key = f"{sandbox}/listed.bin"
    s3_full.put_object(Bucket=bucket, Key=key, Body=BODY)

    response = raw("GET", "", query={"list-type": "2", "prefix": f"{sandbox}/"})

    assert response.status == 200
    body = response.text
    assert "<ListBucketResult" in body
    assert 'xmlns="http://s3.amazonaws.com/doc/2006-03-01/"' in body
    assert "<KeyCount>1</KeyCount>" in body
    assert f"<Key>{key}</Key>" in body
    assert "<IsTruncated>false</IsTruncated>" in body
