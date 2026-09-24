//! What a file change means: the decisions behind a live-development rebuild.
//!
//! This module answers one question — *should this batch of file-system events
//! rebuild the site, and did the rebuild change anything the preview must
//! reload?* It owns the predicates, the content-hash gate, rename pairing and
//! the output diff. It runs no watcher and holds no window: the debouncer, the
//! rebuild lock and every piece of managed state live in
//! [`crate::build_shell::watch`], which calls everything here and decides
//! nothing itself. The two files are one feature; read them together.
//!
//! The split is why this half names no tauri — it travels to `moss-build`,
//! and the other half cannot.
//!
//! ## The debouncer delays; it does not enforce a quiet period
//!
//! `notify-debouncer-full` stamps each raw event with its own arrival time
//! (`push_event`) and releases it `DEBOUNCE_MS` later, draining a path's queue
//! only up to the first event still too young (`debounced_events`). Nothing
//! resets. So it merges what lands inside one drain (tick = `timeout/4` =
//! 62.5ms) and preserves the spacing of everything else: two events 300ms
//! apart are two rebuilds 300ms apart, and raising `DEBOUNCE_MS` only delays
//! both. Measured against 0.3.2 — two writes 40ms apart gave two batches 63ms
//! apart; 80ms apart gave 126ms.
//!
//! This said the opposite until 2026-08-31 ("a resetting 250ms quiet window;
//! 10 plugin downloads, one rebuild"), which is why a user's paired triggers
//! read as a debounce artefact instead of as two real disk events. Coalescing
//! past one drain happens downstream, in the worker's capacity-1 slot.
//!
//! The loop lives inside [`notify_debouncer_full::new_debouncer`]; our wrapper
//! forwards its batched callback onto `trigger_rebuild_with_lock`.
//!
//! ## Rename pairing (PATTERN 6 — INODE+FILE-ID PAIRING)
//!
//! Adopted from `notify-debouncer-full` (canonical inode-pair implementation)
//! and Spacedrive `crates/fs-watcher/src/platform/macos.rs`. The debouncer
//! stitches the platform-specific rename pair —
//!
//! - macOS: `Modify(Name(From))` + `Modify(Name(To))` (FSEvents)
//! - Linux: `Modify(Name(From))` + `Modify(Name(To))` (inotify cookie)
//! - Windows: `Modify(Name(From))` + `Modify(Name(To))`
//!   (`FILE_ACTION_RENAMED_OLD_NAME` + `_NEW_NAME`)
//!
//! — into a single `Modify(Name(Both))` event with `paths = [from, to]`,
//! using inode/file-id matching. This is the unified path for both in-app
//! and external (Finder/CLI/git) renames, replacing the previous
//! `RebuildState.pending_renames` queue.
//!
//! ## Rebuild Coordination
//!
//! When the debounce timer fires, `trigger_rebuild_with_lock()` ensures:
//! - No concurrent rebuilds (would cause "Directory not empty" errors)
//! - At most one pending rebuild queued
//! - Preview refreshes once per rebuild cycle, not per file change
//!
//! ## Features
//! - Async file watching with tokio integration
//! - A fixed 250ms per-event delay via `notify-debouncer-full` (not a quiet period)
//! - Cross-platform rename pair stitching (inode/file-id pairing)
//! - Rebuild locking to prevent concurrent directory operations
//! - Smart diff detection for efficient browser refresh

use crate::types::{content::SiteHashes, events::FileChangeEvent};
use notify::event::{ModifyKind, RenameMode};
use notify::EventKind;
use notify_debouncer_full::DebouncedEvent;
use std::path::{Path, PathBuf};

use crate::build::types::{identity_disagrees, SourceMetadata};

pub mod drift;
pub mod scope;
pub use scope::{
    any_path_passes_filter, any_path_watchable, path_is_watchable, path_passes_filter,
};
pub use scope::{should_watch_file, should_watch_moss_file};

use super::pipeline::load_previous_hashes;
use super::BuildTrigger;


/// Classify an actionable debounced batch.
///
/// `Structural` if ANY event in the batch is a create/remove/rename — a
/// mixed batch (e.g. one content edit + one new file) cannot be salvaged as
/// `ContentOnly` by dropping the structural event, because the rebuild still
/// has to account for it. `ContentOnly` requires EVERY event to be a plain
/// modify. Paths are the same `path_passes_filter`-eligible set the trigger
/// log already computes; `Full` (no paths) should not occur here since the
/// caller already checked `actionable` is non-empty, but is the safe
/// fallback if a future caller passes an empty slice.
pub fn classify_trigger(root: &Path, events: &[&DebouncedEvent]) -> BuildTrigger {
    let mut paths = Vec::new();
    let mut structural = false;
    for ev in events {
        if matches!(ev.kind, EventKind::Create(_) | EventKind::Remove(_))
            || matches!(ev.kind, EventKind::Modify(ModifyKind::Name(_)))
        {
            structural = true;
        }
        paths.extend(ev.paths.iter().filter(|p| path_passes_filter(root, p)).cloned());
    }
    if paths.is_empty() {
        BuildTrigger::Full
    } else if structural {
        BuildTrigger::Structural(paths)
    } else {
        BuildTrigger::ContentOnly(paths)
    }
}

/// Pull `(from_rel, to_rel)` source-relative pairs out of stitched rename
/// events for downstream resolution to `(old_output, new_output)` pairs in
/// `FileChangeEvent.moved_output_paths`.
///
/// PATTERN 6 — INODE+FILE-ID PAIRING via notify-debouncer-full
///
/// The debouncer emits a stitched rename as
/// `EventKind::Modify(ModifyKind::Name(RenameMode::Both))` with `paths =
/// [from, to]`. Events without exactly two paths are skipped — that shape
/// is the crate's contract for a confirmed pair.
pub fn extract_rename_pairs(
    events: &[&DebouncedEvent],
    folder_path: &str,
) -> Vec<(String, String)> {
    let folder = Path::new(folder_path);
    events
        .iter()
        .filter_map(|e| {
            if !matches!(
                e.kind,
                EventKind::Modify(ModifyKind::Name(RenameMode::Both))
            ) {
                return None;
            }
            if e.paths.len() != 2 {
                return None;
            }
            let from = path_to_relative_key(folder, &e.paths[0])?;
            let to = path_to_relative_key(folder, &e.paths[1])?;
            Some((from, to))
        })
        .collect()
}



