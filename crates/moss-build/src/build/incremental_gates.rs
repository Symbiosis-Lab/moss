//! What this build may reuse from the last one.
//!
//! Two independent permissions, and the code that decides them. They live
//! together, away from `build.rs`, because they are one concept answered twice:
//! "is it safe to skip work because nothing that feeds it moved?"
//!
//! Per ADR-010 both are resolved HERE, at the entry point, from the build
//! trigger and the environment — the render phase reads neither. It receives
//! the answers as plain bools on `SiteConfig`.

use super::PipelineConfig;
use crate::build::BuildTrigger;
use std::path::Path;

/// The two independent "may this build reuse the last one's work?" permissions.
///
/// They were one boolean until a real 217-file site measured a `footer.md` edit
/// at 94ms in the markdown phase and a `.moss/theme/style.css` edit at 23-67s:
/// the render skip's markdown-only rule was also switching off the parse cache,
/// which a stylesheet cannot invalidate. They are separate fields, not one
/// flag, because the two answers come from different safety arguments — see
/// [`PipelineConfig::allows_incremental_skip`] and
/// [`PipelineConfig::allows_parse_cache_reuse`] below, which are the only
/// places either is decided.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IncrementalGates {
    /// Stage 5b — may the page-render loop carry forward pages whose
    /// fingerprint did not move? `false` for every non-watch entry point
    /// (`moss build`, `moss preview`, deploy all render every page) and
    /// `false` whenever `MOSS_NO_INCREMENTAL` is set.
    pub render_skip: bool,
    /// Stage 7 — may Loop A replay the previous build's `ParsedDocument`s?
    /// Same entry-point and kill-switch rules; the difference is which changed
    /// paths it tolerates (markdown plus a parse-irrelevant allowlist).
    pub parse_cache: bool,
}

impl PipelineConfig {
    /// Whether the render phase may skip re-rendering pages whose fingerprint
    /// is unchanged (moss#922 Stage 5b).
    ///
    /// Four conditions, all necessary, in the "over-approximate, never
    /// under-approximate" discipline the whole design rests on:
    ///
    /// 1. `MOSS_NO_INCREMENTAL` unset. The kill switch ships with the first
    ///    skip, not after it — it is the bisect tool for the first stale-page
    ///    report. Any value counts, matching `MOSS_WATCH_NO_GATE`.
    /// 2. The trigger is `ContentOnly`. `moss build`, `moss preview`, deploy
    ///    and every other entry point pass `Full`, so their behavior is
    ///    unchanged by this feature. `Structural` (create/remove/rename) is
    ///    deliberately treated like `Full`: it moves pages in and out of
    ///    listings, nav and folder indexes.
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
    pub fn allows_incremental_skip(&self) -> bool {
        if std::env::var("MOSS_NO_INCREMENTAL").is_ok() {
            return false;
        }
        let BuildTrigger::ContentOnly(paths) = &self.trigger else {
            return false;
        };
        !paths.is_empty() && paths.iter().all(|p| is_markdown_ext(p))
    }

    /// Whether Loop A may replay the previous build's `ParsedDocument`s
    /// (moss#922 Stage 7 — see `build/parse_cache.rs`).
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
    /// fingerprint covers that.
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
    /// cached parse is an ADR-013 violation (stale `<picture>` markup).
    /// `.moss/config.toml` is likewise excluded: it moves site scalars that
    /// `process_markdown_file` reads.
    ///
    /// Unrecognised extensions — and extensionless paths — disable the cache.
    /// The allowlist is the whole safety argument, so it over-approximates by
    /// construction: a new file type is ineligible until someone proves it
    /// parse-irrelevant and adds it here.
    pub fn allows_parse_cache_reuse(&self) -> bool {
        if std::env::var("MOSS_NO_INCREMENTAL").is_ok() {
            return false;
        }
        let (BuildTrigger::ContentOnly(paths) | BuildTrigger::Structural(paths)) = &self.trigger
        else {
            return false;
        };
        !paths.is_empty()
            && paths
                .iter()
                .all(|p| is_markdown_ext(p) || is_parse_irrelevant_ext(p))
    }

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
    ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown")
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
