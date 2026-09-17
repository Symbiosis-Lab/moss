//! Ship: turn a sealed manifest into a generation directory.
//!
//! See the module-level architecture section in `src-tauri/src/build.rs` for
//! the stage/site model. This file owns everything between "the manifest is
//! sealed" and "`current` points at a new generation":
//!
//! - the two post-seal passes that make the manifest and the disk agree —
//!   `prune_orphaned_webp_before_ship` and `drop_absent_outputs`. Both act on
//!   the MANIFEST only: staging is what the preview server is reading while
//!   this runs, so nothing here unlinks from it (see `build::pipeline`'s
//!   pre-render sweep);
//! - [`ship_phase`], which walks the sealed entries and derives each site/
//!   file from its stage/ file (apply transform, or recreate a symlink);
//! - [`materialize_and_promote`], which runs the above into a fresh
//!   generation dir and repoints `current`, and [`gc_old_generations`].
//!
//! The manifest is the input, not the directory: a file in staging that no
//! entry names — a sync client's conflicted copy, a stale output from an
//! earlier build — is not shipped and cannot reach the published site.

use std::path::Path;
use std::sync::LazyLock;

use crate::build::manifest::SealedManifest;

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
pub const SHIP_TRANSFORM_REV: u32 = 1;

/// Strips preview-only `data-source-*` attributes from HTML.
/// Matches: data-source-line="N", data-source-range="N-M", data-source-fm="field", data-source-none.
static STRIP_SOURCE_LINE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"\s+data-source-(?:line="\d+"|range="\d+-\d+"|fm="[^"]*"|none)"#).unwrap()
});

/// Strips the preview-only `data-moss-preview` attribute from `<body>`.
static STRIP_PREVIEW_ATTR: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"\s+data-moss-preview"#).unwrap()
});

/// Strips `<!--moss:no-preview-->` and `<!--/moss:no-preview-->` comment
/// markers from the shipped artifact. Content between the markers is kept
/// (e.g. the analytics `<script>`) — only the marker comments are removed.
/// The preview server strips both markers AND content via
/// `strip_preview_only_scripts`; the ship path strips only the markers so
/// the deployed artifact fires the analytics script normally.
static STRIP_NO_PREVIEW_MARKER: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"<!--/?moss:no-preview-->").unwrap()
});

// ---------------------------------------------------------------------------
// ShipTransform
// ---------------------------------------------------------------------------

/// How a file should be transformed during the stage→site ship pass.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShipTransform {
    /// Copy the file byte-for-byte. Used for all non-HTML artifacts.
    CopyAsIs,
    /// Strip preview-only source-annotation attributes before writing to site.
    /// Used for `.html` / `.htm` files.
    StripPreviewAttrs,
}

// ---------------------------------------------------------------------------
// Extension-based dispatch
// ---------------------------------------------------------------------------

/// Classify a relative path into the transform that the ship pass should apply.
///
/// It used to take an `annotations_present` flag, for a publish build that
/// emitted no `data-source-*` attributes and could skip the regex. No such
/// build exists: `emit_source_lines` (`pipeline.rs:1251`) is a literal `true`,
/// so every caller passed `true` and the other arm was a no-op waiting to be
/// wrong — it also skipped `STRIP_NO_PREVIEW_MARKER`, which is not
/// annotation-dependent at all.
///
/// Public so tests can verify classification without running a full ship.
pub fn transform_for(rel_path: &str) -> ShipTransform {
    if rel_path.ends_with(".html") || rel_path.ends_with(".htm") {
        ShipTransform::StripPreviewAttrs
    } else {
        ShipTransform::CopyAsIs
    }
}

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
            let after_preview = STRIP_PREVIEW_ATTR.replace_all(&after_source, "");
            let after_markers = STRIP_NO_PREVIEW_MARKER.replace_all(&after_preview, "");
            after_markers.into_owned().into_bytes()
        }
    }
}

