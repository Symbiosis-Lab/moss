//! Build orchestration for static site generation.
//!
//! `run()` / `build()` is the single entry point for all build paths (CLI and
//! GUI). Behavioral differences between preview/build/watch/no-plugins live in
//! the `Config` passed in, never in branches here (ADR-010).
//!
//! ## Two-Phase Build (ADR-001)
//!
//! **Blocking phase (~1s)**: scan, markdown→HTML, document setup (language,
//! slugs, navigation), HTML/CSS/JS emission, RSS/sitemap/robots.txt. It
//! completes before the browser opens; the user sees the page immediately.
//!
//! **Background phase (async)**: asset copying, video conversion, media
//! collection pages, hash persistence, and the seal/materialize step below.
//! Progress reaches the UI via `emit_tier2(PipelineEvent::BackgroundProgress)`.
//!
//! ## Zero-Flicker Staging → Generations
//!
//! There is no `site/` directory any more. A build writes annotated HTML
//! (`data-source-line` for editor↔preview scroll sync) into
//! `.moss/build/staging/`, and deployment-ready output lives in immutable
//! `.moss/build/generations/<id>/` directories behind the `current` symlink:
//!
//! 1. Build to `staging/`                    (server still serves `current`)
//! 2. Switch server pointer to `staging/`    (instant — preview shows new content)
//! 3. Seal + materialize to `generations/`   (`ship_phase` strips the
//!    `data-source-*` annotations from `.html` during this copy)
//! 4. Atomically swap the `current` symlink  (deployment-ready, zero-flicker)
//! 5. Delete `staging/`                      (RAII guard)
//!
//! The server rests on `staging/` (annotated) while a preview session is live;
//! `current` is always clean production HTML. Background workers write their
//! binary outputs (videos, images) into `staging_dir` only —
//! `materialize_and_promote` propagates the stage into the generation after
//! the seal, so there is no second destination to write during the build.
//!
//! ## File-Tree → Page-Tree Mapping
//!
//! All output paths must flow through `resolve_path_with_overrides()` (defined
//! in `page_map.rs`) before being used for disk writes, hash keys, or event paths.
//! This maps filesystem directory names to their url-overridden equivalents
//! (e.g., "视频/clip.mp4" → "video/clip.mp4").
//!
//! The mapping is carried from the blocking phase to the background phase via
//! `BackgroundContext::dir_overrides`. Each background function reads it:
//!
//! ```text
//! source_path (file-tree)
//!     │
//!     ├─ [read source]  →  use raw source_path (for fs::read, fs::copy SOURCE)
//!     │
//!     └─ [write output]  →  resolve_path_with_overrides()  →  mapped_path
//!                              ├── staging_dir.join(mapped)     // disk output
//!                              ├── hashes.video_outputs key     // stale cleanup
//!                              ├── AssetReady event path        // preview swap
//!                              └── AssetRegistry key            // placeholder lookup
//! ```

use crate::moss_paths::MossPaths;
use crate::build::enhance::ResolvedSlots;
use crate::{build::types::ParsedDocument, types::{content::{ProjectStructure, SiteHashes}, runtime::SiteDirectoryState, services::BuildServices}};
use std::fs;
use std::path::Path;

use super::media::pipeline::{
    compute_expected_dirs, copy_dir_recursive, stage_copy, stage_write,
    remove_stale_dirs, remove_stale_files, remove_stale_html,
};
use crate::build::background::BackgroundHandle;
use crate::build::render::generate_blocking_content;
use crate::build::manifest::PendingManifest;
use crate::build::outcome::BuildStopped;
use super::progress::PipelineEvent;
use crate::advisory::{Action, Advisory, Scope, Severity};
use super::image::dispatch_image_conversions;
use super::video::{cleanup_legacy_video_cache, dispatch_video_conversions};

/// Per-notebook synchronous I/O timeout (seconds).
///
/// Bounds how long any one notebook's blocking filesystem operations
/// (`fs::read_to_string`, `fs::copy`, `extract_asset_dependencies`) may stall
/// the build thread before the pipeline gives up and moves on. The default
/// (30s) is generous enough to allow Dropbox CloudStorage / iCloud
/// rehydration of a single moderate-sized .ipynb (~MBs), and tight enough
/// that one stuck file does not freeze the entire build. On timeout the
/// notebook is skipped and a warning is logged; the next rebuild retries.
///
/// Promoted to a module-level const so future work — e.g. a configurable
/// build-timeout setting or surfacing the timeout in a `PipelineEvent::Warning`
/// — has a single named knob to reach for. See investigation 2026-05-16
/// (chps deploy stuck at "Generating pages... (1 / 2)") and the corresponding
/// arch review note recommending this lift.
const NOTEBOOK_IO_TIMEOUT_SECS: u64 = 30;

/// Above this, a stage-write lock wait is reported at `warn`, not `debug` — a
/// release preview build is where a user feels it and it was invisible there.
/// Well above the uncontended cost (microseconds), well below the ~2s moss#968
/// measured, so it fires on the real condition only.
const STAGE_WRITE_LOCK_WARN: std::time::Duration = std::time::Duration::from_millis(200);

/// Heuristic detection: does this absolute path live under a known cloud-sync
/// mount?
///
/// Cloud-sync clients (Dropbox, OneDrive, Google Drive, iCloud) can block
/// synchronous I/O — `fs::read_to_string`, `fs::copy`, directory walks — for
/// arbitrary durations while reconciling state or rehydrating a dehydrated
/// (cloud-only) file. Builds whose source folder sits under one of these
/// mounts are at elevated risk of stalling; the notebook processing loop logs
/// a one-time warning when this returns `true` so a hung build's stderr makes
/// the cause obvious.
///
/// The check is a substring match — fast, no allocations beyond what `Path`
/// would do, and tolerant of how the user's home directory is spelled.
/// Patterns covered:
///
/// - `~/Library/CloudStorage/...` — consolidated macOS cloud-mount root,
///   post macOS 12.3 (Dropbox, OneDrive, Google Drive land here).
/// - `com~apple~CloudDocs` — iCloud Drive's filesystem name segment.
/// - `~/Dropbox`, `~/Dropbox (Personal/Work/etc)` — legacy pre-CloudStorage
///   Dropbox layout, still present on machines that never migrated.
/// - `/Volumes/GoogleDrive*` — Google Drive's File Stream mount on macOS
///   (pre-CloudStorage / for users still on the standalone client).
/// - `~/OneDrive*` — Windows-style OneDrive folder.
/// The prefix list itself now lives with the nested-root detector
/// (`crate::nested_roots`), which needs it crate-side; this stays the
/// app-tree spelling so 40+ call sites keep their import.
pub(crate) fn is_cloud_storage_path(source_path: &str) -> bool {
    crate::nested_roots::is_cloud_storage_path(source_path)
}

/// The extensions the home election can actually elect, mirroring
/// `moss_core::home::detect_home_file_in_folder`.
///
/// Deliberately *not* [`is_markdown_path`]. The two sets only overlap:
/// `.markdown`/`.mdown`/`.mkd` are pages but can never win the election, and
/// `index.pages`/`index.docx` win it at Priority 3 without being markdown at
/// all. Using the markdown set here let an evicted Pages home page through the
/// gate silently.
fn is_home_candidate_path(p: &std::path::Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "md" | "pages" | "docx" | "doc"))
}

/// Did this build publish a home page it made up?
///
/// **Not** `index.html.is_file()`. The render pass always writes one — the real
/// home document if it parsed, an auto-index of whatever pages were readable if
/// it did not, and the empty-folder onboarding artifact if nothing parsed at all
/// (`render/blocking.rs`: the `documents.is_empty()` →
/// `synthesize_empty_homepage` push, then the unconditional write). So the file's
/// presence answers "will the user see a 404", and that is not the question the
/// cloud gate asks. The question is whether what they see is *their* home page.
///
/// Both eviction forms reach this through `evicted_paths`, which is why the home
/// election is re-run here instead of read off `project_structure.homepage_file`:
///
/// - **Sonoma+** leaves the real name in the walk, so the scan already elected
///   the evicted file and `homepage_file` names it.
/// - **macOS 12–13** replaces the file with a hidden `.name.icloud` sibling, so
///   the real name never reached the scan at all and `homepage_file` names the
///   *runner-up* — a page the user never nominated as their home.
///
/// Re-running the election over (what the scan elected ∪ the evicted root
/// candidates) answers both with one rule: if a name that is still in the cloud
/// outranks what we published, the published home page is a substitute.
/// Should this build raise the cloud waiting screen?
///
/// Pure so the policy can be tested without a vault, a Tauri app handle, or a
/// File Provider — none of which exist on CI, where `is_evicted` is a
/// compile-time `false`.
///
/// Two properties, both of which moss#982 found missing:
///
/// **It is monotonic.** `has_sealed_generation` comes from `current_ptr`, which
/// resolves to the last generation moss sealed for this folder, and a sealed
/// generation never becomes unsealed. So the gate can hold on a cold open and
/// never again — which is what makes a screen with no dismissal control safe.
/// `home_waiting` is re-emitted by every build and the supervisor triggers a
/// build on file arrival, so a re-armable gate can slam a full-window screen
/// back over a site the user is already reading. That is the hazard
/// `docs/archive/2026-08-05-cloud-waiting-screen-redesign.md` §1 removed the
/// escape hatch to contain; a first-run gate cannot re-arm at all. It also
/// matches what `KEEP_GENERATIONS_FLOOR = 2` already guarantees in
/// `store_gc.rs`: from build 2 onward something is always servable.
///
/// **It asks about now, not about scan time.** `cloud_outstanding` folds in the
/// ledger, so an eviction that happened after the scan still counts. The scan
/// count alone is why 578 evicted images could not influence the gate on the
/// incident vault.
///
/// The trade this makes is deliberate: with a servable generation and a home
/// page that has since been evicted, the user sees the substitute home page and
/// the titlebar's download counter rather than the waiting screen. An honest
/// partial state beats a modal blocking a site moss can actually serve. See
/// `docs/archive/2026-08-06-cloud-availability-design.md` §3.4.
///
/// **A site rendered without its sources is not a servable site.** `home_ready`
/// asks only whether an `index.html` was written, and every structural source is
/// deliberately optional to the render — `read_optional_build_input` for the
/// theme, `read_page_source` for each page (failing the build over one file
/// would leave nothing at all to look at). Those together mean a first preview
/// whose sources were still downloading painted a site made of directory names
/// and `Unknown` dates, with no stylesheet, and called it ready. That was the
/// reported symptom: "the preview showed, but the styling and layout are all
/// wrong". `structural_incomplete` is that condition — see
/// [`cloud_ledger::structural_missing_count`], which takes it from what the build recorded
/// rather than re-stat'ing anything here, so the answer is the one the render
/// pass actually acted on. A second opinion taken at gate time could disagree
/// with the HTML already written.
///
/// Both readers request the download, and both `.moss/theme` and the page tree
/// are inside the watcher's allowlist, so the arrival does schedule the rebuild
/// that lowers this — a bounded wait with a live loop behind it, not the
/// unclearable kind `outcome::Disposition::Report` warns about.
///
/// It composes with monotonicity rather than defeating it: `has_sealed_generation`
/// still short-circuits, so this can only ever hold on a cold open, never slam a
/// screen over a site the user is already reading.
///
/// **This is not the publish decision.** `has_sealed_generation` short-circuits
/// here because a sealed generation means there is something to *look at*, which
/// is all a full-window screen needs to know. It says nothing about whether this
/// build's output is fit to replace it — see [`should_publish`], which is the
/// question the same fact was silently answering before moss#1042.
fn cloud_gate_should_hold(
    home_ready: bool,
    has_sealed_generation: bool,
    cloud_outstanding: usize,
    structural_incomplete: bool,
) -> bool {
    let presentable = home_ready && !structural_incomplete;
    let servable = presentable || has_sealed_generation;
    !servable && cloud_outstanding > 0
}

