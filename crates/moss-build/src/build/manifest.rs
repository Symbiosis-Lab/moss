//! Typestate manifest for the build lifecycle.
//!
//! `PendingManifest` accumulates artifact registrations during the render and
//! deferred processing phases. Calling [`PendingManifest::seal`] transitions it
//! to a [`SealedManifest`], which is the read-only deploy contract.
//!
//! The transition point is where the #552 invariant is checked:
//! every key in `blocking_keys` must also appear in `inner.files` OR
//! `inner.image_outputs`. Moving this check from per-site convention to a
//! single `seal()` call makes it impossible to deploy with a
//! partially-populated manifest.
//!
//! # Bucket semantics
//!
//! `inner.files` is always populated — it is the deploy wire manifest and every
//! registered artifact must reach the seta server. `HashBucket` controls only
//! which *additional* tracking set(s) receive the path (for stale-cleanup) and
//! whether `blocking_keys` is set (for HTML-page ordering). This is enforced
//! structurally in `register_with_hash`: the `inner.files` insert happens before
//! the match, so adding a new bucket variant cannot accidentally skip it.
//!
//! | Bucket           | `blocking_keys` | bucket set           |
//! |------------------|:---------------:|:--------------------:|
//! | `Files`          | yes             | —                    |
//! | `ImageOutputs`   | yes             | `image_outputs`      |
//! | `ImageVariants`  | no              | `image_outputs`      |
//! | `VideoOutputs`   | no              | `video_outputs`      |
//! | `NotebookOutputs`| no              | `notebook_outputs`   |
//!
//! (`inner.files` column omitted — it is always `yes` for every bucket.)
//!
//! The seal invariant is `blocking_keys ⊆ (files ∪ image_outputs)`.
//!
//! # History: three identical bugs closed by structural fix (2026-06-11)
//!
//! ImageOutputs (2026-05-15), ImageVariants (2026-05-15), and VideoOutputs
//! (2026-06-11) all had the same root cause: `inner.files.insert()` was an
//! opt-in per-bucket call that was easy to forget on new buckets. Each omission
//! caused the artifact to exist locally in `.moss/build/current/` but never reach
//! the seta server, producing 404s on the live site. The structural fix moves
//! `inner.files.insert()` unconditionally before the match statement so the
//! compiler enforces the invariant — a new bucket cannot skip deploy registration.

/// Reconstructs the published file set from the last deployed generation tree
/// when no publish record exists, so the resting change set carries a real
/// count instead of a blank. Owns the one entry point a seal calls.
pub mod backfill;
/// Publish-time classification of the hash state this module owns: which pages
/// the author added, edited or deleted since the last publish, and which moss
/// merely re-rendered. Pure — see its module docs for why it is not in `deploy`.
pub mod change_set;
/// What is LIVE — uid, URL and source path per page — read out of the publish
/// records for rename detection and duplicate-uid resolution. The record can be
/// withheld by a cloud provider, so "could not read it" is a value rather than
/// an empty map (ADR-062).
pub mod live_baseline;
/// Every root-relative `href`/`src` the emitted HTML asks for that this
/// manifest does not promise — the set difference moss#1187 was never taking.
/// Lives here because the sealed file set is what it reads; advisory only.
pub mod link_audit;
/// Durable record of what the last confirmed publish shipped, one per target
/// under `.moss/deploy/records/`. The other half of `change_set`'s diff.
pub mod published_record;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::build::assets::paths::{compute_binary_hash, compute_manifest_generation_id};
use crate::types::content::{file_entry, SiteHashes};

// ---------------------------------------------------------------------------
// HashBucket
// ---------------------------------------------------------------------------

/// Which registration bucket an artifact belongs to.
///
/// The bucket controls both which field of [`SiteHashes`] receives the entry
/// and whether the path is added to `blocking_keys` (the stale-cleanup guard).
/// See the module-level bucket semantics table for the exact mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashBucket {
    /// Source-derived page artifacts: HTML, CSS, JS, XML, JSON, SVG.
    /// Tracking: `blocking_keys` (HTML-page ordering for stale-cleanup).
    Files,
    /// Generated image outputs produced during the blocking phase (e.g., OG card PNGs).
    /// Tracking: `image_outputs` (stale-cleanup) + `blocking_keys` (HTML ordering).
    /// Distinct from `ImageVariants`: OG cards block rendering and enter `blocking_keys`;
    /// background `.webp` variants do not.
    ImageOutputs,
    /// Background-phase generated image variants (e.g., `.webp` from `run_image_conversion`).
    /// Tracking: `image_outputs` (stale-cleanup). NOT `blocking_keys` — stale-HTML
    /// cleanup must not treat these as HTML pages.
    ImageVariants,
    /// Transcoded video outputs (`.mp4`, `.thumb.jpg`).
    /// Tracking: `video_outputs` (stale-cleanup). NOT `blocking_keys`.
    VideoOutputs,
    /// Notebook render outputs (JupyterLite assets, viewer HTML, `.ipynb` copies).
    /// Tracking: `notebook_outputs` (stale-cleanup). NOT `blocking_keys`.
    NotebookOutputs,
}

