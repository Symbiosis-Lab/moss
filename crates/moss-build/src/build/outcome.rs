//! Why a build stopped — and specifically, whether it *cannot succeed* or
//! *cannot succeed right now*.
//!
//! ## The defect this exists to close
//!
//! The pipeline used to fail with `Result<_, String>`, and two incompatible
//! outcomes mapped onto that one `Err`: a malformed config, a permission
//! error, a full disk — versus a file the cloud provider has not handed back
//! yet. `?` silently picked the first.
//!
//! That mattered because the cloud gate — the verdict that decides between
//! "show the site" and "show the waiting-for-download screen" — is emitted from
//! *inside* `build_inner`, just before the `complete` progress event, since the
//! verdict is the build's own output. Any `?` before that point skipped the
//! gate entirely: no `icloud-sync` event was emitted at all, and the user got
//! `Failed to build: … Resource deadlock avoided (os error 11)` on the
//! onboarding overlay where the waiting screen belonged, on every rebuild,
//! until the provider finished on its own schedule (moss#964).
//!
//! ## The rule
//!
//! **`Deferred` is not reachable by `?`.** There is no `From` impl that
//! produces it. It exists only where a site has looked at an `io::Error` and
//! classified it — [`io_stop`] — or has said so outright
//! ([`BuildStopped::deferred`]). So a `Deferred` outcome is *unforgeable*,
//! which is what makes the gate safe to drive from it.
//!
//! `From<String>` and `From<io::Error>` both produce `Fatal`, so every existing
//! `?` inside the pipeline keeps compiling and keeps meaning exactly what it
//! means today. That is a deliberate limitation, not an oversight: making the
//! compiler enumerate every site would mean rewriting several hundred `?`
//! across the render pass, and those files move into `crates/moss-build` at
//! M6a/M6b where `BuildRecord out` is one of the five ports. What this type
//! buys today is that the *deferred* half can never be produced by accident —
//! the direction that decides which screen the user sees.
//!
//! **Falsifier:** a build that stops on a cloud-unavailable file through a call
//! site that has not been converted to [`io_stop`] reports `Fatal`, and the
//! user sees the error instead of the waiting screen — the pre-fix behaviour,
//! never something worse. If that keeps happening after this lands, the answer
//! is to convert more sites, not to widen the type.
//!
//! ## `Deferred` has no live producer today, and that is the good state
//!
//! Every converted call site reads back a file under `.moss/build.nosync/staging/`, so
//! every one of them takes the `Discard` arm. Nothing in the pipeline currently
//! returns `Deferred`.
//!
//! That is not an oversight, it is what four rounds of read-side audit
//! (moss#956) achieved: the source-read class — pages, theme, config, assets —
//! is handled by the disciplines in `cloud_readiness`
//! (`read_page_source`, `read_optional_build_input`, `retry_after_materialize`),
//! which defer *inside* the build. They record the file and carry on, so the
//! build succeeds and reaches its own gate verdict. A source read that fails the
//! whole build is exactly the thing that should not exist.
//!
//! So this type earns its place two ways, neither of which is a live `Deferred`:
//!
//! - [`io_stop`]'s `Discard` arm is a **behaviour fix**: an evicted staged file
//!   is removed and regenerated instead of failing identically on every
//!   rebuild. Because a failed *open* build leaves no watcher behind to try
//!   again, the removal is paired with a `discarded` verdict that
//!   `build::run_pipeline` answers by re-running the build once.
//! - The type is the **guard** that keeps the good state good. The next read
//!   someone adds gets `Fatal` from `?` — the safe default — and if it needs to
//!   say "not yet", the only way to do so is through a classifier.
//!
//! Do not read the `Deferred` path as dead code to delete. Read it as the reason
//! nobody has to remember this again.
//!
//! Deleted when M6b's `ports.rs` draws the real build-outcome seam. See
//! `docs/archive/2026-08-05-dataless-output-writes-and-gate-preemption.md`.

use std::fmt;
use std::path::Path;

/// Why a build stopped, and whether waiting would help.
#[derive(Clone, PartialEq, Eq)]
pub struct BuildStopped {
    message: String,
    deferred: bool,
    discarded: bool,
}

