//! Ship: turn a sealed manifest into a generation directory.
//!
//! See the module-level architecture section in `build.rs` for the staging /
//! generations model. This file owns everything between "the manifest is
//! sealed" and "`current` points at a new generation":
//!
//! - the two post-seal passes that make the manifest and the disk agree —
//!   `prune_orphaned_webp_before_ship` and `drop_absent_outputs`. Both act on
//!   the MANIFEST only: staging is what the preview server is reading while
//!   this runs, so nothing here unlinks from it (see `build::pipeline`'s
//!   pre-render sweep);
//! - [`ship_phase`], which walks the sealed entries and derives each generation
//!   file from its staged bytes (apply transform, or recreate a symlink);
//! - [`materialize_and_promote`], which runs the above into a fresh
//!   generation dir — seeded, where the platform can clone a directory, from
//!   the last whole one, so only the entries that differ are written — or
//!   finds it already whole on disk, and repoints `current`. Generation GC is
//!   `store_gc`'s.
//!
//! The manifest is the input, not the directory: a file in staging that no
//! entry names — a sync client's conflicted copy, a stale output from an
//! earlier build — is not shipped and cannot reach the published site.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use crate::build::manifest::SealedManifest;

// Defined in `served_path` so `manifest` can ask them without depending on this
// module, which depends on `manifest`; re-exported so callers keep saying
// `ship::transform_for`.
pub use crate::build::served_path::{transform_for, ShipTransform};

// ---------------------------------------------------------------------------
// Regex constants — the one definition. A second copy lived in
// `media/pipeline.rs` until its last caller (`sync_dir`) was deleted.
// ---------------------------------------------------------------------------

/// Bump when any regex below changes, or when `apply_transform` changes what it
/// does with them.
///
/// The slot-injection cache stores the manifest hash of the *transformed* bytes
/// (`enhance.rs`), computed once at miss time. A cache hit then reuses that hash
/// without re-running the transform — so if the transform changes underneath a
/// warm cache, every hit page gets a manifest entry describing bytes moss no
/// longer produces, and deploy, which byte-verifies each upload against the
/// manifest hash, refuses the whole publish. This constant is part of the cache
/// key, so bumping it invalidates those records instead.
pub const SHIP_TRANSFORM_REV: u32 = 3;

/// Strips preview-only `data-source-*` attributes from HTML.
/// Matches: data-source-line="N", data-source-range="N-M", data-source-fm="field", data-source-none.
static STRIP_SOURCE_LINE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"\s+data-source-(?:line="\d+"|range="\d+-\d+"|fm="[^"]*"|none)"#).unwrap()
});

/// Strips the preview-only `data-moss-preview` attribute from `<body>`.
static STRIP_PREVIEW_ATTR: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"\s+data-moss-preview(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]*))?(\s|>)"#)
        .unwrap()
});

// ---------------------------------------------------------------------------
// apply_transform
// ---------------------------------------------------------------------------

/// Strip preview-only `data-source-*` attributes from an HTML fragment.
///
/// Used for injected chrome (footer / slot HTML) that is rendered from a
/// DIFFERENT source file than the page it lands in. Its `data-source-line`
/// values reference the *slot* file's lines, but the editor's scroll-sync
/// searches every `[data-source-line]` element in the page assuming they belong
/// to the file being edited. A footer annotated `data-source-line="1"` renders
/// at the page bottom, so syncing the editor to line 1 would jump the preview to
/// the bottom. Stripping the slot HTML at collection keeps the host page's own
/// annotations authoritative.
pub fn strip_source_annotations(html: &str) -> std::borrow::Cow<'_, str> {
    STRIP_SOURCE_LINE.replace_all(html, "")
}

/// Apply the production transform to in-memory bytes. Invalid UTF-8 in the
/// stripping path is a no-op (input returned unchanged).
///
/// The enum survives the deletion of its payload because it still names the
/// two *I/O paths* ship takes: `CopyAsIs` never reads the file (`fs::copy` is
/// COW on APFS/Btrfs and the assets are large), `StripPreviewAttrs` must.
///
/// Deliberately does NOT strip `data-moss-deploy-only`: that attribute marks
/// a `<script>` a deployed site must still execute (the pageview beacon, the
/// site-wide analytics tag), and it is what lets the preview server strip
/// that same script from a page it serves out of a generation THIS transform
/// already ran on — see `ops::serve::iframe_bridge::strip_preview_only_scripts`.
pub fn apply_transform(transform: ShipTransform, bytes: &[u8]) -> Vec<u8> {
    match transform {
        ShipTransform::CopyAsIs => bytes.to_vec(),
        ShipTransform::StripPreviewAttrs => {
            let Ok(s) = std::str::from_utf8(bytes) else {
                // Not valid UTF-8 — preserve bytes as-is and log.
                log::debug!("[ship] HTML file is not valid UTF-8, skipping strip transform");
                return bytes.to_vec();
            };
            let after_source = STRIP_SOURCE_LINE.replace_all(s, "");
            let after_preview = STRIP_PREVIEW_ATTR.replace_all(&after_source, "$1");
            after_preview.into_owned().into_bytes()
        }
    }
}

// ---------------------------------------------------------------------------
// Ship-by-OID: read from an immutable CAS blob instead of the mutable stage
// path, when one is known to back this entry's exact bytes.
// ---------------------------------------------------------------------------

/// Where [`ship_phase`] and [`drop_absent_outputs`] read one entry's bytes from.
enum ShipRead<'a> {
    /// A file: the CAS blob backing a live `staged_oid`, or `stage_path` itself
    /// when there is none (or its CAS blob has since been collected).
    Path(PathBuf),
    /// The bytes the build kept on the manifest. Present by construction, and
    /// nothing on disk to probe, audit or copy.
    Held(&'a [u8]),
}

/// The one place [`ship_phase`] and [`drop_absent_outputs`] read `rel_path`'s
/// bytes from: held bytes, else the CAS blob backing a live `staged_oid`, else
/// `stage_path` itself.
///
/// Both callers MUST route every presence check and every subsequent read
/// through this SAME resolved value, and neither may recompute it separately.
/// A presence check that asks the CAS while the read that follows targets the
/// stage path (or vice versa) can answer "present" from one and then read the
/// other, genuinely-absent, one — moving the failure a few lines down instead
/// of preventing it, which is exactly the bug a bolted-on presence-only
/// helper would reintroduce. See the module docs for the race this exists to
/// close: between a build sealing a path's hash and shipping its bytes, a
/// second concurrent build can rewrite the mutable stage copy.
fn resolve_ship_source<'a>(
    rel_path: &str,
    stage_path: &Path,
    sealed: &'a SealedManifest,
    object_store: Option<&crate::build::cache::ObjectStore>,
) -> ShipRead<'a> {
    if let Some(bytes) = sealed.held_bytes(rel_path) {
        return ShipRead::Held(bytes);
    }
    if let Some(store) = object_store {
        if let Some(oid) = sealed.staged_oid(rel_path) {
            if let Some(cas_path) = store.get_path(oid) {
                return ShipRead::Path(cas_path);
            }
        }
    }
    ShipRead::Path(stage_path.to_path_buf())
}

/// Compare `rel_path`'s CURRENT stage bytes against what this manifest sealed,
/// for an entry [`ship_phase`] is about to read from the mutable stage path
/// (i.e. one with no live `staged_oid` — [`resolve_ship_source`] fell back).
///
/// A cheap stat match is the common case: no read, no hash, `None`. A stat
/// disagreement demotes to a real hash — computed against the file's current
/// bytes, same discipline `SourceMetadata`'s racy-mtime fast path uses
/// (`build/types.rs`) — because a stat change since seal is exactly what a
/// concurrent build's overwrite produces. Returns the real hash ONLY when it
/// genuinely disagrees with the sealed entry; a stat change that still hashes
/// to the same content (a touch, a benign re-save) is not a race and returns
/// `None` too. The caller never withholds on this — it only logs — so a
/// routine, non-concurrent rewrite (`degrade::apply_to_staging`) re-stamps the
/// fingerprint at write time and never reaches this branch at all.
fn verify_ship_integrity(
    rel_path: &str,
    stage_path: &Path,
    sealed_entry: &str,
    sealed: &SealedManifest,
) -> Option<String> {
    let expected_fp = sealed.ship_fingerprint(rel_path)?;
    let meta = std::fs::metadata(stage_path).ok()?;
    let actual_fp = crate::build::stat::FileStat::of(&meta);
    if actual_fp == *expected_fp {
        return None;
    }
    let bytes = std::fs::read(stage_path).ok()?;
    // Hash what `ship_phase` would actually SHIP, not the raw stage bytes:
    // the manifest's registered hash is of the bytes after `apply_transform`
    // (staging keeps preview annotations an HTML page's shipped copy does
    // not — same reason `apply_post_seal_rewrites`'s callers hash the
    // transformed bytes, never the staged ones). Comparing raw stage bytes
    // here would report a "mismatch" for every ordinary annotated page.
    let shipped = apply_transform(transform_for(rel_path), &bytes);
    let real_hash = crate::build::assets::paths::compute_binary_hash(&shipped);
    let expected_hash = crate::types::content::parse_entry(sealed_entry).1;
    if real_hash == expected_hash {
        None
    } else {
        Some(real_hash)
    }
}

// ---------------------------------------------------------------------------
// ship_phase  (batched, once per build, after the seal)
// ---------------------------------------------------------------------------

