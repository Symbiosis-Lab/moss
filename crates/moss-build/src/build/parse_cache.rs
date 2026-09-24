//! Loop A parse cache.
//!
//! Stage 0 measured Loop A — read + Obsidian-resolve + parse, `render/blocking.rs` —
//! at ~70-74% of render wall time (~16-18s of ~22s on the 216-page harbor/潮汐
//! vault). Stages 4-6 only ever skip Loop B, so a one-file save still paid for
//! parsing all 216 pages. This module lets an unchanged page replay the
//! `ParsedDocument` the previous build produced instead.
//!
//! # The snapshot boundary
//!
//! An entry is the `ParsedDocument` **exactly as Loop A's per-file work leaves
//! it** — after the sequential uid-writeback post-pass, strictly before any
//! whole-corpus "Reduce" pass (`uid_dedup::resolve_duplicate_uids`,
//! `resolve_duplicate_slugs_with_lang`, `apply_cascade`,
//! `populate_direct_children_sorts`, `folder_embed::expand_markers_in_documents`,
//! `email::stamp_inline_subscribe_scopes`, translation-linking). Those passes then run over the assembled vector —
//! some entries replayed, some fresh — unchanged in shape, every build, exactly
//! as today. Caching a POST-Reduce document would carry another page's
//! inherited cascade/translation state forward, which is why the boundary is
//! the whole soundness argument here.
//!
//! # Why the store is in-memory, not a `parse-cache.json`
//!
//! The design doc proposed a file mirroring `HashIndex`/`FacadeCache`. Two
//! facts moved it in-process instead:
//!
//! 1. `ParsedDocument` is not round-trippable through serde. `body_plan`
//!    (what the render phase actually renders) and `outgoing_links`
//!    are `#[serde(skip)]`, and `BodyPlan`/`OutgoingLink` do not implement
//!    `Serialize` at all. A JSON cache would silently replay documents with no
//!    body plan, i.e. HTML missing every grid/cover enhancement — the exact
//!    "silently stale" failure this stage must not have. Making the whole
//!    moss-core `Block` tree serializable is a far larger change than Stage 7.
//! 2. A file would never be read anyway. The cache is only ever consulted under
//!    `BuildTrigger::ContentOnly`, which only the watch loop produces, inside a
//!    long-lived process. Every cross-process entry point (`moss build`,
//!    `moss preview`, deploy) passes `Full` and would cold-miss by construction.
//!
//! So the cache lives for the life of the dev-server process and holds ONE
//! root's entries (a folder switch replaces it rather than accumulating).
//!
//! # Validity
//!
//! An entry for X is replayable when, and only when:
//!
//! * the build is eligible at all (see [`ParseSession::begin`]),
//! * X's own source bytes hash to what they hashed to when the entry was written,
//! * every file in X's **transitive embed closure** still hashes to what it
//!   hashed to then. Transclusion splices a target's raw bytes into the host's
//!   markdown before parsing, so a host whose own bytes never moved can still
//!   need a reparse. The closure is computed with
//!   [`moss_core::dep_graph::DepGraph::embed_closure`] over
//!   `ParsedDocument::embed_deps`, which is a multi-hop walk: `index.md` embeds
//!   `a.md` embeds `b.md` means editing `b.md` misses `index.md` too. (Filtering
//!   the raw pair list on `index.md` — the obvious-looking implementation —
//!   returns only `a.md`.) Note the depth-2 invalidation is a conservative
//!   over-approximation, not a correctness requirement: the live resolver
//!   splices ONE hop, so `b.md`'s bytes never reach `index.md`'s HTML. The
//!   DEPTH-1 case is the load-bearing one, and it is stale-page-real —
//!   verified by mutation in
//!   `tests/incremental_parse_cache.rs::a_transclusion_edit_reaches_the_rendered_html_on_disk`.
//!
//! Content hashes go through [`HashIndex`], the existing `stat → sha256`
//! accelerator, so an unchanged file is not re-read to be hashed. It trusts a
//! hash only for the file's full stat record, so a same-size edit in the same
//! second as the last one is not mistaken for no edit.
//!
//! # What this deliberately does NOT track
//!
//! Loop A also reads per-build inputs that are not any markdown file's bytes:
//! `page_map`/`dir_overrides` (frontmatter `url:` overrides), `external_url_map`,
//! `content_graph`, `event_level_image_lookup` (image dimensions), the asset
//! snapshot, and site-level flags. The first group is folded into a single
//! build-level [`ParseSession::begin`] fingerprint — a mismatch disables the
//! cache for the WHOLE build (full Loop A, cache refreshed for next time),
//! never a partial reuse.
//!
//! Asset-derived inputs are covered by a coarser rule instead: the cache is
//! gated on `SiteConfig::incremental.parse_cache`, resolved by
//! `PipelineConfig::allows_parse_cache_reuse`, which requires every changed
//! path in the batch to be markdown OR on a **parse-irrelevant allowlist**
//! (`css`, `js`, `mjs`, and web-font extensions — see
//! `PARSE_IRRELEVANT_EXTENSIONS` in `build.rs`). An image edit therefore still
//! disables the parse cache for that entire build rather than for the pages
//! that happen to reference it — moss has no per-page asset-dependency
//! tracking, and inventing one is a larger job than this stage. So does a
//! `.moss/config.toml` edit, and so does any extension nobody has proved
//! parse-irrelevant: unrecognised means ineligible, which is the
//! "over-approximate, never under-approximate" choice.
//!
//! The allowlist exists because it was measured. On a real 217-file site a
//! `footer.md` edit spent 94ms in the markdown phase and a
//! `.moss/theme/style.css` edit spent 23-67s — the stylesheet was switching off
//! a cache it cannot invalidate. Stylesheets, scripts and fonts are referenced
//! by the SHELL at render time via `css_version`/`user_css_version`, never read
//! while a document is parsed, and the render side gates them independently
//! (`FacadeCache::asset_versions` → `FullCause::AssetVersionsMoved`).
//!
//! # Kill switch
//!
//! `MOSS_NO_INCREMENTAL` — the same one Stage 5b ships. It is read in BOTH
//! `PipelineConfig::allows_incremental_skip` and
//! `PipelineConfig::allows_parse_cache_reuse` and delivered here as
//! `SiteConfig::incremental.parse_cache`. There is deliberately no second flag:
//! the two gates differ only in which changed paths they tolerate, and the kill
//! switch turns both off at once, so anyone bisecting "the incremental build
//! served something stale" needs neither to remember a second variable.
//!
//! `MOSS_PARSE_CACHE_SHADOW` is NOT a kill switch — it is the rollout/diagnostic
//! mode described in the design doc: decisions are computed and logged but never
//! acted on (every page is parsed for real), and each would-be hit is verified
//! against the freshly parsed document's facade so a stale reuse shows up as a
//! logged error instead of a wrong page.

