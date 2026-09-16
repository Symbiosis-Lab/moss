//! May we show the last build's pages while this build runs?
//!
//! Opening a folder starts the preview server before the build has rendered
//! anything, and rests it on the previous build's frozen generation. That is
//! deliberate: for an unchanged site it puts the real page on screen in
//! milliseconds instead of after a full render, and the finished build then
//! morphs the differences in.
//!
//! It only works while the old pages are still *this site*. When the previous
//! generation came out of a different moss — a version that named its classes
//! differently, shipped different CSS, or laid pages out at different URLs —
//! showing it means the user watches a broken version of their own site for
//! however long the build takes, then watches it change. One real report
//! (LOG-C2CF-T0935-08-14) had the previous generation missing the very page
//! being opened, so the first thing the user saw was a 404 shell with no
//! styling; they reasonably read that as moss having damaged their site.
//!
//! So the seed still happens — resting the server somewhere costs nothing and
//! keeps the rebuild window from 404ing — but moss stops *advertising* the old
//! output as ready to look at. `build.rs` announces the port early only for a
//! [`PriorOutput::Showable`] generation. Otherwise the frontend stays on the
//! affordance it already shows for a first build: the blueprint grid with
//! "Opening folder… / Scanning files… / Generating pages…". No new screen, no
//! new state, and nothing to lower — the ordinary completion event ends it,
//! which is the property a raised gate cannot promise.
//!
//! **Only the builder is checked.** Source edits since the last build make the
//! old pages out of *date*, not out of *shape* — showing them and morphing is
//! exactly right, and gating on them would put every folder-open behind a full
//! render. Plugin changes are deliberately out of scope too: they change what
//! is enhanced into a page rather than the page's shape, and
//! `compute_plugin_fingerprint` reads every plugin file, which is not a cost
//! this decision can carry on the open path. `builder_fingerprint` is a
//! memoized stat of the running binary.

use crate::infra::moss_paths::MossPaths;
use std::path::Path;

/// What the previous build left behind, from the point of view of "can the
/// user look at it right now?".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriorOutput {
    /// Nothing to serve — a first build, or a generation that was cleaned up.
    Absent,
    /// Pages exist, but a different moss produced them. Serve them so the
    /// window doesn't 404; don't put them in front of the user.
    StaleBuilder,
    /// Pages exist and came out of this same binary. Show them immediately.
    Showable,
}

impl PriorOutput {
    /// Whether `build.rs` may emit its early `server-ready` for this output.
    pub fn may_show_early(self) -> bool {
        matches!(self, PriorOutput::Showable)
    }
}

/// The decision itself, over values rather than the disk, so the three cases
/// are testable without a fixture site.
///
/// `previous_builder` is what the last build recorded in `hashes.json`;
/// `None` means it predates the field, which we treat as a different builder —
/// the same "assume stale when unsure" rule the staging hash comparison uses.
pub fn classify(
    dir_has_content: bool,
    previous_builder: Option<&str>,
    builder_now: &str,
) -> PriorOutput {
    if !dir_has_content {
        return PriorOutput::Absent;
    }
    match previous_builder {
        Some(prev) if prev == builder_now => PriorOutput::Showable,
        _ => PriorOutput::StaleBuilder,
    }
}

/// Read the disk and classify. `serve_dir` is what the server was just rested
/// on — the frozen `current` generation, or staging, per `initial_serve_dir()`.
pub fn classify_prior_output(
    folder_path: &Path,
    serve_dir: &Path,
    current_builder: &str,
) -> PriorOutput {
    let dir_has_content = std::fs::read_dir(serve_dir)
        .map(|mut d| d.next().is_some())
        .unwrap_or(false);

    // Read the fingerprint straight out of hashes.json rather than through
    // `load_previous_hashes`, which also runs the path-normalization migration
    // — real work this decision has no business triggering on the open path.
    let previous_builder = std::fs::read_to_string(MossPaths::new(folder_path).hashes())
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| {
            v.get("builder_fingerprint")
                .and_then(|f| f.as_str())
                .map(str::to_owned)
        });

    classify(dir_has_content, previous_builder.as_deref(), current_builder)
}

#[cfg(test)]
#[path = "prior_output_tests.rs"]
mod tests;
