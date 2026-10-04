//! Content slots system for plugin content injection.
//!
//! Replaces the enhance hook system with structured, named injection points.
//! Plugins declare what content to inject and where; Rust handles insertion
//! during template rendering.
//!
//! Design references:
//! - Gatsby SSR: `setHeadComponents()`, `setPostBodyComponents()` — named slot setters
//!   https://www.gatsbyjs.com/docs/reference/config-files/gatsby-ssr/
//! - Hugo: file-based hooks at `layouts/partials/hooks/{head-end,body-end}/`
//!   https://docs.hugoblox.com/reference/extend/
//! - Vite: `transformIndexHtml` returns `HtmlTagDescriptor[]` with `injectTo: 'head' | 'body'`
//!   https://vite.dev/guide/api-plugin.html#transformindexhtml
//! - WordPress: `add_action('wp_head')`, `add_action('wp_footer')` with priority ordering
//!   https://developer.wordpress.org/reference/hooks/wp_head/
//! - Astro: `injectScript('head-inline' | 'before-hydration' | 'page')`
//!   https://docs.astro.build/en/reference/integrations-reference/

use crate::build::lifecycle::cas_heal::{Placement, StagedLinks};
use crate::build::outcome::{io_stop, BuildStopped};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Known slot marker names used in HTML templates.
///
/// This is the **template-marker / plugin-emit** surface — every
/// `<!-- slot:NAME -->` marker the build pipeline knows how to find and
/// replace. It is a strict superset of the frontmatter-targetable surface
/// modeled by `crate::build::slots::Slot`: the four article-area slots
/// (`after-title`, `before-article-end`, `after-article`, `body-end`)
/// have no frontmatter contract, only a plugin-emit / native-injection
/// contract, so they are not (yet) modeled in the `Slot` enum. Folding
/// them in is plausible future work; documenting the split is enough
/// for now.
///
/// Slot content declaration from a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EnhanceContent {
    /// Same HTML injected on every page.
    #[serde(rename = "static")]
    Static { html: String },
    /// Different HTML per page, keyed by page path.
    #[serde(rename = "per-page")]
    PerPage { pages: HashMap<String, String> },
    /// Different HTML per language tree, selected at injection time from the
    /// page's path. `default` applies to pages outside any language tree (the
    /// root / site-default language) and to languages absent from `by_lang`;
    /// `by_lang` maps a language-tree folder name (e.g. `"zh-hans"`) to its
    /// HTML. Native-only today (used for per-language `footer.md`); plugins
    /// continue to emit `static`/`per-page`.
    #[serde(rename = "per-language")]
    PerLanguage {
        default: Option<String>,
        by_lang: HashMap<String, String>,
    },
}

/// Result from a plugin's `enhance()` call (structured slot declarations).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhanceResult {
    pub success: bool,
    pub slots: HashMap<String, EnhanceContent>,
}

/// A single entry in the resolved slots, tracking priority for ordering.
/// Tie-breaking: when two plugins share a priority, alphabetical plugin name wins.
/// (Per Gemini review recommendation for deterministic ordering.)
#[derive(Debug, Clone, Serialize)]
struct ResolvedEntry {
    priority: u32,
    plugin_name: String,
    content: EnhanceContent,
}

/// Merged slot content from all plugins, ready for template injection.
/// Entries per slot are sorted by plugin priority (lower = first).
///
/// `Serialize` is used only to derive a cache-params hash
/// in [`resolved_slots_hash`] — never round-tripped, so no `Deserialize`.
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedSlots {
    entries: HashMap<String, Vec<ResolvedEntry>>,
    /// Names of `build::emit::feature_styles` sheets some entry `<link>`s to.
    ///
    /// Slot resolution can NAME a content-hashed file (the hash is a function
    /// of embedded bytes alone) but has no build context to WRITE one. This
    /// carries the decision forward to `apply_slots_to_stage_and_manifest`,
    /// which has the `PendingManifest`. Without it the linked file would never
    /// be emitted and every page would 404 its stylesheet.
    feature_styles: Vec<&'static str>,
}

impl ResolvedSlots {
    /// Create empty resolved slots (no plugin content).
    pub fn empty() -> Self {
        Self {
            entries: HashMap::new(),
            feature_styles: Vec::new(),
        }
    }

