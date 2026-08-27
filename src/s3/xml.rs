//! S3 wire format.
//!
//! Clients parse these bodies with a real XML parser, so an unescaped `&` in a key does not produce a slightly-off error — it produces a parse failure that surfaces as something else entirely.
use std::fmt::Write as _;

use axum::{
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};

use super::error::S3Error;

/// Escapes the five XML metacharacters.
#[must_use]
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// Renders an S3 error body.
///
/// `Resource` is the logical path the client asked for, never the physical one — the whole point of the gateway is that a client never learns the physical layout, and an error body is the easiest place to leak it.
#[must_use]
pub fn error_body(err: &S3Error, resource: &str, request_id: &str) -> String {
    // S3 names the bucket and key separately as well as the resource, and clients read those:
    // botocore surfaces them on the exception, and a suite that asserts on <Key> sees nothing
    // without them. Both halves are the logical ones the client asked for.
    let trimmed = resource.trim_start_matches('/');
    let (bucket, key) = trimmed.split_once('/').unwrap_or((trimmed, ""));

    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <Error><Code>{}</Code><Message>{}</Message>",
        escape(err.code()),
        escape(&err.message())
    );
    if !bucket.is_empty() {
        let _ = write!(out, "<BucketName>{}</BucketName>", escape(bucket));
    }
    if !key.is_empty() {
        let _ = write!(out, "<Key>{}</Key>", escape(key));
    }
    let _ = write!(
        out,
        "<Resource>{}</Resource><RequestId>{}</RequestId></Error>",
        escape(resource),
        escape(request_id)
    );
    out
}

/// The S3 error code, carried in a response extension so the audit layer can read it.
///
/// An extension rather than a header: it never reaches the wire, and the audit needs a code the
/// status alone cannot supply — the same 403 covers a wrong signature, a missing permission and a
/// full bucket.
#[derive(Debug, Clone)]
pub struct ErrorCode(pub String);

/// An error response for a verb that carries a body.
#[must_use]
pub fn error_response(err: &S3Error, resource: &str, request_id: &str) -> Response {
    let mut res = error_response_inner(err, resource, request_id);
    res.extensions_mut()
        .insert(ErrorCode(err.code().to_string()));
    res
}

fn error_response_inner(err: &S3Error, resource: &str, request_id: &str) -> Response {
    (
        err.status(),
        [
            (header::CONTENT_TYPE, "application/xml".to_string()),
            (
                header::HeaderName::from_static("x-amz-request-id"),
                request_id.to_string(),
            ),
        ],
        error_body(err, resource, request_id),
    )
        .into_response()
}

/// An error response for HEAD, which must carry no body at all.
///
/// botocore reads Content-Length on a HEAD and a body here makes it mis-parse or hang, so the method decides this — `error_response` is never asked to guess.
#[must_use]
pub fn error_response_headless(err: &S3Error, request_id: &str) -> Response {
    let mut res = error_response_headless_inner(err, request_id);
    res.extensions_mut()
        .insert(ErrorCode(err.code().to_string()));
    res
}

fn error_response_headless_inner(err: &S3Error, request_id: &str) -> Response {
    (
        err.status(),
        [(
            header::HeaderName::from_static("x-amz-request-id"),
            request_id.to_string(),
        )],
        (),
    )
        .into_response()
}

/// A 200 with an XML body.
#[must_use]
pub fn ok_xml(body: String, request_id: &str) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/xml".to_string()),
            (
                header::HeaderName::from_static("x-amz-request-id"),
                request_id.to_string(),
            ),
        ],
        body,
    )
        .into_response()
}

/// S3 caps a batch delete at 1000 keys.
pub const MAX_DELETE_KEYS: usize = 1000;

