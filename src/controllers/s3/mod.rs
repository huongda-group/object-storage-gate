//! The S3 route tree.
//!
//! axum cannot route on query parameters, and S3 overloads verbs onto the same path with them — `?uploads`, `?uploadId`, `?list-type=2`, `?delete`.
//! So each `(method, path-shape)` gets one handler that reads the query and dispatches.
//! That layer is forced by the protocol, not chosen.
//!
//! It is also where audit belongs (G7): it is the only place that sees both an auth failure and a result.
pub mod copy;
pub mod listing;
pub mod multipart;
pub mod object;

use std::sync::OnceLock;

use axum::{
    body::Body,
    extract::Request,
    http::{request::Parts, Method, StatusCode},
    response::{IntoResponse, Response},
};
use loco_rs::prelude::*;
use tower::ServiceExt;
use tower_http::services::{ServeDir, ServeFile};
use uuid::Uuid;

use crate::{
    models::{access_keys, audit_logs},
    s3::{error::S3Error, request::query_pairs, xml},
    workers::audit::{AuditArgs, AuditWorker},
};

/// Whether this request is addressed to the S3 data plane at all.
///
/// `/{bucket}/{*key}` matches almost every path on the host, including `/static/js/app.js` and every client-side route the console owns, so the route tree alone cannot tell an object request from a browser navigation.
/// `SigV4` credentials are the signal: an S3 client always presents one, a browser never does.
#[must_use]
pub fn is_s3_request(parts: &Parts) -> bool {
    // 1.
    // Credentials, in either form.
    // An S3 client that signs is unambiguous.
    let signed_header = parts
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("AWS4-HMAC-SHA256"));
    let query = parts.uri.query().unwrap_or_default().to_ascii_lowercase();
    if signed_header || query.contains("x-amz-signature=") {
        return true;
    }

    // 2.
    // A query parameter only S3 uses.
    // An unsigned S3 request still carries these, and a browser never does.
    for marker in [
        "list-type=",
        "uploads",
        "uploadid=",
        "partnumber=",
        "delete",
        "versionid=",
        "x-amz-",
    ] {
        if query.contains(marker) {
            return true;
        }
    }

    // 3.
    // The root is the console unless credentials say otherwise.
    // S3 has no anonymous ListBuckets at all, so an unsigned GET / is a browser asking for the app, never an S3 call.
    if parts.uri.path() == "/" {
        return false;
    }

    // 4.
    // The gateway serves its own paths on these names, so they are never buckets.
    // `buckets::validate_name` refuses the same list, which is what keeps the two ends honest: an unrouted /api path answers from the management API rather than as AccessDenied from S3, and a console asset that does not exist is a 404 from the console rather than an S3 error.
    let path = parts.uri.path();
    let first = path.trim_start_matches('/').split('/').next().unwrap_or("");
    if crate::models::buckets::RESERVED_BUCKET_NAMES.contains(&first) {
        return false;
    }

    // 5.
    // A browser navigation.
    // Only a navigation asks for HTML, so this is what keeps console deep links working without a database lookup.
    if parts
        .headers
        .get("accept")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("text/html"))
    {
        return false;
    }

    // 6.
    // A file the console actually ships.
    // Assets are fetched with Accept: text/css or */*, so they land here rather than above.
    if console_file_exists(path) {
        return false;
    }

    // Everything else is treated as S3, so an unsigned S3 request gets AccessDenied rather than a page of HTML.
    true
}

/// Whether `frontend/dist` holds a file at this path.
fn console_file_exists(path: &str) -> bool {
    let rel = path.trim_start_matches('/');
    if rel.is_empty() || rel.contains("..") {
        return false;
    }
    std::path::Path::new("frontend/dist").join(rel).is_file()
}