    /// Record that a feature stylesheet is linked, and return its `<link>` tag.
    ///
    /// The one way to reference a feature stylesheet: taking the tag and
    /// recording the name are the same act, so a caller cannot link a sheet
    /// the build then fails to emit.
    pub fn link_feature_style(&mut self, name: &'static str) -> String {
        if !self.feature_styles.contains(&name) {
            self.feature_styles.push(name);
        }
        crate::build::emit::feature_styles::link_tag(name)
    }

    /// The feature stylesheets this build must emit.
    pub fn feature_styles(&self) -> &[&'static str] {
        &self.feature_styles
    }

    /// Check if there are no slot entries at all.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Merge all entries from another `ResolvedSlots` into self.
    /// Entries are re-sorted by priority after merge.
    pub fn merge_all(&mut self, other: ResolvedSlots) {
        for name in other.feature_styles {
            if !self.feature_styles.contains(&name) {
                self.feature_styles.push(name);
            }
        }
        for (slot_name, mut entries) in other.entries {
            let target = self.entries.entry(slot_name).or_default();
            target.append(&mut entries);
            target.sort_by(|a, b| {
                a.priority
                    .cmp(&b.priority)
                    .then_with(|| a.plugin_name.cmp(&b.plugin_name))
            });
        }
    }

    /// Merge a plugin's slot result into the resolved slots.
    /// `priority` controls ordering: lower values are inserted first.
    /// `plugin_name` is used for deterministic tie-breaking (alphabetical).
    pub fn merge(&mut self, result: &EnhanceResult, priority: u32, plugin_name: &str) {
        if !result.success {
            log::warn!("Plugin '{}' returned success=false for slot content; skipping its slots", plugin_name);
            return;
        }
        for (slot_name, content) in &result.slots {
            let entry = ResolvedEntry {
                priority,
                plugin_name: plugin_name.to_string(),
                content: content.clone(),
            };
            let entries = self.entries.entry(slot_name.clone()).or_default();
            entries.push(entry);
            entries.sort_by(|a, b| a.priority.cmp(&b.priority).then_with(|| a.plugin_name.cmp(&b.plugin_name)));
        }
    }

    /// Get the combined HTML for a slot on a given page.
    /// Returns `None` if no content exists for this slot/page combination.
    ///
    /// Normalizes page_path for lookup: plugins key by url_path from article_map
    /// (e.g., `文字/article/`) while the renderer uses the output file path
    /// (e.g., `文字/article/index.html`). We try both forms.
    pub fn get_html(&self, slot: &str, page_path: &str) -> Option<String> {
        let entries = self.entries.get(slot)?;

        // Normalize: strip trailing index.html to get the url_path form
        let normalized = if page_path.ends_with("/index.html") {
            page_path.strip_suffix("index.html").unwrap()
        } else {
            page_path
        };

        let mut parts = Vec::new();
        for entry in entries {
            match &entry.content {
                EnhanceContent::Static { html } => parts.push(html.as_str()),
                EnhanceContent::PerPage { pages } => {
                    // Try normalized form first, then original
                    if let Some(html) = pages.get(normalized).or_else(|| pages.get(page_path)) {
                        parts.push(html.as_str());
                    }
                }
                EnhanceContent::PerLanguage { default, by_lang } => {
                    // Select by the page path's language-tree prefix
                    // (e.g. `zh-hans/index.html` → "zh-hans"), falling back to
                    // `default` for root pages and languages without an entry.
                    let lang = moss_core::home::lang_tree_prefix(page_path)
                        .map(|s| s.to_lowercase());
                    let chosen = lang
                        .as_deref()
                        .and_then(|l| by_lang.get(l))
                        .or(default.as_ref());
                    if let Some(html) = chosen {
                        parts.push(html.as_str());
                    }
                }
            }
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n"))
        }
    }
}

