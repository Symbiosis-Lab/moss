//! Page facade fingerprints and the cross-build diff cache (Stage 4 of the
//! incremental-build work).
//!
//! A "facade" is a content fingerprint over every field of a `ParsedDocument`
//! another page's render could observe. It is computed from the `Debug`
//! representation rather than `Serialize`: several fields are
//! `#[serde(skip)]`/`#[specta(skip)]` for frontend reasons (they're derived
//! build state, not data the UI needs) while still being render-relevant —
//! `body_plan` is exactly what `generate_html` reads to produce a page's
//! HTML. `Debug` is unconditionally derived on `ParsedDocument` and, unlike
//! a hand-picked field list, cannot silently drop a newly-added field: if a
//! field doesn't implement `Debug`, the struct fails to compile, not the
//! facade silently to include it.
//!
//! `FacadeCache` persists the previous build's facades to
//! `.moss/build.nosync/cache/dep-cache.json` (`MossPaths::cache_dep_graph`) so a
//! save-triggered rebuild can diff against them — mirrors `HashIndex`
//! (`cache.rs`) for load/save shape, simplified because a facade IS the
//! content signature (no separate stat-based skip-hashing needed the way
//! `HashIndex` avoids re-hashing unchanged file bytes).
//!
//! The live consumer is the Stage 5b render skip in
//! `build/render/blocking.rs`, which pairs the two fingerprints here with
//! rules this module deliberately does NOT encode — see `compute_page_surface`.

use crate::build::types::ParsedDocument;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Compute a page's facade fingerprint (see module docs for what "facade" covers).
///
/// The body text does not go through `Debug`. `Debug for str` escapes every
/// character via `char::escape_debug`, which is a Unicode-table lookup per
/// char; on the CJK reference vault that rendered 13.1 MB at ~17 MB/s — 777 ms
/// of CPU per build across 226 pages, 97% of the whole incremental verdict.
/// Hashing the same bytes runs about two orders of magnitude faster. So the
/// three fields that carry the body (`content`, `html_content`, and the
/// `body_plan` segments that `html_content` is flattened from) are fed to the
/// digest as bytes, and only the page's remaining SHAPE is `Debug`-rendered.
///
/// What is hashed is unchanged — every byte of every field still reaches the
/// digest, so two documents differ here exactly when they differed before.
/// The blank-out discipline is unchanged too: a field added to
/// `ParsedDocument` still lands in the `Debug` half by default. A new field
/// carrying a large body would merely be slow again, never unhashed.
pub fn compute_page_facade(doc: &ParsedDocument) -> String {
    let mut doc = normalized(doc);
    let mut hasher = Sha256::new();
    // Length-prefixed, so no two adjacent fields can trade bytes across the
    // boundary and collide (`content="ab", html=""` vs `content="a", html="b"`).
    for text in [&doc.content, &doc.html_content] {
        hasher.update((text.len() as u64).to_le_bytes());
        hasher.update(text.as_bytes());
    }
    doc.content = String::new();
    doc.html_content = String::new();
    // The plan's SHAPE — segment boundaries, the lede split, typed grid cells
    // — still matters and still goes through `Debug`; only the emitted HTML
    // each segment carries is replaced by its own digest first.
    if let Some(plan) = doc.body_plan.as_mut() {
        plan.map_html(&|html| hash_str(html));
    }
    hasher.update(format!("{doc:?}").as_bytes());
    format!("{:x}", hasher.finalize())
}

/// SHA-256 over a value's `Debug` rendering — the one hashing convention this
/// feature uses, so a value that gains a field gains it in the fingerprint too
/// (or fails to compile). Shared with `build::parse_cache`, which fingerprints
/// Loop A's non-content inputs the same way.
///
/// The caller is responsible for feeding a value with a DETERMINISTIC `Debug`:
/// a `HashMap`'s iteration order is per-instance random, so map-shaped inputs
/// must be converted to `BTreeMap`/sorted `Vec` first. (Stage 5a shipped a bug
/// of exactly this shape — `ParsedDocument::cascade` was a `HashMap` and 208 of
/// 216 pages "changed" on a zero-edit rebuild.)
pub fn debug_hash<T: std::fmt::Debug>(value: &T) -> String {
    hash_str(&format!("{value:?}"))
}

/// SHA-256 over an already-rendered `Debug` string.
///
/// Split out of [`debug_hash`] so the surface can hash one rendering and
/// decompose that same rendering into per-field digests. Rendering twice
/// would let the fingerprint and its own explanation drift.
fn hash_str(rendered: &str) -> String {
    let digest = Sha256::digest(rendered.as_bytes());
    format!("{digest:x}")
}

/// Strip the one field that is not a function of the page's bytes.
///
/// `uid` is minted by `generate_uid()` — a fresh random value on every parse —
/// for any page whose source carries no `uid:`. moss normally writes the
/// minted value straight back into the frontmatter, so the churn lasts one
/// build. It lasts FOREVER for a page with no frontmatter block at all
/// (`footer.md` in the harbor/潮汐 reference vault is exactly this), because
/// there is nowhere to write it: that one page then reports as changed on
/// every build, and since slot pages are global invalidators it would force a
/// full render every single save, silently reducing this whole feature to a
/// no-op.
///
/// Dropping `uid` costs no detection: an authored `uid:` is also carried in
/// `raw_frontmatter`, which is fingerprinted, so a real uid edit still moves
/// both fingerprints.
fn normalized(doc: &ParsedDocument) -> ParsedDocument {
    let mut normalized = doc.clone();
    normalized.uid = None;
    normalized
}

