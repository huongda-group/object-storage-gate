//! The S3 route tree.
//!
//! axum cannot route on query parameters, and S3 overloads verbs onto the same path with them — `?uploads`, `?uploadId`, `?list-type=2`, `?delete`.
//! So each `(method, path-shape)` gets one handler that reads the query and dispatches. That layer is forced by the protocol, not chosen.
//!
//! It is also where audit belongs (G7): it is the only place that sees both an auth failure and a result.
pub mod object;

use std::sync::OnceLock;

use axum::{
    extract::Request,
    http::{request::Parts, Method, StatusCode},
    response::{IntoResponse, Response},
};
use loco_rs::prelude::*;
use tower::ServiceExt;
use tower_http::services::{ServeDir, ServeFile};
use uuid::Uuid;

use crate::{
    models::access_keys,
    s3::{error::S3Error, request::query_pairs, xml},
};

/// Whether this request is addressed to the S3 data plane at all.
///
/// `/{bucket}/{*key}` matches almost every path on the host, including `/static/js/app.js` and every client-side route the console owns, so the route tree alone cannot tell an object request from a browser navigation.
/// `SigV4` credentials are the signal: an S3 client always presents one, a browser never does.
#[must_use]
pub fn is_s3_request(parts: &Parts) -> bool {
    let signed_header = parts
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("AWS4-HMAC-SHA256"));
    signed_header
        || parts
            .uri
            .query()
            .is_some_and(|q| q.to_ascii_lowercase().contains("x-amz-signature="))
}

/// Serves the console for anything that reached an S3 route without S3 credentials.
///
/// This is the other half of `is_s3_request`: the static assets are mounted as the router's fallback, and a fallback never runs once a route matches. Without this, registering the S3 tree turns every console deep link and every bundled asset into an S3 error.
async fn console_fallback(parts: Parts, body: axum::body::Body) -> Response {
    static DIR: OnceLock<Option<ServeDir<ServeFile>>> = OnceLock::new();
    let dir = DIR.get_or_init(|| {
        let folder = std::path::Path::new("frontend/dist");
        folder.is_dir().then(|| {
            // `fallback`, not `not_found_service`: a console deep link must arrive as a 200 carrying index.html, which is what the router's own static fallback does for paths the S3 tree does not shadow.
            ServeDir::new(folder).fallback(ServeFile::new(folder.join("index.html")))
        })
    });

    let Some(dir) = dir.clone() else {
        return (StatusCode::NOT_FOUND, ()).into_response();
    };
    dir.oneshot(Request::from_parts(parts, body))
        .await
        .map_or_else(
            |_| (StatusCode::NOT_FOUND, ()).into_response(),
            IntoResponse::into_response,
        )
}

/// One id per request, echoed in `x-amz-request-id` and in every error body.
/// Clients quote it in bug reports, and G7's audit rows key on it.
#[must_use]
pub fn request_id() -> String {
    Uuid::new_v4().simple().to_string()
}

/// Whether a query parameter is present at all, with or without a value.
#[must_use]
pub fn has_param(query: &[(String, String)], name: &str) -> bool {
    query.iter().any(|(k, _)| k.eq_ignore_ascii_case(name))
}

#[must_use]
pub fn param<'a>(query: &'a [(String, String)], name: &str) -> Option<&'a str> {
    query
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// Turns an `S3Error` into a response, choosing the body-less shape for HEAD.
#[must_use]
pub fn fail(method: &Method, err: &S3Error, resource: &str, request_id: &str) -> Response {
    if method == Method::HEAD {
        xml::error_response_headless(err, request_id)
    } else {
        xml::error_response(err, resource, request_id)
    }
}

fn not_implemented(method: &Method, parts: &Parts, what: &str, rid: &str) -> Response {
    fail(
        method,
        &S3Error::NotImplemented(what.to_string()),
        parts.uri.path(),
        rid,
    )
}

/// As `not_implemented_after_auth`, for the verbs that address a bucket and no object.
async fn not_implemented_after_bucket_auth(
    ctx: &AppContext,
    parts: &Parts,
    action: &str,
    what: &str,
    rid: &str,
) -> Response {
    if let Err(err) = crate::s3::request::S3Request::resolve_bucket_only(ctx, parts, action).await {
        return fail(&parts.method, &err, parts.uri.path(), rid);
    }
    not_implemented(&parts.method, parts, what, rid)
}