/// Replace `<!-- slot:NAME -->` markers in HTML with resolved slot content.
///
/// Known slot markers are replaced with content (or removed if empty).
/// Unknown markers are left untouched.
pub fn inject_slots(html: &str, slots: &ResolvedSlots, page_path: &str) -> String {
    let mut result = html.to_string();
    // Whether footer-left / footer-end resolved to real content THIS pass —
    // known here, from the same `slots.get_html` call the marker loop below
    // already makes, rather than re-derived later from the assembled HTML.
    // `strip_empty_footer` combines these with the render-time
    // `<!-- footer:has-content -->` marker to decide the whole element,
    // never by scanning `result` for visible content.
    let mut footer_left_has_content = false;
    let mut footer_end_has_content = false;
    for slot_name in crate::build::slots::Slot::ALL.map(|s| s.as_str()) {
        let marker = format!("<!-- slot:{} -->", slot_name);
        let has_marker = result.contains(&marker);
        if let Some(content) = slots.get_html(slot_name, page_path) {
            let has_content = !content.trim().is_empty();
            match slot_name {
                "footer-left" => footer_left_has_content = has_content,
                "footer-end" => footer_end_has_content = has_content,
                _ => {}
            }
            if has_marker {
                log::trace!(target: "plugin", "inject_slots: '{}' slot '{}' → {} bytes (marker found)", page_path, slot_name, content.len());
                result = result.replace(&marker, &content);
            } else {
                log::trace!(target: "plugin", "inject_slots: '{}' slot '{}' → content available but NO marker in template", page_path, slot_name);
            }
        } else if has_marker {
            log::trace!(target: "plugin", "inject_slots: '{}' slot '{}' → no content (marker removed)", page_path, slot_name);
            result = result.replace(&marker, "");
        }
    }
    strip_empty_footer(result, footer_left_has_content, footer_end_has_content)
}

/// Remove the whole chrome `<footer>` element when it has nothing left to
/// show; otherwise strip the bookkeeping markers back out so shipped HTML
/// carries no trace of them. The element is found by its own
/// `<!-- moss:footer -->` / `<!-- /moss:footer -->` bounds — fixed,
/// code-controlled tokens, the same trust model `<!-- slot:NAME -->` already
/// uses — never by searching for a literal `<footer` tag, which an author's
/// own raw-HTML `<footer>` elsewhere on the page (moss passes body HTML
/// through verbatim) could also match.
///
/// The keep/drop decision itself is three known facts, never a scan of
/// `html` for visible content: `footer_left_has_content` /
/// `footer_end_has_content` (from `inject_slots`'s own `slots.get_html`
/// calls above — did `footer.md`/a `slot: footer-left` page, or the
/// auto-injected subscribe form/a plugin widget, resolve to anything this
/// pass), and the `<!-- footer:has-content -->` marker, the render-time
/// third fact: whether `generate_footer` emitted any default links or a feed
/// link, which is baked into `html` already and never re-resolved here — the
/// marker is only how it survives the render → slot-injection boundary.
///
/// moss's colophon sits as `<footer>`'s own template sibling, not inside it,
/// so it is untouched either way.
fn strip_empty_footer(html: String, footer_left_has_content: bool, footer_end_has_content: bool) -> String {
    const OPEN: &str = "<!-- moss:footer -->";
    const HAS_CONTENT: &str = "<!-- footer:has-content -->";
    const CLOSE: &str = "<!-- /moss:footer -->";

    let Some(open_start) = html.find(OPEN) else {
        return html;
    };
    let Some(close_rel) = html[open_start..].find(CLOSE) else {
        return html;
    };
    let close_end = open_start + close_rel + CLOSE.len();

    let region = &html[open_start..close_end];
    let has_default_content = region.contains(HAS_CONTENT);

    if footer_left_has_content || footer_end_has_content || has_default_content {
        // Keep the element; erase the bookkeeping markers (each a plain
        // substring removal — `HAS_CONTENT` is a no-op when absent) so the
        // shipped bytes are exactly what `generate_footer` would have
        // produced with no sentinels at all.
        let kept = region.replacen(OPEN, "", 1).replacen(HAS_CONTENT, "", 1).replacen(CLOSE, "", 1);
        return format!("{}{}{}", &html[..open_start], kept, &html[close_end..]);
    }

    format!("{}{}", &html[..open_start], &html[close_end..])
}