/// Compute a page's **surface** fingerprint: everything in the facade except
/// the five excluded fields (four body-only, plus `lang` — see
/// `surface_debug`'s comment on `lang` for why it moved out).
///
/// The facade answers "does THIS page need re-rendering"; the surface answers
/// "could this page's change alter some OTHER page's HTML by a route the
/// dependency graph cannot see" (Stage 5b of the incremental-build work). Only a handful of fields
/// are excluded, and the exclusion is written as a blank-out on a clone rather
/// than a hand-picked include list, so a field added to `ParsedDocument`
/// lands in the surface by default — the rot direction is "more full
/// renders," never "silently skip a page that should have rendered."
///
/// **The surface alone is not a safety argument.** A 2026-07-31 audit of every
/// cross-page read of another document's body found three
/// routes by which one page's raw markdown reaches another page's HTML with
/// no metadata field moving and no link edge to follow:
///
/// 1. Listing hosts derive a listed child's card excerpt — and the listing's
///    style — from the child's `content` when the child has no frontmatter
///    `description:` (`build/folder_embed.rs`).
/// 2. The root homepage's `content` is the last fallback of the description
///    chain, so it lands in EVERY page's `<meta name="description">`.
/// 3. A slot page's rendered body (`footer.md`) is injected into every page.
///
/// So the surface is only half the gate. Routes 2 and 3 are closed by a
/// full-render fallback when a homepage or slot page changes. **Route 1 is
/// closed by a THIRD fingerprint** — the listing group digest
/// (`build/render/incremental/listing.rs`), which hashes the
/// *resolved* excerpt and the member-derived listing plan per folder group.
/// It used to be closed instead by rendering every listing host
/// unconditionally, which cost 114 of 214 pages on every save.
///
/// The listing projection is deliberately NOT folded in here. A non-empty
/// `surface_changed` forces a full site render, so folding the excerpt into
/// the surface would make every body edit a full render — strictly worse than
/// the predicate it replaces. The blast radius of a moved projection must be
/// *the group*, not the site; that difference is the entire point.
///
/// Note in particular that `ParsedDocument::description` is the FRONTMATTER
/// field only — the auto-extracted excerpt is computed at render time and is
/// invisible here.
pub fn compute_page_surface(doc: &ParsedDocument) -> String {
    hash_str(&surface_debug(doc))
}

/// The exact `Debug` rendering [`compute_page_surface`] hashes.
///
/// One function, two readers: the fingerprint hashes the whole string, and
/// [`surface_field_digests`] splits the same string into per-field digests so
/// a `SurfaceChanged` verdict can name the field that moved instead of only
/// the page. See the module docs on the blank-out discipline — a field added
/// to `ParsedDocument` lands in the surface, and in its explanation, with
/// nothing to maintain here.
fn surface_debug(doc: &ParsedDocument) -> String {
    let mut stripped = doc.clone();
    stripped.content = String::new();
    stripped.html_content = String::new();
    stripped.body_plan = None;
    stripped.outgoing_links = Vec::new();
    // Same class as `outgoing_links`: which files this page transcludes is a
    // fact about its own body, invisible to any other page's render. It stays
    // in the FACADE (a retargeted embed does change this page's HTML) and out
    // of the surface.
    stripped.embed_deps = Vec::new();
    // Also body-only, and the one that made this measurable. `hero_html` is
    // the page's own rendered hero, read by exactly one place —
    // `generate_html_inner`'s `hero_section: doc.and_then(|d| d.hero_html…)`
    // — for the page being rendered. No other page observes it. It stays in
    // the FACADE, because it is this page's HTML.
    //
    // Leaving it in the surface coupled the gate to image metadata: the hero
    // markup bakes the image's dimensions, LQIP and dominant colour, and the
    // background media phase fills those in AFTER the build that first sees
    // the image. So every hero-bearing page's surface moved as soon as its
    // cover was enriched, and one enriched cover full-renders the site.
    // Measured on the 223-page harbor vault: ~390ms per edit, on four of
    // six page classes.
    //
    // `hero_image_url` — the genuinely cross-page half, read by the homepage
    // — deliberately stays.
    stripped.hero_html = None;
    // `reading_time` is `word_count / 200` (`markdown/pipeline.rs`) — a
    // non-monotone function of the body, so it steps on any edit that crosses
    // a 200-word boundary. Same class as `hero_html`: an audit for this fix
    // found no cross-page
    // render consumer — every production read is the page's own
    // `ParsedDocument` (`pipeline.rs` sets it once per doc) and every other
    // hit is a test fixture. It stays in the FACADE and out of the surface.
    stripped.reading_time = 0;
    // `lang` — audited again after an earlier pass called `lang`
    // "genuinely cross-page-visible" and stopped there. Re-auditing every
    // cross-page read of another document's `lang` (not just this page's own —
    // that stays in the FACADE and re-renders this page as normal) found
    // exactly two channels, and both are already covered by something OTHER
    // than this field:
    //
    // 1. Translation siblings. `i18n::link::build_translation_links` snapshots
    //    each doc's own `lang` into every OTHER doc's `translations: Vec<TranslationLink>`
    //    at the reduce phase — recomputed on every build, before this
    //    fingerprint is taken (`render/blocking.rs` builds links, then calls
    //    `incremental::verdict::compute`). `translations` stays in the
    //    surface (nothing strips it), so a lang change already moves every
    //    sibling's OWN surface and they re-render — narrowly, not site-wide.
    //    `build/page/meta.rs::build_hreflang_link_tags` and
    //    `render/html.rs`'s switcher both read this same field, never a
    //    sibling's raw `.lang`.
    // 2. The nav language switcher (`render/lang_roots.rs::site_lang_roots` +
    //    `site_publishes_multiple_languages`, read by EVERY page's footer via
    //    `render/html.rs`'s `nav_builder.with_translations`) and the
    //    subscribe-form language sections
    //    (`features/email.rs::derive_language_sections`). Both walk the WHOLE
    //    corpus's `lang` — not just homepage roots — so no per-page surface
    //    field can carry this; it is closed instead by
    //    `render::lang_roots::lang_switcher_globals`, a build-global digest
    //    (`FullCause::LangGlobalsMoved`) hashing what those three functions
    //    actually compute, mirroring how the listing group digest closed the
    //    listing-host case above.
    //
    // What would break this: a NEW consumer that reads some OTHER document's
    // raw `.lang` outside `translations` and outside `lang_switcher_globals`'s
    // three functions. Grep `\.lang\b` across `build/` before trusting this
    // comment on a future change — the two channels above were exhaustive as
    // of this audit, not by construction.
    stripped.lang = crate::i18n::Language::default();
    format!("{:?}", normalized(&stripped))
}

/// Width of one per-field digest inside [`PageFingerprints::fields`].
///
/// Four bytes is plenty because this is a DIAGNOSTIC: a collision costs one
/// unnamed field in a log line. It can never change a render verdict — the
/// authority is `surface`, hashed whole and at full width.
const FIELD_DIGEST_HEX: usize = 8;