/// Pick the `previous_hashes` baseline for `do_rebuild_and_notify`'s
/// pre/post change-detection diff.
///
/// The hash-diff at the end of `do_rebuild_and_notify` decides whether to
/// emit `FileChanged` based on (pre-rebuild hashes) vs (post-rebuild
/// hashes). The implicit assumption is that the on-disk output tree
/// (`.moss/build.nosync/staging/`) reflects the state hashes.json describes — moss
/// is the only writer.
///
/// When an external process wipes staging/ between ticks (iCloud "Optimize
/// Mac Storage" eviction, antivirus quarantine, manual `rm`, etc.),
/// hashes.json survives in `build/` but its referenced files are gone.
/// Without this guard, the pre/post comparison would still match — no
/// refresh emitted, browser stays on its 404 even though the pipeline
/// rewrote staging/ during the rebuild.
///
/// Substituting `SiteHashes::default()` when output is missing makes the
/// post-rebuild diff see "all files new" and emit the refresh.
fn previous_hashes_for_diff(folder_path: &str, output: &Path) -> SiteHashes {
    if output.exists() {
        load_previous_hashes(folder_path)
    } else {
        log::warn!(
            "Output dir missing at rebuild start: {} — treating as full rebuild for refresh emit",
            output.display()
        );
        SiteHashes::default()
    }
}

/// Choose the baseline (`previous`) hashes for the post-rebuild change diff.
///
/// Prefers the PREVIOUS build's race-free in-memory content hashes (`in_memory`,
/// the AppState stash the prior rebuild left behind) over re-reading
/// `hashes.json` from disk. The disk file is written by a DETACHED seal+persist
/// task (`build.rs`) that can land DURING the next rebuild; a disk-loaded
/// baseline can therefore already contain THIS build's hashes, making the
/// just-edited page look unchanged → omitted from `changed_output_files` →
/// spurious `action=none` → no preview refresh (the frequent "I have to Cmd+R"
/// bug). The `new` side of the diff was already made race-free by the
/// 2026-05-31 stale-hash work; this closes the symmetric baseline gap flagged
/// in that plan's §6. Falls back to disk only when no in-memory baseline exists
/// (first rebuild after launch, or CLI/headless/external builds that never
/// stashed). See `AppState::peek_content_hashes`.
///
/// Prior art (carry a race-free version in-memory rather than re-reading a
/// disk artifact a background task may have rewritten): Vite HMR's per-module
/// version/timestamp stamping <https://vite.dev/guide/api-hmr> and webpack's
/// `currentHash`/`lastHash` <https://webpack.js.org/concepts/hot-module-replacement/>.
pub fn baseline_for_rebuild(
    in_memory: Option<SiteHashes>,
    folder_path: &str,
    output: &Path,
) -> SiteHashes {
    match in_memory {
        Some(h) => h,
        None => previous_hashes_for_diff(folder_path, output),
    }
}


/// Compare old and new site hashes to produce a `FileChangeEvent` for the frontend.
///
/// Returns `None` if there are no meaningful changes (suppresses refresh).
/// Returns `Some(event)` with the appropriate fields populated when changes exist.
///
/// This handles three categories of change:
/// - **Changed**: files present in both old and new with different hashes
/// - **New**: files present in new but not in old (e.g., after folder rename creates new paths)
/// - **Deleted**: files present in old but not in new (e.g., old paths after folder rename)
///
/// Without checking new/deleted files, folder renames would be treated as "no changes"
/// and the preview would never refresh, leaving the browser on a now-404 URL.
fn compute_rebuild_event(
    new_hashes: &SiteHashes,
    previous_hashes: &SiteHashes,
) -> Option<FileChangeEvent> {
    // Pagefind is excluded from all three diffs. Its shards are
    // content-addressed (`zh-hant_b02cc96.pf_index`), so a re-index renames
    // every file it touches and the diff reads it as a bulk create + delete —
    // 85 entries between two consecutive riverbend generations, none of them a
    // change to anything the open page renders. The search UI lazy-loads the
    // bundle at query time, so a stale index in an already-open tab costs at
    // most one stale result set until the next navigation; forcing a refresh
    // (and, when the morph declines, a full reload with a flash) to avoid that
    // is the worse trade. This is the PREVIEW-REFRESH decision only — the
    // manifest and the deploy diff still carry `_moss/pagefind/**` in full,
    // which is required (see `feeds/search_lane.rs`: an exemption there
    // would have the deploy read the bundle as removed and delete it live).
    let is_search = crate::build::served_path::ServedPath::is_search_asset;
    let changed_output_files: Vec<String> = new_hashes
        .get_changed_files(previous_hashes)
        .into_iter()
        .filter(|p| !is_search(p))
        .collect();
    let deleted_files: Vec<String> = new_hashes
        .get_deleted_files(previous_hashes)
        .into_iter()
        .filter(|p| !is_search(p))
        .collect();
    let new_files: Vec<String> = new_hashes
        .get_new_files(previous_hashes)
        .into_iter()
        .filter(|p| !is_search(p))
        .collect();

    let has_changes = !changed_output_files.is_empty()
        || !deleted_files.is_empty()
        || !new_files.is_empty();

    // [diag] preview-wait: this is the output diff vs the baseline content-stash.
    // If a new article's output was already folded into the baseline (by the
    // initial import build's re-scan), all counts are 0 → has_changes=false →
    // the refresh is SUPPRESSED even though the page is correct on disk. This log
    // shows whether the article reached the diff or was already in the baseline.
    log::info!(
        "[diag] preview-wait: output diff vs baseline — changed={} new={} deleted={} → {}",
        changed_output_files.len(),
        new_files.len(),
        deleted_files.len(),
        if has_changes { "REFRESH" } else { "SUPPRESS (no output changes)" }
    );

    if !has_changes {
        return None;
    }

    let mut event = FileChangeEvent::new();
    event.set_changed_output_files(changed_output_files);
    if !deleted_files.is_empty() {
        event.deleted_paths = Some(deleted_files);
    }
    Some(event)
}