/// Known slot markers still present in HTML that is about to ship.
///
/// Every `<!-- slot:NAME -->` for a known slot MUST be resolved (replaced or
/// stripped) by `inject_slots` before output reaches a browser. A residual
/// marker means slot injection didn't run on this file — a coverage / serve-path
/// bug — and the raw `<!-- slot:NAME -->` comment would render inertly in the
/// page. Used by the slot-injection pass to emit a WARN so the condition
/// is never silent. Unknown (non-moss) `slot:` comments are ignored.
fn residual_known_slot_markers(html: &str) -> Vec<&'static str> {
    if !html.contains("<!-- slot:") {
        return Vec::new();
    }
    crate::build::slots::Slot::ALL
        .into_iter()
        .map(|slot| slot.as_str())
        .filter(|name| html.contains(&format!("<!-- slot:{} -->", name)))
        .collect()
}

/// HTML files under `dir` eligible for slot injection: real files, `.html`
/// extension, excluding JupyterLite assets (third-party HTML that must not
/// be modified). Used by [`inject_slots_into_directory_cached`].
fn walk_html_files(dir: &std::path::Path) -> impl Iterator<Item = walkdir::DirEntry> + '_ {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "html"))
        .filter(move |e| {
            !e.path()
                .strip_prefix(dir)
                .unwrap_or(e.path())
                .starts_with("jupyter")
        })
}

/// A build-output path, relative to the stage directory, in the `/`-separated
/// build convention. Windows `strip_prefix` yields backslash-separated paths,
/// so this always normalizes.
fn page_path_for(dir: &std::path::Path, entry_path: &std::path::Path) -> String {
    let rel_path = entry_path.strip_prefix(dir).unwrap_or(entry_path);
    moss_core::slug::normalize_separators(&rel_path.to_string_lossy())
}

/// Content-free page id: a one-way hash of the build-output path, NOT the
/// path itself. The path is a slug derived from the user's title — user
/// content the Send-Logs scrub (emails/cards/keys only) would not catch. The
/// hash still correlates repeated WARNs for one page. Emits a WARN so a
/// residual known slot marker is never silent — see `residual_known_slot_markers`
/// for why this must fire even on a `inject_slots_into_directory_cached` hit.
fn log_residual_slot_markers(page_path: &str, residual: &[&str]) {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    page_path.hash(&mut h);
    let page_id = h.finish();
    log::warn!(
        target: "slots",
        "unresolved slot marker(s) {:?} shipped in page#{:016x} — slot injection did not cover this file (raw marker will render; in attribute position it leaks a literal \">\")",
        residual,
        page_id,
    );
}

/// Transform name used for cached `html/slots` output in the `TransformCache`.
const SLOT_INJECT_TRANSFORM: &str = "html/slots";

/// Cached record for an `html/slots` transform: the page's shipped bytes (the
/// blob holding them and their manifest hash), whether injection changed them,
/// and the residual-marker scan result so the `residual_known_slot_markers`
/// diagnostic can still fire on a cache hit (prior-art
/// requirement from `4dca6d3fc`: a coverage bug once shipped a raw marker to a
/// real user's browser — caching must never make that WARN skippable).
///
/// The blob and hash are recorded for EVERY outcome, a no-op included, so a hit
/// hands back the page's [`SlotInjectionReceipt`] without touching its bytes.
/// Recomputing them is a SHA-256, three strip regexes and an xxh3 per page,
/// inside the stage-write lock, scaling with the corpus — and a watch rebuild
/// carries most pages, so the no-op is the outcome most hits have.
///
/// `manifest_hash` is the hash of `content_oid`'s bytes after
/// `ship::apply_transform`. It has to be stored because neither arm can recover
/// it later. The stage holds pre-strip bytes by design (the preview wants the
/// annotations the published site does not), and `content_oid` is SHA-256 of the
/// *un*stripped content — the wrong algorithm over the wrong bytes. Storing it
/// at miss time, when the bytes are in hand, is what lets the registration site
/// stop reading the stage back.
///
/// `rewritten` says whether those bytes differ from the freshly-rendered page
/// this record is keyed on: a hit `link_to`s them into place when they do, and
/// leaves the render's own bytes alone when they do not.
///
/// A shape change here needs no version gate: `read_slot_inject_record` returns
/// `None` on a deserialize failure and falls through to a live re-run — which is
/// how records written before these fields existed are retired.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SlotInjectRecord {
    content_oid: String,
    manifest_hash: String,
    rewritten: bool,
    residual: Vec<String>,
}