/// Split a derived-`Debug` rendering — `Name { a: 1, b: "x" }` — into its
/// top-level `(field, value)` pairs.
///
/// Returns `None` for anything that is not exactly that shape, so a
/// hand-written `Debug` impl (or a nested value reached by mistake) degrades
/// to "no attribution" rather than to a confident wrong one. Nesting is
/// tracked through `{}`, `[]` and `()`, and string literals are skipped whole
/// so a `, ` or a brace inside a page's own markdown cannot split a field.
fn split_debug_fields(rendered: &str) -> Option<Vec<(&str, &str)>> {
    let open = rendered.find(" { ")?;
    if !rendered.ends_with(" }") {
        return None;
    }
    let inner = rendered.get(open + 3..rendered.len() - 2)?;

    let bytes = inner.as_bytes();
    let mut chunks: Vec<&str> = Vec::new();
    let (mut depth, mut in_string, mut escaped, mut start) = (0usize, false, false, 0usize);
    for (i, &c) in bytes.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                in_string = false;
            }
            continue;
        }
        match c {
            b'"' => in_string = true,
            b'{' | b'[' | b'(' => depth += 1,
            b'}' | b']' | b')' => depth = depth.checked_sub(1)?,
            b',' if depth == 0 => {
                chunks.push(inner.get(start..i)?);
                start = i + 1;
            }
            _ => {}
        }
    }
    if depth != 0 || in_string {
        return None;
    }
    chunks.push(inner.get(start..)?);

    chunks
        .into_iter()
        .map(|chunk| {
            let (name, value) = chunk.split_once(": ")?;
            let name = name.trim();
            let valid = !name.is_empty()
                && !name.starts_with(|c: char| c.is_ascii_digit())
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            valid.then_some((name, value))
        })
        .collect()
}

/// The surface's field names, in the order [`PageFingerprints::fields`]
/// digests them.
///
/// Read off `ParsedDocument::default()`'s own `Debug` rather than a written
/// list, for the same reason the surface is a blank-out rather than an
/// include list: a field added to the struct appears here by itself. Empty
/// when the rendering does not parse, which reads downstream as "cannot
/// attribute".
pub fn surface_field_names() -> Vec<String> {
    split_debug_fields(&surface_debug(&ParsedDocument::default()))
        .map(|fields| fields.iter().map(|(name, _)| (*name).to_string()).collect())
        .unwrap_or_default()
}

/// Per-field digests over a surface rendering, concatenated in
/// [`surface_field_names`] order. Empty when the rendering does not parse.
fn surface_field_digests(rendered: &str) -> String {
    let Some(fields) = split_debug_fields(rendered) else {
        return String::new();
    };
    let mut out = String::with_capacity(fields.len() * FIELD_DIGEST_HEX);
    for (_, value) in fields {
        out.extend(hash_str(value).chars().take(FIELD_DIGEST_HEX));
    }
    out
}

/// The names of the surface fields that differ between two `fields` strings.
///
/// Returns nothing at all — rather than a guess — when either side is missing
/// (a cache written before this existed) or when the widths disagree, which
/// is what a `ParsedDocument` that gained a field between the two builds looks
/// like. A wrong field name is worse than none: this whole mechanism exists
/// because three separate diagnoses of one full render each named a different
/// wrong field.
pub fn moved_surface_fields(previous: &str, current: &str, names: &[String]) -> Vec<String> {
    let expected = names.len() * FIELD_DIGEST_HEX;
    if names.is_empty() || previous.len() != expected || current.len() != expected {
        return Vec::new();
    }
    previous
        .as_bytes()
        .chunks_exact(FIELD_DIGEST_HEX)
        .zip(current.as_bytes().chunks_exact(FIELD_DIGEST_HEX))
        .zip(names)
        .filter(|((prev, cur), _)| prev != cur)
        .map(|(_, name)| name.clone())
        .collect()
}

/// Both fingerprints for one page. Stored together so a single cache file
/// answers both questions the Stage 5b gate asks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageFingerprints {
    pub facade: String,
    pub surface: String,
    /// Per-field digests of this page's surface, concatenated in
    /// [`surface_field_names`] order — the explanation of `surface`, never an
    /// input to any verdict.
    ///
    /// It costs ~500 bytes per page in `dep-cache.json` (90 KB -> ~205 KB on
    /// the 223-page harbor vault) and buys a `SurfaceChanged` line that
    /// names the field. That trade was made after a full render whose cause
    /// took three diagnoses to find, two of them wrong, because the only
    /// evidence a whole-struct hash leaves is "something moved".
    ///
    /// `serde(default)` reads a cache written before this field existed as
    /// `""`, which [`moved_surface_fields`] declines to attribute rather than
    /// guessing over.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub fields: String,
}

impl PageFingerprints {
    pub fn of(doc: &ParsedDocument) -> Self {
        let surface = surface_debug(doc);
        Self {
            facade: compute_page_facade(doc),
            fields: surface_field_digests(&surface),
            surface: hash_str(&surface),
        }
    }
}