// ---------------------------------------------------------------------------
// PendingManifest
// ---------------------------------------------------------------------------

/// Mutable manifest accumulator for the render and deferred-processing phases.
///
/// Created from a `carry_forward` snapshot (the previous build's [`SiteHashes`]).
/// The four **output buckets** (`files`, `image_outputs`, `video_outputs`,
/// `notebook_outputs`) start as the carry-forward snapshot but are
/// **mark-and-sweep pruned** at [`seal`][PendingManifest::seal] — only entries
/// whose path was registered via `register*()` (or `apply_message`) during this
/// build survive. The previous attempt at this invariant relied on the
/// on-disk stale-cleanup pass; that pass walks the output tree and cannot
/// reach into the in-memory `files` map, so orphan entries (e.g., from a
/// slug rename) leaked into successive `hashes.json` writes and broke
/// deploy with `"Manifest claims '<path>' exists but it's missing on disk."`
/// See `docs/archive/2026-05-18-manifest-integrity.md`.
///
/// The **cache fields** (`sources`, `builder_fingerprint`)
/// carry forward without sweeping — they belong to a separate concern
/// (change detection across builds) and are written/updated by other paths.
/// Their cross-build consistency is out of scope for `PendingManifest`; a
/// followup will split `SiteHashes` into a proper cache vs. manifest pair.
///
/// `source_to_output` is fully cleared on construction (see `new`).
///
/// Not [`Clone`] — exclusive ownership enforces that only one writer accumulates
/// into the manifest at a time. Call [`seal`][PendingManifest::seal] to finalize.
#[derive(Debug)]
pub struct PendingManifest {
    inner: SiteHashes,
    /// Paths that the blocking phase generated. Must be a subset of
    /// `(inner.files ∪ inner.image_outputs)` at seal time (the #552 invariant).
    /// Consumed by stale-HTML cleanup (C1–C4).
    blocking_keys: HashSet<String>,
    /// Output-bucket paths registered during this build (mark set). At seal
    /// time, the four output buckets are pruned to retain only keys in this
    /// set. Populated unconditionally by `register_with_hash`; not exposed
    /// outside the struct. See module docs for the rationale.
    touched: HashSet<String>,
    /// The PREVIOUS build's `source_to_output`, kept because `new` clears the
    /// live one. Read by [`carry_forward_deferred_page`], which is the only way
    /// to name the output of a page this build could not read — the mark-and-
    /// sweep is otherwise unconditional, and a page that emitted nothing is
    /// indistinguishable from a page that was deleted.
    ///
    /// [`carry_forward_deferred_page`]: PendingManifest::carry_forward_deferred_page
    carried_source_to_output: HashMap<String, String>,
    /// The PREVIOUS build's page-source hashes — every markdown entry in
    /// `inner.sources`. Read by [`carry_forward_page_source`] for pages this
    /// build never read.
    ///
    /// Projecting through the previous `source_to_output` keys, as this once
    /// did, excluded SLOT-ONLY sources — `footer.md` and its per-language
    /// siblings register a hash and deliberately no mapping. They dropped out
    /// of `sources` on the first warm build and stayed out, after which every
    /// sweep pass read each footer as a file with no baseline entry, i.e. a
    /// CREATE, and dispatched a full rebuild that could not clear it
    /// (harbor, 2026-08-20).
    ///
    /// [`carry_forward_page_source`]: PendingManifest::carry_forward_page_source
    carried_page_sources: HashMap<String, crate::build::types::SourceMetadata>,
    /// Page source paths whose hash THIS build registered (freshly read) or
    /// carried forward. Two jobs, both about keeping `sources` and
    /// `source_to_output` covering the same page set:
    ///
    /// 1. `replace_sources` (the deferred asset walk's bulk overwrite) restores
    ///    these entries — the walk `continue`s past markdown, so its map has no
    ///    page entries and its stale-prune would drop every one of them.
    /// 2. `seal` drops the previous build's page entries that are NOT in here,
    ///    which is exactly the set of deleted pages.
    page_sources: HashSet<String>,
    /// Outputs a producer could not verify (an I/O error that was not a
    /// positive `NotFound`), keyed by path with the error that stopped it. A
    /// generation carrying any of these is withheld — see
    /// `ship::ShipVerdict`.
    unverified: std::collections::BTreeMap<String, String>,
}

