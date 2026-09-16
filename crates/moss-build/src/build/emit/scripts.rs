//! Site runtime scripts: hashing and emission for every `_moss/js/*.js`.
//!
//! One emit family, one owner. Before 2026-08-04 this lived as seven
//! near-identical pairs in `blocking.rs` — a `load_js_asset` + hash block near
//! line 1020 and an `if gate { BuildContext::…emit(…) }` block near line 2820,
//! 1,800 lines apart, with the `<script>` tag built in a third file. Adding an
//! eighth (`share-card`) that way is exactly the "more code in `blocking.rs`"
//! NORTH-STAR's interception rule forbids.
//!
//! ## What this module owns, and what it does not
//!
//! It owns **which scripts exist, what gates them, what bytes go to disk, and
//! — for the shell block — the order and `defer` of their `<script>` tags**.
//! Rendering one tag is still `assets/paths.rs`'s job
//! ([`PathResolver::runtime_js_tag`]); this module decides which tags the
//! block contains. Until 2026-08-28 the block was six template variables, so
//! each script cost a hash `let`, a tag `let`, a `ShellVars` field and a
//! `shell.rs` mapping — twice over, because `render/html.rs` paid for the
//! same set again on the isolated-page path (#1149).
//!
//! ## Eager and lazy
//!
//! [`Load::Shell`] and [`Load::Placed`] scripts get a `<script src>` in the
//! page — the difference is only who writes the tag. [`Load::Lazy`] ones are
//! emitted and then fetched on demand by an `import()` in the runtime:
//! `share-card`, whose 12.4 kb of canvas rendering ran on no page until a
//! reader selected text and tapped Share, and `hls.js`, which only a browser
//! with MSE and no native HLS ever needs. Lazy scripts are `format: "esm"` in
//! `scripts/build-backend-scripts.mjs`; the rest are `iife`.
//!
//! See `docs/archive/2026-08-04-ship-what-the-site-needs.md` §4 Milestone B.

use crate::build::assets::paths::{compute_binary_hash, PathResolver};
use crate::build::render::html::load_js_asset;
use crate::build::served_path::ServedPath;
use crate::build::types::SiteAssets;
use include_flate::flate;

// hls.js is 571 kb of minified JS — the one script whose raw bytes are worth
// not carrying in .rodata. Stored deflate-compressed, inflated once on first
// use (path is relative to this crate's manifest dir).
flate!(static HLS_JS: str from "src/assets/js/hls.js");

/// How a script reaches the browser, and — for the eager ones — who places
/// its tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Load {
    /// In the shell's runtime `<script>` block, in table order, with this
    /// `defer`. [`ScriptAssets::shell_tags`] builds the whole block.
    Shell { defer: bool },
    /// Eager, but placed by a template of its own rather than by that block:
    /// `theme` rides `{js_path}` because its tag also carries the lazy chunks'
    /// URLs, and `fullscreen` exists only on the media-collection page.
    /// A `Placed` script may still appear in the shell block's *company* on
    /// another template — `search` is emitted onto the media page next to
    /// `fullscreen` — which is why placement is not one exclusive location.
    Placed,
    /// Fetched by a runtime `import()`. The URL is published to the runtime on
    /// a `data-` attribute, since the source cannot know the content hash.
    Lazy,
}

/// One runtime script and the fact that turns it on.
pub struct SiteScript {
    /// Basename. Becomes `_moss/js/<name>.<hash>.js`.
    pub name: &'static str,
    /// Path under this crate for dev hot-reload via [`load_js_asset`].
    pub dev_path: &'static str,
    /// Returns the esbuild output embedded at compile time. A fn pointer
    /// (like `gate`) rather than a `&'static str` so a row may serve its
    /// bytes from a lazily-inflated static instead of raw `.rodata`.
    pub source: fn() -> &'static str,
    /// Reads the site-level fact that gates this script.
    pub gate: fn(&SiteAssets) -> bool,
    pub load: Load,
}

