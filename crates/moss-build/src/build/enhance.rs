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
use crate::build::components::nav::footer::strip_empty_footer;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    /// What replaces a plugin-styled listing, by the listing page's stage
    /// path. A listing with no entry here keeps the default moss drew. See
    /// `build::listing_style`.
    listings: HashMap<String, crate::build::emit::listing_style::ListingFill>,
    /// How the listing pass reaches the plugins; never part of the cache key.
    #[serde(skip)]
    listing_host: Option<crate::build::emit::listing_style::ListingHost>,
    /// `[site] archival`: what every page says about its build, set by the
    /// pipeline once the source copy it describes is written.
    #[serde(skip_serializing_if = "Option::is_none")]
    edition: Option<crate::build::emit::archival::Edition>,
}

impl ResolvedSlots {
    /// Create empty resolved slots (no plugin content).
    pub fn empty() -> Self {
        Self {
            entries: HashMap::new(),
            feature_styles: Vec::new(),
            listings: HashMap::new(),
            listing_host: None,
            edition: None,
        }
    }

    /// Turn the edition blocks on for every page.
    pub(crate) fn set_edition(&mut self, edition: crate::build::emit::archival::Edition) {
        self.edition = Some(edition);
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

    pub(crate) fn set_listing_host(&mut self, host: Option<crate::build::emit::listing_style::ListingHost>) {
        self.listing_host = host;
    }

    pub(crate) fn listing_host(&self) -> Option<&crate::build::emit::listing_style::ListingHost> {
        self.listing_host.as_ref()
    }

    /// Settle one page's plugin-styled listing.
    pub(crate) fn set_listing(&mut self, page_path: &str, fill: crate::build::emit::listing_style::ListingFill) {
        self.listings.insert(page_path.to_string(), fill);
    }

    /// Check if there are no slot entries at all.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.listings.is_empty()
    }