/// May this build's output replace what the preview is already serving?
///
/// The bug this exists to answer (moss#1042): a client opened a Google Drive
/// vault whose sources had been dehydrated but whose `.moss/build` was still
/// local. The preview showed their real site — the sealed generation, correctly
/// served by the zero-flicker switch — and then a few seconds later replaced it
/// with a build rendered from files that were not there: every title a directory
/// name, every date `Unknown`, no stylesheet. Both halves came from the single
/// fact that a sealed generation existed, used to mean two different things:
/// "there is something to show" (true, and why the good site appeared) and "we
/// need not wait" (false, and why the bad one overwrote it).
///
/// So the publish decision asks only about this build's own inputs. A build that
/// could read every structural source is a true rendering of the site and may
/// publish, however much media is still arriving — media degrades to a
/// placeholder and the site is still the user's site (ADR-013). A build that
/// could not is a rendering of whatever happened to be local, and publishing it
/// is a regression whether or not there is something better underneath.
///
/// **Withholding is safe on a cold vault too.** With nothing sealed, nothing is
/// rolled back by declining to publish, and `cloud_gate_should_hold` above puts
/// the waiting screen up over the same condition. With something sealed, the
/// user keeps reading their real site while the corner counter tells them what
/// is still arriving. Neither state is terminal: the supervisor requests every
/// recorded source and the watcher rebuilds on arrival.
fn should_publish(structural_incomplete: bool) -> bool {
    !structural_incomplete
}

fn emit_initial_build_complete(services: Option<&BuildServices>, site_path: Option<&Path>) {
    let (Some(svc), Some(site_path)) = (services, site_path) else {
        return;
    };
    svc.reporter.stage_ready(site_path);
}

fn home_page_is_a_substitute(project_structure: &ProjectStructure, folder_path: &str) -> bool {
    let root = Path::new(folder_path);

    // The user said which page is home, and that page is here and published.
    // Nothing in the cloud can outrank an explicit marker — it beats every
    // filename rule in `detect_home_file_in_folder_marked` — so re-running the
    // filename election below would only let an evicted also-ran displace a home
    // page that is entirely fine. Reading the marker costs a bounded head read
    // of a file this build just rendered, so it is already local.
    if let Some(elected) = project_structure.homepage_file.as_deref() {
        if crate::build::scan::scan::file_has_home_marker(&root.join(elected)) {
            return false;
        }
    }

    // Root-level home candidates only. A nested page can never win the home
    // election, and an evicted image or video is not a reason to tell the user
    // their site has not arrived.
    let evicted_root: Vec<String> = project_structure
        .evicted_paths
        .iter()
        .filter(|p| is_home_candidate_path(p))
        .filter_map(|p| p.strip_prefix(root).ok())
        .map(|rel| moss_core::slug::normalize_separators(&rel.to_string_lossy()))
        .filter(|rel| !rel.contains('/'))
        .collect();
    if evicted_root.is_empty() {
        return false;
    }

    let mut candidates: Vec<&str> = evicted_root.iter().map(|s| s.as_str()).collect();
    if let Some(elected) = project_structure.homepage_file.as_deref() {
        candidates.push(elected);
    }
    let folder_name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    // Filename rules only. The one marker that can be read was already honoured
    // by the short-circuit at the top; an evicted file cannot be read for its
    // own marker, so there is nothing further to consult here.
    moss_core::home::detect_home_file_in_folder(&candidates, &folder_name)
        .is_some_and(|winner| evicted_root.iter().any(|e| e == winner))
}

/// Read a notebook for its title / manifest entry, distinguishing "not there"
/// from "not down yet".
///
/// Both callers previously used a bare `.ok()`, which silently produced a
/// viewer with no title and a JupyterLite manifest missing the notebook — a
/// degraded page with nothing anywhere recording why, and no download asked
/// for, so it stayed degraded until the user happened to touch the file.
/// Asking is what makes it self-healing: the supervisor sees the arrival and
/// rebuilds.
fn read_notebook_sibling(source: &std::path::Path) -> Option<String> {
    if crate::build::icloud::is_evicted(source) {
        crate::build::cloud_readiness::request_download(source);
        log::warn!("notebook {} is still in the cloud — building without it", source.display());
        return None;
    }
    match fs::read_to_string(source) {
        Ok(text) => Some(text),
        Err(e) if crate::build::icloud::is_offline_not_absent(source, &e) => {
            crate::build::cloud_readiness::request_download(source);
            log::warn!("notebook {} is unreadable but not gone — building without it", source.display());
            None
        }
        Err(_) => None,
    }
}

/// One output moss wrote during notebook processing, and the hash of the bytes
/// it wrote — the digest `PendingManifest::register` would have computed.
///
/// The `ServedPath` rather than a `String` is the point: the producer already
/// knows whether it wrote a verbatim third-party JupyterLite asset or a
/// slug-normalized page, so carrying the constructed type forward means the
/// registration site never has to guess it back from a `jupyter/` prefix.
type NotebookReceipt = (crate::build::served_path::ServedPath, String);