/// Cross-build cache of per-page fingerprints, keyed by source path.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FacadeCache {
    entries: HashMap<String, PageFingerprints>,
    /// A hash of every content-addressed asset version whose filename appears
    /// in emitted HTML — the stylesheet and moss's own JS bundles.
    ///
    /// A build-global, not a per-page fact, and the reason it lives here
    /// rather than in `PageFingerprints`: these assets are referenced by
    /// content-hashed filename (`_moss/style.<hash>.css`). When one changes,
    /// the new file is written and the old one is **deleted**
    /// (`remove_stale_files` unlinks anything absent from this build's
    /// manifest), so every page's `<link href>`/`<script src>` must be
    /// rewritten — including the pages whose own fingerprints did not move
    /// and which the Stage 5b skip would otherwise carry forward verbatim.
    /// The result would be a site that has lost its stylesheet everywhere
    /// except the page that happened to be edited.
    ///
    /// **Keyed on the version, not on what produced it.** An earlier draft
    /// hashed `SiteAssets` — which optional CSS partials this build ships —
    /// but that is one hop upstream of the fact that names the file.
    /// `css_version` also moves for a `tokens.json` edit, a `site.css` edit,
    /// a change to `minify_css`, or simply a newer moss binary, and every one
    /// of those has the same carried-forward-URL exposure. Hash the fact.
    ///
    /// Until now the only thing covering that residue was that the first
    /// build of a process is always `BuildTrigger::Full`, so a binary upgrade
    /// happened to full-render. That is incidental, undocumented as
    /// load-bearing, and exactly the implicit coupling this field removes.
    ///
    /// `serde(default)` so a cache written before this field existed loads as
    /// `""`, mismatches any real version, and forces exactly one full render.
    /// Fail-safe in the correct direction.
    #[serde(default)]
    asset_versions: String,
    /// Per-listing-group digests, keyed by
    /// `GroupKey::id()`.
    ///
    /// **Only digests cross builds.** The group graph itself is rebuilt from
    /// scratch every build — ~120 groups × 214 docs of `starts_with` is
    /// sub-millisecond — so there is nothing to maintain incrementally.
    ///
    /// `serde(default)` gives an empty map for a cache written before this
    /// field existed. Every group then reads as "moved", so every listing host
    /// renders exactly once and the next build settles — the same fail-safe
    /// direction as `asset_versions`.
    #[serde(default)]
    listing_groups: std::collections::BTreeMap<String, crate::build::render::incremental::listing::GroupDigest>,
    /// Digest of every body-derived value that reaches OTHER pages' HTML —
    /// the narrow replacement for "any global-invalidator page's facade
    /// moved". See `render::incremental::verdict::global_contributions` for
    /// what goes in and why that list is exhaustive.
    ///
    /// The point of narrowing it: a home page's whole body used to invalidate
    /// the site, so every save while editing the homepage re-rendered all 223
    /// pages of the harbor vault when the only thing other pages read from
    /// it is its extracted excerpt — which most edits do not touch at all.
    ///
    /// **Keyed by the page that contributes.** It was one digest over all of
    /// them, which told a reader that *a* global-invalidator page changed and
    /// left them to work out which — on a vault with a footer, a homepage and
    /// three language homes, that is a bisect. The parts already existed at
    /// the point the digest was taken; only the key was being thrown away.
    ///
    /// `serde(default)` gives an empty map for a cache written before this
    /// existed. Every current key then reads as moved, so exactly one full
    /// render happens and the next build settles — same fail-safe direction
    /// as `asset_versions`.
    #[serde(default)]
    global_contributions: std::collections::BTreeMap<String, String>,
    /// Build-global inputs to card rendering that no per-child projection
    /// covers (FM-4): `math`, typesetting, site language, `dir_overrides`,
    /// media dimensions, and the two `ProjectStructure` flags that feed the
    /// membership selector directly. A mismatch is a full-render bypass.
    ///
    /// **Keyed by input**, for the same reason as `global_contributions` and
    /// with a sharper edge: `image_files` and `video_files` are in here, so
    /// an image arriving or being enriched full-renders the site through this
    /// branch — which is the exact class of silent whole-site render this
    /// vault has produced twice. One digest could not say that; eight named
    /// ones can.
    #[serde(default)]
    listing_globals: std::collections::BTreeMap<String, String>,
    /// Build-global inputs to the nav language switcher and the subscribe-form
    /// language sections (`build::render::lang_roots::lang_switcher_globals`):
    /// `lang_roots`, `lang_multi`, `lang_email_sections`. A mismatch is a
    /// full-render bypass, same shape as `listing_globals` and for the same
    /// reason — both are rendered into every page, so there is no group
    /// narrower than the site to invalidate at.
    ///
    /// `serde(default)` gives an empty map for a cache written before this
    /// field existed. Every current key then reads as moved, so exactly one
    /// full render happens and the next build settles — same fail-safe
    /// direction as `asset_versions`.
    #[serde(default)]
    lang_globals: std::collections::BTreeMap<String, String>,
}

/// Which keys differ between two digest maps — sorted, and including keys
/// present on only one side.
///
/// Empty means identical, which is the only thing the old whole-map `!=`
/// could say. Everything else here exists so a full-render verdict can name
/// its witness: the digests were always per-part, and collapsing them into
/// one string discarded the only evidence a reader had.
fn moved_keys(
    previous: &std::collections::BTreeMap<String, String>,
    current: &std::collections::BTreeMap<String, String>,
) -> Vec<String> {
    let mut moved: Vec<String> = current
        .iter()
        .filter(|(k, v)| previous.get(*k) != Some(*v))
        .map(|(k, _)| k.clone())
        .collect();
    moved.extend(previous.keys().filter(|k| !current.contains_key(*k)).cloned());
    moved.sort();
    moved.dedup();
    moved
}

impl FacadeCache {
    /// Load a facade cache from a JSON file on disk. Returns an empty cache
    /// if the file doesn't exist or can't be parsed — same fail-open
    /// behavior as `HashIndex::load` (a missing/corrupt cache just means
    /// every page facade-diffs as new, i.e. falls back to full rebuild).
    pub fn load(path: &Path) -> Self {
        let Ok(data) = fs::read_to_string(path) else {
            return Self::default();
        };
        serde_json::from_str(&data).unwrap_or_default()
    }

