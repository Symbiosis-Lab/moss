//! What this build may reuse from the last one.
//!
//! Two independent permissions, and the code that decides them. They live
//! together, away from `build.rs`, because they are one concept answered twice:
//! "is it safe to skip work because nothing that feeds it moved?"
//!
//! Both are resolved HERE, at the entry point, from what the build knows
//! changed ([`ChangeProof`]) and the environment — the render phase reads
//! neither. It receives the answers as plain bools on `SiteConfig`.

use super::PipelineConfig;
use crate::build::BuildTrigger;
use std::path::{Path, PathBuf};

/// The two independent "may this build reuse the last one's work?" permissions.
///
/// They were one boolean until a real 217-file site measured a `footer.md` edit
/// at 94ms in the markdown phase and a `.moss/theme/style.css` edit at 23-67s:
/// the render skip's markdown-only rule was also switching off the parse cache,
/// which a stylesheet cannot invalidate. They are separate fields, not one
/// flag, because the two answers come from different safety arguments — see
/// [`ChangeProof::allows_incremental_skip`] and
/// [`ChangeProof::allows_parse_cache_reuse`] below, which are the only
/// places either is decided.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IncrementalGates {
    /// May the page-render loop carry forward pages whose
    /// fingerprint did not move? `false` without evidence of what changed
    /// (deploy renders every page) and whenever `MOSS_NO_INCREMENTAL` is set.
    pub render_skip: bool,
    /// May Loop A replay the previous build's `ParsedDocument`s?
    /// Same entry-point and kill-switch rules; the difference is which changed
    /// paths it tolerates (markdown plus a parse-irrelevant allowlist).
    pub parse_cache: bool,
}

impl IncrementalGates {
    /// Both permissions for one build, from one [`ChangeProof`]. `builder` is
    /// the fingerprint `run_pipeline` stamps on the manifest it seals.
    pub fn for_build(trigger: &BuildTrigger, folder: &Path, builder: &str, seal: &LastSeal) -> Self {
        let proof = {
            let _trace = crate::build::phase::PhaseTrace::start("change_proof");
            ChangeProof::for_build(trigger, folder, builder, seal)
        };
        crate::build::phase::record_value("proved_from_seal", usize::from(proof == ChangeProof::Manifest));
        Self { render_skip: proof.allows_incremental_skip(), parse_cache: proof.allows_parse_cache_reuse() }
    }
}

/// The last sealed manifest, and this build's `.moss/` measurement against
/// its record. Taken once, at the entry point, before anything in the build
/// reads `.moss/`: the proof compares the measurement and the manifest
/// records it, so a file edited during the build moves the next record.
#[derive(Debug, Default)]
pub struct LastSeal {
    manifest: Option<crate::types::content::SiteHashes>,
    moss: crate::build::inputs::Measured,
}

impl LastSeal {
    pub fn read(folder: &Path) -> Self {
        let _trace = crate::build::phase::PhaseTrace::start("seal_read");
        let manifest = crate::ops::watch::sweep::walk::read_baseline_from_disk(folder);
        let moss = crate::build::inputs::measure(folder, manifest.as_ref().and_then(|m| m.build_inputs.as_ref()));
        Self { manifest, moss }
    }

    /// What this build records of its `.moss/` reads; see
    /// `PendingManifest::set_build_inputs`.
    pub(crate) fn record(&self) -> Option<crate::build::inputs::BuildInputs> {
        self.moss.record.clone()
    }

    /// The manifest as read, before key migration; `build_inner` migrates its
    /// own copy, so the proof and the carry judge one read.
    pub(crate) fn manifest(&self) -> Option<&crate::types::content::SiteHashes> {
        self.manifest.as_ref()
    }
}

/// What this build knows changed since the last sealed build.
///
/// The trigger is the caller's fact; this is what the gates make of it.
#[derive(Debug, Clone, PartialEq)]
pub enum ChangeProof {
    /// The watcher's batch, and whether it created, removed or renamed a file.
    Events { paths: Vec<PathBuf>, structural: bool },
    /// The last sealed manifest proves that nothing but markdown pages changed
    /// since; see [`ChangeProof::from_last_seal`].
    Manifest,
    /// Nothing is known, so every page renders.
    None,
}

impl ChangeProof {
    pub fn for_build(trigger: &BuildTrigger, folder: &Path, builder: &str, seal: &LastSeal) -> Self {
        if kill_switch() {
            return Self::None;
        }
        match trigger {
            BuildTrigger::Full => Self::None,
            BuildTrigger::Structural(paths) => Self::Events { paths: paths.clone(), structural: true },
            BuildTrigger::ContentOnly(paths) => Self::Events { paths: paths.clone(), structural: false },
            BuildTrigger::Unobserved => Self::from_last_seal(folder, builder, seal),
        }
    }

