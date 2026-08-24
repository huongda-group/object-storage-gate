//! S3 wire format.
//!
//! Clients parse these bodies with a real XML parser, so an unescaped `&` in a key does not produce a slightly-off error — it produces a parse failure that surfaces as something else entirely.
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
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <Error><Code>{}</Code><Message>{}</Message>\
         <Resource>{}</Resource><RequestId>{}</RequestId></Error>",
        escape(err.code()),
        escape(&err.message()),
        escape(resource),
        escape(request_id)
    )
}

/// An error response for a verb that carries a body.
#[must_use]
pub fn error_response(err: &S3Error, resource: &str, request_id: &str) -> Response {
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

#[cfg(test)]
mod tests {
    use super::*;

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
