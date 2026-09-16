//! The incremental-render verdict and everything that feeds it (moss#968).
//!
//! Three concerns, one owner:
//!
//! * [`policy`] — [`IncrementalPolicy`], the Config-resolved permission bits
//!   (ADR-010). No pass downstream branches on `BuildTrigger`, `start_server`
//!   or an env var; they ask the policy.
//! * [`listing`] — the listing group model (ADR-044). Membership as a *value*
//!   rather than N×H edges, so a page that merely *can* host a listing no
//!   longer renders unconditionally.
//! * [`verdict`] — [`RenderVerdict`], the one authority on "which pages must
//!   this build re-render". Moved out of `render/blocking.rs`, where the same
//!   computation was produced, logged and then consumed by exactly one
//!   `partition` fifty lines below (moss#968 Finding 1).
//! * [`carry_verify`] — the `MOSS_INCREMENTAL_VERIFY=1` falsifier for the
//!   above (§10 gate 4): render the carried set anyway and prove the served
//!   bytes are the same either way.
//!
//! Design: `docs/archive/2026-08-05-incremental-build-architecture.md`.

pub mod carry_verify;
pub mod listing;
pub mod policy;
pub mod verdict;

pub use carry_verify::CarryVerification;
pub use policy::IncrementalPolicy;
pub use verdict::{FullCause, RenderVerdict, VerdictBasis};