/// Every script moss emits into `_moss/js/`.
///
/// A registration table in the NORTH-STAR pattern-glossary #2 sense; sibling
/// of `emit::stylesheet::CSS_PARTIALS` and `emit::feature_styles::FEATURE_STYLES`.
/// Totality test: `scripts_tests::every_embedded_script_has_a_row`.
pub const SITE_SCRIPTS: &[SiteScript] = &[
    SiteScript {
        name: "theme",
        dev_path: "src/assets/js/theme.js",
        source: || include_str!("../../assets/js/theme.js"),
        gate: |_| true,
        load: Load::Placed,
    },
    // NOTE: no row for the blueprint placeholder. `asset-placeholder.js` never
    // reaches a published site at all — the preview server injects it into
    // <head> on the way out (`preview::iframe_bridge::inject_placeholder_into_head`),
    // so the placeholder is a local working state and a stranger never sees one.
    // It replaced `thumb-swap`, which did have a row here.
    SiteScript {
        name: "preview",
        dev_path: "src/assets/js/preview.js",
        source: || include_str!("../../assets/js/preview.js"),
        gate: |a| a.link_preview,
        load: Load::Shell { defer: false },
    },
    SiteScript {
        name: "heading-anchor",
        dev_path: "src/assets/js/heading-anchor.js",
        source: || include_str!("../../assets/js/heading-anchor.js"),
        gate: |a| a.heading_anchors,
        load: Load::Shell { defer: false },
    },
    SiteScript {
        name: "math-copy",
        dev_path: "src/assets/js/math-copy.js",
        source: || include_str!("../../assets/js/math-copy.js"),
        gate: |a| a.math,
        load: Load::Shell { defer: false },
    },
    SiteScript {
        name: "search",
        dev_path: "src/assets/js/search.js",
        source: || include_str!("../../assets/js/search.js"),
        gate: |a| a.search,
        load: Load::Shell { defer: true },
    },
    SiteScript {
        name: "sidenotes",
        dev_path: "src/assets/js/sidenotes.js",
        source: || include_str!("../../assets/js/sidenotes.js"),
        gate: |a| a.has_footnotes,
        load: Load::Shell { defer: false },
    },
    SiteScript {
        name: "fullscreen",
        dev_path: "src/assets/js/fullscreen.js",
        source: || include_str!("../../assets/js/fullscreen.js"),
        gate: |a| a.media_pages,
        load: Load::Placed,
    },
    SiteScript {
        name: "share-card",
        dev_path: "src/assets/js/share-card.js",
        source: || include_str!("../../assets/js/share-card.js"),
        // Rides theme.js, which is unconditional: `selection-actions.ts` wires
        // the Share button on every page, so the chunk must be fetchable from
        // every page. Gating it would mean gating the button.
        gate: |_| true,
        load: Load::Lazy,
    },
    SiteScript {
        name: "hls-boot",
        dev_path: "src/assets/js/hls-boot.js",
        source: || include_str!("../../assets/js/hls-boot.js"),
        gate: |a| a.video_ladder,
        load: Load::Shell { defer: false },
    },
    SiteScript {
        // hls.js itself: 181 kb gzipped, which is why it is Lazy AND gated.
        // `hls-boot` fetches it only on a browser with MSE and no native HLS —
        // an iPhone, the connection this ladder was measured on, downloads
        // none of it.
        name: "hls",
        dev_path: "src/assets/js/hls.js",
        source: || &HLS_JS,
        gate: |a| a.video_ladder,
        load: Load::Lazy,
    },
];

/// Every script's bytes and content hash for this build, in table order.
///
/// Hashes are computed for ALL scripts, gated or not: they feed
/// `FacadeCache::asset_versions`, whose job is to notice a filename moving.
/// A gate flipping on must not be the first build that learns the hash.
pub struct ScriptAssets {
    resolved: Vec<(&'static SiteScript, Vec<u8>, String)>,
}

impl ScriptAssets {
    /// Read (dev) or take (release) every script's bytes and hash them.
    pub fn resolve() -> Self {
        Self {
            resolved: SITE_SCRIPTS
                .iter()
                .map(|s| {
                    let bytes = load_js_asset(s.dev_path, (s.source)()).into_bytes();
                    let hash = compute_binary_hash(&bytes);
                    (s, bytes, hash)
                })
                .collect(),
        }
    }

    /// This build's content hash for a script.
    ///
    /// # Panics
    /// If `name` is not in [`SITE_SCRIPTS`] — a typo'd asset name would
    /// otherwise silently produce a tag pointing at nothing.
    pub fn hash(&self, name: &str) -> &str {
        self.resolved
            .iter()
            .find(|(s, _, _)| s.name == name)
            .map(|(_, _, h)| h.as_str())
            .unwrap_or_else(|| panic!("no script named '{name}' in SITE_SCRIPTS"))
    }

