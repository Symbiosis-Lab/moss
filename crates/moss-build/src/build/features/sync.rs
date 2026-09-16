//! Background data sync for native features (comments, reviews).
//!
//! ## Why this exists
//!
//! The build pipeline has one critical-path responsibility: turn source files
//! into HTML in the output directory. Network-dependent data (comments fetched
//! from a remote server, review metadata fetched from external URLs) MUST NOT
//! sit on that critical path. If the remote is slow, hung, or unreachable, the
//! build must still complete and the file watcher must still start.
//!
//! ## How
//!
//! `spawn_native_process_sync` returns immediately after spawning a background
//! Tokio task. The task does the slow network work and writes its output to the
//! site's `.moss/data/social/comment.json` and `.moss/data/social/review.json`.
//! The NEXT build's enhance phase reads whatever files currently exist on disk.
//!
//! Tradeoff: the first time a user opens a site, comments are missing from the
//! preview until the second build (the enhance phase has no cached file to
//! read). This is the right tradeoff — a 90-minute hang is much worse than a
//! one-cycle stale comments display.
//!
//! ## Per-folder singleflight
//!
//! Rapid rebuilds (e.g. file watcher firing multiple times in quick succession)
//! must not pile up overlapping fetch tasks. We track the in-flight task per
//! folder; if a sync is already running for a folder, new requests are dropped.
//!
//! ## See also
//!
//! - [build.rs](../build.rs) for where this is invoked.
//! - [comment.rs](comment.rs) and [review.rs](review.rs) for the sync work itself.
//! - moss issue #570 for the bug this prevents.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::build::features::ArticleInfo;
use crate::config::services::ServicesConfig;
use crate::config::deployment::DomainDeploymentConfig;

/// Why a comment sync failed, classified where the typed transport error is
/// still available (`fetch_artalk_comments`). Drives the localized reason
/// phrase on both surfaces (Services row + advisory window) — the raw error
/// string is log/debug material only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum CommentSyncErrorKind {
    /// Transport-level failure (DNS, connect, timeout): from this machine's
    /// point of view the server can't be reached — most commonly no network.
    Offline,
    /// The server answered but with an error (HTTP status, malformed body).
    Server,
    /// A local problem (corrupt/iCloud-dataless archive, I/O, serialization)
    /// or an internal guard. Rendered cause-neutrally — blaming the network
    /// or the server here would misdirect the user away from the real cause.
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct CommentSyncError {
    pub kind: CommentSyncErrorKind,
    pub detail: String,
}

/// String errors reach this via `?` only from local/internal paths (archive
/// guard, I/O, serialization, sync guards) — the network call sites construct
/// classified errors directly.
impl From<String> for CommentSyncError {
    fn from(detail: String) -> Self {
        CommentSyncError { kind: CommentSyncErrorKind::Internal, detail }
    }
}

/// Last comment-sync outcome, persisted across sessions so the Services row
/// can say "Last synced 2d ago — no internet connection" on mount instead of
/// waiting for a live event. Timestamps are unix seconds as f64 (specta
/// exports u64/i64 as TS strings; f64 is the codebase's timestamp idiom).
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct CommentSyncState {
    pub last_success_at: Option<f64>,
    pub last_attempt_at: Option<f64>,
    pub last_error: Option<CommentSyncError>,
}

/// State file location. Under `.moss/data/` but NOT `.moss/data/social/`:
/// social/ is on the watcher allowlist (comment.json rewrites deliberately
/// trigger a refresh build), and this file changes on EVERY sync attempt —
/// inside social/ it would rebuild-loop. Same placement rationale as the
/// `.moss/deploy/` snapshots. Pinned by `sync_state_lives_outside_watched_paths`.
fn comment_sync_state_path(folder: &Path) -> PathBuf {
    folder.join(".moss").join("data").join("comment-sync-state.json")
}

/// Load the persisted sync state. Missing or unparseable file = default
/// (never synced) — the file is moss-written advisory state, not user data,
/// so silent fresh-start is the right recovery.
pub fn load_comment_sync_state(folder_path: &str) -> CommentSyncState {
    let path = comment_sync_state_path(Path::new(folder_path));
    match std::fs::read_to_string(&path) {
        Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
        Err(_) => CommentSyncState::default(),
    }
}

