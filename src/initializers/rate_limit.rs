//! Per-IP rate limiting.
//!
//! loco 0.16 ships no rate-limit middleware, so this is a plain tower layer mounted through an initializer.
//! Without it, `POST /api/auth/login` accepts unlimited password guesses against every account.
use std::task::{Context, Poll};

use async_trait::async_trait;
use axum::{
    body::Body,
    extract::Request,
    response::{IntoResponse, Response},
    Router as AxumRouter,
};
use loco_rs::{
    app::{AppContext, Initializer},
    Result,
};
use std::{future::Future, pin::Pin, sync::Arc};
use tower::{Layer, Service};
use tower_governor::{
    governor::GovernorConfigBuilder, key_extractor::SmartIpKeyExtractor, GovernorLayer,
};

pub struct RateLimitInitializer;

/// Requests allowed per minute per IP once the burst is spent.
fn per_minute() -> u64 {
    std::env::var("RATE_LIMIT_PER_MINUTE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60)
}

/// How many requests an IP may fire back-to-back before the per-minute rate applies.
fn burst() -> u32 {
    std::env::var("RATE_LIMIT_BURST")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30)
}

/// Whether to read the client IP from `Forwarded` / `X-Forwarded-For` instead of the socket.
///
/// Off by default because those headers are client-supplied: trusting them without a proxy that overwrites them lets anyone reset their own bucket by sending a new value.
/// On the other hand, leaving it off behind a reverse proxy makes every request share the proxy's IP, which turns a per-IP limit into a gateway-wide one.
/// Set `RATE_LIMIT_TRUST_PROXY=true` when, and only when, a proxy you control sets the header.
fn trust_proxy() -> bool {
    std::env::var("RATE_LIMIT_TRUST_PROXY")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false)
}

#[async_trait]
impl Initializer for RateLimitInitializer {
    fn name(&self) -> String {
        "rate-limit".to_string()
    }

    async fn after_routes(&self, router: AxumRouter, _ctx: &AppContext) -> Result<AxumRouter> {
        let seconds_per_request = (60 / per_minute().max(1)).max(1);

        if trust_proxy() {
            let config = Arc::new(
                GovernorConfigBuilder::default()
                    .per_second(seconds_per_request)
                    .burst_size(burst())
                    .key_extractor(SmartIpKeyExtractor)
                    .finish()
                    .ok_or_else(|| loco_rs::Error::string("invalid rate limit configuration"))?,
            );
            return Ok(router.layer(ApiOnly::new(GovernorLayer::new(config))));
        }

        let config = Arc::new(
            GovernorConfigBuilder::default()
                .per_second(seconds_per_request)
                .burst_size(burst())
                .finish()
                .ok_or_else(|| loco_rs::Error::string("invalid rate limit configuration"))?,
        );

        Ok(router.layer(ApiOnly::new(GovernorLayer::new(config))))
    }
}

/// Applies an inner layer to the management API only.
///
/// The limiter exists to stop password guessing on `POST /api/auth/login`. Applying it to the S3
/// data plane instead breaks the product: `aws s3 sync` of 1200 objects stops at the ~999th with
/// a 429, and a multipart upload of a large file trips it too. The data plane is already gated by
/// `SigV4` per access key, which is a stronger control than a per-IP bucket.
#[derive(Clone)]
pub struct ApiOnly<L> {
    inner: L,
}

impl<L> ApiOnly<L> {
    pub const fn new(inner: L) -> Self {
        Self { inner }
    }
}

impl<S, L> Layer<S> for ApiOnly<L>
where
    L: Layer<S>,
    S: Clone,
{
    type Service = ApiOnlyService<S, L::Service>;

    fn layer(&self, service: S) -> Self::Service {
        ApiOnlyService {
            limited: self.inner.layer(service.clone()),
            plain: service,
        }
    }
}

/// Routes each request to either the limited or the unlimited copy of the inner service.
#[derive(Clone)]
pub struct ApiOnlyService<S, L> {
    limited: L,
    plain: S,
}

/// Whether the limiter applies to this path.
///
/// Only `/api/*`: everything else is either the console's static assets or the S3 data plane.
fn is_management_api(path: &str) -> bool {
    path == "/api" || path.starts_with("/api/")
}

impl<S, L> Service<Request> for ApiOnlyService<S, L>
where
    S: Service<Request, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
    L: Service<Request, Response = Response, Error = S::Error> + Clone + Send + 'static,
    L::Future: Send + 'static,
{
    type Response = Response<Body>;
    type Error = S::Error;
    type Future =
        Pin<Box<dyn Future<Output = std::result::Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<std::result::Result<(), Self::Error>> {
        self.plain.poll_ready(cx)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        if is_management_api(req.uri().path()) {
            let mut limited = self.limited.clone();
            return Box::pin(
                async move { limited.call(req).await.map(IntoResponse::into_response) },
            );
        }
        let mut plain = self.plain.clone();
        Box::pin(async move { plain.call(req).await.map(IntoResponse::into_response) })
    }
}

#[cfg(test)]
mod tests {
    use super::is_management_api;

    #[test]
    fn only_the_management_api_is_limited() {
        assert!(is_management_api("/api"));
        assert!(is_management_api("/api/auth/login"));
        assert!(is_management_api("/api/admin/pools"));

        // The S3 data plane, the console and its assets are all outside.
        assert!(!is_management_api("/"));
        assert!(!is_management_api("/media-cdn/img/a.png"));
        assert!(!is_management_api("/static/js/app.js"));
        // A bucket that happens to be named apixyz must not be swept in by a prefix match.
        assert!(!is_management_api("/apixyz/a.png"));
    }
}