/// Serves the console for anything that reached an S3 route without S3 credentials.
///
/// This is the other half of `is_s3_request`: the static assets are mounted as the router's fallback, and a fallback never runs once a route matches.
/// Without this, registering the S3 tree turns every console deep link and every bundled asset into an S3 error.
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

/// Records one S3 request, having already produced the response.
///
/// This is the only place that sees both an authentication failure and a result, which is why `S3Request` is a constructor rather than an extractor: an extractor would have refused the request before anything here could describe it.
///
/// A queue outage must not turn a good request into a 500, so a failure to enqueue is logged and the response is returned unchanged.
async fn record_audit(
    ctx: &AppContext,
    parts: &Parts,
    action: &str,
    rid: &str,
    started: std::time::Instant,
    res: Response,
) -> Response {
    let status = res.status().as_u16();
    let bytes = res
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0);

    let (bucket, key) = {
        let (b, k) = crate::s3::request::split_path_public(parts.uri.path());
        (b, crate::s3::request::decode_key(&k))
    };

    // The access key id is taken from what the client presented, even when no such key exists: that is how key probing shows up in the log.
    let presented = crate::s3::sigv4::parse_authorization(&parts.headers)
        .ok()
        .map(|p| p.access_key_id)
        .or_else(|| {
            crate::s3::sigv4::parse_query(&query_pairs(parts))
                .ok()
                .map(|p| p.access_key_id)
        });

    let resolved = match presented.as_deref() {
        Some(id) => access_keys::Model::find_by_access_key_id(&ctx.db, id)
            .await
            .ok(),
        None => None,
    };

    let bucket_id = match (&resolved, bucket.is_empty()) {
        (Some(k), false) => {
            crate::models::buckets::Model::find_by_user_and_name(&ctx.db, k.user_id, &bucket)
                .await
                .ok()
                .flatten()
                .map(|b| b.id)
        }
        _ => None,
    };

    // The S3 code, when the response was an error.
    // A status alone cannot tell a wrong signature from a full bucket, and those are different operational problems.
    let code = res
        .extensions()
        .get::<crate::s3::xml::ErrorCode>()
        .map(|c| c.0.clone());

    let entry = audit_logs::AuditEntry {
        user_id: resolved.as_ref().map(|k| k.user_id),
        access_key_id: presented,
        bucket_id,
        object_key: (!key.is_empty()).then_some(key),
        // A 403 with no resolvable key never reached an action; recording it as `auth` is what makes key probing visible as its own thing.
        action: if status == 403 && resolved.is_none() {
            audit_logs::ACTION_AUTH.to_string()
        } else {
            action.to_string()
        },
        outcome: match code.as_deref() {
            Some("QuotaExceeded") => audit_logs::OUTCOME_QUOTA.to_string(),
            _ => audit_logs::outcome_for(status).to_string(),
        },
        status_code: i32::from(status),
        bytes,
        duration_ms: i32::try_from(started.elapsed().as_millis()).unwrap_or(i32::MAX),
        request_id: rid.to_string(),
        ip: client_ip(parts),
        user_agent: parts
            .headers
            .get("user-agent")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string),
    };

    if let Err(e) = AuditWorker::perform_later(ctx, AuditArgs(entry)).await {
        tracing::error!(error = %e, "could not enqueue an audit entry");
    }
    res
}