impl PendingManifest {
    /// Create a new pending manifest seeded with the previous build's hashes.
    ///
    /// Pass [`SiteHashes::default()`] for first builds.
    ///
    /// `source_to_output` is cleared on construction (NOT carried forward).
    /// Every build's HTML write loop re-registers every live document, so the
    /// only way for a stale entry to persist would be for a deleted source's
    /// mapping to leak into the next manifest. Pruning at construction enforces
    /// "source_to_output reflects only this build's live documents" as a hard
    /// invariant. Drain-time consumers compare against `previous_hashes` (the
    /// PRIOR build's on-disk manifest, loaded fresh by `load_previous_hashes`),
    /// which is unaffected by this clear.
    ///
    /// The other four output buckets (`files`, `image_outputs`, `video_outputs`,
    /// `notebook_outputs`) are NOT cleared here — they carry forward so
    /// mid-build readers (e.g. sitemap generation in
    /// `generate_blocking_content`) see a coherent view. They are mark-and-sweep
    /// pruned at [`seal`][PendingManifest::seal] using the `touched` mark set,
    /// so the on-disk manifest written via `SealedManifest::write_to_disk`
    /// contains only this build's emissions.
    pub fn new(carry_forward: SiteHashes) -> Self {
        let mut inner = carry_forward;
        // THIS build's racily-clean clock, not the carried-forward one: the
        // sweep compares file mtimes against the moment hashing began, and a
        // stale capture time would mark nothing suspect ever again.
        inner.captured_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs());
        let carried_source_to_output = std::mem::take(&mut inner.source_to_output);
        // The page half of `sources` is NOT `source_to_output`'s key set — a
        // slot-only source is a page source with no output. Select on the
        // extension, the property that does separate the halves: the deferred
        // asset walk that owns the other half never inserts markdown.
        let carried_page_sources: HashMap<String, crate::build::types::SourceMetadata> = inner
            .sources
            .iter()
            .filter(|(src, _)| {
                src.rsplit_once('.').is_some_and(|(_, ext)| {
                    crate::build::incremental_gates::is_markdown_extension(ext)
                })
            })
            .map(|(src, meta)| (src.clone(), meta.clone()))
            .collect();
        Self {
            inner,
            blocking_keys: HashSet::new(),
            touched: HashSet::new(),
            carried_source_to_output,
            carried_page_sources,
            page_sources: HashSet::new(),
            unverified: std::collections::BTreeMap::new(),
        }
    }

    /// Record that `rel_path`'s output could not be verified this build.
    pub(crate) fn mark_unverified(&mut self, rel_path: String, detail: String) {
        self.unverified.insert(rel_path, detail);
    }

    /// Keep a page's already-published HTML alive when this build could not read
    /// its source.
    ///
    /// A cloud-evicted page emits nothing, so it never reaches
    /// `register_with_hash` and the mark-and-sweep reads it as deleted — at
    /// which point three separate passes remove it: `stale_carried_forward`
    /// drops the hash, `seal` prunes the bucket, and `remove_stale_files`
    /// unlinks the file. The page 404s, and because `sealed.files()` is what
    /// deploy uploads, **publishing while a page is offline un-publishes it.**
    ///
    /// Re-registering the previous build's own entry is what makes the deferral
    /// honest: the page is still live and still served, moss simply could not
    /// re-derive it this time. The source→output name is the previous build's,
    /// which is the only place it exists — this build never parsed the file.
    ///
    /// `source_rel` is project-relative, matching `register_source_mapping`'s
    /// key domain. Returns the output key when one was reinstated, and `None`
    /// when the page has no published output to protect (a first build, or a
    /// page that has never rendered) — in which case there is nothing to lose.
    ///
    /// **Covers the page, not its satellites.** An OG card is a second output of
    /// the same source, and `source_to_output` is a one-to-one map; carrying the
    /// card forward too needs the multimap that `register_source_mapping`
    /// already documents as a prerequisite. Until then a deferred page's social
    /// card 404s for one build cycle and returns when the page does.
    pub fn carry_forward_deferred_page(&mut self, source_rel: &str) -> Option<String> {
        let key = self.carried_source_to_output.get(source_rel)?.clone();
        let entry = self.inner.files.get(&key)?.clone();
        self.register_with_hash(key.clone(), &entry, HashBucket::Files);
        // The mapping describes what is being served, and this page still is.
        self.inner.source_to_output.insert(source_rel.to_string(), key.clone());
        Some(key)
    }

    /// Register an artifact by computing its xxHash3 and routing it to the
    /// appropriate bucket(s) according to [`HashBucket`] semantics.
    ///
    /// `bytes` is the raw artifact content. The caller is responsible for
    /// writing the file to disk before (or after) calling this method —
    /// `register` only updates the in-memory manifest.
    ///
    /// `rel_path` is `&ServedPath` (not `String`) so the manifest cannot
    /// receive a non-normalized path. This is the public chokepoint that
    /// makes the deploy-time canonicalize match what's on disk regardless
    /// of source case. See `build::served_path` for design.
    pub fn register(&mut self, rel_path: &crate::build::served_path::ServedPath, bytes: &[u8], bucket: HashBucket) {
        let hash = compute_binary_hash(bytes);
        self.register_with_hash(rel_path.as_str().to_string(), &hash, bucket);
    }

    /// Apply a manifest registration from a pre-computed xxHash3 hex digest.
    ///
    /// Coordinator-only escape hatch: the `ManifestCoordinator` owns the
    /// `PendingManifest` exclusively and receives `EmitMessage`s that already
    /// carry a pre-computed hash (the background worker computed it before
    /// sending). This method is NOT part of the public API — use
    /// [`register`][PendingManifest::register] everywhere else.
    pub(crate) fn apply_message(&mut self, rel_path: String, hash: &str, bucket: HashBucket) {
        self.register_with_hash(rel_path, hash, bucket);
    }

    /// Bulk-replace the change-detection cache (`SiteHashes::sources`) with
    /// the map produced by this build's deferred-asset walk.
    ///
    /// `sources` is **cache state**, not output state — it records
    /// `(source_path, SourceMetadata)` so the next build can skip re-hashing
    /// unchanged files. It is NOT subject to mark-and-sweep at `seal()`.
    ///
    /// Before this method existed, `copy_deferred_assets` updated a local
    /// clone of `site_hashes.sources` and discarded it when the function
    /// returned. `PendingManifest::inner.sources` was therefore always
    /// whatever the previous build left in its carry-forward, the on-disk
    /// `hashes.json` was always one build stale, and `prev_sources` at the
    /// start of the next build was two builds stale — defeating the cache.
    ///
    /// Coordinator-only escape hatch (same access pattern as `apply_message`):
    /// the only caller is `ManifestCoordinator::run_until_drained`, which
    /// receives the bulk replacement via `EmitMessage::SourcesReplace`.
    ///
    /// **Page entries survive the replacement.** The walk `continue`s past
    /// markdown, so the map it sends carries no page hashes and its own
    /// stale-prune (`media/pipeline.rs`, against the walk's key set) would drop
    /// the ones `register_page_source_hash` just wrote. Re-inserting them here
    /// keeps the fix inside the type that owns the invariant, rather than
    /// asking the asset walk to know about pages.
    pub(crate) fn replace_sources(&mut self, sources: HashMap<String, crate::build::types::SourceMetadata>) {
        let pages: Vec<(String, crate::build::types::SourceMetadata)> = self
            .page_sources
            .iter()
            .filter_map(|k| self.inner.sources.get(k).map(|m| (k.clone(), m.clone())))
            .collect();
        self.inner.sources = sources;
        self.inner.sources.extend(pages);
    }

    /// Register a source-path → output-path mapping for a markdown page.
    ///
    /// Lets future consumers answer "what output did this source produce?"
    /// without re-deriving slug rules, language-suffix stripping, frontmatter
    /// `url:` overrides, or page_map promotion. Used by the file watcher's
    /// rename-hint resolver — see `build::watch::build_rebuild_event_with_renames`.
    ///
    /// Call this from the HTML write loop in `render::blocking` for every
    /// `ParsedDocument` whose `source_path` is `Some` (skip synthetic pages).
    ///
    /// `source_path` legitimately keeps source case (it's a key for lookup
    /// from a source path back to its output). `served_path` is `&ServedPath`
    /// so the right-hand value matches what's on disk after slug normalization.
    ///
    /// **Invariant: one source → one output.** Callers must not register the
    /// same source twice in one build; duplicate keys silently last-write-win
    /// here. The slug-change pairing in `build_rebuild_event_with_renames`
    /// assumes this — if a single source ever produces multiple outputs
    /// (paginated archives, per-tag variants), `source_to_output` must move to
    /// a multimap and the pairing loop in `watch.rs` must change in lockstep.
    pub fn register_source_mapping(&mut self, source_path: String, served_path: &crate::build::served_path::ServedPath) {
        self.inner.source_to_output.insert(source_path, served_path.as_str().to_string());
    }

    /// Record a page's raw-source-byte hash, keyed identically to
    /// [`register_source_mapping`][PendingManifest::register_source_mapping].
    ///
    /// The pair (`sources`, `source_to_output`) is what publish-time
    /// classification diffs: `source_to_output` says a page exists, `sources`
    /// says whether the author changed it. **Call these two in lockstep.** A
    /// page with a mapping and no hash has no previous hash to compare against,
    /// so the classifier can only read it as added or deleted — and on a
    /// watch-loop rebuild, where most of a large site is skipped, that renders
    /// as "400 deleted". See `carry_forward_page_source` for the skip paths.
    ///
    /// `sources` is asset-only otherwise: the deferred asset walk skips markdown
    /// before it ever inserts. This is the only writer of page entries.
    pub fn register_page_source_hash(&mut self, source_path: String, meta: crate::build::types::SourceMetadata) {
        self.page_sources.insert(source_path.clone());
        self.inner.sources.insert(source_path, meta);
    }

    /// Carry a page's source hash forward from the previous manifest, for pages
    /// this build did not re-read: the Stage-5b skip set, a parse-cache hit, or
    /// an iCloud deferral. The bytes are unchanged by definition on all three
    /// paths — that is *why* they were skipped — so the previous hash is the
    /// current hash.
    ///
    /// `source_rel` is project-relative, matching `register_source_mapping`'s
    /// key domain. Returns `None` when the previous manifest has no hash for
    /// this page (a first build, or one that predates page hashing), which
    /// leaves the page unclassifiable rather than misclassified — see the
    /// hash-carry-gap valve in `deploy::change_set`.
    pub fn carry_forward_page_source(&mut self, source_rel: &str) -> Option<()> {
        let meta = self.carried_page_sources.get(source_rel)?.clone();
        self.page_sources.insert(source_rel.to_string());
        self.inner.sources.insert(source_rel.to_string(), meta);
        Some(())
    }

    /// Set the plugin and builder fingerprints on the pending manifest. These
    /// flow through to the `SealedManifest::write_to_disk` output so the next
    /// build's `load_previous_hashes` can detect plugin/builder changes.
    ///
    /// Pre-#620 Item 2 the fingerprints were set on `site_result.hashes` and
    /// `deferred.site_hashes`, then written to `hashes.json` by the legacy
    /// on-disk fallback in `media/pipeline.rs::copy_deferred_assets`. With
    /// the fallback gone, the only writer is the seal+persist task, so the
    /// fingerprints must reach the pending manifest before it's sealed.
    pub fn set_builder_fingerprint(&mut self, builder_fingerprint: Option<String>) {
        self.inner.builder_fingerprint = builder_fingerprint;
    }

    /// Record the site URL this build resolved. Every page head, feed, and
    /// qr/*.svg embeds it, so the deploy guard compares this against the
    /// URL a publish would use NOW and rebuilds on mismatch.
    pub fn set_site_url(&mut self, url: &str) {
        self.inner.site_url = Some(url.to_string());
    }

    /// Clone the accumulated state into legacy parts without consuming `self`.
    ///
    /// A `pub(crate)` escape hatch for the blocking phase: after all Pattern A
    /// emits have routed through `BuildContext::emit`, `generate_blocking_content`
    /// needs the accumulated state back as `(SiteHashes, HashSet<String>)` for
    /// `SiteResult` and `BackgroundContext::blocking_keys` (#524 Phase 3). It
    /// clones because `pending` is `&mut` there, not owned, and the hatch is
    /// deliberately narrow — `inner` and `blocking_keys` stay private.
    /// Superseded by `seal()` once every background phase routes through
    /// `BuildContext`.
    pub(crate) fn as_parts_clone(&self) -> (SiteHashes, HashSet<String>) {
        (self.inner.clone(), self.blocking_keys.clone())
    }

    /// Read-only access to the accumulated `files` map.
    ///
    /// Used by `generate_blocking_content` for sitemap generation (reads HTML keys
    /// accumulated so far) without consuming the manifest or releasing the borrow.
    pub(crate) fn files(&self) -> &HashMap<String, String> {
        &self.inner.files
    }

    /// Read-only access to the accumulated `source_to_output` map.
    ///
    /// Used by `generate_blocking_content`'s post-emission debug-assert that
    /// catches missed registration — if a future HTML emission path emits
    /// for a `ParsedDocument` with `source_path: Some` but forgets to call
    /// `register_source_mapping`, this is how we'd detect it.
    pub(crate) fn source_to_output(&self) -> &HashMap<String, String> {
        &self.inner.source_to_output
    }

    /// The page source paths this build hashed or carried forward. Read by
    /// `generate_blocking_content`'s post-emission debug-assert, which checks
    /// that this set and `source_to_output`'s key set are the same — the
    /// lockstep invariant `register_page_source_hash` documents.
    pub(crate) fn page_sources(&self) -> &HashSet<String> {
        &self.page_sources
    }

    /// Read-only access to the accumulated `notebook_outputs` set.
    ///
    /// Read by `build_inner`'s stale-HTML sweep, which must not delete a
    /// JupyterLite asset carried forward from the previous build before
    /// `run_notebook_processing` has re-registered it.
    pub(crate) fn notebook_outputs(&self) -> &HashSet<String> {
        &self.inner.notebook_outputs
    }

    /// Re-mark the previous build's notebook outputs, for a build whose notebook
    /// run was cancelled before it produced any.
    ///
    /// Cancellation is not deletion, but `seal`'s mark-and-sweep drops every
    /// carry-forward key this build did not re-register, and the post-seal staging
    /// sweep then unlinks the files — so a folder switch mid-build would take a live
    /// JupyterLite bundle off the published site. Entries go back verbatim rather
    /// than through `register_with_hash`, which would prepend a second `100644:` to
    /// an already-prefixed one; verbatim is what keeps the sealed manifest
    /// byte-identical to the previous build here. Cancel only: a run that finished
    /// with fewer outputs really did lose a notebook. Before moss#618 the deferred
    /// asset walk masked both cases, re-emitting every key that survived its prune.
    pub(crate) fn carry_forward_notebook_outputs(&mut self, previous: &SiteHashes) {
        for key in &previous.notebook_outputs {
            let Some(entry) = previous.files.get(key) else { continue };
            self.touched.insert(key.clone());
            self.inner.files.insert(key.clone(), entry.clone());
            self.inner.notebook_outputs.insert(key.clone());
        }
    }

    /// Force a key into `blocking_keys` without a corresponding `files` entry.
    /// **Test-only.** Used to trigger the seal-time invariant violation.
    #[cfg(test)]
    pub(crate) fn force_blocking_key_for_test(&mut self, key: String) {
        self.blocking_keys.insert(key);
    }

    /// Register a path whose hash the caller already holds.
    ///
    /// Two callers, one shape. A **receipt**: the writer hashed the bytes as it
    /// wrote them (the notebook bundle's ~440 files, an injected page, a
    /// rasterized OG card), so re-reading them to recompute a digest moss
    /// already knows would be waste — and, under ADR-043, a read that can fail.
    /// A **carry-forward**: the previous build's manifest entry, verbatim, for
    /// an artifact this build did not re-emit (moss#922 Stage 5b — an
    /// incrementally skipped page keeps `index.html` exactly as the previous
    /// build wrote it, and without re-registration `remove_stale_html` deletes
    /// it). Same family as
    /// [`carry_forward_deferred_page`][PendingManifest::carry_forward_deferred_page]:
    /// an output that is on disk and is this build's responsibility, without
    /// its bytes being in hand.
    ///
    /// `hash` must be the digest [`register`][PendingManifest::register] would
    /// have produced (or a previous entry, mode prefix and all); a wrong one
    /// makes the deploy diff read an unchanged file as changed, or the reverse.
    /// **Callers must have established that the file is on disk** — this method
    /// cannot, and a manifest entry with no file fails the whole publish.
    pub fn register_hashed(
        &mut self,
        rel_path: &crate::build::served_path::ServedPath,
        hash: &str,
        bucket: HashBucket,
    ) {
        self.register_with_hash(rel_path.as_str().to_string(), hash, bucket);
    }

    fn register_with_hash(&mut self, rel_path: String, hash: &str, bucket: HashBucket) {
        // Mark-and-sweep mark step: record that this build owns `rel_path`.
        // Every output-bucket entry must pass through this chokepoint
        // (`register`, `apply_message`, and the bucket arms below all call
        // `register_with_hash`). At seal time, the four output buckets are
        // pruned to retain only paths in `touched`, so any carry-forward
        // entry from the previous build that this build did not re-register
        // is dropped. See module docs and
        // `docs/archive/2026-05-18-manifest-integrity.md`.
        self.touched.insert(rel_path.clone());

        // Unconditional: every artifact must reach the deploy wire manifest.
        // `inner.files` is the single source `deploy.rs` reads via `sealed.files()`.
        // Inserting here — before the match — makes it structurally impossible for
        // a new bucket to accidentally skip deploy registration.
        //
        // A hash may arrive already carrying a mode prefix: verbatim from the
        // previous manifest (a carry-forward), or `120000:` for a symlink sent
        // by `copy_deferred_assets`. `file_entry()` unconditionally prepends
        // `100644:`, which would make that a malformed `100644:120000:<hash>`,
        // so an existing prefix is preserved instead. A freshly computed hash
        // is 32 hex characters and can never match, in any bucket.
        let entry = if hash.starts_with("100644:") || hash.starts_with("120000:") {
            hash.to_string()
        } else {
            file_entry(hash)
        };
        self.inner.files.insert(rel_path.clone(), entry);

        // Bucket-specific tracking: stale-cleanup sets and blocking_keys only.
        match bucket {
            HashBucket::Files => {
                self.blocking_keys.insert(rel_path);
            }
            HashBucket::ImageOutputs => {
                // OG cards: image_outputs (stale-cleanup) + blocking_keys (HTML ordering).
                self.inner.image_outputs.insert(rel_path.clone());
                self.blocking_keys.insert(rel_path);
            }
            HashBucket::ImageVariants => {
                // Background .webp variants: image_outputs (stale-cleanup), no blocking_keys.
                self.inner.image_outputs.insert(rel_path);
            }
            HashBucket::VideoOutputs => {
                // video_outputs for stale-cleanup; not blocking (not an HTML page).
                self.inner.video_outputs.insert(rel_path);
            }
            HashBucket::NotebookOutputs => {
                // notebook_outputs for stale-cleanup; not blocking (not an HTML page).
                self.inner.notebook_outputs.insert(rel_path);
            }
        }
    }

    /// Seal the manifest, enforcing the `blocking_keys ⊆ (files ∪ image_outputs)`
    /// invariant and mark-and-sweep pruning the four output buckets.
    ///
    /// The debug-assert fires in test / debug builds if any key was added to
    /// `blocking_keys` without a matching entry in either `inner.files` or
    /// `inner.image_outputs`. OG cards (Sites 4, 14b) land in `image_outputs` +
    /// `blocking_keys` but NOT `files`, so the invariant must cover both sets.
    /// In release builds the assert is compiled out.
    ///
    /// After the invariant check, the four output buckets (`files`,
    /// `image_outputs`, `video_outputs`, `notebook_outputs`) are pruned
    /// to retain only entries whose path was registered via `register*` /
    /// `apply_message` during this build (the `touched` mark set). Carry-forward
    /// entries from the previous build that this build did not re-emit are
    /// dropped here, so the sealed manifest reflects only this build's
    /// emissions. The fingerprint cache fields and `source_to_output` (cleared
    /// at construction and re-populated on every live document) are not pruned.
    /// `sources` is pruned only of its **page** half — see the loop below; its
    /// asset half is change-detection cache with its own prune in the asset walk.
    pub fn seal(self) -> SealedManifest {
        debug_assert!(
            self.blocking_keys.iter().all(|k| {
                self.inner.files.contains_key(k) || self.inner.image_outputs.contains(k)
            }),
            "blocking_keys ⊆ (files ∪ image_outputs) invariant violated at seal time"
        );
        let mut inner = self.inner;
        let touched = self.touched;
        // A page the previous build hashed and this build neither re-read nor
        // carried forward is a deleted page. Its hash must go with it, or the
        // classifier reports the same deletion on every publish forever.
        // Restricted to the previous build's page keys, so asset entries — which
        // are cache state with their own prune in the asset walk — are untouched.
        for src in self.carried_source_to_output.keys() {
            if !self.page_sources.contains(src) {
                inner.sources.remove(src);
            }
        }
        inner.files.retain(|k, _| touched.contains(k));
        inner.image_outputs.retain(|k| touched.contains(k));
        inner.video_outputs.retain(|k| touched.contains(k));
        inner.notebook_outputs.retain(|k| touched.contains(k));
        let generation_id = compute_manifest_generation_id(&inner.files);
        SealedManifest {
            inner,
            blocking_keys: self.blocking_keys,
            generation_id,
            unverified: self.unverified,
        }
    }
}