    /// One script's `<script>` tag, gate and `defer` read from its row.
    ///
    /// For a template that places a tag itself — the media-collection page
    /// carries `search` next to its own `fullscreen` — so that the gate and
    /// the `defer` are still the table's and cannot drift from the shell's
    /// copy of the same script.
    ///
    /// # Panics
    /// If `name` is not in [`SITE_SCRIPTS`], like [`ScriptAssets::hash`] — or
    /// if it is [`Load::Lazy`]. A lazy chunk is an ES module: loaded by a
    /// classic `<script src>` its `export` is a syntax error that kills the
    /// file, so the tag would ship a broken page rather than a missing one.
    pub fn tag(&self, name: &str, assets: &SiteAssets, resolver: &PathResolver) -> String {
        let row = self
            .resolved
            .iter()
            .find(|(s, _, _)| s.name == name)
            .unwrap_or_else(|| panic!("no script named '{name}' in SITE_SCRIPTS"));
        assert!(
            row.0.load != Load::Lazy,
            "'{name}' is a lazy chunk — it reaches the browser through the runtime's \
             import(), never a <script src>"
        );
        tag_for(row.0, &row.2, assets, resolver)
    }

    /// The shell's whole runtime `<script>` block, in [`SITE_SCRIPTS`] order.
    ///
    /// One string for one template variable. Order is the table's, so moving
    /// a row moves the tag; a script whose gate is off contributes nothing,
    /// because with the markup it acts on suppressed it would have nothing to
    /// attach to.
    ///
    /// **Every gate here is SITE-level, never per-page**, and that is load
    /// bearing: the preview's morph-guard compares the script set across a
    /// navigation and forces a full reload when it differs, so a tag that came
    /// and went per page would turn every such navigation into a reload.
    pub fn shell_tags(&self, assets: &SiteAssets, resolver: &PathResolver) -> String {
        self.resolved
            .iter()
            .filter(|(script, _, _)| matches!(script.load, Load::Shell { .. }))
            .map(|(script, _, hash)| tag_for(script, hash, assets, resolver))
            .collect()
    }

    /// Every hash in table order — the input to `FacadeCache::asset_versions`.
    pub fn all_hashes(&self) -> Vec<&str> {
        self.resolved.iter().map(|(_, _, h)| h.as_str()).collect()
    }

    /// Write every enabled script into the build output.
    ///
    /// The gate decides the file and the `<script>` tag from one fact, so a
    /// tag can never name a file this skipped.
    pub fn emit(
        &self,
        assets: &SiteAssets,
        output_dir: &std::path::Path,
        pending: &mut crate::build::manifest::PendingManifest,
    ) -> Result<(), String> {
        use crate::build::context::BuildContext;
        use crate::build::manifest::HashBucket;
        for (script, bytes, hash) in &self.resolved {
            if !(script.gate)(assets) {
                continue;
            }
            BuildContext::for_render(output_dir, pending)
                .emit(
                    &ServedPath::for_runtime_js_hashed(script.name, hash)
                        .expect("SITE_SCRIPTS names are valid"),
                    bytes,
                    HashBucket::Files,
                )
                .map_err(|e| format!("Failed to emit _moss/js/{}.<hash>.js: {e}", script.name))?;
        }
        Ok(())
    }
}

/// The tag for one row: the row's gate decides whether there is one at all,
/// and only a shell row can carry `defer` (the placed ones ride templates that
/// spell their own attributes).
fn tag_for(
    script: &SiteScript,
    hash: &str,
    assets: &SiteAssets,
    resolver: &PathResolver,
) -> String {
    resolver.runtime_js_tag(
        script.name,
        hash,
        (script.gate)(assets),
        matches!(script.load, Load::Shell { defer: true }),
    )
}

// There is deliberately no `hash_of(name) -> String` convenience here. One
// existed for two hours and had zero callers: anything that needs a hash needs
// a whole build's worth of them, and [`ScriptAssets::resolve`] is that. A
// single-shot helper is a second hashing path, which is the duplication this
// module exists to delete — `render/html.rs` used to hold exactly one, hashing
// the embedded bytes while the emitter hashed the bytes on disk.

#[cfg(test)]
#[path = "scripts_tests.rs"]
mod tests;
