//! The incremental-render verdict and everything that feeds it.
//!
//! Three concerns, one owner:
//!
//! * [`policy`] — [`IncrementalPolicy`], the Config-resolved permission bits.
//!   No pass downstream branches on `BuildTrigger`, `start_server`
//!   or an env var; they ask the policy.
//! * [`listing`] — the listing group model. Membership as a *value*
//!   rather than N×H edges, so a page that merely *can* host a listing no
//!   longer renders unconditionally.
//! * [`verdict`] — [`RenderVerdict`], the one authority on "which pages must
//!   this build re-render". Moved out of `render/blocking.rs`, where the same
//!   computation was produced, logged and then consumed by exactly one
//!   `partition` fifty lines below.
//! * [`carry_verify`] — the `MOSS_INCREMENTAL_VERIFY=1` falsifier for the
//!   above: render the carried set anyway and prove the served
//!   bytes are the same either way.

pub mod carry_verify;
pub mod listing;
pub mod policy;
pub mod verdict;

pub use carry_verify::CarryVerification;
pub use policy::IncrementalPolicy;
pub use verdict::{FullCause, RenderVerdict, VerdictBasis};