use crate::build::cache::HashIndex;
use crate::build::types::ParsedDocument;
use moss_core::dep_graph::DepGraph;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// One page's replayable parse plus everything needed to prove it still applies.
#[derive(Debug, Clone)]
struct ParseCacheEntry {
    /// SHA-256 of the page's own source bytes, as read by the build that wrote
    /// this entry (i.e. BEFORE that build's uid write-back, if any — which
    /// simply costs the next build one reparse of that page).
    content_hash: String,
    /// `(path, hash)` for every member of the page's transitive embed closure,
    /// as of the same moment. Materialized at write time so validity checking
    /// is a flat comparison and does not depend on re-deriving the graph.
    dep_hashes: Vec<(String, String)>,
    doc: ParsedDocument,
}

/// One root's cached Loop A output, plus the build-level input fingerprint the
/// entries were produced under.
#[derive(Debug, Clone, Default)]
struct ParseCache {
    inputs_fingerprint: String,
    entries: HashMap<String, ParseCacheEntry>,
}

/// The process-global store. One root at a time — switching folders in the dev
/// server replaces it, so this does not grow with the number of folders opened.
static STORE: Mutex<Option<(PathBuf, ParseCache)>> = Mutex::new(None);

/// What the last build's parse cache did. Read by tests and by anyone reading
/// the `incremental` log target.
static LAST_STATS: Mutex<Option<ParseCacheStats>> = Mutex::new(None);

/// Outcome of one build's parse-cache pass.
#[derive(Debug, Clone, Default)]
pub struct ParseCacheStats {
    /// Was the cache eligible to be consulted at all this build?
    pub eligible: bool,
    /// Why not, when `eligible` is false.
    pub reason: String,
    /// Running in `MOSS_PARSE_CACHE_SHADOW` mode — decisions logged, never used.
    pub shadow: bool,
    /// Pages Loop A considered.
    pub total: usize,
    /// Pages whose cached parse was (or, in shadow mode, would have been) reused.
    pub hits: usize,
    /// Source paths that were parsed for real, sorted.
    pub misses: Vec<String>,
    /// Shadow-mode failures: a page the cache called a HIT whose freshly parsed
    /// facade does NOT match the cached one. Must always be empty; a non-empty
    /// list means flipping to real reuse would serve a stale page.
    pub stale_hits: Vec<String>,
}

impl ParseCacheStats {
    /// Was this page reparsed this build?
    pub fn missed(&self, path: &str) -> bool {
        self.misses.iter().any(|p| p == path)
    }
}

/// The last completed build's parse-cache outcome, or `None` before any build.
pub fn last_stats() -> Option<ParseCacheStats> {
    LAST_STATS.lock().ok().and_then(|s| s.clone())
}

/// Forget everything. Test-only lever: cargo runs a test binary's tests in one
/// process, so two tests over two temp vaults would otherwise share a store.
pub fn reset_for_tests() {
    if let Ok(mut store) = STORE.lock() {
        *store = None;
    }
}

/// Content hashing for the current build, memoized.
///
/// Memoization is not just a speed-up: `finish` must record for X exactly the
/// hash `lookup` compared against, even though the uid write-back may have
/// rewritten X's bytes in between.
struct FileHasher {
    root: PathBuf,
    index: Mutex<HashIndex>,
    computed: Mutex<HashMap<String, Option<String>>>,
}

