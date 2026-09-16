//! Live preview/watch/process state — the M6a boundary's "runtime" side.
//!
//! State the running app owns while a folder is open: preview-server
//! handles, rebuild and file-watcher coordination, child-process (FFmpeg)
//! tracking, and the media-conversion lifecycle state. None of this moves
//! into a headless moss-build crate. Sibling runtime modules: [`super::assets`]
//! (the asset promise registry) and [`super::services`] (the `BuildServices`
//! container).
//!
//! `redact_home_dir` lives here (not in `content`) because it reads the
//! process environment (`dirs::home_dir`) — a log-privacy utility of the
//! running app, not build data.
//!
//! Split out of `types.rs` 2026-08-10 (M5a); `types.rs` re-exports every
//! item at its old path, so consumers are unchanged.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};

/// Handle to a running preview server, enabling graceful shutdown.
///
/// Each server spawned by the router's `start_server` returns a `ServerHandle`
/// containing the port, a oneshot shutdown sender, and the shared directory state.
/// This replaces the fire-and-forget pattern where server tasks were spawned
/// without retaining any handle, requiring `lsof` + `kill` for cleanup.
pub struct ServerHandle {
    /// Port the server is listening on
    pub port: u16,
    /// Send `()` to gracefully shut down the server.
    /// `None` only if the sender was already consumed (should not happen in normal flow).
    pub shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    /// Shared directory pointer — the server reads this on every request.
    /// Updating this switches which directory the server serves (zero-flicker).
    pub site_dir: std::sync::Arc<std::sync::RwLock<std::path::PathBuf>>,
    /// The HTTP command carrier mounted on this server, if any. Switch the
    /// served directory through [`ServerHandle::point_at`] (defined beside
    /// the carrier's session, `ops::serve::session`) so the carrier rebinds
    /// in the same step; writing `site_dir` alone leaves the previous
    /// vault's token live until the next carrier request notices.
    pub carrier: Option<crate::ops::serve::InvokeCtx>,
}

impl std::fmt::Debug for ServerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerHandle")
            .field("port", &self.port)
            .field("shutdown_tx", &self.shutdown_tx.as_ref().map(|_| "Some(Sender)"))
            .field("site_dir", &self.site_dir)
            .finish()
    }
}

impl ServerHandle {
    /// Gracefully shut down the server by sending the shutdown signal.
    /// Returns `true` if the signal was sent, `false` if already consumed.
    pub fn shutdown(&mut self) -> bool {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
            true
        } else {
            false
        }
    }
}

/// State management for tracking active preview servers.
///
/// Used to track which servers are running for which folders to enable server reuse
/// and avoid creating duplicate servers for the same content.
///
/// Each entry maps a canonical folder path to a `ServerHandle` that can be used
/// to query the port, update the served directory, or gracefully shut down the server.
#[derive(Default)]
pub struct ServerState {
    /// Map of folder paths to their running server handles
    /// Enables server reuse when building the same folder multiple times
    pub active_servers: std::collections::HashMap<String, ServerHandle>,
}

