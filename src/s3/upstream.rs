//! The outbound half of the data plane.
//!
//! Bodies stream in both directions: `reqwest::Body::wrap_stream` on the way up, the response body handed back as a stream on the way down.
//! A 5 GiB PUT crosses the gateway with constant memory.
use std::{pin::Pin, sync::OnceLock, time::Duration};

use bytes::Bytes;
use chrono::Utc;
use futures_util::Stream;
use sha2::{Digest, Sha256};

use crate::{
    models::pools,
    s3::{error::S3Error, sigv4},
};

/// A response body, handed back without being read into memory.
pub type BoxBody = Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;

/// How long a control request may take. Streamed uploads are deliberately exempt — see `Client::send`.
fn control_timeout() -> Duration {
    static MS: OnceLock<u64> = OnceLock::new();
    Duration::from_millis(*MS.get_or_init(|| {
        std::env::var("OSG_UPSTREAM_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30_000)
    }))
}

/// One `reqwest::Client` for the whole process.
///
/// It holds the connection pool and no credentials, so sharing it across pools is safe and building one per request would mean a TLS handshake per request.
/// The ring crypto provider is installed here rather than at boot because this is the only place in the process that opens a TLS connection, and `install_default` is idempotent by contract — it returns Err if something already installed one, which is not an error for us.
fn http() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
        reqwest::Client::builder()
            .build()
            .expect("reqwest client builds with the ring provider installed")
    })
}

/// A request body on the way up.
pub enum Body {
    Empty,
    Bytes(Vec<u8>),
    /// The streaming case: nothing is buffered, and the payload hash is `UNSIGNED-PAYLOAD`.
    Stream(BoxBody),
}

pub struct UpstreamRequest {
    pub method: String,
    /// The physical key, without the bucket and without a leading slash.
    pub key: String,
    pub query: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub body: Body,
}

impl UpstreamRequest {
    #[must_use]
    pub fn get(key: &str) -> Self {
        Self {
            method: "GET".to_string(),
            key: key.to_string(),
            query: Vec::new(),
            headers: Vec::new(),
            body: Body::Empty,
        }
    }

    #[must_use]
    pub fn head(key: &str) -> Self {
        Self {
            method: "HEAD".to_string(),
            ..Self::get(key)
        }
    }

    #[must_use]
    pub fn delete(key: &str) -> Self {
        Self {
            method: "DELETE".to_string(),
            ..Self::get(key)
        }
    }

    #[must_use]
    pub fn put(key: &str, body: Body) -> Self {
        Self {
            method: "PUT".to_string(),
            body,
            ..Self::get(key)
        }
    }

    #[must_use]
    pub fn with_query(mut self, query: Vec<(String, String)>) -> Self {
        self.query = query;
        self
    }

    #[must_use]
    pub fn with_headers(mut self, headers: Vec<(String, String)>) -> Self {
        self.headers = headers;
        self
    }
}

pub struct UpstreamResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: BoxBody,
}

/// Hand-written because the body is a stream, and because reading it to print it would consume the response.
impl std::fmt::Debug for UpstreamResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpstreamResponse")
            .field("status", &self.status)
            .field("headers", &self.headers)
            .field("body", &"<stream>")
            .finish()
    }
}

pub struct Client {
    endpoint: String,
    region: String,
    physical_bucket: String,
    access_id: String,
    secret: String,
}

impl Client {
    /// # Errors
    /// `InternalError` when the pool has no credentials — the backfill pool is in exactly that state, and sending an unsigned request would be worse than failing.
    pub fn new(pool: &pools::Model) -> Result<Self, S3Error> {
        if !pool.is_configured() {
            tracing::error!(
                pool = %pool.name,
                "pool has no upstream credentials; an admin must configure it"
            );
            return Err(S3Error::InternalError);
        }
        let secret = pool.decrypt_secret().map_err(|e| {
            tracing::error!(pool = %pool.name, error = %e, "pool secret could not be decrypted");
            S3Error::InternalError
        })?;

        let region = pool
            .region
            .clone()
            .unwrap_or_else(|| "us-east-1".to_string());
        let endpoint = pool.api_endpoint.clone().unwrap_or_else(|| {
            // Only AWS has a derivable endpoint; every other provider must configure one, and Client::new refuses below if it did not.
            format!("https://s3.{region}.amazonaws.com")
        });

        Ok(Self {
            endpoint: endpoint.trim_end_matches('/').to_string(),
            region,
            physical_bucket: pool.physical_bucket.clone(),
            access_id: pool.access_id.clone().unwrap_or_default(),
            secret,
        })
    }