    /// Merge a plugin's slot result into the resolved slots.
    /// `priority` controls ordering: lower values are inserted first.
    /// `plugin_name` is used for deterministic tie-breaking (alphabetical).
    pub fn merge(&mut self, result: &EnhanceResult, priority: u32, plugin_name: &str) {
        if !result.success {
            log::info!("Plugin '{}' returned success=false for slot content; skipping its slots", plugin_name);
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
    let mut result = crate::build::emit::listing_style::settle_spans(html, slots.listings.get(page_path));
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
        let content = slots.get_html(slot_name, page_path);
        if let Some(content) = content {
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
    let mut edition_placed = false;
    if let Some(placed) = slots.edition.as_ref().and_then(|e| e.place(&result, page_path)) {
        result = placed;
        edition_placed = true;
    }
    strip_empty_footer(result, footer_left_has_content, footer_end_has_content || edition_placed)
}

/// How every slot marker begins.
const SLOT_MARKER_OPEN: &str = "<!-- slot:";

/// Known slot markers still present in HTML that is about to ship.
///
/// Every `<!-- slot:NAME -->` for a known slot MUST be resolved (replaced or
/// stripped) by `inject_slots` before output reaches a browser. A residual
/// marker means slot injection didn't run on this file — a coverage / serve-path
/// bug — and the raw `<!-- slot:NAME -->` comment would render inertly in the
/// page. Used by the slot-injection pass to emit a WARN so the condition
/// is never silent. Unknown (non-moss) `slot:` comments are ignored.
fn residual_known_slot_markers(html: &str) -> Vec<&'static str> {
    if !html.contains(SLOT_MARKER_OPEN) {
        return Vec::new();
    }
    crate::build::slots::Slot::ALL
        .into_iter()
        .map(|slot| slot.as_str())
        .filter(|name| html.contains(&format!("<!-- slot:{} -->", name)))
        .collect()
}

/// Whether a stage page is one slot injection fills: `.html`, and not under
/// `jupyter/`, which is JupyterLite's third-party HTML.
pub(crate) fn takes_slots(page_path: &str) -> bool {
    page_path.ends_with(".html") && page_path.split('/').next() != Some("jupyter")
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
    log::info!(
        target: "slots",
        "unresolved slot marker(s) {:?} shipped in page#{:016x} — slot injection did not cover this file (raw marker will render; in attribute position it leaks a literal \">\")",
        residual,
        page_id,
    );
}

use crate::build::cache::page_sources::{SLOT_INJECT_TRANSFORM, SLOT_PAGE_TRANSFORM};

/// Cached record for a slot-injection transform: the page's shipped bytes (the
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
/// A shape change here needs no version gate: a record that does not
/// deserialize reads as a miss and falls through to a live re-run, which is how
/// records written before these fields existed are retired.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SlotInjectRecord {
    content_oid: String,
    manifest_hash: String,
    rewritten: bool,
    residual: Vec<String>,
}

/// The slot record for a page's rendered bytes, as this version writes it or as
/// an older one did: an unchanged page needs no new record after an upgrade.
fn slot_record(transform_cache: &crate::build::cache::TransformCache, source_oid: &str, params: &serde_json::Value) -> Option<SlotInjectRecord> {
    transform_cache.summary_with_page(SLOT_PAGE_TRANSFORM, source_oid, params)
        .or_else(|| transform_cache.summary(SLOT_INJECT_TRANSFORM, source_oid, params))
}

/// The slot records this process last used for a stage, by page, so a watch
/// rebuild re-reads none for pages whose bytes before injection are the same:
/// the key is the transform cache's own (page, those bytes' hash, the slot set
/// and the site's mount). A hit still has to find its blob in the store.
struct SlotMemo {
    /// The slot set's hash and [`crate::build::ship::shipped_key`].
    key: String,
    pages: std::collections::HashMap<String, (String, SlotInjectRecord)>,
}

static SLOT_MEMO: crate::build::cache::ProcessMemo<SlotMemo> = crate::build::cache::ProcessMemo::new(crate::build::cache::ROOTS);

/// Test-only: what a pass in a new process starts from.
#[cfg(test)]
pub(crate) fn forget_slot_memo(dir: &std::path::Path) {
    SLOT_MEMO.take(dir);
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
pub(crate) fn resolved_slots_hash(slots: &ResolvedSlots) -> String {
    let bytes = serde_json::to_value(slots)
        .and_then(|canonical| serde_json::to_vec(&canonical))
        .unwrap_or_default();
    crate::build::assets::paths::compute_binary_hash(&bytes)
}

/// A page the pass injected and wrote to the stage. Its blob, record and staged
/// link are written after the preview paints, by [`DeferredPage::settle`].
pub(crate) struct DeferredPage {
    oid: String,
    len: u64,
    /// The stage file right after the pass wrote it, the only one the staged
    /// link may vouch for.
    written: Option<crate::build::stat::FileStat>,
    record: Option<SlotRecordKey>,
    /// What the manifest is told about it. The batch puts the blob's oid on it
    /// once the blob is stored.
    pub receipt: SlotInjectionReceipt,
    /// The served path and manifest entry its blob ships under, once the
    /// manifest has registered the page.
    pub ship: Option<(String, String)>,
}

/// What the batch did with one page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Settled {
    /// Not reached before the session was cancelled: still owed.
    Cancelled,
    /// The store refused it; the page ships from the stage.
    Failed,
    /// Its blob and record are stored.
    Stored,
}

/// Settle `pages` in parallel and save the staged links and verified blobs
/// they made; one [`Settled`] per page, in order. The next save's render
/// carries pages only from a sealed render, and the seal waits for this.
pub(crate) fn settle_pages(
    pages: &mut [DeferredPage], stage: &std::path::Path, paths: &crate::moss_paths::MossPaths,
    objects: &crate::build::cache::ObjectStore, transforms: &crate::build::cache::TransformCache,
    cancelled: impl Fn() -> bool + Sync,
) -> Vec<Settled> {
    use rayon::prelude::*;
    type Batch = (Vec<Settled>, crate::build::cache::HashIndex, StoreFailures);
    let empty = || -> Batch { (Vec::new(), crate::build::cache::HashIndex::new(), StoreFailures::default()) };
    let (settled, links, failures) = pages
        .par_iter_mut()
        .fold(empty, |(mut settled, mut links, mut failures), page| {
            settled.push(if cancelled() {
                Settled::Cancelled
            } else if page.settle(stage, objects, transforms, &mut links, &mut failures) {
                page.receipt.content_oid = Some(page.oid.clone());
                Settled::Stored
            } else {
                Settled::Failed
            });
            (settled, links, failures)
        })
        .reduce(empty, |(mut settled, mut links, mut failures), (more, more_links, more_failures)| {
            settled.extend(more);
            links.entries.extend(more_links.entries);
            failures.merge(more_failures);
            (settled, links, failures)
        });
    // Merged: a pass that saved meanwhile keeps its entries.
    let file = paths.staged_links("pages");
    if let Err(e) = (!links.entries.is_empty()).then(|| links.save_merging(&file)).unwrap_or(Ok(())) {
        log::info!("[slots] could not save {}: {e}", file.display());
    }
    if let Err(e) = objects.save_verified_keeping_unchecked() {
        log::info!("[slots] could not save the verified-blob record: {e}");
    }
    failures.report();
    settled
}

struct SlotRecordKey {
    source_oid: String,
    params: serde_json::Value,
    source_len: u64,
    record: SlotInjectRecord,
}

impl DeferredPage {
    pub(crate) fn oid(&self) -> &str {
        &self.oid
    }

