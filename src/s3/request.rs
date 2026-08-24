//! The isolation boundary.
//!
//! `resolve` is the only constructor, and every function that talks to a store takes an `&S3Request` — so there is no physical key and no pool credential without having gone through authorisation and rewrite.
//! That is a property of the types, not a step someone has to remember.
use axum::http::request::Parts;
use loco_rs::prelude::*;

use crate::{
    models::{access_keys, buckets, pools, users},
    s3::{error::S3Error, sigv4},
};

/// Longest object key S3 accepts, in bytes.
pub const MAX_KEY_LEN: usize = 1024;

pub struct S3Request {
    pub key: access_keys::Model,
    pub user: users::Model,
    pub bucket: buckets::Model,
    pub pool: pools::Model,
    /// What the client asked for.
    pub logical_key: String,
    /// `{user_pid}/{bucket_name}/{logical_key}`, inside `pool.physical_bucket`.
    pub physical_key: String,
}

/// A second object addressed by the same request, as `x-amz-copy-source` does.
pub struct PhysicalRef {
    pub bucket: buckets::Model,
    pub logical_key: String,
    pub physical_key: String,
}

/// Splits `/bucket/some/key` into its two halves.
///
/// The key half stays percent-encoded here and is decoded by the caller, because the signature was computed over the encoded form.
fn split_path(path: &str) -> (String, String) {
    let trimmed = path.trim_start_matches('/');
    match trimmed.split_once('/') {
        Some((bucket, key)) => (bucket.to_string(), key.to_string()),
        None => (trimmed.to_string(), String::new()),
    }
}

#[must_use]
pub fn decode_key(encoded: &str) -> String {
    percent_encoding::percent_decode_str(encoded)
        .decode_utf8_lossy()
        .to_string()
}

/// Rejects a key that cannot safely become a path segment.
///
/// `..` is refused rather than normalised: normalising would turn `img/../../escape` into a key the prefix check already approved under a different name.
///
/// # Errors
/// `KeyTooLong` past 1024 bytes; `InvalidArgument` for a leading slash or a `..` segment.
pub fn validate_logical_key(key: &str) -> Result<(), S3Error> {
    if key.len() > MAX_KEY_LEN {
        return Err(S3Error::KeyTooLong);
    }
    if key.starts_with('/') {
        return Err(S3Error::InvalidArgument(
            "Object key must not start with a slash.".to_string(),
        ));
    }
    if key.split('/').any(|seg| seg == "..") {
        return Err(S3Error::InvalidArgument(
            "Object key must not contain a '..' path segment.".to_string(),
        ));
    }
    Ok(())
}

/// The `x-amz-content-sha256` a client sent, or the empty-body hash when it sent none.
fn payload_hash_of(parts: &Parts) -> Result<String, S3Error> {
    let Some(raw) = parts.headers.get("x-amz-content-sha256") else {
        return Ok(sigv4::EMPTY_PAYLOAD_SHA256.to_string());
    };
    let value = raw.to_str().map_err(|_| S3Error::AccessDenied)?;
    if value.starts_with("STREAMING-") {
        // aws-chunked framing rewrites the body, so the gateway would have to de-frame it before it could proxy anything. Refusing is honest; mis-parsing the frames and storing them as object bytes is not.
        return Err(S3Error::NotImplemented(
            "aws-chunked payload signing (STREAMING-AWS4-HMAC-SHA256-PAYLOAD) is not supported by this gateway".to_string(),
        ));
    }
    Ok(value.to_string())
}

/// The query string as `(name, value)` pairs, decoded.
#[must_use]
pub fn query_pairs(parts: &Parts) -> Vec<(String, String)> {
    parts.uri.query().map_or_else(Vec::new, |q| {
        q.split('&')
            .filter(|s| !s.is_empty())
            .map(|kv| {
                let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
                (decode_key(k), decode_key(v))
            })
            .collect()
    })
}

