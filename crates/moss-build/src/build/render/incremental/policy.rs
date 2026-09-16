//! Config-resolved permission bits for the incremental render (ADR-010).
//!
//! Every "may this build elide X" question is answered ONCE, here, from the
//! already-resolved [`SiteConfig`]. Downstream passes ask the policy; none of
//! them re-derives the answer from `BuildTrigger`, `start_server` or an
//! environment variable. That is the ADR-010 shape: mode differences live in
//! the Config, not as branching inside the pipeline.
//!
//! It ships with exactly **one** field. Fields are added when the elision they
//! gate lands, not in advance — the NORTH-STAR abstraction gate wants two real
//! callers, and a bag of bits for not-yet-existent elisions is predicting, not
//! compressing.

use crate::build::render::config::SiteConfig;

/// What this build is permitted to skip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IncrementalPolicy {
    /// May a page whose fingerprints did not move be carried forward instead
    /// of re-rendered?
    ///
    /// Resolved upstream by `PipelineConfig::allows_incremental_skip`
    /// (`build.rs`), which is false for every entry point except a
    /// markdown-only watch rebuild and which already folds in the
    /// `MOSS_NO_INCREMENTAL` kill switch. This is the total off switch for
    /// moss#922 Stage 5b *and* for the moss#968 listing groups: with it false,
    /// the verdict is `Full(SkipDisabled)` and nothing below runs.
    pub skip_unchanged_renders: bool,
}

impl IncrementalPolicy {
    /// Resolve the policy beside `SiteConfig`, from `SiteConfig`.
    ///
    /// Deliberately NOT read from `PipelineConfig` directly: `SiteConfig`
    /// already carries `allows_incremental_skip()`'s output as
    /// `incremental.render_skip`, and reading it here keeps the seam inside
    /// `build/render/` instead of adding a second parameter to every layer
    /// between `pipeline::build_inner` and `generate_blocking_content`.
    pub fn resolve(site_config: &SiteConfig) -> Self {
        Self { skip_unchanged_renders: site_config.incremental.render_skip }
    }
}