/// Look up the output path that a markdown source file produced.
///
/// Primary: consult `hashes.source_to_output` — populated during the HTML
/// write phase from each `ParsedDocument.source_path → ParsedDocument.url_path`,
/// so this is the canonical mapping. Covers slug normalization
/// (`Hello World.md → hello-world/index.html`), language-suffix stripping
/// (`hello.zh.md → hello/index.html`), frontmatter `url:` overrides,
/// translation-home promotion via page_map, and duplicate-slug
/// dedup-with-suffix.
///
/// Fallback (for manifests written by moss versions before this field
/// existed, where `source_to_output` deserializes to empty): try the two
/// stem-based forms `{stem}/index.html` and `{stem}.html` against
/// `hashes.files`. This heuristic only catches sources whose stem already
/// matches the slug (lowercase-kebab ASCII with no overrides), but provides
/// a graceful degradation until the next build heals the manifest.
///
/// Returns `None` for non-`.md` sources, synthetic pages without a
/// `source_path`, or sources whose output isn't in this manifest at all.
fn find_output_for_source(source: &str, hashes: &SiteHashes) -> Option<String> {
    if let Some(out) = hashes.source_to_output.get(source) {
        return Some(out.clone());
    }
    // Backward-compat fallback for manifests without source_to_output.
    let stem = source.strip_suffix(".md")?;
    let pretty = format!("{}/index.html", stem);
    if hashes.files.contains_key(&pretty) {
        return Some(pretty);
    }
    let flat = format!("{}.html", stem);
    if hashes.files.contains_key(&flat) {
        return Some(flat);
    }
    None
}

/// Diff `previous_hashes.source_to_output` against `new_hashes.source_to_output`
/// to find which source files appeared (creates) and disappeared (deletes)
/// this rebuild, with rename pairs deduped from both sides.
///
/// Returns `(creates, deletes)` in source-path domain (project-root-relative,
/// matching the keys of `SiteHashes.source_to_output`).
///
/// **Why dedup against `rename_pairs`:** a renamed file naturally appears as
/// "old path disappeared, new path appeared" in the per-key diff. The watcher's
/// inode pairing (via `notify-debouncer-full`) already identified the rename;
/// emitting it ALSO as a delete + create would force the EntryRegistry to
/// retire the EntryId (on the spurious delete) and mint a new one (on the
/// spurious create), losing identity across rename. The dedup keeps each
/// FS change in exactly one of the three source-domain fields.
///
/// Order of preference: rename > create > delete. A pair in `rename_pairs`
/// occupies its source AND target paths uniquely.
fn compute_source_change_set(
    new_hashes: &SiteHashes,
    previous_hashes: &SiteHashes,
    rename_pairs: &[(String, String)],
) -> (Vec<String>, Vec<String>) {
    let renamed_old: std::collections::HashSet<&str> =
        rename_pairs.iter().map(|(o, _)| o.as_str()).collect();
    let renamed_new: std::collections::HashSet<&str> =
        rename_pairs.iter().map(|(_, n)| n.as_str()).collect();

    let mut creates: Vec<String> = new_hashes
        .source_to_output
        .keys()
        .filter(|src| !previous_hashes.source_to_output.contains_key(src.as_str()))
        .filter(|src| !renamed_new.contains(src.as_str()))
        .cloned()
        .collect();
    creates.sort();

    let mut deletes: Vec<String> = previous_hashes
        .source_to_output
        .keys()
        .filter(|src| !new_hashes.source_to_output.contains_key(src.as_str()))
        .filter(|src| !renamed_old.contains(src.as_str()))
        .cloned()
        .collect();
    deletes.sort();

    (creates, deletes)
}