/// Parses a `DeleteObjects` request body into its keys and its Quiet flag.
///
/// Hand-rolled rather than given to a full XML deserialiser: the shape is two element names deep and fixed, and the failure mode that matters is a body claiming more keys than S3 allows — which a parser would happily accept.
///
/// # Errors
/// `MalformedXML` for a body that is not a `<Delete>` document, has no keys, or carries more than 1000 of them.
pub fn parse_delete_request(body: &[u8]) -> Result<(Vec<String>, bool), S3Error> {
    let text = std::str::from_utf8(body)
        .map_err(|_| S3Error::MalformedXml("request body is not valid UTF-8".to_string()))?;
    if !text.contains("<Delete") {
        return Err(S3Error::MalformedXml(
            "expected a <Delete> document".to_string(),
        ));
    }

    let quiet = extract(text, "Quiet")
        .first()
        .is_some_and(|v| v.eq_ignore_ascii_case("true"));
    let keys = extract(text, "Key");

    if keys.is_empty() {
        return Err(S3Error::MalformedXml(
            "a delete request must name at least one key".to_string(),
        ));
    }
    if keys.len() > MAX_DELETE_KEYS {
        return Err(S3Error::MalformedXml(format!(
            "a delete request may name at most {MAX_DELETE_KEYS} keys"
        )));
    }
    Ok((keys, quiet))
}

/// Every text value of `<tag>` in document order.
fn extract(text: &str, tag: &str) -> Vec<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(&open) {
        rest = &rest[start + open.len()..];
        let Some(end) = rest.find(&close) else { break };
        out.push(unescape(rest[..end].trim()));
        rest = &rest[end + close.len()..];
    }
    out
}

/// Reverses `escape`, and also decodes the numeric entities other servers emit.
///
/// `MinIO` writes a quoted `ETag` as `&#34;abc&#34;` rather than `&quot;abc&quot;`. Leaving those
/// undecoded stores the entity text as part of the `ETag`, and the value the gateway hands back is
/// then escaped again — so a client sees a literal `&#34;` where a quote belongs.
#[must_use]
pub fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&#60;", "<")
        .replace("&gt;", ">")
        .replace("&#62;", ">")
        .replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        // Last, so `&amp;lt;` does not become `<`.
        .replace("&amp;", "&")
        .replace("&#38;", "&")
}

/// Renders a `DeleteResult`.
///
/// Quiet mode omits the `<Deleted>` entries and keeps the errors, which is the only difference S3 defines.
#[must_use]
pub fn delete_result(deleted: &[String], errors: &[(String, S3Error)], quiet: bool) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<DeleteResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">",
    );
    if !quiet {
        for key in deleted {
            let _ = write!(out, "<Deleted><Key>{}</Key></Deleted>", escape(key));
        }
    }
    for (key, err) in errors {
        let _ = write!(
            out,
            "<Error><Key>{}</Key><Code>{}</Code><Message>{}</Message></Error>",
            escape(key),
            escape(err.code()),
            escape(&err.message())
        );
    }
    out.push_str("</DeleteResult>");
    out
}

/// Everything a `ListBucketResult` needs, so the renderer takes one argument rather than ten.
pub struct ListingView<'a> {
    pub bucket: &'a str,
    pub prefix: &'a str,
    pub delimiter: Option<char>,
    pub max_keys: u64,
    pub continuation_token: Option<&'a str>,
    pub start_after: Option<&'a str>,
    pub url_encode: bool,
}

/// One object as `ListObjectsV2` reports it.
pub struct ListingRow<'a> {
    pub key: &'a str,
    pub size: i64,
    pub etag: &'a str,
    pub modified: &'a str,
}

/// URL-encodes a key when the client asked for `encoding-type=url`.
///
/// botocore sends it when a key can contain characters that XML cannot carry safely; without it a
/// key holding a control character produces a document the client cannot parse.
fn maybe_encode(value: &str, url_encode: bool) -> String {
    if url_encode {
        percent_encoding::utf8_percent_encode(value, LISTING_ENCODE).to_string()
    } else {
        escape(value)
    }
}

/// The same unreserved set `SigV4` uses, plus `/`: a listing keeps path separators readable.
const LISTING_ENCODE: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~')
    .remove(b'/');

