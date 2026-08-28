//! `SigV4`, running in both directions.
//!
//! `canonical_request` is shared: verification rebuilds the string the client signed, signing builds the string the gateway is about to sign.
//! One implementation means a bug shows up on both sides at once instead of hiding on one.
use axum::http::HeaderMap;
use chrono::{DateTime, NaiveDateTime, Utc};
use hmac::{Hmac, KeyInit, Mac};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use sha2::{Digest, Sha256};

use super::error::S3Error;

type HmacSha256 = Hmac<Sha256>;

/// Everything unreserved per RFC 3986 (`A-Za-z0-9-_.~`) stays literal; everything else is percent-encoded.
/// This is where a hand-rolled signer usually goes wrong: `+` for space, or encoding `-._~`, both produce a signature that never matches.
const QUERY_ENCODE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// The path set is the query set plus a literal `/`: segments are joined by slashes, not encoded into them.
const PATH_ENCODE: &AsciiSet = &QUERY_ENCODE.remove(b'/');

/// The sha256 of an empty body, which S3 clients send verbatim as `x-amz-content-sha256` on requests with no payload.
pub const EMPTY_PAYLOAD_SHA256: &str =
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// What a client sends instead of a hash when the body is streamed and cannot be hashed up front.
pub const UNSIGNED_PAYLOAD: &str = "UNSIGNED-PAYLOAD";

pub struct CanonicalParts {
    pub method: String,
    /// The request path.
    ///
    /// When `uri_already_encoded` is false this is the decoded path and `canonical_request` percent-encodes it exactly once.
    /// When true it is the path exactly as it arrived on the wire and is used verbatim.
    pub uri: String,
    /// `(name, value)` pairs, decoded and unsorted.
    pub query: Vec<(String, String)>,
    /// `(lowercase-name, value)` pairs, unsorted; duplicates allowed and kept in order.
    pub headers: Vec<(String, String)>,
    /// Lowercase names, sorted, as they appear in `SignedHeaders`.
    pub signed_headers: Vec<String>,
    pub payload_hash: String,
    /// Whether `uri` is already in its canonical, percent-encoded form.
    ///
    /// Verification sets this: the client computed its signature over the exact string it put in the request line, so decoding and re-encoding it can only introduce a mismatch — a client that wrote `%7E` where this code would write `~` would fail to verify for no reason.
    /// Signing an outbound request sets it false, because there the gateway holds a decoded key and must encode it once.
    pub uri_already_encoded: bool,
    /// S3 does not normalise the path; every other service does.
    ///
    /// The AWS vector suite uses a non-S3 service, so it exercises the normalising branch — which is why this is a flag and not a constant.
    /// Normalising an S3 key rewrites `a/../b` to `b` and `//x` to `/x`, and the signature then fails to match every real client while the vectors stay green.
    pub normalise_path: bool,
}

/// Collapses `.` and `..` segments and repeated slashes, per RFC 3986 section 6.2.2.3.
///
/// Only for non-S3 services.
/// The suite's own `normalize-path.txt` spells out the S3 exception: an object literally named `my-object//example//photo.user` must keep both double slashes.
fn normalise(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    let trailing = path.ends_with('/') || path.ends_with("/.") || path.ends_with("/..");
    let mut s = String::from("/");
    s.push_str(&out.join("/"));
    if trailing && !out.is_empty() {
        s.push('/');
    }
    s
}