    /// Save the facade cache to a JSON file on disk (atomic write, same
    /// `.pending`-suffixed temp-then-rename shape as `HashIndex::save` — see
    /// that method's doc comment for the iCloud-exclusion rationale).
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            crate::build::io_utils::create_output_dir_all(parent)
                .map_err(|e| format!("Failed to create dir {}: {}", parent.display(), e))?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize FacadeCache: {}", e))?;
        let tmp = path.with_extension(format!("json.pending.{}", uuid::Uuid::new_v4()));
        fs::write(&tmp, json.as_bytes())  // allow:raw_write the temp for this cache's own atomic save under .moss/cache
            .map_err(|e| format!("Failed to write {}: {}", tmp.display(), e))?;
        // allow:unlink rename into place outside staging
        fs::rename(&tmp, path)
            .map_err(|e| format!("Failed to rename {} -> {}: {}", tmp.display(), path.display(), e))
    }

    /// Build a cache directly from this build's computed fingerprints (the
    /// shape to `save()` after a build completes).
    pub fn from_facades(entries: HashMap<String, PageFingerprints>) -> Self {
        Self {
            entries,
            asset_versions: String::new(),
            listing_groups: std::collections::BTreeMap::new(),
            listing_globals: std::collections::BTreeMap::new(),
            global_contributions: std::collections::BTreeMap::new(),
            lang_globals: std::collections::BTreeMap::new(),
        }
    }

    /// Record this build's listing globals and per-group digests. Chained onto
    /// `from_facades` alongside `with_asset_versions`.
    pub fn with_listing(
        mut self,
        globals: std::collections::BTreeMap<String, String>,
        groups: std::collections::BTreeMap<
            String,
            crate::build::render::incremental::listing::GroupDigest,
        >,
    ) -> Self {
        self.listing_globals = globals;
        self.listing_groups = groups;
        self
    }

    /// This build's digest for one listing group, as the cache-writing build
    /// recorded it. `None` means the group is unknown to the cache, which the
    /// caller must read as "moved".
    pub fn listing_digest(
        &self,
        key: &crate::build::render::incremental::listing::GroupKey,
    ) -> Option<&crate::build::render::incremental::listing::GroupDigest> {
        self.listing_groups.get(&key.id())
    }

    /// True when a build-global input to card rendering moved. Callers must
    /// treat it as a full-render bypass — see the `listing_globals` field docs.
    pub fn listing_globals_changed(
        &self,
        current: &std::collections::BTreeMap<String, String>,
    ) -> Vec<String> {
        moved_keys(&self.listing_globals, current)
    }

    /// Record this build's language-switcher globals. Chained onto `from_facades`.
    pub fn with_lang_globals(
        mut self,
        globals: std::collections::BTreeMap<String, String>,
    ) -> Self {
        self.lang_globals = globals;
        self
    }

    /// True when a build-global input to the nav language switcher or the
    /// subscribe-form language sections moved. Callers must treat it as a
    /// full-render bypass — see the `lang_globals` field docs.
    pub fn lang_globals_changed(
        &self,
        current: &std::collections::BTreeMap<String, String>,
    ) -> Vec<String> {
        moved_keys(&self.lang_globals, current)
    }

    /// Record this build's global contributions. Chained onto `from_facades`.
    pub fn with_global_contributions(
        mut self,
        digests: std::collections::BTreeMap<String, String>,
    ) -> Self {
        self.global_contributions = digests;
        self
    }

    /// True when a body-derived value that other pages read moved. Callers
    /// must treat it as a full-render bypass — see the field docs.
    pub fn global_contributions_changed(
        &self,
        current: &std::collections::BTreeMap<String, String>,
    ) -> Vec<String> {
        moved_keys(&self.global_contributions, current)
    }

    /// Record this build's asset versions, so the next build can detect one
    /// moving. Chained onto `from_facades`.
    pub fn with_asset_versions(mut self, versions: String) -> Self {
        self.asset_versions = versions;
        self
    }

    /// True when this build's content-addressed asset filenames differ from
    /// the ones the cache-writing build emitted. Callers must treat it as a
    /// global invalidator — see the `asset_versions` field docs.
    pub fn asset_versions_changed(&self, current: &str) -> bool {
        self.asset_versions != current
    }

    /// True when this cache holds nothing — a cold or unreadable cache, which
    /// every caller must treat as "render everything."
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// True when this cache covers exactly the same set of source paths as
    /// `current`. A page appearing or disappearing is a structural change no
    /// per-page diff can reason about, so the caller falls back to a full
    /// render.
    pub fn covers_same_paths(&self, current: &HashMap<String, PageFingerprints>) -> bool {
        self.entries.len() == current.len() && current.keys().all(|p| self.entries.contains_key(p))
    }

    /// The paths [`covers_same_paths`] disagreed on — appeared in `current`, or
    /// vanished from it. Diagnostic only: "a page appeared or disappeared" is
    /// not actionable on a 226-page vault until it says which one.
    ///
    /// [`covers_same_paths`]: FacadeCache::covers_same_paths
    pub fn paths_symmetric_difference(
        &self,
        current: &HashMap<String, PageFingerprints>,
    ) -> Vec<String> {
        let appeared = current.keys().filter(|p| !self.entries.contains_key(*p));
        let vanished = self.entries.keys().filter(|p| !current.contains_key(*p));
        appeared.chain(vanished).cloned().collect()
    }

    /// Source paths whose facade in `current` differs from (or is absent
    /// from) this cache. A path present in `self` but absent from `current`
    /// (the source was deleted) is NOT reported here — deletions are a
    /// structural change the caller already has via `BuildTrigger`, not a
    /// facade-diff concern.
    pub fn changed_paths(&self, current: &HashMap<String, PageFingerprints>) -> Vec<String> {
        current
            .iter()
            .filter(|(path, fp)| self.entries.get(*path).map(|e| &e.facade) != Some(&fp.facade))
            .map(|(path, _)| path.clone())
            .collect()
    }

    /// Source paths whose **surface** differs — the cross-page-visible
    /// fingerprint. Non-empty means some page's change could reach another
    /// page's HTML by a route the dependency graph cannot see (site nav,
    /// breadcrumbs, series siblings, listings, translation counterparts),
    /// so the caller must render everything.
    pub fn surface_changed_paths(&self, current: &HashMap<String, PageFingerprints>) -> Vec<String> {
        current
            .iter()
            .filter(|(path, fp)| self.entries.get(*path).map(|e| &e.surface) != Some(&fp.surface))
            .map(|(path, _)| path.clone())
            .collect()
    }

    /// The per-field surface digests this cache recorded for `path`, for
    /// [`moved_surface_fields`] to diff against the current build's. `""` for
    /// a page the cache has never seen — which attributes to nothing, which is
    /// right: a page that is new has no field that "moved".
    pub fn surface_fields_of(&self, path: &str) -> &str {
        self.entries.get(path).map(|e| e.fields.as_str()).unwrap_or("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(facade: &str, surface: &str) -> PageFingerprints {
        PageFingerprints {
            facade: facade.to_string(),
            surface: surface.to_string(),
            fields: String::new(),
        }
    }

    /// Every name the splitter reports has to be a real field of
    /// `ParsedDocument`, and there have to be a lot of them. Asserting the
    /// exact list would go red on every field added to the struct — which is
    /// churn, not a signal — so this checks the SHAPE the attribution needs:
    /// enough fields to be a real decomposition, all of them identifiers, and
    /// the specific ones other code in this module reasons about by name.
    #[test]
    fn the_surface_decomposes_into_named_fields() {
        let names = surface_field_names();
        assert!(names.len() > 50, "only {} fields parsed out of the surface's Debug rendering — the splitter is not seeing the struct", names.len());
        for name in &names {
            assert!(
                name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "{name:?} is not an identifier — the Debug rendering did not split cleanly"
            );
        }
        for expected in ["title", "slug", "date", "hero_image_url", "cover", "raw_frontmatter"] {
            assert!(names.contains(&expected.to_string()), "surface field list is missing {expected}");
        }
    }

    /// The point of the whole mechanism: the verdict can say WHICH field.
    #[test]
    fn a_metadata_edit_names_the_field_that_moved() {
        let a = ParsedDocument { title: "A".to_string(), content: "body".to_string(), ..Default::default() };
        let mut b = a.clone();
        b.title = "A different title".to_string();

        let names = surface_field_names();
        let moved = moved_surface_fields(
            &PageFingerprints::of(&a).fields,
            &PageFingerprints::of(&b).fields,
            &names,
        );
        assert_eq!(moved, ["title"]);
    }

    /// A body edit moves the facade, not the surface — so there is no field to
    /// name, and naming one anyway would be the exact failure this replaces.
    #[test]
    fn a_body_edit_names_no_surface_field() {
        let a = ParsedDocument { content: "first body".to_string(), html_content: "<p>first</p>".to_string(), ..Default::default() };
        let b = ParsedDocument { content: "second body".to_string(), html_content: "<p>second</p>".to_string(), ..Default::default() };
        let names = surface_field_names();
        assert!(moved_surface_fields(
            &PageFingerprints::of(&a).fields,
            &PageFingerprints::of(&b).fields,
            &names,
        )
        .is_empty());
    }

    /// A page's own text is full of commas and braces. If those split the
    /// rendering, every field after the first comma in a body would misalign
    /// and the attribution would name fields at random.
    #[test]
    fn punctuation_in_a_field_value_does_not_split_it() {
        let a = ParsedDocument { title: "Hello, {world}: [a, b]".to_string(), ..Default::default() };
        let mut b = a.clone();
        b.title = "Hello, {world}: [a, c]".to_string();

        let names = surface_field_names();
        assert_eq!(names.len() * FIELD_DIGEST_HEX, PageFingerprints::of(&a).fields.len());
        assert_eq!(
            moved_surface_fields(
                &PageFingerprints::of(&a).fields,
                &PageFingerprints::of(&b).fields,
                &names,
            ),
            ["title"]
        );
    }

    /// A cache written by a binary whose `ParsedDocument` had a different
    /// field count must attribute NOTHING rather than shift every name by one.
    #[test]
    fn a_width_mismatch_attributes_nothing() {
        let names = surface_field_names();
        let current = PageFingerprints::of(&ParsedDocument::default()).fields;
        assert!(moved_surface_fields("", &current, &names).is_empty());
        assert!(moved_surface_fields(&current[..current.len() - FIELD_DIGEST_HEX], &current, &names).is_empty());
        assert!(moved_surface_fields(&current, &current, &[]).is_empty());
    }

    /// The splitter refuses anything that is not a derived struct `Debug`,
    /// because a confident wrong parse is worse than no attribution.
    #[test]
    fn the_splitter_refuses_shapes_it_cannot_read() {
        assert!(split_debug_fields("not a struct").is_none());
        assert!(split_debug_fields("Name { unterminated: \"x }").is_none());
        assert!(split_debug_fields("Name { 1nvalid: 2 }").is_none());
        assert!(split_debug_fields("Name { a: 1, b: [2, 3] }").is_some());
    }

    /// The global invalidator's mechanism. The end-to-end test in
    /// `tests/incremental_render_skip.rs` cannot reach the cases that matter
    /// here — a changed `tokens.json` or a different moss binary are not
    /// constructible from inside one test run — so this is where the
    /// invalidator itself is pinned.
    fn map(entries: &[(&str, &str)]) -> std::collections::BTreeMap<String, String> {
        entries.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect()
    }

    /// The point of keying these two digests: a full render can say WHICH
    /// input or page forced it. `image_files` is the one that earned it — a
    /// cover arriving or being enriched full-renders the site through the
    /// listing-globals branch, and a single combined hash reported that as
    /// "listing globals moved" and nothing more.
    #[test]
    fn a_moved_build_global_names_the_part_that_moved() {
        let before = map(&[("math", "a"), ("image_files", "b"), ("site_lang", "c")]);
        let cache = FacadeCache::from_facades(HashMap::new())
            .with_listing(before.clone(), std::collections::BTreeMap::new())
            .with_global_contributions(map(&[("footer.md", "f"), ("index.md", "i")]));

        assert!(cache.listing_globals_changed(&before).is_empty(), "identical inputs must not fire");
        assert_eq!(
            cache.listing_globals_changed(&map(&[("math", "a"), ("image_files", "MOVED"), ("site_lang", "c")])),
            ["image_files"]
        );
        assert_eq!(
            cache.global_contributions_changed(&map(&[("footer.md", "CHANGED"), ("index.md", "i")])),
            ["footer.md"]
        );
    }

    /// A part that vanishes is a move too — a deleted slot page stops
    /// contributing, and every page that spliced its body must re-render.
    /// Comparing only the current side would silently miss that.
    #[test]
    fn a_vanished_build_global_part_still_counts_as_moved() {
        let cache = FacadeCache::from_facades(HashMap::new())
            .with_global_contributions(map(&[("footer.md", "f"), ("index.md", "i")]));
        assert_eq!(cache.global_contributions_changed(&map(&[("index.md", "i")])), ["footer.md"]);
    }

    #[test]
    fn asset_versions_round_trip_and_detect_a_move() {
        let cache = FacadeCache::from_facades(HashMap::new()).with_asset_versions("v1".to_string());
        assert!(!cache.asset_versions_changed("v1"), "the same versions must not invalidate");
        assert!(cache.asset_versions_changed("v2"), "a moved asset must invalidate");
    }

    /// A cache written before the field existed must invalidate, not silently
    /// agree. `serde(default)` gives `""`, which mismatches every real hash —
    /// so the first build after an upgrade full-renders once and then settles.
    #[test]
    fn asset_versions_absent_from_a_legacy_cache_forces_one_full_render() {
        let legacy = r#"{"entries":{}}"#;
        let cache: FacadeCache = serde_json::from_str(legacy).expect("legacy cache must load");
        assert!(
            cache.asset_versions_changed("any-real-hash"),
            "a cache with no recorded asset versions must force a full render"
        );
    }

    #[test]
    fn facade_is_deterministic() {
        let doc = ParsedDocument { title: "Hello".to_string(), ..Default::default() };
        assert_eq!(compute_page_facade(&doc), compute_page_facade(&doc));
    }

    #[test]
    fn facade_changes_when_a_field_changes() {
        let a = ParsedDocument { title: "A".to_string(), ..Default::default() };
        let b = ParsedDocument { title: "B".to_string(), ..Default::default() };
        assert_ne!(compute_page_facade(&a), compute_page_facade(&b));
    }

    #[test]
    fn a_freshly_minted_uid_does_not_move_either_fingerprint() {
        // `footer.md` in the harbor/潮汐 reference vault has no frontmatter
        // block, so the uid moss mints for it can never be written back and is
        // random on every build. Left in the fingerprint it forced a full
        // render on every save forever — see `normalized`.
        let a = ParsedDocument { uid: Some("aaaaaaaa".to_string()), ..Default::default() };
        let b = ParsedDocument { uid: Some("bbbbbbbb".to_string()), ..Default::default() };
        assert_eq!(compute_page_facade(&a), compute_page_facade(&b));
        assert_eq!(compute_page_surface(&a), compute_page_surface(&b));
    }

    #[test]
    fn an_authored_uid_edit_still_moves_both_fingerprints() {
        // Detection is not lost: a persisted `uid:` also lives in the parsed
        // frontmatter map, which stays in both fingerprints.
        let with_uid = |uid: &str| {
            let mut raw = std::collections::BTreeMap::new();
            raw.insert("uid".to_string(), serde_json::Value::String(uid.to_string()));
            ParsedDocument { uid: Some(uid.to_string()), raw_frontmatter: raw, ..Default::default() }
        };
        let a = with_uid("aaaaaaaa");
        let b = with_uid("bbbbbbbb");
        assert_ne!(compute_page_facade(&a), compute_page_facade(&b));
        assert_ne!(compute_page_surface(&a), compute_page_surface(&b));
    }

    #[test]
    fn body_edit_changes_the_facade_but_not_the_surface() {
        let a = ParsedDocument { content: "one".to_string(), html_content: "<p>one</p>".to_string(), ..Default::default() };
        let b = ParsedDocument { content: "two".to_string(), html_content: "<p>two</p>".to_string(), ..Default::default() };
        assert_ne!(compute_page_facade(&a), compute_page_facade(&b));
        assert_eq!(compute_page_surface(&a), compute_page_surface(&b));
    }

    #[test]
    fn the_two_text_fields_cannot_trade_bytes_across_their_boundary() {
        // `content` and `html_content` are hashed as raw bytes rather than
        // Debug-rendered, so nothing quotes them apart any more. Each is
        // length-prefixed for that reason; without the prefix these two
        // documents would concatenate to the same byte stream, and one of them
        // could then be carried while stale.
        let a = ParsedDocument { content: "ab".to_string(), html_content: String::new(), ..Default::default() };
        let b = ParsedDocument { content: "a".to_string(), html_content: "b".to_string(), ..Default::default() };
        assert_ne!(compute_page_facade(&a), compute_page_facade(&b));
    }

    #[test]
    fn a_body_plan_html_edit_still_moves_the_facade() {
        // Each segment's HTML is replaced by its own digest before the plan is
        // Debug-rendered. That digest still has to discriminate: the plan is
        // what the folder-cover column is split from, and a stale one ships a
        // page whose lede does not match its body.
        let plan = |html: &str| ParsedDocument {
            body_plan: Some(crate::build::markdown::body_plan::BodyPlan {
                segments: vec![crate::build::markdown::body_plan::BodySegment::Html(html.to_string())],
                lede_segments: 1,
            }),
            ..Default::default()
        };
        assert_ne!(
            compute_page_facade(&plan("<p>one</p>")),
            compute_page_facade(&plan("<p>two</p>"))
        );
    }

    #[test]
    fn an_enriched_hero_moves_the_facade_but_not_the_surface() {
        // The hero markup bakes the cover's LQIP and dominant colour, which
        // the background media phase fills in one build late. In the surface
        // that made a settling image set full-render the site; only the page
        // itself reads its own `hero_html` (`render/html.rs`, `hero_section`).
        let hero = |h: &str| ParsedDocument { hero_html: Some(h.to_string()), ..Default::default() };
        let bare = hero("<section class=\"moss-hero\"><img src=\"c.webp\"></section>");
        let enriched = hero(
            "<section class=\"moss-hero\" style=\"--cover-color:#123456\">\
             <img src=\"c.webp\" style=\"background-image:url(data:image/jpeg;base64,zz)\"></section>",
        );
        assert_ne!(
            compute_page_facade(&bare),
            compute_page_facade(&enriched),
            "the page's own HTML did change — it must re-render"
        );
        assert_eq!(
            compute_page_surface(&bare),
            compute_page_surface(&enriched),
            "no other page reads this page's hero markup — it must not full-render the site"
        );
    }

    #[test]
    fn a_reading_time_step_moves_the_facade_but_not_the_surface() {
        // reading_time = word_count / 200 steps on any edit that crosses a
        // 200-word boundary. No production code reads another page's
        // reading_time (grep audit, see the comment in surface_debug), so a
        // step must re-render this page's own HTML but not full-render the site.
        let a = ParsedDocument { reading_time: 3, ..Default::default() };
        let b = ParsedDocument { reading_time: 4, ..Default::default() };
        assert_ne!(compute_page_facade(&a), compute_page_facade(&b), "reading_time is part of this page's own HTML");
        assert_eq!(
            compute_page_surface(&a),
            compute_page_surface(&b),
            "no other page reads this page's reading_time — it must not full-render the site"
        );
    }

    #[test]
    fn a_lang_change_moves_the_facade_but_not_the_surface() {
        // `lang` drives this page's own `<html lang>`, hreflang, and interface
        // strings — that must still cost its own re-render. What must NOT
        // happen any more is a full SITE render: the only two cross-page
        // reads of another doc's `lang` are `translations` (still in the
        // surface, moved on the affected siblings directly — see the next
        // test) and the two whole-corpus functions
        // `render::lang_roots::lang_switcher_globals` now fingerprints
        // separately (`verdict_tests.rs`).
        let a = ParsedDocument { lang: crate::i18n::Language::En, ..Default::default() };
        let b = ParsedDocument { lang: crate::i18n::Language::ZhHans, ..Default::default() };
        assert_ne!(compute_page_facade(&a), compute_page_facade(&b), "lang is part of this page's own HTML");
        assert_eq!(
            compute_page_surface(&a),
            compute_page_surface(&b),
            "an ordinary page's lang must not full-render the site"
        );
    }

    #[test]
    fn a_translations_change_still_moves_the_surface() {
        // The channel that makes the narrowing above safe: when a sibling's
        // lang changes, THIS page's own `translations` entry for that sibling
        // is recomputed to match (`i18n::link::build_translation_links`,
        // called every build before the facade diff). That field is NOT
        // blanked out, so the surface must still move — this is what re-
        // renders translation siblings without re-rendering the whole site.
        use crate::i18n::link::TranslationLink;
        let link = |lang: crate::i18n::Language| TranslationLink { lang_tag: lang.as_bcp47_attr().to_string(), url_path: "en/foo.html".to_string(), display_name: "EN" };
        let a = ParsedDocument { translations: vec![link(crate::i18n::Language::En)], ..Default::default() };
        let b = ParsedDocument { translations: vec![link(crate::i18n::Language::ZhHans)], ..Default::default() };
        assert_ne!(
            compute_page_surface(&a),
            compute_page_surface(&b),
            "a sibling's relabeled translation must still re-render THIS page"
        );
    }

    #[test]
    fn a_moved_hero_image_url_still_moves_the_surface() {
        // The cross-page half stays: the homepage reads other pages'
        // `hero_image_url`.
        let a = ParsedDocument { hero_image_url: Some("/a.webp".to_string()), ..Default::default() };
        let b = ParsedDocument { hero_image_url: Some("/b.webp".to_string()), ..Default::default() };
        assert_ne!(compute_page_surface(&a), compute_page_surface(&b));
    }

    #[test]
    fn metadata_edit_changes_the_surface() {
        // Every field another page's render can observe stays in the surface —
        // the label drives nav, breadcrumbs, listings and OG cards.
        let a = ParsedDocument { label: "A".to_string(), ..Default::default() };
        let b = ParsedDocument { label: "B".to_string(), ..Default::default() };
        assert_ne!(compute_page_surface(&a), compute_page_surface(&b));
    }

    #[test]
    fn frontmatter_description_change_moves_the_surface() {
        // The FRONTMATTER description is a materialized field, so it is caught
        // by the surface. The auto-extracted excerpt is not — it is derived at
        // render time from `content`, which is why blocking.rs renders every
        // listing host unconditionally.
        let a = ParsedDocument { content: "one".to_string(), description: Some("one".to_string()), ..Default::default() };
        let b = ParsedDocument { content: "two".to_string(), description: Some("two".to_string()), ..Default::default() };
        assert_ne!(compute_page_surface(&a), compute_page_surface(&b));
    }

    #[test]
    fn empty_cache_reports_every_current_path_as_changed() {
        let cache = FacadeCache::default();
        let mut current = HashMap::new();
        current.insert("a.md".to_string(), fp("hash-a", "s-a"));
        current.insert("b.md".to_string(), fp("hash-b", "s-b"));
        let mut changed = cache.changed_paths(&current);
        changed.sort();
        assert_eq!(changed, ["a.md", "b.md"]);
        assert!(cache.is_empty());
    }

    #[test]
    fn unchanged_facade_is_not_reported() {
        let mut entries = HashMap::new();
        entries.insert("a.md".to_string(), fp("hash-a", "s-a"));
        let cache = FacadeCache::from_facades(entries);
        let mut current = HashMap::new();
        current.insert("a.md".to_string(), fp("hash-a", "s-a"));
        assert!(cache.changed_paths(&current).is_empty());
        assert!(cache.surface_changed_paths(&current).is_empty());
        assert!(cache.covers_same_paths(&current));
    }

    #[test]
    fn changed_facade_is_reported() {
        let mut entries = HashMap::new();
        entries.insert("a.md".to_string(), fp("hash-a-old", "s-a"));
        let cache = FacadeCache::from_facades(entries);
        let mut current = HashMap::new();
        current.insert("a.md".to_string(), fp("hash-a-new", "s-a"));
        assert_eq!(cache.changed_paths(&current), ["a.md"]);
        // Body-only edit: the facade moved, the surface did not.
        assert!(cache.surface_changed_paths(&current).is_empty());
    }

    #[test]
    fn changed_surface_is_reported() {
        let mut entries = HashMap::new();
        entries.insert("a.md".to_string(), fp("hash-a-old", "s-a-old"));
        let cache = FacadeCache::from_facades(entries);
        let mut current = HashMap::new();
        current.insert("a.md".to_string(), fp("hash-a-new", "s-a-new"));
        assert_eq!(cache.surface_changed_paths(&current), ["a.md"]);
    }

    #[test]
    fn a_new_page_breaks_path_coverage() {
        let mut entries = HashMap::new();
        entries.insert("a.md".to_string(), fp("hash-a", "s-a"));
        let cache = FacadeCache::from_facades(entries);
        let mut current = HashMap::new();
        current.insert("a.md".to_string(), fp("hash-a", "s-a"));
        current.insert("b.md".to_string(), fp("hash-b", "s-b"));
        assert!(!cache.covers_same_paths(&current));
    }

    #[test]
    fn deleted_path_is_not_reported_as_changed() {
        // a.md existed in the previous build's cache but isn't in `current`
        // (the caller already has deletion info via BuildTrigger — see
        // module docs).
        let mut entries = HashMap::new();
        entries.insert("a.md".to_string(), fp("hash-a", "s-a"));
        let cache = FacadeCache::from_facades(entries);
        let current: HashMap<String, PageFingerprints> = HashMap::new();
        assert!(cache.changed_paths(&current).is_empty());
        // ...but it does break path coverage, so the caller falls back to full.
        assert!(!cache.covers_same_paths(&current));
    }

    #[test]
    fn load_missing_file_returns_empty_cache() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        let cache = FacadeCache::load(&path);
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dep-cache.json");
        let mut entries = HashMap::new();
        entries.insert("a.md".to_string(), fp("hash-a", "s-a"));
        let cache = FacadeCache::from_facades(entries);
        cache.save(&path).unwrap();
        let loaded = FacadeCache::load(&path);
        assert_eq!(loaded.entries.get("a.md"), Some(&fp("hash-a", "s-a")));
    }
}
