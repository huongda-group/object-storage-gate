//! Listing, served entirely from the database.
//!
//! No upstream call: "quota is DB-driven, never bucket-scanned" applies to listing too, and a
//! `ListObjectsV2` that asked the store would make every page cost a round trip to it.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

use crate::s3::error::S3Error;

/// S3's ceiling, and what a client gets when it asks for more.
pub const MAX_KEYS_LIMIT: u64 = 1000;

/// One row as the listing sees it, so `roll_up` can be tested without a database.
#[derive(Debug, Clone)]
pub struct Row {
    pub key: String,
    pub size: i64,
    pub etag: String,
    pub modified: String,
}

#[derive(Debug, Default)]
pub struct Page {
    pub contents: Vec<Row>,
    pub common_prefixes: Vec<String>,
    pub is_truncated: bool,
    pub next_token: Option<String>,
}

/// Groups a page of keys into `Contents` and `CommonPrefixes`.
///
/// The delimiter is searched in the part of the key *after* the prefix. Searching from the start
/// of the key would roll everything under `img/` into the single entry `img/` when that is the
/// prefix being listed.
///
/// A common prefix counts against `limit` the same as a key does, because S3's `KeyCount` is the
/// sum of both and a client sizing its buffer from it would under-allocate otherwise.
///
/// `next_token` is the last key *consumed*, not the last key emitted in `Contents`: a page that
/// ended part-way through a roll-up must resume after the whole group, or the next page repeats
/// the common prefix and the client sees the folder twice.
#[must_use]
pub fn roll_up(rows: Vec<Row>, prefix: &str, delimiter: Option<char>, limit: u64) -> Page {
    let mut page = Page::default();
    let mut emitted: u64 = 0;
    let mut last_consumed: Option<String> = None;

    for row in rows {
        if emitted >= limit {
            page.is_truncated = true;
            break;
        }

        let Some(delim) = delimiter else {
            last_consumed = Some(row.key.clone());
            page.contents.push(row);
            emitted += 1;
            continue;
        };

        let rest = row.key.strip_prefix(prefix).unwrap_or(&row.key);
        last_consumed = Some(row.key.clone());

        let Some(at) = rest.find(delim) else {
            page.contents.push(row);
            emitted += 1;
            continue;
        };

        let group = format!("{prefix}{}{delim}", &rest[..at]);
        if page.common_prefixes.last() == Some(&group) {
            // Already rolled up; the key is consumed but does not count again.
            continue;
        }
        page.common_prefixes.push(group);
        emitted += 1;
    }

    if page.is_truncated {
        page.next_token = last_consumed.as_deref().map(encode_token);
    }
    page
}

/// Base64 of the resume key.
///
/// S3's token is opaque and clients must not parse it; encoding keeps anyone from depending on its
/// shape, and it survives a key containing characters that would break a bare query parameter.
#[must_use]
pub fn encode_token(key: &str) -> String {
    URL_SAFE_NO_PAD.encode(key.as_bytes())
}

/// # Errors
/// `InvalidArgument` when the token is not the shape this gateway issued — a client that
/// hand-crafts one gets a clear refusal rather than a silently wrong page.
pub fn decode_token(token: &str) -> Result<String, S3Error> {
    let bytes = URL_SAFE_NO_PAD
        .decode(token.as_bytes())
        .map_err(|_| S3Error::InvalidArgument("continuation-token is not valid".to_string()))?;
    String::from_utf8(bytes)
        .map_err(|_| S3Error::InvalidArgument("continuation-token is not valid".to_string()))
}

/// Whether this key's prefix policy allows listing `prefix`.
///
/// A key with no prefixes may list anything. A scoped key may list only a prefix that is at or
/// below one of its own: with `img/` allowed, `img/` and `img/2026/` are fine, `im` and the empty
/// prefix are not — listing `im` would return `img/...` and disclose the folder structure the
/// scope exists to fence off.
///
/// The check is on the requested prefix rather than on the rows that came back. Filtering
/// afterwards means the query already read keys the caller may not see, and one missed filter is a
/// disclosure.
#[must_use]
pub fn may_list(allowed: &[String], prefix: &str) -> bool {
    if allowed.is_empty() {
        return true;
    }
    // One condition, not two. An earlier version also allowed `prefix.starts_with(a)`, which is
    // the same shape as the separator bug P3 fixed: with `img` allowed it let `imgsecret/` through.
    // `prefix_allows` already covers at-or-below, and it enforces the separator.
    allowed
        .iter()
        .any(|a| crate::models::access_keys::prefix_allows(a, prefix))
}

// ---- handlers ----

use axum::{
    body::Body,
    http::{request::Parts, StatusCode},
    response::Response,
};
use loco_rs::prelude::*;

use crate::{
    controllers::s3::fail,
    models::{access_keys, buckets, objects},
    s3::{
        request::{query_pairs, S3Request},
        xml,
    },
};