/// Verifies the request signature and returns the key that signed it.
///
/// # Errors
/// `AccessDenied` when nothing was presented, `InvalidAccessKeyId` when the key is unknown or no longer usable, `SignatureDoesNotMatch` or `RequestTimeTooSkewed` from the verifier.
pub async fn authenticate(ctx: &AppContext, parts: &Parts) -> Result<access_keys::Model, S3Error> {
    let query = query_pairs(parts);
    let presigned = query
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("X-Amz-Signature"));

    let presented = if presigned {
        sigv4::parse_query(&query)?
    } else {
        sigv4::parse_authorization(&parts.headers)?
    };

    let key = access_keys::Model::find_by_access_key_id(&ctx.db, &presented.access_key_id)
        .await
        .map_err(|_| S3Error::InvalidAccessKeyId)?;
    let secret = key.decrypt_secret().map_err(|e| {
        tracing::error!(error = %e, "access key secret could not be decrypted");
        S3Error::InternalError
    })?;

    // Only the headers the client said it signed take part, in the values it sent them with.
    let mut headers: Vec<(String, String)> = Vec::new();
    for name in &presented.signed_headers {
        for value in parts.headers.get_all(name.as_str()) {
            headers.push((
                name.to_ascii_lowercase(),
                value.to_str().unwrap_or_default().to_string(),
            ));
        }
    }

    let canonical = sigv4::CanonicalParts {
        method: parts.method.to_string(),
        // The path exactly as it arrived: the client signed that string, and decoding then re-encoding it can only introduce a mismatch.
        uri: parts.uri.path().to_string(),
        query: if presigned {
            query
                .iter()
                .filter(|(k, _)| !k.eq_ignore_ascii_case("X-Amz-Signature"))
                .cloned()
                .collect()
        } else {
            query
        },
        headers,
        signed_headers: presented.signed_headers.clone(),
        payload_hash: if presigned {
            sigv4::UNSIGNED_PAYLOAD.to_string()
        } else {
            payload_hash_of(parts)?
        },
        uri_already_encoded: true,
        normalise_path: false,
    };

    let now = chrono::Utc::now();
    if presigned {
        sigv4::check_expiry(&presented, now)?;
    }
    sigv4::verify(&presented, &secret, &canonical, now)?;

    Ok(key)
}

impl S3Request {
    /// Verbs that address an object: authenticate, locate, validate, authorise, rewrite.
    ///
    /// # Errors
    /// Every failure is an `S3Error` carrying the code a client can act on. See spec §6.
    pub async fn resolve(ctx: &AppContext, parts: &Parts, action: &str) -> Result<Self, S3Error> {
        let (bucket_name, encoded_key) = split_path(parts.uri.path());
        let logical_key = decode_key(&encoded_key);

        let key = authenticate(ctx, parts).await?;
        let user = users::Model::find_by_id(&ctx.db, key.user_id)
            .await
            .map_err(|_| S3Error::InternalError)?;

        // Scoped to this key's owner: another user's bucket reads as absent, not as forbidden, so a probe cannot confirm it exists.
        let bucket = buckets::Model::find_by_user_and_name(&ctx.db, user.id, &bucket_name)
            .await
            .map_err(|_| S3Error::InternalError)?
            .ok_or(S3Error::NoSuchBucket)?;

        let pool = pools::Model::find_by_id(&ctx.db, bucket.pool_id)
            .await
            .map_err(|_| S3Error::InternalError)?;

        // Validated before the prefix check, so a key containing `..` cannot slip past a prefix rule that approved it under a different name.
        validate_logical_key(&logical_key)?;

        // Both authorisation checks run before any rewrite, so a refusal cannot have produced a physical key.
        if !key
            .allows_action(&ctx.db, action)
            .await
            .map_err(|_| S3Error::InternalError)?
        {
            return Err(S3Error::AccessDenied);
        }
        if !key
            .allows_key(&ctx.db, &logical_key)
            .await
            .map_err(|_| S3Error::InternalError)?
        {
            return Err(S3Error::AccessDenied);
        }

        let physical_key = physical_key_for(&user, &bucket, &logical_key);

        Ok(Self {
            key,
            user,
            bucket,
            pool,
            logical_key,
            physical_key,
        })
    }

    /// For the two verbs that address a bucket and no object: `ListBuckets` and `HeadBucket`.
    ///
    /// `logical_key` and `physical_key` are empty, and reading them is a programming error — nothing but those two callers may use this.
    ///
    /// # Errors
    /// As `resolve`, minus the key-shaped failures.
    pub async fn resolve_bucket_only(
        ctx: &AppContext,
        parts: &Parts,
        action: &str,
    ) -> Result<Self, S3Error> {
        let (bucket_name, _) = split_path(parts.uri.path());

        let key = authenticate(ctx, parts).await?;
        let user = users::Model::find_by_id(&ctx.db, key.user_id)
            .await
            .map_err(|_| S3Error::InternalError)?;

        if !key
            .allows_action(&ctx.db, action)
            .await
            .map_err(|_| S3Error::InternalError)?
        {
            return Err(S3Error::AccessDenied);
        }

        let bucket = buckets::Model::find_by_user_and_name(&ctx.db, user.id, &bucket_name)
            .await
            .map_err(|_| S3Error::InternalError)?
            .ok_or(S3Error::NoSuchBucket)?;
        let pool = pools::Model::find_by_id(&ctx.db, bucket.pool_id)
            .await
            .map_err(|_| S3Error::InternalError)?;

        Ok(Self {
            key,
            user,
            bucket,
            pool,
            logical_key: String::new(),
            physical_key: String::new(),
        })
    }