    /// Path-style addressing: `{endpoint}/{physical_bucket}/{key}`.
    ///
    /// Virtual-host style would put the physical bucket in the hostname, which needs DNS per bucket and buys nothing here — there is only ever one physical bucket per pool.
    fn path_for(&self, key: &str) -> String {
        format!("/{}/{}", self.physical_bucket, key.trim_start_matches('/'))
    }

    fn host(&self) -> Result<String, S3Error> {
        let url = reqwest::Url::parse(&self.endpoint).map_err(|e| {
            tracing::error!(endpoint = %self.endpoint, error = %e, "pool api_endpoint is not a URL");
            S3Error::InternalError
        })?;
        let host = url.host_str().ok_or(S3Error::InternalError)?.to_string();
        Ok(match url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host,
        })
    }

    /// Signs and sends. The response body is handed back as a stream.
    ///
    /// # Errors
    /// `Upstream` for a 4xx the client can act on, `InternalError` for a 5xx or a transport failure — an upstream that is having a bad day is not the client's fault and its detail is not the client's business.
    pub async fn send(&self, req: UpstreamRequest) -> Result<UpstreamResponse, S3Error> {
        let now = Utc::now();
        let datetime = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date = now.format("%Y%m%d").to_string();

        let (payload_hash, streamed) = match &req.body {
            Body::Empty => (sigv4::EMPTY_PAYLOAD_SHA256.to_string(), false),
            Body::Bytes(b) => (hex::encode(Sha256::digest(b)), false),
            // A streamed body cannot be hashed before it is sent. S3, R2 and MinIO all accept UNSIGNED-PAYLOAD over HTTPS; a provider that does not would fail every write at once, which is at least easy to diagnose.
            Body::Stream(_) => (sigv4::UNSIGNED_PAYLOAD.to_string(), true),
        };

        let path = self.path_for(&req.key);
        let host = self.host()?;

        let mut headers: Vec<(String, String)> = vec![
            ("host".to_string(), host),
            ("x-amz-date".to_string(), datetime.clone()),
            ("x-amz-content-sha256".to_string(), payload_hash.clone()),
        ];
        for (k, v) in &req.headers {
            headers.push((k.to_ascii_lowercase(), v.clone()));
        }

        let mut signed_headers: Vec<String> = headers.iter().map(|(k, _)| k.clone()).collect();
        signed_headers.sort();
        signed_headers.dedup();

        let parts = sigv4::CanonicalParts {
            method: req.method.clone(),
            uri: path.clone(),
            query: req.query.clone(),
            headers: headers.clone(),
            signed_headers: signed_headers.clone(),
            payload_hash,
            // The gateway holds a decoded physical key here, so it encodes once.
            uri_already_encoded: false,
            // S3 never normalises, and the gateway only ever talks to S3-compatible stores.
            normalise_path: false,
        };
        let scope = format!("{date}/{}/s3/aws4_request", self.region);
        let sig = sigv4::signature(
            &sigv4::signing_key(&self.secret, &date, &self.region, "s3"),
            &sigv4::string_to_sign(&datetime, &scope, &sigv4::canonical_request(&parts)),
        );
        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={}, Signature={sig}",
            self.access_id,
            signed_headers.join(";")
        );

        // The URL is built from the canonical form that was just signed, so what goes on the wire cannot drift from what the signature covers.
        // Letting the HTTP client re-encode the query is exactly how a signer and a sender disagree.
        let canonical_q = sigv4::canonical_query(&req.query);
        let url = if canonical_q.is_empty() {
            format!("{}{}", self.endpoint, sigv4::canonical_uri(&path, false))
        } else {
            format!(
                "{}{}?{canonical_q}",
                self.endpoint,
                sigv4::canonical_uri(&path, false)
            )
        };
        let method = reqwest::Method::from_bytes(req.method.as_bytes())
            .map_err(|_| S3Error::InternalError)?;

        let mut builder = http().request(method, &url);
        for (k, v) in &headers {
            // host is set by reqwest from the URL; sending it twice makes hyper reject the request.
            if k != "host" {
                builder = builder.header(k, v);
            }
        }
        builder = builder.header("authorization", authorization);
        // The timeout covers control operations only. Applying one to a streamed upload would cap how long a legitimate 5 GiB PUT may take, which breaks the feature it is meant to protect.
        if !streamed {
            builder = builder.timeout(control_timeout());
        }
        builder = match req.body {
            Body::Empty => builder,
            Body::Bytes(b) => builder.body(b),
            Body::Stream(s) => builder.body(reqwest::Body::wrap_stream(s)),
        };

        let res = builder.send().await.map_err(|e| {
            tracing::error!(error = %e, "upstream request failed");
            S3Error::InternalError
        })?;

        let status = res.status().as_u16();
        let headers: Vec<(String, String)> = res
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_string(),
                    v.to_str().unwrap_or_default().to_string(),
                )
            })
            .collect();

        if status >= 400 {
            return Err(Self::classify(
                status,
                &res.text().await.unwrap_or_default(),
            ));
        }

        Ok(UpstreamResponse {
            status,
            headers,
            body: Box::pin(futures_util::TryStreamExt::map_err(
                res.bytes_stream(),
                |e| std::io::Error::other(e.to_string()),
            )),
        })
    }

    /// Turns an upstream failure into something the client can act on, without forwarding anything that names the physical layout.
    ///
    /// The `<Code>` survives because clients branch on it. The body does not: it carries the physical bucket and key.
    fn classify(status: u16, body: &str) -> S3Error {
        if status >= 500 {
            tracing::error!(status, body = %body, "upstream returned a server error");
            return S3Error::InternalError;
        }
        let code =
            between(body, "<Code>", "</Code>").unwrap_or_else(|| "InternalError".to_string());
        let message = between(body, "<Message>", "</Message>")
            .unwrap_or_else(|| "The upstream store refused the request.".to_string());
        tracing::warn!(status, code = %code, "upstream returned a client error");
        S3Error::Upstream {
            code,
            status,
            message,
        }
    }
}