/// What the slot pass reports about one page it scanned: the bytes the page
/// ships as, in the two forms the manifest needs.
///
/// One per scanned page, rewritten or not — a page injection left alone still
/// needs a CAS object for `ship_phase` to read instead of the shared stage
/// path. The pass scans everything in the stage, so a receipt is a claim about
/// bytes on disk, not a claim that this build produced them: the consumer
/// (`emit::slots::apply_to_stage_and_manifest`) decides which receipts belong
/// to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotInjectionReceipt {
    /// Stage-relative, `/`-separated.
    pub page_path: String,
    /// Hash of the bytes the SITE serves: the staged bytes after
    /// `ship::apply_transform`.
    pub manifest_hash: String,
    /// The CAS object holding the staged bytes exactly as they stand after
    /// injection (before `apply_transform`, which `ship_phase` applies on read).
    /// `None` when the store could not take them — the page then ships from the
    /// stage under a fingerprint, as before ship-by-oid existed.
    pub content_oid: Option<String>,
}

/// Canonical hash of the fully-resolved slot content, for the `slots_hash`
/// cache-params field.
///
/// The bytes hashed must not depend on `HashMap` iteration order, which is
/// random per instance: `ResolvedSlots` is rebuilt from scratch every build, so
/// an order-dependent hash differs build to build and the slot-injection cache
/// misses on pages that did not change.
///
/// `serde_json::to_vec(slots)` is NOT order-independent — it walks each
/// `HashMap` in its own iteration order and writes keys as it meets them.
/// Going through [`serde_json::Value`] first is what sorts: `Value::Object`
/// is a `BTreeMap` unless serde_json's `preserve_order` feature is on (nothing
/// in the build graph enables it), and every nested map goes through the same
/// conversion. If a dependency ever turns that feature on,
/// `slots_hash_does_not_depend_on_map_iteration_order` fails.
fn resolved_slots_hash(slots: &ResolvedSlots) -> String {
    let bytes = serde_json::to_value(slots)
        .and_then(|canonical| serde_json::to_vec(&canonical))
        .unwrap_or_default();
    crate::build::assets::paths::compute_binary_hash(&bytes)
}

fn read_slot_inject_record(
    object_store: &crate::build::cache::ObjectStore,
    record_oid: &str,
) -> Option<SlotInjectRecord> {
    let blob_path = object_store.get_path(record_oid)?;
    let raw = std::fs::read(blob_path).ok()?;
    serde_json::from_slice(&raw).ok()
}

fn write_slot_inject_record(
    object_store: &crate::build::cache::ObjectStore,
    transform_cache: &crate::build::cache::TransformCache,
    source_oid: &str,
    source_size: u64,
    params: &serde_json::Value,
    record: &SlotInjectRecord,
) {
    let json_bytes = match serde_json::to_vec(record) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("Failed to serialize html/slots record: {}", e);
            return;
        }
    };
    let record_oid = match object_store.store_bytes(&json_bytes) {
        Ok(oid) => oid,
        Err(e) => {
            log::warn!("Failed to store html/slots record blob: {}", e);
            return;
        }
    };
    let entry = crate::build::cache::TransformEntry {
        oid: record_oid,
        size: json_bytes.len() as u64,
        params: params.clone(),
    };
    let merged = transform_cache.merge(source_oid, source_size, crate::build::cache::RecordMode::Request, |rec| {
        rec.transforms.insert(SLOT_INJECT_TRANSFORM.to_string(), entry);
    });
    if let Err(e) = merged {
        log::warn!("Failed to write html/slots transform record: {}", e);
    }
}

/// Pages the object store could not take during one pass.
///
/// A full or unwritable store must cost a page its CAS object, never the build,
/// so storing fails OPEN here and the reason is kept for one report at the end:
/// a store that is full fails every page, and a WARN per page is a corpus's
/// worth of the same line.
#[derive(Default)]
struct StoreFailures {
    pages: usize,
    first_reason: Option<String>,
}