/// Renders `ListBucketResult` in the tag order S3 uses.
///
/// botocore parses by name, but some older clients do not, and matching the order costs nothing.
/// `StorageClass` is always `STANDARD`: the gateway does not model storage classes, and omitting
/// the tag makes some clients error.
#[must_use]
pub fn list_objects_v2(
    view: &ListingView<'_>,
    rows: &[ListingRow<'_>],
    common_prefixes: &[String],
    is_truncated: bool,
    next_token: Option<&str>,
) -> String {
    let key_count = rows.len() + common_prefixes.len();
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ListBucketResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">",
    );
    let _ = write!(out, "<Name>{}</Name>", escape(view.bucket));
    let _ = write!(
        out,
        "<Prefix>{}</Prefix>",
        maybe_encode(view.prefix, view.url_encode)
    );
    let _ = write!(out, "<KeyCount>{key_count}</KeyCount>");
    let _ = write!(out, "<MaxKeys>{}</MaxKeys>", view.max_keys);
    if let Some(d) = view.delimiter {
        let _ = write!(
            out,
            "<Delimiter>{}</Delimiter>",
            maybe_encode(&d.to_string(), view.url_encode)
        );
    }
    let _ = write!(out, "<IsTruncated>{is_truncated}</IsTruncated>");
    if let Some(t) = view.continuation_token {
        let _ = write!(out, "<ContinuationToken>{}</ContinuationToken>", escape(t));
    }
    if let Some(t) = next_token {
        let _ = write!(
            out,
            "<NextContinuationToken>{}</NextContinuationToken>",
            escape(t)
        );
    }
    if let Some(s) = view.start_after {
        let _ = write!(
            out,
            "<StartAfter>{}</StartAfter>",
            maybe_encode(s, view.url_encode)
        );
    }
    if view.url_encode {
        out.push_str("<EncodingType>url</EncodingType>");
    }
    for row in rows {
        let _ = write!(
            out,
            "<Contents><Key>{}</Key><LastModified>{}</LastModified><ETag>{}</ETag><Size>{}</Size><StorageClass>STANDARD</StorageClass></Contents>",
            maybe_encode(row.key, view.url_encode),
            escape(row.modified),
            escape(row.etag),
            row.size
        );
    }
    for p in common_prefixes {
        let _ = write!(
            out,
            "<CommonPrefixes><Prefix>{}</Prefix></CommonPrefixes>",
            maybe_encode(p, view.url_encode)
        );
    }
    out.push_str("</ListBucketResult>");
    out
}

/// Renders `ListAllMyBucketsResult`.
///
/// `Owner.DisplayName` is the account name, never its email: any access key of the account can
/// read this response, including one handed to a third party, and the email does not belong in it.
#[must_use]
pub fn list_buckets(owner_id: &str, owner_name: &str, buckets: &[(String, String)]) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ListAllMyBucketsResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">",
    );
    let _ = write!(
        out,
        "<Owner><ID>{}</ID><DisplayName>{}</DisplayName></Owner><Buckets>",
        escape(owner_id),
        escape(owner_name)
    );
    for (name, created) in buckets {
        let _ = write!(
            out,
            "<Bucket><Name>{}</Name><CreationDate>{}</CreationDate></Bucket>",
            escape(name),
            escape(created)
        );
    }
    out.push_str("</Buckets></ListAllMyBucketsResult>");
    out
}

/// Renders `InitiateMultipartUploadResult`.
#[must_use]
pub fn initiate_multipart(bucket: &str, key: &str, upload_id: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <InitiateMultipartUploadResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
         <Bucket>{}</Bucket><Key>{}</Key><UploadId>{}</UploadId>\
         </InitiateMultipartUploadResult>",
        escape(bucket),
        escape(key),
        escape(upload_id)
    )
}

/// Renders `CompleteMultipartUploadResult`.
///
/// `Location` names the logical bucket and key only. A URL carrying the physical bucket would hand the client the layout the gateway exists to hide.
#[must_use]
pub fn complete_multipart(bucket: &str, key: &str, etag: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <CompleteMultipartUploadResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
         <Location>/{}/{}</Location><Bucket>{}</Bucket><Key>{}</Key><ETag>{}</ETag>\
         </CompleteMultipartUploadResult>",
        escape(bucket),
        escape(key),
        escape(bucket),
        escape(key),
        escape(etag)
    )
}