/// Pulls one XML element's text out without parsing the document.
///
/// The error body is the only XML the gateway reads from upstream and it is a fixed two-field shape, so a full parser here would be a dependency for one call site.
fn between(haystack: &str, open: &str, close: &str) -> Option<String> {
    let start = haystack.find(open)? + open.len();
    let rest = &haystack[start..];
    let end = rest.find(close)?;
    Some(rest[..end].trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_keeps_the_code_and_drops_everything_else() {
        let err = Client::classify(
            404,
            r"<?xml version='1.0'?><Error><Code>NoSuchKey</Code>
              <Message>The specified key does not exist.</Message>
              <Key>osg-main/1111/media-cdn/gone.jpg</Key>
              <BucketName>osg-main</BucketName></Error>",
        );
        assert_eq!(err.code(), "NoSuchKey");
        let rendered = format!("{}{}", err.code(), err.message());
        assert!(!rendered.contains("osg-main"), "leaked: {rendered}");
    }

    #[test]
    fn classify_turns_a_5xx_into_an_internal_error_with_no_detail() {
        let err = Client::classify(503, "upstream is having a day");
        assert_eq!(err.code(), "InternalError");
        assert!(!err.message().contains("having a day"));
    }

    /// An upstream that answers 4xx with something other than S3 error XML must still produce a usable code rather than panicking or echoing the body.
    #[test]
    fn classify_survives_a_body_that_is_not_s3_error_xml() {
        let err = Client::classify(400, "<html>go away</html>");
        assert_eq!(err.code(), "InternalError");
        assert_eq!(err.status().as_u16(), 400);
        assert!(!err.message().contains("go away"));
    }
}
