//! The S3 data plane.
//!
//! `sigv4` verifies what a client signed and signs what the gateway sends upstream — one canonical-request implementation running in both directions.
//! `upstream` streams bodies through without buffering them.
//! `error` maps every failure to an S3 error code, because a client that cannot read the code cannot act on it.
pub mod error;
pub mod sigv4;
pub mod upstream;