impl StoreFailures {
    /// `store_bytes`, with `None` for a page the store could not take.
    fn store(&mut self, object_store: &crate::build::cache::ObjectStore, bytes: &[u8]) -> Option<String> {
        match object_store.store_bytes(bytes) {
            Ok(oid) => Some(oid),
            Err(reason) => {
                self.pages += 1;
                self.first_reason.get_or_insert(reason);
                None
            }
        }
    }

    fn report(self) {
        if let Some(reason) = self.first_reason {
            log::warn!(
                target: "slots",
                "{} page(s) could not be stored in the content store ({}); they ship from the stage under a fingerprint instead of an immutable copy",
                self.pages,
                reason,
            );
        }
    }
}

/// Put a page's final bytes at `path`: link the blob `oid` unless `staged` vouches
/// the file already is it, else write `bytes` (no blob, or the link failed: a real
/// write's failure is one `io_stop` can classify). `true` when it wrote.
fn place_page(root: &std::path::Path, path: &std::path::Path, oid: Option<&str>, bytes: Option<&str>,
    objects: &crate::build::cache::ObjectStore, staged: &mut StagedLinks) -> Result<bool, BuildStopped> {
    if let Some(oid) = oid {
        match staged.link(objects, oid, path) {
            Placement::Held => return Ok(false),
            Placement::Linked => return Ok(true),
            Placement::Unverified(e) => return Err(io_stop(root, "check staged page", path, e)),
            Placement::Failed(e) if bytes.is_none() => return Err(e.into()),
            Placement::Failed(_) => {}
        }
    }
    let bytes = bytes.ok_or_else(|| format!("no bytes and no blob to stage {}", path.display()))?;
    // `write_output`, not `fs::write`: an `O_TRUNC` open of a stage page the
    // sync client evicted fails EDEADLK.
    crate::build::io_utils::write_output(path, bytes.as_bytes())
        .map_err(|e| io_stop(root, "write injected page", path, e))?;
    Ok(true)
}

