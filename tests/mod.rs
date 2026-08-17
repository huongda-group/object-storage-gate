// `axum_test::TestServer` is deliberately not Send, so every helper that awaits a request
// trips clippy::future_not_send under --all-targets. Nothing in the suite is spawned across
// threads, so the lint has nothing to protect here.
#![allow(clippy::future_not_send)]

mod models;
mod requests;
mod tasks;
mod workers;