impl BuildStopped {
    /// The build cannot succeed until something outside moss changes — today,
    /// always a file the cloud provider has not handed back.
    ///
    /// Not reachable by `?`. Prefer [`io_stop`], which classifies rather than
    /// asserts; use this only where the deferral is known without an
    /// `io::Error` in hand.
    pub fn deferred(message: impl Into<String>) -> Self {
        Self { message: message.into(), deferred: true, discarded: false }
    }

    /// The build stopped on a file it has just unlinked from the stage.
    ///
    /// Not reachable by `?`, for the same reason [`BuildStopped::deferred`] is
    /// not: the caller re-runs the build on it, so a `?` that could forge one
    /// would turn an ordinary failure into a silent second build. Produced by
    /// [`io_stop`]'s `Discard` arm, and only when the removal succeeded —
    /// re-running against a file that is still there would fail identically.
    pub fn discarded(message: impl Into<String>) -> Self {
        Self { message: message.into(), deferred: false, discarded: true }
    }

    /// Would waiting help? Drives the cloud gate on the failure path.
    pub fn is_deferred(&self) -> bool {
        self.deferred
    }

    /// Was the cause removed on the way out? Drives the single rebuild in
    /// `build::run_pipeline` — see [`BuildStopped::discarded`].
    pub fn is_discarded(&self) -> bool {
        self.discarded
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    /// Unwrap to the string the rest of moss still speaks.
    pub fn into_message(self) -> String {
        self.message
    }

    /// Say what moss was doing when it stopped, **keeping the verdict**.
    ///
    /// This is the only safe way to add context. `map_err(|e| format!("…: {e}"))`
    /// — the idiom used everywhere else in the pipeline — would rebuild a
    /// `Fatal` from the text and silently discard the deferral, which is the
    /// exact bug this type exists to prevent. That idiom does not compile here:
    /// `BuildStopped` deliberately implements no `Display`.
    pub fn with_context(mut self, doing: &str) -> Self {
        self.message = format!("{doing}: {}", self.message);
        self
    }
}

/// **No `Display`, on purpose.** See [`BuildStopped::with_context`]: without it,
/// `format!("{e}")` cannot silently downgrade a deferred stop to a fatal one.
/// `Debug` is derived and shows both fields, which is what a panic message or a
/// log line actually wants here.
impl fmt::Debug for BuildStopped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match (self.deferred, self.discarded) {
            (true, _) => "Deferred",
            (_, true) => "Discarded",
            _ => "Fatal",
        };
        write!(f, "BuildStopped::{kind}({:?})", self.message)
    }
}

/// Every `?` on a `Result<_, String>` inside the pipeline lands here — and
/// means `Fatal`, exactly as it does today. See the module doc.
impl From<String> for BuildStopped {
    fn from(message: String) -> Self {
        Self { message, deferred: false, discarded: false }
    }
}

impl From<&str> for BuildStopped {
    fn from(message: &str) -> Self {
        Self::from(message.to_string())
    }
}

/// A bare `io::Error` carries no context about *what* was being read, so it
/// cannot be classified against a path. Fatal. Reach for [`io_stop`] instead.
impl From<std::io::Error> for BuildStopped {
    fn from(e: std::io::Error) -> Self {
        Self::from(e.to_string())
    }
}