/// Ship one generation: copy exactly what the sealed manifest lists.
///
/// Each entry ships from the bytes the manifest holds (`sealed.held_bytes`) or
/// from its immutable CAS blob when the manifest recorded one
/// (`sealed.staged_oid`, still live) — see [`resolve_ship_source`] — and
/// from the mutable `stage_dir` copy otherwise, exactly as before. The
/// immutable sources are what close a real race: `stage_dir` is shared and mutable across
/// concurrent builds of the same folder, so a second build can rewrite a path
/// between this build sealing its hash and this call reading its bytes, and
/// the generation would then receive the wrong bytes under a frozen hash. An
/// entry with no live `staged_oid` still gets a best-effort audit —
/// [`verify_ship_integrity`] — that can only log the disagreement, never
/// withhold on it.
///
/// The manifest is the single owner of what a generation contains. This used
/// to walk `stage_dir` and ship whatever was there, which made the disk a
/// second owner — and the disk holds things moss did not write. A Dropbox
/// conflicted copy of a page, or a half-synced file the provider has evicted,
/// was copied into the generation on the strength of being present, and one
/// failed copy returned `Err` and blocked promotion of an otherwise complete
/// build. Iterating the manifest instead, a file moss did not register is
/// never copied, never served, and is swept after the seal as unregistered.
///
/// Every bucket's registration also inserts into `files` (`manifest.rs`), so
/// `sealed.files()` is the whole generation and the per-bucket chain the old
/// test-only `ship` carried is redundant.
///
/// The mode prefix decides the arm, not the extension. `copy_output` is
/// `fs::copy` and follows links, so shipping a `120000:` entry through the
/// file arm would put a regular file under an entry that says symlink and
/// break deploy's `MODE_SYMLINK` handling.
///
/// - `120000:` — `read_link` and recreate the link (unix); copy the target's
///   content on Windows, where symlinks need elevation.
/// - `100644:` — [`ShipTransform`], as before: strip preview attributes from
///   HTML, `fs::copy` everything else. The copy is COW on APFS/Btrfs and
///   gives independent inodes: a hardlink-based ship let one iCloud eviction
///   zero both stage and generation, leaving permanent 0-byte stubs.
///
/// An entry whose stage file is not present is skipped, not counted: the
/// presence pass leaves exactly one such class behind (`_moss/math/`, kept on
/// purpose), and reading an evicted source is the fault this whole change
/// exists to stop being fatal. What remains countable is a write fault on the
/// generation side — per-file faults are counted, not raised, so one bad entry
/// does not hide the rest, and a non-zero count returns `Err` so the
/// generation is never promoted.
///
/// `seeded` is the whole generation `site_dir` was cloned from, if any: an
/// entry it still holds unchanged is not written again — see
/// [`crate::build::store_gc::WholeGeneration::still_holds`].
///
/// `Ok` carries how many entries shipped bytes other than the ones their
/// manifest hash names — see [`verify_ship_integrity`].
pub fn ship_phase(
    stage_dir: &Path,
    site_dir: &Path,
    sealed: &SealedManifest,
    object_store: Option<&crate::build::cache::ObjectStore>,
    seeded: Option<&crate::build::store_gc::WholeGeneration>,
) -> std::io::Result<usize> {
    // Count per-file faults so a PARTIAL materialize reports failure (Err),
    // not success. Fix B's mat_ok gate relies on this: a partial generation
    // must fall back to last-known-good, never be promoted or advertised.
    let mut failures = 0u32;
    let mut drifted = 0;

    for (rel_path, entry) in sealed.files() {
        let stage_path = stage_dir.join(rel_path);
        let site_path = site_dir.join(rel_path);
        if seeded.is_some_and(|base| base.still_holds(rel_path, entry, &site_path)) {
            continue;
        }
        // The ONE source every check and read below uses. Held bytes are
        // themselves; a live `staged_oid` resolves to its immutable CAS blob;
        // everything else resolves to `stage_path` unchanged. See
        // `resolve_ship_source`'s doc comment for why a second,
        // independently-computed path here would reopen the exact race this
        // function exists to close.
        let source_path = match resolve_ship_source(rel_path, &stage_path, sealed, object_store) {
            ShipRead::Path(path) => path,
            // Already in memory, and always a plain file (`register_held` takes
            // nothing else): write it and skip the presence probe and audit,
            // both of which are about a stage file this entry does not read.
            ShipRead::Held(bytes) => {
                if let Err(e) = crate::build::io_utils::write_output(&site_path, bytes) {
                    log::warn!("[ship_phase] write failed for {:?}: {}", site_path, e);
                    failures += 1;
                }
                continue;
            }
        };
        let (mode, _) = crate::types::content::parse_entry(entry);

        // `drop_absent_outputs` has already removed every entry with no output
        // — except the `_moss/math/` exemption it keeps deliberately,
        // whose bytes may be evicted. Reading one here is the
        // EDEADLK that failed the whole generation and left `current` where it
        // was, so the exemption is skipped rather than copied.
        //
        // The two passes ask one predicate, but they must not collapse to one
        // verdict: anything else that is absent vanished between them, and
        // promoting a generation short of a file its own manifest names only
        // moves the failure to the next publish. That stays a counted failure.
        if !crate::build::io_utils::entry_output_present(&source_path, mode) {
            if rel_path.starts_with(crate::build::served_path::MATH_PNG_PREFIX) {
                continue;
            }
            log::warn!(
                "[ship_phase] {:?} is named by the manifest but is not an output; \
                 it went absent after the presence pass",
                source_path
            );
            failures += 1;
            continue;
        }

        // Only meaningful when `source_path` fell back to the mutable stage
        // copy: a CAS-backed entry is immutable by construction and a
        // symlink/no-fingerprint entry returns `None` immediately — see
        // `verify_ship_integrity`. Never gates shipping; it only makes a
        // seal-to-ship race audible.
        if source_path == stage_path {
            if let Some(real_hash) = verify_ship_integrity(rel_path, &stage_path, entry, sealed) {
                drifted += 1;
                log::warn!(
                    "[ship_phase] {:?} changed after this manifest sealed (now hashes to {}, \
                     manifest says {}) — shipping the current bytes rather than withholding the \
                     page; a concurrent build most likely rewrote this path",
                    stage_path, real_hash, entry
                );
            }
        }

        if let Some(parent) = site_path.parent() {
            if let Err(e) = crate::build::io_utils::create_output_dir_all(parent) {
                log::warn!("[ship_phase] create_dir_all for parent {:?}: {}", parent, e);
                failures += 1;
                continue;
            }
        }
        if mode == crate::types::content::MODE_SYMLINK {
            match std::fs::read_link(&source_path) {
                Ok(target) => {
                    #[cfg(unix)]
                    {
                        if let Err(e) = crate::build::io_utils::replace_with_symlink(&target, &site_path) {
                            log::warn!("[ship_phase] Failed to recreate symlink at {:?}: {}", site_path, e);
                            failures += 1;
                        }
                    }
                    #[cfg(windows)]
                    {
                        let _ = if site_path.is_dir() {
                            // allow:unlink the generation being materialized, which nothing serves before promote
                            crate::build::io_utils::remove_output_dir_all(&site_path)
                        } else {
                            // allow:unlink the generation being materialized, which nothing serves before promote
                            std::fs::remove_file(&site_path)
                        };
                        let target_abs = if target.is_absolute() {
                            target.clone()
                        } else {
                            source_path.parent().map(|p| p.join(&target)).unwrap_or(target.clone())
                        };
                        // Unlike unix, this arm READS the target, so presence
                        // has to be asked of the object being read.
                        if !crate::build::io_utils::output_present(&target_abs)
                            && !target_abs.is_dir()
                        {
                            continue;
                        }
                        let res = if target_abs.is_dir() {
                            // `copy_dir_recursive` reports the pairs it copied;
                            // the arms must agree on a type and nothing here
                            // reads them.
                            crate::build::media::pipeline::copy_dir_recursive(&target_abs, &site_path)
                                .map(|_| ())
                        } else {
                            crate::build::io_utils::copy_output(&target_abs, &site_path)
                                .map_err(|e| e.to_string())
                        };
                        if let Err(e) = res {
                            log::warn!("[ship_phase] Failed to copy symlink target {:?} -> {:?}: {}", target_abs, site_path, e);
                            failures += 1;
                        }
                    }
                }
                Err(e) => {
                    log::warn!("[ship_phase] read_link failed for {:?}: {}", source_path, e);
                    failures += 1;
                }
            }
            continue;
        }

        match transform_for(rel_path) {
            ShipTransform::StripPreviewAttrs => {
                match std::fs::read(&source_path) {
                    Ok(bytes) => {
                        let stripped = apply_transform(ShipTransform::StripPreviewAttrs, &bytes);
                        // `write_output` renames a fresh inode into place, so it
                        // can neither truncate an inode shared with `source_path`
                        // nor materialize a dataless destination.
                        if let Err(e) = crate::build::io_utils::write_output(&site_path, &stripped) {
                            log::warn!("[ship_phase] write failed for {:?}: {}", site_path, e);
                            failures += 1;
                        }
                    }
                    Err(e) => {
                        log::warn!("[ship_phase] read failed for {:?}: {}", source_path, e);
                        failures += 1;
                    }
                }
            }
            ShipTransform::CopyAsIs => {
                // `fs::copy` (COW on APFS via `fclonefileat(2)`,
                // `copy_file_range(2)` on Linux Btrfs/XFS) rather than
                // `fs::hard_link`: hardlinks share an inode, and a cloud
                // provider evicting that inode turns BOTH stage and generation
                // into 0-byte stubs. `copy_output` copies into a temp sibling and
                // renames, so the destination is never opened with `O_TRUNC`
                // and a cloud-evicted `site_path` cannot force materialization.
                if let Err(e) = crate::build::io_utils::copy_output(&source_path, &site_path) {
                    log::warn!("[ship_phase] copy failed for {:?}: {}", source_path, e);
                    failures += 1;
                }
            }
        }
    }

    if failures > 0 {
        return Err(std::io::Error::other(format!(
            "ship_phase: {} file(s) failed to materialize into {:?}",
            failures, site_dir
        )));
    }
    Ok(drifted)
}

// ---------------------------------------------------------------------------
// materialize_and_promote  (seal+persist entry point)
// ---------------------------------------------------------------------------

/// Monotonic source of promotion epochs (see [`next_promotion_epoch`]).
static PROMOTION_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn next_promotion_epoch() -> u64 {
    PROMOTION_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
}

/// What [`materialize_and_promote`] did with the generation it was handed.
///
/// The question every caller downstream is really asking is whether `current`
/// points at this generation, because advertising a manifest whose generation is
/// not the served one is a rollback in a different guise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Promotion {
    /// Frozen on disk, and `current` was repointed at it.
    Promoted,
    /// Frozen on disk, but a newer build had already promoted, so this tail's
    /// swap was refused. Not an error — the newer generation is
    /// the right one.
    Superseded,
    /// Not frozen at all: the sealed attempt has unresolved required inputs,
    /// or the presence pass could not stand behind what it registered
    /// (`WithholdReason::Unverified` / `ImplausibleLoss`).
    ///
    /// Nothing is copied and `current` is untouched, which keeps `current` and
    /// `hashes.json` describing the same, last-complete build. Freezing the
    /// generation without promoting it would leave that pair disagreeing — the
    /// mismatch `tail_owns_shared_state` exists to prevent — and there is
    /// nothing to keep anyway: the rebuild that the missing sources' arrival
    /// triggers renders the site properly from scratch.
    ///
    /// The reason says which input failed — see [`WithholdReason`].
    Withheld(WithholdReason),
}

/// Whether a sealed generation may replace what `current` serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShipVerdict {
    Ship,
    Withhold(WithholdReason),
}

/// Why a generation was not promoted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WithholdReason {
    /// This sealed attempt did not read every required structural input.
    /// The prior complete generation remains the durable baseline while the
    /// focused preview can still show any usable route.
    SourcesDownloading,
    /// The presence pass or a producer met an I/O error that was not a positive
    /// `NotFound` on `entries` outputs. An unreadable output is not a missing
    /// one, so nothing was dropped for them — and a generation that could not
    /// read what it checked is not one to ship. `sample` names up to three,
    /// each with its error.
    Unverified { entries: usize, sample: Vec<String> },
    /// The presence pass dropped more of the manifest than a healthy build ever
    /// loses between registration and seal — see [`PRESENCE_LOSS_FLOOR`].
    ImplausibleLoss { lost: usize, of: usize },
}

/// The presence pass may drop up to `max(PRESENCE_LOSS_FLOOR, entries / 20)`
/// entries before the generation is withheld. Policy, not physics: a healthy
/// pass drops only what vanished between registration and seal, which the
/// field log put at zero, and the incident pass that could read nothing dropped
/// 822 of 822.
pub const PRESENCE_LOSS_FLOOR: usize = 16;

impl ShipVerdict {
    /// The durable verdict after checking output presence and the sealed
    /// attempt's required input evidence.
    pub fn after_presence_pass(
        sealed: &crate::build::manifest::SealedManifest,
        entries: usize,
        lost: usize,
    ) -> Self {
        if !sealed.unverified().is_empty() {
            return ShipVerdict::Withhold(WithholdReason::Unverified {
                entries: sealed.unverified().len(),
                sample: sealed
                    .unverified()
                    .iter()
                    .take(3)
                    .map(|(key, err)| format!("{key} ({err})"))
                    .collect(),
            });
        }
        if lost > PRESENCE_LOSS_FLOOR.max(entries / 20) {
            return ShipVerdict::Withhold(WithholdReason::ImplausibleLoss { lost, of: entries });
        }
        if !sealed.unresolved_inputs().is_empty() {
            return ShipVerdict::Withhold(WithholdReason::SourcesDownloading);
        }
        ShipVerdict::Ship
    }

    /// Whether the served HTML may still be repaired in place: not when the
    /// generation is about to be withheld for reading its own tree badly.
    pub fn repairs_staging(&self) -> bool {
        !matches!(
            self,
            ShipVerdict::Withhold(WithholdReason::Unverified { .. } | WithholdReason::ImplausibleLoss { .. })
        )
    }
}

/// Whether a seal tail with this outcome still owns the folder's **shared**
/// build state — `hashes.json` and the single `staging/` tree, neither of which
/// is generation-scoped.
///
/// `Superseded` is the one outcome that says no. A newer build has already
/// written the manifest describing what `current` serves and swept staging to
/// its own view; an older tail writing over either leaves them disagreeing.
/// Before the promotion guard the two rolled back together — stale, but
/// mutually consistent — so this rule is a direct consequence of the guard.
/// `search_lane::settle_for_publish` is what makes the disagreement reach a
/// user: it fingerprints the page set from `hashes.json` and indexes the bytes
/// under `current`, so a mismatched pair publishes a bundle stamped with a page
/// set it does not cover.
///
/// A *failed* materialize is not superseded: `current` stays put, but staging
/// is still this build's output, so this tail is still the one that must
/// persist its manifest and sweep.
///
/// `Withheld` says no for the mirror-image reason. Nothing newer has run, but
/// `current` still points at an older generation on purpose, and writing this
/// build's `hashes.json` would describe a page set that generation does not
/// contain — the same disagreement, reached by declining rather than by losing a
/// race.
pub fn tail_owns_shared_state(promotion: &Result<Promotion, String>) -> bool {
    !matches!(promotion, Ok(Promotion::Superseded) | Ok(Promotion::Withheld(_)))
}