/// Compose a `FileChangeEvent` from the output-hash diff plus the watcher's
/// stitched rename pairs. Wraps [`compute_rebuild_event`] with these
/// behaviors:
///
/// PATTERN 6 — INODE+FILE-ID PAIRING via notify-debouncer-full
///
/// 1. For each `(old_source_path, new_source_path)` pair from the watcher's
///    stitched rename events, looks up the OLD output path in
///    `previous_hashes` and the NEW output path in `new_hashes`. Emits
///    `(old_output, new_output)` pairs in `moved_output_paths` so they match
///    the iframe's `currentPath` (which is itself an output path after
///    `extractPathFromUrl` normalization).
/// 2. Additionally pairs **source-stable URL changes** by walking
///    `previous_hashes.source_to_output` and looking up each source in
///    `new_hashes.source_to_output`. When a source key is present in both
///    manifests with different output paths, the source filename didn't
///    change but its slug did — e.g., the user removed frontmatter `title:`
///    so the slug falls back from title-derived to filename-derived. These
///    pairs are merged into `moved_output_paths` so the frontend takes the
///    `update-url` branch instead of redirecting home. Note: this pass
///    consults the manifest's `source_to_output` map directly, so legacy
///    manifests written before that field existed get no slug-change
///    protection until the next build heals — same graceful-degradation
///    posture as `find_output_for_source`'s fallback path.
/// 3. Removes the looked-up old output paths from `deleted_paths`. The
///    frontend's deletion-priority check (preview-actions.ts:54-71) would
///    otherwise direct-match the old output path and redirect home,
///    shadowing the rename signal.
///
/// If a pair can't be resolved to both old and new output paths (e.g. the
/// rebuild ran but didn't see the rename, or the source produces no output —
/// notebook/asset renames where the manifest doesn't carry a source mapping),
/// the pair is silently dropped. `deleted_paths` keeps the entry and the
/// iframe redirects home, which is the pre-fix fallback: better to redirect
/// home than to navigate to a non-existent URL.
fn build_rebuild_event_with_renames(
    new_hashes: &SiteHashes,
    previous_hashes: &SiteHashes,
    rename_pairs: &[(String, String)],
) -> Option<FileChangeEvent> {
    // Resolve each source-domain pair to output-domain paths via the manifest.
    // Pairs that can't resolve to BOTH old and new outputs are dropped — see
    // the function doc-comment for rationale.
    let mut output_pairs: Vec<(String, String)> = rename_pairs
        .iter()
        .filter_map(|(old_src, new_src)| {
            let old_out = find_output_for_source(old_src, previous_hashes)?;
            let new_out = find_output_for_source(new_src, new_hashes)?;
            Some((old_out, new_out))
        })
        .collect();

    // Pair source-stable URL changes (same source filename, different output URL).
    // When the user edits data the slug rule reads — e.g., removes frontmatter
    // `title:` so slug falls back from title-derived to filename-derived — the
    // source filename is unchanged but its output URL moves. The frontend's
    // deletion-priority check (preview-actions.ts:54-71) would otherwise see the
    // old output in deleted_paths and redirect to home, snapping the editor and
    // file tree to the home page via the preview→editor sync. Pairing them as
    // renames lets the frontend take the update-url branch and preserves context.
    let existing_old_outs: std::collections::HashSet<String> = output_pairs
        .iter()
        .map(|(o, _)| o.clone())
        .collect();
    for (source, old_out) in &previous_hashes.source_to_output {
        if let Some(new_out) = new_hashes.source_to_output.get(source) {
            if new_out != old_out && !existing_old_outs.contains(old_out) {
                output_pairs.push((old_out.clone(), new_out.clone()));
            }
        }
    }

    // Source-domain change set, computed from the in-memory `source_to_output`
    // map — which is cleared and re-populated every build, so it reflects THIS
    // build's renders. This is the race-free deletion signal: a page not
    // rendered this build is absent from `new.source_to_output` even though its
    // OUTPUT may still linger in `new.files` (carry-forward from the previous
    // manifest, not pruned until `seal`), which is why the `files`-set
    // `get_deleted_files` diff inside `compute_rebuild_event` cannot see a
    // deletion under the in-memory-hash path. Computed before the diff so its
    // mapped output deletions are merged into `deleted_paths` *before* the
    // rename-suppression pass below.
    let (source_creates, source_deletes) =
        compute_source_change_set(new_hashes, previous_hashes, rename_pairs);

    // Map source-domain deletions to OUTPUT-domain paths via the PREVIOUS
    // manifest (which still holds the deleted page's source→output mapping) so
    // they reach `deleted_paths` and the frontend redirects the preview home.
    let mapped_deletes: Vec<String> = source_deletes
        .iter()
        .filter_map(|src| find_output_for_source(src, previous_hashes))
        .collect();

    let mut event = compute_rebuild_event(new_hashes, previous_hashes);

    if !mapped_deletes.is_empty() {
        let mut e = event.unwrap_or_else(FileChangeEvent::new);
        let dp = e.deleted_paths.get_or_insert_with(Vec::new);
        for d in mapped_deletes {
            if !dp.contains(&d) {
                dp.push(d);
            }
        }
        event = Some(e);
    }

    if !output_pairs.is_empty() {
        let mut e = event.unwrap_or_else(FileChangeEvent::new);

        // Suppress the matched old output paths from deleted_paths so the
        // frontend's deletion-priority check doesn't shadow the rename.
        if let Some(deleted) = e.deleted_paths.as_mut() {
            let suppress: std::collections::HashSet<&str> =
                output_pairs.iter().map(|(o, _)| o.as_str()).collect();
            deleted.retain(|d| !suppress.contains(d.as_str()));
            if deleted.is_empty() {
                e.deleted_paths = None;
            }
        }

        e.moved_output_paths = Some(output_pairs);
        event = Some(e);
    }

    // Source-domain fields: surface the per-source changes the watcher detected
    // (and that compute_rebuild_event/output_pairs intentionally collapsed to
    // output-domain). Consumed by the EntryRegistry.
    let source_changes_present = !source_creates.is_empty()
        || !source_deletes.is_empty()
        || !rename_pairs.is_empty();

    if source_changes_present {
        let mut e = event.unwrap_or_else(FileChangeEvent::new);
        if !source_creates.is_empty() {
            e.source_creates = Some(source_creates);
        }
        if !source_deletes.is_empty() {
            e.source_deletes = Some(source_deletes);
        }
        if !rename_pairs.is_empty() {
            e.source_renames = Some(rename_pairs.to_vec());
        }
        event = Some(e);
    }

    event
}

/// Select the new hashes for the refresh decision and compute the
/// `FileChangeEvent`.
///
/// This is the testable core of the stale-hash race fix. `do_rebuild_and_notify`
/// extracts the in-memory stash from `AppState` and calls this function so the
/// decision can be unit-tested without a `tauri::AppHandle`.
///
/// **Why `stash` instead of re-reading `hashes.json`:**
/// `hashes.json` is written by a detached seal+persist task that first awaits
/// the entire background media phase. Re-reading it synchronously right after
/// `run_pipeline()` returns races that write. On asset-heavy sites the read
/// always loses → "no output changes → suppress refresh" even though the page
/// genuinely changed. The stash carries the build's own synchronously-computed
/// hashes, eliminating the race. Fallback to the disk read only when the stash
/// is absent (CLI / headless / no AppState).
pub fn decide_rebuild_event(
    stash: Option<SiteHashes>,
    folder_path: &str,
    previous_hashes: &SiteHashes,
    rename_pairs: &[(String, String)],
) -> Option<FileChangeEvent> {
    let new_hashes = if stash.is_some() {
        log::debug!("[watch] refresh decision: using in-memory content hashes (stash)");
        stash.unwrap()
    } else {
        log::info!("[watch] refresh decision: stash absent — falling back to disk hashes.json (CLI/headless or stash not written)");
        load_previous_hashes(folder_path)
    };
    build_rebuild_event_with_renames(&new_hashes, previous_hashes, rename_pairs)
}

// ---------------------------------------------------------------------------
// Content-hash gate: suppress watcher events when the files they reference are
// byte-identical to the prior build's source manifest. Breaks the runaway
// rebuild loop induced by cloud-sync providers (Dropbox, iCloud) that re-emit
// metadata events for files moss just read.
// ---------------------------------------------------------------------------