// `unlink_if_hardlinked_to` used to live here: it removed a `site_path` that
// shared an inode with `stage_path`, so that `fs::copy`'s `O_TRUNC` open could
// not zero the source out from under the read. `io_utils::copy_output` and
// `io_utils::write_output` never open the destination at all — they populate a
// temp sibling and `rename(2)` it into place — so the hazard is closed by
// construction and the helper had no callers left. See ADR-043.

// ---------------------------------------------------------------------------
// ship_phase  (batched, end-of-blocking)
// ---------------------------------------------------------------------------

/// Ship one generation: copy exactly what the sealed manifest lists.
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
///   zero both stage and site, leaving permanent 0-byte stubs.
///
/// An entry whose stage file is not present is skipped, not counted: the
/// presence pass leaves exactly one such class behind (`_moss/math/`, kept on
/// purpose), and reading an evicted source is the fault this whole change
/// exists to stop being fatal. What remains countable is a write fault on the
/// generation side — per-file faults are counted, not raised, so one bad entry
/// does not hide the rest, and a non-zero count returns `Err` so the
/// generation is never promoted.
///
/// `cancel` is checked between entries. When fired (folder switch / window
/// close) this returns `Ok(())`, matching the cancellation semantics of the
/// `copy_dir_all` it replaced (see #506).
pub fn ship_phase(
    stage_dir: &Path,
    site_dir: &Path,
    sealed: &SealedManifest,
    cancel: Option<&tokio_util::sync::CancellationToken>,
) -> std::io::Result<()> {
    // Count per-file faults so a PARTIAL materialize reports failure (Err),
    // not success. Fix B's mat_ok gate relies on this: a partial generation
    // must fall back to last-known-good, never be promoted or advertised.
    let mut failures = 0u32;

    for (rel_path, entry) in sealed.files() {
        if let Some(c) = cancel {
            if c.is_cancelled() {
                log::info!("ship_phase cancelled (folder closed)");
                return Ok(());
            }
        }

        let stage_path = stage_dir.join(rel_path);
        let site_path = site_dir.join(rel_path);
        let (mode, _) = crate::types::content::parse_entry(entry);

        // `drop_absent_outputs` has already removed every entry with no output
        // — except the `_moss/math/` exemption it keeps deliberately
        // (ADR-030), whose bytes may be evicted. Reading one here is the
        // EDEADLK that failed the whole generation and left `current` where it
        // was, so the exemption is skipped rather than copied.
        //
        // The two passes ask one predicate, but they must not collapse to one
        // verdict: anything else that is absent vanished between them, and
        // promoting a generation short of a file its own manifest names only
        // moves the failure to the next publish. That stays a counted failure.
        if !crate::build::io_utils::entry_output_present(&stage_path, mode) {
            if rel_path.starts_with(crate::build::served_path::MATH_PNG_PREFIX) {
                continue;
            }
            log::warn!(
                "[ship_phase] {:?} is named by the manifest but is not an output; \
                 it went absent after the presence pass",
                stage_path
            );
            failures += 1;
            continue;
        }

        if let Some(parent) = site_path.parent() {
            if let Err(e) = crate::build::io_utils::create_output_dir_all(parent) {
                log::warn!("[ship_phase] create_dir_all for parent {:?}: {}", parent, e);
                failures += 1;
                continue;
            }
        }
        if mode == crate::types::content::MODE_SYMLINK {
            match std::fs::read_link(&stage_path) {
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
                            stage_path.parent().map(|p| p.join(&target)).unwrap_or(target.clone())
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
                    log::warn!("[ship_phase] read_link failed for {:?}: {}", stage_path, e);
                    failures += 1;
                }
            }
            continue;
        }

        match transform_for(rel_path) {
            ShipTransform::StripPreviewAttrs => {
                match std::fs::read(&stage_path) {
                    Ok(bytes) => {
                        let stripped = apply_transform(ShipTransform::StripPreviewAttrs, &bytes);
                        // `write_output` renames a fresh inode into place, so it
                        // can neither truncate an inode shared with `stage_path`
                        // nor materialize a dataless destination (ADR-043).
                        if let Err(e) = crate::build::io_utils::write_output(&site_path, &stripped) {
                            log::warn!("[ship_phase] write failed for {:?}: {}", site_path, e);
                            failures += 1;
                        }
                    }
                    Err(e) => {
                        log::warn!("[ship_phase] read failed for {:?}: {}", stage_path, e);
                        failures += 1;
                    }
                }
            }
            ShipTransform::CopyAsIs => {
                // `fs::copy` (COW on APFS via `fclonefileat(2)`,
                // `copy_file_range(2)` on Linux Btrfs/XFS) rather than
                // `fs::hard_link`: hardlinks share an inode, and a cloud
                // provider evicting that inode turns BOTH stage and site into
                // 0-byte stubs. `copy_output` copies into a temp sibling and
                // renames, so the destination is never opened with `O_TRUNC`
                // and a cloud-evicted `site_path` cannot force materialization
                // (ADR-043).
                if let Err(e) = crate::build::io_utils::copy_output(&stage_path, &site_path) {
                    log::warn!("[ship_phase] copy failed for {:?}: {}", stage_path, e);
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
    Ok(())
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
    /// swap was refused (moss#968 §5d). Not an error — the newer generation is
    /// the right one.
    Superseded,
    /// Not frozen at all: the build could not read its own structural sources,
    /// so its output is a rendering of what happened to be local rather than of
    /// the site (moss#1042, `pipeline::should_publish`).
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
    /// The build could not read its own structural sources (moss#1042,
    /// `pipeline::should_publish`).
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
    /// The verdict a presence pass over a manifest of `entries` reaches, having
    /// dropped `lost` of them and left `sealed.unverified()` behind.
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
/// atomically swap `.moss/build/current` → the new generation.
///
/// Caller must ensure `staging/` is fully populated (post-barrier). The
/// generation dir is created inside this function via `create_dir_all`.
///
/// `epoch` orders this build against every other build of the same folder; the
/// swap goes through `lifecycle::promote`, which refuses it when a newer build
/// has already promoted.
///
/// `verdict` combines `pipeline::should_publish` (carried through
/// `PipelineRunOutput`) with the presence pass's own
/// ([`ShipVerdict::after_presence_pass`]). A `Withhold` returns
/// [`Promotion::Withheld`] before anything is copied.
pub fn materialize_and_promote(
    sealed: &crate::build::manifest::SealedManifest,
    mp: &crate::moss_paths::MossPaths,
    stage_dir: &std::path::Path,
    cancel: Option<&tokio_util::sync::CancellationToken>,
    epoch: u64,
    render: Option<u64>,
    verdict: ShipVerdict,
) -> Result<Promotion, String> {
    if let ShipVerdict::Withhold(reason) = verdict {
        match &reason {
            WithholdReason::SourcesDownloading => log::info!(
                "[cloud] withholding generation {} — the build could not read every structural \
                 source, so `current` stays on the last complete one",
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
    crate::build::io_utils::create_output_dir_all(&gen_dir)
        .map_err(|e| format!("Failed to create generation dir: {}", e))?;
    ship_phase(stage_dir, &gen_dir, sealed, cancel)
        .map_err(|e| format!("Failed to materialize generation {}: {}", sealed.generation_id(), e))?;
    let promoted = crate::build::lifecycle::promote(mp, epoch, render, sealed.generation_id())
        .map_err(|e| format!("Failed to set current_ptr: {}", e))?;
    Ok(if promoted { Promotion::Promoted } else { Promotion::Superseded })
}

/// Drop unreferenced `.webp` variants from `sealed`, before anything persists
/// or ships this generation (moss#976 B2). Called from
/// [`crate::build::degrade::repair_staged_html`], the tail of
/// `advertise_sealed`, which since #1097 is the one seal tail on every path —
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
/// is upload bytes and seta quota (moss-seta#297 S1), NOT local disk — the
/// pruned blob stays in `cache/objects`, kept reachable by its transform
/// record for as long as the source image is in the vault, so `cache::gc`
/// will not collect it. See `build::site_config` for why the off switch exists.
///
/// Returns the condemned keys. A converged build must return NONE: heal-then-
/// prune leaves the same bytes on disk either way, so the end state is
/// identical whether the two agree or fight, and only this set distinguishes
/// them (moss#1085).
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
        // page referenced (moss#976: 525 files deleted, 207 images 404-ing).
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

    // Carry the verdict forward for the next build's producers (moss#1085).
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
/// still be reading `stage_dir` while the seal tail runs (moss#1187-adjacent;
/// see `build::pipeline::sweep_staging`'s doc for the 404 this avoids).
/// `sweep_staging` is how those bytes are normally reclaimed, but it runs at
/// the START of a FUTURE build in the same folder, using the manifest that
/// build inherits. A build that is the last one in its process never gets a
/// future build to do that, so without this call its orphaned `.webp` bytes
/// sit in `stage_dir` forever and ship in anything that reads that tree
/// directly — a raw copy of staging, a snapshot test comparing it
/// byte-for-byte. moss#976 B2 measured exactly that shape on a real site.
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
/// `_moss/math/` is the exception and keeps its entry (ADR-030): those PNGs
/// are append-only and the published site still serves them, so un-promising
/// one deletes it from a live site. A download is requested instead —
/// fire-and-forget, the same pattern the Wait arm and theme assets use — and
/// the local preview 404s that one image until the bytes arrive. [`ship_phase`]
/// skips it by asking the same predicate, so the kept entry never becomes a
/// read of absent bytes.
pub(crate) fn drop_absent_outputs(
    stage_dir: &std::path::Path,
    sealed: &mut crate::build::manifest::SealedManifest,
) -> std::collections::HashSet<String> {
    use crate::build::io_utils::Presence;
    use crate::build::served_path::MATH_PNG_PREFIX;
    let mut absent: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut unverified: Vec<(String, String)> = Vec::new();
    for (rel, entry) in sealed.files() {
        let path = stage_dir.join(rel);
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
    // set — the ADR-030 carve-out above keeps those entries, so a math PNG
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
/// tier ADR-030 carves out), OG cards and video keys out. `<video>` must not
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
    let unregistered: std::collections::HashSet<String> = scan
        .tails
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
// Generation GC
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::manifest::{HashBucket, PendingManifest};
    use crate::types::content::SiteHashes;
    use tempfile::tempdir;

    #[test]
    fn transform_for_html_returns_strip() {
        assert_eq!(
            transform_for("index.html"),
            ShipTransform::StripPreviewAttrs
        );
        assert_eq!(
            transform_for("articles/foo/index.html"),
            ShipTransform::StripPreviewAttrs
        );
        assert_eq!(
            transform_for("legacy.htm"),
            ShipTransform::StripPreviewAttrs
        );
    }

    #[test]
    fn transform_for_non_html_returns_copy() {
        assert_eq!(transform_for("style.css"), ShipTransform::CopyAsIs);
        assert_eq!(transform_for("og/home.png"), ShipTransform::CopyAsIs);
        assert_eq!(transform_for("rss.xml"), ShipTransform::CopyAsIs);
        assert_eq!(transform_for("data.json"), ShipTransform::CopyAsIs);
        assert_eq!(transform_for("video.mp4"), ShipTransform::CopyAsIs);
    }

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
        // data-moss-preview is used as a bare attribute (no ="..."); the regex
        // matches the leading whitespace + the attribute name.
        let html = r#"<body data-moss-preview>content</body>"#;
        let stripped = apply_transform(ShipTransform::StripPreviewAttrs, html.as_bytes());
        assert_eq!(
            std::str::from_utf8(&stripped).unwrap(),
            r#"<body>content</body>"#
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

    /// A folder switch mid-ship must copy nothing, not a partial generation.
    /// `ship_phase` inherited the cancellation contract from the `sync_dir` it
    /// replaced, and it is the one behaviour of that helper the other ship tests
    /// do not cover.
    #[test]
    fn ship_phase_respects_a_pre_set_cancel() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        std::fs::create_dir_all(&src).unwrap();
        for i in 0..5 {
            std::fs::write(src.join(format!("f{}.txt", i)), b"x").unwrap();
        }
        let cancel = tokio_util::sync::CancellationToken::new();
        cancel.cancel();
        let names: Vec<String> = (0..5).map(|i| format!("f{}.txt", i)).collect();
        let sealed = manifest_of(&names.iter().map(|n| (n.as_str(), &b"x"[..])).collect::<Vec<_>>());
        ship_phase(&src, &dst, &sealed, Some(&cancel)).unwrap();
        assert!(
            !dst.exists() || std::fs::read_dir(&dst).unwrap().next().is_none(),
            "no files should be copied when cancel is pre-set"
        );
    }

    /// Fail closed, end to end: one unreadable page and the prune condemns
    /// NOTHING, keeps the manifest whole, and records no verdict.
    ///
    /// The scan's token matching errs wide, but its I/O used to err into an
    /// irreversible delete: an unreadable page silently shrank the reference
    /// set, and a smaller reference set authorizes more deletion (moss#976).
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
        ship_phase(stage.path(), site.path(), &sealed, None).unwrap();

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

        ship_phase(stage.path(), site.path(), &sealed, None).unwrap();

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
        let result = ship_phase(stage.path(), site.path(), &sealed, None);
        assert!(
            result.is_err(),
            "ship_phase must return Err when a file fails to materialize"
        );
    }

    /// One absent condition, two fates. Skipping the math exemption is what
    /// keeps an evicted PNG from failing the whole generation (ADR-030);
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
        ship_phase(stage.path(), site.path(), &pending.seal(), None)
            .expect("an absent math PNG is skipped, not counted");

        let sealed = manifest_of(&[("page/index.html", b"<h1>hi</h1>")]);
        assert!(
            ship_phase(stage.path(), site.path(), &sealed, None).is_err(),
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
        ship_phase(stage.path(), site.path(), &sealed, None).unwrap();

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
        ship_phase(stage.path(), site.path(), &sealed, None).unwrap();

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
        ship_phase(stage.path(), site.path(), &sealed, None).unwrap();

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
        // shared inode is replaced rather than truncated (ADR-043).
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
        ship_phase(stage.path(), site.path(), &sealed, None).unwrap();

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
        ship_phase(stage.path(), site.path(), &sealed, None).unwrap();

        assert_eq!(
            std::fs::read(site.path().join("clip.mp4")).unwrap(),
            b"recovered bytes",
            "site/clip.mp4 must be overwritten with stage bytes, not left as 0-byte stub"
        );
    }

    #[test]
    fn apply_strip_removes_no_preview_markers_keeps_content() {
        let html = r#"<body data-moss-preview><!--moss:no-preview--><script src="a.js"></script><!--/moss:no-preview-->x</body>"#;
        let out = std::str::from_utf8(
            &apply_transform(ShipTransform::StripPreviewAttrs, html.as_bytes())
        ).unwrap().to_string();
        assert!(out.contains("a.js"), "analytics content must survive into the artifact");
        assert!(!out.contains("moss:no-preview"), "marker comments must be stripped on ship");
    }

    // ─── Promotion ordering (#968 §5d) ──────────────────────────────────────

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
            None,
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
            materialize_and_promote(&sealed, &mp, &stage, None, next_promotion_epoch(), None, verdict)
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
            tails: [
                "assets/gone.webp",
                "gone.webp",
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
        // `path_suffixes` emits every shorter tail of a token, so the scan
        // reports `gone.webp` beside `assets/gone.webp`. It is inert rather
        // than wrong: `degrade` resolves each srcset URL against its own page
        // before testing membership, so a tail no page resolves to strips
        // nothing — and one that does resolve names a root-level file that
        // this same predicate has already found unregistered and absent.
        assert!(strip.contains("gone.webp"), "{strip:?}");
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
            "ADR-030 math PNGs are append-only and served from the live site; the \
             `.webp` scope is what keeps them (and OG cards) out: {strip:?}"
        );
        assert!(
            !strip.contains("videos/talk.mp4"),
            "video has no `set_failed` path and an emptied <video> falls through to \
             nothing — widening this scope must be a visible edit: {strip:?}"
        );
    }
}
