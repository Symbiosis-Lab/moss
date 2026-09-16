//! The moss-seta API client.
//!
//! Crossed into this crate by [ADR-078](../../../../docs/decisions/ADR-078-the-seta-client-ships-in-the-open-half.md)
//! so the open binary can answer `deploy` and `domain`. Split per
//! `docs/archive/2026-04-24-codebase-restructure-continuation-plan.md` Task 7.

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