/// Outcome of comparing a watcher event's file against the stored source manifest.
#[derive(Debug, PartialEq)]
pub(crate) enum SourceCheck {
    /// Fast path (size+mtime match) or slow path (content hash matches) — suppress.
    Unchanged,
    /// File's current bytes differ from the manifest — rebuild.
    Changed,
    /// File does not exist OR metadata/read/hash I/O failed — caller treats as rebuild (fail open).
    Unknown,
}

/// Compare a file on disk against its previous `SourceMetadata`.
///
/// 1. If size differs → `Changed` (short-circuit; avoid hashing huge files).
/// 2. If size matches AND the mtime matches at full nanosecond precision →
///    `Unchanged` (fast path). This requires an mtime on BOTH sides: the
///    filesystem must report one now, and the manifest must carry
///    `mtime_nanos` (written since this field existed, from a filesystem
///    that had an mtime). A missing mtime anywhere, or a manifest with only
///    whole-second precision, falls through to hashing — a whole-second
///    match cannot rule out a same-size rewrite in the same second, and a
///    size-only match must never suppress.
/// 3. Otherwise → read and hash; compare to `meta.hash`.
///    - Hash match → `Unchanged`.
///    - Hash mismatch → `Changed`.
/// 4. Any I/O error while reading metadata or bytes → `Unknown` (fail open).
pub(crate) fn source_metadata_matches(meta: &SourceMetadata, fs_path: &Path) -> SourceCheck {
    source_metadata_matches_at(meta, fs_path, None)
}

/// One timestamp granularity, generously: exFAT stores mtimes at 2s
/// resolution and SMB servers round to 1–2s, so a write landing inside this
/// window of the manifest's capture could share the recorded mtime while
/// carrying different bytes.
pub(crate) const RACY_WRITE_EPSILON_SECS: u64 = 2;

/// git's racily-clean rule: is this entry's mtime too close to the moment
/// the manifest was captured to trust a whole-timestamp match?
///
/// `captured_at` is [`SiteHashes::captured_at`]; `None` (old manifest, no
/// clock) fails open — nothing is suspect, the fast path keeps working.
/// A racy entry is not "changed" — it merely loses the fast path and is
/// disposed of by the hash tier.
///
/// The window is TWO-sided (`|mtime − captured_at| ≤ ε`), deliberately. The
/// racy case is a write straddling the capture moment; a recorded mtime far
/// in the FUTURE (a file synced from a device with a fast clock) is not
/// ambiguous — a later change would move it off the recorded value like any
/// other mtime. One-sided (`mtime + ε ≥ cap`) marked every future-dated file
/// racy forever, which on a 2s sweep cadence meant re-hashing it every pass
/// for the life of the session.
pub(crate) fn mtime_is_racy(meta: &SourceMetadata, captured_at: Option<u64>) -> bool {
    match captured_at {
        Some(cap) => meta.mtime.abs_diff(cap) <= RACY_WRITE_EPSILON_SECS,
        None => false,
    }
}

/// [`source_metadata_matches`] with the sweep's two demotions armed:
/// ctime/inode disagreement (userland can forge mtime but not ctime, and
/// replace-via-rename changes the inode) and the racy-write guard. Both only
/// ever route to the hash tier — they can never suppress, so a false
/// positive costs one hash and self-absorbs.
pub(crate) fn source_metadata_matches_at(
    meta: &SourceMetadata,
    fs_path: &Path,
    captured_at: Option<u64>,
) -> SourceCheck {
    let md = match std::fs::metadata(fs_path) {
        Ok(m) => m,
        Err(_) => return SourceCheck::Unknown,
    };
    match source_metadata_verdict(meta, &md, fs_path, captured_at) {
        SourceVerdict::Changed => SourceCheck::Changed,
        SourceVerdict::Unchanged { .. } => SourceCheck::Unchanged,
        SourceVerdict::Unknown => SourceCheck::Unknown,
    }
}

/// [`SourceCheck`], plus what the hash tier learned when it had to run.
///
/// `Unchanged { refreshed: Some(_) }` means the bytes provably match but the
/// stat record does not — a provider re-materialization (evict → identical
/// re-download) rewrites ctime and inode without changing content. Callers
/// that hold the baseline (the sweep) write the refreshed record back so the
/// hash is paid ONCE; without the write-back every subsequent pass re-hashes
/// the same file forever, because no build ever runs to absorb the new
/// identity into the manifest.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SourceVerdict {
    Changed,
    Unchanged {
        /// The stat record to store so the next look fast-paths. `None` when
        /// the fast path already matched (nothing to absorb).
        refreshed: Option<SourceMetadata>,
    },
    Unknown,
}

/// The verdict core, over a caller-supplied stat. The three-tier compare
/// itself: size, then trusted mtime(ns), then hash — with the sweep's two
/// demotions armed (ctime/inode disagreement, racy-write window). Both
/// demotions only ever route to the hash tier — they can never suppress, so
/// a false positive costs one hash and, via `refreshed`, absorbs itself.
///
/// Taking `md` as a parameter is what lets the sweep reuse the stat its walk
/// already paid for instead of stat'ing every file a second time per pass.
pub(crate) fn source_metadata_verdict(
    meta: &SourceMetadata,
    md: &std::fs::Metadata,
    fs_path: &Path,
    captured_at: Option<u64>,
) -> SourceVerdict {
    use std::time::UNIX_EPOCH;

    // Size differs: content definitely changed. Don't hash (file may be huge).
    if md.len() != meta.size {
        return SourceVerdict::Changed;
    }

    let (fs_ctime, fs_inode) = crate::build::types::stat_identity(md);
    let fast_path_trusted = !identity_disagrees(meta.ctime, fs_ctime)
        && !identity_disagrees(meta.inode, fs_inode)
        && !mtime_is_racy(meta, captured_at);

    let fs_mtime = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok());
    if let (true, Some(d), Some(nanos)) = (fast_path_trusted, fs_mtime, meta.mtime_nanos) {
        if d.as_secs() == meta.mtime && d.subsec_nanos() == nanos {
            return SourceVerdict::Unchanged { refreshed: None };
        }
    }

    // No trustworthy mtime match: hash to disambiguate.
    use sha2::{Digest, Sha256};
    let bytes = match std::fs::read(fs_path) {
        Ok(b) => b,
        Err(_) => return SourceVerdict::Unknown,
    };
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let computed = format!("{:x}", hasher.finalize());

    if computed == meta.hash {
        SourceVerdict::Unchanged {
            refreshed: Some(SourceMetadata {
                hash: meta.hash.clone(),
                size: md.len(),
                mtime: fs_mtime.map(|d| d.as_secs()).unwrap_or(meta.mtime),
                mtime_nanos: fs_mtime.map(|d| d.subsec_nanos()).or(meta.mtime_nanos),
                ctime: fs_ctime,
                inode: fs_inode,
            }),
        }
    } else {
        SourceVerdict::Changed
    }
}