// ---------------------------------------------------------------------------
// SealedManifest
// ---------------------------------------------------------------------------

/// Read-only manifest produced by [`PendingManifest::seal`].
///
/// The type-level transition from `PendingManifest` → `SealedManifest`
/// is the deploy contract: callers that accept `&SealedManifest` know all
/// artifact registrations are complete and the invariant has been checked.
#[derive(Debug, Clone)]
pub struct SealedManifest {
    inner: SiteHashes,
    blocking_keys: HashSet<String>,
    /// Content-derived identity computed at seal time. 16 lowercase hex chars
    /// (xxh3_64 over BTreeMap-sorted `"{path}\x00{entry_value}\n"` pairs).
    /// Same `files()` content → same id across sessions and build modes.
    generation_id: String,
    /// Every output a producer or the presence pass could not verify, with the
    /// error — never persisted; it exists to withhold this generation.
    unverified: std::collections::BTreeMap<String, String>,
}

impl SealedManifest {
    /// Outputs this generation could not verify, path → error. Non-empty means
    /// the generation must not be promoted (`ship::ShipVerdict`).
    pub fn unverified(&self) -> &std::collections::BTreeMap<String, String> {
        &self.unverified
    }

    /// Record an output the presence pass could not verify. Its entry is kept:
    /// an unreadable output is not a missing one.
    pub(crate) fn mark_unverified(&mut self, rel_path: String, detail: String) {
        self.unverified.insert(rel_path, detail);
    }