    /// Store the page's blob from its stage file, then its record, and add its
    /// staged link to `links` if the stage still holds the file the pass wrote.
    /// Whether the blob was stored.
    pub(crate) fn settle(&self, stage: &std::path::Path, objects: &crate::build::cache::ObjectStore,
        transforms: &crate::build::cache::TransformCache, links: &mut crate::build::cache::HashIndex,
        failures: &mut StoreFailures) -> bool {
        let path = stage.join(&self.receipt.page_path);
        let recorded_at = crate::build::stat::recording_clock();
        if let Err(reason) = objects.store_hashed_file(&path, &self.oid) {
            failures.fail(reason);
            return false;
        }
        if let Some(key) = &self.record {
            transforms.put_with_page(SLOT_PAGE_TRANSFORM, &key.source_oid, key.source_len, &key.params, &key.record, (&self.oid, self.len));
        }
        if let Some(now) = self.still_staged(&path, recorded_at) {
            links.update_read_at(self.receipt.page_path.clone(), &now, self.oid.clone(), recorded_at);
        }
        true
    }

    /// The stat of the stage file at `path` if it is still the one the pass
    /// wrote (one a later pass replaced is that pass's), judged against a clock
    /// sampled before.
    fn still_staged(&self, path: &std::path::Path, recorded_at: Option<u64>) -> Option<crate::build::stat::FileStat> {
        let now = std::fs::symlink_metadata(path).ok().map(|md| crate::build::stat::FileStat::of(&md))?;
        let written = self.written.as_ref()?;
        (written.same_inode_and_write(&now, recorded_at)
            || (now.inode.is_none() && crate::build::cache::ObjectStore::hash_file(path).is_ok_and(|hash| hash == self.oid)))
            .then_some(now)
    }
}

/// Pages the object store could not take during one batch.
///
/// A full or unwritable store must cost a page its CAS object, never the build,
/// so storing fails OPEN here and the reason is kept for one report at the end:
/// a store that is full fails every page, and a WARN per page is a corpus's
/// worth of the same line.
#[derive(Default)]
pub(crate) struct StoreFailures {
    pages: usize,
    first_reason: Option<String>,
}

impl StoreFailures {
    fn fail(&mut self, reason: String) {
        self.pages += 1;
        self.first_reason.get_or_insert(reason);
    }

    fn merge(&mut self, other: Self) {
        self.pages += other.pages;
        if self.first_reason.is_none() {
            self.first_reason = other.first_reason;
        }
    }