impl std::fmt::Debug for ServerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerState")
            .field("active_servers", &self.active_servers.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// Shared state for the current site directory path.
///
/// This enables zero-flicker preview during rebuilds by allowing the server to
/// dynamically resolve which directory to serve on each request.
///
/// ## Zero-Flicker Preview Pattern
///
/// The problem: During rebuilds, we need to swap the site directory. The naive
/// approach of renaming directories has a gap where the directory doesn't exist:
/// ```text
/// fs::rename(site, site-old)  // site/ is GONE
/// fs::rename(site-new, site)  // site/ is back
/// // ^ During this gap, incoming requests get 404 → preview flickers
/// ```
///
/// The solution: Use a staging directory and dynamically switch the server's pointer:
/// ```text
/// 1. Build to site-stage/           (server still serves site/)
/// 2. Switch pointer to site-stage/  (instant, atomic - preview shows new content)
/// 3. Copy site-stage/ → site/       (canonical directory updated for deployment)
/// 4. Switch pointer back to site/   (instant, atomic)
/// 5. Delete site-stage/             (cleanup)
/// ```
///
/// This keeps `/site` as the single source of truth for deployment (git-tracked)
/// while ensuring the preview never shows 404s or blank pages.
///
/// ## Why RwLock?
///
/// - Read operations (serving requests) are extremely fast (~20ns)
/// - Write operations (pointer switches) are rare and instant
/// - Multiple readers can serve requests concurrently
/// - Writers (rebuild) get exclusive access for the brief switch
#[derive(Debug)]
pub struct SiteDirectoryState {
    /// Current directory the server should serve files from.
    /// Updated atomically during rebuilds to switch between staging and site.
    pub current_dir: std::sync::Arc<std::sync::RwLock<std::path::PathBuf>>,
}

impl SiteDirectoryState {
    /// Create a new state with the given initial directory
    pub fn new(initial_dir: std::path::PathBuf) -> Self {
        Self {
            current_dir: std::sync::Arc::new(std::sync::RwLock::new(initial_dir)),
        }
    }

    /// Switch to a new directory (atomic from server's perspective).
    ///
    /// Returns whether the directory actually changed. Callers log the switch,
    /// and the `initial-build-complete` listener that drives it fires on every
    /// rebuild rather than only the first — so without this it announced 63
    /// switches in one session's log, none of which moved anything (moss#1174).
    pub fn switch_to(&self, new_dir: std::path::PathBuf) -> bool {
        let mut current = self.current_dir.write().unwrap();
        if *current == new_dir {
            return false;
        }
        *current = new_dir;
        true
    }
}

/// Observability flags for the watch-mode rebuild path.
///
/// DERIVED signals, not the coordination mechanism: serialization of rebuilds
/// lives in the per-folder request-slot worker
/// (`build_shell::watch::worker`), which keeps these flags updated for their
/// readers — `deploy::wait_for_rebuild_quiet` and the preview-wait
/// diagnostics. (Under `MOSS_REBUILD_WORKER=off` the inline fallback still
/// uses `is_rebuilding` as its actual lock.)
///
/// Known debt: app-global while the slots are per-folder, so with two folders
/// open one folder's build makes the other read as busy. Same over-waiting
/// the old lock had; making these per-folder is future work, not phase 1a.
#[derive(Debug, Default)]
pub struct RebuildState {
    /// A rebuild is executing right now (the worker is mid-build).
    pub is_rebuilding: std::sync::atomic::AtomicBool,

    /// A rebuild request is queued (the folder's request slot is occupied).
    pub rebuild_pending: std::sync::atomic::AtomicBool,
}

/// State for managing file watcher lifecycle.
///
/// Tracks shutdown channels for file watchers by folder path, allowing
/// graceful shutdown when the preview window is closed.
///
/// ## Usage
/// - When starting a file watcher, register a shutdown sender
/// - When closing preview window, send shutdown signal to stop watcher
/// - Prevents resource leaks from orphaned file watchers
#[derive(Default)]
pub struct FileWatcherState {
    /// Shutdown senders keyed by folder path.
    /// Send to the channel to signal the file watcher to stop.
    pub shutdown_senders: std::sync::Mutex<HashMap<String, tokio::sync::oneshot::Sender<()>>>,
}

impl FileWatcherState {
    /// Is a watcher subscribed to this folder right now?
    ///
    /// Asked by code that writes a watched file (`.moss/config.toml`) and would
    /// otherwise also kick its own rebuild — see
    /// `plugins::plugin_install::rebuild_after_config_write`. One user action
    /// must produce one rebuild, and this is how a writer decides whether it
    /// owns that rebuild or the watcher does.
    ///
    /// Matches the exact key `start_file_watching` was handed, so a caller
    /// whose path string differs (trailing slash, unresolved symlink) reads as
    /// unwatched and rebuilds itself: one redundant build, never a change that
    /// nothing rebuilds. Pinned by test below.
    pub fn is_watching(&self, folder_path: &str) -> bool {
        self.shutdown_senders
            .lock()
            .map(|senders| senders.contains_key(folder_path))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod file_watcher_state_tests {
    use super::FileWatcherState;

    #[test]
    fn is_watching_matches_the_exact_registered_key() {
        let state = FileWatcherState::default();
        let (tx, _rx) = tokio::sync::oneshot::channel::<()>();
        state
            .shutdown_senders
            .lock()
            .unwrap()
            .insert("/Users/user/site".to_string(), tx);

        assert!(state.is_watching("/Users/user/site"));
        assert!(
            !state.is_watching("/Users/user/site/"),
            "a near-miss key must read as unwatched — the caller then rebuilds, \
             which is the safe direction"
        );
        assert!(!state.is_watching("/Users/user/other"));
    }
}

/// Registry for tracking child processes (FFmpeg) that need cleanup on folder switch.
///
/// # Architecture: PID-Based Process Tracking
///
/// This registry uses PID-based tracking instead of storing `Child` handles because:
/// 1. FFmpeg processes are spawned in synchronous code that doesn't have access to Tauri state
/// 2. The code that spawns processes needs to retain ownership of `Child` to call `.wait()`
/// 3. PIDs can be shared across threads without ownership concerns
///
/// When switching between project folders during preview, we need to:
/// 1. Kill any running FFmpeg processes from the previous folder via `kill_all()`
/// 2. Clear this registry before starting new build
///
/// This prevents orphaned processes from consuming resources and potentially
/// writing to the wrong output directory.
///
/// # Usage Pattern
/// ```ignore
/// let mut child = Command::new("ffmpeg").args(...).spawn()?;
/// let pid = child.id();
/// registry.register_pid(pid);
///
/// // ... wait for process or handle cancellation ...
///
/// registry.unregister_pid(pid);
/// ```
pub struct ChildProcessRegistry {
    /// Set of active process PIDs being tracked
    pub pids: std::sync::Mutex<HashSet<u32>>,
}

impl ChildProcessRegistry {
    pub fn new() -> Self {
        Self {
            pids: std::sync::Mutex::new(HashSet::new()),
        }
    }

    /// Register a child process for tracking (backward compat).
    /// Extracts PID and delegates to `register_pid`.
    #[allow(dead_code)]
    pub fn register(&self, child: &std::process::Child) {
        self.register_pid(child.id());
    }

    /// Register a process PID for tracking.
    pub fn register_pid(&self, pid: u32) {
        if let Ok(mut pids) = self.pids.lock() {
            pids.insert(pid);
        }
    }

    /// Unregister a process PID after it completes.
    /// Should be called when a process exits normally to avoid
    /// killing a reused PID.
    pub fn unregister_pid(&self, pid: u32) {
        if let Ok(mut pids) = self.pids.lock() {
            pids.remove(&pid);
        }
    }

    /// Kill all tracked processes and clear the registry.
    /// Called during folder switch to prevent orphaned processes.
    ///
    /// Uses a process group check to mitigate PID reuse races: if a child
    /// exits and the OS reassigns its PID to an unrelated process before
    /// `unregister_pid` runs, we skip the kill instead of sending SIGKILL
    /// to the wrong process.
    #[cfg(unix)]
    pub fn kill_all(&self) {
        // SAFETY: getpgrp() is a read-only query with no preconditions.
        let our_pgid = unsafe { libc::getpgrp() };

        if let Ok(mut pids) = self.pids.lock() {
            for pid in pids.drain() {
                let pid_i32 = pid as i32;

                // SAFETY: getpgid() is a read-only query. If the PID is invalid
                // (process already exited), getpgid returns -1.
                let target_pgid = unsafe { libc::getpgid(pid_i32) };

                if target_pgid == our_pgid {
                    // SAFETY: We verified this PID belongs to our process group,
                    // mitigating the PID reuse race. SIGKILL ensures immediate
                    // termination of the child process.
                    unsafe {
                        libc::kill(pid_i32, libc::SIGKILL);
                    }
                } else if target_pgid == -1 {
                    // Process already exited — nothing to kill
                } else {
                    log::warn!(
                        "Skipping kill for PID {}: belongs to process group {} (ours: {})",
                        pid, target_pgid, our_pgid
                    );
                }
            }
        }
    }

    /// Kill all tracked processes and clear the registry (Windows).
    #[cfg(windows)]
    pub fn kill_all(&self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, TerminateProcess, PROCESS_TERMINATE,
        };

        if let Ok(mut pids) = self.pids.lock() {
            for pid in pids.drain() {
                // SAFETY: OpenProcess on an exited/invalid PID returns a null
                // handle, which we check before TerminateProcess. CloseHandle
                // releases the handle. Exit code 1 marks abnormal termination.
                unsafe {
                    let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
                    if !handle.is_null() {
                        let _ = TerminateProcess(handle, 1);
                        let _ = CloseHandle(handle);
                    }
                }
            }
        }
    }
}

impl Default for ChildProcessRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// State for managing video conversion lifecycle.
///
/// # Architecture: cancellation moved to `FolderSession`
///
/// As of #614 G4 (Track A), cancellation is driven exclusively by
/// `FolderSession::cancel`. This state retains:
/// - `conversion_id`: monotonically increasing epoch for filtering stale
///   progress events and for inter-task epoch checks
/// - `last_video_fingerprint`: short-circuits re-dispatch when the video set
///   hasn't changed
///
/// The legacy `cancel()` / `is_cancelled()` methods remain as deprecated
/// no-ops to keep API churn local; callers that need to read cancellation
/// must source it from `FolderSession::cancel` instead.
#[derive(Debug, Default)]
pub struct VideoConversionState {
    /// Monotonically increasing ID for the current conversion
    conversion_id: AtomicU64,
    /// Fingerprint of the last dispatched video set (SHA-256 of sorted video
    /// paths + sizes + mtimes + compression params). When a rebuild triggers
    /// dispatch and the fingerprint matches, we skip cancellation and let the
    /// in-progress conversion continue — the video set hasn't changed, so there's
    /// no reason to restart.
    last_video_fingerprint: std::sync::Mutex<Option<String>>,
}

impl VideoConversionState {
    /// Create a new state (starts with id=0)
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            conversion_id: AtomicU64::new(0),
            last_video_fingerprint: std::sync::Mutex::new(None),
        }
    }

    /// Start a new conversion session.
    /// Increments the conversion ID. Returns the new conversion ID.
    ///
    /// (Pre-Track A this also reset a `cancelled: AtomicBool`. That field
    /// is now gone; cancellation flows through `FolderSession::cancel`.)
    pub fn start_new_conversion(&self) -> u64 {
        self.conversion_id.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Deprecated: cancellation is now driven by `FolderSession::cancel`.
    /// Kept as a no-op so legacy callers compile during migration.
    #[deprecated(note = "Cancellation is now driven by FolderSession::cancel; this is a no-op.")]
    pub fn cancel(&self) {}

    /// Deprecated: always returns false. Use `session.cancel.is_cancelled()` instead.
    #[deprecated(note = "Always returns false; use session.cancel.is_cancelled() instead.")]
    #[allow(dead_code)]
    pub fn is_cancelled(&self) -> bool {
        false
    }

    /// Get the current conversion ID.
    pub fn current_id(&self) -> u64 {
        self.conversion_id.load(Ordering::SeqCst)
    }

    /// Check if the video set fingerprint matches the last dispatched conversion.
    /// Returns true if the fingerprint matches (no change), false otherwise.
    /// Updates the stored fingerprint to the new value.
    pub fn check_and_update_fingerprint(&self, new_fingerprint: &str) -> bool {
        let mut last = self.last_video_fingerprint.lock().unwrap();
        let matches = last.as_deref() == Some(new_fingerprint);
        if !matches {
            *last = Some(new_fingerprint.to_string());
        }
        matches
    }
}

/// Field-less placeholder for image conversion lifecycle.
///
/// As of #614 G4 (Track A), image cancellation flows through
/// `FolderSession::cancel`. This type stays in managed state to preserve
/// `BuildServices.image_cancellation` plumbing; the legacy methods remain
/// as deprecated no-ops to keep API churn local.
#[derive(Debug, Default)]
pub struct ImageConversionState;

impl ImageConversionState {
    /// Create a new state.
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self
    }

    /// Deprecated: was used to clear the cancel flag. The flag no longer
    /// exists; cancellation is driven by `FolderSession::cancel`.
    #[deprecated(note = "No-op; cancel state is now owned by FolderSession.")]
    #[allow(dead_code)]
    pub fn start_new_conversion(&self) {}

    /// Deprecated: cancellation is now driven by `FolderSession::cancel`.
    #[deprecated(note = "Cancellation is now driven by FolderSession::cancel; this is a no-op.")]
    pub fn cancel(&self) {}

    /// Deprecated: always returns false. Use `session.cancel.is_cancelled()` instead.
    #[deprecated(note = "Always returns false; use session.cancel.is_cancelled() instead.")]
    #[allow(dead_code)]
    pub fn is_cancelled(&self) -> bool {
        false
    }
}

