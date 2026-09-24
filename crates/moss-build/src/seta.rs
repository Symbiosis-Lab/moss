//! The moss-seta API client.
//!
//! Crossed into this crate so the open binary can answer `deploy` and `domain`.

pub mod client;
pub mod signing;
pub mod domains;
pub mod dns;
pub mod cdn;
pub(crate) mod tls;
pub mod sites;
pub(crate) mod chunked_upload;
pub mod upload_policy;
pub(crate) mod failure_class;
