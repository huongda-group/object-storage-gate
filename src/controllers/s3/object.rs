//! `GetObject` and `HeadObject`.
//!
//! Body and headers come from upstream; the `objects` row is metadata for listing and quota, not the source of truth for content.
use std::fmt::Write as _;

use axum::{
    body::Body,
    http::{request::Parts, HeaderName, HeaderValue, StatusCode},
    response::Response,
};
use loco_rs::prelude::*;

use crate::{
    controllers::s3::fail,
    models::{access_keys, objects},
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

/// Headers forwarded from the client up to the store on a write.
///
/// A whitelist, not a blacklist. `x-amz-acl`, `x-amz-server-side-encryption` and
/// `x-amz-storage-class` are deliberately absent: forwarding one would let a client set a public
/// ACL on an object inside the shared physical bucket, opening their data over a path the gateway
/// knows nothing about. Spec §19 puts ACL and SSE out of scope.
const FORWARD_ON_WRITE: &[&str] = &[
    "content-type",
    "content-encoding",
    "content-disposition",
    "content-language",
    "cache-control",
    "expires",
];

fn forwarded_write_headers(parts: &Parts) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (name, value) in &parts.headers {
        let lower = name.as_str().to_ascii_lowercase();
        // `x-amz-meta-*` is the client's own metadata and belongs to them.
        if !FORWARD_ON_WRITE.contains(&lower.as_str()) && !lower.starts_with("x-amz-meta-") {
            continue;
        }
        if let Ok(v) = value.to_str() {
            out.push((lower, v.to_string()));
        }
    }
    out
}

/// The `Content-Length` a client declared.
///
/// Required: the reservation needs a size before any byte moves, and a chunked body would have to
/// be buffered to find out — which is the thing streaming exists to avoid.
fn content_length(parts: &Parts) -> Result<i64, S3Error> {
    parts
        .headers
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|len| *len >= 0)
        .ok_or(S3Error::MissingContentLength)
}

fn etag_of(res: &UpstreamResponse) -> String {
    res.headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("etag"))
        .map_or_else(String::new, |(_, v)| v.clone())
}

fn content_type_of(parts: &Parts) -> String {
    parts
        .headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string()
}