/// Process notebook files in the background: download JupyterLite assets if not
/// cached, copy them to the output, and generate viewer HTML pages.
///
/// Follows the same pattern as `run_video_conversion()`:
/// - Runs after the preview opens (background phase)
/// - Emits progress events for the UI
///
/// # JupyterLite Lazy Download
///
/// JupyterLite (~20MB WASM + JS + CSS) is only downloaded when notebooks are
/// first encountered. Once cached at `~/.moss/assets/jupyterlite/`, subsequent
/// builds reuse the cached copy. This is triggered here (not during scan) so
/// the scan phase stays fast and network-free.
///
/// # Output Layout
///
/// ```text
/// <output>/
///   jupyter/           ← JupyterLite assets (shared across all notebooks)
///     notebooks/       ← Notebook viewer interface
///     lab/             ← Full JupyterLab interface
///     ...
///   notebooks/
///     analysis.ipynb   ← Original notebook (for JupyterLite to load)
///     analysis.html    ← Viewer page with JupyterLite iframe
/// ```
fn run_notebook_processing(
    services: Option<&BuildServices>,
    notebook_files: &[crate::types::content::FileInfo],
    source_path: &str,
    staging_dir: &std::path::Path,
    cancel_flag: Option<&std::sync::atomic::AtomicBool>,
) -> Vec<NotebookReceipt> {
    use crate::build::assets::asset_resolver::resolve_asset_directory;
    use crate::build::notebook::{collect_notebooks, generate_contents_manifest, generate_viewer_html_with_content, jupyterlite_asset_config, patch_jupyterlite_config, CONTENTS_ALL_JSON_FILE};

    let items = collect_notebooks(notebook_files);
    if items.is_empty() {
        return Vec::new();
    }

    // (Pre-Track A this called `svc.notebook_cancellation.start_new_conversion()`
    // to clear the legacy AtomicBool flag at the start of each batch. The
    // flag is gone — folder-switch cancel now flows through
    // `FolderSession::cancel`, and the bridge in the caller seeds a fresh
    // local AtomicBool per call. No reset needed.)

    // Check cancellation before starting (cancel_flag is bridged from
    // FolderSession::cancel; only fires if the session was already cancelled
    // before this batch began).
    if cancel_flag.map_or(false, |f| f.load(std::sync::atomic::Ordering::SeqCst)) {
        log::info!("Notebook processing cancelled before start");
        return Vec::new();
    }

    let total = items.len() as u32;
    let reporter = services.map_or(crate::build::ports::reporter::discarding(), |s| s.reporter.as_ref());
    // `map_or(true, …)`, not `reporter.is_terminal()`: no services at all is
    // also headless, and `discarding()` cannot say so — see `HeadlessReporter`.
    let is_headless = services.map_or(true, |s| s.reporter.is_terminal());

    if is_headless {
        eprintln!("Processing {} notebook(s)...", total);
    }

    // Step 1: Resolve JupyterLite assets (download if not cached)
    let config = jupyterlite_asset_config();

    // Progress callback for download (emit events for GUI)
    let progress_callback: Option<Box<crate::build::assets::download::DownloadProgress>> = services
        .map(|svc| svc.reporter.clone())
        .filter(|r| r.shell_listening())
        .map(|reporter| {
            let f: Box<crate::build::assets::download::DownloadProgress> =
                Box::new(move |bytes_downloaded: u64, total_bytes: Option<u64>| {
                    reporter.download_progress("jupyterlite", bytes_downloaded, total_bytes);
                });
            f
        });

    // Check cancellation before potentially slow download
    if cancel_flag.map_or(false, |f| f.load(std::sync::atomic::Ordering::SeqCst)) {
        log::info!("Notebook processing cancelled before JupyterLite download");
        return Vec::new();
    }

    let jl_assets = match resolve_asset_directory(
        &config,
        progress_callback.as_deref(),
    ) {
        Ok(path) => path,
        Err(e) => {
            log::warn!("JupyterLite assets not available: {}. Notebooks will be copied without viewer.", e);
            if is_headless {
                eprintln!("Warning: JupyterLite not available: {}", e);
            }
            // Fallback: copy .ipynb files as-is without JupyterLite
            let mut fallback_paths = Vec::new();
            for item in &items {
                let source = std::path::Path::new(source_path).join(&item.source_path);
                // .unwrap(): source_path comes from the notebook scanner which
                // produces relative paths under the source folder — known valid.
                let out_path = crate::build::served_path::ServedPath::from_source(&item.source_path).unwrap();
                match stage_copy(&source, &out_path, staging_dir) {
                    Ok(hash) => fallback_paths.push((out_path, hash)),
                    Err(e) => log::warn!("Failed to copy notebook {}: {}", item.source_path, e),
                }
            }
            return fallback_paths;
        }
    };

    // Step 2: Copy JupyterLite assets to output/jupyter/
    // IMPORTANT: Must happen BEFORE creating jupyter/files/ subdirectory,
    // otherwise the existence check sees the partial directory and skips the copy.
    let jupyter_output = staging_dir.join("jupyter");
    let mut bundle_receipts = match copy_dir_recursive(&jl_assets, &jupyter_output) {
        Ok(receipts) => receipts,
        Err(e) => {
            log::warn!("Failed to copy JupyterLite assets: {}", e);
            return Vec::new();
        }
    };
    log::info!("Copied JupyterLite assets to {}", jupyter_output.display());

    // Point the bundle at /jupyter/ (its shipped "./" resolves against the
    // page, not JupyterLite's root) and at the contents index written below.
    //
    // Read the SOURCE bundle, not the copy in the stage. Reading the staged
    // copy back was an `if let Ok`, so a cloud-evicted destination silently
    // skipped the rewrite and shipped a JupyterLite whose baseUrl still said
    // "./" — a broken viewer, reported as a successful build.
    let jl_config = jupyter_output.join("jupyter-lite.json");
    if let Ok(config_str) = fs::read_to_string(jl_assets.join("jupyter-lite.json")) {
        if let Some(json) = patch_jupyterlite_config(&config_str) {
            if crate::build::io_utils::write_output(&jl_config, json.as_bytes()).is_ok() {
                // This build rewrote the file the bundle copy had just placed,
                // so its receipt replaces the one `copy_dir_recursive` returned.
                let rewritten = crate::build::assets::paths::compute_binary_hash(json.as_bytes());
                if let Some(slot) = bundle_receipts
                    .iter_mut()
                    .find(|(rel, _)| rel == "jupyter-lite.json")
                {
                    slot.1 = rewritten;
                }
            }
        }
    }

    // Step 3: Process each notebook
    // Notebooks are copied to two locations:
    // 1. Their natural path in the output (e.g., /resources/analysis.ipynb) — for direct access
    // 2. /jupyter/files/<filename> — where JupyterLite looks for notebook files
    let jupyter_files_dir = staging_dir.join("jupyter").join("files");
    let _ = crate::build::io_utils::create_output_dir_all(&jupyter_files_dir);

    // The manifest lists what moss wrote, and only that. Every entry here is a
    // receipt from a write this build performed — never a walk of the
    // destination, which laundered whatever a sync client had left in
    // `staging/jupyter/` into the manifest as moss's own output.
    let mut generated_paths: Vec<NotebookReceipt> = bundle_receipts
        .into_iter()
        .map(|(rel, hash)| {
            (
                crate::build::served_path::ServedPath::for_jupyterlite_asset(
                    std::path::Path::new(&format!("jupyter/{rel}")),
                ),
                hash,
            )
        })
        .collect();

    // Detect cloud-storage paths (see `is_cloud_storage_path`). Logged once
    // before the loop so a hung build's `moss build` stderr makes the cause
    // obvious.
    let is_cloud_storage = is_cloud_storage_path(source_path);
    if is_cloud_storage && !items.is_empty() {
        log::warn!(
            "Notebook processing on cloud-storage path ({} items): {}. \
             Synchronous I/O may block if the cloud-sync client (Dropbox/\
             OneDrive/Google Drive/iCloud) is reconciling or files are \
             dehydrated. Watch for the per-notebook progress events below \
             to identify any file that hangs.",
            items.len(),
            source_path
        );
    }

    // Per-notebook I/O outcome — exactly one of three states. Carries enough
    // for the caller to (a) decide whether to continue accumulating
    // `generated_paths` and (b) surface a user-visible warning when the
    // build silently skipped a file.
    enum IoOutcome {
        /// Worker thread returned successfully.
        Done(Vec<NotebookReceipt>),
        /// Worker thread returned an error (e.g. `fs::copy` failed because
        /// the cloud-sync client deleted the file out from under us).
        Errored(String),
        /// Worker thread is still running after the timeout. The notebook
        /// is skipped on this build; the next rebuild retries.
        TimedOut,
    }

    // Per-notebook I/O timeout — bounds the time any single notebook's
    // synchronous filesystem operations can hold up the build thread.
    // Cloud-storage paths (Dropbox CloudStorage, iCloud) can stall
    // `fs::read_to_string` and `fs::copy` indefinitely while the
    // sync client reconciles or hydrates a dehydrated file. Pre-2026-05
    // a single stuck notebook would freeze the entire build pipeline
    // at "Generating pages... (i / N)" with no escape; see
    // investigation 2026-05-16. The helper spawns the I/O block on a
    // detached worker thread, waits up to `NOTEBOOK_IO_TIMEOUT_SECS`
    // (module-level const) on a channel, and on timeout returns
    // `TimedOut` so the caller can surface a UI warning. The leaked
    // worker thread completes silently when the OS eventually unblocks
    // the syscall.
    fn with_io_timeout<F>(item_path: &str, f: F) -> IoOutcome
    where
        F: FnOnce() -> Result<Vec<NotebookReceipt>, String> + Send + 'static,
    {
        use std::sync::mpsc;
        use std::time::Duration;
        let (tx, rx) = mpsc::sync_channel(1);
        let label = item_path.to_string();
        std::thread::spawn(move || {
            let r = f();
            let _ = tx.send(r);
        });
        match rx.recv_timeout(Duration::from_secs(NOTEBOOK_IO_TIMEOUT_SECS)) {
            Ok(Ok(paths)) => IoOutcome::Done(paths),
            Ok(Err(e)) => {
                log::warn!(
                    "[notebook] I/O for '{}' returned an error: {}; continuing",
                    label, e
                );
                IoOutcome::Errored(e)
            }
            Err(_) => {
                log::warn!(
                    "[notebook] I/O for '{}' timed out after {}s. Likely cloud-storage \
                     dehydration (Dropbox CloudStorage / iCloud). The notebook is \
                     skipped on this build; the next rebuild will retry. Worker \
                     thread is detached and will exit when the OS unblocks.",
                    label, NOTEBOOK_IO_TIMEOUT_SECS
                );
                IoOutcome::TimedOut
            }
        }
    }

    for (i, item) in items.iter().enumerate() {
        // Check cancellation before each notebook
        if cancel_flag.map_or(false, |f| f.load(std::sync::atomic::Ordering::SeqCst)) {
            log::info!("Notebook processing cancelled after {}/{} notebooks", i, total);
            break;
        }

        // Emit a START-of-iteration progress event so the UI advances *before*
        // any I/O fires. Without this, a hang inside `dual_copy` or
        // `fs::read_to_string` (common when the source is on Dropbox FUSE or
        // similar dehydration-prone cloud storage) leaves the progress panel
        // frozen on the *previous* notebook's "done" message with no
        // indication of which file is stuck. See investigation 2026-05-16
        // (chps deploy stuck at "Generating pages... (1 / 2)").
        reporter.report(&PipelineEvent::BackgroundProgress {
            task: "notebooks".to_string(),
            current: i as u32,
            total,
            message: crate::infra::app_advisory::fmt("processing_notebook", &[("n", &(i + 1).to_string()), ("total", &total.to_string()), ("name", &item.source_path)]),
            completed: false,
            advisories: vec![],
        });

        // Per-notebook I/O is wrapped in `with_io_timeout` so a stuck
        // syscall (Dropbox FUSE, iCloud rehydration) bounds the build's
        // wait. The closure captures owned PathBuf clones of the
        // staging/canonical dirs because the detached worker thread may
        // outlive this iteration's borrow.
        let source = std::path::Path::new(source_path).join(&item.source_path);
        let item_source_path = item.source_path.clone();
        let staging_owned = staging_dir.to_path_buf();
        let notebook_paths = with_io_timeout(&item.source_path, move || {
            let mut produced = Vec::new();
            // Copy .ipynb to its natural served path. Both this path and the
            // viewer .html below normalize through ServedPath so the manifest
            // registration matches what's on disk regardless of source-folder
            // casing.
            // .unwrap(): source_path comes from the notebook scanner — known valid.
            let ipynb_out = crate::build::served_path::ServedPath::from_source(&item_source_path).unwrap();
            let hash = stage_copy(&source, &ipynb_out, &staging_owned)
                .map_err(|e| format!("Failed to copy notebook: {}", e))?;
            produced.push((ipynb_out, hash));

            // Also copy to /jupyter/files/ so JupyterLite can load it
            let notebook_filename = std::path::Path::new(&item_source_path)
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| item_source_path.clone());
            let jl_files_path = crate::build::served_path::ServedPath::from_source(
                &format!("jupyter/files/{}", notebook_filename),
            ).unwrap();
            // Best-effort, as before — but a copy that failed now leaves no
            // receipt, so the manifest cannot promise a file that is not there.
            if let Ok(hash) = stage_copy(&source, &jl_files_path, &staging_owned) {
                produced.push((jl_files_path, hash));
            }

            // Generate viewer HTML alongside the .ipynb.
            // Read notebook content to extract title from metadata or first heading.
            let notebook_content = read_notebook_sibling(&source);
            let viewer_html = generate_viewer_html_with_content(
                &notebook_filename,
                "/jupyter",
                notebook_content.as_deref(),
            );
            let viewer_relative_raw = std::path::Path::new(&item_source_path)
                .with_extension("html")
                .to_string_lossy()
                .to_string();
            let viewer_relative = crate::build::served_path::ServedPath::from_source(&viewer_relative_raw).unwrap();
            match stage_write(&viewer_html, &viewer_relative, &staging_owned) {
                Ok(hash) => produced.push((viewer_relative, hash)),
                Err(e) => log::warn!("Failed to write viewer HTML: {}", e),
            }
            Ok(produced)
        });
        match notebook_paths {
            IoOutcome::Done(paths) => generated_paths.extend(paths),
            IoOutcome::TimedOut => {
                // Surface the skip to the UI as a warning attached to the
                // notebooks progress channel. Without this, the user sees a
                // notebook silently skipped (the log line is invisible from
                // the app) and assumes the build succeeded fully. The
                // BackgroundProgress event's `warnings` array is consumed by
                // the frontend's toast surface, so the user gets one toast
                // per timed-out notebook with the filename to investigate.
                reporter.report(&PipelineEvent::BackgroundProgress {
                    task: "notebooks".to_string(),
                    current: (i + 1) as u32,
                    total,
                    message: crate::infra::app_advisory::fmt("skipped_notebook", &[("n", &(i + 1).to_string()), ("total", &total.to_string()), ("secs", &NOTEBOOK_IO_TIMEOUT_SECS.to_string()), ("name", &item.source_path)]),
                    completed: i + 1 == items.len(),
                    advisories: vec![Advisory::for_source(
                        Scope::File,
                        Severity::ShippedDegraded,
                        &item.source_path,
                        crate::infra::app_advisory::fmt("notebook_skip_advisory", &[("secs", &NOTEBOOK_IO_TIMEOUT_SECS.to_string())]),
                        Action::None,
                    )],
                });
                continue;
            }
            IoOutcome::Errored(_) => {
                // Error already logged inside with_io_timeout. Don't surface
                // as a UI warning yet — errored I/O is often transient (file
                // deleted mid-build by cloud-sync) and noisy to toast. Future
                // work could distinguish "errored once" from "errored across
                // consecutive builds" and toast only the latter.
                continue;
            }
        }

        if is_headless {
            eprintln!("[{}/{}] Done: {}", i + 1, total, item.source_path);
        }

        // Emit progress
        reporter.report(&PipelineEvent::BackgroundProgress {
            task: "notebooks".to_string(),
            current: (i + 1) as u32,
            total,
            message: crate::infra::app_advisory::fmt("processing_notebook_short", &[("n", &(i + 1).to_string()), ("total", &total.to_string())]),
            completed: i + 1 == items.len(),
            advisories: vec![],
        });
    }

    // Step 3.5: Copy sibling data files to /jupyter/files/
    // Allows notebooks to load data via pd.read_csv('data.csv') or np.loadtxt('data.csv').
    // Data files are any non-.ipynb, non-.md files in the same directory as a notebook.
    let mut data_filenames: Vec<String> = Vec::new();
    {
        let mut seen_dirs = std::collections::HashSet::new();
        for item in &items {
            let source_file = std::path::Path::new(source_path).join(&item.source_path);
            if let Some(parent) = source_file.parent() {
                if seen_dirs.insert(parent.to_path_buf()) {
                    if let Ok(entries) = fs::read_dir(parent) {
                        for entry in entries.filter_map(|e| e.ok()) {
                            let path = entry.path();
                            if !path.is_file() { continue; }
                            let ext = path.extension()
                                .and_then(|e| e.to_str())
                                .unwrap_or("")
                                .to_lowercase();
                            // Skip notebooks, markdown, and hidden files (.DS_Store, .gitignore, etc.)
                            if ext == "ipynb" || ext == "md" || ext == "markdown" { continue; }
                            if let Some(fname) = path.file_name().and_then(|n| n.to_str()) {
                                if fname.starts_with('.') { continue; }
                                // .unwrap(): hardcoded prefix "jupyter/files/" + fname from disk — known valid.
                                let jl_path = crate::build::served_path::ServedPath::from_source(
                                    &format!("jupyter/files/{}", fname),
                                ).unwrap();
                                if let Ok(hash) = stage_copy(&path, &jl_path, staging_dir) {
                                    generated_paths.push((jl_path, hash));
                                    data_filenames.push(fname.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Step 4: Generate JupyterLite contents manifest (api/contents/all.json)
    // Without this, JupyterLite cannot discover notebooks in the files/ directory.
    {
        let mut filenames: Vec<&str> = Vec::new();
        let mut contents: Vec<String> = Vec::new();
        for item in &items {
            let source = std::path::Path::new(source_path).join(&item.source_path);
            if let Some(content) = read_notebook_sibling(&source) {
                let fname = std::path::Path::new(&item.source_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&item.source_path);
                filenames.push(fname);
                contents.push(content);
            }
        }
        let content_refs: Vec<&str> = contents.iter().map(|s| s.as_str()).collect();
        let data_refs: Vec<&str> = data_filenames.iter().map(|s| s.as_str()).collect();
        let manifest = generate_contents_manifest(&filenames, &content_refs, &data_refs);
        // .unwrap(): literal prefix + the same basename the bundle config
        // names, so the file written and the file fetched cannot drift.
        let manifest_path = crate::build::served_path::ServedPath::from_source(
            &format!("jupyter/api/contents/{CONTENTS_ALL_JSON_FILE}"),
        )
        .unwrap();
        match stage_write(&manifest, &manifest_path, staging_dir) {
            Ok(hash) => generated_paths.push((manifest_path, hash)),
            Err(e) => log::warn!("Failed to write JupyterLite contents manifest: {}", e),
        }
    }

    if is_headless {
        eprintln!(
            "Notebook processing complete ({} notebook{})",
            total,
            if total == 1 { "" } else { "s" }
        );
    }

    log::info!(
        "Notebook processing complete: {} notebook(s) with JupyterLite",
        total
    );

    generated_paths
}

/// Build a static site from source folder.
///
/// This is the single source of truth for the build process. Both CLI and GUI
/// call this function, ensuring consistent behavior across all entry points.
///
/// ## Zero-Flicker Staging Pattern
///
/// To prevent preview flicker during rebuilds, we use a staging directory and
/// dynamically switch the server's directory pointer:
///
/// 1. Build to `.moss/build/staging/`         (server still serves `current` generation)
/// 2. Switch server pointer to `staging/`     (instant - preview shows new content)
/// 3. Seal + materialize to `generations/`    (immutable frozen generation created)
/// 4. Atomically swap `current` symlink       (deployment-ready, zero-flicker)
/// 5. Delete `staging/`                       (cleanup via RAII guard)
///
/// The `current` symlink is the single source of truth for deployment while
/// the preview never shows 404s or blank pages.
///
/// ## DetachedRegistry Integration
///
/// We track build at the `build()` function level (not deeper) because:
/// 1. This is the single entry point for all build
/// 2. Tracking at a higher level provides cleaner semantics
/// 3. The tracker is for window close decisions, not detailed progress
///
/// # Arguments
/// * `folder_path` - Path to the source folder
/// * `site_dir_state` - Optional state for zero-flicker directory switching
/// * `progress_sender` - Optional channel for progress updates (None for CLI)
/// * `preview_port` - Asked, at completion time, what port is serving this
///   folder — see [`crate::build::ports::LivePortResolver`]
/// * `app_handle` - Optional Tauri app handle for emitting events (None for CLI/tests)
///
/// # Returns
/// * `Ok((bool, Option<BackgroundHandle>))` - Whether the site is empty and the background
///   materialization handle. The handle's `await_completion()` returns the `SealedManifest`
///   once all background workers (video, image, media) have finished. Callers that need to
///   deploy should store the handle until deploy time rather than dropping it immediately.
/// * `Err(BuildStopped)` - Why the build stopped, verdict included.
///
/// # Note
/// Generator plugin support is handled at the build.rs orchestration layer.
/// This function only runs the internal generator.
/// Outcome of [`run`].
///
/// `PipelineRunOutput` keeps the build result pieces in caller priority:
/// - `is_empty`: whether the site has no content files
/// - `bg_handle`: optional materialization barrier for background-phase outputs
/// - `build_documents`: parsed page slice used for native/plugin slot generation
/// - `content_hashes`: in-memory `SiteHashes` clone used by the watcher.
/// - `cancelled`: `true` for an abandoned build, never for a real completion.
///   The other fields hold whatever that build got as far as computing before
///   the folder closed — they describe no site the user is looking at, so
///   callers must neither read `is_empty` as an "empty site" signal nor emit
///   the completion progress event this result would otherwise trigger.
/// - `home_ready`: whether this build actually produced a home page for the
///   preview to serve. See the field's own doc — it decides whether the user
///   sees the site or a "waiting for cloud sync" screen.
pub struct PipelineRunOutput {
    pub is_empty: bool,
    pub bg_handle: Option<BackgroundHandle>,
    pub build_documents: Vec<ParsedDocument>,
    pub content_hashes: SiteHashes,
    /// Media references this build could not resolve to a file. Empty is the
    /// healthy answer and is meaningful — see `AppState::last_missing_media`.
    pub missing_media: Vec<crate::build::types::MissingMedia>,
    pub cancelled: bool,
    /// Whether the build left a home page in the directory the preview serves.
    ///
    /// Read from the staged output rather than predicted from the source
    /// scan, because a prediction is a second opinion that can disagree with
    /// the build — and when the two disagree the user gets either a waiting
    /// screen over a site that built fine, or a live preview of a 404. The
    /// build's output is the same thing the preview server will read, so it
    /// cannot drift from it.
    ///
    /// `false` on an empty site too: there is genuinely nothing to show. The
    /// caller distinguishes the cases via `is_empty`.
    pub home_ready: bool,
    /// Whether this build's output may replace the site already published.
    ///
    /// `false` when a structural source — a page, the config, the user
    /// stylesheet — was still in the cloud, so what the build rendered is a
    /// picture of what happened to be local rather than of the site. The
    /// pipeline has already declined to switch the preview onto it; this
    /// carries the same verdict out to the seal tail, which must also decline
    /// to repoint `current` at it (`ship::Promotion::Withheld`). See
    /// `should_publish`.
    ///
    /// Threaded rather than re-asked of the ledger at seal time on purpose: the
    /// tail runs after the build, and by then the missing sources may have
    /// arrived — which would license promoting a generation that was still
    /// built without them.
    pub publishable: bool,
    /// The render number `lifecycle::show_render` minted for this build, which
    /// the seal tail hands to `lifecycle::promote`. `None` for a build that
    /// stopped before it rendered.
    pub render_seq: Option<u64>,
}

/// Resolves all native/plugin slot content after marked HTML and article-map
/// have been generated, but before stage is shipped, hashed for preview diffing,
/// or sealed for deploy.
///
/// The `&str` is the site's resolved language code and the `Option<bool>`
/// is the site-wide `[site] comments` default, both handed DOWN rather than
/// re-derived — each is one fact a second read could tell twice, disagreeing.
pub type SlotResolver = Box<
    dyn FnOnce(&[ParsedDocument], &str, Option<bool>) -> Result<ResolvedSlots, String> + Send + 'static,
>;

/// [`run_leased`] for a caller that holds no cache-write lease — tests and
/// hosts driving the pipeline directly. `build::run_pipeline` takes one and
/// calls [`run_leased`].
#[allow(clippy::too_many_arguments)]
pub fn run(
    root: &crate::vault::paths::VaultRoot,
    site_dir_state: Option<&SiteDirectoryState>,
    progress_sender: Option<&dyn crate::build::ports::reporter::BuildReporter>,
    preview_port: &crate::build::ports::LivePortResolver,
    services: Option<&BuildServices>,
    slot_resolver: Option<SlotResolver>,
    project_structure: &ProjectStructure,
    site_url_override: Option<String>,
    incremental: crate::build::render::IncrementalGates,
    search_freshness: crate::build::feeds::search_lane::Freshness,
    cache_keys: &crate::build::ports::CacheKeyInputs,
) -> Result<PipelineRunOutput, BuildStopped> {
    run_leased(root, site_dir_state, progress_sender, preview_port, services, slot_resolver, project_structure, site_url_override, incremental, search_freshness, cache_keys, None)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_leased(
    root: &crate::vault::paths::VaultRoot,
    site_dir_state: Option<&SiteDirectoryState>,
    progress_sender: Option<&dyn crate::build::ports::reporter::BuildReporter>,
    preview_port: &crate::build::ports::LivePortResolver,
    services: Option<&BuildServices>,
    slot_resolver: Option<SlotResolver>,
    project_structure: &ProjectStructure,
    site_url_override: Option<String>,
    // moss#922: resolved by `PipelineConfig::allows_incremental_skip` (Stage
    // 5b render skip) and `::allows_parse_cache_reuse` (Stage 7 parse cache) at
    // the entry point, so the render phase never reads the trigger or the
    // environment itself (ADR-010).
    incremental: crate::build::render::IncrementalGates,
    search_freshness: crate::build::feeds::search_lane::Freshness, // ADR-045
    cache_keys: &crate::build::ports::CacheKeyInputs,
    // The caller's `lifecycle::CacheWriteLease`, carried into the background
    // handle so it drops when the workers have joined.
    cache_lease: Option<crate::build::lifecycle::CacheWriteLease>,
) -> Result<PipelineRunOutput, BuildStopped> {
    // Track build state in the FolderSession's UiBound counter for window
    // close / CLI-wait decisions. begin_ui_bound is a no-op in headless mode.
    if let Some(svc) = services {
        svc.begin_ui_bound();
    }

    // Use inner function pattern to ensure build state is cleared on ALL exit paths
    let result = build_inner(root, site_dir_state, progress_sender, preview_port, services, slot_resolver, project_structure, site_url_override, incremental, search_freshness, cache_keys, cache_lease);

    // Clear build state after build completes (success or error)
    if let Some(svc) = services {
        svc.end_ui_bound();
    }

    // The gate verdict is emitted from INSIDE `build_inner`, so any `?` before
    // it skips the gate and the user gets a raw errno where the waiting screen
    // belongs (moss#964). This is the one place that can still speak, and
    // `is_deferred()` is unforgeable by `?` — see `build::outcome`.
    if let Err(ref stopped) = result {
        if stopped.is_deferred() {
            super::cloud_readiness::raise_gate_for_a_deferred_build(
                root.as_str(),
                project_structure.evicted_count,
                &project_structure.evicted_paths,
                services.map_or(crate::build::ports::reporter::discarding(), |s| s.reporter.as_ref()),
                stopped.message(),
            );
        }
    }

    // `is_deferred` has done its work by here; `is_discarded` has not — the
    // rebuild it asks for needs a resolver only `run_pipeline` can mint.
    result
}

/// Inner build function. `root` carries the folder path AND THE folder name (resolved once
/// at the entry point by `vault::paths`), so no consumer re-derives it. Separated so the
/// outer fn can manage DetachedRegistry state on all exit paths (both Ok and Err).
///
/// Returns `PipelineRunOutput`:
/// - `is_empty` — whether the site has no content files
/// - `bg_handle` — materialization barrier for background-phase outputs.
///   `None` when there are no background tasks (image_items, video_items, and
///   deferred are all absent). The caller should store the handle until deploy
///   time (Task 9), or await it inline for headless/CLI builds.
/// - `build_documents` — parsed page slice used for native/plugin slot generation
/// - `content_hashes` — the build's in-memory `SiteHashes` clone, used by the
///   watcher to decide refreshes without re-reading `hashes.json`.
fn build_inner(
    root: &crate::vault::paths::VaultRoot,
    site_dir_state: Option<&SiteDirectoryState>,
    progress_sender: Option<&dyn crate::build::ports::reporter::BuildReporter>,
    preview_port: &crate::build::ports::LivePortResolver,
    services: Option<&BuildServices>,
    slot_resolver: Option<SlotResolver>,
    project_structure: &ProjectStructure,
    site_url_override: Option<String>,
    incremental: crate::build::render::IncrementalGates,
    search_freshness: crate::build::feeds::search_lane::Freshness,
    cache_keys: &crate::build::ports::CacheKeyInputs,
    cache_lease: Option<crate::build::lifecycle::CacheWriteLease>,
    // `BuildStopped`, not `String`: the gate verdict is emitted from inside this
    // function, so the error type must be able to say "not yet" for the caller
    // to raise the gate instead of reporting. See `build::outcome` (moss#964).
) -> Result<PipelineRunOutput, BuildStopped> {
    let build_start = std::time::Instant::now();
    let folder_path = root.as_str(); // the PATH only; `root.name()` is the one name source
    let paths = MossPaths::new(root.path());
    let moss_dir = paths.root().to_path_buf();
    let cloud_reporter =
        services.map_or_else(crate::build::ports::reporter::discard_owned, |s| s.reporter.clone());

    // ProjectStructure is passed in from run_pipeline() — no redundant scan needed.
    // Emit cloud-sync progress if evicted files were detected during the scan.
    //
    // NOTE: The event is still named `icloud-sync` for backwards compatibility
    // with existing frontend listeners, but `SF_DATALESS` is set by every
    // macOS File Provider extension (iCloud, Dropbox, Google Drive, OneDrive,
    // Box), not just iCloud. We detect the provider from the project path so
    // the UI can render an accurate label.
    // This build's record of what it could not read starts empty. The gate is
    // the build's own output, recomputed fresh on every attempt — a file that
    // arrived since the last build must not still be counted against it.
    super::cloud_ledger::begin_build(Path::new(folder_path));

    let icloud_count = project_structure.evicted_count;
    // Detected unconditionally, not only when the scan found something.
    // `detect_from_path` matches the path against the known File Provider
    // roots — it is a string comparison, not a filesystem probe — and gating it
    // on `icloud_count > 0` meant a folder whose files were evicted AFTER the
    // scan reported its provider as "unknown", which the waiting screen renders
    // as "cloud" and which loses the pin tip naming the one fix the user can
    // actually apply.
    let cloud_provider = super::cloud_provider::detect_from_path(Path::new(folder_path));
    let provider_id = cloud_provider.map(|p| p.id()).unwrap_or("unknown"); // logs only

    // One emitter, called once, after the build knows its own answer.
    //
    // The previous design emitted from four places across the build — a
    // pre-build "downloading", two phases inside the blocking wait, and a
    // post-build "done" — and each one was a separate guess at what the user
    // should be looking at. They could and did disagree. This emits the
    // build's actual outcome, and nothing emits before it, so there is no
    // window in which the UI is acting on a guess.
    // `total` is passed in rather than read from `icloud_count` because the
    // scan count is only half the picture — see the `cloud_outstanding`
    // computation at the gate site for why the ledger is the other half.
    let emit_cloud_gate = |waiting: bool, evicted: &[std::path::PathBuf], total: usize| {
        if !cloud_reporter.shell_listening() {
            return;
        }
        // `is_still_in_the_cloud`, not `is_evicted`: a pre-Sonoma placeholder
        // leaves no file at the real path at all, so the plain eviction check
        // would count a file that has not arrived as arrived.
        let remaining = evicted
            .iter()
            .filter(|p| crate::build::icloud::is_still_in_the_cloud(p))
            .count()
            .max(super::cloud_ledger::outstanding(Path::new(folder_path)));
        // A total below `remaining` would render as "700 of 3 downloaded".
        let total = total.max(remaining);
        let phase = if waiting { "home_waiting" } else { "home_ready" };
        log::info!(
            "cloud-sync: build finished {} ({} of {} file(s) still in the cloud, provider: {})",
            if waiting { "WITHOUT a servable site" } else { "with a servable site" },
            remaining,
            total,
            provider_id
        );
        cloud_reporter.cloud_sync(&crate::build::ports::reporter::CloudSync {
            folder: folder_path,
            phase,
            provider: cloud_provider,
            total,
            remaining,
            // The build does not compute the blocking subset; its own gate
            // verdict is the answer to that question (moss#1077).
            blocking: None,
            unavailable: &[],
        });
    };

    if icloud_count > 0 {
        log::info!(
            "cloud-sync: {} evicted file(s) detected during scan (provider: {})",
            icloud_count,
            provider_id
        );
    }
    log::debug!(target: "timing", "[build] using pre-scanned structure ({} files, {} evicted): {:?}",
        project_structure.total_files, icloud_count, build_start.elapsed());

    // Send generating progress
    send_progress(progress_sender, "generating", &crate::infra::app_advisory::t("generating_site"), 50, false, None, None);

    let stage_dir = paths.staging_dir();

    // One-time migration: remove legacy path-based video cache.
    cleanup_legacy_video_cache(&moss_dir);

    // Step 0: where the preview rests while this build rewrites staging, and
    // whether this build may unlink from it. Both are `lifecycle`'s to decide:
    // it parks the preview on `current` only when that is no step back from
    // the render on screen, and a permit is the only licence to unlink below.
    if let Some(state) = site_dir_state {
        crate::build::lifecycle::adopt_server(&paths, &state.current_dir);
    }
    let sweep_permit = crate::build::lifecycle::park_for_rebuild(
        &paths,
        cache_lease.is_some(),
        services.map(|s| s.cancellation.protected_outputs()).unwrap_or_default(),
    );
    crate::build::phase::record_count("parked_current", usize::from(sweep_permit.is_some()));
    let current_ptr_exists = paths.current_ptr().exists();

    // Step 1: Load previous hashes BEFORE building
    // CRITICAL: Must read hashes before generate_blocking_content() runs, because
    // the deferred phase writes new hashes to .moss/build/hashes.json after asset copying.
    // If we read after, we'd be comparing new hashes against themselves!
    let previous_hashes = load_previous_hashes(folder_path);
    log::debug!(target: "timing", "[build] staging: load_hashes: {:?}", build_start.elapsed());

    // An evicted home page used to block this thread here for up to
    // HOME_PAGE_DEADLINE, polling for it to materialize before the build could
    // start. On a vault with 724 dataless files that read as moss freezing at
    // startup, and the wait could not win anyway: macOS abandons a download
    // with no retry and no notification, so the poll had nothing to wait for.
    //
    // The build now simply runs. Whether the home page rendered is answered by
    // the one authority that cannot drift from what the preview will serve —
    // the build's own output — and reported as `home_ready` below. Evicted
    // files are deferred rather than waited on. See docs/archive/2026-08-03-
    // dataless-fail-fast-and-build-driven-cloud-gate.md.
    //
    // The build does NOT hand the files over. The supervisor does, once per
    // sweep, from a walk of the whole vault — so a build doing it too would be
    // a second source of the same list, on every rebuild, including the
    // rebuilds the supervisor itself triggers as files arrive. Nothing would
    // break (the readers dedup), but "who names the files" would have two
    // answers, and only one of them knows what has already arrived.
    // One authority: `build_shell::watch::sweep`. The one exception stays where
    // it belongs — `read_page_source` asks for the specific file it just failed
    // to read, which is a fact only it knows.

    // Seal-persist-race-404 fix: acquire the folder's stage-write guard
    // BEFORE this build starts mutating `stage_dir`, and hold it through the
    // end of this build's own synchronous stage-writing span — which
    // extends through notebook processing below (NOT just remove_stale_html;
    // notebook processing also writes into `stage_dir` and must be covered
    // too). This is the other half of the mutual exclusion whose seal-tail
    // side lives in `build.rs::advertise_sealed` (`ship_phase` +
    // stale-file/dir cleanup) — see the doc comment on
    // `FolderSession::stage_write_lock`. Without it, this build's writes can
    // interleave with a PRIOR generation's still-running detached seal task
    // reading/deleting from this same `stage_dir`, corrupting the
    // generation that gets promoted to `current`.
    //
    // `blocking_lock_stage_write` is safe here because `build_inner` runs
    // exclusively on tokio's blocking pool (via `spawn_blocking` — see
    // `pipeline::run`'s callers in `build.rs`); `None` (headless/no
    // FolderSession, e.g. one-shot CLI builds) means no concurrent rebuild
    // is possible, so no guard is needed.
    //
    // The WAIT is timed, not elapsed-since-start: only the wait says whether the
    // PREVIOUS build's seal tail (which takes this same mutex in
    // `advertise_sealed`, behind every background worker) is on this build's
    // critical path — moss#968 Finding 3, hypothesis H-lock, against H-config
    // just below.
    let t_lock = std::time::Instant::now();
    let _stage_write_guard = services
        .and_then(|s| s.session.as_ref())
        .map(|s| s.blocking_lock_stage_write());
    let lock_wait = t_lock.elapsed();
    log::log!(
        target: "timing",
        if lock_wait >= STAGE_WRITE_LOCK_WARN { log::Level::Warn } else { log::Level::Debug },
        "[build] staging: waited {lock_wait:?} for the stage-write lock (above {STAGE_WRITE_LOCK_WARN:?} means the previous build's seal tail is on this build's critical path — moss#968)",
    );

    // Step 2: Ensure staging directory exists (persistent across rebuilds)
    // We keep site-stage/ between rebuilds so that mtime+size checks can skip
    // unchanged binary assets (videos, images) — avoids re-reading/writing 1GB+.
    //
    // `create_output_dir_all`, not `fs::create_dir_all`: on a cloud-synced vault
    // this is the FIRST thing the build does to the output tree, and a raw
    // `create_dir_all` here failed `EDEADLK` on a Google Drive vault's first
    // preview — putting "Resource deadlock avoided (os error 11)" on the
    // onboarding overlay, before the build could reach the cloud gate that
    // explains itself. The staging tree is regenerable, so ADR-043 applies:
    // unreadable is absent, and it is replaced rather than waited for.
    crate::build::io_utils::create_output_dir_all(&stage_dir)
        .map_err(|e| format!("Failed to create staging directory: {}", e))?;

    // Step 2b: sweep the previous build's residue out of staging.
    sweep_staging(&stage_dir, &previous_hashes, sweep_permit.as_ref());

    // Step 3: Build to staging directory using internal generator
    //
    // ALWAYS annotate /stage with data-source-* (+ data-moss-preview). The preview
    // server rests on /stage (the zero-flicker dance below flips to it at build
    // completion), so click-to-source and scroll-sync need annotations on EVERY
    // build — not just start_server=true ones. The previous `server_port.is_some()` coupling
    // left /stage un-annotated on every watch rebuild (start_server=false) → click-to-source
    // died after the first edit (regression from the documented intent in 70e231d96).
    // `ship_phase` strips BOTH data-source-* and data-moss-preview when shipping
    // /stage→/site, so /site stays deploy-clean regardless. The preview port is
    // still used for the completion progress message (send_progress) — it is no
    // longer the annotation lever. See docs/reference/editor-preview-sync.md
    // mechanism 1.
    let emit_source_lines = true;

    // The `.moss/` on-disk migrations that used to run here — the
    // `config.toml` schema bump and the legacy email-path rename — are now a
    // precondition the entry point satisfies via
    // `HostPorts::run_vault_migrations`, before anything reads the vault
    // (ADR-059). They write files the user owns, so they cannot travel into
    // the compiler crate; the config read below still runs no migrations of
    // its own and still depends on them having happened.

    // H-config (closed 2026-08-25): ONE read+parse of `.moss/config.toml`
    // feeds every `[site]` field below — this span used to be a dozen
    // independent parses of the same file. Still timed so it can be weighed
    // against the lock wait.
    let t_config = std::time::Instant::now();
    let cfg = crate::build::site_config::read_project_config(folder_path).ok();
    crate::build::progress::report_config_version_ahead(progress_sender, cfg.as_ref());
    let site_str =
        |field: &str| cfg.as_ref().and_then(|c| c.site_str(field)).map(str::to_string);
    let site_bool = |field: &str| cfg.as_ref().and_then(|c| c.site_bool(field));

    // ONE resolve for the whole build, off the ONE config parse above (see
    // `site_comments` just below too) — two ladders that could drift is how
    // this presented before, as English email chrome on a Chinese site.
    let site_lang = crate::i18n::detect::resolve_site_default_lang(
        site_str("lang").as_deref(),
        project_structure.homepage_file.as_deref(),
        &project_structure.markdown_files,
        &project_structure.root_path,
        crate::i18n::build_default_language(),
    );
    let site_comments = site_bool("comments");
    // Read site-level config from .moss/config.toml [site] section
    let site_config = crate::build::render::SiteConfig {
        lang: site_lang.clone(),
        typesetting: site_str("typesetting"),
        content_width: site_str("content_width"),
        comments: site_comments,
        // Default-on for everyone — new sites and pre-existing ones alike.
        // The v2→v3 migration is a no-op stamp (does not write the key);
        // absence-as-true means migrated and brand-new sites both get the
        // Pandoc-style implicit-figure rule. Only authors who explicitly
        // write `[site].implicit_figure = false` opt out. Sites that
        // migrated under the *previous* PR (since reverted) still carry
        // a literal `false` value — they remain opted out as a legacy
        // artifact; deleting that line opts back in. See
        // docs/archive/2026-05-05-figure-captions-design.md.
        implicit_figure: site_bool("implicit_figure").unwrap_or(true),
        // Default-on by key absence (ADR-030). Deliberately NOT the
        // implicit_figure story above: nothing has ever written
        // `[site].math`, so there is no legacy value to preserve and no
        // migration involved — absence simply means "the author never
        // said", and the answer to that is yes. Explicit
        // `[site].math = false` is the only way to turn it off.
        math: site_bool("math").unwrap_or(true),
        // Default-on by key absence, same story as `math`: nothing has ever
        // written `[site].hard_line_breaks`, so absence means "the author
        // never said" — and the answer is Obsidian's, because that is where
        // the vault renders while being written. Explicit
        // `[site].hard_line_breaks = false` restores CommonMark.
        hard_line_breaks: site_bool("hard_line_breaks").unwrap_or(true),
        // Default-on by key absence, same story as `math`: nothing has ever
        // written `[site].link_preview`, so absence means "the author never
        // said" — and the answer is yes, the popup ships. Explicit
        // `[site].link_preview = false` turns it off.
        link_preview: site_bool("link_preview").unwrap_or(true),
        // Default-on by key absence, same story as `math`: nothing has ever
        // written `[site].heading_anchors`, so absence means "the author
        // never said" — and the answer is yes, headings get the `#`.
        // Explicit `[site].heading_anchors = false` turns it off.
        heading_anchors: site_bool("heading_anchors").unwrap_or(true),
        // Default-OFF by key absence, unlike `math`: extra reading chrome is
        // asked for, not inherited, so a site that never said anything gets no
        // floating nav (ADR-049 §1 as amended 2026-08-30).
        floating_nav: site_bool("floating_nav").unwrap_or(false),
        site_url_override,
        ai_policy: site_str("ai_policy"),
        // One switch: the per-site Services-tab toggle, absent key = off
        // (graduated out of `experimental.preview_features` 2026-08-31 —
        // ADR-037 "Gating"). Resolved here into the ONE value the nav button
        // and the index emitter both read, so a button can never point at an
        // index that was never built.
        search: site_bool("search").unwrap_or(false),
        // Default-on by key absence, same story as `math`: nothing has ever
        // written `[terms]`, so absence means "the author never said" — and
        // the answer is yes, `author:`/`tags:` values get term pages. Explicit
        // `[terms].author = false` / `[terms].tags = false` turns a dimension
        // off for sites that use those fields as pure metadata.
        terms_author: cfg.as_ref().and_then(|c| c.terms_bool("author")).unwrap_or(true),
        terms_tags: cfg.as_ref().and_then(|c| c.terms_bool("tags")).unwrap_or(true),
        incremental,
    };
    log::debug!(target: "timing", "[build] staging: config_reads (1 parse) took {:?}", t_config.elapsed());
    // Construct PendingManifest seeded with previous_hashes (carry-forward).
    // Passed to generate_blocking_content so Pattern A emits register via ctx.emit.
    // After the call, pending holds the accumulated site_hashes + blocking_keys;
    let mut pending = PendingManifest::new(previous_hashes.clone());
    // `documents` is the parsed page slice (production type `ParsedDocument`,
    // not yet the typed-AST `moss_core::ast::Document`). PR7b (moss#599)
    // threads it back to `build.rs` so native slot generation can read typed
    // feature flags + slot-only files without the previous filesystem rescan
    // hacks (`project_has_inline_subscribe`, `render_footer_pages_from_disk`).
    // When the typed-AST migration completes (Phase 4+), this evolves to
    // `Vec<moss_core::ast::Document>`.
    let (mut site_result, mut background_ctx, documents, carry_verification) = generate_blocking_content(root, project_structure, &stage_dir, services, progress_sender, emit_source_lines, site_config, &mut pending)?;
    log::debug!(target: "timing", "[build] staging: generate_blocking_content: {:?}", build_start.elapsed());

    // Step 3b: Resolve and inject slots into the marked stage BEFORE hash
    // comparison, ship_phase, or manifest seal. The rendered page loop above
    // registers marker-bearing bytes; this phase rewrites those entries so all
    // downstream artifact boundaries see the final HTML bytes.
    let resolved_slots = match slot_resolver {
        Some(resolve) => resolve(&documents, &site_lang, site_comments)?,
        None => ResolvedSlots::empty(),
    };
    // Runs the enhance hook over the stage, re-registers every page it
    // rewrote, and (under `MOSS_INCREMENTAL_VERIFY=1`) settles the carry
    // check against the resulting final bytes. See `build::emit::slots`.
    crate::build::emit::slots::apply_to_stage_and_manifest(
        &paths,
        &stage_dir,
        &resolved_slots,
        &mut pending,
        &mut site_result,
        carry_verification,
    )?;

    // Store plugin + builder fingerprints so they persist to hashes.json.
    // Set on site_result.hashes (used for change detection below) and on
    // `pending`, whose seal is the sole writer of hashes.json.
    log::debug!(target: "timing", "[build] staging: pre_builder_fingerprint: {:?}", build_start.elapsed());
    let current_builder_fingerprint = cache_keys.builder.clone();
    log::debug!(target: "timing", "[build] staging: builder_fingerprint: {:?}", build_start.elapsed());
    site_result.hashes.builder_fingerprint = Some(current_builder_fingerprint.clone());
    // Propagate the fingerprint to `pending` so the SealedManifest produced
    // by the seal+persist side task carries it. Pre-#620 Item 2 the legacy
    // on-disk write at `media/pipeline.rs` serialized `&site_hashes` directly
    // (which had the fingerprint); now the only writer is the seal task, so
    // it must reach `pending` before it sealed.
    pending.set_builder_fingerprint(Some(current_builder_fingerprint.clone()));

    // Step 4: Compare hashes to detect changes
    // For first builds (site_has_content == false), always proceed with copy.
    // For rebuilds, check modified/new/deleted files and the builder fingerprint.
    let builder_changed = match &previous_hashes.builder_fingerprint {
        Some(prev) => *prev != current_builder_fingerprint,
        None => true,
    };
    // Clone the in-memory content hashes for the watcher's refresh decision.
    // Captured here (before any later move of `site_result`) and returned in
    // `PipelineRunOutput::content_hashes` so `do_rebuild_and_notify` never
    // re-reads the asynchronously-persisted hashes.json. See
    // AppState::last_content_hashes.
    let content_hashes_for_watch = site_result.hashes.clone();
    let missing_media_for_publish = site_result.missing_media.clone();
    let new_hashes = &site_result.hashes;
    let has_output_changes = !current_ptr_exists
        || builder_changed
        || !new_hashes.get_changed_files(&previous_hashes).is_empty()
        || !new_hashes.get_new_files(&previous_hashes).is_empty()
        || !new_hashes.get_deleted_files(&previous_hashes).is_empty();

    log::debug!(target: "timing", "[build] staging: hash_compare (changed={}): {:?}", has_output_changes, build_start.elapsed());

    let is_empty = project_structure.total_files == 0;

    // The gate, decided and emitted BEFORE the "complete" progress event.
    //
    // Ordering is load-bearing, not tidiness. "complete" is what drives the
    // frontend to `renderStatus('ready')` and `loadPreview()`, so emitting the
    // gate after it would paint the substitute home page first and drop the
    // waiting screen over it a moment later — a flash of a site the user does
    // not have. Emitted here, the frontend never renders the wrong answer.
    let home_substituted = home_page_is_a_substitute(project_structure, folder_path);
    let home_ready = crate::build::io_utils::output_present(&stage_dir.join("index.html")) && !home_substituted;
    // Raising the gate is a claim about the CLOUD, and `home_substituted` is
    // already exactly that claim: it is true only when a file still up there
    // outranks what we published. `icloud_count > 0` stays as the guard for the
    // other way to miss a home page — no `index.html` at all — because a site
    // that simply failed to produce one is not a cloud story, and the cloud
    // screen would be a lie.
    //
    // Lowering it keys off whether a gate is actually up, NOT off this build's
    // eviction count. The build that finally succeeds is usually the one where
    // nothing is evicted any more, so a symmetric condition would leave the
    // waiting screen up over a finished site forever.
    // The last cancellation check before this build touches app-global state.
    //
    // `SiteDirectoryState` is shared across folders, so `switch_to` below
    // repoints the LIVE preview — whichever folder it currently belongs to. A
    // cloud-arrival rebuild of a large evicted vault runs for minutes; if the
    // user closes that folder and opens another meanwhile, the old build
    // finishing would silently serve folder A's site in folder B's window.
    //
    // Bailing here rather than mid-write is deliberate: everything above this
    // point wrote only into this build's own staging directory, which the next
    // build overwrites anyway.
    if services
        .and_then(|s| s.session.as_ref())
        .is_some_and(|s| s.cancel.is_cancelled())
    {
        log::info!("[build] folder closed during the build — not switching the preview");
        return Ok(PipelineRunOutput {
            is_empty,
            bg_handle: None,
            build_documents: documents,
            content_hashes: content_hashes_for_watch,
            missing_media: missing_media_for_publish,
            cancelled: true,
            home_ready,
            // The folder is closed; nothing downstream should promote this
            // build's generation over whatever the user opens next.
            publishable: false,
            render_seq: None,
        });
    }

    // How much of this site is still in the cloud, as of NOW.
    //
    // `icloud_count` alone is the blindness moss#982 measured: it comes from the
    // source scan, which runs before the build and prunes dot-directories, so it
    // is stale by construction for anything the provider evicted afterwards. On
    // the incident vault it was the reason 578 evicted images could not
    // influence the gate. The ledger is the live half — every read site that
    // classified a cloud failure during THIS build wrote to it — so the two are
    // combined rather than one replacing the other: the scan sees files no read
    // site reached, and the ledger sees evictions the scan predates.
    let cloud_outstanding = icloud_count.max(super::cloud_ledger::outstanding(Path::new(folder_path)));

    // Did this build render without sources it needed — a page, the config, the
    // user stylesheet — because they are still downloading? See
    // `cloud_ledger::structural_missing_count` for why both halves are consulted.
    let still_in_the_cloud_at_scan: Vec<std::path::PathBuf> = project_structure
        .evicted_paths
        .iter()
        .filter(|p| super::icloud::is_still_in_the_cloud(p))
        .cloned()
        .collect();
    let structural_missing = super::cloud_ledger::structural_missing_count(
        &still_in_the_cloud_at_scan,
        super::cloud_ledger::structural_outstanding(Path::new(folder_path)),
    );
    let structural_incomplete = structural_missing > 0;

    // The policy — monotonic, and asking about now rather than about scan time
    // — lives in `cloud_gate_should_hold`, next to its own reasoning and tests.
    let waiting =
        cloud_gate_should_hold(home_ready, current_ptr_exists, cloud_outstanding, structural_incomplete);
    // And whether this build's output may replace what is already served, which
    // is a different question from whether to cover the window — see
    // `should_publish`. Carried out to the seal tail in `PipelineRunOutput`.
    let publishable = should_publish(structural_incomplete);
    if waiting {
        super::cloud_readiness::mark_gated(folder_path);
        emit_cloud_gate(true, &project_structure.evicted_paths, cloud_outstanding);
    } else if super::cloud_readiness::take_gate(folder_path) {
        emit_cloud_gate(false, &project_structure.evicted_paths, cloud_outstanding);
    }

    let render_seq: Option<u64>;
    let announce: Option<std::path::PathBuf>;
    crate::build::phase::record_count("served_staging", usize::from(publishable));
    if has_output_changes {

        // Step 4b: Remove stale HTML pages from deleted source folders.
        //
        // Runs unconditionally, including when this build deferred a page.
        // Each deferred page's published output is reinstated in `blocking_keys`
        // by `PendingManifest::carry_forward_deferred_page` (see the render
        // pass), so it is protected by name rather than by skipping the sweep.
        // `pending.notebook_outputs()` carries background-phase JupyterLite
        // assets (jupyter/**/index.html) forward from the previous build, so
        // stale-html cleanup here doesn't delete them before
        // `run_notebook_processing` below re-registers them.
        if let Some(permit) = sweep_permit.as_ref() {
            remove_stale_html(&stage_dir, &background_ctx.blocking_keys, pending.notebook_outputs(), permit);
        }
        // gen_dir is immutable post-materialize — stale-html cleanup runs on staging only.

        // Step 5: show this render — unless this build could not read its own
        // sources. Showing it is the moment the user sees this build's output,
        // so an unpublishable build must not (moss#1042).
        let (seq, announced) = crate::build::lifecycle::show_render(&paths, publishable);
        render_seq = Some(seq);
        announce = announced;
        if !publishable {
            // The count the decision was made from, not a fresh read of the
            // ledger: this line explains a choice already taken, and asking
            // again here is how it came to report zero of the thing it was
            // withholding for (moss#1061).
            log::info!(
                "[cloud] not switching the preview to this build — {} structural source(s) are \
                 still downloading, so it would replace the served site with one rendered without \
                 them. The arrival of any of them rebuilds.",
                structural_missing
            );
        }

        log::debug!(target: "timing", "[build] staging: switch_to_stage: {:?}", build_start.elapsed());

        // The cloud-sync gate event is emitted once, at the end of the build,
        // from the build's own output — see `emit_cloud_gate`.

        // Asked HERE, not carried in. A build that started no server of its own
        // may have had one come up while it ran — on the incident vault, fourteen
        // seconds after this build was dispatched and nineteen seconds before it
        // reached this line. See `ports::LivePortResolver` (moss#1061).
        send_progress(progress_sender, "complete", &crate::infra::app_advisory::t("build_complete"), 100, true, preview_port(), Some(is_empty));

        // Server stays on site-stage/ (step 5) — no switch back to site/.
        // site-stage/ has data-source-line annotations for editor↔preview scroll sync.

        // Emit 'initial-build-complete' with the directory actually being
        // served — staging normally, the sealed generation when this build was
        // withheld.
        emit_initial_build_complete(services, announce.as_deref());

    } else {
        // No output changes — staging still holds this render; show it the same
        // way, under the same publishable guard.
        let (seq, announced) = crate::build::lifecycle::show_render(&paths, publishable);
        render_seq = Some(seq);
        announce = announced;

        // Asked HERE, not carried in. A build that started no server of its own
        // may have had one come up while it ran — on the incident vault, fourteen
        // seconds after this build was dispatched and nineteen seconds before it
        // reached this line. See `ports::LivePortResolver` (moss#1061).
        send_progress(progress_sender, "complete", &crate::infra::app_advisory::t("build_complete"), 100, true, preview_port(), Some(is_empty));

        emit_initial_build_complete(services, announce.as_deref());
    }
    if let Some(seq) = render_seq {
        crate::build::phase::record_count("render", seq as usize);
    }

    // Step 6c: no search worker. Pagefind is generation-free (ADR-045) — cost
    // scales with corpus, not delta — so it runs on its own lane, adopted below.
    let search_enabled = background_ctx.search_enabled;

    // === BACKGROUND PHASE (spawn, don't block) ===
    // ADR-001: Background work runs after preview opens.
    // Notebook processing runs BEFORE background asset dispatch so its
    // generated paths are registered before stale cleanup could remove them.
    let notebook_files = std::mem::take(&mut background_ctx.notebook_files);
    if !notebook_files.is_empty() {
        // Cancellation flag: bridged from `FolderSession::cancel` to a local
        // AtomicBool so the existing notebook cancel-check signature is
        // preserved. In headless mode (no session) the flag stays false.
        let local_cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        if let Some(s) = services.and_then(|s| s.session.as_ref()) {
            s.bridge_to_atomic(local_cancel.clone());
        }
        let cancel: Option<&std::sync::atomic::AtomicBool> = Some(&*local_cancel);
        let notebook_outputs = run_notebook_processing(
            services,
            &notebook_files,
            &background_ctx.source_path,
            &background_ctx.staging_dir,
            cancel,
        );
        // Register the receipts. `pending` is still alive here (constructed in
        // build_inner before generate_blocking_content and passed as &mut); the
        // coordinator takes ownership of it below.
        //
        // There is no read here. This loop used to `std::fs::read` every
        // registered output back to hash it — which is where the CPHS vault
        // died: the walk above had laundered 382 Dropbox conflicted-copy twins
        // into the output list, the twins were cloud-evicted, and reading one
        // returns EDEADLK under the dataless fail-fast policy. That stop
        // classified as Report, so it was fatal, and because `build_folder`
        // applies `?` before `start_file_watching` there was no watcher left to
        // retry. Nothing under `.moss/build/` needs to be read to find out what
        // moss just wrote there: the writer already knew.
        for (out_path, hash) in &notebook_outputs {
            pending.register_hashed(out_path, hash, crate::build::manifest::HashBucket::NotebookOutputs);
        }

        // A cancelled run returns an empty vec, which the loop above would
        // otherwise seal as "this site has no notebooks any more" while the
        // bundle is still sitting in staging. See the method's doc.
        if local_cancel.load(std::sync::atomic::Ordering::SeqCst) {
            log::info!("Notebook run cancelled; carrying forward the previous notebook outputs");
            pending.carry_forward_notebook_outputs(&background_ctx.previous_hashes);
        }
    }

    // Adopt the search bundle from the lane's receipt (ADR-045): inside the
    // stage-write span (it lays files into `stage_dir`) and before the seal.
    // Mark-and-sweep — skipping it deletes `_moss/pagefind/*` off the live site.
    let search_paths = MossPaths::new(std::path::Path::new(folder_path));
    let adoption = crate::build::feeds::search_lane::adopt_into(
        &search_paths, &stage_dir, &mut pending, search_enabled, search_freshness);
    log::debug!(target: "search", "search bundle: {:?}", adoption);

    // Seal-persist-race-404 fix: release the stage-write guard now — this
    // build's own synchronous stage-writing span (generate_blocking_content
    // through the notebook-processing block just above, which itself writes
    // into `stage_dir` via `run_notebook_processing`)
    // is complete. Everything from here on (image/video/asset dispatch) is
    // genuinely-async background work, sequenced before THIS build's own
    // seal task (which awaits `await_completion` before touching
    // `stage_dir` again) — no other build's seal-tail can be in that window
    // yet. Releasing here (rather than holding through the background
    // phase) keeps the guard scoped to the fast synchronous path, matching
    // how quickly the watcher's `is_rebuilding` lock already releases today
    // (it covers this entire synchronous span, including notebook
    // processing, but not the detached background dispatch below).
    drop(_stage_write_guard);

    // Subscribe landing pages moved to generate_blocking_content (Task 5 of moss#524).

    // === BackgroundHandle dispatch ===
    // Spawn all three background dispatches into a single BackgroundHandle.
    // The coordinator accumulates registrations from all three workers and
    // seals a SealedManifest when all workers complete. The handle is returned
    // to the caller, who stores it in AppState until deploy (Task 9).
    //
    // Carry-forward: pass `pending` (render-phase manifest) to the coordinator
    // so deferred-phase emits accumulate on top of render-phase state, producing
    // a single SealedManifest that covers both phases.

    // No gate: every build has at least the asset walk, so there is always a
    // worker and always a seal. The zero-worker branch that used to sit beside
    // this — reachable only when `deferred` was `None`, which
    // `generate_blocking_content` never produced — went with `DeferredWork`
    // (moss#618). Search dispatches no worker of its own (ADR-045); its bundle
    // is adopted from a receipt above.
    // Post-#620 Item 2 invariant: build_inner runs inside a Tokio runtime.
    // The legacy `tokio::runtime::Handle::try_current().is_ok()` fallback to
    // a synchronous dispatch path has been removed; existing tests that
    // exercise this code now run with `#[tokio::test]` (or are gated to the
    // no-background-work branch below).
    debug_assert!(
        tokio::runtime::Handle::try_current().is_ok(),
        "build_inner must run inside a tokio runtime; #[test] callers must \
         migrate to #[tokio::test]"
    );

    let bg_handle = {
        // Clone what we need before consuming background_ctx by video dispatch.
        let image_items_empty = background_ctx.image_items.is_empty();
        let video_items_empty = background_ctx.video_items.is_empty();
        // The asset walk reads the same context the image and video workers do;
        // clone it here, before the video dispatch below consumes the original.
        let ctx_for_assets = background_ctx.clone();

        // Step 3 Phase 4 (review fix). The parent Build Job is NO LONGER spawned
        // eagerly here — `collect_videos_for_conversion` returns every .mov
        // regardless of the transform cache, so a non-empty `video_items` is NOT
        // evidence of real work (a content-only rebuild on a video site cache-skips
        // every video). Instead the parent is minted LAZILY: the first media worker
        // that does real work (a conversion or an advisory) calls
        // `BuildServices::ensure_build_parent`, which spawns the parent once and
        // caches its id on the shared `build_parent` cell. A content-only rebuild
        // never reaches that → no parent → zero Jobs (invariant #6 / the heartbeat).
        //
        // Stash the render-phase `html_total` (= `documents.len()`, the html_pages
        // task total) into the shared atomic so the post-join barrier sources the
        // Build receipt's `Amount { count, "pages" }` from the SAME count the legacy
        // media-less frontend receipt reports (FIX 3 — page-count consistency). The
        // narrower `SiteResult.page_count` would diverge with vs without media.
        let html_total = documents.len();
        if let Some(s) = services {
            s.page_count
                .store(html_total, std::sync::atomic::Ordering::Relaxed);
        }

        // Owned Arc<BuildServices> handles for the 'static worker closures.
        // `BuildServices::clone` is Arc-clone all the way down, so every handle
        // shares the SAME `build_parent` cell — whichever worker mints the
        // parent first is observed by the others and by the post-join barrier.
        let svc_arc_for_image: Option<std::sync::Arc<BuildServices>> =
            services.map(|s| std::sync::Arc::new(s.clone()));
        let svc_arc_for_video = svc_arc_for_image.clone();

        // The post-join barrier owns EVERY terminal receipt this build owes: the
        // (lazily-minted) parent Job's terminal state and the `BuildComplete`
        // event. A SINGLE point after every media worker joins — images-only,
        // images-finish-last and epoch-supersede all covered. Its Drop guard
        // cancels a leaked Running parent on any non-await path. Attached
        // whenever there are services at all; each half no-ops on its own when
        // its channel is absent (headless drives no Job and has no event sink).
        // Uses `background_ctx.start_time` so the receipts' span matches the
        // workers' `total_time_ms`.
        let terminal_barrier = crate::build::background::BuildTerminalBarrier::from_services(
            services,
            background_ctx.start_time,
        );
        // For the assets worker we need the reporter and the session
        // separately — copy_deferred_assets takes the reporter by ref.
        // Issue #697: pass the AssetRegistry so static assets (audio/PDF/etc.)
        // are registered as Ready after copy, enabling editor hover-preview.
        let reporter_for_assets = services.map(|s| s.reporter.clone());
        let session_for_assets = services.and_then(|s| s.session.clone());
        let asset_registry_for_assets = services.and_then(|s| s.assets.clone());
        // Shared counter: the assets worker writes skipped_symlinks after
        // copy_deferred_assets; the video worker reads it when emitting BuildComplete.
        let skipped_symlinks_for_assets = services.map(|s| s.skipped_symlinks.clone());

        let register_workers = |tx: tokio::sync::mpsc::Sender<crate::build::coordinator::EmitMessage>,
                                workers: &mut tokio::task::JoinSet<Result<(), crate::build::background::BuildError>>| {
            // Worker 1: Image conversion.
            // dispatch_image_conversions internally spawns its own async tasks in GUI
            // mode (via tauri::async_runtime::spawn) and runs synchronously in headless
            // mode. The worker just calls it and returns; the tx clone lives inside the
            // internally-spawned task until run_image_conversion completes.
            if !image_items_empty {
                let ctx_for_image = background_ctx.clone();
                let tx_image = tx.clone();
                workers.spawn(async move {
                    tokio::task::spawn_blocking(move || {
                        dispatch_image_conversions(
                            svc_arc_for_image.as_deref(),
                            &ctx_for_image,
                            Some(tx_image),
                        );
                    })
                    .await
                    .map_err(crate::build::background::BuildError::from)
                });
            }

            // Worker 2: Video conversion (consumes background_ctx).
            // Same dispatch pattern as image: internally spawns or runs sync.
            if !video_items_empty {
                let tx_video = tx.clone();
                workers.spawn(async move {
                    tokio::task::spawn_blocking(move || {
                        dispatch_video_conversions(
                            svc_arc_for_video.as_deref(),
                            background_ctx,
                            Some(tx_video),
                        );
                    })
                    .await
                    .map_err(crate::build::background::BuildError::from)
                });
            }

            // Worker 3: Background asset copy.
            // copy_deferred_assets runs synchronously (no internal spawn).
            {
                let tx_assets = tx.clone();
                if let Some(ref s) = session_for_assets {
                    s.begin_ui_bound();
                }
                workers.spawn(async move {
                    tokio::task::spawn_blocking(move || {
                        let reporter_for_assets = reporter_for_assets
                            .as_deref()
                            .unwrap_or(crate::build::ports::reporter::discarding());
                        let stats = crate::build::media::pipeline::copy_deferred_assets(
                            &ctx_for_assets,
                            reporter_for_assets,
                            tx_assets,
                            asset_registry_for_assets,
                        );
                        // Publish the skipped-symlink count so the video worker
                        // (which emits BuildComplete) can include it in the event.
                        if let Some(counter) = skipped_symlinks_for_assets {
                            counter.fetch_add(
                                stats.skipped_symlinks,
                                std::sync::atomic::Ordering::Relaxed,
                            );
                        }
                        // Task 2.2: emit a BackgroundProgress advisory when symlinks
                        // were skipped — surfaces on the L1 hairline dot WITHOUT a toast.
                        if let Some(ev) = crate::build::progress::make_symlink_skip_advisory(stats.skipped_symlinks) {
                            reporter_for_assets.report(&ev);
                        }
                        if let Some(s) = session_for_assets {
                            s.end_ui_bound();
                        }
                    })
                    .await
                    .map_err(crate::build::background::BuildError::from)
                });
            }
            // No search worker (ADR-045): a generation-free worker is never
            // awaited by a seal, so the previous build's index cannot land on
            // this build's critical path. Its output is adopted from a receipt.

            // tx dropped here → coordinator drains only worker clones
        };
        let handle = BackgroundHandle::spawn_with_pending_and_terminal(
            pending,
            terminal_barrier,
            cache_lease,
            register_workers,
        );
        Some(handle)
    };

    log::debug!(target: "timing", "[build] total: {:?}", build_start.elapsed());

    // The gate was decided and emitted above, before "complete" — see the
    // `home_substituted` block. `home_ready` is carried down to here only to be
    // reported to the caller.

    Ok(PipelineRunOutput {
        is_empty,
        bg_handle,
        build_documents: documents,
        content_hashes: content_hashes_for_watch,
        missing_media: missing_media_for_publish,
        cancelled: false,
        home_ready,
        publishable,
        render_seq,
    })
}

/// Helper to send Tier-1 progress updates (no-op when nothing is listening).
///
/// `None` means Tier-1 has nowhere to go — the CLI and test builds — not that
/// there is no reporter. See `build::tier1_reporter`.
pub(crate) fn send_progress(
    reporter: Option<&dyn crate::build::ports::reporter::BuildReporter>,
    step: &str,
    msg: &str,
    pct: u8,
    done: bool,
    port: Option<u16>,
    empty: Option<bool>,
) {
    if let Some(r) = reporter {
        r.report(&super::progress::PipelineEvent::Progress {
            step: step.to_string(),
            message: msg.to_string(),
            percentage: pct,
            completed: done,
            port,
            is_empty: empty,
        });
    }
}

/// Unlink whatever the previous build's manifest does not name: variants the
/// orphan prune condemned, entries the presence pass dropped, outputs of pages
/// that no longer exist, `.pending.*` litter from an interrupted write.
///
/// Staging is the tree the preview reads, so this needs a
/// `lifecycle::SweepPermit`. Without one it does nothing: the leftovers cost
/// local disk until the next caught-up build, never a generation, because
/// `ship_phase` copies `sealed.files()` and nothing else. A one-shot build
/// never gets a next build, so `ship::reclaim_staging_now` is its last chance.
///
/// An empty previous manifest (first build, or one that could not be read)
/// authorizes nothing either.
fn sweep_staging(
    stage_dir: &Path,
    previous: &SiteHashes,
    permit: Option<&crate::build::lifecycle::SweepPermit>,
) {
    let Some(permit) = permit.filter(|_| !previous.files.is_empty()) else {
        return;
    };
    let files = remove_stale_files(stage_dir, previous, "staging", permit);
    let dirs = remove_stale_dirs(stage_dir, &compute_expected_dirs(previous), permit);
    crate::build::phase::record_count("swept", files.removed());
    if files.removed() > 0 || dirs > 0 {
        log::info!("staging sweep: {}, {} dir(s)", files.describe(), dirs);
    }
}

/// Load previous hashes from .moss/build/hashes.json.
///
/// On load, runs `migrate_to_normalized_paths` to lowercase any keys that
/// predate the ServedPath chokepoint. This is a one-time, opportunistic
/// migration: subsequent builds re-derive state, so the migration is
/// self-healing — no version field needed in hashes.json.
///
/// See `docs/archive/2026-05-07-output-path-normalization.md` for the rationale.
pub fn load_previous_hashes(folder_path: &str) -> SiteHashes {
    let paths = MossPaths::new(Path::new(folder_path));
    let hashes_path = paths.hashes();
    let mut hashes: SiteHashes = if let Ok(content) = fs::read_to_string(&hashes_path) {
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        SiteHashes::default()
    };
    migrate_to_normalized_paths(&mut hashes);
    hashes
}

/// Normalize all path-typed entries in `hashes` so they match what
/// post-PR builds emit. Existing sites built with older moss versions
/// have capital-cased entries (e.g., `Resources/habitable-zone.html`)
/// that diverge from what the new build pipeline produces.
///
/// Migrated:
/// - keys of `files: HashMap<String, String>`
/// - elements of `image_outputs`, `video_outputs`, `notebook_outputs` sets
/// - **values** of `source_to_output: HashMap<String, String>` (output paths;
///   keys legitimately keep source case as they look up output by source)
///
/// NOT migrated:
/// - `blocking_keys` — that field lives on `PendingManifest`, not `SiteHashes`,
///   and is not persisted to `hashes.json`.
/// - `sources` — keys are source paths (legitimately keep source case);
///   values are `SourceMetadata` not paths.
///
/// Collisions resolve by last-writer-wins via natural HashMap insert
/// semantics. The next build re-derives state, so non-deterministic
/// collision handling does not propagate.
fn migrate_to_normalized_paths(hashes: &mut SiteHashes) {
    use crate::build::served_path::ServedPath;
    use std::collections::{HashMap, HashSet};

    fn migrate_map_keys<V: Clone>(map: &mut HashMap<String, V>) -> usize {
        let mut migrated = 0;
        let keys: Vec<String> = map.keys().cloned().collect();
        for k in keys {
            // Generated artifact paths (e.g. _moss/og/..., _moss/style.css) will fail
            // from_source validation — they don't need directory-slug migration since
            // their paths are fixed by named constructors. Skip them.
            let normalized = match ServedPath::from_source(&k) {
                Ok(sp) => sp.into_string(),
                Err(_) => continue,
            };
            if normalized != k {
                if let Some(v) = map.remove(&k) {
                    map.insert(normalized, v);
                    migrated += 1;
                }
            }
        }
        migrated
    }

    fn migrate_set(set: &mut HashSet<String>) -> usize {
        let mut migrated = 0;
        let items: Vec<String> = set.iter().cloned().collect();
        for k in items {
            // Generated artifact paths (e.g. _moss/og/...) will fail from_source
            // validation — skip them; they don't need directory-slug migration.
            let normalized = match ServedPath::from_source(&k) {
                Ok(sp) => sp.into_string(),
                Err(_) => continue,
            };
            if normalized != k {
                set.remove(&k);
                set.insert(normalized);
                migrated += 1;
            }
        }
        migrated
    }

    fn migrate_map_values(map: &mut HashMap<String, String>) -> usize {
        let mut migrated = 0;
        for v in map.values_mut() {
            // Generated artifact paths will fail from_source — skip them.
            let normalized = match ServedPath::from_source(v) {
                Ok(sp) => sp.into_string(),
                Err(_) => continue,
            };
            if normalized != *v {
                *v = normalized;
                migrated += 1;
            }
        }
        migrated
    }

    let total = migrate_map_keys(&mut hashes.files)
        + migrate_set(&mut hashes.image_outputs)
        + migrate_set(&mut hashes.video_outputs)
        + migrate_set(&mut hashes.notebook_outputs)
        + migrate_map_values(&mut hashes.source_to_output);

    if total > 0 {
        log::info!(
            "[hashes.json] Migrated {} non-normalized entries to slug form (one-time)",
            total
        );
    }
}

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod tests;