    /// Whether the render phase may skip re-rendering pages whose fingerprint
    /// is unchanged.
    ///
    /// Four conditions, all necessary, in the "over-approximate, never
    /// under-approximate" discipline the whole design rests on:
    ///
    /// 1. `MOSS_NO_INCREMENTAL` unset ([`ChangeProof::for_build`] answers
    ///    `None` under it). The kill switch ships with the first skip, not
    ///    after it — it is the bisect tool for the first stale-page report.
    ///    Any value counts, matching `MOSS_WATCH_NO_GATE`.
    /// 2. The batch is content-only. Deploy and the other entry points that
    ///    pass `Full` render everything. A `Structural` batch
    ///    (create/remove/rename) is deliberately treated like `Full`: it moves
    ///    pages in and out of listings, nav and folder indexes.
    /// 3. Every changed path is markdown. A `.moss/config.toml` or `.css`
    ///    edit also arrives as a plain modify, but it moves build-globals
    ///    (`css_version`, `site_url`, layout config) that are baked into
    ///    EVERY page's HTML while changing no page's fingerprint — the one
    ///    way this design could silently serve a stale whole site.
    /// 4. At least one path — an empty batch carries no information.
    ///
    /// Condition 3 stays MARKDOWN-ONLY here even though the parse cache below
    /// now admits `.css`/`.js`/fonts. Not because a CSS edit is unsafe for the
    /// render skip — `FacadeCache::asset_versions`
    /// (`render/incremental/facade.rs:145-176`) already folds `css_version`,
    /// `user_css_version` and `user_js_version` and yields
    /// `FullCause::AssetVersionsMoved`, so a stylesheet edit forces a full
    /// render through a path that does not consult this flag at all. Relaxing
    /// condition 3 would therefore change nothing for CSS while widening the
    /// blast radius of every OTHER build-global this one boolean happens to
    /// stand in for. Two gates, two rules: this one is coarse and cheap to
    /// reason about, the parse cache's is narrow because that is where the
    /// 23-67s lives.
    ///
    /// [`ChangeProof::Manifest`] meets conditions 2 to 4 by construction: the
    /// proof itself checked that no file was created, removed or renamed, that
    /// only markdown changed and that every build-global input held.
    pub fn allows_incremental_skip(&self) -> bool {
        match self {
            Self::Events { paths, structural: false } => !paths.is_empty() && paths.iter().all(|p| is_markdown_ext(p)),
            Self::Manifest => true,
            Self::Events { structural: true, .. } | Self::None => false,
        }
    }

    /// Whether Loop A may replay the previous build's `ParsedDocument`s
    /// (see `build/parse_cache.rs`).
    ///
    /// Conditions 1 and 4 above hold verbatim — same kill switch, same
    /// non-empty batch. The other two differ, and both differences are the
    /// point of having two gates.
    ///
    /// Condition 2 is WIDER: `Structural` is admitted, not just `ContentOnly`.
    /// The trigger is not what makes this safe — [`inputs_fingerprint`]
    /// (`build/parse_cache.rs`) is. It hashes the sorted `markdown_files`
    /// list, so a create, delete or rename of any markdown file moves the
    /// fingerprint and bypasses the cache for the WHOLE build no matter which
    /// trigger carried it. Excluding `Structural` here therefore protected
    /// nothing that was not already protected, while charging a full reparse
    /// — 6.5 s of an 11.9 s rebuild on a 226-page vault — to every batch that
    /// was structural for a reason no markdown parse can see. The render skip
    /// above still refuses `Structural`, and must: listings, nav and folder
    /// indexes move on a create without any page's fingerprint moving, and no
    /// fingerprint covers that. The render verdict's page-set comparison
    /// (`FullCause::PathSetMoved`) is the second guard: a batch the watcher
    /// read as content-only that did add or drop a page still renders in full.
    ///
    /// Condition 3 is NARROWER: instead of "every changed path is markdown",
    /// it is "every changed path is markdown OR **parse-irrelevant**".
    ///
    /// A path is parse-irrelevant when its bytes provably cannot change any
    /// markdown file's parse. That is a claim about Loop A's inputs, not about
    /// the page's final HTML: stylesheets, scripts and fonts are referenced by
    /// the SHELL (`css_version` and friends, applied at render time), never
    /// read while a document is parsed. Images are NOT in the list and must
    /// never be — `event_level_image_lookup` bakes dimensions, LQIP and the
    /// `<picture><source>` wrap into parsed HTML, so an image edit replaying a
    /// cached parse would leave the `<picture>` markup stale.
    /// `.moss/config.toml` is likewise excluded: it moves site scalars that
    /// `process_markdown_file` reads.
    ///
    /// Unrecognised extensions — and extensionless paths — disable the cache.
    /// The allowlist is the whole safety argument, so it over-approximates by
    /// construction: a new file type is ineligible until someone proves it
    /// parse-irrelevant and adds it here.
    pub fn allows_parse_cache_reuse(&self) -> bool {
        match self {
            Self::Events { paths, .. } => !paths.is_empty()
                && paths.iter().all(|p| is_markdown_ext(p) || is_parse_irrelevant_ext(p)),
            Self::Manifest => true,
            Self::None => false,
        }
    }