/// The slot-injection pass over the stage, used by the live pipeline.
///
/// Its pages are the ones this build rendered, still in memory (`rendered`: the
/// pass is their one writer), and every other `.html` already in the stage. The
/// cache keys on content: `source_oid` is `xxh3:<hash of the page as rendered>`;
/// `params` covers `page_path` plus a hash of the ENTIRE resolved slot set
/// ([`resolved_slots_hash`]), so it can never alias a change that lands
/// somewhere else in `ResolvedSlots`. A hit names the blob of the final bytes
/// and re-emits the cached residual-marker WARN (a diagnostic must never become
/// cache-skippable); a miss injects once and stores the result. Pages reach the
/// stage through `staged`, which skips a file it vouches already holds the blob.
///
/// Returns one [`SlotInjectionReceipt`] per page scanned, whichever way it went.
/// A CAS that cannot take a page's bytes costs that page its `content_oid`, never
/// the build: a full or unwritable object store must not fail a build that
/// succeeds today for a page that never needed a blob.
pub(crate) fn inject_slots_into_directory_cached(
    // The VAULT root, not `dir` (which is the stage below it): `io_stop` asks
    // the watcher predicates about the path inside the vault.
    root: &std::path::Path,
    dir: &std::path::Path,
    slots: &ResolvedSlots,
    object_store: &crate::build::cache::ObjectStore,
    transform_cache: &crate::build::cache::TransformCache,
    rendered: std::collections::BTreeMap<String, String>,
    staged: &mut StagedLinks,
    // `BuildStopped`: this reads the stage back, so a cloud eviction here must
    // stay distinguishable from a plugin returning garbage. See `build::outcome`.
) -> Result<Vec<SlotInjectionReceipt>, BuildStopped> {
    let slots_hash = resolved_slots_hash(slots);
    let mut receipts = Vec::new();
    let mut scanned = 0usize;
    let mut rewritten = 0usize;
    let mut written = 0usize;
    let mut residual_files = 0usize;
    let mut store_failures = StoreFailures::default();
    // The part that scales with the CORPUS (walk, read, hash: paid even when
    // nothing changed); the rest, inject and write, scales with the DELTA.
    let mut walk_ms = std::time::Duration::ZERO;
    let t_pass = std::time::Instant::now();

    let t_walk = std::time::Instant::now();
    let staged_only: Vec<String> = walk_html_files(dir)
        .map(|entry| page_path_for(dir, entry.path()))
        .filter(|page_path| !rendered.contains_key(page_path))
        .collect();
    walk_ms += t_walk.elapsed();
    let pages = rendered.into_iter().map(|(page_path, html)| (page_path, Some(html)));
    for (page_path, unwritten) in pages.chain(staged_only.into_iter().map(|page_path| (page_path, None))) {
        let t_walk = std::time::Instant::now();
        let path = dir.join(&page_path);
        // A stage page is written only if injection changes it; a rendered one always is.
        let in_stage = unwritten.is_none();
        let html = match unwritten {
            Some(html) => html,
            // The sync client can evict a stage page; `io_stop` keeps that
            // distinguishable from a real failure all the way up to the cloud gate.
            None => std::fs::read_to_string(&path).map_err(|e| io_stop(root, "re-read for slot injection", &path, e))?,
        };
        scanned += 1;

        let source_oid = format!(
            "xxh3:{}",
            crate::build::assets::paths::compute_binary_hash(html.as_bytes())
        );
        walk_ms += t_walk.elapsed();
        // `ship_rev` is in the key because the record stores a hash of the
        // SHIPPED bytes; see `ship::SHIP_TRANSFORM_REV`.
        let params = serde_json::json!({
            "page_path": page_path,
            "slots_hash": slots_hash,
            "ship_rev": crate::build::ship::SHIP_TRANSFORM_REV,
        });

        let hit = transform_cache
            .find_cached_output(&source_oid, SLOT_INJECT_TRANSFORM, &params, crate::build::cache::RecordMode::Request)
            // Present but unreadable/corrupt (or written before this shape): a miss.
            .and_then(|record_oid| read_slot_inject_record(object_store, &record_oid))
            // Its blob may have been collected since: a miss too, re-run live. A
            // receipt must never name a blob `ship_phase` would not find.
            .filter(|record| object_store.get_path(&record.content_oid).is_some());
        let (changed, manifest_hash, content_oid, injected) = if let Some(record) = hit {
            if !record.residual.is_empty() {
                residual_files += 1;
                let residual: Vec<&str> = record.residual.iter().map(String::as_str).collect();
                log_residual_slot_markers(&page_path, &residual);
            }
            (record.rewritten, record.manifest_hash, Some(record.content_oid), None)
        } else {
            // Miss (or stale record) — do the real work.
            let injected = inject_slots(&html, slots, &page_path);
            let residual = residual_known_slot_markers(&injected);
            if !residual.is_empty() {
                residual_files += 1;
                log_residual_slot_markers(&page_path, &residual);
            }
            let changed_page = injected != html;
            // The manifest records the bytes the SITE will serve, which are the
            // stripped ones — computed here, from the bytes in hand, because
            // this is the last moment anything holds them.
            let manifest_hash = crate::build::assets::paths::compute_binary_hash(
                &crate::build::ship::apply_transform(
                    crate::build::ship::transform_for(&page_path),
                    injected.as_bytes(),
                ),
            );
            let content_oid = store_failures.store(object_store, injected.as_bytes());
            // Cache only what a hit can serve whole: a record without its blob would
            // have to be a miss again anyway.
            if let Some(oid) = &content_oid {
                write_slot_inject_record(
                    object_store,
                    transform_cache,
                    &source_oid,
                    html.len() as u64,
                    &params,
                    &SlotInjectRecord {
                        content_oid: oid.clone(),
                        manifest_hash: manifest_hash.clone(),
                        rewritten: changed_page,
                        residual: residual.iter().map(|s| s.to_string()).collect(),
                    },
                );
            }
            (changed_page, manifest_hash, content_oid, Some(injected))
        };
        rewritten += changed as usize;
        if changed || !in_stage {
            written += place_page(root, &path, content_oid.as_deref(), injected.as_deref(), object_store, staged)? as usize;
        }
        receipts.push(SlotInjectionReceipt { page_path, manifest_hash, content_oid });
    }

    store_failures.report();
    let total = t_pass.elapsed();
    log::info!(
        target: "slots",
        "slot injection: {} html scanned, {} rewritten, {} written, {} with unresolved markers",
        scanned,
        rewritten,
        written,
        residual_files,
    );
    log::info!(
        target: "timing",
        "[slots] {:?} total = walk+read+hash {:?} ({} files, corpus-scaled) + inject/write {:?} ({} files, delta-scaled)",
        total,
        walk_ms,
        scanned,
        total.saturating_sub(walk_ms),
        rewritten,
    );
    Ok(receipts)
}