impl FileHasher {
    fn new(root: &Path, index: HashIndex) -> Self {
        Self {
            root: root.to_path_buf(),
            index: Mutex::new(index),
            computed: Mutex::new(HashMap::new()),
        }
    }

    /// SHA-256 of `relative_path`'s current bytes, or `None` if it can't be
    /// read (a missing embed target, say — which every caller must treat as
    /// "cannot prove unchanged", i.e. a miss).
    fn hash(&self, relative_path: &str) -> Option<String> {
        if let Some(hit) = self
            .computed
            .lock()
            .ok()
            .and_then(|c| c.get(relative_path).cloned())
        {
            return hit;
        }
        let computed = self.compute(relative_path);
        if let Ok(mut cache) = self.computed.lock() {
            cache.insert(relative_path.to_string(), computed.clone());
        }
        computed
    }

    fn compute(&self, relative_path: &str) -> Option<String> {
        self.compute_with(relative_path, |abs| std::fs::read(abs).ok())
    }

    /// [`compute`](Self::compute) with the read passed in, so a test can change the
    /// file between the read and the record. The index does the rest — stat before the
    /// read, no read of a page still in the cloud, the record — as it does for every
    /// other reader of it. Rayon's Loop A calls this from many threads, and the index
    /// lock is held across the read; a page is small and a hit (the usual answer)
    /// costs a stat.
    fn compute_with(&self, relative_path: &str, read: impl FnOnce(&Path) -> Option<Vec<u8>>) -> Option<String> {
        let abs = self.root.join(relative_path);
        let mut index = self.index.lock().unwrap_or_else(|e| e.into_inner());
        index
            .resolve_with(
                &abs,
                relative_path,
                |path| read(path).map(|bytes| format!("{:x}", Sha256::digest(&bytes))).ok_or_else(|| "unreadable".to_string()),
                crate::build::icloud::is_still_in_the_cloud,
            )
            .ok()
    }

    /// Memoize the hash of bytes the caller has ALREADY read, without touching
    /// the stat-keyed index.
    ///
    /// Loop A reads a page's source before parsing it, and may then write a
    /// freshly minted `uid:` back into that same file. The hash that describes
    /// the parse is the one over the bytes that were parsed — the PRE-write-back
    /// ones. Recording those bytes against the POST-write-back `(size, mtime)`
    /// in `HashIndex` would make the next build call a changed file unchanged,
    /// so this path deliberately only fills the per-build memo: the next build
    /// re-hashes, sees the difference, and reparses that page once.
    fn note_bytes(&self, relative_path: &str, bytes: &[u8]) {
        let hash = format!("{:x}", Sha256::digest(bytes));
        if let Ok(mut cache) = self.computed.lock() {
            cache.insert(relative_path.to_string(), Some(hash));
        }
    }

    fn into_index(self) -> HashIndex {
        self.index.into_inner().unwrap_or_else(|e| e.into_inner())
    }
}

/// One build's use of the parse cache. Created before Loop A, consumed after it.
pub struct ParseSession {
    root: PathBuf,
    index_path: PathBuf,
    /// The previous build's cache, present only when this build may consult it.
    previous: Option<ParseCache>,
    inputs_fingerprint: String,
    shadow: bool,
    hasher: FileHasher,
    /// Decisions taken during Loop A: `path → hit`.
    decisions: Mutex<HashMap<String, bool>>,
    stats: Mutex<ParseCacheStats>,
}

impl ParseSession {
    /// Open the parse cache for this build.
    ///
    /// `enabled` is `SiteConfig::incremental.parse_cache` — already false for
    /// every entry point but a watch rebuild whose whole batch is markdown or
    /// parse-irrelevant, and false whenever `MOSS_NO_INCREMENTAL` is set. `inputs_fingerprint` covers Loop A's
    /// non-content inputs (see [`inputs_fingerprint`]); a change disables reuse
    /// for the whole build while still refreshing the cache for the next one.
    pub fn begin(
        root: &Path,
        index_path: &Path,
        enabled: bool,
        inputs_fingerprint: String,
    ) -> Self {
        let shadow = std::env::var("MOSS_PARSE_CACHE_SHADOW").is_ok();
        let stored = STORE
            .lock()
            .ok()
            .and_then(|s| s.as_ref().filter(|(r, _)| r == root).map(|(_, c)| c.clone()));

        let (previous, reason) = if !enabled {
            (None, "not an incremental-eligible build".to_string())
        } else {
            match stored {
                None => (None, "cold cache (no previous build in this process)".to_string()),
                Some(cache) if cache.inputs_fingerprint != inputs_fingerprint => (
                    None,
                    "build-level parse inputs changed (page_map/url overrides/site flags)"
                        .to_string(),
                ),
                Some(cache) => (Some(cache), String::new()),
            }
        };

        let stats = ParseCacheStats {
            eligible: previous.is_some(),
            reason,
            shadow,
            ..Default::default()
        };

        Self {
            root: root.to_path_buf(),
            index_path: index_path.to_path_buf(),
            previous,
            inputs_fingerprint,
            shadow,
            hasher: FileHasher::new(root, HashIndex::load(index_path)),
            decisions: Mutex::new(HashMap::new()),
            stats: Mutex::new(stats),
        }
    }