    /// What a build with no events can prove from the last sealed manifest:
    /// that only markdown pages changed since.
    ///
    /// Every rule below falls back to [`ChangeProof::None`]:
    ///
    /// - The manifest is missing or unreadable, or a different moss binary
    ///   sealed it (its templates are an input no file shows).
    /// - Its [`BuildInputs`](crate::build::inputs::BuildInputs) are missing,
    ///   or a `.moss/` file, a link card or an environment variable they
    ///   recorded moved. They name what the last build read outside the page
    ///   tree, so no list of such inputs is kept here.
    /// - The page tree moved in a way the watcher would have called
    ///   structural, or a non-markdown file changed. The measurement is the
    ///   watch sweep's own, judged by the same three-tier compare against the
    ///   manifest's capture clock, so a file the sweep would call unchanged is
    ///   unchanged here too.
    /// - Anything is still in the cloud, could not be judged, or took longer
    ///   than the sweep's own pass budget.
    fn from_last_seal(folder: &Path, builder: &str, seal: &LastSeal) -> Self {
        use crate::build::watch::drift;
        use crate::ops::watch::sweep::{walk, WALK_DEADLINE};
        let Some(baseline) = seal.manifest.as_ref() else { return Self::None };
        if baseline.builder_fingerprint.as_deref() != Some(builder) || !seal.moss.holds {
            return Self::None;
        }
        let deadline = std::time::Instant::now() + WALK_DEADLINE;
        let Ok(walked) = walk::walk_with_error(folder, Some(deadline), None) else { return Self::None };
        if walked.deadline_blown || !walked.dataless.is_empty() {
            return Self::None;
        }
        let report = drift::detect_drift(baseline, &walked.files, &[], folder, drift::probe_missing, Some(deadline), true);
        let only_pages = report.drifted.iter()
            .all(|(rel, drift)| drift.kind == drift::DriftKind::Edited && crate::build::manifest::is_page_source_key(rel));
        if report.deadline_blown || report.offline > 0 || !only_pages {
            return Self::None;
        }
        Self::Manifest
    }
}

/// The bisect tool for a stale-page report: any value turns both gates off.
fn kill_switch() -> bool {
    std::env::var("MOSS_NO_INCREMENTAL").is_ok()
}

/// `foo.md` / `foo.markdown` — the only sources Loop A parses.
fn is_markdown_ext(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(is_markdown_extension)
}

impl PipelineConfig {
    /// May image dominant-colour/LQIP extraction be deferred off the blocking
    /// scan, to be enriched later by the background media phase?
    ///
    /// The third permission of the same family as the two above, and it was
    /// decided the same wrong way they once were: from a flag that means
    /// something adjacent. `defer_placeholders` read `start_server`, which is
    /// orchestration ("this build launches the server") and true only for the
    /// folder-open build — so every WATCH rebuild took the deploy path and
    /// full-decoded every image with no content-hash cache entry. Measured on
    /// a 876-image vault: 794 decodes, 14s of a 16.0s rebuild, in front of the
    /// user's first save; and colour/LQIP flipping None → Some moves the
    /// SURFACE of every page showing those images, forcing a whole-site
    /// re-render on top.
    ///
    /// Ask the question by name. `server_port` is the watcher's own "a server
    /// is already serving this folder"; it is conservative, so a false
    /// negative only costs work and can never ship a bare placeholder. Deploy,
    /// plugin-install and CLI builds leave it `None` and keep baking.
    pub fn defers_image_placeholders(&self) -> bool {
        self.start_server || self.server_port.is_some()
    }
}

/// [`is_markdown_ext`] over a bare extension, so callers holding a
/// project-relative `&str` key (the manifest's `sources`) share the one answer.
pub(crate) fn is_markdown_extension(ext: &str) -> bool {
    crate::build::scan::classify::is_page_source(&ext.to_ascii_lowercase())
}

/// Extensions whose bytes are never read while a markdown file is parsed.
///
/// Presentation-layer assets only: stylesheets, scripts, web fonts. Everything
/// they affect is applied at render time from a version stamp, which the render
/// side gates independently (`FacadeCache::asset_versions`). Deliberately NOT
/// here: any image or media extension (dimensions and LQIP are baked into
/// parsed HTML), `.toml` (site scalars), `.json`, `.yaml`, and anything else.
const PARSE_IRRELEVANT_EXTENSIONS: &[&str] =
    &["css", "js", "mjs", "woff", "woff2", "ttf", "otf", "eot"];

fn is_parse_irrelevant_ext(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| PARSE_IRRELEVANT_EXTENSIONS.iter().any(|k| e.eq_ignore_ascii_case(k)))
}

#[cfg(test)]
#[path = "incremental_gates_tests.rs"]
mod tests;