/// Copy `staging/` → `generations/<gen-id>/` (stripping dev annotations) and
/// atomically swap `.moss/build.nosync/current` → the new generation.
///
/// Caller must ensure `staging/` is fully populated (post-barrier). The
/// generation dir is created inside this function via `create_dir_all`.
///
/// A generation already whole on disk is promoted without a copy — see
/// [`crate::build::store_gc::GenerationWriteLock::holds`].
///
/// `epoch` orders this build against every other build of the same folder; the
/// swap goes through `lifecycle::promote`, which refuses it when a newer build
/// has already promoted.
///
/// `verdict` covers the seal's required inputs and output presence pass
/// ([`ShipVerdict::after_presence_pass`]). A `Withhold` returns
/// [`Promotion::Withheld`] before anything is copied.
pub fn materialize_and_promote(
    sealed: &crate::build::manifest::SealedManifest,
    mp: &crate::moss_paths::MossPaths,
    stage_dir: &std::path::Path,
    epoch: u64,
    render: Option<u64>,
    verdict: ShipVerdict,
) -> Result<Promotion, String> {
    if let ShipVerdict::Withhold(reason) = verdict {
        match &reason {
            WithholdReason::SourcesDownloading => log::info!(
                "[cloud] withholding generation {} — required inputs remain unresolved, \
                 so `current` stays on the last complete one",
                sealed.generation_id()
            ),
            WithholdReason::Unverified { entries, sample } => log::warn!(
                "generation {} withheld: {} of {} entries unverifiable ({})",
                sealed.generation_id(),
                entries,
                sealed.files().len(),
                sample.join(", ")
            ),
            WithholdReason::ImplausibleLoss { lost, of } => log::warn!(
                "generation {} withheld: presence pass lost {} of {} entries (limit {})",
                sealed.generation_id(),
                lost,
                of,
                PRESENCE_LOSS_FLOOR.max(of / 20)
            ),
        }
        if reason != WithholdReason::SourcesDownloading {
            // The tree on screen is the one that could not be read.
            crate::build::lifecycle::withdraw_render(mp, render);
        }
        return Ok(Promotion::Withheld(reason));
    }
    let gen_dir = mp.generation_dir(sealed.generation_id());
    // Held across the copy and the promote, so no process's generation GC
    // removes `gen_dir` under the copy or between the copy and `current`
    // naming it. Finished only on success: a copy that fails or is cut off
    // leaves its lock file behind, and GC removes the directory later.
    let write_lock =
        crate::build::store_gc::GenerationWriteLock::acquire(&mp.generations_dir(), sealed.generation_id())
            .map_err(|e| format!("Failed to lock generation {}: {}", sealed.generation_id(), e))?;
    let mut drifted = 0;
    let copied = !write_lock.holds(&gen_dir, sealed.files());
    if copied {
        // Seeded from the last whole generation by one copy-on-write clone
        // where the platform has one, so only the entries that differ are
        // written; otherwise every entry is copied.
        let base = crate::build::lifecycle::whole_generation(mp).filter(|base| write_lock.seed_from(&gen_dir, base));
        crate::build::io_utils::create_output_dir_all(&gen_dir)
            .and_then(|()| crate::build::store_gc::prune_to_manifest(&gen_dir, sealed.files()))
            .map_err(|e| format!("Failed to prepare generation dir: {}", e))?;
        // Ship-by-OID: read a `staged_oid` entry from its
        // immutable CAS blob instead of the mutable `stage_dir` copy. Depends on
        // the entry's CAS blob surviving a concurrent build's GC across this
        // whole call — see `CacheWriteLease` at this function's own call sites.
        let object_store = crate::build::cache::ObjectStore::for_site(mp);
        drifted = ship_phase(stage_dir, &gen_dir, sealed, Some(&object_store), base.as_deref())
            .map_err(|e| format!("Failed to materialize generation {}: {}", sealed.generation_id(), e))?;
    } else {
        log::info!("generation {} is already on disk — promoting it without a copy", sealed.generation_id());
    }
    sealed.write_preview_originals(&mp.generations_dir())
        .map_err(|e| format!("Failed to write generation original identities: {e}"))?;
    let promoted = crate::build::lifecycle::promote(mp, epoch, render, sealed.generation_id(), copied)
        .map_err(|e| format!("Failed to set current_ptr: {}", e))?;
    // A generation holding bytes its id does not describe keeps its lock file,
    // so the next seal of this id copies it again instead of reusing it.
    if drifted == 0 {
        write_lock.finish();
        crate::build::lifecycle::remember_whole(mp, &gen_dir, sealed);
    }
    Ok(if promoted { Promotion::Promoted } else { Promotion::Superseded })
}

/// Drop unreferenced `.webp` variants from `sealed`, before anything persists
/// or ships this generation. Called from
/// [`crate::build::degrade::repair_staged_html`], the tail of
/// `advertise_sealed`, which is the one seal tail on every path —
/// it writes `hashes.json` and materializes from `stage_dir` right after.
///
/// **It does not touch `stage_dir`.** `ship_phase` copies `sealed.files()` and
/// nothing else, so an entry dropped here cannot reach the generation whatever
/// staging holds; unlinking the staged bytes as well bought nothing but local
/// disk, and bought it out of the directory the preview server is reading at
/// that instant. `build::pipeline`'s pre-render sweep unlinks them at the next
/// build's start, after the server has moved to `current` — and
/// [`reclaim_staging_now`] unlinks them right after this call returns, on the
/// one path that proves there is no next build to wait for.
///
/// Opt-out via `[build].prune_orphaned_images = false`. Default on: the win
/// is upload bytes and seta quota, NOT local disk — the
/// pruned blob stays in `cache/objects`, kept reachable by its transform
/// record for as long as the source image is in the vault, so `cache::gc`
/// will not collect it. See `build::site_config` for why the off switch exists.
///
/// Returns the condemned keys. A converged build must return NONE: heal-then-
/// prune leaves the same bytes on disk either way, so the end state is
/// identical whether the two agree or fight, and only this set distinguishes
/// them.
///
/// `scan` is passed in rather than taken here because
/// [`unregistered_referenced_variants`] reads the same one: the two passes
/// only stay disjoint — this one acts on what the scan did NOT see, that one
/// on what it did — if there is exactly one scan to disagree about. It is
/// also what keeps that pass alive when `prune_orphaned_images` is off.
pub(crate) fn prune_orphaned_webp_before_ship(
    mp: &crate::moss_paths::MossPaths,
    sealed: &mut crate::build::manifest::SealedManifest,
    scan: &crate::build::media::orphan_prune::ReferenceScan,
) -> std::collections::HashSet<String> {
    let project_path = mp.project_root().to_string_lossy().to_string();
    if !crate::build::site_config::get_build_prune_orphaned_images(&project_path).unwrap_or(true) {
        log::info!("orphan prune: disabled via [build].prune_orphaned_images");
        return std::collections::HashSet::new();
    }
    if !scan.unreadable.is_empty() {
        // Fail closed. An unreadable page shrinks the reference set, and a
        // smaller reference set authorizes MORE deletion — so a single
        // eviction or mid-flight write could delete every variant only that
        // page referenced (an earlier incident: 525 files deleted, 207 images 404-ing).
        // Skipping costs this generation some upload bytes; deleting wrongly
        // costs the site. The verdict is deliberately left untouched: a prune
        // that did not run has judged nothing, and writing an empty or partial
        // verdict here would carry this blindness into the next build's
        // producers.
        let sample: Vec<String> = scan
            .unreadable
            .iter()
            .take(3)
            .map(|p| p.display().to_string())
            .collect();
        log::warn!(
            "orphan prune: skipped — {} file(s)/dir(s) under staging could not be read, so the \
             reference set is incomplete and pruning from it could delete live images: {}",
            scan.unreadable.len(),
            sample.join(", ")
        );
        return std::collections::HashSet::new();
    }
    let referenced = &scan.tails;
    let outputs = sealed.image_outputs().clone();
    let removed_keys =
        crate::build::media::orphan_prune::orphaned_webp_keys(&outputs, referenced);
    if !removed_keys.is_empty() {
        log::info!(
            "orphan prune: {} unreferenced .webp variant(s) dropped from the generation; \
             staging keeps the bytes until the next build sweeps it",
            removed_keys.len()
        );
    }
    sealed.remove_entries(&removed_keys);

    // Carry the verdict forward for the next build's producers.
    // `removed_keys` alone is NOT the verdict: a converged build removes
    // nothing — precisely because the producers honored the last answer — so
    // storing only this build's removals empties the set and restarts the
    // churn one build later. A key leaves only when THIS scan sees a reference,
    // and this scan reads the finished staging tree, so it is the one that can
    // see a plugin's or a notebook's. `hashes.json` is still the previous
    // build's here; `write_to_disk` is the caller's next step.
    let previously_pruned = std::fs::read_to_string(mp.hashes())
        .ok()
        .and_then(|c| serde_json::from_str::<crate::types::content::SiteHashes>(&c).ok())
        .map(|h| h.pruned_image_outputs)
        .unwrap_or_default();
    let mut verdict: std::collections::HashSet<String> = previously_pruned
        .into_iter()
        .filter(|k| !referenced.contains(k))
        .collect();
    verdict.extend(removed_keys.iter().cloned());
    sealed.set_pruned_image_outputs(verdict);
    // The keys travel out so `degrade` can strip the `<source>` elements that
    // pointed at them. An unshipped file whose reference survives in HTML is a
    // live 404, and `<picture>` renders it blank rather than falling back.
    removed_keys
}

/// Reclaim `stage_dir` bytes this build orphaned, for the ONE build shape
/// where nothing will ever sweep them: a one-shot invocation (`moss build`,
/// `build_sync`, every snapshot-test fixture) whose caller drops the tokio
/// runtime as soon as the seal tail returns.
///
/// `prune_orphaned_webp_before_ship` and `drop_absent_outputs` only ever drop
/// entries from `sealed` — see their docs — because the preview server may
/// still be reading `stage_dir` while the seal tail runs
/// (see `build::pipeline::sweep_staging`'s doc for the 404 this avoids).
/// `sweep_staging` is how those bytes are normally reclaimed, but it runs at
/// the START of a FUTURE build in the same folder, using the manifest that
/// build inherits. A build that is the last one in its process never gets a
/// future build to do that, so without this call its orphaned `.webp` bytes
/// sit in `stage_dir` forever and ship in anything that reads that tree
/// directly — a raw copy of staging, a snapshot test comparing it
/// byte-for-byte. Measured exactly that shape on a real site.
///
/// `sealed` must be the FINAL manifest — call this after every pass that can
/// drop an entry (`degrade::repair_staged_html`), never before. The permit is
/// `lifecycle::final_build_permit`, minted where the caller proves nothing
/// reads `stage_dir` again.
pub(crate) fn reclaim_staging_now(
    stage_dir: &std::path::Path,
    sealed: &crate::build::manifest::SealedManifest,
    permit: &crate::build::lifecycle::SweepPermit,
) {
    let hashes = sealed.site_hashes_view();
    crate::build::media::pipeline::remove_stale_files(stage_dir, hashes, "staging (final build)", permit);
    crate::build::media::pipeline::remove_stale_dirs(
        stage_dir,
        &crate::build::media::pipeline::compute_expected_dirs(hashes),
        permit,
    );
}

/// Drop manifest entries whose output is not on disk.
///
/// The last owner of "the manifest and the generation agree". Registration
/// happens from receipts, so an entry is normally exactly what this build
/// wrote — but an entry can also be carried from the previous build, and
/// between the two builds a sync client may have evicted the file, or the
/// user may have deleted it. `output_present` is one `symlink_metadata` per
/// entry and no read, so this costs a stat per manifest entry and cannot
/// itself hit the eviction fault it exists to find.
///
/// Read-only against `stage_dir`. It used to unlink the unusable file — a
/// 0-byte stub or a dataless placeholder — so the next build would regenerate
/// rather than trust it. `build::pipeline`'s pre-render sweep does that
/// instead, keyed off the same dropped entry and still before any producer
/// looks, but at a moment when the preview server is no longer reading
/// staging.
///
/// `_moss/math/` is the exception and keeps its entry: those PNGs
/// are append-only and the published site still serves them, so un-promising
/// one deletes it from a live site. A download is requested instead —
/// fire-and-forget, the same pattern the Wait arm and theme assets use — and
/// the local preview 404s that one image until the bytes arrive. [`ship_phase`]
/// skips it by asking the same predicate, so the kept entry never becomes a
/// read of absent bytes.
pub(crate) fn drop_absent_outputs(
    stage_dir: &std::path::Path,
    sealed: &mut crate::build::manifest::SealedManifest,
    object_store: Option<&crate::build::cache::ObjectStore>,
) -> std::collections::HashSet<String> {
    use crate::build::io_utils::Presence;
    use crate::build::served_path::MATH_PNG_PREFIX;
    let mut absent: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut unverified: Vec<(String, String)> = Vec::new();
    for (rel, entry) in sealed.files() {
        let stage_path = stage_dir.join(rel);
        // Same resolution `ship_phase` uses (`resolve_ship_source`): a
        // CAS-backed entry's presence is asked of its CAS blob, never of the
        // mutable stage copy — otherwise this pass can answer "present" from
        // one path while `ship_phase` reads the other, genuinely-absent, one,
        // moving the failure a few lines down instead of preventing it.
        let ShipRead::Path(path) = resolve_ship_source(rel, &stage_path, sealed, object_store) else {
            continue; // held bytes are present by construction
        };
        let (mode, _) = crate::types::content::parse_entry(entry);
        let presence = crate::build::io_utils::probe_output(&path, mode);
        if presence.is_present() {
            continue;
        }
        if rel.starts_with(MATH_PNG_PREFIX) {
            crate::build::cloud_readiness::request_download(&path);
            continue;
        }
        match presence {
            // An error that is not a positive `NotFound` is no answer: the
            // entry stays, and the generation that carries it is withheld.
            Presence::Unverified(err) => unverified.push((rel.clone(), err.to_string())),
            _ => {
                absent.insert(rel.clone());
            }
        }
    }
    for (rel, err) in unverified {
        sealed.mark_unverified(rel, err);
    }
    if !absent.is_empty() {
        log::info!(
            "presence pass: {} manifest entr(ies) had no output and were dropped before ship",
            absent.len()
        );
    }
    sealed.remove_entries(&absent);
    // Returned for the same reason the prune returns its keys: dropping a
    // manifest entry removes the file from the live site at the generation
    // swap, so its `<source>` must go too. `_moss/math/` never reaches this
    // set — the carve-out above keeps those entries, so a math PNG
    // awaiting download is never stripped from a published page.
    absent
}