/// A verb that is not built yet, but still runs the full authorisation chain first.
///
/// Answering 501 before authenticating would make an unsigned request learn which verbs exist, and would make the isolation tests unable to assert a uniform refusal — a scoped key writing outside its prefix must be refused for that reason, not for the unrelated reason that the verb is unfinished.
async fn not_implemented_after_auth(
    ctx: &AppContext,
    parts: &Parts,
    action: &str,
    what: &str,
    rid: &str,
) -> Response {
    if let Err(err) = crate::s3::request::S3Request::resolve(ctx, parts, action).await {
        return fail(&parts.method, &err, parts.uri.path(), rid);
    }
    not_implemented(&parts.method, parts, what, rid)
}

async fn bucket_get(State(ctx): State<AppContext>, req: Request) -> Response {
    let (parts, body) = req.into_parts();
    if !is_s3_request(&parts) {
        return console_fallback(parts, body).await;
    }
    let rid = request_id();
    let query = query_pairs(&parts);

    let (action, what) = if param(&query, "list-type") == Some("2") {
        (
            access_keys::ACTION_LIST,
            "ListObjectsV2 is not implemented yet",
        )
    } else if has_param(&query, "uploads") {
        (
            access_keys::ACTION_MULTIPART,
            "ListMultipartUploads is not implemented yet",
        )
    } else {
        // ListObjects V1 differs from V2 only in the pagination token names, and aws-cli, boto3 and rclone all send V2.
        (
            access_keys::ACTION_LIST,
            "ListObjects (V1) is not supported; send list-type=2",
        )
    };
    not_implemented_after_bucket_auth(&ctx, &parts, action, what, &rid).await
}

async fn bucket_head(State(ctx): State<AppContext>, req: Request) -> Response {
    let (parts, body) = req.into_parts();
    if !is_s3_request(&parts) {
        return console_fallback(parts, body).await;
    }
    let rid = request_id();
    not_implemented_after_bucket_auth(
        &ctx,
        &parts,
        access_keys::ACTION_LIST,
        "HeadBucket is not implemented yet",
        &rid,
    )
    .await
}

async fn bucket_post(State(ctx): State<AppContext>, req: Request) -> Response {
    let (parts, body) = req.into_parts();
    if !is_s3_request(&parts) {
        return console_fallback(parts, body).await;
    }
    let rid = request_id();
    let query = query_pairs(&parts);

    if !has_param(&query, "delete") {
        return not_implemented_after_bucket_auth(
            &ctx,
            &parts,
            access_keys::ACTION_DELETE,
            "POST on a bucket takes ?delete",
            &rid,
        )
        .await;
    }

    let Ok(bytes) = axum::body::to_bytes(body, 8 * 1024 * 1024).await else {
        return fail(
            &parts.method,
            &S3Error::MalformedXml("could not read the request body".to_string()),
            parts.uri.path(),
            &rid,
        );
    };
    object::delete_objects(&ctx, &parts, bytes.to_vec(), &rid).await
}

/// A bucket is the unit of billing, so it is created in the console, not by a client.
///
/// This one answers before authenticating, unlike every other unimplemented branch: the name in the path is a bucket that does not exist yet, so resolving it first would answer `NoSuchBucket` and hide the sentence that tells the operator where to go.
/// It reveals nothing — the answer is the same for every path.
async fn bucket_write_refused(req: Request) -> Response {
    let (parts, body) = req.into_parts();
    if !is_s3_request(&parts) {
        return console_fallback(parts, body).await;
    }
    let rid = request_id();
    not_implemented(
        &parts.method.clone(),
        &parts,
        "Buckets are created and deleted in the console, not over S3",
        &rid,
    )
}

async fn object_get(State(ctx): State<AppContext>, req: Request) -> Response {
    let (parts, body) = req.into_parts();
    if !is_s3_request(&parts) {
        return console_fallback(parts, body).await;
    }
    let rid = request_id();
    let query = query_pairs(&parts);

    if has_param(&query, "uploadId") {
        return not_implemented_after_auth(
            &ctx,
            &parts,
            access_keys::ACTION_MULTIPART,
            "ListParts is not implemented yet",
            &rid,
        )
        .await;
    }
    object::get(&ctx, &parts, &rid).await
}

