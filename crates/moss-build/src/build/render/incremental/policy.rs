//! Config-resolved permission bits for the incremental render.
//!
//! Every "may this build elide X" question is answered ONCE, here, from the
//! already-resolved [`SiteConfig`]. Downstream passes ask the policy; none of
//! them re-derives the answer from `BuildTrigger`, `start_server` or an
//! environment variable. Mode differences live in
//! the Config, not as branching inside the pipeline.
//!
//! Fields are added when the elision they gate lands, not in advance: a bag of
//! bits for elisions that do not exist yet is predicting, not compressing.

use crate::build::render::config::SiteConfig;

/// What this build is permitted to skip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IncrementalPolicy {
    /// May a page whose fingerprints did not move be carried forward instead
    /// of re-rendered?
    ///
    /// Resolved upstream by `ChangeProof::allows_incremental_skip`
    /// (`incremental_gates.rs`), which is true only for a markdown-only watch
    /// rebuild or a build the last seal proves touched only markdown, and
    /// which already folds in the `MOSS_NO_INCREMENTAL` kill switch. This is the total off switch for
    /// the render skip *and* for the listing groups: with it false,
    /// the verdict is `Full(SkipDisabled)` and nothing below runs.
    pub skip_unchanged_renders: bool,
    /// May a page that would render only because an image it shows has a new
    /// placeholder wait for the settled build? See
    /// `IncrementalGates::defer_image_placeholders`.
    pub defer_image_placeholders: bool,
}

impl IncrementalPolicy {
    /// Resolve the policy beside `SiteConfig`, from `SiteConfig`.
    ///
    /// Deliberately NOT read from `PipelineConfig` directly: `SiteConfig`
    /// already carries `ChangeProof::allows_incremental_skip`'s output as
    /// `incremental.render_skip`, and reading it here keeps the seam inside
    /// `build/render/` instead of adding a second parameter to every layer
    /// between `pipeline::build_inner` and `generate_blocking_content`.
    pub fn resolve(site_config: &SiteConfig) -> Self {
        Self {
            skip_unchanged_renders: site_config.incremental.render_skip,
            defer_image_placeholders: site_config.incremental.defer_image_placeholders,
        }
    }
}