/// Referenced `.webp` URLs that no manifest entry and no staged file backs —
/// the fourth and last source of a live 404 in shipped HTML.
///
/// The three passes upstream all start from something moss recorded: a failed
/// encode, a manifest key it pruned, a manifest key whose bytes went missing.
/// A variant promised with `set_pending` but never produced and never failed
/// is in none of them — it has no manifest entry at all — yet its `<source>`
/// is in the staged HTML. Manifest membership is the load-bearing test:
/// [`ship_phase`] copies `sealed.files()` and nothing else, so a key with no
/// entry never reaches the generation whatever staging holds.
///
/// Two independent "it is gone" oracles, conjoined, because each covers the
/// other's blind spot. The manifest half misses a file that ships *through*
/// an entry rather than *as* one — `copy_deferred_assets` preserves a
/// passthrough subtree as a single symlink entry, so `sealed.files()` holds
/// `myapp` and not `myapp/logo.webp`. The existence half catches that.
///
/// **The existence half is a bare `symlink_metadata`, deliberately NOT
/// `entry_output_present`.** This pass asks only whether bytes were ever
/// written here at all. Whether bytes that DO exist are evicted, dataless or
/// zero-length is a different question, owned by [`drop_absent_outputs`] and
/// the encoder's `set_failed`, which already ask it with the right carve-outs.
/// Consulting the evicted bit here would strip a healthy variant on a synced
/// vault, which is why this pass needs no eviction carve-out of its own.
///
/// Scope is `.webp` only, which keeps `.png` (including the `_moss/math/`
/// tier the carve-out above covers), OG cards and video keys out. `<video>` must not
/// ride along: video has no `set_failed` path, and an emptied `<video>` falls
/// through to nothing where a `<picture>` falls through to its `<img>`.
///
/// Unlike the prune this must NOT fail closed on an incomplete scan. The
/// prune's arithmetic inverts here: a shrunken reference set authorizes more
/// deletion there, but yields FEWER repairs here, so refusing to act would
/// disable the repair exactly when staging is degraded and it is most needed.
pub(crate) fn unregistered_referenced_variants(
    scan: &crate::build::media::orphan_prune::ReferenceScan,
    sealed: &crate::build::manifest::SealedManifest,
    stage_dir: &std::path::Path,
) -> std::collections::HashSet<String> {
    if !scan.unreadable.is_empty() {
        log::warn!(
            "unregistered-reference pass: {} file(s)/dir(s) under staging could not be read, so \
             some live 404s may go unrepaired this build",
            scan.unreadable.len()
        );
    }
    // `resolved`, not `tails`: the suffix widening that keeps the prune
    // conservative turns every nested reference into a handful of keys that
    // exist nowhere, and here each one reads as a missing variant. They could
    // never strip a live `<source>` — `degrade` matches exact resolved URLs —
    // but on a real site they numbered in the thousands on every build, so the
    // HTML repair re-read every page and the log reported a breakage that was
    // not there.
    let unregistered: std::collections::HashSet<String> = scan
        .resolved
        .iter()
        .filter(|key| key.ends_with(".webp"))
        .filter(|key| !sealed.files().contains_key(key.as_str()))
        .filter(|key| {
            // Positively gone only: an unreadable path strips nothing.
            let path = stage_dir.join(key.as_str());
            std::fs::symlink_metadata(&path)
                .is_err_and(|e| crate::build::icloud::is_definitely_absent(&path, &e))
        })
        .cloned()
        .collect();
    if !unregistered.is_empty() {
        log::info!(
            "unregistered-reference pass: {} referenced .webp URL(s) have neither a manifest \
             entry nor bytes on disk and will be stripped from HTML",
            unregistered.len()
        );
    }
    unregistered
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::manifest::{HashBucket, PendingManifest};
    use crate::build::store_gc::{GenerationWriteLock, WholeGeneration};
    use crate::types::content::SiteHashes;
    use tempfile::tempdir;

    #[test]
    fn apply_strip_removes_data_source_line() {
        let html = r#"<p data-source-line="42">hello</p>"#;
        let stripped = apply_transform(ShipTransform::StripPreviewAttrs, html.as_bytes());
        assert_eq!(std::str::from_utf8(&stripped).unwrap(), r#"<p>hello</p>"#);
    }

    #[test]
    fn apply_strip_removes_data_source_range() {
        let html = r#"<div data-source-range="10-20">x</div>"#;
        let stripped = apply_transform(ShipTransform::StripPreviewAttrs, html.as_bytes());
        assert_eq!(std::str::from_utf8(&stripped).unwrap(), r#"<div>x</div>"#);
    }

    #[test]
    fn apply_strip_removes_data_source_fm_and_none() {
        // data-source-fm has a quoted value; data-source-none is a bare attribute.
        let html = r#"<h1 data-source-fm="title">T</h1><h2 data-source-none>X</h2>"#;
        let stripped = apply_transform(ShipTransform::StripPreviewAttrs, html.as_bytes());
        assert_eq!(
            std::str::from_utf8(&stripped).unwrap(),
            r#"<h1>T</h1><h2>X</h2>"#
        );
    }

    #[test]
    fn apply_strip_removes_data_moss_preview() {
        for attr in [
            "data-moss-preview",
            "data-moss-preview=\"\"",
            "data-moss-preview='yes'",
            "data-moss-preview=true",
            "data-moss-preview = \"true\"",
        ] {
            let html = format!("<body {attr} data-page=\"home\">content</body>");
            let stripped = apply_transform(ShipTransform::StripPreviewAttrs, html.as_bytes());
            assert_eq!(
                std::str::from_utf8(&stripped).unwrap(),
                r#"<body data-page="home">content</body>"#,
                "{attr}"
            );
        }
    }

    #[test]
    fn apply_strip_preserves_preview_attribute_prefixes() {
        let html = r#"<body data-moss-preview-extra="keep" data-moss-preview="x">content</body>"#;
        let stripped = apply_transform(ShipTransform::StripPreviewAttrs, html.as_bytes());
        assert_eq!(
            std::str::from_utf8(&stripped).unwrap(),
            r#"<body data-moss-preview-extra="keep">content</body>"#
        );
    }

    #[test]
    fn apply_strip_preserves_other_attrs() {
        let html = r#"<p class="x" data-source-line="42" id="y">hello</p>"#;
        let stripped = apply_transform(ShipTransform::StripPreviewAttrs, html.as_bytes());
        assert_eq!(
            std::str::from_utf8(&stripped).unwrap(),
            r#"<p class="x" id="y">hello</p>"#
        );
    }

    #[test]
    fn apply_copy_as_is_returns_identical() {
        let bytes = b"<html>no annotations</html>";
        let result = apply_transform(ShipTransform::CopyAsIs, bytes);
        assert_eq!(result, bytes);
    }

    #[test]
    fn apply_strip_passes_through_invalid_utf8() {
        let bytes = &[0xFF, 0xFE, 0xFD][..];
        let result = apply_transform(ShipTransform::StripPreviewAttrs, bytes);
        assert_eq!(result, bytes);
    }

    // -----------------------------------------------------------------------
    // Exhaustive annotation variants
    // -----------------------------------------------------------------------

    #[test]
    fn strip_regex_covers_all_known_source_annotation_variants() {
        // Every known data-source-* variant must be matched by STRIP_SOURCE_LINE.
        // When you add a new annotation variant, add a case here.
        let cases = [
            r#"<p data-source-line="1">text</p>"#,
            r#"<div data-source-range="5-12">grid</div>"#,
            r#"<span data-source-fm="title">My Title</span>"#,
            r#"<span data-source-fm="date">2026-01-01</span>"#,
            r#"<div data-source-fm="cover">cover media</div>"#,
            r#"<div data-source-fm="children">listing</div>"#,
            r#"<div data-source-fm="breadcrumb">trail</div>"#,
            r#"<img data-source-fm="logo" src="logo.svg">"#,
            r#"<nav data-source-none>nav content</nav>"#,
        ];
        for input in &cases {
            let stripped = STRIP_SOURCE_LINE.replace_all(input, "");
            assert!(
                !stripped.contains("data-source-"),
                "STRIP_SOURCE_LINE failed to strip: {input}\nGot: {stripped}"
            );
        }
    }

    #[test]
    fn strip_regex_does_not_strip_non_source_data_attrs() {
        let input = r#"<div data-layout="grid" data-columns="2">ok</div>"#;
        let stripped = STRIP_SOURCE_LINE.replace_all(input, "");
        assert_eq!(stripped, input, "strip should not touch non-source data attrs");
        // `data-moss-preview` is STRIP_PREVIEW_ATTR's job. The two regexes stay
        // separate so each can be read on its own; folding them would make this
        // assertion the only thing standing between a marker and the site.
        let preview = r#"<body data-moss-preview>ok</body>"#;
        assert_eq!(STRIP_SOURCE_LINE.replace_all(preview, ""), preview);
    }

    #[test]
    fn apply_transform_strips_source_annotations() {
        let html = b"<p data-source-line=\"5\">text</p>";
        let result = apply_transform(ShipTransform::StripPreviewAttrs, html);
        assert_eq!(result, b"<p>text</p>".to_vec());
    }

    /// Fail closed, end to end: one unreadable page and the prune condemns
    /// NOTHING, keeps the manifest whole, and records no verdict.
    ///
    /// The scan's token matching errs wide, but its I/O used to err into an
    /// irreversible delete: an unreadable page silently shrank the reference
    /// set, and a smaller reference set authorizes more deletion.
    /// The control arm runs first so "condemned nothing" cannot pass because
    /// the orphan was unprunable to begin with.
    #[cfg(unix)]
    #[test]
    fn prune_condemns_nothing_when_a_staged_page_cannot_be_read() {
        use std::os::unix::fs::PermissionsExt;

        let staged = |vault: &std::path::Path| {
            let mp = crate::moss_paths::MossPaths::new(vault);
            let stage = mp.staging_dir();
            std::fs::create_dir_all(stage.join("assets")).unwrap();
            std::fs::write(stage.join("index.html"), b"<html>no images here</html>").unwrap();
            std::fs::write(stage.join("assets/orphan.webp"), b"fake webp bytes").unwrap();
            let mut pending = PendingManifest::new(SiteHashes::default());
            pending.register(
                &crate::build::served_path::ServedPath::from_source("assets/orphan.webp").unwrap(),
                b"fake webp bytes",
                HashBucket::ImageVariants,
            );
            (mp, stage, pending.seal())
        };

        let control_vault = tempdir().unwrap();
        let (mp, stage, mut sealed) = staged(control_vault.path());
        let scan = crate::build::media::orphan_prune::extract_referenced_tails(&stage);
        let control = prune_orphaned_webp_before_ship(&mp, &mut sealed, &scan);
        assert!(
            control.contains("assets/orphan.webp"),
            "control: this orphan IS prunable"
        );

        let vault = tempdir().unwrap();
        let (mp, stage, mut sealed) = staged(vault.path());
        let page = stage.join("index.html");
        std::fs::set_permissions(&page, std::fs::Permissions::from_mode(0o000)).unwrap();
        assert!(
            std::fs::read(&page).is_err(),
            "mode 0o000 must genuinely block the read — as root it would not, \
             and this test would prove nothing"
        );

        let scan = crate::build::media::orphan_prune::extract_referenced_tails(&stage);
        let removed = prune_orphaned_webp_before_ship(&mp, &mut sealed, &scan);

        assert!(removed.is_empty(), "removed: {removed:?}");
        assert!(
            stage.join("assets/orphan.webp").exists(),
            "a variant must survive a scan that could not read the whole tree"
        );
        assert!(
            sealed.image_outputs().contains("assets/orphan.webp"),
            "and it must stay in the manifest, or ship drops what is still on disk"
        );
        assert!(
            sealed.site_hashes_view().pruned_image_outputs.is_empty(),
            "a prune that did not run has judged nothing — recording a verdict \
             here carries the blindness into the next build's producers"
        );
    }

    /// Seal a manifest naming exactly `entries`, so a test states what the
    /// build registered rather than what it happened to leave on disk.
    fn manifest_of(entries: &[(&str, &[u8])]) -> SealedManifest {
        let mut pending = PendingManifest::new(SiteHashes::default());
        for (rel, bytes) in entries {
            let sp = crate::build::served_path::ServedPath::from_source(rel).unwrap();
            pending.register(&sp, bytes, HashBucket::Files);
        }
        pending.seal()
    }

    /// The property the whole change exists for: the generation is the
    /// manifest, not the directory. A Dropbox conflicted copy sitting in
    /// staging is exactly this orphan — present, readable, and not moss's.
    #[test]
    fn ship_phase_ships_only_what_the_manifest_lists() {
        let stage = tempdir().unwrap();
        let site = tempdir().unwrap();

        std::fs::write(stage.path().join("registered.css"), b"x").unwrap();
        std::fs::write(stage.path().join("orphan.txt"), b"should not ship").unwrap();
        std::fs::write(stage.path().join("page (Conflicted Copy).html"), b"<h1>twin</h1>").unwrap();

        let sealed = manifest_of(&[("registered.css", b"x")]);
        ship_phase(stage.path(), site.path(), &sealed, None, None).unwrap();

        assert!(site.path().join("registered.css").exists());
        assert!(!site.path().join("orphan.txt").exists());
        assert!(
            !site.path().join("page (Conflicted Copy).html").exists(),
            "a file moss did not write must never reach the generation"
        );
    }

    /// A `120000:` entry must round-trip as a link. `copy_output` is
    /// `fs::copy` and follows links, so deciding the arm by extension would
    /// ship a regular file under an entry that says symlink and break
    /// deploy's `MODE_SYMLINK` handling.
    #[cfg(unix)]
    #[test]
    fn ship_phase_round_trips_a_symlink_entry_as_a_link() {
        let stage = tempdir().unwrap();
        let site = tempdir().unwrap();

        std::fs::create_dir_all(stage.path().join("resources/app")).unwrap();
        std::fs::write(stage.path().join("resources/app/index.html"), b"<h1>app</h1>").unwrap();
        std::os::unix::fs::symlink("resources/app", stage.path().join("myapp")).unwrap();

        let mut pending = PendingManifest::new(SiteHashes::default());
        pending.register(
            &crate::build::served_path::ServedPath::from_source("resources/app/index.html").unwrap(),
            b"<h1>app</h1>",
            HashBucket::Files,
        );
        pending.register_hashed(
            &crate::build::served_path::ServedPath::from_source("myapp").unwrap(),
            &crate::types::content::symlink_entry("resources/app"),
            HashBucket::Files,
        );
        let sealed = pending.seal();

        ship_phase(stage.path(), site.path(), &sealed, None, None).unwrap();

        let meta = std::fs::symlink_metadata(site.path().join("myapp")).unwrap();
        assert!(meta.file_type().is_symlink(), "the entry says symlink; the generation must hold one");
        assert_eq!(
            std::fs::read_link(site.path().join("myapp")).unwrap(),
            std::path::Path::new("resources/app")
        );
    }

    #[test]
    fn ship_phase_returns_err_on_partial_copy() {
        // A per-file fault during materialize must make ship_phase report Err, so
        // materialize_and_promote -> mat_ok=false -> deploy keeps last-known-good and
        // `current` is never swapped to a partial generation. (Pre-merge blocker B1.)
        let test_tmp = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
        std::fs::create_dir_all(&test_tmp).expect("create target/test-tmp");
        let stage = tempfile::TempDir::new_in(&test_tmp).unwrap();
        let site = tempfile::TempDir::new_in(&test_tmp).unwrap();

        std::fs::create_dir_all(stage.path().join("sub")).unwrap();
        std::fs::write(stage.path().join("sub/page.html"), b"<html/>").unwrap();
        // Block the destination: site/sub as a FILE makes create_dir_all(site/sub)
        // fail with ENOTDIR, so the page cannot be materialized.
        std::fs::write(site.path().join("sub"), b"blocker").unwrap();

        let sealed = manifest_of(&[("sub/page.html", b"<html/>")]);
        let result = ship_phase(stage.path(), site.path(), &sealed, None, None);
        assert!(
            result.is_err(),
            "ship_phase must return Err when a file fails to materialize"
        );
    }

    /// One absent condition, two fates. Skipping the math exemption is what
    /// keeps an evicted PNG from failing the whole generation;
    /// skipping anything else would promote a generation short of a file its
    /// own manifest names, which only moves the failure to the next publish.
    #[test]
    fn ship_phase_skips_an_absent_math_png_and_fails_on_any_other_absence() {
        let stage = tempdir().unwrap();
        let site = tempdir().unwrap();

        let math = crate::build::served_path::ServedPath::for_math_png("87ba30f2b3c09ca9").unwrap();
        let mut pending = PendingManifest::new(SiteHashes::default());
        pending.register_hashed(&math, &crate::types::content::file_entry("cccc"), HashBucket::Files);
        // Neither file is written: both are absent for the same reason.
        ship_phase(stage.path(), site.path(), &pending.seal(), None, None)
            .expect("an absent math PNG is skipped, not counted");

        let sealed = manifest_of(&[("page/index.html", b"<h1>hi</h1>")]);
        assert!(
            ship_phase(stage.path(), site.path(), &sealed, None, None).is_err(),
            "an ordinary entry that has no output must not be promoted away quietly"
        );
    }

    #[test]
    fn ship_phase_strips_html_only() {
        // HTML gets StripPreviewAttrs; CSS gets CopyAsIs.
        let stage = tempdir().unwrap();
        let site = tempdir().unwrap();

        let preview_html = r#"<body data-moss-preview><p data-source-line="1">hi</p></body>"#;
        let css = b"body{margin:0}";

        std::fs::write(stage.path().join("index.html"), preview_html).unwrap();
        std::fs::write(stage.path().join("style.css"), css).unwrap();

        let sealed = manifest_of(&[
            ("index.html", preview_html.as_bytes()),
            ("style.css", css),
        ]);
        ship_phase(stage.path(), site.path(), &sealed, None, None).unwrap();

        // HTML in site/ must have annotations stripped
        let shipped_html = std::fs::read_to_string(site.path().join("index.html")).unwrap();
        assert_eq!(shipped_html, r#"<body><p>hi</p></body>"#);

        // CSS in site/ must be byte-identical
        assert_eq!(std::fs::read(site.path().join("style.css")).unwrap(), css);

        // Stage files must be UNCHANGED (ship_phase reads stage, writes site)
        let stage_html = std::fs::read_to_string(stage.path().join("index.html")).unwrap();
        assert_eq!(stage_html, preview_html, "ship_phase must not modify stage files");
    }

    #[test]
    fn ship_phase_creates_parent_dirs() {
        // Files in nested subdirectories must have parent dirs created.
        let stage = tempdir().unwrap();
        let site = tempdir().unwrap();

        std::fs::create_dir_all(stage.path().join("articles/foo")).unwrap();
        std::fs::write(stage.path().join("articles/foo/bar.html"), b"<x/>").unwrap();

        let sealed = manifest_of(&[("articles/foo/bar.html", b"<x/>")]);
        ship_phase(stage.path(), site.path(), &sealed, None, None).unwrap();

        assert!(site.path().join("articles/foo/bar.html").exists());
    }

    #[cfg(unix)]
    #[test]
    fn ship_phase_uses_independent_inodes_for_cloud_safety() {
        // CopyAsIs files must NOT hardlink stage→site. A hardlink-based
        // ship made one iCloud eviction zero both copies, leaving permanent
        // 0-byte stubs (the asset-link-recovery bug). Verify that for a
        // CopyAsIs file (e.g. .mp4) the destination is an independent inode.
        use std::os::unix::fs::MetadataExt;

        let stage = tempdir().unwrap();
        let site = tempdir().unwrap();

        // .mp4 is CopyAsIs (not StripPreviewAttrs).
        std::fs::write(stage.path().join("clip.mp4"), b"video bytes").unwrap();

        let sealed = manifest_of(&[("clip.mp4", b"video bytes")]);
        ship_phase(stage.path(), site.path(), &sealed, None, None).unwrap();

        let stage_meta = std::fs::metadata(stage.path().join("clip.mp4")).unwrap();
        let site_meta = std::fs::metadata(site.path().join("clip.mp4")).unwrap();

        assert_ne!(
            stage_meta.ino(),
            site_meta.ino(),
            "site/clip.mp4 must not share an inode with stage/clip.mp4 (iCloud-eviction shared-fate)"
        );
        // Content must still be the same (COW or full copy, both work).
        assert_eq!(
            std::fs::read(site.path().join("clip.mp4")).unwrap(),
            b"video bytes"
        );
    }

    #[cfg(unix)]
    #[test]
    fn ship_phase_recovers_from_hardlinked_zero_byte_pair() {
        // Regression: a previous build hardlinked stage↔site via
        // `fs::hard_link` (the old ship implementation). iCloud later
        // evicted the shared inode, leaving BOTH files at 0 bytes
        // sharing one inode. The next build:
        //   1. emit (via std::fs::write) restores stage to N bytes —
        //      but since site shares the inode, site also goes to N bytes
        //      (still nlink=2).
        //   2. ship_phase runs fs::copy(stage, site). On macOS, fs::copy
        //      opens site with O_WRONLY|O_TRUNC which truncates the
        //      shared inode FIRST, zeroing stage in the process, then
        //      reads 0 bytes from stage and writes 0 bytes to site.
        //      Both end up as 0-byte stubs again — every. single. build.
        //
        // The fix: `io_utils::copy_output` never opens site_path at all — it
        // copies into a temp sibling and rename(2)s it into place, so the
        // shared inode is replaced rather than truncated.
        use std::os::unix::fs::MetadataExt;

        let stage = tempdir().unwrap();
        let site = tempdir().unwrap();

        std::fs::write(stage.path().join("clip.mp4"), b"video bytes").unwrap();
        // allow:hard_link (test scaffold — synthesizes the pre-fix shared-inode state this regression test recovers from)
        std::fs::hard_link(stage.path().join("clip.mp4"), site.path().join("clip.mp4"))
            .unwrap();
        let stage_ino_before = std::fs::metadata(stage.path().join("clip.mp4")).unwrap().ino();
        let site_ino_before = std::fs::metadata(site.path().join("clip.mp4")).unwrap().ino();
        assert_eq!(
            stage_ino_before, site_ino_before,
            "test setup: stage and site must start hardlinked"
        );

        let sealed = manifest_of(&[("clip.mp4", b"video bytes")]);
        ship_phase(stage.path(), site.path(), &sealed, None, None).unwrap();

        // Stage must STILL have the bytes (the bug zeroed it during fs::copy).
        let stage_bytes = std::fs::read(stage.path().join("clip.mp4")).unwrap();
        assert_eq!(
            stage_bytes, b"video bytes",
            "stage/clip.mp4 must retain its bytes after ship_phase (was zeroed by O_TRUNC on shared inode in the pre-fix code)"
        );

        assert_eq!(std::fs::read(site.path().join("clip.mp4")).unwrap(), b"video bytes");

        let stage_meta = std::fs::metadata(stage.path().join("clip.mp4")).unwrap();
        let site_meta = std::fs::metadata(site.path().join("clip.mp4")).unwrap();
        assert_ne!(
            stage_meta.ino(),
            site_meta.ino(),
            "stage and site must end with independent inodes"
        );
    }

    #[test]
    fn ship_phase_overwrites_zero_byte_stub_at_site() {
        // Regression: after iCloud eviction of a hardlinked stage↔site pair,
        // the next build's stage repair (via ObjectStore::link_to) recreates
        // stage with the real bytes, but site/ retains the 0-byte stub.
        // ship_phase MUST overwrite that stub with the stage bytes.
        let stage = tempdir().unwrap();
        let site = tempdir().unwrap();

        std::fs::write(stage.path().join("clip.mp4"), b"recovered bytes").unwrap();
        std::fs::write(site.path().join("clip.mp4"), b"").unwrap();

        let sealed = manifest_of(&[("clip.mp4", b"recovered bytes")]);
        ship_phase(stage.path(), site.path(), &sealed, None, None).unwrap();

        assert_eq!(
            std::fs::read(site.path().join("clip.mp4")).unwrap(),
            b"recovered bytes",
            "site/clip.mp4 must be overwritten with stage bytes, not left as 0-byte stub"
        );
    }

    #[test]
    fn apply_strip_keeps_deploy_only_script_and_its_attribute() {
        // The deployed artifact must keep running the beacon/analytics
        // script AND the attribute the preview server keys its strip on —
        // ship must not repeat the old markers' mistake of leaving that
        // script unmarked for a later, generation-blind server-side strip.
        let html = r#"<body data-moss-preview><script src="a.js" data-moss-deploy-only></script>x</body>"#;
        let out = std::str::from_utf8(
            &apply_transform(ShipTransform::StripPreviewAttrs, html.as_bytes())
        ).unwrap().to_string();
        assert_eq!(out, r#"<body><script src="a.js" data-moss-deploy-only></script>x</body>"#);
    }

    // ─── Promotion ordering ──────────────────────────────────────

    fn promo_paths(tmp: &tempfile::TempDir, gens: &[&str]) -> crate::moss_paths::MossPaths {
        let mp = crate::moss_paths::MossPaths::new(tmp.path());
        for gen in gens {
            std::fs::create_dir_all(mp.generation_dir(gen)).unwrap();
        }
        mp
    }

    /// Which generation `current` points at — via the marker `set_current_ptr`
    /// writes, the same source `site_dir_for_plugin` reads, rather than a
    /// `which.txt` planted in each generation dir just for this test.
    fn served_generation(mp: &crate::moss_paths::MossPaths) -> String {
        mp.current_generation_id().unwrap()
    }

    /// `hashes.json` and `staging/` are shared across a folder's builds, unlike
    /// a generation directory. A superseded tail persisting its own manifest
    /// left the file describing generation N while `current` served N+1 — and
    /// `settle_for_publish` reads exactly that pair, so it would fingerprint
    /// N's page set and index N+1's bytes, publishing a receipt that lies about
    /// which pages its bundle covers. The sweep half is worse: N's view would
    /// delete N+1's new files out of the shared staging tree.
    #[test]
    fn a_superseded_tail_owns_no_shared_state() {
        assert!(!tail_owns_shared_state(&Ok(Promotion::Superseded)));
        assert!(tail_owns_shared_state(&Ok(Promotion::Promoted)));
        assert!(
            tail_owns_shared_state(&Err("materialize failed".to_string())),
            "a failed materialize still leaves staging as THIS build's to persist and sweep"
        );
        assert!(
            !tail_owns_shared_state(&Ok(Promotion::Withheld(WithholdReason::SourcesDownloading))),
            "a withheld tail leaves `current` on an older generation on purpose, so writing \
             its manifest would produce the same lying pair by a different route"
        );
    }

    /// The publish gate, at the one place it can still be reached. An
    /// unpublishable build must not copy a generation into `generations/` at
    /// all — not freeze-then-refuse-to-promote, which would leave a generation
    /// on disk that nothing points at and that GC would have to reason about.
    #[test]
    fn an_unpublishable_build_is_withheld_before_anything_is_copied() {
        let tmp = tempdir().unwrap();
        let mp = crate::moss_paths::MossPaths::new(tmp.path());
        let stage = mp.staging_dir();
        std::fs::create_dir_all(&stage).unwrap();
        std::fs::write(stage.join("index.html"), b"<html></html>").unwrap();

        let mut pending = PendingManifest::new(SiteHashes::default());
        pending.register(
            &crate::build::served_path::ServedPath::from_source("index.html").unwrap(),
            b"<html></html>",
            HashBucket::Files,
        );
        let sealed = pending.seal();
        let outcome = materialize_and_promote(
            &sealed,
            &mp,
            &stage,
            next_promotion_epoch(),
            None,
            ShipVerdict::Withhold(WithholdReason::SourcesDownloading),
        );

        assert_eq!(outcome.unwrap(), Promotion::Withheld(WithholdReason::SourcesDownloading));
        assert!(
            !mp.generation_dir(sealed.generation_id()).exists(),
            "nothing was frozen"
        );
        assert!(!mp.current_ptr().exists(), "and `current` was never created");
    }

    /// The presence pass over a manifest of `names`, with only `on_disk` of them
    /// staged, then the promotion the seal tail would attempt with its verdict.
    fn presence_then_promote(
        tmp: &tempfile::TempDir,
        names: &[String],
        on_disk: &[String],
    ) -> (crate::moss_paths::MossPaths, SealedManifest, Promotion) {
        let mp = promo_paths(tmp, &["g1"]);
        mp.set_current_ptr("g1").unwrap();
        let stage = mp.staging_dir();
        std::fs::create_dir_all(stage.join("assets")).unwrap();
        let mut pending = PendingManifest::new(SiteHashes::default());
        for name in names {
            let bytes = format!("bytes of {name}");
            if on_disk.contains(name) {
                std::fs::write(stage.join(name), &bytes).unwrap();
            }
            pending.register(
                &crate::build::served_path::ServedPath::from_source(name).unwrap(),
                bytes.as_bytes(),
                HashBucket::Files,
            );
        }
        let mut sealed = pending.seal();
        let verdict = crate::build::degrade::repair_staged_html(
            &mp,
            &stage,
            &mut sealed,
            std::collections::HashSet::new(),
        );
        let promotion =
            materialize_and_promote(&sealed, &mp, &stage, next_promotion_epoch(), None, verdict)
                .unwrap();
        (mp, sealed, promotion)
    }

    /// A presence pass that finds nothing it was told exists has not found a
    /// site with no files; it has failed to look. 404c promoted exactly that —
    /// 822 of 822 entries dropped and an empty generation served.
    #[test]
    fn a_presence_pass_that_loses_the_whole_manifest_withholds_the_generation() {
        let tmp = tempdir().unwrap();
        let names: Vec<String> = (0..20).map(|i| format!("assets/f{i}.css")).collect();

        let (mp, _, promotion) = presence_then_promote(&tmp, &names, &[]);

        assert_eq!(
            promotion,
            Promotion::Withheld(WithholdReason::ImplausibleLoss { lost: 20, of: 20 })
        );
        assert_eq!(served_generation(&mp), "g1", "`current` stays on the last good generation");
    }

    /// The other side of the line: what vanishes between registration and seal
    /// in a healthy build is dropped as always, and the generation ships.
    #[test]
    fn a_presence_pass_that_loses_one_entry_still_promotes() {
        let tmp = tempdir().unwrap();
        let names: Vec<String> = (0..40).map(|i| format!("assets/f{i}.css")).collect();

        let (mp, sealed, promotion) = presence_then_promote(&tmp, &names, &names[1..]);

        assert_eq!(promotion, Promotion::Promoted);
        assert_eq!(sealed.files().len(), 39);
        assert_eq!(served_generation(&mp), sealed.generation_id());
    }

    /// A marker that outlived its pointer — `retire_legacy_roots` removes a
    /// legacy `current` and leaves the marker — must not let a re-seal of the
    /// same generation skip the repoint and leave the site unserved.
    #[test]
    fn resealing_the_marked_generation_repoints_a_missing_current() {
        let tmp = tempdir().unwrap();
        let mp = crate::moss_paths::MossPaths::new(tmp.path());
        let stage = mp.staging_dir();
        std::fs::create_dir_all(&stage).unwrap();
        std::fs::write(stage.join("index.html"), b"<h1>home</h1>").unwrap();
        let sealed = manifest_of(&[("index.html", b"<h1>home</h1>")]);
        let promote = || {
            materialize_and_promote(&sealed, &mp, &stage, next_promotion_epoch(), None, ShipVerdict::Ship).unwrap()
        };
        assert_eq!(promote(), Promotion::Promoted);

        let current = mp.current_ptr();
        std::fs::remove_file(&current).or_else(|_| std::fs::remove_dir_all(&current)).unwrap();
        assert_eq!(served_generation(&mp), sealed.generation_id(), "sanity: the marker survives");
        assert_eq!(promote(), Promotion::Promoted);

        assert!(current.join("index.html").is_file(), "`current` must serve the generation again");
    }

    /// A generation that shipped bytes other than its manifest's — a concurrent
    /// build rewrote a stage file after the seal — does not hold what its id
    /// describes, so the next seal of that id must copy it again, not reuse it.
    #[test]
    fn a_generation_that_shipped_drifted_bytes_is_copied_again() {
        let tmp = tempdir().unwrap();
        let mp = crate::moss_paths::MossPaths::new(tmp.path());
        let stage = mp.staging_dir();
        std::fs::create_dir_all(&stage).unwrap();
        std::fs::write(stage.join("page.html"), b"<h1>original</h1>").unwrap();
        let mut sealed = manifest_of(&[("page.html", b"<h1>original</h1>")]);
        sealed.stamp_all_ship_fingerprints(&stage);
        let shipped = mp.generation_dir(sealed.generation_id()).join("page.html");
        let promote = |sealed: &SealedManifest| {
            materialize_and_promote(sealed, &mp, &stage, next_promotion_epoch(), None, ShipVerdict::Ship).unwrap()
        };

        std::fs::write(stage.join("page.html"), b"<h1>RACED</h1>").unwrap();
        assert_eq!(promote(&sealed), Promotion::Promoted);
        assert_eq!(std::fs::read(&shipped).unwrap(), b"<h1>RACED</h1>", "sanity: the drifted bytes shipped");

        std::fs::write(stage.join("page.html"), b"<h1>original</h1>").unwrap();
        sealed.stamp_all_ship_fingerprints(&stage);
        #[cfg(unix)]
        let pointer = || std::os::unix::fs::MetadataExt::ino(&std::fs::symlink_metadata(mp.current_ptr()).unwrap());
        #[cfg(unix)]
        let pointer_before = pointer();
        assert_eq!(promote(&sealed), Promotion::Promoted);
        assert_eq!(std::fs::read(&shipped).unwrap(), b"<h1>original</h1>", "the generation must be copied again");
        // Windows `current` is a copy of the generation, so the fixed bytes
        // reach it only if the re-copy repoints `current` even though the
        // marker already names this generation.
        assert_eq!(std::fs::read(mp.current_ptr().join("page.html")).unwrap(), b"<h1>original</h1>");
        #[cfg(unix)]
        assert_ne!(pointer(), pointer_before, "a re-copied generation must be repointed");
    }

    // ─── Seeding a generation from the last whole one ────────────

    /// A generation, and the edit after it: a changed page, an unchanged
    /// stylesheet, a removed file and a removed directory, and a file that
    /// becomes a directory and a directory that becomes a file.
    const SEED_BASE: &[(&str, &[u8])] = &[
        ("index.html", b"<h1>home v1</h1>"),
        ("kept.css", b"body{}"),
        ("gone.txt", b"bye"),
        ("old/page.html", b"<p>old</p>"),
        ("flip", b"a file, then a directory"),
        ("dir2/x.txt", b"a directory, then a file"),
    ];
    const SEED_NEXT: &[(&str, &[u8])] = &[
        ("index.html", b"<h1>home v2</h1>"),
        ("kept.css", b"body{}"),
        ("new/added.html", b"<p>new</p>"),
        ("flip/index.html", b"<p>now a directory</p>"),
        ("dir2", b"now a file"),
    ];

    /// Stage `entries` and seal a manifest naming exactly them.
    fn stage_and_seal(mp: &crate::moss_paths::MossPaths, entries: &[(&str, &[u8])]) -> SealedManifest {
        for (rel, bytes) in entries {
            let path = mp.staging_dir().join(rel);
            if path.is_dir() {
                std::fs::remove_dir_all(&path).unwrap();
            }
            if path.parent().unwrap().is_file() {
                std::fs::remove_file(path.parent().unwrap()).unwrap();
            }
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        manifest_of(entries)
    }

    fn promote_sealed(mp: &crate::moss_paths::MossPaths, sealed: &SealedManifest) {
        let promotion =
            materialize_and_promote(sealed, mp, &mp.staging_dir(), next_promotion_epoch(), None, ShipVerdict::Ship);
        assert_eq!(promotion, Ok(Promotion::Promoted));
    }

    /// Every path under `root`, directories included, with each file's bytes.
    fn tree(root: &Path) -> std::collections::BTreeMap<String, Option<Vec<u8>>> {
        let mut out = std::collections::BTreeMap::new();
        let mut dirs = vec![root.to_path_buf()];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                let rel = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
                if path.is_dir() {
                    dirs.push(path);
                    out.insert(rel, None);
                } else {
                    out.insert(rel, Some(std::fs::read(&path).unwrap()));
                }
            }
        }
        out
    }

    /// The tree a full copy of `sealed` produces from the current staging.
    fn full_copy(mp: &crate::moss_paths::MossPaths, sealed: &SealedManifest) -> std::collections::BTreeMap<String, Option<Vec<u8>>> {
        let reference = tempdir().unwrap();
        ship_phase(&mp.staging_dir(), reference.path(), sealed, None, None).unwrap();
        tree(reference.path())
    }

    fn lock_file(mp: &crate::moss_paths::MossPaths, gen_id: &str) -> PathBuf {
        mp.generations_dir().join(format!(".{gen_id}.writing"))
    }

    /// Seeding writes only what differs, and the tree it leaves is the one a
    /// full copy would: what the edit removed is gone, including directories
    /// it emptied and a path that changed between file and directory.
    #[test]
    fn a_generation_seeded_from_the_last_whole_one_is_the_tree_a_full_copy_makes() {
        let tmp = tempdir().unwrap();
        let mp = crate::moss_paths::MossPaths::new(tmp.path());
        // Held so a parallel test's folder lookup cannot evict this folder's record.
        let _record = crate::build::lifecycle::lock_for(&mp);
        let base = stage_and_seal(&mp, SEED_BASE);
        promote_sealed(&mp, &base);
        // A seeded copy keeps the base's file, mtime and all; a full copy
        // writes it anew from staging, whose copy is marked with another mtime.
        let marked = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
        let staged = mp.staging_dir().join("kept.css");
        std::fs::File::options().write(true).open(&staged).unwrap().set_modified(marked).unwrap();
        let base_mtime = |p: &Path| std::fs::metadata(p.join("kept.css")).unwrap().modified().unwrap();
        let in_base = base_mtime(&mp.generation_dir(base.generation_id()));

        let next = stage_and_seal(&mp, SEED_NEXT);
        promote_sealed(&mp, &next);

        let gen_dir = mp.generation_dir(next.generation_id());
        assert_eq!(tree(&gen_dir), full_copy(&mp, &next));
        assert!(!lock_file(&mp, next.generation_id()).exists(), "the seeded copy finished");
        let seeded = base_mtime(&gen_dir) == in_base;
        assert_eq!(seeded, crate::build::io_utils::CLONES_DIRS, "seeded exactly where a directory can be cloned");
    }

    /// A copy cut off right after its seed leaves the base's outputs under the
    /// new id. Its lock file stays, so `current` never moves to it and the
    /// next seal of the id copies it again, to the exact tree.
    #[test]
    fn a_copy_cut_off_after_its_seed_is_never_promoted_and_is_copied_again() {
        let tmp = tempdir().unwrap();
        let mp = crate::moss_paths::MossPaths::new(tmp.path());
        let base = stage_and_seal(&mp, SEED_BASE);
        promote_sealed(&mp, &base);
        let next = stage_and_seal(&mp, SEED_NEXT);
        let gen_dir = mp.generation_dir(next.generation_id());

        let lock = crate::build::store_gc::GenerationWriteLock::acquire(&mp.generations_dir(), next.generation_id())
            .unwrap();
        assert_eq!(whole(&mp, &base).is_some_and(|whole| lock.seed_from(&gen_dir, &whole)), crate::build::io_utils::CLONES_DIRS);
        drop(lock); // the process exits mid-copy
        assert!(lock_file(&mp, next.generation_id()).exists());
        assert_eq!(served_generation(&mp), base.generation_id());

        promote_sealed(&mp, &next);
        assert_eq!(tree(&gen_dir), full_copy(&mp, &next));
        assert!(!lock_file(&mp, next.generation_id()).exists());
    }

    /// Seed `generation`, on disk, as it would be recorded for the next copy.
    fn whole(mp: &crate::moss_paths::MossPaths, sealed: &SealedManifest) -> Option<WholeGeneration> {
        WholeGeneration::record(&mp.generation_dir(sealed.generation_id()), sealed.generation_id(), sealed.files())
    }

    /// A file edited inside the seed after it finished — by a person, or by
    /// another process — is carried by the clone, and must be written again.
    #[test]
    fn a_file_edited_in_the_seed_is_shipped_again() {
        let tmp = tempdir().unwrap();
        let mp = crate::moss_paths::MossPaths::new(tmp.path());
        let _record = crate::build::lifecycle::lock_for(&mp);
        let base = stage_and_seal(&mp, SEED_BASE);
        promote_sealed(&mp, &base);
        std::fs::write(mp.generation_dir(base.generation_id()).join("kept.css"), b"BODY{}").unwrap();

        let next = stage_and_seal(&mp, SEED_NEXT);
        promote_sealed(&mp, &next);
        assert_eq!(tree(&mp.generation_dir(next.generation_id())), full_copy(&mp, &next));
    }

    /// A seed another process removed and copied again under the same id is
    /// not the one recorded, even where a file's size and mtime still match.
    #[test]
    fn a_seed_copied_again_under_its_id_is_not_seeded_from() {
        let tmp = tempdir().unwrap();
        let mp = crate::moss_paths::MossPaths::new(tmp.path());
        let _record = crate::build::lifecycle::lock_for(&mp);
        let base = stage_and_seal(&mp, SEED_BASE);
        promote_sealed(&mp, &base);
        let base_dir = mp.generation_dir(base.generation_id());
        let moved = tmp.path().join("moved");
        std::fs::rename(&base_dir, &moved).unwrap();
        for (rel, _) in SEED_BASE {
            let (from, to) = (moved.join(rel), base_dir.join(rel));
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::copy(&from, &to).unwrap();
            let mtime = std::fs::metadata(&from).unwrap().modified().unwrap();
            if *rel == "kept.css" {
                std::fs::write(&to, b"BODY{}").unwrap();
            }
            std::fs::File::options().write(true).open(&to).unwrap().set_modified(mtime).unwrap();
        }

        let next = stage_and_seal(&mp, SEED_NEXT);
        promote_sealed(&mp, &next);
        assert_eq!(tree(&mp.generation_dir(next.generation_id())), full_copy(&mp, &next));
    }

    /// A seed file the cloud provider has evicted keeps its size and mtime,
    /// but its bytes are not there to clone: it is not held.
    #[test]
    fn an_evicted_seed_file_is_not_held() {
        let tmp = tempdir().unwrap();
        let mp = crate::moss_paths::MossPaths::new(tmp.path());
        let base = stage_and_seal(&mp, SEED_BASE);
        promote_sealed(&mp, &base);
        let Some(whole) = whole(&mp, &base) else { return };
        let kept = mp.generation_dir(base.generation_id()).join("kept.css");
        let entry = &base.files()["kept.css"];
        assert!(whole.still_holds("kept.css", entry, &kept), "sanity: held while on disk");
        let _cloud = crate::build::icloud::pretend::evicted(&kept);
        assert!(!whole.still_holds("kept.css", entry, &kept));
    }

    /// A seed whose lock another writer holds is not waited for.
    #[test]
    fn a_seed_whose_lock_is_held_is_not_seeded_from() {
        let tmp = tempdir().unwrap();
        let mp = crate::moss_paths::MossPaths::new(tmp.path());
        let base = stage_and_seal(&mp, SEED_BASE);
        promote_sealed(&mp, &base);
        let next = stage_and_seal(&mp, SEED_NEXT);
        let Some(whole) = whole(&mp, &base) else { return };
        let _held = GenerationWriteLock::acquire(&mp.generations_dir(), base.generation_id()).unwrap();
        let lock = GenerationWriteLock::acquire(&mp.generations_dir(), next.generation_id()).unwrap();
        assert!(!lock.seed_from(&mp.generation_dir(next.generation_id()), &whole));
        assert!(!mp.generation_dir(next.generation_id()).exists());
    }

    /// A math PNG the seed lacks — its bytes were evicted when the seed
    /// shipped — is shipped once they are back, though its entry is unchanged.
    #[test]
    fn a_seeded_copy_ships_an_unchanged_entry_its_seed_lacks() {
        let tmp = tempdir().unwrap();
        let mp = crate::moss_paths::MossPaths::new(tmp.path());
        let _record = crate::build::lifecycle::lock_for(&mp);
        let math = crate::build::served_path::ServedPath::for_math_png("87ba30f2b3c09ca9").unwrap();
        let with_math = |page: &[u8]| {
            let mut pending = PendingManifest::new(SiteHashes::default());
            let sp = crate::build::served_path::ServedPath::from_source("index.html").unwrap();
            pending.register(&sp, page, HashBucket::Files);
            pending.register_hashed(&math, &crate::types::content::file_entry("cccc"), HashBucket::Files);
            std::fs::write(mp.staging_dir().join("index.html"), page).unwrap();
            pending.seal()
        };
        std::fs::create_dir_all(mp.staging_dir()).unwrap();
        let base = with_math(b"<h1>v1</h1>");
        promote_sealed(&mp, &base);
        let shipped = |sealed: &SealedManifest| mp.generation_dir(sealed.generation_id()).join(math.as_str());
        assert!(!shipped(&base).exists(), "sanity: the evicted PNG was skipped");

        let staged = mp.staging_dir().join(math.as_str());
        std::fs::create_dir_all(staged.parent().unwrap()).unwrap();
        std::fs::write(&staged, b"png").unwrap();
        let next = with_math(b"<h1>v2</h1>");
        promote_sealed(&mp, &next);
        assert_eq!(std::fs::read(shipped(&next)).unwrap(), b"png");
    }

    /// The last whole generation is no seed once a copy of it was cut off or
    /// drifted (its lock file is back) or once it is gone: every entry is
    /// copied, and none of its bytes reach the new generation.
    #[test]
    fn a_copy_without_a_usable_seed_copies_every_entry() {
        let tmp = tempdir().unwrap();
        let mp = crate::moss_paths::MossPaths::new(tmp.path());
        let _record = crate::build::lifecycle::lock_for(&mp);
        let base = stage_and_seal(&mp, SEED_BASE);
        promote_sealed(&mp, &base);
        // Same size and mtime, so only the lock file tells.
        let kept = mp.generation_dir(base.generation_id()).join("kept.css");
        let mtime = std::fs::metadata(&kept).unwrap().modified().unwrap();
        std::fs::write(&kept, b"BODY{}").unwrap();
        std::fs::File::options().write(true).open(&kept).unwrap().set_modified(mtime).unwrap();
        std::fs::write(lock_file(&mp, base.generation_id()), b"").unwrap();

        let next = stage_and_seal(&mp, SEED_NEXT);
        promote_sealed(&mp, &next);
        assert_eq!(tree(&mp.generation_dir(next.generation_id())), full_copy(&mp, &next));

        std::fs::remove_dir_all(mp.generation_dir(next.generation_id())).unwrap();
        let mut last = SEED_NEXT.to_vec();
        last.push(("later.html", b"<p>later</p>"));
        let last = stage_and_seal(&mp, &last);
        promote_sealed(&mp, &last);
        assert_eq!(tree(&mp.generation_dir(last.generation_id())), full_copy(&mp, &last));
    }

    /// One row per way a referenced `.webp` can fail to be gone, over a single
    /// hand-built scan — the predicate is three conjuncts and each keeps
    /// something real alive.
    #[cfg(unix)]
    #[test]
    fn unregistered_referenced_variants_keeps_everything_that_is_not_gone() {
        let tmp = tempdir().unwrap();
        let stage = tmp.path();
        std::fs::create_dir_all(stage.join("assets")).unwrap();

        // A passthrough subtree ships as ONE symlink entry (`myapp`), so
        // `sealed.files()` never holds `myapp/logo.webp` even though the file
        // ships. The existence half is the only thing covering that.
        let real = tmp.path().join("outside");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("logo.webp"), b"logo").unwrap();
        std::os::unix::fs::symlink(&real, stage.join("myapp")).unwrap();
        // Unregistered and 0-byte: `entry_output_present` calls this absent,
        // `symlink_metadata` calls it present, and this pass asks only whether
        // bytes were ever written at all — whether existing bytes are usable
        // belongs to `drop_absent_outputs` and the encoder's `set_failed`,
        // which ask it with the eviction carve-outs this pass must not repeat.
        // Swapping the predicate back turns this row red.
        std::fs::write(stage.join("assets/stub.webp"), b"").unwrap();

        let mut pending = PendingManifest::new(SiteHashes::default());
        pending.register(
            &crate::build::served_path::ServedPath::from_source("assets/real.webp").unwrap(),
            b"real",
            HashBucket::ImageVariants,
        );
        let sealed = pending.seal();

        let scan = crate::build::media::orphan_prune::ReferenceScan {
            tails: std::collections::HashSet::new(),
            resolved: [
                "assets/gone.webp",
                "myapp/logo.webp",
                "assets/stub.webp",
                "assets/real.webp",
                "_moss/math/eq1.png",
                "videos/talk.mp4",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            unreadable: vec![],
        };

        let strip = unregistered_referenced_variants(&scan, &sealed, stage);

        assert!(strip.contains("assets/gone.webp"), "{strip:?}");
        assert!(
            !strip.contains("myapp/logo.webp"),
            "a file inside a passthrough subtree ships through the subtree's own \
             symlink entry, so the manifest half alone would strip a live URL: {strip:?}"
        );
        assert!(
            !strip.contains("assets/stub.webp"),
            "bytes exist here, so this pass has no verdict — swapping the bare \
             `symlink_metadata` for `entry_output_present` strips it, and on a \
             synced vault would strip healthy evicted variants the same way: {strip:?}"
        );
        assert!(!strip.contains("assets/real.webp"), "{strip:?}");
        assert!(
            !strip.contains("_moss/math/eq1.png"),
            "math PNGs are append-only and served from the live site; the \
             `.webp` scope is what keeps them (and OG cards) out: {strip:?}"
        );
        assert!(
            !strip.contains("videos/talk.mp4"),
            "video has no `set_failed` path and an emptied <video> falls through to \
             nothing — widening this scope must be a visible edit: {strip:?}"
        );
    }

    /// A page below the site root that references its own, present,
    /// registered variant: nothing is missing, so nothing may be reported.
    /// The prune's suffix widening turns that one reference into
    /// `b/assets/x.webp`, `assets/x.webp` and `x.webp`, none of which exist,
    /// and reading `tails` here reported each as a variant to strip — on
    /// every build, for every nested reference on the site.
    #[test]
    fn a_nested_page_with_every_variant_present_has_nothing_to_strip() {
        let tmp = tempdir().unwrap();
        let stage = tmp.path();
        std::fs::create_dir_all(stage.join("a/b/assets")).unwrap();
        std::fs::write(
            stage.join("a/b/index.html"),
            r#"<picture><source srcset="/a/b/assets/x.webp" type="image/webp"><img src="assets/x.png"></picture>"#,
        )
        .unwrap();
        std::fs::write(stage.join("a/b/assets/x.webp"), b"x").unwrap();
        let mut pending = PendingManifest::new(SiteHashes::default());
        pending.register(
            &crate::build::served_path::ServedPath::from_source("a/b/assets/x.webp").unwrap(),
            b"x",
            HashBucket::ImageVariants,
        );
        let sealed = pending.seal();

        let scan = crate::build::media::orphan_prune::extract_referenced_tails(stage);
        let strip = unregistered_referenced_variants(&scan, &sealed, stage);

        assert!(strip.is_empty(), "{strip:?}");
    }

    /// The same, for a `../` reference: the prefix the token pattern cannot
    /// start on has to be taken back, or `../assets/x.webp` on a page in
    /// `a/b/` resolves beside the page, to a key that does not exist.
    #[test]
    fn a_parent_relative_reference_to_a_present_variant_has_nothing_to_strip() {
        let tmp = tempdir().unwrap();
        let stage = tmp.path();
        std::fs::create_dir_all(stage.join("a/b")).unwrap();
        std::fs::create_dir_all(stage.join("a/assets")).unwrap();
        std::fs::write(
            stage.join("a/b/index.html"),
            r#"<picture><source srcset="../assets/x.webp" type="image/webp"><img src="../assets/x.png"></picture>"#,
        )
        .unwrap();
        std::fs::write(stage.join("a/assets/x.webp"), b"x").unwrap();
        let mut pending = PendingManifest::new(SiteHashes::default());
        pending.register(
            &crate::build::served_path::ServedPath::from_source("a/assets/x.webp").unwrap(),
            b"x",
            HashBucket::ImageVariants,
        );
        let sealed = pending.seal();

        let scan = crate::build::media::orphan_prune::extract_referenced_tails(stage);
        let strip = unregistered_referenced_variants(&scan, &sealed, stage);

        assert!(strip.is_empty(), "{strip:?}");
    }

    // ─── Ship-by-OID ────────────────────────────────────

    /// The property ship-by-OID exists for: between this build sealing a
    /// path's hash and shipping its bytes, a second concurrent build can
    /// rewrite the mutable stage copy. An entry with a live `staged_oid` must
    /// ship the immutable CAS bytes it was sealed against, not whatever the
    /// stage path happens to hold by the time `ship_phase` gets to it.
    #[test]
    fn ship_phase_ships_correct_bytes_from_cas_despite_stage_dir_being_overwritten() {
        let stage = tempdir().unwrap();
        let site = tempdir().unwrap();
        let cache = tempdir().unwrap();

        let object_store = crate::build::cache::ObjectStore::new(cache.path().to_path_buf());
        let oid = object_store.store_bytes(b"X", crate::build::cache::RecordMode::Request).unwrap();

        std::fs::write(stage.path().join("style.css"), b"placeholder").unwrap();

        let mut pending = PendingManifest::new(SiteHashes::default());
        let sp = crate::build::served_path::ServedPath::from_source("style.css").unwrap();
        pending.apply_message(sp.as_str().to_string(), "deadbeefdeadbeef", HashBucket::Files, Some(oid));
        let sealed = pending.seal();

        // A concurrent build rewrites the mutable stage copy after this
        // manifest's hash was sealed against "X".
        std::fs::write(stage.path().join("style.css"), b"Y").unwrap();

        ship_phase(stage.path(), site.path(), &sealed, Some(&object_store), None).unwrap();

        assert_eq!(
            std::fs::read(site.path().join("style.css")).unwrap(),
            b"X",
            "the generation must get the bytes the CAS blob was sealed against, not \
             whatever a concurrent build left in the mutable stage path"
        );
    }

    /// Today a false absence here calls `sealed.remove_entries` and the file
    /// silently vanishes from the promoted generation — a real 404 on the
    /// live site. An entry with a live `staged_oid` must be judged present by
    /// its CAS blob, not by a stage copy this generation was never going to
    /// read from anyway.
    #[test]
    fn drop_absent_outputs_keeps_a_cas_backed_entry_whose_stage_copy_is_transiently_absent() {
        let stage = tempdir().unwrap();
        let cache = tempdir().unwrap();

        let object_store = crate::build::cache::ObjectStore::new(cache.path().to_path_buf());
        let oid = object_store.store_bytes(b"stable bytes", crate::build::cache::RecordMode::Request).unwrap();

        // The stage copy existed once but is transiently gone — an eviction,
        // a mid-write, anything short of moss deciding the file is gone.
        std::fs::write(stage.path().join("asset.bin"), b"placeholder").unwrap();
        std::fs::remove_file(stage.path().join("asset.bin")).unwrap();

        let mut pending = PendingManifest::new(SiteHashes::default());
        let sp = crate::build::served_path::ServedPath::from_source("asset.bin").unwrap();
        pending.apply_message(sp.as_str().to_string(), "cafefacecafeface", HashBucket::Files, Some(oid));
        let mut sealed = pending.seal();

        let dropped = drop_absent_outputs(stage.path(), &mut sealed, Some(&object_store));

        assert!(dropped.is_empty(), "nothing should be dropped: {dropped:?}");
        assert!(
            sealed.files().contains_key("asset.bin"),
            "a CAS-backed entry must survive a transiently-absent stage copy"
        );
    }

    /// The property `Held` exists for: a derived output has no CAS blob, so
    /// before it existed a rewrite of its stage path by a later build reached
    /// this generation under this manifest's frozen hash.
    #[test]
    fn ship_phase_ships_held_bytes_despite_stage_dir_being_overwritten() {
        let stage = tempdir().unwrap();
        let site = tempdir().unwrap();
        let mut pending = PendingManifest::new(SiteHashes::default());
        let sp = crate::build::served_path::ServedPath::from_source("sitemap.xml").unwrap();
        pending.register_held(&sp, b"<urlset>A</urlset>".to_vec(), HashBucket::Files).unwrap();
        let sealed = pending.seal();

        // A concurrent build rewrites the mutable stage copy after this
        // manifest's hash was sealed.
        std::fs::write(stage.path().join("sitemap.xml"), b"<urlset>B</urlset>").unwrap();

        ship_phase(stage.path(), site.path(), &sealed, None, None).unwrap();

        assert_eq!(
            std::fs::read(site.path().join("sitemap.xml")).unwrap(),
            b"<urlset>A</urlset>",
            "the generation must get the bytes this manifest hashed, not the stage's"
        );
    }

    /// A held entry has no stage file to read, so its absence is neither a
    /// dropped entry nor a failed ship.
    #[test]
    fn a_held_entry_ships_and_survives_the_presence_pass_with_no_stage_file() {
        let stage = tempdir().unwrap();
        let site = tempdir().unwrap();
        let mut pending = PendingManifest::new(SiteHashes::default());
        let sp = crate::build::served_path::ServedPath::from_source("llms.txt").unwrap();
        pending.register_held(&sp, b"everything".to_vec(), HashBucket::Files).unwrap();
        let mut sealed = pending.seal();

        let dropped = drop_absent_outputs(stage.path(), &mut sealed, None);
        assert!(dropped.is_empty(), "held bytes are present by construction: {dropped:?}");
        assert!(sealed.files().contains_key("llms.txt"));

        ship_phase(stage.path(), site.path(), &sealed, None, None).unwrap();
        assert_eq!(std::fs::read(site.path().join("llms.txt")).unwrap(), b"everything");
    }

    /// A failed write of held bytes must fail the ship. Otherwise the
    /// generation is promoted one file short of what its own manifest names, and
    /// the gap surfaces at the next publish instead of here. Only that entry
    /// fails: the rest of the generation is still written, as for any per-file
    /// fault (`ship_phase`'s doc). Entries are visited in hash-map order, so
    /// several healthy ones make it likely that some come after the bad one.
    #[test]
    fn a_held_output_that_cannot_be_written_fails_the_ship_and_spares_the_rest() {
        let stage = tempdir().unwrap();
        let site = tempdir().unwrap();
        let healthy = ["sitemap.xml", "rss.xml", "a.txt", "b.txt", "c.txt", "d.txt", "e.txt", "f.txt"];
        let mut pending = PendingManifest::new(SiteHashes::default());
        for rel in healthy.iter().chain(&["llms.txt"]) {
            let sp = crate::build::served_path::ServedPath::from_source(rel).unwrap();
            pending.register_held(&sp, format!("bytes of {rel}").into_bytes(), HashBucket::Files).unwrap();
        }
        let sealed = pending.seal();
        // A directory where `llms.txt` must land: the write cannot replace it.
        std::fs::create_dir(site.path().join("llms.txt")).unwrap();

        let err = ship_phase(stage.path(), site.path(), &sealed, None, None)
            .expect_err("a generation missing a file its manifest names must not ship");

        assert!(err.to_string().contains("1 file(s) failed"), "{err}");
        for rel in healthy {
            assert_eq!(std::fs::read(site.path().join(rel)).unwrap(), format!("bytes of {rel}").into_bytes(), "{rel}");
        }
    }

    /// One test that a genuine post-seal byte change is caught...
    #[test]
    fn ship_phase_integrity_check_catches_a_genuine_post_seal_byte_change() {
        let stage = tempdir().unwrap();
        std::fs::write(stage.path().join("page.html"), b"<h1>original</h1>").unwrap();

        let mut sealed = manifest_of(&[("page.html", b"<h1>original</h1>")]);
        sealed.stamp_all_ship_fingerprints(stage.path());

        // A concurrent build rewrites the path after the fingerprint was
        // taken — the exact race this whole change exists to make audible.
        std::fs::write(stage.path().join("page.html"), b"<h1>RACED</h1>").unwrap();

        let entry = sealed.files().get("page.html").unwrap().clone();
        let detected =
            verify_ship_integrity("page.html", &stage.path().join("page.html"), &entry, &sealed);
        assert!(
            detected.is_some(),
            "a genuine post-seal byte change must be caught, not silently shipped unexamined"
        );
    }

    /// ...and one that isolates `verify_ship_integrity`'s hash-fallback branch
    /// on its own: a stale, un-re-stamped fingerprint must not be reported as
    /// a race once the real hash comparison agrees. (The post-seal repair
    /// path that keeps a fingerprint fresh in practice —
    /// `degrade::apply_to_staging`'s rewrite-then-re-stamp discipline — is a
    /// different scenario, owned by
    /// `ship_phase_reflects_a_post_seal_repair_not_a_stale_cas_entry`
    /// (`media/pipeline_tests.rs`); after c0a7d05 that path can never leave a
    /// stale fingerprint, so it is not reachable via repair and this test
    /// exercises the fallback directly instead.)
    #[test]
    fn verify_ship_integrity_hash_fallback_ignores_a_stale_fingerprint() {
        // A real page carries preview annotations in staging that its shipped
        // copy does not (`apply_transform`/`StripPreviewAttrs`). The manifest
        // hash is always of the SHIPPED (transformed) bytes, never the staged
        // ones verbatim — comparing the current bytes' RAW hash against it
        // would flag every ordinary rewrite of an annotated page as a race.
        // That is the specific wrong turn a previous revision took.
        //
        // The fingerprint is deliberately left STALE (never re-stamped) here:
        // this test is about `verify_ship_integrity`'s own fail-open
        // discipline resolving a stat mismatch correctly on its own, not
        // about `degrade::apply_to_staging`'s separate re-stamp call (which
        // `ship_phase_reflects_a_post_seal_repair_not_a_stale_cas_entry`
        // exercises for real). Re-stamping here would make the stat check
        // return `None` before ever reaching the hash comparison this test
        // means to exercise.
        let stage = tempdir().unwrap();
        let original = r#"<body data-moss-preview><h1>original</h1></body>"#;
        std::fs::write(stage.path().join("page.html"), original).unwrap();

        let shipped_original = apply_transform(transform_for("page.html"), original.as_bytes());
        let hash_original = crate::build::assets::paths::compute_binary_hash(&shipped_original);
        let mut pending = PendingManifest::new(SiteHashes::default());
        let sp = crate::build::served_path::ServedPath::from_source("page.html").unwrap();
        pending.register_hashed(&sp, &hash_original, HashBucket::Files);
        let mut sealed = pending.seal();
        sealed.stamp_all_ship_fingerprints(stage.path());

        // A routine, non-concurrent rewrite — what `degrade::apply_to_staging`
        // does: new (still-annotated) bytes written straight to stage, and
        // the manifest hash updated to match the new SHIPPED bytes. A
        // deliberately different length so the fingerprint's `size` field
        // disagrees regardless of filesystem timestamp resolution.
        let repaired = r#"<body data-moss-preview><h1>this page was repaired</h1></body>"#;
        std::fs::write(stage.path().join("page.html"), repaired).unwrap();
        let shipped_repaired = apply_transform(transform_for("page.html"), repaired.as_bytes());
        let new_hash = crate::build::assets::paths::compute_binary_hash(&shipped_repaired);
        let mut rewrites = std::collections::HashMap::new();
        rewrites.insert("page.html".to_string(), new_hash);
        sealed.apply_post_seal_rewrites(rewrites);

        let entry = sealed.files().get("page.html").unwrap().clone();
        let detected =
            verify_ship_integrity("page.html", &stage.path().join("page.html"), &entry, &sealed);
        assert!(
            detected.is_none(),
            "a routine, annotated-HTML rewrite whose registered hash was kept in sync must \
             not be flagged as a race, even with a stale (un-re-stamped) fingerprint"
        );
    }
}