async fn object_head(State(ctx): State<AppContext>, req: Request) -> Response {
    let (parts, body) = req.into_parts();
    if !is_s3_request(&parts) {
        return console_fallback(parts, body).await;
    }
    let rid = request_id();
    object::head(&ctx, &parts, &rid).await
}

async fn object_put(State(ctx): State<AppContext>, req: Request) -> Response {
    let (parts, body) = req.into_parts();
    if !is_s3_request(&parts) {
        return console_fallback(parts, body).await;
    }
    let rid = request_id();
    let query = query_pairs(&parts);
    let is_copy = parts.headers.contains_key("x-amz-copy-source");

    if has_param(&query, "uploadId") && has_param(&query, "partNumber") {
        return not_implemented_after_auth(
            &ctx,
            &parts,
            access_keys::ACTION_MULTIPART,
            if is_copy {
                "UploadPartCopy is not implemented yet"
            } else {
                "UploadPart is not implemented yet"
            },
            &rid,
        )
        .await;
    }
    if is_copy {
        return not_implemented_after_auth(
            &ctx,
            &parts,
            access_keys::ACTION_WRITE,
            "CopyObject is not implemented yet",
            &rid,
        )
        .await;
    }

    // The body is read into memory because `begin_put` needs the size before any byte moves.
    // ponytail: buffers the whole object, so a 5 GiB PutObject holds 5 GiB of RSS.
    // Upgrade path: hand the axum body straight to `upstream::Body::Stream`, which the client already supports; that needs the handler to trust Content-Length for the reservation and stop taking `Vec<u8>`.
    let Ok(bytes) = axum::body::to_bytes(body, usize::MAX).await else {
        return fail(
            &parts.method,
            &S3Error::InvalidRequest("could not read the request body".to_string()),
            parts.uri.path(),
            &rid,
        );
    };
    object::put(&ctx, &parts, bytes.to_vec(), &rid).await
}

async fn object_post(State(ctx): State<AppContext>, req: Request) -> Response {
    let (parts, body) = req.into_parts();
    if !is_s3_request(&parts) {
        return console_fallback(parts, body).await;
    }
    let rid = request_id();
    let query = query_pairs(&parts);

    let what = if has_param(&query, "uploads") {
        "CreateMultipartUpload is not implemented yet"
    } else if has_param(&query, "uploadId") {
        "CompleteMultipartUpload is not implemented yet"
    } else {
        "POST on an object takes ?uploads or ?uploadId"
    };
    not_implemented_after_auth(&ctx, &parts, access_keys::ACTION_MULTIPART, what, &rid).await
}

async fn object_delete(State(ctx): State<AppContext>, req: Request) -> Response {
    let (parts, body) = req.into_parts();
    if !is_s3_request(&parts) {
        return console_fallback(parts, body).await;
    }
    let rid = request_id();
    let query = query_pairs(&parts);

    if has_param(&query, "uploadId") {
        return not_implemented_after_auth(
            &ctx,
            &parts,
            access_keys::ACTION_MULTIPART,
            "AbortMultipartUpload is not implemented yet",
            &rid,
        )
        .await;
    }
    object::delete(&ctx, &parts, &rid).await
}

pub fn routes() -> Routes {
    Routes::new()
        // `/` is deliberately not routed here.
        // The console is served from the static fallback at `/`, and an S3 route on it shadows the whole SPA — the symptom is a blank console, which does not point at routing.
        // ListBuckets lands in G5 and has to solve that collision by looking at whether the request carries SigV4 credentials at all.
        .add(
            "/{bucket}",
            get(bucket_get)
                .head(bucket_head)
                .post(bucket_post)
                .put(bucket_write_refused)
                .delete(bucket_write_refused),
        )
        .add(
            "/{bucket}/{*key}",
            get(object_get)
                .head(object_head)
                .put(object_put)
                .post(object_post)
                .delete(object_delete),
        )
}
