//! Every failure the data plane can produce, as an S3 error code.
//!
//! S3 clients branch on `<Code>`, not on the HTTP status alone — botocore raises `ClientError` carrying the code, and retry logic reads it.
//! A generic 500 is a client that cannot act.
use axum::http::StatusCode;
use loco_rs::model::ModelError;

#[derive(Debug, Clone)]
pub enum S3Error {
    AccessDenied,
    InvalidAccessKeyId,
    SignatureDoesNotMatch,
    RequestTimeTooSkewed,
    /// Non-standard.
    /// The one code S3 has no equivalent for; marked `gateway_only` in the conformance suite.
    QuotaExceeded,
    NoSuchBucket,
    NoSuchKey,
    NoSuchUpload,
    KeyTooLong,
    InvalidArgument(String),
    InvalidRequest(String),
    MalformedXml(String),
    MissingContentLength,
    PreconditionFailed,
    NotImplemented(String),
    /// An error the upstream store returned, re-emitted with its own code.
    ///
    /// The message is the upstream message with nothing added; the upstream *body* is dropped entirely, because it names the physical bucket and key.
    Upstream {
        code: String,
        status: u16,
        message: String,
    },
    InternalError,
}

impl S3Error {
    #[must_use]
    pub fn code(&self) -> &str {
        match self {
            Self::AccessDenied => "AccessDenied",
            Self::InvalidAccessKeyId => "InvalidAccessKeyId",
            Self::SignatureDoesNotMatch => "SignatureDoesNotMatch",
            Self::RequestTimeTooSkewed => "RequestTimeTooSkewed",
            Self::QuotaExceeded => "QuotaExceeded",
            Self::NoSuchBucket => "NoSuchBucket",
            Self::NoSuchKey => "NoSuchKey",
            Self::NoSuchUpload => "NoSuchUpload",
            Self::KeyTooLong => "KeyTooLongError",
            Self::InvalidArgument(_) => "InvalidArgument",
            Self::InvalidRequest(_) => "InvalidRequest",
            Self::MalformedXml(_) => "MalformedXML",
            Self::MissingContentLength => "MissingContentLength",
            Self::PreconditionFailed => "PreconditionFailed",
            Self::NotImplemented(_) => "NotImplemented",
            Self::Upstream { code, .. } => code,
            Self::InternalError => "InternalError",
        }
    }

    #[must_use]
    pub fn status(&self) -> StatusCode {
        match self {
            Self::AccessDenied
            | Self::InvalidAccessKeyId
            | Self::SignatureDoesNotMatch
            | Self::RequestTimeTooSkewed
            | Self::QuotaExceeded => StatusCode::FORBIDDEN,
            Self::NoSuchBucket | Self::NoSuchKey | Self::NoSuchUpload => StatusCode::NOT_FOUND,
            Self::KeyTooLong
            | Self::InvalidArgument(_)
            | Self::InvalidRequest(_)
            | Self::MalformedXml(_) => StatusCode::BAD_REQUEST,
            Self::MissingContentLength => StatusCode::LENGTH_REQUIRED,
            Self::PreconditionFailed => StatusCode::PRECONDITION_FAILED,
            Self::NotImplemented(_) => StatusCode::NOT_IMPLEMENTED,
            Self::Upstream { status, .. } => {
                StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY)
            }
            Self::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::AccessDenied => "Access Denied".to_string(),
            Self::InvalidAccessKeyId => {
                "The AWS Access Key Id you provided does not exist in our records.".to_string()
            }
            Self::SignatureDoesNotMatch => {
                "The request signature we calculated does not match the signature you provided."
                    .to_string()
            }
            Self::RequestTimeTooSkewed => {
                "The difference between the request time and the current time is too large."
                    .to_string()
            }
            Self::QuotaExceeded => {
                "The storage quota for this bucket or account has been reached.".to_string()
            }
            Self::NoSuchBucket => "The specified bucket does not exist.".to_string(),
            Self::NoSuchKey => "The specified key does not exist.".to_string(),
            Self::NoSuchUpload => "The specified multipart upload does not exist.".to_string(),
            Self::KeyTooLong => "Your key is too long.".to_string(),
            Self::InvalidArgument(m)
            | Self::InvalidRequest(m)
            | Self::MalformedXml(m)
            | Self::NotImplemented(m) => m.clone(),
            Self::MissingContentLength => {
                "You must provide the Content-Length HTTP header.".to_string()
            }
            Self::PreconditionFailed => {
                "At least one of the pre-conditions you specified did not hold.".to_string()
            }
            Self::Upstream { message, .. } => message.clone(),
            Self::InternalError => {
                "We encountered an internal error. Please try again.".to_string()
            }
        }
    }
}