/// Classify an `io::Error` against the path it came from, and stop the build
/// accordingly.
///
/// `context` is the human half of the message, in the imperative the rest of
/// the pipeline uses — `"read OG card for hashing"`, `"ship HTML"`. The path
/// and the OS error are appended.
///
/// Uses `icloud::is_offline_not_absent`, which covers **both** eviction forms:
/// the Sonoma+ `EDEADLK` from the fail-fast policy, and the pre-Sonoma
/// `.name.icloud` placeholder that gives a real `ENOENT` on the real path. A
/// classifier that only looked at the errno would miss half of moss's supported
/// macOS versions.
///
/// ## Three kinds of offline file, three different answers
///
/// The question is never "is this file important?" — it is **"would waiting
/// actually end?"** A `Deferred` stop raises a screen that only a *later,
/// successful build* can lower, so it is a promise that the file's arrival will
/// trip the watcher into rebuilding. Where that link is missing, deferring
/// strands the user in front of a progress bar with nothing behind it and no
/// way out — the screen has no dismiss control, so only a later successful
/// build (or re-opening the folder) ends it.
///
/// [`Disposition`] answers that question; the three cases are documented there.
///
/// `root` is the **vault root**. The watcher predicates [`Disposition`] asks
/// judge the path INSIDE the vault, and handing them an absolute path lets a
/// dot-prefixed ancestor — a Google shared-drive vault lives below
/// `.shortcut-targets-by-id`, an iCloud one below `~/Library/Mobile
/// Documents/` — condemn every file in the vault, so every dataless read
/// reported a raw OS error instead of raising the waiting screen (#1067).
pub fn io_stop(root: &Path, context: &str, path: &Path, e: std::io::Error) -> BuildStopped {
    let message = format!("Failed to {context} {}: {e}", path.display());
    if !crate::build::icloud::is_offline_not_absent(path, &e) {
        return BuildStopped::from(message);
    }
    match disposition(root, path) {
        Disposition::Discard => {
            let removed = std::fs::remove_file(path).is_ok();
            log::warn!(
                "{} was evicted from the build stage{} — the next build will \
                 re-render it (ADR-043)",
                path.display(),
                if removed { ", and has been discarded" } else { " (could not discard it)" }
            );
            // Only a removal earns the rebuild: if the file is still there the
            // second attempt reads the same evicted inode and stops in the
            // same place.
            if removed { BuildStopped::discarded(message) } else { BuildStopped::from(message) }
        }
        Disposition::Wait => {
            crate::build::cloud_readiness::request_download(path);
            BuildStopped::deferred(message)
        }
        Disposition::Report => {
            log::warn!(
                "{} is still in the cloud, but its arrival would not trigger a rebuild \
                 — reporting rather than raising an unclearable waiting screen",
                path.display()
            );
            BuildStopped::from(message)
        }
    }
}

/// What to do about a file the cloud has not handed back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Disposition {
    /// **Anything under `build.nosync/staging/`, `build.nosync/cache/` or the
    /// store at `cache/`** — never a sealed generation, which the preview may
    /// be serving. A store
    /// blob is waited for deliberately, once, by `ObjectStore::ready_blob`;
    /// a read that still stops on it has already lost that wait, and a
    /// `Report` here would raise a waiting screen nothing lowers (the
    /// supervisor never sweeps the store). Delete it and
    /// fail; the caller re-runs the build once and it is written fresh. Per
    /// ADR-043 a dataless file in the stage is absent — moss regenerates it
    /// from source and does not want the bytes back, so waiting is pure delay.
    /// `unlink` does not touch data extents, so removing it cannot
    /// materialize, and removing it is what makes the failure self-healing
    /// rather than repeating against the same evicted inode on every rebuild —
    /// the shape of the moss#964 report.
    ///
    /// **The rule is "delete only what the next build is guaranteed to
    /// rewrite", and the whole of staging and the CAS now qualify.** The build
    /// registers only what it wrote itself, and every presence check treats a
    /// dataless output as absent, so a staged page, an OG card or a cached blob
    /// that disappears is regenerated rather than carried. A CAS miss costs a
    /// re-derivation, which is what a cache is for.
    ///
    /// `generations/<id>/` is deliberately **not** here: it is the immutable
    /// output the preview server is serving *right now*, and a build writes a
    /// *new* generation rather than rewriting that one. It falls to
    /// [`Disposition::Report`], which loses nothing.
    Discard,
    /// **Anything the watcher rebuilds on** — the user's markdown and assets,
    /// `.moss/theme`, `.moss/assets`, `.moss/config.toml`, `.moss/data/social`.
    /// Ask for the download and raise the gate: the bytes exist nowhere else, so
    /// waiting is the only thing that helps, and the loop closes (supervisor
    /// sweeps → arrival trips the watcher → rebuild → `home_ready` → gate
    /// lowered).
    Wait,
    /// **Everything else** — `generations/`, `.moss/plugins`,
    /// `.moss/identity`, most of `.moss/data`, and any vault
    /// file whose extension the watcher ignores (`.docx`, `.pages`, …). Report
    /// the failure.
    ///
    /// Not because these are unimportant — several are build inputs — but
    /// because a waiting screen raised over one would never come down. The
    /// rebuild that lowers the gate is scheduled by
    /// `watch::any_path_passes_filter`, and what that rejects, no arrival can
    /// wake. The download supervisor's sweep applies the same filter
    /// (`build_shell::watch::sweep`), so for most of this set nothing is even
    /// downloading.
    Report,
}