fn param_of<'a>(query: &'a [(String, String)], name: &str) -> Option<&'a str> {
    query
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

pub async fn list_objects_v2(ctx: &AppContext, parts: &Parts, rid: &str) -> Response {
    match list_objects_v2_inner(ctx, parts).await {
        Ok(body) => xml::ok_xml(body, rid),
        Err(err) => fail(&parts.method, &err, parts.uri.path(), rid),
    }
}

async fn list_objects_v2_inner(ctx: &AppContext, parts: &Parts) -> Result<String, S3Error> {
    let req = S3Request::resolve_bucket_only(ctx, parts, access_keys::ACTION_LIST).await?;
    let query = query_pairs(parts);

    let prefix = param_of(&query, "prefix").unwrap_or_default().to_string();
    let delimiter = param_of(&query, "delimiter").and_then(|d| d.chars().next());
    let url_encode =
        param_of(&query, "encoding-type").is_some_and(|e| e.eq_ignore_ascii_case("url"));
    // Capped rather than refused: S3 answers a too-large max-keys with 1000, and erroring would break clients that always ask for the maximum.
    let max_keys = param_of(&query, "max-keys")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(MAX_KEYS_LIMIT)
        .clamp(1, MAX_KEYS_LIMIT);
    let continuation_token = param_of(&query, "continuation-token").map(str::to_string);
    let start_after = param_of(&query, "start-after").map(str::to_string);

    let allowed = req
        .key
        .prefixes(&ctx.db)
        .await
        .map_err(|_| S3Error::InternalError)?;
    if !may_list(&allowed, &prefix) {
        return Err(S3Error::AccessDenied);
    }

    // The continuation token wins over start-after, which is what S3 does when both arrive.
    let after = match &continuation_token {
        Some(t) => Some(decode_token(t)?),
        None => start_after.clone(),
    };

    let rows = objects::Model::list_page(
        &ctx.db,
        &objects::ListQuery {
            bucket_id: req.bucket.id,
            prefix: prefix.clone(),
            after,
            limit: max_keys,
        },
    )
    .await
    .map_err(|_| S3Error::InternalError)?;

    let page = roll_up(
        rows.iter()
            .map(|o| Row {
                key: o.object_key.clone(),
                size: o.size,
                etag: o.etag.clone(),
                modified: o.updated_at.to_rfc3339(),
            })
            .collect(),
        &prefix,
        delimiter,
        max_keys,
    );

    let view = xml::ListingView {
        bucket: &req.bucket.name,
        prefix: &prefix,
        delimiter,
        max_keys,
        continuation_token: continuation_token.as_deref(),
        start_after: start_after.as_deref(),
        url_encode,
    };
    let contents: Vec<xml::ListingRow<'_>> = page
        .contents
        .iter()
        .map(|r| xml::ListingRow {
            key: &r.key,
            size: r.size,
            etag: &r.etag,
            modified: &r.modified,
        })
        .collect();

    Ok(xml::list_objects_v2(
        &view,
        &contents,
        &page.common_prefixes,
        page.is_truncated,
        page.next_token.as_deref(),
    ))
}

/// `ListBuckets` needs no object action: a key with only `read` still enumerates its buckets, the
/// same way an IAM key with `s3:ListAllMyBuckets` does. Authentication alone is the gate.
pub async fn list_buckets(ctx: &AppContext, parts: &Parts, rid: &str) -> Response {
    match list_buckets_inner(ctx, parts).await {
        Ok(body) => xml::ok_xml(body, rid),
        Err(err) => fail(&parts.method, &err, parts.uri.path(), rid),
    }
}

async fn list_buckets_inner(ctx: &AppContext, parts: &Parts) -> Result<String, S3Error> {
    let key = crate::s3::request::authenticate(ctx, parts).await?;
    let user = crate::models::users::Model::find_by_id(&ctx.db, key.user_id)
        .await
        .map_err(|_| S3Error::InternalError)?;

    let rows = buckets::Model::list_for_user(&ctx.db, user.id)
        .await
        .map_err(|_| S3Error::InternalError)?;
    let listed: Vec<(String, String)> = rows
        .iter()
        .map(|b| (b.name.clone(), b.created_at.to_rfc3339()))
        .collect();

    Ok(xml::list_buckets(
        &user.pid.to_string(),
        &user.name,
        &listed,
    ))
}