/// Normalize an absolute event path into the forward-slash, folder-relative key
/// used by `SiteHashes.sources`.
///
/// Returns `None` if the path is not under `folder`. On macOS, `notify` may
/// report `/private/var/...` while `folder` was opened as `/var/...` (symlink);
/// we `canonicalize` both sides before comparing to avoid false negatives.
///
/// If canonicalization fails on either side (e.g. the file has been removed
/// between the event firing and the gate reading it), we fall back to the raw
/// paths. If that fallback produces a prefix mismatch, we return `None` and
/// the gate fails open — `Remove` events never reach this function today
/// (`should_gate_modify_event` filters them), but keep the behavior in mind
/// if the gate's scope is widened.
pub fn path_to_relative_key(folder: &Path, abs: &Path) -> Option<String> {
    let folder_canon = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    let abs_canon = std::fs::canonicalize(abs).unwrap_or_else(|_| abs.to_path_buf());
    let rel = abs_canon.strip_prefix(&folder_canon).ok()?;
    let mut parts = Vec::new();
    for comp in rel.components() {
        use std::path::Component;
        match comp {
            Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
            _ => return None,
        }
    }
    Some(parts.join("/"))
}

/// Decide whether a watcher event with the given paths should trigger a rebuild.
///
/// Consults `SiteHashes.sources` from `baseline` when given — the caller's
/// race-free in-memory stash of the LAST build's hashes (see
/// `AppState::peek_content_hashes`) — falling back to the on-disk manifest
/// only when no stash exists (first rebuild after launch, CLI/headless).
/// The disk file is written by a DETACHED seal task minutes after the build
/// renders, so during that tail it still describes the build BEFORE last:
/// an edit-then-undo lands the file back on the stale manifest's bytes, the
/// disk-based gate calls it `Unchanged`, and the revert never renders. Same
/// race `baseline_for_rebuild` closes for the refresh diff; this closes it
/// for the gate.
///
/// Returns `true` (caller must proceed with rebuild) if any of:
///   - the manifest is missing/corrupt (first build, or write-crash),
///   - any path is not in the manifest (new file, or outside the asset pipeline),
///   - any path's current bytes differ from the stored `SourceMetadata`,
///   - any I/O error or path-outside-folder issue (fail open).
///
/// Returns `false` (suppress) only when every path mapped to an `Unchanged`
/// manifest entry. Callers MUST only consult this for `Modify` events — see
/// `should_gate_modify_event`. `Create`/`Remove` must always trigger a rebuild
/// because the manifest cannot represent a file that didn't exist last build.
pub fn should_rebuild_for_paths(
    folder_path: &str,
    event_paths: &[PathBuf],
    baseline: Option<&SiteHashes>,
) -> bool {
    let folder = Path::new(folder_path);
    let loaded;
    let hashes = match baseline {
        Some(h) => h,
        None => {
            loaded = super::pipeline::load_previous_hashes(folder_path);
            &loaded
        }
    };
    // load_previous_hashes returns SiteHashes::default() on missing/corrupt
    // manifest. That default has an empty `sources` map, so the first path
    // lookup below returns None and we fail open — correct behavior.

    for abs in event_paths {
        let rel = match path_to_relative_key(folder, abs) {
            Some(r) => r,
            None => return true, // outside folder or non-normal component → fail open
        };
        let meta = match hashes.source_metadata_for(&rel) {
            Some(m) => m,
            None => return true, // unknown to manifest → fail open
        };
        match source_metadata_matches(meta, abs) {
            SourceCheck::Unchanged => continue,
            SourceCheck::Changed | SourceCheck::Unknown => return true,
        }
    }
    false
}

/// Whether the platform watcher told us it lost events and we must re-scan.
pub fn watcher_lost_events(events: &[DebouncedEvent]) -> bool {
    events.iter().any(|ev| ev.need_rescan())
}

/// Decide whether a notify event should kick off a rebuild.
///
/// Most metadata events (permissions, ownership, extended attrs) are filtered
/// to avoid iCloud Drive sync noise. WriteTime is kept because editors like
/// Obsidian use atomic saves (write temp → rename), which macOS FSEvents may
/// report as metadata-only on iCloud paths. The `Metadata(Any)` branch is the
/// permissive catch-all for atomic-save variants we cannot enumerate.
///
/// However, `Metadata(Any)` on a **directory** is the iCloud materialization
/// signal: when iCloud downloads an evicted file inside a folder, FSEvents
/// emits a directory metadata event. Reacting to those triggers a rebuild,
/// which reads more files, which materializes more files, which fires more
/// directory metadata events — a self-sustaining loop. The directory case is
/// safe to drop because folder add/remove/rename arrive as separate event
/// kinds (`Create`, `Remove`, `Modify(Name)`), not as `Metadata(Any)`.
pub fn should_recompile_for_event(
    kind: notify::EventKind,
    paths: &[std::path::PathBuf],
) -> bool {
    should_recompile_for_event_with(kind, paths, |p| p.is_dir())
}