pub async fn put(ctx: &AppContext, parts: &Parts, body: Vec<u8>, rid: &str) -> Response {
    match put_inner(ctx, parts, body).await {
        Ok(res) => {
            let mut builder = Response::builder()
                .status(StatusCode::from_u16(res.status).unwrap_or(StatusCode::OK))
                .header("x-amz-request-id", rid);
            apply_response_headers(&mut builder, &res);
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

async fn put_inner(
    ctx: &AppContext,
    parts: &Parts,
    body: Vec<u8>,
) -> Result<UpstreamResponse, S3Error> {
    // resolve() rejects an aws-chunked payload hash before anything else, so a 501 costs no reservation.
    let req = S3Request::resolve(ctx, parts, access_keys::ACTION_WRITE).await?;
    let len = content_length(parts)?;
    let client = upstream::Client::new(&req.pool)?;

    // The hold is taken before a single byte leaves the gateway: an over-quota write must never reach the store.
    let pending = objects::Model::begin_put(&ctx.db, req.bucket.id, &req.logical_key, len).await?;

    let upstream_req = UpstreamRequest::put(&req.physical_key, upstream::Body::Bytes(body))
        .with_headers(forwarded_write_headers(parts));

    match client.send(upstream_req).await {
        Ok(res) => {
            let etag = etag_of(&res);
            pending
                .commit(&ctx.db, &etag, &content_type_of(parts))
                .await?;
            Ok(res)
        }
        Err(e) => {
            // Give the hold straight back. A failed upload that keeps its reservation is a bucket that slowly refuses writes with nothing in the logs to explain it.
            pending.abort(&ctx.db).await?;
            Err(e)
        }
    }
}

pub async fn delete(ctx: &AppContext, parts: &Parts, rid: &str) -> Response {
    match delete_inner(ctx, parts).await {
        Ok(()) => Response::builder()
            .status(StatusCode::NO_CONTENT)
            .header("x-amz-request-id", rid)
            .body(Body::empty())
            .unwrap_or_else(|_| {
                fail(
                    &parts.method,
                    &S3Error::InternalError,
                    parts.uri.path(),
                    rid,
                )
            }),
        Err(err) => fail(&parts.method, &err, parts.uri.path(), rid),
    }
}

async fn delete_inner(ctx: &AppContext, parts: &Parts) -> Result<(), S3Error> {
    let req = S3Request::resolve(ctx, parts, access_keys::ACTION_DELETE).await?;
    let client = upstream::Client::new(&req.pool)?;

    client
        .send(UpstreamRequest::delete(&req.physical_key))
        .await?;

    // Metadata comes off after the store confirmed. The other order loses track of an object that still exists; this order can leave a row behind if the process dies here, which reconcile_quota fixes.
    objects::Model::delete(&ctx.db, req.bucket.id, &req.logical_key).await?;
    Ok(())
}

/// `POST /{bucket}?delete` — the batch form.
///
/// Authorisation is per key rather than up front: a batch has no single key for `resolve` to check a prefix against, and S3's batch semantics turn a refused key into one `<Error>` entry rather than a refusal of the whole request. A whole-request refusal would let one bad key undo 999 good ones.
pub async fn delete_objects(ctx: &AppContext, parts: &Parts, body: Vec<u8>, rid: &str) -> Response {
    match delete_objects_inner(ctx, parts, body).await {
        Ok(xml) => crate::s3::xml::ok_xml(xml, rid),
        Err(err) => fail(&parts.method, &err, parts.uri.path(), rid),
    }
}

async fn delete_objects_inner(
    ctx: &AppContext,
    parts: &Parts,
    body: Vec<u8>,
) -> Result<String, S3Error> {
    let req = S3Request::resolve_bucket_only(ctx, parts, access_keys::ACTION_DELETE).await?;
    let (keys, quiet) = crate::s3::xml::parse_delete_request(&body)?;

    let mut allowed: Vec<String> = Vec::new();
    let mut errors: Vec<(String, S3Error)> = Vec::new();
    for key in keys {
        if let Err(e) = crate::s3::request::validate_logical_key(&key) {
            errors.push((key, e));
            continue;
        }
        let permitted = req
            .key
            .allows_key(&ctx.db, &key)
            .await
            .map_err(|_| S3Error::InternalError)?;
        if permitted {
            allowed.push(key);
        } else {
            errors.push((key, S3Error::AccessDenied));
        }
    }

    if allowed.is_empty() {
        return Ok(crate::s3::xml::delete_result(&[], &errors, quiet));
    }

    // Only the authorised keys are rewritten, and only those reach the store. An implementation that authorises and then forwards the whole list passes every response assertion while still deleting data out of scope.
    let client = upstream::Client::new(&req.pool)?;
    let mut upstream_body = String::from("<Delete><Quiet>true</Quiet>");
    for key in &allowed {
        let physical = format!("{}/{}/{}", req.user.pid, req.bucket.name, key);
        let _ = write!(
            upstream_body,
            "<Object><Key>{}</Key></Object>",
            crate::s3::xml::escape(&physical)
        );
    }
    upstream_body.push_str("</Delete>");

    let upstream_req = UpstreamRequest {
        method: "POST".to_string(),
        key: String::new(),
        query: vec![("delete".to_string(), String::new())],
        headers: vec![("content-type".to_string(), "application/xml".to_string())],
        body: upstream::Body::Bytes(upstream_body.into_bytes()),
    };
    client.send(upstream_req).await?;

    for key in &allowed {
        objects::Model::delete(&ctx.db, req.bucket.id, key).await?;
    }

    Ok(crate::s3::xml::delete_result(&allowed, &errors, quiet))
}