    /// All output-path → mode-tagged-hash entries in the deploy manifest.
    pub fn files(&self) -> &HashMap<String, String> {
        &self.inner.files
    }

    /// Source-relative path → hash of the bytes that produced it.
    ///
    /// Two populations share this map: **pages** (written by
    /// [`PendingManifest::register_page_source_hash`], keyed identically to
    /// [`source_to_output`][SealedManifest::source_to_output]) and **assets**
    /// (written by the deferred media walk, which is where the map started).
    /// Publish-time classification reads the page half; the asset half is
    /// change-detection cache. Both are SHA-256 hex, one hash domain.
    pub fn sources(&self) -> &HashMap<String, crate::build::types::SourceMetadata> {
        &self.inner.sources
    }

    /// Source-relative path → the output path that page was rendered to.
    ///
    /// Its key set is the site's live page set: publish-time classification
    /// reads "in here, not in the previous snapshot" as *added* and the
    /// converse as *deleted*, so a page missing a mapping is invisible to the
    /// change set rather than misreported.
    pub fn source_to_output(&self) -> &HashMap<String, String> {
        &self.inner.source_to_output
    }

    /// The site URL this generation was built with; `None` when the
    /// manifest predates the field (pre-upgrade hashes.json).
    pub fn site_url(&self) -> Option<&str> {
        self.inner.site_url.as_deref()
    }