/// Parses the part list a client sends with `CompleteMultipartUpload`.
///
/// # Errors
/// `MalformedXML` when the body is not a `<CompleteMultipartUpload>` document or names no parts.
pub fn parse_complete_request(body: &[u8]) -> Result<Vec<(u32, String)>, S3Error> {
    let text = std::str::from_utf8(body)
        .map_err(|_| S3Error::MalformedXml("request body is not valid UTF-8".to_string()))?;
    let numbers = extract(text, "PartNumber");
    let etags = extract(text, "ETag");
    if numbers.is_empty() || numbers.len() != etags.len() {
        return Err(S3Error::MalformedXml(
            "expected a <CompleteMultipartUpload> document listing PartNumber and ETag pairs"
                .to_string(),
        ));
    }
    numbers
        .into_iter()
        .zip(etags)
        .map(|(n, e)| {
            n.parse::<u32>()
                .map(|n| (n, e))
                .map_err(|_| S3Error::MalformedXml("PartNumber must be a number".to_string()))
        })
        .collect()
}

/// Renders the body the gateway sends upstream to complete an upload.
#[must_use]
pub fn complete_request(parts: &[(u32, String)]) -> String {
    let mut out = String::from("<CompleteMultipartUpload>");
    for (n, etag) in parts {
        let _ = write!(
            out,
            "<Part><PartNumber>{n}</PartNumber><ETag>{}</ETag></Part>",
            escape(etag)
        );
    }
    out.push_str("</CompleteMultipartUpload>");
    out
}

/// Renders `CopyObjectResult`, which is also the shape `UploadPartCopy` answers with under a different root.
#[must_use]
pub fn copy_result(root: &str, etag: &str, modified: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <{root} xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
         <LastModified>{}</LastModified><ETag>{}</ETag></{root}>",
        escape(modified),
        escape(etag)
    )
}

/// Renders `ListPartsResult`.
#[must_use]
pub fn list_parts(
    bucket: &str,
    key: &str,
    upload_id: &str,
    parts: &[(u32, String, i64)],
) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ListPartsResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">",
    );
    let _ = write!(
        out,
        "<Bucket>{}</Bucket><Key>{}</Key><UploadId>{}</UploadId><IsTruncated>false</IsTruncated>",
        escape(bucket),
        escape(key),
        escape(upload_id)
    );
    for (n, etag, size) in parts {
        let _ = write!(
            out,
            "<Part><PartNumber>{n}</PartNumber><ETag>{}</ETag><Size>{size}</Size></Part>",
            escape(etag)
        );
    }
    out.push_str("</ListPartsResult>");
    out
}

