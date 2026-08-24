//! `CopyObject` and `UploadPartCopy`.
//!
//! Both ends of a copy go through the same resolver. Two ends checked by two different pieces of
//! code is the classic way one of them ends up unchecked.
use axum::{http::request::Parts, response::Response};
use loco_rs::prelude::*;

use crate::{
    controllers::s3::{fail, param},
    models::{access_keys, objects},
    s3::{
        error::S3Error,
        request::{query_pairs, S3Request},
        upstream::{self, UpstreamRequest},
        xml,
    },
};

fn copy_source_of(parts: &Parts) -> Result<String, S3Error> {
    parts
        .headers
        .get("x-amz-copy-source")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .ok_or_else(|| S3Error::InvalidArgument("x-amz-copy-source is required".to_string()))
}

fn etag_of(res: &upstream::UpstreamResponse) -> String {
    res.headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("etag"))
        .map_or_else(String::new, |(_, v)| v.clone())
}

pub async fn copy_object(ctx: &AppContext, parts: &Parts, rid: &str) -> Response {
    match copy_object_inner(ctx, parts).await {
        Ok(body) => xml::ok_xml(body, rid),
        Err(err) => fail(&parts.method, &err, parts.uri.path(), rid),
    }
}

async fn copy_object_inner(ctx: &AppContext, parts: &Parts) -> Result<String, S3Error> {
    let dest = S3Request::resolve(ctx, parts, access_keys::ACTION_WRITE).await?;
    let source = dest
        .resolve_copy_source(ctx, &copy_source_of(parts)?)
        .await?;

    // The source must exist as metadata, because that is where the size for the reservation comes from — asking the store would cost a round trip and still not be the number the counters use.
    let row = objects::Model::get(&ctx.db, source.bucket.id, &source.logical_key)
        .await?
        .ok_or(S3Error::NoSuchKey)?;

    let client = upstream::Client::new(&dest.pool)?;
    let pending =
        objects::Model::begin_put(&ctx.db, dest.bucket.id, &dest.logical_key, row.size).await?;

    // The header must be rewritten to the physical source. Forwarding the client's logical value makes the store look for a key it has never had, and the failure comes back as NoSuchKey — which reads as "your object is missing", not as "the gateway forgot to rewrite".
    let sent = client
        .send(
            UpstreamRequest::put(&dest.physical_key, upstream::Body::Empty).with_headers(vec![(
                "x-amz-copy-source".to_string(),
                format!("/{}/{}", dest.pool.physical_bucket, source.physical_key),
            )]),
        )
        .await;

    match sent {
        Ok(res) => {
            let etag = etag_of(&res);
            let written = pending.commit(&ctx.db, &etag, &row.content_type).await?;
            Ok(xml::copy_result(
                "CopyObjectResult",
                &etag,
                &written.updated_at.to_rfc3339(),
            ))
        }
        Err(e) => {
            pending.abort(&ctx.db).await?;
            Err(e)
        }
    }
}

pub async fn upload_part_copy(ctx: &AppContext, parts: &Parts, rid: &str) -> Response {
    match upload_part_copy_inner(ctx, parts).await {
        Ok(body) => xml::ok_xml(body, rid),
        Err(err) => fail(&parts.method, &err, parts.uri.path(), rid),
    }
}

async fn upload_part_copy_inner(ctx: &AppContext, parts: &Parts) -> Result<String, S3Error> {
    use crate::models::{multipart_uploads, quota};

    let dest = S3Request::resolve(ctx, parts, access_keys::ACTION_MULTIPART).await?;
    let source = dest
        .resolve_copy_source(ctx, &copy_source_of(parts)?)
        .await?;

    let query = query_pairs(parts);
    let upload_id = param(&query, "uploadId").unwrap_or_default().to_string();
    let part_number = param(&query, "partNumber").unwrap_or_default().to_string();

    let upload =
        multipart_uploads::Model::find_for(&ctx.db, &upload_id, dest.bucket.id, &dest.logical_key)
            .await
            .map_err(|_| S3Error::NoSuchUpload)?;

    let row = objects::Model::get(&ctx.db, source.bucket.id, &source.logical_key)
        .await?
        .ok_or(S3Error::NoSuchKey)?;

    let client = upstream::Client::new(&dest.pool)?;
    let reservation = quota::reserve(&ctx.db, dest.bucket.id, row.size).await?;

    let sent = client
        .send(UpstreamRequest {
            method: "PUT".to_string(),
            key: dest.physical_key.clone(),
            query: vec![
                ("uploadId".to_string(), upload.upstream_upload_id.clone()),
                ("partNumber".to_string(), part_number),
            ],
            headers: vec![(
                "x-amz-copy-source".to_string(),
                format!("/{}/{}", dest.pool.physical_bucket, source.physical_key),
            )],
            body: upstream::Body::Empty,
        })
        .await;

    match sent {
        Ok(res) => {
            multipart_uploads::Model::add_reserved(&ctx.db, upload.id, row.size).await?;
            Ok(xml::copy_result(
                "CopyPartResult",
                &etag_of(&res),
                &row.updated_at.to_rfc3339(),
            ))
        }
        Err(e) => {
            quota::release(&ctx.db, &reservation).await?;
            Err(e)
        }
    }
}