    /// True when nothing this build can consult the cache — Loop A callers can
    /// skip the per-file lookup entirely.
    pub fn is_cold(&self) -> bool {
        self.previous.is_none()
    }

    /// The cached parse for `relative_path`, if it provably still applies.
    ///
    /// Records the decision either way. Returns `None` in shadow mode even on a
    /// hit, so the caller always does the real work — the decision is still
    /// recorded, and [`Self::verify_shadow`] checks it against the real parse.
    pub fn lookup(&self, relative_path: &str) -> Option<ParsedDocument> {
        let hit = self.decide(relative_path);
        if let Ok(mut decisions) = self.decisions.lock() {
            decisions.insert(relative_path.to_string(), hit.is_some());
        }
        if self.shadow {
            return None;
        }
        hit
    }

    fn decide(&self, relative_path: &str) -> Option<ParsedDocument> {
        let previous = self.previous.as_ref()?;
        let entry = previous.entries.get(relative_path)?;
        if self.hasher.hash(relative_path).as_deref() != Some(entry.content_hash.as_str()) {
            return None;
        }
        for (dep, hash) in &entry.dep_hashes {
            if self.hasher.hash(dep).as_deref() != Some(hash.as_str()) {
                return None;
            }
        }
        Some(entry.doc.clone())
    }

    /// Tell the session what a page's source bytes were at the moment Loop A
    /// parsed them (see [`FileHasher::note_bytes`] for why this is not the same
    /// as hashing the file again afterwards).
    pub fn note_source_bytes(&self, relative_path: &str, bytes: &[u8]) {
        self.hasher.note_bytes(relative_path, bytes);
    }

    /// Shadow-mode falsifier: a page the cache called a HIT must parse to the
    /// same facade it was cached with. Called after the real parse; a no-op
    /// outside shadow mode.
    pub fn verify_shadow(&self, relative_path: &str, fresh: &ParsedDocument) {
        if !self.shadow {
            return;
        }
        let Some(previous) = self.previous.as_ref() else {
            return;
        };
        let was_hit = self
            .decisions
            .lock()
            .ok()
            .and_then(|d| d.get(relative_path).copied())
            .unwrap_or(false);
        if !was_hit {
            return;
        }
        let Some(entry) = previous.entries.get(relative_path) else {
            return;
        };
        if crate::build::facade::compute_page_facade(&entry.doc)
            != crate::build::facade::compute_page_facade(fresh)
        {
            log::error!(
                target: "incremental",
                "parse-cache SHADOW STALE HIT: {relative_path} would have replayed a parse that no longer matches"
            );
            if let Ok(mut stats) = self.stats.lock() {
                stats.stale_hits.push(relative_path.to_string());
            }
        }
    }

    /// Record this build's Loop A output as the next build's cache, publish
    /// stats, and persist the hash index.
    ///
    /// `documents` must be the PRE-Reduce snapshot — see the module docs.
    pub fn finish(self, documents: &[ParsedDocument]) {
        // Group every recorded transclusion pair by its immediate embedder, so
        // multi-hop chains are walked, not filtered. Pairs repeat once per
        // ancestor; `with_embed_pairs` collapses them.
        let pairs: Vec<(&str, &str)> = documents
            .iter()
            .flat_map(|doc| {
                doc.embed_deps
                    .iter()
                    .map(|(target, embedder)| (target.as_str(), embedder.as_str()))
            })
            .collect();
        let graph = DepGraph::default().with_embed_pairs(pairs);

        let mut entries: HashMap<String, ParseCacheEntry> = HashMap::new();
        for doc in documents {
            let Some(path) = doc.source_path.as_ref() else {
                continue;
            };
            let Some(content_hash) = self.hasher.hash(path) else {
                continue;
            };
            let mut dep_hashes = Vec::new();
            let mut incomplete = false;
            for dep in graph.embed_closure(path) {
                match self.hasher.hash(&dep) {
                    Some(hash) => dep_hashes.push((dep, hash)),
                    // An embed target we cannot hash (missing file, unreadable)
                    // can never be proven unchanged, so caching the page would
                    // only produce a guaranteed miss — or worse, a hit that
                    // ignores it. Drop the entry instead.
                    None => {
                        incomplete = true;
                        break;
                    }
                }
            }
            if incomplete {
                continue;
            }
            dep_hashes.sort();
            entries.insert(
                path.clone(),
                ParseCacheEntry {
                    content_hash,
                    dep_hashes,
                    doc: doc.clone(),
                },
            );
        }

        let decisions = self
            .decisions
            .into_inner()
            .unwrap_or_else(|e| e.into_inner());
        let mut stats = self.stats.into_inner().unwrap_or_else(|e| e.into_inner());
        stats.total = decisions.len().max(documents.iter().filter(|d| d.source_path.is_some()).count());
        stats.hits = decisions.values().filter(|hit| **hit).count();
        stats.misses = decisions
            .iter()
            .filter(|(_, hit)| !**hit)
            .map(|(path, _)| path.clone())
            .collect();
        stats.misses.sort();

        if stats.eligible {
            log::info!(
                target: "incremental",
                "parse cache{}: {} of {} pages replayed, {} reparsed{}",
                if stats.shadow { " (SHADOW — nothing reused)" } else { "" },
                stats.hits,
                stats.total,
                stats.misses.len(),
                if stats.stale_hits.is_empty() {
                    String::new()
                } else {
                    format!(", {} STALE HITS", stats.stale_hits.len())
                },
            );
            if !stats.misses.is_empty() && stats.misses.len() <= 20 {
                log::debug!(target: "incremental", "parse cache reparsed: {:?}", stats.misses);
            }
        } else {
            log::info!(
                target: "incremental",
                "parse cache not consulted: {}",
                stats.reason
            );
        }

        if let Ok(mut last) = LAST_STATS.lock() {
            *last = Some(stats);
        }

        let cache = ParseCache {
            inputs_fingerprint: self.inputs_fingerprint,
            entries,
        };
        if let Ok(mut store) = STORE.lock() {
            *store = Some((self.root, cache));
        }

        // Merge, don't overwrite: the background image/video workers own
        // disjoint keys in the same index (see `HashIndex::save_merging`).
        if let Err(e) = self.hasher.into_index().save_merging(&self.index_path) {
            log::warn!(target: "incremental", "failed to save hash index: {e}");
        }
    }
}