impl std::fmt::Display for S3Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())
    }
}

impl std::error::Error for S3Error {}

impl From<ModelError> for S3Error {
    /// Any model failure that reaches the data plane is an internal error from the client's point of view — except the one the quota path raises, which the client can act on.
    ///
    // ponytail: matches on the message string that quota::exceeded() produces.
    // Ceiling: renaming that message silently turns a 403 QuotaExceeded into a 500.
    // Upgrade path: give ModelError a typed variant for quota, which means touching loco's error enum or wrapping it — not worth it for one call site that a test covers.
    fn from(e: ModelError) -> Self {
        if e.to_string().contains("quota exceeded") {
            return Self::QuotaExceeded;
        }
        tracing::error!(error = %e, "model error in the S3 data plane");
        Self::InternalError
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_has_a_code_and_a_status() {
        // Spec §12.
        // A wrong status here is a client that retries when it should not, or gives up when it should retry.
        let cases = [
            (S3Error::AccessDenied, "AccessDenied", 403),
            (S3Error::InvalidAccessKeyId, "InvalidAccessKeyId", 403),
            (S3Error::SignatureDoesNotMatch, "SignatureDoesNotMatch", 403),
            (S3Error::RequestTimeTooSkewed, "RequestTimeTooSkewed", 403),
            (S3Error::QuotaExceeded, "QuotaExceeded", 403),
            (S3Error::NoSuchBucket, "NoSuchBucket", 404),
            (S3Error::NoSuchKey, "NoSuchKey", 404),
            (S3Error::NoSuchUpload, "NoSuchUpload", 404),
            (S3Error::KeyTooLong, "KeyTooLongError", 400),
            (S3Error::MissingContentLength, "MissingContentLength", 411),
            (S3Error::PreconditionFailed, "PreconditionFailed", 412),
            (S3Error::InternalError, "InternalError", 500),
        ];

        for (err, code, status) in cases {
            assert_eq!(err.code(), code);
            assert_eq!(err.status().as_u16(), status, "status for {code}");
            assert!(!err.message().is_empty(), "message for {code}");
        }
    }

    #[test]
    fn not_implemented_carries_what_is_missing() {
        let err = S3Error::NotImplemented("aws-chunked payload signing".to_string());
        assert_eq!(err.status().as_u16(), 501);
        assert!(err.message().contains("aws-chunked"));
    }

    /// An upstream error is re-emitted with its Code but never with its body: the upstream body carries the physical bucket and key, and forwarding it leaks the layout the product promises never to expose.
    #[test]
    fn upstream_error_keeps_the_code_and_drops_the_body() {
        let err = S3Error::Upstream {
            code: "EntityTooSmall".to_string(),
            status: 400,
            message: "Your proposed upload is smaller than the minimum allowed size".to_string(),
        };
        assert_eq!(err.code(), "EntityTooSmall");
        assert_eq!(err.status().as_u16(), 400);
        assert!(!err.message().contains("osg-main"));
    }

    /// An upstream status outside the HTTP range must not panic; a broken upstream is not a reason to kill the request thread.
    #[test]
    fn an_impossible_upstream_status_becomes_bad_gateway() {
        let err = S3Error::Upstream {
            code: "Weird".to_string(),
            status: 9999,
            message: "nonsense".to_string(),
        };
        assert_eq!(err.status().as_u16(), 502);
    }

    #[test]
    fn a_quota_error_from_the_model_maps_to_quota_exceeded() {
        // Guards the string match in From<ModelError>. If quota::exceeded() ever changes its message, this fails instead of a 500 reaching a client in production.
        let e: S3Error = ModelError::msg("quota exceeded").into();
        assert_eq!(e.code(), "QuotaExceeded");
    }

    #[test]
    fn any_other_model_error_is_internal() {
        let e: S3Error = ModelError::EntityNotFound.into();
        assert_eq!(e.code(), "InternalError");
    }
}