/// Trims a header value and collapses every run of internal whitespace to one space.
///
/// The AWS prose exempts quoted strings; the vector `get-header-value-trim` does not — it expects `"a   b   c"` to become `"a b c"`.
/// The vector wins.
fn canonical_header_value(v: &str) -> String {
    v.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[must_use]
pub fn canonical_uri(path: &str, normalise_path: bool) -> String {
    canonical_uri_with(path, normalise_path, false)
}

#[must_use]
pub fn canonical_uri_with(path: &str, normalise_path: bool, already_encoded: bool) -> String {
    if already_encoded {
        return if path.is_empty() {
            "/".to_string()
        } else {
            path.to_string()
        };
    }
    let p = if normalise_path {
        normalise(path)
    } else if path.is_empty() {
        "/".to_string()
    } else {
        path.to_string()
    };
    utf8_percent_encode(&p, PATH_ENCODE).to_string()
}

#[must_use]
pub fn canonical_query(query: &[(String, String)]) -> String {
    let mut pairs: Vec<(String, String)> = query
        .iter()
        .map(|(k, v)| {
            (
                utf8_percent_encode(k, QUERY_ENCODE).to_string(),
                utf8_percent_encode(v, QUERY_ENCODE).to_string(),
            )
        })
        .collect();
    // Sorted on the encoded bytes, name first then value — not a case-insensitive sort.
    pairs.sort();
    pairs
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

#[must_use]
pub fn canonical_request(p: &CanonicalParts) -> String {
    // Duplicate names collapse into one line, values comma-joined in the order they arrived.
    let mut names: Vec<&str> = Vec::new();
    for (name, _) in &p.headers {
        if !names.contains(&name.as_str()) {
            names.push(name);
        }
    }
    names.sort_unstable();

    let mut headers = String::new();
    for name in names {
        let joined = p
            .headers
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, v)| canonical_header_value(v))
            .collect::<Vec<_>>()
            .join(",");
        headers.push_str(name);
        headers.push(':');
        headers.push_str(&joined);
        headers.push('\n');
    }

    let mut signed = p.signed_headers.clone();
    signed.sort();
    signed.dedup();

    format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        p.method,
        canonical_uri_with(&p.uri, p.normalise_path, p.uri_already_encoded),
        canonical_query(&p.query),
        headers,
        signed.join(";"),
        p.payload_hash
    )
}

#[must_use]
pub fn string_to_sign(datetime: &str, scope: &str, canonical: &str) -> String {
    format!(
        "AWS4-HMAC-SHA256\n{datetime}\n{scope}\n{}",
        hex::encode(Sha256::digest(canonical.as_bytes()))
    )
}