/// Wrap any `<style>…</style>` blocks in the `head-end` slot of the given
/// `ResolvedSlots` in `@layer plugins { … }`, so third-party plugin CSS
/// participates in the cascade contract at the correct priority.
///
/// Only the `head-end` slot is touched — other slots (body-end, after-title,
/// etc.) carry HTML elements that are not stylesheet declarations.  The wrapping
/// applies to every `EnhanceContent::Static` entry whose HTML contains at least
/// one `<style>` tag; per-page entries are also processed.
///
/// Use this on the plugin `ResolvedSlots` **before** merging them into the
/// native-slot set so that native CSS (review, comments) is not mistakenly
/// wrapped.
pub fn wrap_plugin_head_end_css_in_layer(mut slots: ResolvedSlots) -> ResolvedSlots {
    let entries = match slots.entries.get_mut("head-end") {
        Some(e) => e,
        None => return slots,
    };

    for entry in entries.iter_mut() {
        match &mut entry.content {
            EnhanceContent::Static { html } => {
                *html = wrap_style_blocks_in_layer(html);
            }
            EnhanceContent::PerPage { pages } => {
                for html in pages.values_mut() {
                    *html = wrap_style_blocks_in_layer(html);
                }
            }
            EnhanceContent::PerLanguage { default, by_lang } => {
                if let Some(html) = default {
                    *html = wrap_style_blocks_in_layer(html);
                }
                for html in by_lang.values_mut() {
                    *html = wrap_style_blocks_in_layer(html);
                }
            }
        }
    }
    slots
}

/// Wraps every `<style>…</style>` block in the input HTML with
/// `<style>@layer plugins{…}</style>`, leaving non-`<style>` content
/// (script tags, meta tags, etc.) untouched.
///
/// # Edge case — `@import` inside a plugin `<style>`
///
/// A plugin `<style>` that contains a top-level `@import` rule becomes invalid
/// once nested inside `@layer plugins{…}` (the CSS spec forbids `@import` after
/// any non-import/charset rule in a stylesheet, and the layer wrapper counts as
/// one). No current bundled plugin uses `@import` inside a `<style>` tag, so
/// this is not triggered in practice. If a future plugin needs it, the solution
/// is to emit a `<link rel="stylesheet">` instead of a `<style>`.
pub(crate) fn wrap_style_blocks_in_layer(html: &str) -> String {
    // Fast path: nothing to do if no <style> tag present.
    if !html.contains("<style") {
        return html.to_string();
    }
    let mut result = String::with_capacity(html.len() + 64);
    let mut remainder = html;
    while let Some((before, after_open_kw)) = remainder.split_once("<style") {
        // Push everything before the opening <style tag verbatim.
        result.push_str(before);
        // Split off the rest of the opening tag (up to the first `>`) and the
        // element's inner CSS (up to the matching `</style>`). Either one
        // missing means malformed markup — bail and keep the rest as-is.
        let Some((open_attrs, after_open_tag)) = after_open_kw.split_once('>') else {
            result.push_str("<style");
            result.push_str(after_open_kw);
            return result;
        };
        let Some((inner, after_close)) = after_open_tag.split_once("</style>") else {
            result.push_str("<style");
            result.push_str(after_open_kw);
            return result;
        };
        // Emit: `<style …>@layer plugins{…}</style>`
        result.push_str("<style");
        result.push_str(open_attrs);
        result.push_str(">@layer plugins{");
        result.push_str(inner);
        result.push_str("}</style>");
        remainder = after_close;
    }
    // Append any trailing content after the last </style>.
    result.push_str(remainder);
    result
}

#[cfg(test)]
#[path = "enhance_tests.rs"]
mod tests;
