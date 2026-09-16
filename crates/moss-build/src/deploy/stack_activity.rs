//! Who is touching the OnionPress stack right now — asked across PROCESSES.
//!
//! Two moss binaries reach the same machine-scoped stack. The app installs it,
//! rotates its bundle, starts it and supervises it; `moss deploy` in a terminal
//! publishes to it. They are separate processes, so the only exclusion either
//! can trust is one the kernel keeps. This module is that exclusion, and it is
//! deliberately the whole of it: one object, one place, both questions.
//!
//! ## The two questions, and why one primitive cannot answer both
//!
//! **"Is someone rotating the app bundle?"** is a lock. The install pipeline
//! copies a fresh `OnionPress.app` to a scratch path and renames it into place
//! (`system::stack_install`), and its own justification for rotating rather
//! than deleting first is that a failed delete-then-rename leaves a running
//! stack with no binary to stop it. Anything that starts, quits or removes the
//! stack must therefore be excluded from that window — and when the holder
//! dies, the exclusion must end *immediately*, because a dead installer is
//! rotating nothing. That is exactly `flock`: held on the open file
//! description, released by the kernel when the process goes away.
//!
//! **"Did a publish just start?"** is a lease. It suppresses the supervisor's
//! recovery ladder so moss does not restart the stack out from under bytes
//! that are still being uploaded — and it must OUTLIVE the process that took
//! it, because a publish that dies leaves its half-uploaded generation behind
//! and because the verification window after a publish is part of the publish.
//! Equally, it must expire on its own: a publish that dies without telling
//! anyone must not disable recovery forever. That is a timestamp, and `flock`
//! is the wrong primitive for it in both directions.
//!
//! So: one [`StackActivity`], two files, because the two failure modes want
//! opposite answers to "what happens when the holder dies".
//!
//! ## Where the files live
//!
//! Siblings of the stack dir, never inside it. `onionpress_uninstall` does
//! `remove_dir_all(~/.moss/stacks/onionpress)`, so a lock file inside it is
//! deleted from under a live `flock` — after which two processes each hold a
//! lock on a different unlinked inode and neither can see the other. The
//! WordPress credentials file already sits one level up for the analogous
//! reason (`system::stack_install::wp_credentials`).
//!
//! ## What this is not
//!
//! It is not a second supervisor and it grants no new verb. The stack's own
//! launcher already holds a cross-process PID lock against a double `start`,
//! and that stays the owner of that race; ADR-050 (the plugin's, in
//! `plugins/onionpress/docs/decisions/`) still governs what moss may do at
//! all. This module only makes the exclusions moss ALREADY documented true
//! between processes instead of within one.

use std::path::{Path, PathBuf};
use std::time::Duration;

use fs2::FileExt;

use crate::system::stack_exec::layout::stacks_root;

/// How long a caller waits for the lock before being refused.
///
/// Generous, because the longest legitimate holder is a whole install — DMG
/// download, verify, copy, bring-up — and refusing a caller that would have
/// been served in a minute is worse than making it wait. Bounded, because the
/// holder is a live process that may itself be wedged, and a terminal publish
/// that hangs forever with no output is indistinguishable from a broken moss.
pub const LOCK_WAIT: Duration = Duration::from_secs(20 * 60);

/// The one stack channel moss knows how to acquire, which is also the deploy
/// plugin id that routes a publish to it. `system::stack_install` re-exports
/// it as `ONIONPRESS_CHANNEL_ID`.
pub const ONIONPRESS: &str = "onionpress";

/// How long after a publish starts the supervisor's ladder stays suppressed.
///
/// The number lives here, not beside the policy that reads it, because a
/// terminal publish and the app's supervisor are different processes and a
/// lease written with one window and read against another is a lease neither
/// side honours. `system::stack_serving` re-exports it.
pub const PUBLISH_LEASE: u64 = 600;

/// Poll interval while queued. Coarse: the wait is minutes, and every wake-up
/// is a syscall on a machine that is busy copying an app bundle.
const POLL: Duration = Duration::from_millis(200);

/// The record a holder writes so a waiter can say WHO it is waiting for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    pub pid: u32,
    /// What the holder is doing, in words a refusal can print.
    pub verb: String,
    /// Unix seconds when it took the lock.
    pub since: u64,
}