/// Renders `ListMultipartUploadsResult`.
#[must_use]
pub fn list_multipart_uploads(bucket: &str, uploads: &[(String, String, String)]) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ListMultipartUploadsResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">",
    );
    let _ = write!(
        out,
        "<Bucket>{}</Bucket><IsTruncated>false</IsTruncated>",
        escape(bucket)
    );
    for (key, upload_id, initiated) in uploads {
        let _ = write!(
            out,
            "<Upload><Key>{}</Key><UploadId>{}</UploadId><Initiated>{}</Initiated></Upload>",
            escape(key),
            escape(upload_id),
            escape(initiated)
        );
    }
    out.push_str("</ListMultipartUploadsResult>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_delete_request_parses_its_keys_and_quiet_flag() {
        let (keys, quiet) = parse_delete_request(
            b"<Delete><Object><Key>a.bin</Key></Object><Object><Key>b.bin</Key></Object></Delete>",
        )
        .unwrap();
        assert_eq!(keys, vec!["a.bin".to_string(), "b.bin".to_string()]);
        assert!(!quiet);

        let (_, quiet) = parse_delete_request(
            b"<Delete><Quiet>true</Quiet><Object><Key>a.bin</Key></Object></Delete>",
        )
        .unwrap();
        assert!(quiet);
    }

    /// A key containing `&` arrives escaped and must come back out as the key the client meant.
    #[test]
    fn a_delete_key_round_trips_through_escaping() {
        let (keys, _) =
            parse_delete_request(b"<Delete><Object><Key>a&amp;b&lt;c.bin</Key></Object></Delete>")
                .unwrap();
        assert_eq!(keys, vec!["a&b<c.bin".to_string()]);

        let body = delete_result(&keys, &[], false);
        assert!(body.contains("<Key>a&amp;b&lt;c.bin</Key>"), "{body}");
    }

    #[test]
    fn a_delete_request_is_bounded_and_must_name_something() {
        let mut body = String::from("<Delete>");
        for i in 0..=MAX_DELETE_KEYS {
            let _ = write!(body, "<Object><Key>k{i}</Key></Object>");
        }
        body.push_str("</Delete>");
        assert!(parse_delete_request(body.as_bytes()).is_err());

        assert!(parse_delete_request(b"<Delete></Delete>").is_err());
        assert!(parse_delete_request(b"not xml at all").is_err());
    }

    #[test]
    fn quiet_mode_keeps_the_errors_and_drops_the_deleted_list() {
        let deleted = vec!["a.bin".to_string()];
        let errors = vec![("b.bin".to_string(), S3Error::AccessDenied)];

        let loud = delete_result(&deleted, &errors, false);
        assert!(loud.contains("<Deleted><Key>a.bin</Key></Deleted>"));
        assert!(loud.contains("<Code>AccessDenied</Code>"));

        let quiet = delete_result(&deleted, &errors, true);
        assert!(!quiet.contains("<Deleted>"));
        assert!(quiet.contains("<Code>AccessDenied</Code>"));
    }

    #[test]
    fn a_complete_request_parses_its_part_list() {
        let parts = parse_complete_request(
            br"<CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>&quot;a&quot;</ETag></Part><Part><PartNumber>2</PartNumber><ETag>&quot;b&quot;</ETag></Part></CompleteMultipartUpload>",
        )
        .unwrap();
        assert_eq!(
            parts,
            vec![(1, "\"a\"".to_string()), (2, "\"b\"".to_string())]
        );

        assert!(parse_complete_request(b"<CompleteMultipartUpload/>").is_err());
        assert!(parse_complete_request(b"nonsense").is_err());
    }

    /// The Location a client gets must name the logical bucket, never the physical one.
    #[test]
    fn a_complete_result_never_names_the_physical_bucket() {
        let body = complete_multipart("media-cdn", "img/a.png", "\"e\"");
        assert!(
            body.contains("<Location>/media-cdn/img/a.png</Location>"),
            "{body}"
        );
        assert!(!body.contains("osg-main"));
    }

    #[test]
    fn metacharacters_are_escaped() {
        assert_eq!(escape("a&b<c>d\"e'f"), "a&amp;b&lt;c&gt;d&quot;e&apos;f");
    }

    /// A key containing `&` is legal in S3 and would otherwise produce XML no client can parse.
    #[test]
    fn a_key_with_metacharacters_still_produces_parseable_xml() {
        let body = error_body(&S3Error::NoSuchKey, "/bkt/a&b<c>.png", "req-1");
        assert!(body.contains("<Resource>/bkt/a&amp;b&lt;c&gt;.png</Resource>"));
        assert!(!body.contains("a&b"));
    }

    #[test]
    fn an_error_body_carries_every_field_a_client_reads() {
        let body = error_body(&S3Error::AccessDenied, "/media-cdn/a.png", "req-9");
        assert!(body.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(body.contains("<Code>AccessDenied</Code>"));
        assert!(body.contains("<Message>Access Denied</Message>"));
        assert!(body.contains("<Resource>/media-cdn/a.png</Resource>"));
        assert!(body.contains("<RequestId>req-9</RequestId>"));
    }
}
