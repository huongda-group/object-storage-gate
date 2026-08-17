//! Per-IP rate limiting.
//!
//! loco 0.16 ships no rate-limit middleware, so this is a plain tower layer mounted through an initializer.
//! Without it, `POST /api/auth/login` accepts unlimited password guesses against every account.
use async_trait::async_trait;
use axum::Router as AxumRouter;
use loco_rs::{
    app::{AppContext, Initializer},
    Result,
};
use std::sync::Arc;
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

    // ponytail: applied to the whole router, not per-route — tower_governor takes no route filter, and a nested router is more code than this is worth today.
    // Ceiling: the S3 data plane must be excluded before slice #3 ships, or a legitimate multipart upload will trip it.
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
            return Ok(router.layer(GovernorLayer::new(config)));
        }

        let config = Arc::new(
            GovernorConfigBuilder::default()
                .per_second(seconds_per_request)
                .burst_size(burst())
                .finish()
                .ok_or_else(|| loco_rs::Error::string("invalid rate limit configuration"))?,
        );

        Ok(router.layer(GovernorLayer::new(config)))
    }
}