/// A scope limits objects, not whether the bucket exists, so a scoped key gets a 200 here.
pub async fn head_bucket(ctx: &AppContext, parts: &Parts, rid: &str) -> Response {
    match S3Request::resolve_bucket_only(ctx, parts, access_keys::ACTION_LIST).await {
        Ok(_) => Response::builder()
            .status(StatusCode::OK)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(keys: &[&str]) -> Vec<Row> {
        keys.iter()
            .map(|k| Row {
                key: (*k).to_string(),
                size: 1,
                etag: "e".to_string(),
                modified: "2026-08-17T00:00:00.000Z".to_string(),
            })
            .collect()
    }

    fn keys_of(page: &Page) -> Vec<&str> {
        page.contents.iter().map(|c| c.key.as_str()).collect()
    }

    #[test]
    fn without_a_delimiter_every_key_is_content() {
        let page = roll_up(rows(&["a/1", "a/2", "b"]), "", None, 10);
        assert_eq!(page.contents.len(), 3);
        assert!(page.common_prefixes.is_empty());
        assert!(!page.is_truncated);
    }

    #[test]
    fn a_delimiter_rolls_up_and_dedups() {
        let page = roll_up(rows(&["a/1", "a/2", "b/1", "top"]), "", Some('/'), 10);

        assert_eq!(page.common_prefixes, vec!["a/", "b/"]);
        assert_eq!(
            keys_of(&page),
            vec!["top"],
            "keys rolled into a common prefix must not also appear in Contents"
        );
    }

    /// The delimiter is searched after the prefix, not from the start of the key — otherwise listing prefix `img/` with delimiter `/` rolls everything into one entry `img/`.
    #[test]
    fn the_delimiter_is_searched_after_the_prefix() {
        let page = roll_up(
            rows(&[
                "img/2025/c.png",
                "img/2026/a.png",
                "img/2026/b.png",
                "img/top.png",
            ]),
            "img/",
            Some('/'),
            10,
        );

        assert_eq!(page.common_prefixes, vec!["img/2025/", "img/2026/"]);
        assert_eq!(keys_of(&page), vec!["img/top.png"]);
    }

    #[test]
    fn common_prefixes_count_towards_the_limit() {
        let page = roll_up(rows(&["a/1", "b/1", "c/1", "d/1"]), "", Some('/'), 2);

        assert_eq!(page.common_prefixes.len(), 2);
        assert!(page.is_truncated);
        assert!(page.next_token.is_some());
    }

    /// The token must resume where the page stopped, including mid-roll-up.
    #[test]
    fn the_token_resumes_after_the_last_key_consumed() {
        let all = rows(&["a/1", "a/2", "b/1", "c/1"]);
        let first = roll_up(all, "", Some('/'), 2);

        assert_eq!(first.common_prefixes, vec!["a/", "b/"]);
        assert!(first.is_truncated);

        // b/1, not a/2: resuming after a/2 would emit b/ a second time and the client would see the folder twice.
        let token = decode_token(first.next_token.as_ref().unwrap()).unwrap();
        assert_eq!(token, "b/1");
    }

    #[test]
    fn a_token_round_trips_and_a_tampered_one_is_rejected() {
        let t = encode_token("img/a.png");
        assert_ne!(t, "img/a.png", "the token must look opaque");
        assert_eq!(decode_token(&t).unwrap(), "img/a.png");
        assert!(decode_token("!!!not base64!!!").is_err());
    }

    #[test]
    fn an_empty_result_has_nothing_and_no_token() {
        let page = roll_up(vec![], "", None, 10);
        assert!(page.contents.is_empty());
        assert!(page.common_prefixes.is_empty());
        assert!(!page.is_truncated);
        assert!(page.next_token.is_none());
    }

    /// A key exactly equal to the prefix is content, not a common prefix — S3 emits it in Contents, and dropping it loses an object from every listing.
    #[test]
    fn a_key_equal_to_the_prefix_stays_in_contents() {
        let page = roll_up(rows(&["img/", "img/a.png"]), "img/", Some('/'), 10);
        assert!(page.contents.iter().any(|c| c.key == "img/"));
    }

    #[test]
    fn may_list_allows_everything_for_an_unscoped_key() {
        assert!(may_list(&[], ""));
        assert!(may_list(&[], "anything/"));
    }

    /// Both directions of the scope rule, because `may_list` is an `or` of two conditions and that is exactly the shape that goes wrong by being too wide.
    #[test]
    fn may_list_allows_at_or_below_the_scope_and_nothing_above() {
        let allowed = vec!["img/".to_string()];

        assert!(may_list(&allowed, "img/"));
        assert!(may_list(&allowed, "img/2026/"));
        assert!(may_list(&allowed, "img/2026/a.png"));

        assert!(
            !may_list(&allowed, ""),
            "the bucket root is above the scope"
        );
        assert!(
            !may_list(&allowed, "im"),
            "a parent prefix discloses structure"
        );
        assert!(!may_list(&allowed, "docs/"));
        assert!(
            !may_list(&allowed, "imgsecret/"),
            "the separator rule applies here too"
        );
    }

    /// A scope without a trailing slash still fences on the separator.
    #[test]
    fn may_list_respects_the_separator_for_a_scope_without_a_slash() {
        let allowed = vec!["img".to_string()];
        assert!(may_list(&allowed, "img"));
        assert!(may_list(&allowed, "img/2026/"));
        assert!(!may_list(&allowed, "imgsecret/"));
    }
}