/// The build-level scalars `process_markdown_file` reads, as the
/// `site_scalars` argument of [`inputs_fingerprint`]. A function rather than a
/// tuple written at the call site so a test can build exactly what production
/// hashes: `render/blocking.rs` passes the same `SiteMarkdown` value to the
/// call, so the two cannot disagree about which `[site]` flags a parse reads.
pub fn site_scalars<'a>(
    site_lang: crate::i18n::Language,
    site_id: Option<&'a str>,
    seta_url: &'a str,
    site: crate::build::markdown::SiteMarkdown<'a>,
    emit_source_lines: bool,
    has_content_folders: bool,
) -> impl std::fmt::Debug + 'a {
    (site_lang, site_id, seta_url, site, emit_source_lines, has_content_folders)
}

/// Fingerprint of Loop A's non-content inputs.
///
/// Every one of these is a whole-corpus pre-scan result threaded into
/// `process_markdown_file`, so a change to any of them can alter a page's parse
/// output without touching that page's bytes — the classic example being a
/// folder index's frontmatter `url:`, which moves `page_map`'s shape for its
/// whole subtree. A mismatch bypasses the cache for the entire build.
///
/// Maps arrive as `HashMap`s whose `Debug` order is per-instance random, so
/// they are sorted into `BTreeMap`/`Vec` before hashing (the Stage 5a
/// determinism bug, one phase earlier).
///
/// NOT included, deliberately: `content_graph` (a pure function of the file
/// path list, which IS included), and the asset/media inputs
/// (`event_level_image_lookup`, the asset snapshot) — see the module docs for
/// why those are handled by the whole-build markdown-only gate instead.
///
/// SEE ALSO — there are TWO whole-build bypasses, not one, and they are
/// computed by entirely separate code paths. This one is PRE-parse and input-
/// SHAPE-based; its post-parse, CONTENT-based sibling is the
/// `global_invalidator_changed` check in `render/incremental/verdict.rs`,
/// which forces a full RENDER when a slot page's body or a home page's
/// extracted excerpt moves. If you are bisecting a stale-page report, check
/// both — neither one subsumes the other.
#[allow(clippy::too_many_arguments)]
pub fn inputs_fingerprint(
    markdown_files: &[String],
    page_map: &HashMap<String, String>,
    dir_overrides: &HashMap<String, String>,
    external_url_map: &HashMap<String, String>,
    home_file_winners: &std::collections::HashSet<String>,
    root_folder_name: &str,
    site_scalars: &dyn std::fmt::Debug,
) -> String {
    use std::collections::BTreeMap;
    let mut files: Vec<&String> = markdown_files.iter().collect();
    files.sort();
    let mut winners: Vec<&String> = home_file_winners.iter().collect();
    winners.sort();
    let ordered = (
        files,
        page_map.iter().collect::<BTreeMap<_, _>>(),
        dir_overrides.iter().collect::<BTreeMap<_, _>>(),
        external_url_map.iter().collect::<BTreeMap<_, _>>(),
        winners,
        root_folder_name,
        format!("{site_scalars:?}"),
    );
    crate::build::facade::debug_hash(&ordered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::cache::FileStat;

    /// The store is process-global, and cargo runs these tests in parallel
    /// threads. Every test that touches it takes this lock.
    fn store_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn fingerprint_of(page_map: &HashMap<String, String>) -> String {
        inputs_fingerprint(
            &["a.md".to_string(), "b.md".to_string()],
            page_map,
            &HashMap::new(),
            &HashMap::new(),
            &std::collections::HashSet::new(),
            "site",
            &("en", true),
        )
    }

    #[test]
    fn inputs_fingerprint_is_stable_across_map_instances() {
        // Two HashMaps with the same contents hash the same even though their
        // iteration orders differ — the Stage 5a determinism bug, one phase
        // earlier. Build the second map in the opposite insertion order.
        let mut reversed = HashMap::new();
        for (k, v) in [("z.md", "z/index.html"), ("y.md", "y/index.html"), ("x.md", "x/index.html")] {
            reversed.insert(k.to_string(), v.to_string());
        }
        let forward = map(&[
            ("x.md", "x/index.html"),
            ("y.md", "y/index.html"),
            ("z.md", "z/index.html"),
        ]);
        assert_eq!(fingerprint_of(&forward), fingerprint_of(&reversed));
    }

    #[test]
    fn a_url_override_moves_the_inputs_fingerprint() {
        let before = map(&[("notes/index.md", "notes/index.html")]);
        let after = map(&[("notes/index.md", "writing/index.html")]);
        assert_ne!(fingerprint_of(&before), fingerprint_of(&after));
    }

    #[test]
    fn a_site_scalar_moves_the_inputs_fingerprint() {
        let page_map = map(&[("a.md", "a/index.html")]);
        let a = inputs_fingerprint(
            &["a.md".to_string()],
            &page_map,
            &HashMap::new(),
            &HashMap::new(),
            &std::collections::HashSet::new(),
            "site",
            &("en", true),
        );
        let b = inputs_fingerprint(
            &["a.md".to_string()],
            &page_map,
            &HashMap::new(),
            &HashMap::new(),
            &std::collections::HashSet::new(),
            "site",
            &("en", false),
        );
        assert_ne!(a, b);
    }

    #[test]
    fn a_site_typesetting_toggle_moves_the_inputs_fingerprint() {
        // A vertical page's body images declare a different `sizes=`, so a
        // `[site] typesetting` edit must not replay bodies parsed under the
        // old value.
        let page_map = map(&[("a.md", "a/index.html")]);
        let fingerprint = |typesetting: Option<&str>| {
            let cfg = crate::build::render::config::SiteConfig {
                typesetting: typesetting.map(String::from),
                ..Default::default()
            };
            let scalars = site_scalars(crate::i18n::Language::En, None, "", cfg.markdown(), false, false);
            let fp = inputs_fingerprint(
                &["a.md".to_string()],
                &page_map,
                &HashMap::new(),
                &HashMap::new(),
                &std::collections::HashSet::new(),
                "site",
                &scalars,
            );
            fp
        };
        assert_ne!(fingerprint(None), fingerprint(Some("vertical")));
    }

    #[test]
    fn a_new_file_moves_the_inputs_fingerprint() {
        let page_map = map(&[("a.md", "a/index.html")]);
        let one = inputs_fingerprint(
            &["a.md".to_string()],
            &page_map,
            &HashMap::new(),
            &HashMap::new(),
            &std::collections::HashSet::new(),
            "site",
            &(),
        );
        let two = inputs_fingerprint(
            &["a.md".to_string(), "b.md".to_string()],
            &page_map,
            &HashMap::new(),
            &HashMap::new(),
            &std::collections::HashSet::new(),
            "site",
            &(),
        );
        assert_ne!(one, two);
    }

    #[test]
    fn a_disabled_session_never_hits() {
        let _guard = store_lock();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "hello").unwrap();
        reset_for_tests();

        // Warm the store with a real entry.
        let warm = ParseSession::begin(
            dir.path(),
            &dir.path().join("hash-index.json"),
            true,
            "fp".to_string(),
        );
        assert!(warm.is_cold(), "first session in a process is always cold");
        let doc = ParsedDocument {
            source_path: Some("a.md".to_string()),
            ..Default::default()
        };
        warm.finish(std::slice::from_ref(&doc));

        // Same fingerprint, enabled → hit.
        let hot = ParseSession::begin(
            dir.path(),
            &dir.path().join("hash-index.json"),
            true,
            "fp".to_string(),
        );
        assert!(!hot.is_cold());
        assert!(hot.lookup("a.md").is_some());
        hot.finish(std::slice::from_ref(&doc));

        // `enabled == false` (a Full trigger, or MOSS_NO_INCREMENTAL) → cold.
        let disabled = ParseSession::begin(
            dir.path(),
            &dir.path().join("hash-index.json"),
            false,
            "fp".to_string(),
        );
        assert!(disabled.is_cold());
        assert!(disabled.lookup("a.md").is_none());
        disabled.finish(std::slice::from_ref(&doc));
        reset_for_tests();
    }

    #[test]
    fn an_edit_to_the_page_itself_misses() {
        let _guard = store_lock();
        let dir = tempfile::tempdir().unwrap();
        let index = dir.path().join("hash-index.json");
        std::fs::write(dir.path().join("a.md"), "hello").unwrap();
        reset_for_tests();
        let doc = ParsedDocument {
            source_path: Some("a.md".to_string()),
            ..Default::default()
        };

        ParseSession::begin(dir.path(), &index, true, "fp".to_string())
            .finish(std::slice::from_ref(&doc));
        std::fs::write(dir.path().join("a.md"), "hello, edited").unwrap();

        let session = ParseSession::begin(dir.path(), &index, true, "fp".to_string());
        assert!(session.lookup("a.md").is_none());
        session.finish(std::slice::from_ref(&doc));
        reset_for_tests();
    }

    /// A same-size edit in the same wall-clock second as the write the last build
    /// hashed is still an edit. The content hash comes from the stat-keyed index, and
    /// a stat that cannot tell the two writes apart replays the old parse.
    #[test]
    fn a_same_size_edit_in_the_same_second_misses() {
        let _guard = store_lock();
        let dir = tempfile::tempdir().unwrap();
        let index = dir.path().join("hash-index.json");
        let page = dir.path().join("a.md");
        std::fs::write(&page, "hello").unwrap();
        reset_for_tests();
        let doc = ParsedDocument {
            source_path: Some("a.md".to_string()),
            ..Default::default()
        };

        ParseSession::begin(dir.path(), &index, true, "fp".to_string())
            .finish(std::slice::from_ref(&doc));
        let before = std::fs::metadata(&page).unwrap().modified().unwrap();
        std::fs::write(&page, "world").unwrap();
        FileStat::stamp_in_the_second_of(&page, before);

        let session = ParseSession::begin(dir.path(), &index, true, "fp".to_string());
        assert!(session.lookup("a.md").is_none());
        session.finish(std::slice::from_ref(&doc));
        reset_for_tests();
    }

    /// A replace-via-rename — the atomic-save pattern — that carries the old mtime
    /// across and keeps the size: only the inode says the page is a different file.
    #[cfg(unix)]
    #[test]
    fn a_page_replaced_by_rename_with_its_size_and_mtime_kept_misses() {
        let _guard = store_lock();
        let dir = tempfile::tempdir().unwrap();
        let index = dir.path().join("hash-index.json");
        let page = dir.path().join("a.md");
        std::fs::write(&page, "hello").unwrap();
        reset_for_tests();
        let doc = ParsedDocument {
            source_path: Some("a.md".to_string()),
            ..Default::default()
        };

        ParseSession::begin(dir.path(), &index, true, "fp".to_string())
            .finish(std::slice::from_ref(&doc));
        FileStat::replace_by_rename_keeping_mtime(&page, b"world");

        let session = ParseSession::begin(dir.path(), &index, true, "fp".to_string());
        assert!(session.lookup("a.md").is_none(), "replayed the parse of the page that was replaced");
        session.finish(std::slice::from_ref(&doc));
        reset_for_tests();
    }

    /// What `compute` feeds the index: the file's whole stat record. A record that
    /// differs in any one field is not this file's — and the hash in it, planted here
    /// so a wrongly trusted entry is visible, must not come back. The exact record is
    /// the control: a hit is answered from the index without reading the file.
    #[test]
    fn the_hasher_trusts_an_entry_only_for_the_files_whole_stat_record() {
        let dir = tempfile::tempdir().unwrap();
        let page = dir.path().join("a.md");
        std::fs::write(&page, "hello").unwrap();
        let real = FileStat::of(&std::fs::metadata(&page).unwrap());
        let read = format!("{:x}", Sha256::digest(b"hello"));

        let hashed_with_entry_recorded_at = |recorded: &FileStat| {
            let mut index = HashIndex::new();
            index.update("a.md".to_string(), recorded, "planted".to_string());
            FileHasher::new(dir.path(), index).hash("a.md")
        };

        assert_eq!(hashed_with_entry_recorded_at(&real).as_deref(), Some("planted"), "control: the exact record hits");
        for (field, changed) in real.each_field_changed() {
            assert_eq!(
                hashed_with_entry_recorded_at(&changed).as_deref(),
                Some(read.as_str()),
                "an entry recorded for another {field} was trusted"
            );
        }
    }

    /// The stat is taken before the bytes are read, so a write landing during the read
    /// leaves an entry the file no longer matches. Recording the stat afterwards would
    /// pair the new file's stat with the old bytes' hash and vouch for it. The read is
    /// injected, so the write lands exactly between the two.
    #[test]
    fn the_hasher_records_the_stat_the_file_had_before_it_was_read() {
        let dir = tempfile::tempdir().unwrap();
        let page = dir.path().join("a.md");
        std::fs::write(&page, "hello").unwrap();
        let hasher = FileHasher::new(dir.path(), HashIndex::new());

        let hash = hasher.compute_with("a.md", |abs| {
            let bytes = std::fs::read(abs).ok();
            std::fs::write(abs, "rewritten while it was being hashed").unwrap();
            bytes
        });

        assert_eq!(hash, Some(format!("{:x}", Sha256::digest(b"hello"))), "the hash is of the bytes that were read");
        let now = FileStat::of(&std::fs::metadata(&page).unwrap());
        assert!(
            hasher.into_index().lookup("a.md", &now).is_none(),
            "the index vouches for the rewritten file with the hash of the old bytes"
        );
    }

    /// A page still in the cloud is not read to learn its hash. The desktop app's
    /// fail-fast policy turns that read into an error and a miss, but the headless CLI's
    /// watch rebuild would block on the download. A provider re-materialising a page
    /// changes its ctime and inode, so the index no longer vouches for it and a hasher
    /// that reads on a miss would read exactly the pages the provider just evicted. The
    /// entry here was recorded before that; the file is marked evicted once it is on disk.
    #[test]
    fn the_hasher_does_not_read_a_page_that_is_still_in_the_cloud() {
        let dir = tempfile::tempdir().unwrap();
        let page = dir.path().join("a.md");
        std::fs::write(&page, "hello").unwrap();
        let real = FileStat::of(&std::fs::metadata(&page).unwrap());
        let mut index = HashIndex::new();
        index.update("a.md".to_string(), &FileStat { ctime: Some(1), inode: Some(1), ..real }, "old".to_string());
        let hasher = FileHasher::new(dir.path(), index);
        let _cloud = crate::build::icloud::pretend::evicted(&page);

        let mut read_called = false;
        let hash = hasher.compute_with("a.md", |abs| {
            read_called = true;
            std::fs::read(abs).ok()
        });

        assert!(!read_called, "the hasher read a page that is still in the cloud");
        assert_eq!(hash, None, "a page that cannot be read cannot be proven unchanged");
    }

    #[test]
    fn an_edit_two_embed_hops_down_misses_the_top_page() {
        // index.md ← a.md ← b.md. Editing b.md must miss ALL THREE, which a
        // flat `embed_deps` filter on index.md would get wrong.
        let _guard = store_lock();
        let dir = tempfile::tempdir().unwrap();
        let index_json = dir.path().join("hash-index.json");
        for (name, body) in [("index.md", "top"), ("a.md", "mid"), ("b.md", "leaf")] {
            std::fs::write(dir.path().join(name), body).unwrap();
        }
        reset_for_tests();

        let doc = |path: &str, deps: &[(&str, &str)]| ParsedDocument {
            source_path: Some(path.to_string()),
            embed_deps: deps
                .iter()
                .map(|(t, s)| (t.to_string(), s.to_string()))
                .collect(),
            ..Default::default()
        };
        // As `resolve_embeds` reports it: nested pairs carry the intermediate
        // file as their source and bubble up to every ancestor.
        let docs = vec![
            doc("index.md", &[("a.md", "index.md"), ("b.md", "a.md")]),
            doc("a.md", &[("b.md", "a.md")]),
            doc("b.md", &[]),
        ];

        ParseSession::begin(dir.path(), &index_json, true, "fp".to_string()).finish(&docs);
        std::fs::write(dir.path().join("b.md"), "leaf, edited").unwrap();

        let session = ParseSession::begin(dir.path(), &index_json, true, "fp".to_string());
        assert!(session.lookup("b.md").is_none(), "the edited leaf");
        assert!(session.lookup("a.md").is_none(), "its direct embedder");
        assert!(
            session.lookup("index.md").is_none(),
            "the two-hop ancestor — the bug this design fixes"
        );
        session.finish(&docs);
        reset_for_tests();
    }

    #[test]
    fn an_unrelated_edit_leaves_an_embed_chain_hot() {
        let _guard = store_lock();
        let dir = tempfile::tempdir().unwrap();
        let index_json = dir.path().join("hash-index.json");
        for name in ["index.md", "a.md", "b.md", "other.md"] {
            std::fs::write(dir.path().join(name), "body").unwrap();
        }
        reset_for_tests();
        let doc = |path: &str, deps: &[(&str, &str)]| ParsedDocument {
            source_path: Some(path.to_string()),
            embed_deps: deps
                .iter()
                .map(|(t, s)| (t.to_string(), s.to_string()))
                .collect(),
            ..Default::default()
        };
        let docs = vec![
            doc("index.md", &[("a.md", "index.md"), ("b.md", "a.md")]),
            doc("a.md", &[("b.md", "a.md")]),
            doc("b.md", &[]),
            doc("other.md", &[]),
        ];
        ParseSession::begin(dir.path(), &index_json, true, "fp".to_string()).finish(&docs);
        std::fs::write(dir.path().join("other.md"), "body, edited").unwrap();

        let session = ParseSession::begin(dir.path(), &index_json, true, "fp".to_string());
        assert!(session.lookup("other.md").is_none());
        assert!(session.lookup("index.md").is_some());
        assert!(session.lookup("a.md").is_some());
        assert!(session.lookup("b.md").is_some());
        session.finish(&docs);
        reset_for_tests();
    }

    #[test]
    fn a_changed_inputs_fingerprint_disables_the_whole_build() {
        let _guard = store_lock();
        let dir = tempfile::tempdir().unwrap();
        let index_json = dir.path().join("hash-index.json");
        for name in ["a.md", "b.md"] {
            std::fs::write(dir.path().join(name), "body").unwrap();
        }
        reset_for_tests();
        let docs: Vec<ParsedDocument> = ["a.md", "b.md"]
            .iter()
            .map(|p| ParsedDocument {
                source_path: Some(p.to_string()),
                ..Default::default()
            })
            .collect();
        ParseSession::begin(dir.path(), &index_json, true, "fp-1".to_string()).finish(&docs);

        // Nothing on disk changed, but a folder index's `url:` moved page_map.
        let session = ParseSession::begin(dir.path(), &index_json, true, "fp-2".to_string());
        assert!(session.is_cold());
        assert!(session.lookup("a.md").is_none());
        assert!(session.lookup("b.md").is_none());
        session.finish(&docs);
        reset_for_tests();
    }
}