#[must_use]
pub fn signing_key(secret: &str, date: &str, region: &str, service: &str) -> [u8; 32] {
    let mut key = hmac(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    key = hmac(&key, region.as_bytes());
    key = hmac(&key, service.as_bytes());
    hmac(&key, b"aws4_request")
}

#[must_use]
pub fn signature(key: &[u8; 32], string_to_sign: &str) -> String {
    hex::encode(hmac(key, string_to_sign.as_bytes()))
}

fn hmac(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().into()
}

/// AWS's default tolerance, and what every S3 client assumes.
pub const CLOCK_SKEW_SECS: i64 = 900;

/// The longest life S3 gives a presigned URL, seven days.
///
/// A presigned URL is a bearer token, and the skew rule does not apply to one, so this ceiling is the only thing standing between a signing client and a link that never expires.
pub const MAX_PRESIGNED_EXPIRY_SECS: u64 = 604_800;

/// What the client presented, in either the header or the query form.
pub struct PresentedSignature {
    pub access_key_id: String,
    pub date: String,
    pub region: String,
    pub service: String,
    pub signed_headers: Vec<String>,
    pub signature: String,
    pub datetime: String,
    /// Only the presigned form carries this, in seconds from `datetime`.
    pub expires: Option<u64>,
}

/// Parses `20150830T123600Z`.
#[must_use]
pub fn parse_amz_datetime(s: &str) -> Option<DateTime<Utc>> {
    NaiveDateTime::parse_from_str(s, "%Y%m%dT%H%M%SZ")
        .ok()
        .map(|naive| naive.and_utc())
}

/// Splits `AKID/20150830/us-east-1/s3/aws4_request` into its four meaningful parts.
fn split_credential(cred: &str) -> Option<(String, String, String, String)> {
    let mut it = cred.split('/');
    let key = it.next()?.to_string();
    let date = it.next()?.to_string();
    let region = it.next()?.to_string();
    let service = it.next()?.to_string();
    if it.next()? != "aws4_request" || it.next().is_some() {
        return None;
    }
    Some((key, date, region, service))
}

/// Parses `Authorization: AWS4-HMAC-SHA256 Credential=…, SignedHeaders=…, Signature=…`.
///
/// # Errors
/// `AccessDenied` when the header is absent or not `SigV4` at all — a request that never tried to authenticate is not a signature failure, and telling the two apart is what lets a client know whether to retry with credentials.
pub fn parse_authorization(headers: &HeaderMap) -> Result<PresentedSignature, S3Error> {
    let raw = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or(S3Error::AccessDenied)?;
    let rest = raw
        .strip_prefix("AWS4-HMAC-SHA256 ")
        .ok_or(S3Error::AccessDenied)?;

    let mut credential = None;
    let mut signed_headers = None;
    let mut signature = None;
    for field in rest.split(',') {
        let field = field.trim();
        if let Some(v) = field.strip_prefix("Credential=") {
            credential = Some(v.to_string());
        } else if let Some(v) = field.strip_prefix("SignedHeaders=") {
            signed_headers = Some(v.to_string());
        } else if let Some(v) = field.strip_prefix("Signature=") {
            signature = Some(v.to_string());
        }
    }

    let (access_key_id, date, region, service) =
        split_credential(&credential.ok_or(S3Error::AccessDenied)?).ok_or(S3Error::AccessDenied)?;

    // x-amz-date is the signed timestamp; Date is the fallback S3 still accepts.
    let datetime = headers
        .get("x-amz-date")
        .and_then(|v| v.to_str().ok())
        .ok_or(S3Error::AccessDenied)?
        .to_string();

    Ok(PresentedSignature {
        access_key_id,
        date,
        region,
        service,
        signed_headers: signed_headers
            .ok_or(S3Error::AccessDenied)?
            .split(';')
            .map(str::to_string)
            .collect(),
        signature: signature.ok_or(S3Error::AccessDenied)?,
        datetime,
        expires: None,
    })
}

/// Parses the presigned form, where every field is an `X-Amz-*` query parameter.
///
/// # Errors
/// `AccessDenied` when the required params are absent or malformed.
pub fn parse_query(query: &[(String, String)]) -> Result<PresentedSignature, S3Error> {
    let get = |name: &str| -> Option<&str> {
        query
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    };

    if get("X-Amz-Algorithm") != Some("AWS4-HMAC-SHA256") {
        return Err(S3Error::AccessDenied);
    }

    let (access_key_id, date, region, service) =
        split_credential(get("X-Amz-Credential").ok_or(S3Error::AccessDenied)?)
            .ok_or(S3Error::AccessDenied)?;

    Ok(PresentedSignature {
        access_key_id,
        date,
        region,
        service,
        signed_headers: get("X-Amz-SignedHeaders")
            .ok_or(S3Error::AccessDenied)?
            .split(';')
            .map(str::to_string)
            .collect(),
        signature: get("X-Amz-Signature")
            .ok_or(S3Error::AccessDenied)?
            .to_string(),
        datetime: get("X-Amz-Date").ok_or(S3Error::AccessDenied)?.to_string(),
        expires: Some(
            get("X-Amz-Expires")
                .ok_or(S3Error::AccessDenied)?
                .parse()
                .map_err(|_| S3Error::AccessDenied)?,
        ),
    })
}

/// The canonical query string for a presigned request, which excludes `X-Amz-Signature`.
///
/// Including it makes every presigned URL fail, and the failure looks exactly like a wrong secret.
#[must_use]
pub fn canonical_query_for_presigned(query: &[(String, String)]) -> String {
    let kept: Vec<(String, String)> = query
        .iter()
        .filter(|(k, _)| !k.eq_ignore_ascii_case("X-Amz-Signature"))
        .cloned()
        .collect();
    canonical_query(&kept)
}

/// Whether a presigned signature is still inside the window its issuer chose.
///
/// This is the only expiry check a presigned request gets: `verify` skips the clock skew rule for one, so a window wider than `MAX_PRESIGNED_EXPIRY_SECS` is refused here rather than left to run forever.
///
/// # Errors
/// `AccessDenied` once `datetime + expires` has passed, or when `expires` exceeds seven days.
/// Not `RequestTimeTooSkewed`: an expired link is not a clock problem, and a client that retries after fixing its clock will fail again.
pub fn check_expiry(presented: &PresentedSignature, now: DateTime<Utc>) -> Result<(), S3Error> {
    let Some(expires) = presented.expires else {
        return Ok(());
    };
    if expires > MAX_PRESIGNED_EXPIRY_SECS {
        return Err(S3Error::AccessDenied);
    }
    let signed_at = parse_amz_datetime(&presented.datetime).ok_or(S3Error::AccessDenied)?;
    let age = (now - signed_at).num_seconds();
    if age < 0 || age > i64::try_from(expires).unwrap_or(i64::MAX) {
        return Err(S3Error::AccessDenied);
    }
    Ok(())
}

/// Recomputes the signature and compares it in constant time.
///
/// The clock check runs before the HMAC: rejecting a stale request costs one comparison, and verifying it first would spend a full HMAC on requests that are refused anyway.
///
/// A presigned signature is exempt from the skew rule: it carries its own window in `X-Amz-Expires` and `check_expiry` is what enforces it.
/// Applying skew to one caps every presigned URL at fifteen minutes no matter what expiry its issuer asked for.
///
/// # Errors
/// `RequestTimeTooSkewed` beyond ±15 minutes for a header signature; `SignatureDoesNotMatch` otherwise.
pub fn verify(
    presented: &PresentedSignature,
    secret: &str,
    parts: &CanonicalParts,
    now: DateTime<Utc>,
) -> Result<(), S3Error> {
    let signed_at = parse_amz_datetime(&presented.datetime).ok_or(S3Error::RequestTimeTooSkewed)?;
    if presented.expires.is_none() && (now - signed_at).num_seconds().abs() > CLOCK_SKEW_SECS {
        return Err(S3Error::RequestTimeTooSkewed);
    }

    let scope = format!(
        "{}/{}/{}/aws4_request",
        presented.date, presented.region, presented.service
    );
    let expected = signature(
        &signing_key(
            secret,
            &presented.date,
            &presented.region,
            &presented.service,
        ),
        &string_to_sign(&presented.datetime, &scope, &canonical_request(parts)),
    );

    // Constant-time: a byte-by-byte early return leaks the signature one byte at a time, and the signature is the one field the caller fully controls.
    if constant_time_eq(expected.as_bytes(), presented.signature.as_bytes()) {
        Ok(())
    } else {
        Err(S3Error::SignatureDoesNotMatch)
    }
}

/// Length mismatch returns early on purpose: a hex signature is always 64 characters, so the length carries no secret.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path};

    const VECTOR_SECRET: &str = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";
    const VECTOR_REGION: &str = "us-east-1";
    const VECTOR_SERVICE: &str = "service";
    const VECTOR_DATETIME: &str = "20150830T123600Z";
    const VECTOR_DATE: &str = "20150830";

    fn decode(s: &str) -> String {
        percent_encoding::percent_decode_str(s)
            .decode_utf8_lossy()
            .to_string()
    }

    /// Parses a `.req` file: request line, headers (with folded continuations), blank line, body.
    fn parse_req(raw: &str) -> CanonicalParts {
        let mut lines = raw.split('\n');
        let request_line = lines.next().expect("request line").trim_end_matches('\r');
        // The target may itself contain spaces (`get-space`), so it is everything between the first space and the trailing HTTP version, not the second whitespace-separated field.
        let (method, rest) = request_line.split_once(' ').expect("method and target");
        let method = method.to_string();
        let target = rest
            .rsplit_once(' ')
            .map_or_else(|| rest.to_string(), |(t, _version)| t.to_string());

        let (path_enc, query_enc) = target.split_once('?').unwrap_or((target.as_str(), ""));
        let query: Vec<(String, String)> = if query_enc.is_empty() {
            Vec::new()
        } else {
            query_enc
                .split('&')
                .map(|kv| {
                    let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
                    (decode(k), decode(v))
                })
                .collect()
        };

        let mut headers: Vec<(String, String)> = Vec::new();
        let mut body = String::new();
        let mut in_body = false;
        for line in lines {
            let line = line.trim_end_matches('\r');
            if in_body {
                if !body.is_empty() {
                    body.push('\n');
                }
                body.push_str(line);
                continue;
            }
            if line.is_empty() {
                in_body = true;
                continue;
            }
            if line.starts_with(' ') || line.starts_with('\t') {
                // A folded continuation belongs to the header above it.
                if let Some(last) = headers.last_mut() {
                    last.1.push(' ');
                    last.1.push_str(line.trim());
                }
                continue;
            }
            let (name, value) = line.split_once(':').expect("header line");
            headers.push((name.to_ascii_lowercase(), value.to_string()));
        }

        let mut signed_headers: Vec<String> = headers.iter().map(|(n, _)| n.clone()).collect();
        signed_headers.sort();
        signed_headers.dedup();

        CanonicalParts {
            method,
            uri: decode(path_enc),
            query,
            headers,
            signed_headers,
            payload_hash: hex::encode(Sha256::digest(body.as_bytes())),
            uri_already_encoded: false,
            // The suite's service is `service`, not s3, so it exercises the normalising branch.
            normalise_path: true,
        }
    }

    /// The one case whose `.req` does not contain everything its `.creq` signs.
    ///
    /// Its `.req` carries only Host and X-Amz-Date, while its `.creq` also signs `x-amz-security-token`: the suite expects a *signer* holding STS credentials to add that header itself.
    /// A verifier — which is what this runner models — can only see headers that arrived.
    /// The gateway never uses STS credentials, so nothing here would add it either.
    /// `the_skipped_case_is_skipped_for_the_reason_claimed` re-checks that this is still true, so a corrected vector turns into a failing test rather than a silent exclusion.
    const SKIP: &[&str] = &["get-vanilla-with-session-token"];

    fn run_case(dir: &Path) -> bool {
        let name = dir.file_name().unwrap().to_str().unwrap();
        let req_path = dir.join(format!("{name}.req"));
        if !req_path.exists() {
            return false;
        }
        if SKIP.contains(&name) {
            return false;
        }
        let req = fs::read_to_string(&req_path).unwrap();
        let want_creq = fs::read_to_string(dir.join(format!("{name}.creq"))).unwrap();
        let want_sts = fs::read_to_string(dir.join(format!("{name}.sts"))).unwrap();
        let want_authz = fs::read_to_string(dir.join(format!("{name}.authz"))).unwrap();

        let parts = parse_req(&req);
        let creq = canonical_request(&parts);
        assert_eq!(creq, want_creq.trim_end(), "canonical request for {name}");

        let scope = format!("{VECTOR_DATE}/{VECTOR_REGION}/{VECTOR_SERVICE}/aws4_request");
        let sts = string_to_sign(VECTOR_DATETIME, &scope, &creq);
        assert_eq!(sts, want_sts.trim_end(), "string to sign for {name}");

        // The full chain: the Authorization header the suite publishes must come out byte for byte.
        let sig = signature(
            &signing_key(VECTOR_SECRET, VECTOR_DATE, VECTOR_REGION, VECTOR_SERVICE),
            &sts,
        );
        let want_sig = want_authz
            .trim_end()
            .rsplit_once("Signature=")
            .expect("authz carries a signature")
            .1
            .to_string();
        assert_eq!(sig, want_sig, "signature for {name}");
        true
    }

    fn walk(dir: &Path, ran: &mut usize) {
        if run_case(dir) {
            *ran += 1;
            return;
        }
        // A leaf case that was skipped has no subdirectories, so this recursion is a no-op for it.
        for entry in fs::read_dir(dir).unwrap() {
            let child = entry.unwrap().path();
            if child.is_dir() {
                walk(&child, ran);
            }
        }
    }

    #[test]
    fn matches_the_aws_test_suite() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/s3_vectors");
        let mut ran = 0;
        walk(&root, &mut ran);
        // A passing run over zero cases is the failure mode this guards.
        // The suite ships 34 leaf cases and SKIP excludes one.
        assert_eq!(ran, 33, "expected 33 vector cases to run, ran {ran}");
    }

    /// The signing key has no separately published expected value for these inputs, so it is checked the only honest way: through the signature the suite publishes.
    ///
    /// `run_case` asserts this for every vector; this spells the chain out once so a reader can see where the key enters.
    #[test]
    fn the_full_chain_reproduces_the_published_authorization_header() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/s3_vectors/get-vanilla");
        let parts = parse_req(&fs::read_to_string(root.join("get-vanilla.req")).unwrap());
        let scope = format!("{VECTOR_DATE}/{VECTOR_REGION}/{VECTOR_SERVICE}/aws4_request");

        let sig = signature(
            &signing_key(VECTOR_SECRET, VECTOR_DATE, VECTOR_REGION, VECTOR_SERVICE),
            &string_to_sign(VECTOR_DATETIME, &scope, &canonical_request(&parts)),
        );

        let authz = fs::read_to_string(root.join("get-vanilla.authz")).unwrap();
        assert!(
            authz.trim_end().ends_with(&format!("Signature={sig}")),
            "computed {sig}, suite published {authz}"
        );
    }

    /// Guards the one exclusion in `SKIP`, so it can never quietly become a way to hide a real failure.
    #[test]
    fn the_skipped_case_is_skipped_for_the_reason_claimed() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/s3_vectors");
        for name in SKIP {
            let dir = root.join(name);
            let req = fs::read_to_string(dir.join(format!("{name}.req"))).unwrap();
            let creq = fs::read_to_string(dir.join(format!("{name}.creq"))).unwrap();
            assert!(
                !req.to_ascii_lowercase().contains("x-amz-security-token"),
                "{name}.req now carries the token; remove it from SKIP and let the case run"
            );
            assert!(
                creq.contains("x-amz-security-token"),
                "{name}.creq no longer signs the token; the exclusion is obsolete"
            );
        }
    }

    /// Not covered by the AWS suite, and the most dangerous default in this module.
    ///
    /// The suite signs for service `service`, which normalises; S3 does not.
    /// If the S3 path took the normalising branch, a key named `a/../b` would be signed as `b` and `//x` as `/x` — every real client would get `SignatureDoesNotMatch` while every vector above stayed green.
    #[test]
    fn the_s3_path_is_not_normalised() {
        assert_eq!(canonical_uri("/bkt/a/../b", false), "/bkt/a/../b");
        assert_eq!(
            canonical_uri("/bkt/my-object//example//photo.user", false),
            "/bkt/my-object//example//photo.user"
        );

        // And the normalising branch really does differ, so the assertion above is not vacuous.
        assert_eq!(canonical_uri("/bkt/a/../b", true), "/bkt/b");
        assert_eq!(
            canonical_uri("/bkt/my-object//example//photo.user", true),
            "/bkt/my-object/example/photo.user"
        );
    }

    /// A path is encoded once, not twice: a key arriving as `a b.jpg` signs as `a%20b.jpg`, never `a%2520b.jpg`.
    #[test]
    fn a_path_is_encoded_exactly_once() {
        assert_eq!(canonical_uri("/bkt/a b.jpg", false), "/bkt/a%20b.jpg");
        assert_eq!(canonical_uri("/bkt/$delete", false), "/bkt/%24delete");
        assert_eq!(canonical_uri("/bkt/-._~", false), "/bkt/-._~");
    }

    /// A slash inside a query value is encoded; a slash in the path is not.
    #[test]
    fn query_encoding_differs_from_path_encoding() {
        let q = vec![("prefix".to_string(), "a/b".to_string())];
        assert_eq!(canonical_query(&q), "prefix=a%2Fb");
        assert_eq!(canonical_uri("/a/b", false), "/a/b");
    }

    // ---- verify ----

    /// Builds a real signed GET so the verify tests exercise the same code path a client would.
    fn a_signed_get(secret: &str, when: &str) -> (PresentedSignature, CanonicalParts) {
        let parts = CanonicalParts {
            method: "GET".to_string(),
            uri: "/osg-main/1111/media-cdn/a.jpg".to_string(),
            query: Vec::new(),
            headers: vec![
                ("host".to_string(), "s3.example.com".to_string()),
                ("x-amz-date".to_string(), when.to_string()),
            ],
            signed_headers: vec!["host".to_string(), "x-amz-date".to_string()],
            payload_hash: EMPTY_PAYLOAD_SHA256.to_string(),
            uri_already_encoded: false,
            normalise_path: false,
        };
        let date = &when[..8];
        let scope = format!("{date}/{VECTOR_REGION}/s3/aws4_request");
        let sig = signature(
            &signing_key(secret, date, VECTOR_REGION, "s3"),
            &string_to_sign(when, &scope, &canonical_request(&parts)),
        );
        let presented = PresentedSignature {
            access_key_id: "OSGTESTKEYID".to_string(),
            date: date.to_string(),
            region: VECTOR_REGION.to_string(),
            service: "s3".to_string(),
            signed_headers: vec!["host".to_string(), "x-amz-date".to_string()],
            signature: sig,
            datetime: when.to_string(),
            expires: None,
        };
        (presented, parts)
    }

    fn presigned_at(when: &str, expires: u64) -> PresentedSignature {
        PresentedSignature {
            access_key_id: "OSGTESTKEYID".to_string(),
            date: when[..8].to_string(),
            region: VECTOR_REGION.to_string(),
            service: "s3".to_string(),
            signed_headers: vec!["host".to_string()],
            signature: "0".repeat(64),
            datetime: when.to_string(),
            expires: Some(expires),
        }
    }

    #[test]
    fn a_correctly_signed_request_verifies() {
        let (sig, parts) = a_signed_get(VECTOR_SECRET, "20150830T123600Z");
        let now = parse_amz_datetime("20150830T123600Z").unwrap();
        assert!(verify(&sig, VECTOR_SECRET, &parts, now).is_ok());
    }

    #[test]
    fn a_wrong_secret_does_not_verify() {
        let (sig, parts) = a_signed_get(VECTOR_SECRET, "20150830T123600Z");
        let now = parse_amz_datetime("20150830T123600Z").unwrap();
        assert!(matches!(
            verify(&sig, "not-the-secret", &parts, now),
            Err(S3Error::SignatureDoesNotMatch)
        ));
    }

    /// A tampered request must fail even though the signature itself is untouched — that is the whole point of signing the canonical request rather than just the timestamp.
    #[test]
    fn a_tampered_key_does_not_verify() {
        let (sig, mut parts) = a_signed_get(VECTOR_SECRET, "20150830T123600Z");
        parts.uri = "/osg-main/2222/media-cdn/a.jpg".to_string();
        let now = parse_amz_datetime("20150830T123600Z").unwrap();
        assert!(matches!(
            verify(&sig, VECTOR_SECRET, &parts, now),
            Err(S3Error::SignatureDoesNotMatch)
        ));
    }

    /// Spec §5.1 step 4.
    #[test]
    fn a_signature_inside_the_window_verifies() {
        let (sig, parts) = a_signed_get(VECTOR_SECRET, "20150830T123600Z");
        let now = parse_amz_datetime("20150830T124000Z").unwrap();
        assert!(verify(&sig, VECTOR_SECRET, &parts, now).is_ok());
    }

    #[test]
    fn a_signature_outside_the_window_is_refused() {
        let (sig, parts) = a_signed_get(VECTOR_SECRET, "20150830T123600Z");
        let now = parse_amz_datetime("20150830T140000Z").unwrap();
        assert!(matches!(
            verify(&sig, VECTOR_SECRET, &parts, now),
            Err(S3Error::RequestTimeTooSkewed)
        ));
    }

    /// Skew is symmetric: a client whose clock runs fast is as suspect as one running slow.
    #[test]
    fn a_signature_from_the_future_is_refused() {
        let (sig, parts) = a_signed_get(VECTOR_SECRET, "20150830T140000Z");
        let now = parse_amz_datetime("20150830T123600Z").unwrap();
        assert!(matches!(
            verify(&sig, VECTOR_SECRET, &parts, now),
            Err(S3Error::RequestTimeTooSkewed)
        ));
    }

    /// The boundary itself, in both directions: 900s is inside, 901s is not.
    #[test]
    fn the_skew_boundary_is_exactly_fifteen_minutes() {
        let (sig, parts) = a_signed_get(VECTOR_SECRET, "20150830T120000Z");
        let inside = parse_amz_datetime("20150830T121500Z").unwrap();
        let outside = parse_amz_datetime("20150830T121501Z").unwrap();
        assert!(verify(&sig, VECTOR_SECRET, &parts, inside).is_ok());
        assert!(verify(&sig, VECTOR_SECRET, &parts, outside).is_err());
    }

    /// Skew does not apply to a presigned signature, which carries its own window.
    ///
    /// Reaching `SignatureDoesNotMatch` is the point: the placeholder signature is wrong, so getting that far proves the clock check let the request through.
    /// Applying skew here capped every presigned URL at fifteen minutes whatever `X-Amz-Expires` said.
    #[test]
    fn a_presigned_signature_is_exempt_from_the_skew_rule() {
        let (_, parts) = a_signed_get(VECTOR_SECRET, "20150830T120000Z");
        let sig = presigned_at("20150830T120000Z", 3600);
        let now = parse_amz_datetime("20150830T124500Z").unwrap();
        assert!(matches!(
            verify(&sig, VECTOR_SECRET, &parts, now),
            Err(S3Error::SignatureDoesNotMatch)
        ));
    }

    #[test]
    fn a_missing_authorization_header_is_access_denied() {
        let headers = HeaderMap::new();
        assert!(matches!(
            parse_authorization(&headers),
            Err(S3Error::AccessDenied)
        ));
    }

    #[test]
    fn a_malformed_authorization_header_is_access_denied() {
        for bad in [
            "Bearer abc",
            "AWS4-HMAC-SHA256 nonsense",
            "AWS4-HMAC-SHA256 Credential=x, SignedHeaders=host",
            "AWS4-HMAC-SHA256 Credential=x/y/z, SignedHeaders=host, Signature=s",
            "AWS4-HMAC-SHA256 Credential=a/b/c/d/not_aws4_request, SignedHeaders=host, Signature=s",
            "AWS4-HMAC-SHA256 Credential=a/b/c/d/aws4_request/extra, SignedHeaders=host, Signature=s",
        ] {
            let mut headers = HeaderMap::new();
            headers.insert("authorization", bad.parse().unwrap());
            headers.insert("x-amz-date", "20150830T123600Z".parse().unwrap());
            assert!(
                parse_authorization(&headers).is_err(),
                "should reject {bad:?}"
            );
        }
    }

    /// A well-formed Authorization header with no x-amz-date has nothing to check skew against.
    #[test]
    fn an_authorization_header_without_a_date_is_access_denied() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            "AWS4-HMAC-SHA256 Credential=AKID/20150830/us-east-1/s3/aws4_request, \
             SignedHeaders=host, Signature=abc"
                .parse()
                .unwrap(),
        );
        assert!(matches!(
            parse_authorization(&headers),
            Err(S3Error::AccessDenied)
        ));
    }

    #[test]
    fn a_well_formed_authorization_header_parses() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/s3/aws4_request, \
             SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=deadbeef"
                .parse()
                .unwrap(),
        );
        headers.insert("x-amz-date", "20150830T123600Z".parse().unwrap());

        let sig = parse_authorization(&headers).unwrap();
        assert_eq!(sig.access_key_id, "AKIDEXAMPLE");
        assert_eq!(sig.date, "20150830");
        assert_eq!(sig.region, "us-east-1");
        assert_eq!(sig.service, "s3");
        assert_eq!(sig.signature, "deadbeef");
        assert_eq!(sig.signed_headers.len(), 3);
        assert_eq!(sig.expires, None);
    }

    /// Presigned form.
    /// Spec §5.3.
    #[test]
    fn a_presigned_query_parses_and_carries_its_expiry() {
        let q = vec![
            ("X-Amz-Algorithm".into(), "AWS4-HMAC-SHA256".into()),
            (
                "X-Amz-Credential".into(),
                "AKIDEXAMPLE/20150830/us-east-1/s3/aws4_request".into(),
            ),
            ("X-Amz-Date".into(), "20150830T123600Z".into()),
            ("X-Amz-Expires".into(), "3600".into()),
            ("X-Amz-SignedHeaders".into(), "host".into()),
            ("X-Amz-Signature".into(), "abc".into()),
        ];
        let sig = parse_query(&q).unwrap();
        assert_eq!(sig.access_key_id, "AKIDEXAMPLE");
        assert_eq!(sig.expires, Some(3600));
    }

    #[test]
    fn a_query_without_the_algorithm_is_access_denied() {
        let q = vec![("X-Amz-Signature".into(), "abc".into())];
        assert!(matches!(parse_query(&q), Err(S3Error::AccessDenied)));
    }

    #[test]
    fn a_presigned_signature_inside_its_window_is_accepted() {
        let sig = presigned_at("20150830T123600Z", 3600);
        let now = parse_amz_datetime("20150830T124000Z").unwrap();
        assert!(check_expiry(&sig, now).is_ok());
    }

    #[test]
    fn an_expired_presigned_signature_is_access_denied() {
        let sig = presigned_at("20150830T123600Z", 60);
        let now = parse_amz_datetime("20150830T124000Z").unwrap();
        assert!(matches!(
            check_expiry(&sig, now),
            Err(S3Error::AccessDenied)
        ));
    }

    /// Seven days is the ceiling S3 puts on a presigned URL, and with skew exempted it is the only thing keeping a signing client from minting a link that never expires.
    #[test]
    fn a_presigned_window_beyond_seven_days_is_access_denied() {
        let sig = presigned_at("20150830T123600Z", MAX_PRESIGNED_EXPIRY_SECS + 1);
        let now = parse_amz_datetime("20150830T124000Z").unwrap();
        assert!(matches!(
            check_expiry(&sig, now),
            Err(S3Error::AccessDenied)
        ));

        let at_limit = presigned_at("20150830T123600Z", MAX_PRESIGNED_EXPIRY_SECS);
        assert!(check_expiry(&at_limit, now).is_ok());
    }

    /// A link whose start time has not arrived yet is refused too; otherwise a client with a fast clock could mint one good for twice its stated life.
    #[test]
    fn a_presigned_signature_from_the_future_is_access_denied() {
        let sig = presigned_at("20150830T140000Z", 60);
        let now = parse_amz_datetime("20150830T123600Z").unwrap();
        assert!(matches!(
            check_expiry(&sig, now),
            Err(S3Error::AccessDenied)
        ));
    }

    /// The canonical query string for a presigned request excludes X-Amz-Signature — including it makes every presigned URL fail, and the failure looks like a wrong secret.
    #[test]
    fn the_signature_param_is_excluded_from_the_canonical_query() {
        let q = vec![
            ("X-Amz-Signature".into(), "abc".into()),
            ("X-Amz-Date".into(), "20150830T123600Z".into()),
        ];
        let canonical = canonical_query_for_presigned(&q);
        assert!(!canonical.contains("X-Amz-Signature"));
        assert!(canonical.contains("X-Amz-Date"));
    }

    #[test]
    fn constant_time_eq_still_compares_correctly() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(constant_time_eq(b"", b""));
    }
}