/// Decide by asking the watcher — the single source of truth for "does an
/// arrival here trigger a rebuild?".
///
/// One caveat on "the watcher decides": the watcher also drops an event whose
/// paths are *all* moss-written (`watch::all_paths_moss_written` — a root
/// `AGENTS.md`, `CLAUDE.md`, `GEMINI.md`), which this does not ask, so those
/// three are classified `Wait` here. Nothing reads them through `io_stop`
/// today; convert one and add the test.
///
/// **Both** watcher predicates, because the rebuild needs both:
/// `path_is_watchable` rejects dotfiles and `node_modules` (and delegates to
/// `should_watch_moss_file` inside `.moss/`), and `path_passes_filter` applies
/// the extension allowlist. Restating either one here would be a second copy of
/// a list that has already drifted once; delegating means a new watched
/// extension is picked up for free.
///
/// The staging check splits on the `.moss` *component* rather than a substring,
/// so a vault whose own directory name contains `.moss` cannot be misread. It
/// takes the last such component, matching what `path_is_watchable` does with
/// the vault-relative path.
fn disposition(root: &Path, path: &Path) -> Disposition {
    let regenerable = moss_relative_tail(path).is_some_and(|rel| {
        rel.starts_with("build.nosync/staging/")
            || rel.starts_with("build.nosync/cache/")
            || rel.starts_with("cache/")
    });
    if regenerable {
        return Disposition::Discard;
    }
    use crate::build::watch::scope::{path_is_watchable, path_passes_filter};
    if path_is_watchable(root, path) && path_passes_filter(root, path) {
        return Disposition::Wait;
    }
    Disposition::Report
}

/// Run a build attempt, and run it exactly once more if the first one stopped
/// on a discard.
///
/// `io_stop`'s `Discard` arm unlinks the evicted file it stopped on, so by the
/// time the error arrives the cause is gone and a second attempt regenerates
/// it. That matters because a failed **open** build never reaches
/// `start_file_watching`: there is no watcher and no worker behind it, so
/// without this the user reads an error page that only re-opening the folder
/// clears.
///
/// Called from `build::run_pipeline` and not from inside `pipeline::run`,
/// because the retry needs a fresh `SlotResolver` — a `Box<dyn FnOnce>` that
/// `build_inner` consumes before any surviving Discard site can fire, and a
/// second build that reused it would ship pages with their `<!-- slot -->`
/// markers unresolved. `attempt` mints one per call.
///
/// It lives beside the flag rather than beside its caller so that both go when
/// the type does (see the module doc's last line).
///
/// Exactly once: a second consecutive discard means the removal did not settle
/// anything, so it stops the build like any other failure.
pub(crate) async fn retry_once_after_discard<T, A, F>(mut attempt: A) -> Result<T, String>
where
    A: FnMut() -> F,
    F: std::future::Future<Output = Result<T, BuildStopped>>,
{
    let outcome = match attempt().await {
        Err(stopped) if stopped.is_discarded() => {
            log::warn!(
                "build stopped on a file it discarded from the stage ({}) — building again",
                stopped.message()
            );
            attempt().await
        }
        first => first,
    };
    // The rest of moss speaks `String`; both verdicts have been read by here.
    outcome.map_err(BuildStopped::into_message)
}

/// The path's tail after its last `.moss` component, `/`-separated.
fn moss_relative_tail(path: &Path) -> Option<String> {
    let mut tail = None;
    let mut components = path.components();
    while let Some(c) = components.next() {
        if c.as_os_str() == ".moss" {
            tail = Some(components.clone().collect::<std::path::PathBuf>());
        }
    }
    Some(tail?.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
#[path = "outcome_tests.rs"]
mod tests;