impl Holder {
    /// One line naming the holder, for the middle of a refusal sentence.
    fn describe(&self, now: u64) -> String {
        let ago = now.saturating_sub(self.since);
        format!("{} (pid {}, started {} ago)", self.verb, self.pid, human_secs(ago))
    }
}

fn human_secs(s: u64) -> String {
    if s < 90 {
        format!("{s}s")
    } else {
        format!("{}m", s / 60)
    }
}

/// The held lock. Releasing is `drop`, and so is dying.
#[derive(Debug)]
pub struct Held {
    file: std::fs::File,
    holder_path: PathBuf,
}

impl Drop for Held {
    fn drop(&mut self) {
        // The record is removed before the unlock, so a reader never sees a
        // pid that no longer holds anything. Best-effort either way: an
        // unreadable record only costs a refusal its name.
        let _ = std::fs::remove_file(&self.holder_path);
        let _ = FileExt::unlock(&self.file);
    }
}

/// One machine's stack activity, rooted at a stacks dir.
#[derive(Debug, Clone)]
pub struct StackActivity {
    root: PathBuf,
}

impl StackActivity {
    /// The machine's own, at [`stacks_root`].
    pub fn machine() -> Result<Self, String> {
        Ok(Self::at(stacks_root()?))
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// `<stacks>/.stack-activity.lock`
    pub fn lock_path(&self) -> PathBuf {
        self.root.join(".stack-activity.lock")
    }

    /// `<stacks>/.stack-activity.holder` — who holds the lock, OFF the locked
    /// file: Windows' mandatory byte-range lock blocks a cross-handle READ of
    /// a file this handle holds locked, not only a second attempt to lock it.
    pub fn holder_path(&self) -> PathBuf {
        self.root.join(".stack-activity.holder")
    }

    /// `<stacks>/.publish-lease`
    pub fn lease_path(&self) -> PathBuf {
        self.root.join(".publish-lease")
    }

    fn open_lock(&self) -> Result<std::fs::File, String> {
        std::fs::create_dir_all(&self.root)
            .map_err(|e| format!("create {}: {e}", self.root.display()))?;
        // allow:raw_write the lock file is machine state under ~/.moss/stacks, not build output, and flock needs a persistent open descriptor
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.lock_path())
            .map_err(|e| format!("open {}: {e}", self.lock_path().display()))
    }

    /// Take the lock, announcing the wait through `queued` if (and only if)
    /// there is one, and refusing after [`LOCK_WAIT`].
    ///
    /// `verb` is a present participle a refusal can print — "installing the
    /// OnionPress stack" — and it is written into the lock file so the NEXT
    /// caller's refusal can name it.
    pub fn hold(
        &self,
        verb: &str,
        wait: Duration,
        mut queued: impl FnMut(),
    ) -> Result<Held, String> {
        let file = self.open_lock()?;
        let deadline = std::time::Instant::now() + wait;
        let mut announced = false;
        loop {
            match file.try_lock_exclusive() {
                Ok(()) => break,
                // fs2's WOULD_BLOCK-shaped error, not a hardcoded ErrorKind:
                // on Windows a contended try-lock reports ERROR_LOCK_VIOLATION,
                // which rustc's own error-kind table never maps to WouldBlock.
                Err(e) if e.kind() != fs2::lock_contended_error().kind() => {
                    return Err(format!("lock {}: {e}", self.lock_path().display()));
                }
                Err(_) => {
                    if !announced {
                        queued();
                        announced = true;
                    }
                    if std::time::Instant::now() >= deadline {
                        return Err(self.busy_message(verb));
                    }
                    std::thread::sleep(POLL);
                }
            }
        }
        // Off the locked file: Windows' mandatory byte-range lock blocks a
        // cross-handle READ of a file this handle holds locked, not only a
        // second lock attempt, so a waiter naming the holder in
        // `busy_message` needs its own, unlocked sibling file.
        let _ = self.write_holder(std::process::id(), unix_secs(), verb);
        Ok(Held { file, holder_path: self.holder_path() })
    }

    /// What a caller is told when the wait ran out. Names the holder and the
    /// one thing the reader can do about it — this reaches a terminal author
    /// as the whole explanation, so "resource busy" is not enough.
    fn busy_message(&self, verb: &str) -> String {
        let now = unix_secs();
        match self.holder() {
            Some(h) => format!(
                "cannot start {verb}: another moss process is already {} \
                 — wait for it to finish, or quit that moss, then try again",
                h.describe(now)
            ),
            None => format!(
                "cannot start {verb}: another moss process holds the stack lock at {} \
                 — wait for it to finish, or quit that moss, then try again",
                self.lock_path().display()
            ),
        }
    }