/// Same as `should_recompile_for_event` but with the directory check injected.
/// Pure function — no I/O — so the directory rule can be exercised in tests
/// without touching the filesystem.
pub(crate) fn should_recompile_for_event_with(
    kind: notify::EventKind,
    paths: &[std::path::PathBuf],
    is_dir: impl Fn(&std::path::Path) -> bool,
) -> bool {
    use notify::event::{MetadataKind, ModifyKind};
    use notify::EventKind;

    match kind {
        EventKind::Create(_) | EventKind::Remove(_) => true,
        EventKind::Modify(ModifyKind::Metadata(MetadataKind::WriteTime)) => true,
        EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any)) => {
            // Suppress only if every referenced path is a directory.
            // Mixed batches (file + dir) keep the file's signal. Empty path
            // lists fall through as accept — preserves the prior behavior
            // when notify reports a kind without a path.
            paths.is_empty() || !paths.iter().all(|p| is_dir(p))
        }
        EventKind::Modify(ModifyKind::Metadata(_)) => false,
        EventKind::Modify(_) => true,
        _ => false,
    }
}

/// Whether the content-hash gate should be consulted for this event kind.
///
/// Only `Modify` events are gate-eligible. `Create` and `Remove` must always
/// trigger a rebuild because the source manifest cannot represent a file that
/// didn't exist during the prior build.
pub(crate) fn should_gate_modify_event(kind: notify::EventKind) -> bool {
    matches!(kind, notify::EventKind::Modify(_))
}

/// What the pump may decide about one event WITHOUT touching file contents.
///
/// The pump … never touches the disk — the content-hash gate's stat+SHA-256
/// moves to the build worker: the event loop applies only the cheap
/// verdicts here; the hash tier (`should_rebuild_for_paths`) runs at the
/// worker's admission, where a wedged read costs one parked build instead of
/// the whole event loop.
#[derive(Debug, PartialEq)]
pub enum PumpGate {
    /// Drop the event on the pump — every path is a root agent-instruction
    /// file, which the scan skips and which moss itself writes.
    Suppress,
    /// Rebuild unconditionally — the kill-switch is set, or the event kind is
    /// one the hash gate can never vouch for (`Create`/`Remove`).
    Proceed,
    /// Gate-eligible `Modify`: enqueue, and let the worker run the
    /// stat+SHA-256 check (`should_rebuild_for_paths`) at admission.
    DeferHashCheck,
}

/// The pump's half of the old `evaluate_gate`: everything that needs no file
/// I/O. The hash half stays [`should_rebuild_for_paths`], now called by the
/// rebuild worker at admission with the paths this verdict deferred.
///
/// (`is_root_agent_config` canonicalizes, but only after a pure file-NAME
/// match, so the common event costs the pump nothing.)
pub fn pump_gate(kind: notify::EventKind, folder_path: &str, paths: &[PathBuf]) -> PumpGate {
    // Precedes even the kill-switch — see `classify::is_root_agent_config`.
    let root = Path::new(folder_path);
    use crate::build::scan::classify::is_root_agent_config;
    if !paths.is_empty() && paths.iter().all(|p| is_root_agent_config(root, p)) {
        return PumpGate::Suppress;
    }
    // Kill-switch: any set value disables the gate entirely.
    if std::env::var("MOSS_WATCH_NO_GATE").is_ok() {
        return PumpGate::Proceed;
    }
    if !should_gate_modify_event(kind) {
        return PumpGate::Proceed;
    }
    PumpGate::DeferHashCheck
}

// ── SourceAssetChanged helpers ─────────────────────────────────────────────

/// Image extensions whose changes should notify the editor via `SourceAssetChanged`.
pub const IMAGE_EXTENSIONS: &[&str] = &[
    // Kept aligned with the editor's image MIME map (asset_resolver::mime_for_path):
    // every extension that maps to `image/*` there must notify the editor here.
    "jpg", "jpeg", "png", "gif", "webp", "svg", "avif", "bmp", "heic", "heif", "tiff", "tif",
    "ico",
];

/// Project-root-relative `/`-prefixed request path for an image source file
/// under `root`, else None. Pure (no kind check) — kind is filtered by caller.
///
/// Handles two cases:
/// - For files that EXIST on disk: uses `canonicalize` on both sides.
/// - For files that are GONE (deleted/renamed away): does a lexical strip of
///   the (already-absolute) event path against `root`'s canonical form,
///   because `canonicalize` would fail on a missing file.
///
/// Returns the path with a leading `/`, normalizing `\` to `/`, e.g.
/// `/图片/摄影/X.jpeg`. Returns `None` if `abs` is not under `root` or does
/// not have an image extension.
pub(crate) fn source_image_request_path(root: &Path, abs: &Path) -> Option<String> {
    // Check extension first (cheap) before touching the filesystem.
    let ext = abs
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase());
    let is_image = ext
        .as_deref()
        .map(|e| IMAGE_EXTENSIONS.contains(&e))
        .unwrap_or(false);
    if !is_image {
        return None;
    }
    request_path_under_root(root, abs)
}

/// Which `SourceAssetChanged` emissions one debounced event owes the editor.
///
/// Per-path policy (the editor's reference resolver revalidates its whole
/// cache on ANY `SourceAssetChanged`, so the only cost of an emission is one
/// debounced re-resolve batch):
///
/// - **image extension** — structural (create/remove/rename) AND in-place
///   content modifies, because the editor also busts the `?_t=` token so the
///   inline `<img>` repaints fresh source bytes. This is the pre-existing
///   source-image behavior, unchanged.
/// - **other registered asset extension** (pdf/video/audio/model/… from the
///   `moss_core` asset registry, minus markdown) — structural events only.
///   Resolution is path-keyed, so a move/delete/create changes the answer; an
///   in-place edit does not.
/// - **no extension, or an unregistered extension naming a live directory** —
///   structural events only. This is the Finder FOLDER move/rename case: the
///   event's paths are the directory, so no per-child event ever carries an
///   asset extension, yet every asset under it just changed location. A
///   directory that no longer exists (remove / rename-from) can't be probed;
///   the extension-less heuristic covers the common folder name, and a stitched
///   rename's to-side still probes true.
/// - **markdown** — never. Page edits/renames are the rebuild's job
///   (`FileChanged`), and emitting here would re-resolve on every save.
pub fn source_asset_request_paths(
    root: &Path,
    kind: EventKind,
    paths: &[PathBuf],
) -> Vec<String> {
    let structural = matches!(
        kind,
        EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_))
    );
    let content_modify = matches!(
        kind,
        EventKind::Modify(mk) if !matches!(mk, ModifyKind::Metadata(_) | ModifyKind::Name(_))
    );

    let mut out = Vec::new();
    for abs in paths {
        let ext = abs
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase());
        let eligible = match ext.as_deref() {
            Some(e) if IMAGE_EXTENSIONS.contains(&e) => structural || content_modify,
            Some("md") | Some("markdown") => false,
            Some(e) => {
                structural
                    && (moss_core::resolve::asset_registry::asset_info(e).is_some()
                        || abs.is_dir())
            }
            None => structural,
        };
        if !eligible {
            continue;
        }
        if let Some(rp) = request_path_under_root(root, abs) {
            out.push(rp);
        }
    }
    out
}