/// Field-less placeholder for notebook (JupyterLite) processing lifecycle.
///
/// As of #614 G4 (Track A), notebook cancellation flows through
/// `FolderSession::cancel`. This type stays in managed state to preserve
/// `BuildServices.notebook_cancellation` plumbing; the legacy methods remain
/// as deprecated no-ops to keep API churn local.
#[derive(Debug, Default)]
pub struct NotebookConversionState;

impl NotebookConversionState {
    /// Create a new state.
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self
    }

    /// Deprecated: was used to clear the cancel flag. The flag no longer
    /// exists; cancellation is driven by `FolderSession::cancel`.
    #[deprecated(note = "No-op; cancel state is now owned by FolderSession.")]
    #[allow(dead_code)]
    pub fn start_new_conversion(&self) {}

    /// Deprecated: cancellation is now driven by `FolderSession::cancel`.
    #[deprecated(note = "Cancellation is now driven by FolderSession::cancel; this is a no-op.")]
    pub fn cancel(&self) {}

    /// Deprecated: always returns false. Use `session.cancel.is_cancelled()` instead.
    #[deprecated(note = "Always returns false; use session.cancel.is_cancelled() instead.")]
    #[allow(dead_code)]
    pub fn is_cancelled(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Privacy utilities
// ---------------------------------------------------------------------------

/// Replace the user's home directory prefix with `~` for privacy in logs.
///
/// Production logs are sent to our server when users click "Send Logs".
/// Full absolute paths like `/Users/liuguo/Library/Mobile Documents/...`
/// reveal the macOS username, iCloud usage, and vault locations.
/// This function strips that information before logging.
pub fn redact_home_dir(path: &str) -> String {
    use std::sync::OnceLock;
    static HOME: OnceLock<Option<String>> = OnceLock::new();
    let home = HOME.get_or_init(|| dirs::home_dir().map(|h| h.to_string_lossy().into_owned()));
    if let Some(rest) = home.as_deref().and_then(|h| path.strip_prefix(h)) {
        return format!("~{rest}");
    }
    path.to_string()
}

#[cfg(test)]
mod redact_tests {
    use super::*;

    #[test]
    fn test_redact_home_dir_replaces_prefix() {
        let home = dirs::home_dir().unwrap();
        let path = format!("{}/Documents/test", home.display());
        assert_eq!(redact_home_dir(&path), "~/Documents/test");
    }

    #[test]
    fn test_redact_home_dir_no_match() {
        assert_eq!(redact_home_dir("/tmp/some/path"), "/tmp/some/path");
    }

    #[test]
    fn test_redact_home_dir_empty_string() {
        assert_eq!(redact_home_dir(""), "");
    }

    #[test]
    fn test_redact_home_dir_exact_home() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(redact_home_dir(&home.to_string_lossy()), "~");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a ServerHandle for testing
    fn test_handle(port: u16) -> ServerHandle {
        let (tx, _rx) = tokio::sync::oneshot::channel::<()>();
        ServerHandle {
            port,
            shutdown_tx: Some(tx),
            site_dir: std::sync::Arc::new(std::sync::RwLock::new(std::path::PathBuf::new())),
            carrier: None,
        }
    }

    #[test]
    fn test_server_state_registration() {
        let mut state = ServerState::default();
        state.active_servers.insert("/path1".to_string(), test_handle(3000));

        assert_eq!(state.active_servers.get("/path1").map(|h| h.port), Some(3000));
        assert_eq!(state.active_servers.get("/nonexistent").map(|h| h.port), None);
    }

    #[test]
    fn test_server_state_removal() {
        let mut state = ServerState::default();
        state.active_servers.insert("/path1".to_string(), test_handle(3000));

        state.active_servers.remove("/path1");
        assert_eq!(state.active_servers.get("/path1").map(|h| h.port), None);
    }

    #[test]
    fn test_server_state_duplicate_paths() {
        let mut state = ServerState::default();
        state.active_servers.insert("/path1".to_string(), test_handle(3000));
        state.active_servers.insert("/path1".to_string(), test_handle(4000));

        // Should overwrite with new handle
        assert_eq!(state.active_servers.get("/path1").map(|h| h.port), Some(4000));
    }

    #[test]
    fn test_server_state_multiple_servers() {
        let mut state = ServerState::default();
        state.active_servers.insert("/path1".to_string(), test_handle(3000));
        state.active_servers.insert("/path2".to_string(), test_handle(4000));
        state.active_servers.insert("/path3".to_string(), test_handle(5000));

        assert_eq!(state.active_servers.len(), 3);
        assert_eq!(state.active_servers.get("/path2").map(|h| h.port), Some(4000));
    }

    #[test]
    fn test_server_handle_shutdown_sends_signal() {
        let (tx, mut rx) = tokio::sync::oneshot::channel::<()>();
        let mut handle = ServerHandle {
            port: 8080,
            shutdown_tx: Some(tx),
            site_dir: std::sync::Arc::new(std::sync::RwLock::new(std::path::PathBuf::new())),
            carrier: None,
        };

        assert!(handle.shutdown(), "First shutdown should succeed");
        assert!(rx.try_recv().is_ok(), "Receiver should get signal");
        assert!(!handle.shutdown(), "Second shutdown should return false");
    }

    #[test]
    fn test_shutdown_all_servers_drains_state() {
        let mut state = ServerState::default();
        state.active_servers.insert("/path1".to_string(), test_handle(3000));
        state.active_servers.insert("/path2".to_string(), test_handle(4000));

        assert_eq!(state.active_servers.len(), 2);
        // Drain all handles
        for (_, mut handle) in state.active_servers.drain() {
            handle.shutdown();
        }
        assert_eq!(state.active_servers.len(), 0);
    }

    #[test]
    fn test_server_handle_debug_with_sender() {
        let handle = test_handle(8080);
        let debug = format!("{:?}", handle);
        assert!(debug.contains("ServerHandle"));
        assert!(debug.contains("8080"));
        assert!(debug.contains("Some(Sender)"));
    }

    #[test]
    fn test_server_handle_debug_without_sender() {
        let mut handle = test_handle(8080);
        handle.shutdown_tx = None;
        let debug = format!("{:?}", handle);
        assert!(debug.contains("ServerHandle"));
        assert!(debug.contains("None"));
    }

    #[test]
    fn test_server_state_debug() {
        let mut state = ServerState::default();
        state.active_servers.insert("/my/path".to_string(), test_handle(3000));
        let debug = format!("{:?}", state);
        assert!(debug.contains("ServerState"));
        assert!(debug.contains("/my/path"));
    }

    #[test]
    fn test_server_handle_site_dir_update() {
        let handle = test_handle(8080);
        handle.point_at(std::path::PathBuf::from("/new/path"));
        // Verify the update is visible
        let dir = handle.site_dir.read().unwrap();
        assert_eq!(*dir, std::path::PathBuf::from("/new/path"));
    }

    #[test]
    fn test_rebuild_state_default() {
        use std::sync::atomic::Ordering;

        let state = RebuildState::default();
        assert!(!state.is_rebuilding.load(Ordering::SeqCst));
        assert!(!state.rebuild_pending.load(Ordering::SeqCst));
    }

    // The old lock-and-pending / compare-exchange tests are gone with the
    // mechanism they pinned: since phase 1a these flags are derived signals
    // kept fresh by the request-slot worker, whose behavior is tested in
    // `build_shell::watch::worker`.

    // =========================================
    // SiteDirectoryState Tests
    // =========================================

    #[test]
    fn test_site_directory_state_new() {
        let state = SiteDirectoryState::new(std::path::PathBuf::from("/folder_a/.moss/build/current"));
        let current = state.current_dir.read().unwrap();
        assert_eq!(*current, std::path::PathBuf::from("/folder_a/.moss/build/current"));
    }

    #[test]
    fn test_site_directory_state_switch_to_new_folder() {
        // Behavior: When switching from folder A to folder B, the server should
        // continue running but serve content from folder B's active generation.
        // This is used for zero-flicker folder switching without restarting servers.
        let state = SiteDirectoryState::new(std::path::PathBuf::from("/folder_a/.moss/build/current"));

        // Switch to folder B
        assert!(state.switch_to(std::path::PathBuf::from("/folder_b/.moss/build/current")));
        // Re-switching to the same directory is not a switch. The caller logs
        // off this, and its `initial-build-complete` listener fires on every
        // rebuild — 63 announced switches in one session's log, none of which
        // moved anything (moss#1174).
        assert!(!state.switch_to(std::path::PathBuf::from("/folder_b/.moss/build/current")));

        let current = state.current_dir.read().unwrap();
        assert_eq!(
            *current,
            std::path::PathBuf::from("/folder_b/.moss/build/current"),
            "After switch_to, server should serve folder_b content"
        );
    }

    #[test]
    fn test_site_directory_state_switch_preserves_arc() {
        // Behavior: The Arc should remain the same after switching, allowing
        // the server to pick up the new directory on next request without restart.
        let state = SiteDirectoryState::new(std::path::PathBuf::from("/initial"));

        // Get a clone of the Arc (simulating what the server would hold)
        let server_ref = state.current_dir.clone();

        // Switch directories
        state.switch_to(std::path::PathBuf::from("/switched"));

        // Server's reference should see the new directory
        let server_sees = server_ref.read().unwrap();
        assert_eq!(
            *server_sees,
            std::path::PathBuf::from("/switched"),
            "Server's Arc reference should see the switched directory"
        );
    }

    // =========================================
    // TDD Tests for FFmpeg Cancellation Feature
    // =========================================
    //
    // These tests are written FIRST (RED phase) to define the expected behavior
    // of the new PID-based ChildProcessRegistry and VideoConversionState.

    /// Test that ChildProcessRegistry can register and kill processes by PID.
    ///
    /// This tests the new PID-based design that replaces the broken Child-ownership model.
    /// The registry should:
    /// 1. Accept a PID (u32) instead of taking ownership of Child
    /// 2. Kill processes via libc::kill() on folder switch
    /// 3. Successfully terminate long-running processes
    #[test]
    #[cfg(unix)]
    fn test_child_process_registry_kills_by_pid() {
        let registry = ChildProcessRegistry::new();

        // Spawn a long-running process (sleep for 60 seconds)
        let mut child = std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .expect("Failed to spawn sleep process");

        let pid = child.id();

        // Register PID with registry (new API)
        registry.register_pid(pid);

        // Kill via registry
        registry.kill_all();

        // Verify process was killed (wait should return quickly with failure status)
        let status = child.wait().expect("Failed to wait for child");

        // Process should have been killed (not exited successfully)
        assert!(!status.success(), "Process should have been killed, not exited normally");

        // On Unix, killed by SIGKILL should show signal 9
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            assert_eq!(status.signal(), Some(9), "Process should have been killed by SIGKILL");
        }
    }

    /// Test that ChildProcessRegistry can unregister PIDs after process completes.
    ///
    /// When a process completes normally, we should unregister it so kill_all()
    /// doesn't try to kill a non-existent process (which could accidentally kill
    /// a reused PID).
    #[test]
    fn test_child_process_registry_unregister_pid() {
        let registry = ChildProcessRegistry::new();

        // Register a fake PID
        registry.register_pid(99999);

        // Unregister it
        registry.unregister_pid(99999);

        // After unregister, the PID should not be in the registry
        // kill_all should be a no-op
        registry.kill_all(); // Should not panic or error
    }

    /// Test that kill_all verifies process group before sending SIGKILL.
    ///
    /// After the PID reuse fix, kill_all checks that the target PID belongs
    /// to our process group before killing. This test verifies:
    /// 1. A child in our process group is still killed correctly
    /// 2. The registry is drained after kill_all
    #[test]
    #[cfg(unix)]
    fn test_kill_all_checks_process_group() {
        let registry = ChildProcessRegistry::new();

        // Spawn a child (inherits our process group)
        let mut child = std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .expect("Failed to spawn sleep process");
        let child_pid = child.id();

        // Also register PID 1 (launchd — different process group).
        // The process group check should skip it.
        registry.register_pid(child_pid);
        registry.register_pid(1);

        registry.kill_all();

        // Our child should have been killed
        let status = child.wait().expect("Failed to wait for child");
        assert!(!status.success(), "Child should have been killed");

        // Registry should be fully drained
        let pids = registry.pids.lock().unwrap();
        assert!(pids.is_empty(), "Registry should be empty after kill_all");
    }

    /// Test that VideoConversionState's `start_new_conversion` increments
    /// the conversion ID monotonically.
    ///
    /// (Pre-Track A this also tested cancel-flag round-trip behavior. The
    /// `cancelled: AtomicBool` field is gone — cancellation lives on
    /// `FolderSession::cancel`. This test now only covers the id-increment
    /// behavior that remained.)
    #[test]
    fn test_video_conversion_state_id_increments() {
        let state = VideoConversionState::default();

        let id1 = state.start_new_conversion();
        let id2 = state.start_new_conversion();
        assert!(id2 > id1, "Conversion ID should increment");
    }

    /// Test VideoConversionState thread safety: concurrent
    /// `start_new_conversion` calls produce monotonically increasing,
    /// non-duplicate ids.
    #[test]
    fn test_video_conversion_state_thread_safety() {
        use std::sync::{Arc, Mutex};
        use std::thread;

        let state = Arc::new(VideoConversionState::default());
        let ids = Arc::new(Mutex::new(Vec::<u64>::new()));
        let mut handles = vec![];

        for _ in 0..10 {
            let state_clone = Arc::clone(&state);
            let ids_clone = Arc::clone(&ids);
            handles.push(thread::spawn(move || {
                let id = state_clone.start_new_conversion();
                ids_clone.lock().unwrap().push(id);
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let mut got = ids.lock().unwrap().clone();
        got.sort();
        // 10 distinct, contiguous ids starting at 1.
        assert_eq!(got.len(), 10);
        for (i, id) in got.iter().enumerate() {
            assert_eq!(*id, (i + 1) as u64);
        }
    }

    /// Test that conversion_id is monotonically increasing.
    ///
    /// This is important for filtering stale progress events from old conversions.
    #[test]
    fn test_video_conversion_state_id_monotonic() {
        let state = VideoConversionState::default();

        let mut prev_id = 0;
        for _ in 0..100 {
            let id = state.start_new_conversion();
            assert!(id > prev_id, "Conversion ID must be monotonically increasing");
            prev_id = id;
        }
    }

    #[test]
    fn test_video_conversion_fingerprint_first_call_returns_false() {
        let state = VideoConversionState::new();
        // First call with any fingerprint should return false (no previous)
        assert!(!state.check_and_update_fingerprint("abc123"));
    }

    #[test]
    fn test_video_conversion_fingerprint_same_returns_true() {
        let state = VideoConversionState::new();
        state.check_and_update_fingerprint("abc123");
        // Second call with same fingerprint should return true (match)
        assert!(state.check_and_update_fingerprint("abc123"));
    }

    #[test]
    fn test_video_conversion_fingerprint_different_returns_false() {
        let state = VideoConversionState::new();
        state.check_and_update_fingerprint("abc123");
        // Different fingerprint should return false and update
        assert!(!state.check_and_update_fingerprint("def456"));
        // Now the new fingerprint should match
        assert!(state.check_and_update_fingerprint("def456"));
    }
}