    /// The content-derived generation identity computed at seal time.
    /// 16 lowercase hex characters. Stable: same `files()` content always
    /// produces the same `generation_id` regardless of insertion order.
    pub fn generation_id(&self) -> &str {
        &self.generation_id
    }

    /// Paths generated by the blocking phase. Used by stale-HTML cleanup.
    pub fn blocking_keys(&self) -> &HashSet<String> {
        &self.blocking_keys
    }

    /// Paths of generated image variants (`.webp`, OG card PNGs, …).
    pub fn image_outputs(&self) -> &HashSet<String> {
        &self.inner.image_outputs
    }

    /// Paths of generated video outputs (`.mp4`, `.thumb.jpg`, …).
    pub fn video_outputs(&self) -> &HashSet<String> {
        &self.inner.video_outputs
    }

    /// Paths of notebook render outputs (JupyterLite assets, viewer HTML, …).
    pub fn notebook_outputs(&self) -> &HashSet<String> {
        &self.inner.notebook_outputs
    }

    /// Borrow the inner [`SiteHashes`] for read-only consumers like
    /// `remove_stale_files` that need the full bucket-set view.
    ///
    /// Used by the seal+persist side task in `build.rs` to run stale-file
    /// cleanup AFTER background workers' EmitMessages have been merged into
    /// the manifest, closing the deferred-phase race in #621.
    pub fn site_hashes_view(&self) -> &SiteHashes {
        &self.inner
    }