    /// Resolves `x-amz-copy-source` under the same key's policy as the destination.
    ///
    /// Deliberately reuses the same checks as `resolve`, not a parallel implementation: the two ends of a copy are the classic place where one side gets checked and the other does not.
    ///
    /// # Errors
    /// `InvalidArgument` for a malformed header, then the same failures `resolve` produces for the source object.
    pub async fn resolve_copy_source(
        &self,
        ctx: &AppContext,
        header: &str,
    ) -> Result<PhysicalRef, S3Error> {
        // Both `/bucket/key` and `bucket/key` are accepted; S3 clients emit both.
        let raw = header.trim_start_matches('/');
        let (bucket_name, encoded_key) = raw.split_once('/').ok_or_else(|| {
            S3Error::InvalidArgument("x-amz-copy-source must be /bucket/key".to_string())
        })?;
        // A versionId suffix is parsed off and ignored: versioning is schema-only for now, and silently treating "?versionId=x" as part of the key would copy the wrong object.
        let encoded_key = encoded_key
            .split_once("?versionId=")
            .map_or(encoded_key, |(k, _)| k);
        let logical_key = decode_key(encoded_key);

        if bucket_name.is_empty() || logical_key.is_empty() {
            return Err(S3Error::InvalidArgument(
                "x-amz-copy-source must be /bucket/key".to_string(),
            ));
        }
        validate_logical_key(&logical_key)?;

        let bucket = buckets::Model::find_by_user_and_name(&ctx.db, self.user.id, bucket_name)
            .await
            .map_err(|_| S3Error::InternalError)?
            .ok_or(S3Error::NoSuchBucket)?;

        if !self
            .key
            .allows_key(&ctx.db, &logical_key)
            .await
            .map_err(|_| S3Error::InternalError)?
        {
            return Err(S3Error::AccessDenied);
        }

        let physical_key = physical_key_for(&self.user, &bucket, &logical_key);
        Ok(PhysicalRef {
            bucket,
            logical_key,
            physical_key,
        })
    }
}

/// The one place a physical key is built. Every caller goes through `resolve` or `resolve_copy_source`, both of which authorise first.
fn physical_key_for(user: &users::Model, bucket: &buckets::Model, logical_key: &str) -> String {
    format!("{}/{}/{}", user.pid, bucket.name, logical_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_path_separates_the_bucket_from_the_key() {
        assert_eq!(
            split_path("/media-cdn/img/a.png"),
            ("media-cdn".to_string(), "img/a.png".to_string())
        );
        assert_eq!(
            split_path("/media-cdn"),
            ("media-cdn".to_string(), String::new())
        );
        assert_eq!(
            split_path("/media-cdn/"),
            ("media-cdn".to_string(), String::new())
        );
        assert_eq!(split_path("/"), (String::new(), String::new()));
    }

    /// A key with a slash in it stays one key; only the first slash separates bucket from key.
    #[test]
    fn a_key_may_contain_slashes() {
        let (bucket, key) = split_path("/media-cdn/a/b/c/d.png");
        assert_eq!(bucket, "media-cdn");
        assert_eq!(key, "a/b/c/d.png");
    }

    #[test]
    fn traversal_is_refused_rather_than_normalised() {
        for bad in [
            "../other/a.png",
            "img/../../escape",
            "a/../../../etc/passwd",
            "..",
            "img/..",
        ] {
            assert!(
                validate_logical_key(bad).is_err(),
                "key {bad:?} should be refused"
            );
        }
    }

    /// `..` inside a segment name is a legitimate key; only a whole segment equal to `..` is traversal.
    #[test]
    fn a_double_dot_inside_a_name_is_allowed() {
        assert!(validate_logical_key("weird..name.png").is_ok());
        assert!(validate_logical_key("a/..b/c").is_ok());
        assert!(validate_logical_key("a/b../c").is_ok());
    }

    #[test]
    fn an_absolute_key_is_refused() {
        assert!(validate_logical_key("/img/a.png").is_err());
    }

    #[test]
    fn an_over_long_key_is_key_too_long() {
        let key = "a".repeat(MAX_KEY_LEN + 1);
        assert!(matches!(
            validate_logical_key(&key),
            Err(S3Error::KeyTooLong)
        ));
        assert!(validate_logical_key(&"a".repeat(MAX_KEY_LEN)).is_ok());
    }
}