/// Project-root-relative `/`-prefixed request path for `abs` under `root`,
/// else None. Shared normalization for the source-asset pass — extension
/// policy lives in the callers.
fn request_path_under_root(root: &Path, abs: &Path) -> Option<String> {
    let root_canon = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());

    // Try canonicalize on the abs path first (works if the file exists).
    let rel = if let Ok(abs_canon) = std::fs::canonicalize(abs) {
        abs_canon.strip_prefix(&root_canon).ok()?.to_path_buf()
    } else {
        // File is gone (delete/rename-away). Fall back to lexical strip.
        abs.strip_prefix(&root_canon).ok()?.to_path_buf()
    };

    // Only Normal components allowed (no "..", ".", prefix roots).
    // Also exclude any component that matches is_excluded_dir_name (e.g. `.moss`,
    // `node_modules`, `.git`) so build-output images written under `.moss/build.nosync/`
    // never flood the editor with SourceAssetChanged events.
    let mut parts: Vec<String> = Vec::new();
    for comp in rel.components() {
        use std::path::Component;
        match comp {
            Component::Normal(s) => {
                let s_str = s.to_string_lossy();
                if crate::build::scan::classify::is_excluded_dir_name(&s_str) {
                    return None;
                }
                parts.push(s_str.replace('\\', "/"));
            }
            _ => return None,
        }
    }
    if parts.is_empty() {
        return None;
    }
    // Project-root-relative SOURCE path for the editor's SourceAssetChanged / moss-source://
    // refresh (e.g. `/图片/摄影/X.jpeg`), not a deployed/served site URL — so it does not
    // route through ServedPath. Marker must stay on the format! line (lint is same-line only).
    Some(format!("/{}", parts.join("/"))) // allow:served-path-url-construct
}

/// Collect project-root-relative keys for all RAW file-create events in a batch.
///
/// Used by the RAW CREATE PASS to emit `RawFileCreated` before the rebuild-eligibility
/// gate, so the editor can flash the deepest visible ancestor folder even for files that
/// don't trigger a rebuild (e.g. a newly downloaded Matters article during import).
///
/// `is_dir` is injected so macOS `CreateKind::Any` folder events can be excluded without
/// a real `Path::is_dir()` call in tests. In production, pass `|p| p.is_dir()`.
pub fn collect_raw_create_keys(
    events: &[DebouncedEvent],
    root: &Path,
    is_dir: impl Fn(&Path) -> bool,
) -> Vec<String> {
    let mut keys = std::collections::HashSet::new();
    for ev in events {
        if !matches!(ev.kind, EventKind::Create(_)) { continue; }
        for abs in &ev.paths {
            if is_dir(abs) { continue; }                 // exclude folder creates
            if let Some(key) = raw_create_key(root, abs) { keys.insert(key); }
        }
    }
    let mut out: Vec<String> = keys.into_iter().collect();
    out.sort();
    out
}

/// Convert an absolute path into the project-root-relative forward-slash key used by
/// `RawFileCreated` (no leading slash, nothing the file tree hides, no non-Normal components).
///
/// The only consumer of this event flashes the file's row in the editor tree, so a key
/// naming something the tree does not show is a promise about a row that does not exist.
/// The filter therefore defers to `scan::classify::is_hidden` — the same predicate
/// `list_tree` filters with — rather than keeping a second list in step by hand. That
/// second list had already drifted: it excluded `.moss/`, `.git/` and `node_modules/`,
/// but not a root `AGENTS.md`/`CLAUDE.md`/`GEMINI.md`, which the tree hides and which
/// **moss writes itself** when agent-file sync is on. So the common case — moss creating
/// the very file it hides — announced a path no row could match.
fn raw_create_key(root: &Path, abs: &Path) -> Option<String> {
    // path_is_watchable rejects dotfiles/dirs (e.g. `.git`, `.secret.md`) ANYWHERE in the
    // path and `node_modules`. It is still needed alongside `is_hidden`, which only
    // applies its dotfile rule at the project root. It does NOT exclude all `.moss/`
    // subdirs — it explicitly allows `.moss/theme/`, `.moss/data/social/`,
    // `.moss/assets/`, `config.toml` (see `should_watch_moss_file`); the component loop
    // BELOW is the authoritative `.moss/` exclusion here. Do NOT remove either as
    // redundant.
    if !path_is_watchable(root, abs) { return None; }    // rejects dotfiles + node_modules
    let root_canon = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let abs_canon = std::fs::canonicalize(abs).unwrap_or_else(|_| abs.to_path_buf());
    let rel = abs_canon.strip_prefix(&root_canon).ok()?;
    let mut parts: Vec<String> = Vec::new();
    for comp in rel.components() {
        match comp {
            std::path::Component::Normal(s) => {
                let s_str = s.to_string_lossy().replace('\\', "/");
                // `parent_relative` is exactly what `list_tree` passes when it filters
                // this entry: "" at the project root, else the accumulated prefix.
                // `show_internal: false` because the flash targets the default tree.
                if crate::build::scan::classify::is_hidden(&s_str, &parts.join("/"), false) {
                    return None; // .moss/, .git/, node_modules/, target/, root agent configs
                }
                parts.push(s_str);
            }
            _ => return None,
        }
    }
    if parts.is_empty() { return None; }
    Some(parts.join("/"))                                 // NO leading slash
}

#[cfg(test)]
#[path = "watch_tests.rs"]
mod tests;
