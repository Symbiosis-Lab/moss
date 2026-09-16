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
/// `Serialize` (moss#919 item 2) is used only to derive a cache-params hash
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
    for slot_name in crate::build::slots::Slot::ALL.map(|s| s.as_str()) {
        let marker = format!("<!-- slot:{} -->", slot_name);
        let has_marker = result.contains(&marker);
        if let Some(content) = slots.get_html(slot_name, page_path) {
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
    result
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

/// Cached record for an `html/slots` transform: what the injection produced
/// (`None` when it was a no-op — no known marker was present to substitute),
/// plus the residual-marker scan result so the `residual_known_slot_markers`
/// diagnostic can still fire on a cache hit (moss#919 item 2, prior-art
/// requirement from `4dca6d3fc`: a coverage bug once shipped a raw marker to a
/// real user's browser — caching must never make that WARN skippable).
///
/// `injected` is one `Option` holding two values, not two `Option`s that must
/// agree: the content-store OID of the injected bytes, and the **manifest
/// hash** of those bytes after `ship::apply_transform`. Both exist exactly when
/// injection rewrote the page, so a state where one is present and the other is
/// not cannot be written down.
///
/// The manifest hash has to be stored because neither arm can recover it later.
/// The stage holds pre-strip bytes by design (the preview wants the annotations
/// the published site does not), and the cache-hit arm `link_to`s a blob keyed
/// by SHA-256 of the *un*stripped content — the wrong algorithm over the wrong
/// bytes. Storing it at miss time, when the injected bytes are in hand, is what
/// lets the registration site stop reading the stage back.
///
/// A shape change here needs no version gate: `read_slot_inject_record` returns
/// `None` on a deserialize failure and falls through to a live re-run.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SlotInjectRecord {
    injected: Option<(String, String)>,
    residual: Vec<String>,
}

/// Canonical hash of the fully-resolved slot content, for the `slots_hash`
/// cache-params field. `serde_json::to_vec` serializes `HashMap` fields via
/// `serde_json::Value`'s (non-`preserve_order`) `Map`, which is BTreeMap-
/// backed and therefore key-sorted — so this hash is stable across runs
/// regardless of the source `HashMap`s' random iteration order (the
/// `26b400251` HashMap-nondeterminism lesson, satisfied for free rather than
/// by hand-sorting every nested map).
fn resolved_slots_hash(slots: &ResolvedSlots) -> String {
    let bytes = serde_json::to_vec(slots).unwrap_or_default();
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
    let mut rec = transform_cache
        .get(source_oid)
        .unwrap_or_else(|| crate::build::cache::TransformRecord {
            source_oid: source_oid.to_string(),
            source_size,
            transforms: std::collections::HashMap::new(),
        });
    rec.transforms.insert(SLOT_INJECT_TRANSFORM.to_string(), entry);
    if let Err(e) = transform_cache.put(&rec) {
        log::warn!("Failed to write html/slots transform record: {}", e);
    }
}

/// The slot-injection pass over the stage, used by the live pipeline.
///
/// `generate_blocking_content` re-renders and writes every page's HTML on
/// every build unconditionally (no incremental skip), so a staged file's
/// mtime is never a valid "unchanged" signal — this keys on content instead.
/// `source_oid` is `xxh3:<hash of this file's freshly-written bytes>` (the
/// same non-SHA256-key convention the scan stat-key branch already uses);
/// `params` covers `page_path` plus a hash of the ENTIRE resolved slot set
/// ([`resolved_slots_hash`]) — deliberately the whole set, not just what
/// applies to this one page, so this cache can never alias a change to
/// content that ultimately lands somewhere else in `ResolvedSlots`.
///
/// On a hit, `link_to`s the previously-injected bytes into place (or leaves
/// the freshly-rendered raw bytes alone, for the cached no-op case) and
/// re-emits the cached residual-marker WARN if the miss that produced this
/// entry had one — the diagnostic must never become cache-skippable. On a
/// miss, runs the real read/inject/diff once and stores the result.
pub fn inject_slots_into_directory_cached(
    // The VAULT root, not `dir` (which is the stage below it): `io_stop` asks
    // the watcher predicates about the path inside the vault (#1067).
    root: &std::path::Path,
    dir: &std::path::Path,
    slots: &ResolvedSlots,
    object_store: &crate::build::cache::ObjectStore,
    transform_cache: &crate::build::cache::TransformCache,
    // `BuildStopped`: this reads the stage back, so a cloud eviction here must
    // stay distinguishable from a plugin returning garbage. See `build::outcome`.
) -> Result<Vec<(String, String)>, BuildStopped> {
    let slots_hash = resolved_slots_hash(slots);
    let mut changed = Vec::new();
    let mut scanned = 0usize;
    let mut residual_files = 0usize;
    // Split the pass into the part that scales with the CORPUS (walk every HTML
    // file, read it, hash it — paid even when nothing changed) and the part that
    // scales with the DELTA (inject + write). moss#968 predicted the walk is the
    // majority of the ~0.9s and that narrowing the render set therefore reclaims
    // less of it than it looks; these two numbers settle that.
    // `walk` is measured directly and `rewrite` derived as the remainder, because
    // the cache-hit path below leaves the loop body through three separate
    // `continue`s and an accumulator at each one would rot the moment a fourth
    // is added.
    let mut walk_ms = std::time::Duration::ZERO;
    let t_pass = std::time::Instant::now();

    for entry in walk_html_files(dir) {
        let t_walk = std::time::Instant::now();
        let page_path = page_path_for(dir, entry.path());
        // Reads back HTML this build just wrote into the stage. The sync client
        // can evict it in between; `io_stop` keeps that answer distinguishable
        // from a real failure all the way up to the cloud gate (moss#964).
        let html = std::fs::read_to_string(entry.path())
            .map_err(|e| io_stop(root, "re-read for slot injection", entry.path(), e))?;
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

        if let Some(record_oid) =
            transform_cache.find_cached_output(&source_oid, SLOT_INJECT_TRANSFORM, &params)
        {
            if let Some(record) = read_slot_inject_record(object_store, &record_oid) {
                if !record.residual.is_empty() {
                    residual_files += 1;
                    let residual: Vec<&str> = record.residual.iter().map(String::as_str).collect();
                    log_residual_slot_markers(&page_path, &residual);
                }
                match record.injected {
                    Some((content_oid, manifest_hash))
                        if object_store.get_path(&content_oid).is_some() =>
                    {
                        object_store.link_to(&content_oid, entry.path())?;
                        changed.push((page_path, manifest_hash));
                        continue;
                    }
                    None => continue, // cached no-op — the freshly-rendered raw bytes are already correct
                    Some(_) => {}     // inner blob GC'd — fall through to a live re-run
                }
            }
            // Record present but unreadable/corrupt — fall through as well.
        }

        // Miss (or stale record) — do the real work.
        let injected = inject_slots(&html, slots, &page_path);
        let residual = residual_known_slot_markers(&injected);
        if !residual.is_empty() {
            residual_files += 1;
            log_residual_slot_markers(&page_path, &residual);
        }
        let injected_record = if injected != html {
            // `write_output`, not `fs::write`: `dir` is `.moss/build/staging/`,
            // and an `O_TRUNC` open of a page the sync client evicted between
            // render and injection fails EDEADLK (ADR-043).
            crate::build::io_utils::write_output(entry.path(), injected.as_bytes())
                .map_err(|e| io_stop(root, "write injected page", entry.path(), e))?;
            // The manifest records the bytes the SITE will serve, which are the
            // stripped ones — computed here, from the bytes in hand, because
            // this is the last moment anything holds them.
            let manifest_hash = crate::build::assets::paths::compute_binary_hash(
                &crate::build::ship::apply_transform(
                    crate::build::ship::transform_for(&page_path),
                    injected.as_bytes(),
                ),
            );
            changed.push((page_path.clone(), manifest_hash.clone()));
            Some((object_store.store_bytes(injected.as_bytes())?, manifest_hash))
        } else {
            None
        };
        write_slot_inject_record(
            object_store,
            transform_cache,
            &source_oid,
            html.len() as u64,
            &params,
            &SlotInjectRecord {
                injected: injected_record,
                residual: residual.iter().map(|s| s.to_string()).collect(),
            },
        );
    }

    let total = t_pass.elapsed();
    log::info!(
        target: "slots",
        "slot injection: {} html scanned, {} rewritten, {} with unresolved markers",
        scanned,
        changed.len(),
        residual_files,
    );
    log::info!(
        target: "timing",
        "[slots] {:?} total = walk+read+hash {:?} ({} files, corpus-scaled) + inject/write {:?} ({} files, delta-scaled)",
        total,
        walk_ms,
        scanned,
        total.saturating_sub(walk_ms),
        changed.len(),
    );
    Ok(changed)
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