/// The client address, as far as the gateway can tell.
///
/// Reads `x-forwarded-for` only when the rate limiter is configured to trust it — the header is client-supplied, and an audit log full of addresses the client chose is worse than one that says `unknown`.
fn client_ip(parts: &Parts) -> String {
    let trusts_proxy =
        std::env::var("RATE_LIMIT_TRUST_PROXY").is_ok_and(|v| v == "true" || v == "1");
    if trusts_proxy {
        if let Some(v) = parts
            .headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
        {
            if let Some(first) = v.split(',').next() {
                return first.trim().to_string();
            }
        }
    }
    parts
        .extensions
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map_or_else(|| "unknown".to_string(), |c| c.0.ip().to_string())
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

/// The one place that sees both an authentication failure and a result.
///
/// `S3Request` is a constructor rather than an extractor precisely so this can exist: an extractor would have refused the request before anything here could describe it.
macro_rules! s3_handler {
    ($name:ident, $imp:ident, $action:expr) => {
        async fn $name(State(ctx): State<AppContext>, req: Request) -> Response {
            let (parts, body) = req.into_parts();
            if !is_s3_request(&parts) {
                return console_fallback(parts, body).await;
            }
            let rid = request_id();
            let started = std::time::Instant::now();
            let res = $imp(&ctx, &parts, body, &rid).await;
            record_audit(&ctx, &parts, $action, &rid, started, res).await
        }
    };
}

s3_handler!(list_buckets, list_buckets_impl, access_keys::ACTION_LIST);
s3_handler!(bucket_get, bucket_get_impl, access_keys::ACTION_LIST);
s3_handler!(bucket_head, bucket_head_impl, access_keys::ACTION_LIST);
s3_handler!(bucket_post, bucket_post_impl, access_keys::ACTION_DELETE);
s3_handler!(
    bucket_write_refused,
    bucket_write_refused_impl,
    access_keys::ACTION_WRITE
);
s3_handler!(object_get, object_get_impl, access_keys::ACTION_READ);
s3_handler!(object_head, object_head_impl, access_keys::ACTION_READ);
s3_handler!(object_put, object_put_impl, access_keys::ACTION_WRITE);
s3_handler!(object_post, object_post_impl, access_keys::ACTION_MULTIPART);
s3_handler!(
    object_delete,
    object_delete_impl,
    access_keys::ACTION_DELETE
);

async fn list_buckets_impl(ctx: &AppContext, parts: &Parts, _body: Body, rid: &str) -> Response {
    listing::list_buckets(ctx, parts, rid).await
}

async fn bucket_get_impl(ctx: &AppContext, parts: &Parts, _body: Body, rid: &str) -> Response {
    let query = query_pairs(parts);

    if param(&query, "list-type") == Some("2") {
        return listing::list_objects_v2(ctx, parts, rid).await;
    }
    if has_param(&query, "uploads") {
        return multipart::list_uploads(ctx, parts, rid).await;
    }
    // ListObjects V1 differs from V2 only in the pagination token names, and aws-cli, boto3 and rclone all send V2.
    not_implemented_after_bucket_auth(
        ctx,
        parts,
        access_keys::ACTION_LIST,
        "ListObjects (V1) is not supported; send list-type=2",
        rid,
    )
    .await
}

async fn bucket_head_impl(ctx: &AppContext, parts: &Parts, _body: Body, rid: &str) -> Response {
    listing::head_bucket(ctx, parts, rid).await
}

async fn bucket_post_impl(ctx: &AppContext, parts: &Parts, body: Body, rid: &str) -> Response {
    let query = query_pairs(parts);

    if !has_param(&query, "delete") {
        return not_implemented_after_bucket_auth(
            ctx,
            parts,
            access_keys::ACTION_DELETE,
            "POST on a bucket takes ?delete",
            rid,
        )
        .await;
    }

    let Ok(bytes) = axum::body::to_bytes(body, 8 * 1024 * 1024).await else {
        return fail(
            &parts.method,
            &S3Error::MalformedXml("could not read the request body".to_string()),
            parts.uri.path(),
            rid,
        );
    };
    object::delete_objects(ctx, parts, bytes.to_vec(), rid).await
}

/// A bucket is the unit of billing, so it is created in the console, not by a client.
///
/// This one answers before authenticating, unlike every other unimplemented branch: the name in the path is a bucket that does not exist yet, so resolving it first would answer `NoSuchBucket` and hide the sentence that tells the operator where to go.
/// It reveals nothing — the answer is the same for every path.
#[allow(clippy::unused_async)] // Kept async so every handler impl has one shape and the macro can call them all the same way.
async fn bucket_write_refused_impl(
    _ctx: &AppContext,
    parts: &Parts,
    _body: Body,
    rid: &str,
) -> Response {
    not_implemented(
        &parts.method,
        parts,
        "Buckets are created and deleted in the console, not over S3",
        rid,
    )
}

async fn object_get_impl(ctx: &AppContext, parts: &Parts, _body: Body, rid: &str) -> Response {
    let query = query_pairs(parts);

    if has_param(&query, "uploadId") {
        return multipart::list_parts(ctx, parts, rid).await;
    }
    object::get(ctx, parts, rid).await
}

async fn object_head_impl(ctx: &AppContext, parts: &Parts, _body: Body, rid: &str) -> Response {
    object::head(ctx, parts, rid).await
}

async fn object_put_impl(ctx: &AppContext, parts: &Parts, body: Body, rid: &str) -> Response {
    let query = query_pairs(parts);
    let is_copy = parts.headers.contains_key("x-amz-copy-source");

    if has_param(&query, "uploadId") && has_param(&query, "partNumber") {
        return if is_copy {
            copy::upload_part_copy(ctx, parts, rid).await
        } else {
            multipart::upload_part(ctx, parts, body, rid).await
        };
    }
    if is_copy {
        return copy::copy_object(ctx, parts, rid).await;
    }

    // The body is handed over unread: the reservation comes from Content-Length, so an over-quota upload is refused after the headers rather than after the client has pushed the whole object into the gateway.
    object::put(ctx, parts, body, rid).await
}

async fn object_post_impl(ctx: &AppContext, parts: &Parts, body: Body, rid: &str) -> Response {
    let query = query_pairs(parts);

    if has_param(&query, "uploads") {
        return multipart::create(ctx, parts, rid).await;
    }
    if has_param(&query, "uploadId") {
        let Ok(bytes) = axum::body::to_bytes(body, 8 * 1024 * 1024).await else {
            return fail(
                &parts.method,
                &S3Error::MalformedXml("could not read the request body".to_string()),
                parts.uri.path(),
                rid,
            );
        };
        return multipart::complete(ctx, parts, bytes.to_vec(), rid).await;
    }
    not_implemented_after_auth(
        ctx,
        parts,
        access_keys::ACTION_MULTIPART,
        "POST on an object takes ?uploads or ?uploadId",
        rid,
    )
    .await
}

async fn object_delete_impl(ctx: &AppContext, parts: &Parts, _body: Body, rid: &str) -> Response {
    let query = query_pairs(parts);

    if has_param(&query, "uploadId") {
        return multipart::abort(ctx, parts, rid).await;
    }
    object::delete(ctx, parts, rid).await
}

pub fn routes() -> Routes {
    Routes::new()
        // `/` is safe to route only because `is_s3_request` sends an unsigned request to the console: an S3 handler that answered every GET / would shadow the whole SPA, and the symptom is a blank console rather than anything that points at routing.
        .add("/", get(list_buckets))
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

/// The trailing-slash form of a bucket path, registered straight on the axum router.
///
/// Clients send both `/{bucket}` and `/{bucket}/` — botocore's own URL builder appends the slash for a bucket-level request — and to axum those are different routes. loco's `Routes` refuses the second as a duplicate of the first, so it is added here instead.
///
/// Normalising the path in a layer would be the obvious alternative and is wrong: the client signed the URI it sent, so trimming the slash before verification turns every such request into `SignatureDoesNotMatch`.
pub fn trailing_slash_bucket_router(ctx: AppContext) -> axum::Router {
    axum::Router::new()
        .route(
            "/{bucket}/",
            get(bucket_get)
                .head(bucket_head)
                .post(bucket_post)
                .put(bucket_write_refused)
                .delete(bucket_write_refused),
        )
        .with_state(ctx)
}