    /// Apply post-seal HTML rewrites (moss#867 honest degradation): update
    /// the hash entry for each rewritten page and recompute `generation_id`
    /// so the corrected content becomes deploy-visible.
    ///
    /// `rewrites` maps served path → the raw (unprefixed) content hash of
    /// the bytes the SITE will serve — that is, the new staged bytes after
    /// `ship::apply_transform`, not the staged bytes themselves. Staging
    /// keeps the preview annotations the generation strips, so hashing what
    /// was just written to `stage_dir` describes a file that never exists
    /// and deploy's integrity check refuses the whole upload.
    ///
    /// Otherwise mirrors `register_with_hash`'s `Files`-bucket path (never
    /// carries a pre-existing `100644:`/`120000:` prefix; HTML pages are
    /// always freshly generated, never copied verbatim like a symlink). A
    /// no-op (generation_id unchanged) when `rewrites` is empty.
    pub fn apply_post_seal_rewrites(&mut self, rewrites: HashMap<String, String>) {
        if rewrites.is_empty() {
            return;
        }
        for (path, hash) in rewrites {
            self.inner.files.insert(path, file_entry(&hash));
        }
        self.generation_id = compute_manifest_generation_id(&self.inner.files);
    }

    /// Drop pruned image-variant keys from the manifest (moss#976 B2): the
    /// bytes were deleted from `stage_dir` by `media::orphan_prune` because
    /// nothing in the emitted output references them, so the manifest must
    /// stop advertising them too — otherwise the NEXT build's incremental
    /// diff sees a `files`/`image_outputs` entry for a path that no longer
    /// exists in the generation. Recomputes `generation_id` for the same
    /// reason [`apply_post_seal_rewrites`] does: the file set changed.
    ///
    /// Must run BEFORE [`write_to_disk`][Self::write_to_disk] — the whole
    /// point is that the persisted manifest matches what
    /// `ship::materialize_and_promote` actually copies.
    /// Record the ship-time prune's verdict for the next build's producers —
    /// see `SiteHashes::pruned_image_outputs`.
    ///
    /// Deliberately NOT folded into `remove_entries`: that call
    /// carries only what THIS build removed, and a converged build removes
    /// nothing. Accumulating is the caller's job, since only the caller also
    /// knows what is still unreferenced.
    pub fn set_pruned_image_outputs(&mut self, keys: HashSet<String>) {
        self.inner.pruned_image_outputs = keys;
    }