fn store_comment_sync_state(folder_path: &str, state: &CommentSyncState) {
    let path = comment_sync_state_path(Path::new(folder_path));
    let write = || -> Result<(), String> {
        let parent = path.parent().expect("state path always has a parent");
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let json = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
        std::fs::write(&path, json).map_err(|e| e.to_string())  // allow:raw_write user state under .moss/data, not regenerable output
    };
    if let Err(e) = write() {
        // Advisory state only — losing a write degrades the settings row's
        // status line, never the sync itself. Log and move on.
        log::warn!(target: "sync", "could not persist comment-sync state: {e}");
    }
}

fn record_comment_sync_success(folder_path: &str, at: f64) {
    let mut state = load_comment_sync_state(folder_path);
    state.last_success_at = Some(at);
    state.last_attempt_at = Some(at);
    state.last_error = None;
    store_comment_sync_state(folder_path, &state);
}

fn record_comment_sync_failure(folder_path: &str, at: f64, err: &CommentSyncError) {
    let mut state = load_comment_sync_state(folder_path);
    state.last_attempt_at = Some(at);
    state.last_error = Some(err.clone());
    store_comment_sync_state(folder_path, &state);
}

fn unix_now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Tracks folders currently being synced so we don't pile up overlapping tasks.
///
/// A folder is added on `spawn_native_process_sync` and removed when the task
/// finishes. If `try_acquire` returns `false`, a sync is already running and
/// this request is dropped (next rebuild will try again).
fn inflight() -> &'static Mutex<HashSet<String>> {
    use std::sync::OnceLock;
    static INFLIGHT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    INFLIGHT.get_or_init(|| Mutex::new(HashSet::new()))
}