    pub(crate) fn report(self) {
        if let Some(reason) = self.first_reason {
            log::info!(
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
        match staged.link(objects, oid, path, None) {
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
/// cache-skippable); a miss injects once and writes the stage, and its blob and
/// record wait in [`SlotPass::deferred`] so the preview does not. A hit reaches
/// the stage through `staged`, which skips a file it vouches already holds the blob.
///
/// Returns one [`SlotInjectionReceipt`] per page scanned, whichever way it went;
/// a miss's names no blob until the batch stores one.
pub(crate) fn inject_slots_into_directory_cached(
    // The VAULT root, not `dir` (which is the stage below it): `io_stop` asks
    // the watcher predicates about the path inside the vault.
    root: &std::path::Path,
    dir: &std::path::Path,
    slots: &ResolvedSlots,
    // [`resolved_slots_hash`] of `slots`, which the caller needs first.
    slots_hash: &str,
    // The site's mount: part of what a page ships as, so of its manifest hash.
    mount: &crate::build::site_meta::mount::Mount,
    object_store: &crate::build::cache::ObjectStore,
    transform_cache: &crate::build::cache::TransformCache,
    rendered: std::collections::BTreeMap<String, String>,
    // Every page the manifest has registered that takes slots, by stage path. Only these ship,
    // so the stage is never listed: at 10k pages the listing alone outcost
    // the injection.
    registered: Vec<String>,
    // Stage pages proved unchanged since the pass that last filled them, by path,
    // with their manifest entry. Each is reported from its staged link, unread.
    carried: &std::collections::BTreeMap<String, String>,
    // Pages an earlier pass with these slots wrote whose batch stopped before
    // storing them: each still in the stage as written is reported unread and owed again.
    unsettled: Vec<DeferredPage>,
    staged: &mut StagedLinks,
    // `BuildStopped`: this reads the stage back, so a cloud eviction here must
    // stay distinguishable from a plugin returning garbage. See `build::outcome`.
) -> Result<SlotPass, BuildStopped> {
    let mut receipts = Vec::new();
    let mut scanned = 0usize;
    let mut rewritten = 0usize;
    let mut written = 0usize;
    let mut residual_files = 0usize;
    let mut deferred = Vec::new();
    // The part that scales with the CORPUS (walk, read, hash: paid even when
    // nothing changed); the rest, inject and write, scales with the DELTA.
    let mut walk_ms = std::time::Duration::ZERO;
    let t_pass = std::time::Instant::now();
    let _phase = crate::build::phase::PhaseTrace::start("slot_injection");
    let memo_key = format!("{slots_hash}:{}", crate::build::ship::shipped_key(mount));
    let remembered = SLOT_MEMO.take(dir).filter(|memo| memo.key == memo_key).map(|memo| memo.pages).unwrap_or_default();
    let mut kept = std::collections::HashMap::new();

    let t_walk = std::time::Instant::now();
    let mut unsettled: std::collections::HashMap<String, DeferredPage> =
        unsettled.into_iter().map(|page| (page.receipt.page_path.clone(), page)).collect();
    let recorded_at = crate::build::stat::recording_clock();
    let staged_only: Vec<String> = registered
        .into_iter()
        .filter(|page_path| !rendered.contains_key(page_path))
        .filter(|page_path| {
            let owed = unsettled.remove(page_path.as_str()).filter(|page| page.still_staged(&dir.join(page_path), recorded_at).is_some());
            if let Some((page, manifest_hash)) = owed.and_then(|page| {
                let manifest_hash = page.record.as_ref()?.record.manifest_hash.clone();
                Some((page, manifest_hash))
            }) {
                deferred.push(DeferredPage { receipt: SlotInjectionReceipt { page_path: page_path.clone(), manifest_hash, content_oid: None }, ship: None, ..page });
                return false;
            }
            let Some(oid) = staged.vouched_oid(page_path).filter(|oid| object_store.holds(oid)) else { return true };
            // A page newer than the sealed manifest that this process's last pass
            // filled: its record names the blob the link vouches for.
            let manifest_hash = carried.get(page_path).cloned().or_else(|| {
                remembered.get(page_path).filter(|(_, record)| record.content_oid == oid).map(|(_, record)| record.manifest_hash.clone())
            });
            let Some(manifest_hash) = manifest_hash else { return true };
            receipts.push(SlotInjectionReceipt { page_path: page_path.clone(), manifest_hash, content_oid: Some(oid) });
            false
        })
        .collect();
    walk_ms += t_walk.elapsed();
    // A page carried unread this pass keeps its record for the pass that renders it again.
    for page_path in receipts.iter().map(|receipt| &receipt.page_path).chain(deferred.iter().map(|page| &page.receipt.page_path)) {
        if let Some(entry) = remembered.get(page_path) {
            kept.insert(page_path.clone(), entry.clone());
        }
    }
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
            None => match std::fs::read_to_string(&path) {
                Ok(html) => html,
                // A carried page that left the stage after the render: `rereads_left`
                // discards the build, which runs again, rather than show an error.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(io_stop(root, "re-read for slot injection", &path, e)),
            },
        };
        // A page with no slot marker (a redirect stub) is injected live and keeps
        // no record: reading the record back costs more than the work it saves.
        let keyed = html.contains(SLOT_MARKER_OPEN);
        scanned += (in_stage || keyed) as usize;

        let key = keyed.then(|| {
            let source_oid = format!("xxh3:{}", crate::build::assets::paths::compute_binary_hash(html.as_bytes()));
            // `ship_rev` is in the key because the record stores a hash of the
            // SHIPPED bytes; see `ship::SHIP_TRANSFORM_REV`.
            let mut params = serde_json::json!({
                "page_path": page_path,
                "slots_hash": slots_hash,
                "ship_rev": crate::build::ship::SHIP_TRANSFORM_REV,
            });
            // A separate key, not `shipped_key`, so a root site's records keep
            // the key they were written under.
            if !mount.is_root() {
                params["mount"] = mount.as_str().into();
            }
            (source_oid, params)
        });
        walk_ms += t_walk.elapsed();

        let hit = key.as_ref().and_then(|(source_oid, params)| {
            remembered.get(&page_path).filter(|(oid, _)| oid == source_oid).map(|(_, record)| record.clone())
                .or_else(|| slot_record(transform_cache, source_oid, params))
        })
            // Its blob may have been collected since: a miss too, re-run live. A
            // receipt must never name a blob `ship_phase` would not find.
            .filter(|record| object_store.get_path(&record.content_oid).is_some());
        let (changed, manifest_hash, content_oid, injected, owed) = if let (Some(record), Some((source_oid, _))) = (hit, &key) {
            kept.insert(page_path.clone(), (source_oid.clone(), record.clone()));
            if !record.residual.is_empty() {
                residual_files += 1;
                let residual: Vec<&str> = record.residual.iter().map(String::as_str).collect();
                log_residual_slot_markers(&page_path, &residual);
            }
            (record.rewritten, record.manifest_hash, Some(record.content_oid), None, None)
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
            let manifest_hash = crate::build::ship::shipped_hash(&page_path, mount, injected.as_bytes());
            let oid = crate::build::cache::ObjectStore::oid_of(injected.as_bytes());
            // In the memo now: a hit there still needs the blob the batch stores.
            let record = key.map(|(source_oid, params)| {
                let record = SlotInjectRecord {
                    content_oid: oid.clone(),
                    manifest_hash: manifest_hash.clone(),
                    rewritten: changed_page,
                    residual: residual.iter().map(|s| s.to_string()).collect(),
                };
                kept.insert(page_path.clone(), (source_oid.clone(), record.clone()));
                SlotRecordKey { source_oid, params, source_len: html.len() as u64, record }
            });
            // A blob the store holds already (a page with no marker is the same bytes
            // every pass) ships as a hit does; the batch still verifies it before
            // vouching for the stage file.
            let stored = object_store.holds(&oid).then(|| oid.clone());
            let owed = (stored.is_none() || record.is_some()).then(|| (oid, injected.len() as u64, record));
            (changed_page, manifest_hash, stored, Some(injected), owed)
        };
        rewritten += changed as usize;
        if changed || !in_stage {
            let bytes = injected.as_deref();
            written += place_page(root, &path, content_oid.as_deref(), bytes, object_store, staged)? as usize;
        }
        let receipt = SlotInjectionReceipt { page_path, manifest_hash, content_oid };
        if let Some((oid, len, record)) = owed {
            let written = std::fs::symlink_metadata(&path).ok().map(|md| crate::build::stat::FileStat::of(&md));
            deferred.push(DeferredPage { oid, len, written, record, receipt, ship: None });
        } else {
            receipts.push(receipt);
        }
    }

    let sources = kept.iter().map(|(page, (oid, _))| (page.clone(), oid.clone())).collect();
    SLOT_MEMO.keep(dir, SlotMemo { key: memo_key, pages: kept });
    crate::build::phase::record_count("slot_html_scanned", scanned);
    crate::build::phase::record_count("slot_html_rewritten", rewritten);
    let total = t_pass.elapsed();
    log::info!(
        target: "slots",
        "slot injection: {} html scanned, {} carried unread, {} rewritten, {} written, {} with unresolved markers",
        scanned,
        receipts.len() + deferred.len() - scanned,
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
    Ok(SlotPass { receipts, scanned, sources, deferred })
}

/// A receipt per page the pass covered, `scanned` of which it read and hashed:
/// in `receipts`, or on the deferred page itself.
pub(crate) struct SlotPass {
    pub receipts: Vec<SlotInjectionReceipt>,
    pub scanned: usize,
    /// Each page this pass has a record for, with that record's source oid.
    pub sources: std::collections::BTreeMap<String, String>,
    /// The pages it wrote, whose blobs and records wait for the after-paint batch,
    /// each carrying its own receipt.
    pub deferred: Vec<DeferredPage>,
}

#[cfg(test)]
#[path = "enhance_tests.rs"]
mod tests;