    /// Drop `keys` from the generation, from every bucket — an entry left in
    /// one the caller did not think of names a path the generation does not
    /// contain, and deploy refuses the whole upload on one of those. The
    /// generation id is recomputed so a manifest that loses an entry after
    /// seal gets a new identity.
    pub fn remove_entries(&mut self, keys: &HashSet<String>) {
        if keys.is_empty() {
            return;
        }
        for key in keys {
            self.inner.files.remove(key);
            self.inner.image_outputs.remove(key);
            self.inner.video_outputs.remove(key);
            self.inner.notebook_outputs.remove(key);
            self.blocking_keys.remove(key);
        }
        self.generation_id = compute_manifest_generation_id(&self.inner.files);
    }

    /// Serialize `inner` as pretty JSON and write to `path`.
    ///
    /// Matches the format written by the existing pipeline
    /// (`serde_json::to_string_pretty(&site_hashes)` in `media/pipeline.rs`).
    pub fn write_to_disk(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(&self.inner)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        crate::build::io_utils::write_output(path, json.as_bytes())
    }

    /// Unwrap into the underlying [`SiteHashes`].
    ///
    /// For compatibility with call sites that still expect raw `SiteHashes`.
    /// `blocking_keys` is dropped here — it is not part of `SiteHashes`.
    pub fn into_inner(self) -> SiteHashes {
        self.inner
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "manifest_tests.rs"]
mod tests;
