//! `GetObject` and `HeadObject`.
//!
//! Body and headers come from upstream; the `objects` row is metadata for listing and quota, not the source of truth for content.
use axum::{
    body::Body,
    http::{request::Parts, HeaderName, HeaderValue, StatusCode},
    response::Response,
};
use loco_rs::prelude::*;

use crate::{
    controllers::s3::fail,
    models::access_keys,
    s3::{
        error::S3Error,
        request::S3Request,
        upstream::{self, UpstreamRequest, UpstreamResponse},
    },
};

/// Headers forwarded from the client to upstream on a read.
///
/// A whitelist, not a blacklist: forwarding an unknown client header can change upstream behaviour in ways the gateway did not intend, and the failure would look like a storage bug.
const FORWARD_TO_UPSTREAM: &[&str] = &[
    "range",
    "if-match",
    "if-none-match",
    "if-modified-since",
    "if-unmodified-since",
];

/// Headers forwarded from upstream back to the client.
///
/// Also a whitelist: some providers answer with debug headers that name the physical bucket, and forwarding upstream headers blind is how the layout leaks.
/// `x-amz-meta-*` is matched by prefix rather than listed, because it is the client's own metadata coming back.
const FORWARD_TO_CLIENT: &[&str] = &[
    "content-type",
    "content-length",
    "content-range",
    "content-encoding",
    "content-disposition",
    "cache-control",
    "etag",
    "last-modified",
    "accept-ranges",
    "expires",
];

fn forwarded_request_headers(parts: &Parts) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for name in FORWARD_TO_UPSTREAM {
        if let Some(v) = parts.headers.get(*name) {
            if let Ok(s) = v.to_str() {
                out.push(((*name).to_string(), s.to_string()));
            }
        }
    }
    out
}

fn apply_response_headers(builder: &mut axum::http::response::Builder, res: &UpstreamResponse) {
    let Some(headers) = builder.headers_mut() else {
        return;
    };
    for (name, value) in &res.headers {
        let lower = name.to_ascii_lowercase();
        if !FORWARD_TO_CLIENT.contains(&lower.as_str()) && !lower.starts_with("x-amz-meta-") {
            continue;
        }
        if let (Ok(n), Ok(v)) = (
            HeaderName::from_bytes(lower.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            headers.insert(n, v);
        }
    }
}

/// Runs the read chain and returns the upstream response, or the error to render.
async fn fetch(ctx: &AppContext, parts: &Parts, method: &str) -> Result<UpstreamResponse, S3Error> {
    let req = S3Request::resolve(ctx, parts, access_keys::ACTION_READ).await?;
    let client = upstream::Client::new(&req.pool)?;

    let upstream_req = UpstreamRequest {
        method: method.to_string(),
        key: req.physical_key.clone(),
        query: Vec::new(),
        headers: forwarded_request_headers(parts),
        body: upstream::Body::Empty,
    };
    client.send(upstream_req).await
}

pub async fn get(ctx: &AppContext, parts: &Parts, rid: &str) -> Response {
    match fetch(ctx, parts, "GET").await {
        Ok(res) => {
            let mut builder = Response::builder()
                .status(StatusCode::from_u16(res.status).unwrap_or(StatusCode::OK))
                .header("x-amz-request-id", rid);
            apply_response_headers(&mut builder, &res);
            builder
                .body(Body::from_stream(res.body))
                .unwrap_or_else(|_| {
                    fail(
                        &parts.method,
                        &S3Error::InternalError,
                        parts.uri.path(),
                        rid,
                    )
                })
        }
        Err(err) => fail(&parts.method, &err, parts.uri.path(), rid),
    }
}

pub async fn head(ctx: &AppContext, parts: &Parts, rid: &str) -> Response {
    match fetch(ctx, parts, "HEAD").await {
        Ok(res) => {
            let mut builder = Response::builder()
                .status(StatusCode::from_u16(res.status).unwrap_or(StatusCode::OK))
                .header("x-amz-request-id", rid);
            apply_response_headers(&mut builder, &res);
            // HEAD never carries a body: botocore reads Content-Length and a body here makes it mis-parse or hang.
            builder.body(Body::empty()).unwrap_or_else(|_| {
                fail(
                    &parts.method,
                    &S3Error::InternalError,
                    parts.uri.path(),
                    rid,
                )
            })
        }
        Err(err) => fail(&parts.method, &err, parts.uri.path(), rid),
    }
}
