//! Multipart upload.
//!
//! The store keeps the parts. The gateway keeps two facts: which upstream `UploadId` a client's
//! `UploadId` maps to, and how much quota the upload is holding so an abort gives back exactly
//! that.
use axum::{body::Body, http::request::Parts, response::Response};
use loco_rs::prelude::*;

use crate::{
    controllers::s3::{fail, param},
    models::{access_keys, multipart_uploads, objects, quota},
    s3::{
        error::S3Error,
        request::{query_pairs, S3Request},
        upstream::{self, UpstreamRequest},
        xml,
    },
};

/// Reads a whole upstream response body. Only used for the small XML answers multipart returns.
async fn body_text(res: upstream::UpstreamResponse) -> String {
    use futures_util::StreamExt;
    let mut body = res.body;
    let mut out = Vec::new();
    while let Some(chunk) = body.next().await {
        match chunk {
            Ok(b) => out.extend_from_slice(&b),
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

fn tag(text: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = text.find(&open)? + open.len();
    let rest = &text[start..];
    let end = rest.find(&close)?;
    Some(rest[..end].trim().to_string())
}

fn etag_header(res: &upstream::UpstreamResponse) -> String {
    res.headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("etag"))
        .map_or_else(String::new, |(_, v)| v.clone())
}

pub async fn create(ctx: &AppContext, parts: &Parts, rid: &str) -> Response {
    match create_inner(ctx, parts).await {
        Ok(body) => xml::ok_xml(body, rid),
        Err(err) => fail(&parts.method, &err, parts.uri.path(), rid),
    }
}

async fn create_inner(ctx: &AppContext, parts: &Parts) -> Result<String, S3Error> {
    let req = S3Request::resolve(ctx, parts, access_keys::ACTION_MULTIPART).await?;
    let client = upstream::Client::new(&req.pool)?;

    let res = client
        .send(UpstreamRequest {
            method: "POST".to_string(),
            key: req.physical_key.clone(),
            query: vec![("uploads".to_string(), String::new())],
            headers: Vec::new(),
            body: upstream::Body::Empty,
        })
        .await?;

    let text = body_text(res).await;
    let upstream_id = tag(&text, "UploadId").ok_or_else(|| {
        tracing::error!("upstream did not return an UploadId");
        S3Error::InternalError
    })?;

    let row =
        multipart_uploads::Model::create(&ctx.db, req.bucket.id, &req.logical_key, &upstream_id)
            .await?;

    // The client gets our pid, never the upstream id: an upstream identifier in a client's hands is a piece of the physical layout.
    Ok(xml::initiate_multipart(
        &req.bucket.name,
        &req.logical_key,
        &row.pid.to_string(),
    ))
}

pub async fn upload_part(ctx: &AppContext, parts: &Parts, body: Body, rid: &str) -> Response {
    match upload_part_inner(ctx, parts, body).await {
        Ok(etag) => Response::builder()
            .status(200)
            .header("x-amz-request-id", rid)
            .header("etag", etag)
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

async fn upload_part_inner(ctx: &AppContext, parts: &Parts, body: Body) -> Result<String, S3Error> {
    let req = S3Request::resolve(ctx, parts, access_keys::ACTION_MULTIPART).await?;
    let query = query_pairs(parts);
    let upload_id = param(&query, "uploadId").unwrap_or_default().to_string();
    let part_number = param(&query, "partNumber").unwrap_or_default().to_string();

    // Pinned to the bucket and key from the path: an UploadId issued for one bucket must not be usable against another's path.
    let upload =
        multipart_uploads::Model::find_for(&ctx.db, &upload_id, req.bucket.id, &req.logical_key)
            .await
            .map_err(|_| S3Error::NoSuchUpload)?;

    let len = parts
        .headers
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|l| *l >= 0)
        .ok_or(S3Error::MissingContentLength)?;

    let client = upstream::Client::new(&req.pool)?;
    let reservation = quota::reserve(&ctx.db, req.bucket.id, len).await?;

    let stream = futures_util::TryStreamExt::map_err(body.into_data_stream(), |e| {
        std::io::Error::other(e.to_string())
    });
    let sent = client
        .send(UpstreamRequest {
            method: "PUT".to_string(),
            key: req.physical_key.clone(),
            query: vec![
                ("uploadId".to_string(), upload.upstream_upload_id.clone()),
                ("partNumber".to_string(), part_number),
            ],
            headers: Vec::new(),
            body: upstream::Body::Stream {
                body: Box::pin(stream),
                length: u64::try_from(len).unwrap_or(0),
            },
        })
        .await;

    match sent {
        Ok(res) => {
            // The running total is what Abort gives back; a lost update here leaks quota nobody can trace.
            multipart_uploads::Model::add_reserved(&ctx.db, upload.id, len).await?;
            Ok(etag_header(&res))
        }
        Err(e) => {
            quota::release(&ctx.db, &reservation).await?;
            Err(e)
        }
    }
}

pub async fn abort(ctx: &AppContext, parts: &Parts, rid: &str) -> Response {
    match abort_inner(ctx, parts).await {
        Ok(()) => Response::builder()
            .status(204)
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

async fn abort_inner(ctx: &AppContext, parts: &Parts) -> Result<(), S3Error> {
    let req = S3Request::resolve(ctx, parts, access_keys::ACTION_MULTIPART).await?;
    let query = query_pairs(parts);
    let upload_id = param(&query, "uploadId").unwrap_or_default().to_string();

    let upload =
        multipart_uploads::Model::find_for(&ctx.db, &upload_id, req.bucket.id, &req.logical_key)
            .await
            .map_err(|_| S3Error::NoSuchUpload)?;

    let client = upstream::Client::new(&req.pool)?;
    client
        .send(UpstreamRequest {
            method: "DELETE".to_string(),
            key: req.physical_key.clone(),
            query: vec![("uploadId".to_string(), upload.upstream_upload_id.clone())],
            headers: Vec::new(),
            body: upstream::Body::Empty,
        })
        .await?;

    // Give back exactly what the parts reserved, then drop the row.
    let hold = quota::held(&ctx.db, req.bucket.id, upload.reserved_bytes).await?;
    quota::release(&ctx.db, &hold).await?;
    upload.remove(&ctx.db).await?;
    Ok(())
}

pub async fn complete(ctx: &AppContext, parts: &Parts, body: Vec<u8>, rid: &str) -> Response {
    match complete_inner(ctx, parts, body).await {
        Ok(xml_body) => xml::ok_xml(xml_body, rid),
        Err(err) => fail(&parts.method, &err, parts.uri.path(), rid),
    }
}

async fn complete_inner(ctx: &AppContext, parts: &Parts, body: Vec<u8>) -> Result<String, S3Error> {
    let req = S3Request::resolve(ctx, parts, access_keys::ACTION_MULTIPART).await?;
    let query = query_pairs(parts);
    let upload_id = param(&query, "uploadId").unwrap_or_default().to_string();

    let upload =
        multipart_uploads::Model::find_for(&ctx.db, &upload_id, req.bucket.id, &req.logical_key)
            .await
            .map_err(|_| S3Error::NoSuchUpload)?;

    let part_list = xml::parse_complete_request(&body)?;
    let client = upstream::Client::new(&req.pool)?;

    let res = client
        .send(UpstreamRequest {
            method: "POST".to_string(),
            key: req.physical_key.clone(),
            query: vec![("uploadId".to_string(), upload.upstream_upload_id.clone())],
            headers: vec![("content-type".to_string(), "application/xml".to_string())],
            body: upstream::Body::Bytes(xml::complete_request(&part_list).into_bytes()),
        })
        .await?;

    let text = body_text(res).await;
    let etag = tag(&text, "ETag").unwrap_or_default();

    // The real size comes from the store, not from adding up what the client said each part was: the parts are the store's, and its answer is the only one that matches the bytes.
    let head = client
        .send(UpstreamRequest::head(&req.physical_key))
        .await?;
    let size = head
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<i64>().ok())
        .unwrap_or(0);

    // record_put, not put_object: the reservation was accumulated across the parts, so this path owns the accounting and must not charge again.
    objects::Model::record_put(
        &ctx.db,
        req.bucket.id,
        &req.logical_key,
        size,
        &etag,
        "application/octet-stream",
    )
    .await?;
    // Two steps rather than one bespoke query: turn the accumulated hold into used bytes, then settle the difference between what the parts reserved and what the store says the object actually is.
    // A positive difference is not re-checked against the quota — the parts were already admitted, and refusing here would leave an object in the store that the counters deny.
    let hold = quota::held(&ctx.db, req.bucket.id, upload.reserved_bytes).await?;
    quota::commit(&ctx.db, &hold, 1).await?;
    if size != upload.reserved_bytes {
        tracing::warn!(
            reserved = upload.reserved_bytes,
            actual = size,
            "multipart object size differs from the sum of its reserved parts"
        );
        quota::settle(&ctx.db, req.bucket.id, size - upload.reserved_bytes, 0).await?;
    }
    upload.remove(&ctx.db).await?;

    Ok(xml::complete_multipart(
        &req.bucket.name,
        &req.logical_key,
        &etag,
    ))
}

/// `ListParts` asks the store, because the store is where the parts are.
pub async fn list_parts(ctx: &AppContext, parts: &Parts, rid: &str) -> Response {
    match list_parts_inner(ctx, parts).await {
        Ok(body) => xml::ok_xml(body, rid),
        Err(err) => fail(&parts.method, &err, parts.uri.path(), rid),
    }
}

async fn list_parts_inner(ctx: &AppContext, parts: &Parts) -> Result<String, S3Error> {
    let req = S3Request::resolve(ctx, parts, access_keys::ACTION_MULTIPART).await?;
    let query = query_pairs(parts);
    let upload_id = param(&query, "uploadId").unwrap_or_default().to_string();

    let upload =
        multipart_uploads::Model::find_for(&ctx.db, &upload_id, req.bucket.id, &req.logical_key)
            .await
            .map_err(|_| S3Error::NoSuchUpload)?;

    let client = upstream::Client::new(&req.pool)?;
    let res = client
        .send(UpstreamRequest {
            method: "GET".to_string(),
            key: req.physical_key.clone(),
            query: vec![("uploadId".to_string(), upload.upstream_upload_id.clone())],
            headers: Vec::new(),
            body: upstream::Body::Empty,
        })
        .await?;

    let text = body_text(res).await;
    let listed = parse_parts(&text);

    Ok(xml::list_parts(
        &req.bucket.name,
        &req.logical_key,
        &upload_id,
        &listed,
    ))
}

/// Pulls `(PartNumber, ETag, Size)` out of an upstream `ListPartsResult`.
///
/// Re-rendered rather than forwarded: the upstream body names the physical bucket and key.
fn parse_parts(text: &str) -> Vec<(u32, String, i64)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("<Part>") {
        rest = &rest[i + 6..];
        let Some(j) = rest.find("</Part>") else { break };
        let one = &rest[..j];
        let number = tag(one, "PartNumber").and_then(|v| v.parse().ok());
        let etag = tag(one, "ETag");
        let size = tag(one, "Size").and_then(|v| v.parse().ok());
        if let (Some(n), Some(e), Some(s)) = (number, etag, size) {
            out.push((n, e, s));
        }
        rest = &rest[j + 7..];
    }
    out
}

/// `ListMultipartUploads` reads this gateway's own table: the store's answer would carry physical
/// keys and would also list uploads belonging to other tenants in the same physical bucket.
pub async fn list_uploads(ctx: &AppContext, parts: &Parts, rid: &str) -> Response {
    match list_uploads_inner(ctx, parts).await {
        Ok(body) => xml::ok_xml(body, rid),
        Err(err) => fail(&parts.method, &err, parts.uri.path(), rid),
    }
}

async fn list_uploads_inner(ctx: &AppContext, parts: &Parts) -> Result<String, S3Error> {
    let req = S3Request::resolve_bucket_only(ctx, parts, access_keys::ACTION_MULTIPART).await?;
    let query = query_pairs(parts);
    let prefix = param(&query, "prefix").unwrap_or_default();

    let allowed = req
        .key
        .prefixes(&ctx.db)
        .await
        .map_err(|_| S3Error::InternalError)?;
    if !crate::controllers::s3::listing::may_list(&allowed, prefix) {
        return Err(S3Error::AccessDenied);
    }

    let rows = multipart_uploads::Model::list_for_bucket(&ctx.db, req.bucket.id, prefix).await?;
    let listed: Vec<(String, String, String)> = rows
        .iter()
        .map(|u| {
            (
                u.object_key.clone(),
                u.pid.to_string(),
                u.created_at.to_rfc3339(),
            )
        })
        .collect();

    Ok(xml::list_multipart_uploads(&req.bucket.name, &listed))
}