fn try_acquire(folder: &str) -> bool {
    let mut guard = match inflight().lock() {
        Ok(g) => g,
        // Lock poisoned (a previous sync panicked while holding it). Recover by
        // clearing — a stale entry would otherwise block all future syncs.
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.insert(folder.to_string())
}

fn release(folder: &str) {
    if let Ok(mut guard) = inflight().lock() {
        guard.remove(folder);
    }
}

/// Advisory surfaced when the background comment sync fails. Covers both
/// network failures (server unreachable) and local I/O / archive-clobber
/// guard errors (corrupt or iCloud-dataless file). Remote scope: the local
/// build shipped fine; the baked comments may just be stale.
///
/// Shows the classified localized reason, not the raw error — the Services
/// row and this advisory must tell the same story per locale (the raw
/// transport string is English-only and lands in the log instead).
fn comment_sync_failure_advisory(err: &CommentSyncError) -> crate::advisory::Advisory {
    let reason_key = match err.kind {
        CommentSyncErrorKind::Offline => "sync_reason_offline",
        CommentSyncErrorKind::Server => "sync_reason_server",
        CommentSyncErrorKind::Internal => "sync_reason_internal",
    };
    let reason = crate::infra::app_advisory::t(reason_key);
    crate::advisory::Advisory {
        scope: crate::advisory::Scope::Remote,
        severity: crate::advisory::Severity::NeedsAction,
        item: None,
        what: crate::infra::app_advisory::fmt("comment_sync_failed", &[("err", &reason)]),
        action: crate::advisory::Action::None,
    }
}

/// Spawn a background task that refreshes comment + review caches for `folder`.
/// Returns immediately. Does NOT block the build pipeline on network I/O.
///
/// Called from `run_pipeline` AFTER the build has produced the latest
/// article-map but BEFORE returning. The next build cycle will pick up
/// whatever this task has written by then.
///
/// No-op when:
/// - A sync for this folder is already in-flight (singleflight)
/// - `article-map.json` does not yet exist (first build)
/// - The site has no comments or review services configured
pub fn spawn_native_process_sync(
    folder_path: String,
    services_config: ServicesConfig,
    domain_config: DomainDeploymentConfig,
    reporter: std::sync::Arc<dyn crate::build::ports::reporter::BuildReporter>,
    spawner: &dyn crate::build::ports::spawner::Spawner,
) {
    if !try_acquire(&folder_path) {
        log::debug!(target: "sync", "skipping native-process sync — another sync is in flight for {}", folder_path);
        return;
    }

    // Fire-and-forget: the completion event is the only thing anyone waits for,
    // and it goes out through the reporter.
    let _ = spawner.spawn(Box::pin(async move {
        let started = Instant::now();
        let result = tokio::task::spawn_blocking({
            let folder_path = folder_path.clone();
            move || run_native_process_sync(&folder_path, &services_config, &domain_config)
        }).await;
        release(&folder_path);

        match result {
            Ok((stats, advisories)) => {
                log::info!(
                    target: "sync",
                    "native-process sync done in {:?} (reviews={}, comments_changed={}, link_meta={})",
                    started.elapsed(), stats.reviews_fetched, stats.comments_changed, stats.link_meta_prewarmed
                );
                // Always emit the completion event (success or failure) so the
                // frontend (Services > Comments) can update its "last synced"
                // status line regardless of outcome. Empty advisories = success.
                reporter.report(&crate::build::progress::PipelineEvent::BackgroundProgress {
                    task: "comment-sync".to_string(),
                    current: 1,
                    total: 1,
                    message: "comment-sync (1/1)".to_string(),
                    completed: true,
                    advisories,
                });
            }
            Err(e) => log::warn!(target: "sync", "native-process task panicked: {:?}", e),
        }
    }));
}

#[derive(Default)]
struct SyncStats {
    reviews_fetched: usize,
    comments_changed: usize,
    link_meta_prewarmed: usize,
}

/// Body of the background sync. Runs on a `spawn_blocking` worker; the network
/// calls inside are synchronous (`ureq`) and that's fine here because the
/// blocking pool exists for exactly this use case.
///
/// Returns `(stats, advisories)` — advisories are non-empty only when the
/// comment sync fails. The caller emits them as a `BackgroundProgress` event.
fn run_native_process_sync(
    folder_path: &str,
    services_config: &ServicesConfig,
    domain_config: &DomainDeploymentConfig,
) -> (SyncStats, Vec<crate::advisory::Advisory>) {
    let mut stats = SyncStats::default();
    let mut advisories: Vec<crate::advisory::Advisory> = Vec::new();

    let article_map_path = Path::new(folder_path)
        .join(".moss")
        .join("build")
        .join("article-map.json");
    if !article_map_path.exists() {
        log::debug!(target: "sync", "no article-map.json yet — nothing to sync");
        return (stats, advisories);
    }
    let article_map = crate::build::load_article_map_for_features(&article_map_path);

    // Review: filter for articles with review_of URLs
    let review_inputs: Vec<super::review::ReviewArticleInput> = article_map
        .iter()
        .filter_map(|(uid, info): (&String, &ArticleInfo)| {
            info.review_of.as_ref().map(|review_of| super::review::ReviewArticleInput {
                uid: uid.clone(),
                review_of: review_of.clone(),
                rating: info.rating,
            })
        })
        .collect();
    if !review_inputs.is_empty() {
        let phase_start = Instant::now();
        match super::review::process_reviews(folder_path, &review_inputs) {
            Ok(n) => {
                stats.reviews_fetched = n;
                if n > 0 {
                    log::info!(target: "sync", "reviews: fetched metadata for {} articles in {:?}", n, phase_start.elapsed());
                }
            }
            Err(e) => log::warn!(target: "sync", "reviews fetch failed after {:?}: {}", phase_start.elapsed(), e),
        }
    }

    // Comments: gate on resolved server URL (explicit self-hosted override →
    // env-derived moss-operated server → empty = never deployed / skip).
    let env = crate::build::site_config::resolve_environment(folder_path);
    let comments_server = services_config
        .comments
        .as_ref()
        .filter(|c| c.common.enabled != Some(false))
        .map(|c| {
            c.resolved_server_url(
                domain_config.domain.as_deref(),
                domain_config.site_id.as_deref(),
                &env,
            )
        })
        .filter(|u| !u.is_empty());
    if let Some(server_url) = comments_server {
        // Sync must use the same Artalk site_name as render/submission.
        // For moss-hosted sites, that's the deployed site_id; undeployed custom
        // server_url setups still fall back to "localhost".
        let site_name = domain_config
            .site_id
            .as_deref()
            .unwrap_or("localhost");
        let comment_articles: Vec<(String, String)> = article_map
            .iter()
            .map(|(uid, info)| (uid.clone(), info.url_path.clone()))
            .collect();
        if !comment_articles.is_empty() {
            let phase_start = Instant::now();
            // Hard deadline: even though each request has its own ureq timeout,
            // the loop iterates per-article and could pile up. Bound the total
            // wall-clock time with a separate watchdog thread that logs a loud
            // warning if we blow past the budget.
            let watchdog = spawn_watchdog(
                "comment fetch",
                Duration::from_secs(60),
            );
            match super::comment::process_comments(
                folder_path,
                &server_url,
                site_name,
                &comment_articles,
            ) {
                Ok(outcome) => {
                    stats.comments_changed = outcome.changed as usize;
                    record_comment_sync_success(folder_path, unix_now_secs());
                    log::info!(target: "sync", "comments: synced {} articles against {} in {:?} (changed={})", comment_articles.len(), server_url, phase_start.elapsed(), outcome.changed);
                }
                Err(e) => {
                    log::warn!(target: "sync", "comment sync failed ({:?}): {}", e.kind, e.detail);
                    record_comment_sync_failure(folder_path, unix_now_secs(), &e);
                    advisories.push(comment_sync_failure_advisory(&e));
                }
            }
            watchdog.cancel();
        }
    }

    // Link metadata prewarm: refresh the cache for any external URLs the most
    // recent build needed (persisted at .moss/build/link-meta-urls.json).
    // Render reads from cache only — this is what makes those reads fast and
    // keeps the build off the network.
    let moss_dir = Path::new(folder_path).join(".moss");
    let url_list = crate::build::page::link_meta::load_persisted_url_list(&moss_dir);
    if !url_list.is_empty() {
        let phase_start = Instant::now();
        // Reuse the parallel fetcher so GUI and CLI prewarm follow the same
        // path (parallelism + per-URL freshness skip + budget). The fetcher
        // has its own internal budget, but we keep this watchdog as a belt-
        // and-braces signal in case the bound is breached.
        let watchdog = spawn_watchdog("link-meta prewarm", Duration::from_secs(120));
        let url_refs: Vec<&str> = url_list.iter().map(|s| s.as_str()).collect();
        let fetched = crate::build::page::link_meta::fetch_all_link_meta_parallel(&url_refs, &moss_dir);
        watchdog.cancel();
        stats.link_meta_prewarmed = url_list.len();
        log::info!(target: "sync", "link-meta: prewarmed {} URLs ({} fetched, rest fresh) in {:?}", url_list.len(), fetched, phase_start.elapsed());
    }

    (stats, advisories)
}

/// Lightweight watchdog: if `budget` elapses before `cancel` is called, log a
/// loud warning naming the slow operation. Does not interrupt — just makes the
/// stall visible. Safer than killing the worker mid-write; the upstream
/// HTTP-client timeout bug should be fixed independently.
struct Watchdog {
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Watchdog {
    fn cancel(self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

fn spawn_watchdog(label: &'static str, budget: Duration) -> Watchdog {
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cancelled_clone = cancelled.clone();
    std::thread::spawn(move || {
        let started = Instant::now();
        while started.elapsed() < budget {
            if cancelled_clone.load(std::sync::atomic::Ordering::SeqCst) {
                return;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        if !cancelled_clone.load(std::sync::atomic::Ordering::SeqCst) {
            log::warn!(
                target: "sync",
                "watchdog: '{}' is still running after {:?} — likely a slow/unreachable upstream. \
                 Build is unaffected; this sync is in the background.",
                label,
                budget
            );
        }
    });
    Watchdog { cancelled }
}

// The two `#[tauri::command]` wrappers over this module — `trigger_comment_sync`
// and `get_comment_sync_state` — live in `crate::channels::comments`. They are
// GUI surface, and this file is scheduled to become `moss-build channels/`,
// which cannot hold a `#[tauri::command]`. The work itself stays here.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_sync_failure_advisory_shape() {
        let adv = comment_sync_failure_advisory(&CommentSyncError {
            kind: CommentSyncErrorKind::Offline,
            detail: "comment server unreachable: timeout".into(),
        });
        assert!(matches!(adv.severity, crate::advisory::Severity::NeedsAction));
        assert!(matches!(adv.scope, crate::advisory::Scope::Remote));
        assert!(adv.what.contains("comments may be stale"));
        // The advisory shows the classified localized reason, never the raw
        // transport error string (which is English-only and alarming).
        assert!(
            adv.what.contains("no internet connection"),
            "got: {}",
            adv.what
        );
        assert!(!adv.what.contains("timeout"), "raw error leaked: {}", adv.what);
    }

    #[test]
    fn internal_errors_get_cause_neutral_copy() {
        // String errors (archive guard, I/O) classify as Internal via From —
        // and Internal must never render as a network or server problem.
        let err: CommentSyncError = "corrupt comment.json — not overwriting".to_string().into();
        assert!(matches!(err.kind, CommentSyncErrorKind::Internal));
        let adv = comment_sync_failure_advisory(&err);
        assert!(
            adv.what.contains("a problem with comment data"),
            "got: {}",
            adv.what
        );
        assert!(!adv.what.contains("server unreachable"), "misdirects to server: {}", adv.what);
        assert!(!adv.what.contains("internet"), "misdirects to network: {}", adv.what);
    }

    #[test]
    fn singleflight_drops_concurrent_requests() {
        let folder = "/tmp/test-singleflight";
        // Manually clear any leftover state from prior tests
        if let Ok(mut g) = inflight().lock() {
            g.remove(folder);
        }
        assert!(try_acquire(folder), "first acquire should succeed");
        assert!(!try_acquire(folder), "second acquire while in-flight must fail");
        release(folder);
        assert!(try_acquire(folder), "acquire after release should succeed");
        release(folder);
    }

    #[test]
    fn watchdog_cancel_suppresses_warning() {
        let w = spawn_watchdog("test op", Duration::from_secs(60));
        // Cancellation is best-effort; just verify the API works without panic.
        w.cancel();
    }

    fn tmp_dir() -> tempfile::TempDir {
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let tmp_base = repo_root.join("target").join("test-tmp");
        std::fs::create_dir_all(&tmp_base).unwrap();
        tempfile::TempDir::new_in(&tmp_base).unwrap()
    }

    #[test]
    fn sync_state_missing_file_defaults_empty() {
        let dir = tmp_dir();
        let state = load_comment_sync_state(dir.path().to_str().unwrap());
        assert!(state.last_success_at.is_none());
        assert!(state.last_attempt_at.is_none());
        assert!(state.last_error.is_none());
    }

    #[test]
    fn sync_state_success_then_failure_preserves_last_success() {
        let dir = tmp_dir();
        let folder = dir.path().to_str().unwrap();

        record_comment_sync_success(folder, 1_000.0);
        let s = load_comment_sync_state(folder);
        assert_eq!(s.last_success_at, Some(1_000.0));
        assert_eq!(s.last_attempt_at, Some(1_000.0));
        assert!(s.last_error.is_none());

        record_comment_sync_failure(
            folder,
            2_000.0,
            &CommentSyncError {
                kind: CommentSyncErrorKind::Offline,
                detail: "comment server unreachable: connect refused".into(),
            },
        );
        let s = load_comment_sync_state(folder);
        assert_eq!(
            s.last_success_at,
            Some(1_000.0),
            "a failure must not clobber the last-success timestamp"
        );
        assert_eq!(s.last_attempt_at, Some(2_000.0));
        let err = s.last_error.expect("failure must be recorded");
        assert!(matches!(err.kind, CommentSyncErrorKind::Offline));

        record_comment_sync_success(folder, 3_000.0);
        let s = load_comment_sync_state(folder);
        assert_eq!(s.last_success_at, Some(3_000.0));
        assert!(s.last_error.is_none(), "a success must clear the error");
    }

    #[test]
    fn sync_state_corrupt_file_defaults_empty() {
        let dir = tmp_dir();
        let folder = dir.path().to_str().unwrap();
        let p = comment_sync_state_path(dir.path());
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{not json").unwrap();
        let state = load_comment_sync_state(folder);
        assert!(state.last_success_at.is_none());
        assert!(state.last_error.is_none());
    }

    #[test]
    fn sync_state_lives_outside_watched_paths() {
        // .moss/data/social/** is watched (comment.json rewrites deliberately
        // trigger a refresh build). The state file is rewritten on EVERY sync
        // completion, so it must live outside the watcher allowlist or each
        // sync would trigger a rebuild → sync → rebuild loop.
        let p = comment_sync_state_path(std::path::Path::new("/site"));
        let s = p.to_string_lossy().replace('\\', "/");
        assert!(s.contains("/.moss/data/"), "state belongs under .moss/data/: {s}");
        assert!(
            !s.contains("/.moss/data/social/"),
            "must not be watcher-visible: {s}"
        );
        let after_moss = "data/comment-sync-state.json";
        assert!(!crate::build::watch::should_watch_moss_file(after_moss));
    }
}
