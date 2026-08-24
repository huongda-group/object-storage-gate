//! Signs a request the way a real S3 client does, so tests exercise the verifier rather than a shortcut around it.
//!
//! Deliberately not built on `sigv4::canonical_request`: a test signer that shares the implementation under test agrees with it even when both are wrong.
//! This one follows the AWS documentation directly — and it can still be wrong, which is why G3 ends with a check against aws-cli.
use chrono::{DateTime, Utc};
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

const UNRESERVED: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.~";

fn enc(s: &str, keep_slash: bool) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        if UNRESERVED.contains(b) || (keep_slash && *b == b'/') {
            out.push(*b as char);
        } else {
            use std::fmt::Write as _;
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

fn mac(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut m = HmacSha256::new_from_slice(key).unwrap();
    m.update(data);
    m.finalize().into_bytes().into()
}

#[derive(Clone)]
pub struct TestSigner {
    pub access_key_id: String,
    pub secret: String,
    pub region: String,
    pub host: String,
}

impl TestSigner {
    #[must_use]
    pub fn new(access_key_id: &str, secret: &str) -> Self {
        Self {
            access_key_id: access_key_id.to_string(),
            secret: secret.to_string(),
            region: "us-east-1".to_string(),
            host: "gateway.test".to_string(),
        }
    }

    #[must_use]
    pub fn with_id(&self, id: &str) -> Self {
        Self {
            access_key_id: id.to_string(),
            ..self.clone()
        }
    }

    #[must_use]
    pub fn with_secret(&self, secret: &str) -> Self {
        Self {
            secret: secret.to_string(),
            ..self.clone()
        }
    }

    /// The headers a signed request carries.
    ///
    /// `path` is the raw, already-encoded path as it will appear in the request line.
    #[must_use]
    pub fn sign(
        &self,
        method: &str,
        path: &str,
        query: &[(&str, &str)],
        body: &[u8],
        extra: &[(&str, &str)],
    ) -> Vec<(String, String)> {
        self.sign_at(Utc::now(), method, path, query, body, extra)
    }

    /// Same, at a chosen instant, for the clock-skew tests.
    #[must_use]
    pub fn sign_at(
        &self,
        at: DateTime<Utc>,
        method: &str,
        path: &str,
        query: &[(&str, &str)],
        body: &[u8],
        extra: &[(&str, &str)],
    ) -> Vec<(String, String)> {
        let datetime = at.format("%Y%m%dT%H%M%SZ").to_string();
        let date = at.format("%Y%m%d").to_string();
        let payload_hash = hex::encode(Sha256::digest(body));

        let mut headers: Vec<(String, String)> = vec![
            ("host".to_string(), self.host.clone()),
            ("x-amz-content-sha256".to_string(), payload_hash.clone()),
            ("x-amz-date".to_string(), datetime.clone()),
        ];
        for (k, v) in extra {
            headers.push((k.to_ascii_lowercase(), (*v).to_string()));
        }
        headers.sort();

        let mut canonical_headers = String::new();
        for (k, v) in &headers {
            use std::fmt::Write as _;
            let _ = writeln!(canonical_headers, "{k}:{}", v.trim());
        }
        let signed_headers = headers
            .iter()
            .map(|(k, _)| k.as_str())
            .collect::<Vec<_>>()
            .join(";");

        let mut q: Vec<(String, String)> = query
            .iter()
            .map(|(k, v)| (enc(k, false), enc(v, false)))
            .collect();
        q.sort();
        let canonical_query = q
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");

        let canonical = format!(
            "{method}\n{path}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
        );
        let scope = format!("{date}/{}/s3/aws4_request", self.region);
        let sts = format!(
            "AWS4-HMAC-SHA256\n{datetime}\n{scope}\n{}",
            hex::encode(Sha256::digest(canonical.as_bytes()))
        );

        let mut k = mac(format!("AWS4{}", self.secret).as_bytes(), date.as_bytes());
        k = mac(&k, self.region.as_bytes());
        k = mac(&k, b"s3");
        k = mac(&k, b"aws4_request");
        let signature = hex::encode(mac(&k, sts.as_bytes()));

        let mut out = headers;
        out.push((
            "authorization".to_string(),
            format!(
                "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
                self.access_key_id
            ),
        ));
        out
    }

    /// A signature that is well-formed but wrong, for the `SignatureDoesNotMatch` path.
    #[must_use]
    pub fn sign_tampered(&self, method: &str, path: &str, body: &[u8]) -> Vec<(String, String)> {
        let mut headers = self.sign(method, path, &[], body, &[]);
        for h in &mut headers {
            if h.0 == "authorization" {
                let (head, _) = h.1.rsplit_once("Signature=").unwrap();
                h.1 = format!("{head}Signature={}", "0".repeat(64));
            }
        }
        headers
    }

    /// A presigned query string, without the leading `?`.
    ///
    /// Unused until G6 wires presigned URLs; kept here because the signer is the piece those tests will need and writing it beside the header form is where the two shapes can be compared.
    #[allow(dead_code)]
    #[must_use]
    pub fn presign(&self, method: &str, path: &str, expires_secs: u64) -> String {
        let at = Utc::now();
        self.presign_at(at, method, path, expires_secs)
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn presign_at(
        &self,
        at: DateTime<Utc>,
        method: &str,
        path: &str,
        expires_secs: u64,
    ) -> String {
        let datetime = at.format("%Y%m%dT%H%M%SZ").to_string();
        let date = at.format("%Y%m%d").to_string();
        let scope = format!("{date}/{}/s3/aws4_request", self.region);

        let mut q: Vec<(String, String)> = vec![
            ("X-Amz-Algorithm".into(), "AWS4-HMAC-SHA256".into()),
            (
                "X-Amz-Credential".into(),
                format!("{}/{scope}", self.access_key_id),
            ),
            ("X-Amz-Date".into(), datetime.clone()),
            ("X-Amz-Expires".into(), expires_secs.to_string()),
            ("X-Amz-SignedHeaders".into(), "host".into()),
        ];
        q = q
            .iter()
            .map(|(k, v)| (enc(k, false), enc(v, false)))
            .collect();
        q.sort();
        let canonical_query = q
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");

        let canonical = format!(
            "{method}\n{path}\n{canonical_query}\nhost:{}\n\nhost\nUNSIGNED-PAYLOAD",
            self.host
        );
        let sts = format!(
            "AWS4-HMAC-SHA256\n{datetime}\n{scope}\n{}",
            hex::encode(Sha256::digest(canonical.as_bytes()))
        );
        let mut k = mac(format!("AWS4{}", self.secret).as_bytes(), date.as_bytes());
        k = mac(&k, self.region.as_bytes());
        k = mac(&k, b"s3");
        k = mac(&k, b"aws4_request");
        let signature = hex::encode(mac(&k, sts.as_bytes()));

        format!("{canonical_query}&X-Amz-Signature={signature}")
    }
}

/// Percent-encodes an object key the way a client puts it in the request line.
#[must_use]
pub fn encode_path(path: &str) -> String {
    enc(path, true)
}
