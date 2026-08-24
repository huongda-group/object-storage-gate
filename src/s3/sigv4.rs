//! `SigV4`, running in both directions.
//!
//! `canonical_request` is shared: verification rebuilds the string the client signed, signing builds the string the gateway is about to sign.
//! One implementation means a bug shows up on both sides at once instead of hiding on one.
use hmac::{Hmac, KeyInit, Mac};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use sha2::{Digest, Sha256};

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
    /// The decoded path. `canonical_request` percent-encodes it exactly once, which is what both S3 and every other service expect.
    pub uri: String,
    /// `(name, value)` pairs, decoded and unsorted.
    pub query: Vec<(String, String)>,
    /// `(lowercase-name, value)` pairs, unsorted; duplicates allowed and kept in order.
    pub headers: Vec<(String, String)>,
    /// Lowercase names, sorted, as they appear in `SignedHeaders`.
    pub signed_headers: Vec<String>,
    pub payload_hash: String,
    /// S3 does not normalise the path; every other service does.
    ///
    /// The AWS vector suite uses a non-S3 service, so it exercises the normalising branch — which is why this is a flag and not a constant.
    /// Normalising an S3 key rewrites `a/../b` to `b` and `//x` to `/x`, and the signature then fails to match every real client while the vectors stay green.
    pub normalise_path: bool,
}

/// Collapses `.` and `..` segments and repeated slashes, per RFC 3986 section 6.2.2.3.
///
/// Only for non-S3 services. The suite's own `normalize-path.txt` spells out the S3 exception: an object literally named `my-object//example//photo.user` must keep both double slashes.
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
        canonical_uri(&p.uri, p.normalise_path),
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
        // A passing run over zero cases is the failure mode this guards. The suite ships 34 leaf cases and SKIP excludes one.
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
    /// The suite signs for service `service`, which normalises; S3 does not. If the S3 path took the normalising branch, a key named `a/../b` would be signed as `b` and `//x` as `/x` — every real client would get `SignatureDoesNotMatch` while every vector above stayed green.
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
}