    /// Is anything holding the lock — an install, an uninstall, a recovery?
    ///
    /// Acquire-and-release rather than a read of the record: the record is
    /// advisory and can be stale, while the lock cannot be. `false` when the
    /// file is absent, which is the same answer as "nothing has ever installed
    /// a stack here" — and asked WITHOUT creating it, because the supervisor
    /// asks on every tick and a machine that publishes nowhere near OnionPress
    /// should not grow a lock file for a stack it does not have.
    pub fn busy(&self) -> bool {
        // allow:raw_read machine state under ~/.moss/stacks, not vault input
        let Ok(file) = std::fs::File::open(self.lock_path()) else {
            return false;
        };
        match file.try_lock_exclusive() {
            Ok(()) => {
                let _ = FileExt::unlock(&file);
                false
            }
            Err(_) => true,
        }
    }

    /// Who says they hold it. Advisory: read WITHOUT the lock, because the
    /// only caller is a refusal that has already failed to get it.
    pub fn holder(&self) -> Option<Holder> {
        // allow:raw_read machine state under ~/.moss/stacks, not vault input
        parse_holder(&std::fs::read_to_string(self.holder_path()).ok()?)
    }

    /// The holder record, off the locked file (see `holder_path`).
    fn write_holder(&self, pid: u32, since: u64, verb: &str) -> Result<(), String> {
        self.write_atomic(&self.holder_path(), &format!("{pid} {since} {verb}"))
    }

    /// A publish is starting (or its verification is still running): suppress
    /// the supervisor's ladder until `now + lease`.
    ///
    /// Monotonic in the only direction that matters — a shorter lease never
    /// shortens a longer one already on disk, so two overlapping publishes
    /// cannot leave the stack unprotected. Best-effort: a lease that cannot be
    /// written costs the publish its protection, not its bytes, so it is a log
    /// line and not a failure.
    pub fn note_publish(&self, now: u64, lease: u64) {
        let until = now + lease;
        if self.publish_lease_until() >= until {
            return;
        }
        if let Err(e) = self.write_lease(until) {
            log::warn!(target: "moss::deploy", "could not record the publish lease: {e}");
        }
    }

    fn write_lease(&self, until: u64) -> Result<(), String> {
        self.write_atomic(&self.lease_path(), &until.to_string())
    }

    /// tmp-then-rename into `path`, so a reader never parses a half-written
    /// file — shared by `write_holder` and `write_lease`, whose only
    /// difference was the file and the contents.
    fn write_atomic(&self, path: &Path, contents: &str) -> Result<(), String> {
        std::fs::create_dir_all(&self.root)
            .map_err(|e| format!("create {}: {e}", self.root.display()))?;
        // `.tmp` appended to the OsStr, not `with_extension` — these filenames already carry a dot (`.stack-activity.holder`), which `with_extension` would eat instead of extend.
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        // allow:raw_write machine state under ~/.moss/stacks, never inside a vault; the tmp is freshly minted and renamed into place below
        std::fs::write(&tmp, contents).map_err(|e| format!("write {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("rename {}: {e}", path.display()))
    }

    /// Unix seconds the current publish lease runs to, or 0 if there is none.
    /// Any unreadable or unparsable file is 0 — the safe direction, because 0
    /// means "recovery is allowed", i.e. the behaviour moss had before a lease
    /// existed at all.
    pub fn publish_lease_until(&self) -> u64 {
        // allow:raw_read machine state under ~/.moss/stacks, not vault input
        std::fs::read_to_string(self.lease_path())
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0)
    }
}

fn parse_holder(raw: &str) -> Option<Holder> {
    let mut parts = raw.trim().splitn(3, ' ');
    let pid = parts.next()?.parse().ok()?;
    let since = parts.next()?.parse().ok()?;
    let verb = parts.next()?.trim();
    (!verb.is_empty()).then(|| Holder { pid, verb: verb.to_string(), since })
}

/// Unix seconds. Wall clock deliberately: every reader of these two files is a
/// different process, so a process-relative clock cannot be compared.
pub fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "stack_activity_tests.rs"]
mod tests;
