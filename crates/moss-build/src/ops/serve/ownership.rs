//! Which process, on this machine, is serving a given folder right now.
//!
//! Two moss processes can be pointed at the same site folder — the desktop
//! app and a CLI build an editor plugin spawns, say — and until this module
//! existed neither could tell. Each one mints its own token, binds its own
//! port, and the second simply shadows the first's work with no way for
//! anything to notice. This is the record that lets a second process find
//! the first instead of racing it: one owner per folder per machine,
//! advertised the moment a server binds and retracted the moment it stops.
//!
//! Two files per folder, both named by [`folder_id`] (the same hash
//! [`crate::infra::folder_lock`] already uses for its cross-process build
//! lock, so a folder has one identity everywhere this crate names one):
//!
//! * `<moss_home>/locks/<folder-id>.owner` — an `flock`. Held for exactly as
//!   long as the owning server runs; the kernel clears it the instant the
//!   holding process dies, by a crash or by `kill -9`, with no cleanup step
//!   required. This is the authoritative half: whoever holds the lock is the
//!   owner, full stop.
//! * `<moss_home>/servers/<folder-id>.json` — an [`OwnerRecord`] naming the
//!   current (or, if the owner crashed, the last) holder: its host kind,
//!   pid, version, and the URL a client can reach it at. This is the
//!   advisory half — stale the moment its writer dies — which is why
//!   [`find_live_owner`] never trusts it alone and always confirms it
//!   against the URL's own health endpoint.
//!
//! [`acquire`] writes both; [`find_live_owner`] reads only the record and
//! verifies it over HTTP. A caller that wants the authoritative answer
//! (can I serve this folder myself?) calls [`acquire`]; a caller that wants
//! to know who else might be serving it (should I show the user a link
//! instead of starting my own server?) calls [`find_live_owner`].

use std::path::{Path, PathBuf};

use fs2::FileExt;

use crate::infra::atomic_write;
use crate::infra::folder_lock::folder_id;
use crate::infra::home::moss_home;

/// Which kind of process holds a folder's ownership record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum HostKind {
    /// The desktop app.
    Desktop,
    /// `moss build <folder> --serve` (moss-cli or the app binary's headless
    /// interception — the two are byte-identical, so neither needs its own
    /// kind).
    Cli,
    /// A future standalone `moss serve`-style long-running host, distinct
    /// from a one-shot CLI build. Unused by any caller yet; named now so
    /// [`OwnerRecord`]'s `kind` field never needs a breaking enum change to
    /// add it later.
    Server,
}

/// One process's claim on one folder: who, running what, reachable where,
/// since when.
///
/// `folder` is stored canonicalized (the same path [`folder_id`] hashed),
/// so a reader never has to re-resolve it to know what this record is
/// about.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OwnerRecord {
    pub folder: PathBuf,
    pub pid: u32,
    pub kind: HostKind,
    pub version: String,
    pub url: String,
    /// ISO-8601, [`crate::system::now_iso`]'s form — the convention this
    /// crate already uses for a persisted record's timestamp (the recents
    /// list, the registry cache).
    pub started_at: String,
}

/// `<moss_home>/servers/<folder-id>.json` — where [`acquire`] writes and
/// [`find_live_owner`] reads a folder's [`OwnerRecord`].
fn record_path(folder: &Path) -> Result<PathBuf, String> {
    Ok(moss_home()?.join("servers").join(format!("{}.json", folder_id(folder))))
}

/// `<moss_home>/locks/<folder-id>.owner` — the `flock` [`acquire`] takes.
/// Sibling to, but distinct from, `infra::folder_lock`'s `.build` lock: a
/// build and the server reading its output are different scopes (a build
/// can run against a folder nobody is serving, and the server keeps running
/// long after any one build finishes), so they must not contend with each
/// other.
fn owner_lock_path(folder: &Path) -> Result<PathBuf, String> {
    Ok(moss_home()?.join("locks").join(format!("{}.owner", folder_id(folder))))
}

/// Returned by [`acquire`] when another live-or-crashed process already
/// holds the folder's owner lock: the record it last wrote, read back for
/// the caller to report (and, for a crashed holder with a since-released
/// lock, overwritten by this same call — see [`acquire`]'s doc).
#[derive(Debug)]
pub struct ExistingOwner(pub OwnerRecord);

/// Folders this PROCESS currently holds the owner lock for, as a weak
/// reference to the open file each [`OwnerGuard`] for that folder shares.
///
/// `flock` is scoped to an open file description, not to a process: opening
/// the SAME lock path a second time and calling `try_lock_exclusive` on the
/// new handle conflicts with the first even from the SAME process. That is
/// the right behavior for a second OS process, which is what this module
/// exists to catch — but a single process legitimately runs two servers
/// against one vault for a moment during a lifecycle handoff (and
/// `the_seal_tail_leaves_the_served_staging_tree_alone` exercises exactly
/// that in-process shape). This table is the fast path for that case: a
/// second `acquire` for a folder THIS process already holds upgrades the
/// weak reference and reuses the held file instead of attempting — and
/// losing — a second lock against itself.
///
/// `Weak`, not a strong `Arc` plus a manual count: the `Arc`'s own strong
/// count already IS that count, kept correct by every `OwnerGuard`'s own
/// `Clone`/`Drop` instead of by a second number this map would have to keep
/// in step with it by hand.
static HELD: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, std::sync::Weak<std::fs::File>>>> =
    std::sync::OnceLock::new();

fn held() -> &'static std::sync::Mutex<std::collections::HashMap<String, std::sync::Weak<std::fs::File>>> {
    HELD.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// The held owner lock and the record it wrote. Dropping it releases the
/// lock (the kernel does this itself, even on a hard kill — see the module
/// doc) and best-effort removes the record file once the LAST guard for this
/// folder (in this process — see [`HELD`]) drops, so a clean shutdown leaves
/// no stale advertisement behind for [`find_live_owner`] to have to notice
/// is live or confirm is not.
#[derive(Debug)]
pub struct OwnerGuard {
    /// `None` when ownership could not be tracked at all (home directory
    /// unresolvable, lock file unopenable) — see [`acquire`]'s fallback.
    /// Held only to keep the `flock` alive via the open file description;
    /// never read after construction.
    _lock: Option<std::sync::Arc<std::fs::File>>,
    /// Same key [`HELD`] uses. `None` alongside `_lock: None` for the
    /// untracked fallback — nothing to decrement.
    folder_id: Option<String>,
    record_path: Option<PathBuf>,
}

impl Drop for OwnerGuard {
    fn drop(&mut self) {
        let Some(id) = &self.folder_id else { return };
        // Held across the strong-count check AND the removal, so a
        // concurrent `acquire` cannot upgrade this same folder's `Weak`
        // between this guard deciding it is the last one and actually
        // removing the entry — `acquire`'s own fast path takes this same
        // lock before it calls `upgrade`.
        let mut table = held().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let is_last = self._lock.as_ref().is_some_and(|file| std::sync::Arc::strong_count(file) == 1);
        if is_last {
            table.remove(id);
            drop(table);
            if let Some(path) = &self.record_path {
                // allow:unlink removes this folder's owner record under moss_home, never a served staging path
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

/// Try to become `folder`'s recorded owner.
///
/// Takes the owner lock WITHOUT blocking — a serve session has no
/// legitimate reason to wait for one; if the folder is already owned, the
/// caller's job is to say so, not to queue behind it like a second build
/// does for `infra::folder_lock`'s build lock.
///
/// * **Lock free** (nobody holds it, including a holder that has since
///   crashed — the kernel released the `flock` the moment that process
///   died, whether or not it ever wrote a record): take it, write a fresh
///   [`OwnerRecord`], return the guard. A stale record from a crashed
///   holder is simply overwritten; nothing has to notice it was stale
///   first, because the lock being free already answers that question.
/// * **Lock held**: read the current record and return it as
///   [`ExistingOwner`], for the caller to report.
///
/// Resolving `moss_home()` or opening the lock file can fail (an
/// unresolvable home directory, a permissions error) independently of
/// contention. That failure has no slot in this function's two-outcome
/// contract, so it degrades the same way `InvokeCtx::bind`'s record publish
/// already does elsewhere in this module family: log it and proceed
/// untracked, rather than making ownership tracking a reason a folder can't
/// be served at all.
pub fn acquire(folder: &Path, kind: HostKind, version: String, url: String) -> Result<OwnerGuard, ExistingOwner> {
    let canonical = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    let id = folder_id(&canonical);
    let record_path_for_id = record_path(&canonical).ok();

    // In-process fast path first — see `HELD`'s doc for why a second `flock`
    // attempt against our own already-held one would otherwise lose. Falls
    // through to a fresh acquire on a stale/missing weak reference, which
    // the same lock discipline in `OwnerGuard`'s `Drop` makes only a
    // defensive case, never one this path actually needs to hit.
    {
        let table = held().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(file) = table.get(&id).and_then(std::sync::Weak::upgrade) {
            return Ok(OwnerGuard { _lock: Some(file), folder_id: Some(id), record_path: record_path_for_id });
        }
    }

    let lock_path = match owner_lock_path(&canonical) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("could not resolve the owner-lock path for {}: {e}", canonical.display());
            return Ok(OwnerGuard { _lock: None, folder_id: None, record_path: None });
        }
    };
    if let Some(parent) = lock_path.parent() {
        // allow:raw_write ~/.moss/locks is machine state under moss_home, not build output
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::warn!("could not create {}: {e}", parent.display());
            return Ok(OwnerGuard { _lock: None, folder_id: None, record_path: None });
        }
    }
    // allow:raw_write the lock file is machine state under ~/.moss/locks, not build output
    let file = match std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&lock_path) {
        Ok(f) => f,
        Err(e) => {
            log::warn!("could not open {}: {e}", lock_path.display());
            return Ok(OwnerGuard { _lock: None, folder_id: None, record_path: None });
        }
    };

    match file.try_lock_exclusive() {
        Ok(()) => {
            let record = OwnerRecord {
                folder: canonical.clone(),
                pid: std::process::id(),
                kind,
                version,
                url,
                started_at: crate::system::now_iso(),
            };
            // `record_path_for_id` was resolved from the same `moss_home()`
            // call `owner_lock_path` already succeeded with above, so `None`
            // here in practice never happens alongside a successful lock —
            // handled anyway rather than assumed.
            if let Some(path) = &record_path_for_id {
                if let Err(e) = atomic_write::write_json_atomic(path, &record) {
                    log::warn!("could not persist the owner record at {}: {e}", path.display());
                }
            }
            let file = std::sync::Arc::new(file);
            held()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(id.clone(), std::sync::Arc::downgrade(&file));
            Ok(OwnerGuard { _lock: Some(file), folder_id: Some(id), record_path: record_path_for_id })
        }
        // fs2's WOULD_BLOCK-shaped error, not a hardcoded ErrorKind — see
        // `deploy::stack_activity::hold` / `infra::folder_lock::acquire` for
        // why a contended try-lock's error kind is platform-dependent.
        Err(e) if e.kind() == fs2::lock_contended_error().kind() => {
            Err(ExistingOwner(read_record(&canonical).unwrap_or_else(|| OwnerRecord {
                folder: canonical.clone(),
                pid: 0,
                kind: HostKind::Server,
                version: "unknown".to_string(),
                url: String::new(),
                started_at: String::new(),
            })))
        }
        Err(e) => {
            log::warn!("could not lock {}: {e}", lock_path.display());
            Ok(OwnerGuard { _lock: None, folder_id: None, record_path: None })
        }
    }
}

/// Where [`acquire`] writes `folder`'s owner record. `pub` (not
/// `#[cfg(test)]`) solely so `moss-cli/tests/folder_owner_record_test.rs` — a
/// separate crate, which cannot see this crate's own test-only items — can
/// assert on the record file's existence directly: [`find_live_owner`] alone
/// cannot distinguish "no file ever written" from "a file naming a now-dead
/// process," which is exactly the distinction that test makes after killing
/// a server to simulate a crash.
pub fn record_path_for_test(folder: &Path) -> PathBuf {
    let canonical = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    record_path(&canonical).unwrap_or_else(|e| panic!("could not resolve a test record path: {e}"))
}

fn read_record(folder: &Path) -> Option<OwnerRecord> {
    let path = record_path(folder).ok()?;
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Is `folder` served, right now, by a process this machine can still
/// reach? Reads the folder's [`OwnerRecord`] if one exists, then confirms it
/// by probing the record's own `url` at [`super::port::MOSS_HEALTH_PATH`]:
/// a record alone is never enough, because its writer could have crashed
/// (the lock already answers that, but reading the lock state from outside
/// the holding process is not portable) or — same process, same lock,
/// different folder now — been re-pointed at another folder without
/// releasing this one's lock, which a later folder-switch change may do.
///
/// The one HTTP check this performs reuses [`super::port::blocking_get`],
/// the exact GET [`super::port::verify_server_ready`] already makes — not
/// the whole of that function, because the two probes disagree on retry
/// policy as much as on what they're checking: `verify_server_ready` polls
/// for up to a second while a server it already decided to wait for is
/// still binding, where this is a single-shot "does anyone answer at all
/// right now" used for discovery, not for a server this caller is itself
/// starting.
///
/// `None` for every negative: no record, the record's process gone quiet,
/// a 4xx/5xx, a body with no `folder_id` (an older moss, schema 1 — the
/// schema's own doc comment calls for exactly this `>=` gate rather than a
/// hard equality, so a newer schema 3 reader still accepts this one), or a
/// `folder_id` that names a different folder (the same owner process, moved
/// on).
pub async fn find_live_owner(folder: &Path) -> Option<OwnerRecord> {
    let canonical = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    let record = read_record(&canonical)?;
    let url = format!("{}{}", record.url, super::port::MOSS_HEALTH_PATH);
    let body = super::port::blocking_get(url).await.ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&body).ok()?;
    let schema = parsed.get("schema").and_then(serde_json::Value::as_u64).unwrap_or(0);
    if schema < 2 {
        return None;
    }
    let seen_folder_id = parsed.get("folder_id").and_then(serde_json::Value::as_str)?;
    if seen_folder_id == folder_id(&canonical) {
        Some(record)
    } else {
        None
    }
}

/// The one line the CLI names an existing owner with — shared so a later
/// standby mode reports the same owner the same way rather than growing a
/// second wording.
pub fn already_served_message(owner: &OwnerRecord) -> String {
    format!(
        "this folder is already served at {} by {:?} (pid {}, moss {})",
        owner.url, owner.kind, owner.pid, owner.version
    )
}

/// The `moss build --serve` exit code for "this folder already has a live
/// owner" — distinct from 0 (success) and 1 (every other build failure), so
/// a caller can tell the two apart without parsing stderr.
pub const ALREADY_SERVED_EXIT_CODE: i32 = 3;

/// The vault `site_dir` currently resolves to — a folder's identity for
/// ownership purposes is its vault root, not whichever generation the cell
/// happens to be pointing a server at right now. [`acquire_for_site_dir`]
/// resolves it once up front and passes the result on to
/// [`wait_for_owner_to_leave`], which takes the already-resolved path rather
/// than re-resolving it itself.
fn resolve_vault_folder(site_dir: &std::sync::Arc<std::sync::RwLock<PathBuf>>) -> Option<PathBuf> {
    site_dir
        .read()
        .ok()
        .and_then(|dir| crate::vault::paths::VaultRoot::find_containing(&dir).map(|v| v.path().to_path_buf()))
}

/// [`acquire`] for whichever folder `site_dir` currently resolves to, for
/// `start_server` to call once it knows the port it bound. Lives here rather
/// than inline in `router.rs` so that file stays about routing: this is the
/// one place that needs the vault-from-site_dir resolution and the acquire
/// call together, and `router.rs` just reacts to the outcome.
///
/// `Ok(None)` when `site_dir` resolves to no vault — not every headless case
/// is vault-rooted, same as `InvokeCtx::bind` already documents, and such a
/// server is simply not discoverable by folder.
///
/// `standby_on_conflict` is the one-shot `moss build --serve` refusal's
/// opposite: a `--serve --watch` child (an editor plugin's long-lived
/// handle on the preview, say) has no reason to give up just because another
/// process is serving this folder right now. Set, a conflict reports the
/// existing owner's URL on stderr in the same wording a normal bind prints
/// ([`preview_ready_line`]) plus [`standing_by_message`], then
/// waits for that owner to leave ([`wait_for_owner_to_leave`]) and retries —
/// repeatedly, since the folder could be re-claimed by yet another process in
/// the gap between "owner gone" and this retry. Unset (the default, and every
/// `--serve` without `--watch`), a conflict is [`already_served_message`],
/// ready to hand back as the reason the server never started — the original
/// no-`--watch` conflict behaviour, unchanged.
pub async fn acquire_for_site_dir(
    site_dir: &std::sync::Arc<std::sync::RwLock<PathBuf>>,
    kind: HostKind,
    version: String,
    url: String,
    standby_on_conflict: bool,
) -> Result<Option<OwnerGuard>, String> {
    let Some(folder) = resolve_vault_folder(site_dir) else {
        return Ok(None);
    };
    loop {
        match acquire(&folder, kind, version.clone(), url.clone()) {
            Ok(guard) => return Ok(Some(guard)),
            Err(ExistingOwner(existing)) => {
                if !standby_on_conflict {
                    return Err(already_served_message(&existing));
                }
                crate::cli_eprintln!("{}", preview_ready_line(&existing.url));
                crate::cli_eprintln!("{}", standing_by_message(&existing));
                wait_for_owner_to_leave(&folder).await;
            }
        }
    }
}

/// The line a build names its own preview URL with, once a server is up.
/// Reused verbatim by [`acquire_for_site_dir`]'s standby branch, which
/// reports an owner's URL without having just run a build of its own, so the
/// two call sites can't drift into two different "here's where to look"
/// wordings.
pub fn preview_ready_line(url: &str) -> String {
    format!("🌐 Preview server ready! Access at {}", url)
}

/// The one line a standby takeover names the owner it is waiting on with —
/// printed once, right after [`preview_ready_line`] reports that owner's
/// URL, by [`acquire_for_site_dir`]'s standby branch.
pub fn standing_by_message(owner: &OwnerRecord) -> String {
    format!("served by {:?} (pid {}, moss {}); standing by", owner.kind, owner.pid, owner.version)
}

/// Block until `folder` has no live owner. Polls [`find_live_owner`] on a
/// plain fixed interval — this is a standby wait behind a long-running
/// `--watch` child, not a latency-sensitive probe, so there is no backoff or
/// event source to wire up: the folder's owner leaving is a rare event (a
/// crash, or the desktop app closing the vault) and two seconds of staleness
/// on noticing it costs nothing a human would feel.
pub async fn wait_for_owner_to_leave(folder: &Path) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        if find_live_owner(folder).await.is_none() {
            return;
        }
    }
}

/// Wait up to `timeout` for `folder` to GET a live owner — the reverse wait
/// of [`wait_for_owner_to_leave`], for a process that just honored a yield
/// and must not simply reacquire the lock it just freed. A folder is briefly
/// ownerless right after a yield, and reacquiring immediately (the lock IS
/// free) would make the yielder its own next owner, defeating the whole
/// point: this wait is what leaves the window open for someone else — the
/// caller `/__moss/yield` exists for — to claim it first. A record naming
/// THIS process's own pid is ignored rather than accepted: the `OwnerGuard`
/// that erases our previous record only drops once the server we just told
/// to shut down actually finishes doing so, so the very first poll can still
/// read our own, about-to-vanish record.
///
/// `None` if nobody else claims the folder before `timeout` elapses — a
/// yield is a courtesy, not a guarantee, and the caller falls back to
/// resuming service itself rather than waiting forever for a requester who
/// may have gone away. Same fixed 2-second poll as `wait_for_owner_to_leave`
/// otherwise, capped so the final wait never overshoots `timeout`.
pub async fn wait_for_owner_to_appear(folder: &Path, timeout: std::time::Duration) -> Option<OwnerRecord> {
    let own_pid = std::process::id();
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Some(owner) = find_live_owner(folder).await {
            if owner.pid != own_pid {
                return Some(owner);
            }
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return None;
        }
        tokio::time::sleep(remaining.min(std::time::Duration::from_secs(2))).await;
    }
}

/// The one line a `--serve` process prints the moment it honors a
/// `/__moss/yield` request, before it drops its own ownership — shared so
/// the `--watch` standby branch and the one-shot exit-0 branch of
/// `ops::run_headless_build` cannot drift into two different wordings.
pub fn yielded_message(folder: &str) -> String {
    format!("Yielding ownership of {folder}")
}

/// What [`request_yield`] learned from asking a folder's live owner to give
/// it up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YieldOutcome {
    /// The owner admitted the request (202) and will drop ownership shortly.
    Accepted,
    /// The owner is a [`HostKind::Desktop`] host, which a yield never moves.
    RefusedDesktop,
    /// Nobody is recorded as serving `folder` right now — nothing to yield.
    NoOwner,
}

/// Ask whichever process currently serves `folder` to give it up over
/// `POST /__moss/yield`, so a fuller engine elsewhere on the machine (the
/// desktop app) can take the folder over. Reads the live owner's record
/// (for its URL), then the vault's
/// `.moss/build.nosync/http-token` (for the same bearer token the carrier
/// checks — see [`super::carrier_token`]), and reports what the owner
/// answered rather than assuming the request landed.
pub async fn request_yield(folder: &Path) -> Result<YieldOutcome, String> {
    let canonical = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    let Some(owner) = find_live_owner(&canonical).await else {
        return Ok(YieldOutcome::NoOwner);
    };
    let vault = crate::vault::paths::VaultRoot::find_containing(&canonical)
        .ok_or_else(|| format!("{} is not inside a vault", canonical.display()))?;
    let token_path = super::carrier_token::token_path(vault.path());
    let token = std::fs::read_to_string(&token_path)
        .map_err(|e| format!("could not read the HTTP carrier token at {}: {e}", token_path.display()))?;
    let url = format!("{}/__moss/yield", owner.url);
    let post_url = url.clone();
    let token = token.trim().to_string();
    let result = tokio::task::spawn_blocking(move || {
        ureq::post(&post_url)
            .set(super::carrier_token::TOKEN_HEADER, &token)
            .timeout(std::time::Duration::from_secs(5))
            .send_string("")
    })
    .await
    .map_err(|e| format!("yield request to {url} panicked: {e}"))?;
    match result {
        Ok(resp) if resp.status() == 202 => Ok(YieldOutcome::Accepted),
        Ok(resp) => Err(format!("unexpected response from {url}: {}", resp.status())),
        Err(ureq::Error::Status(409, _)) => Ok(YieldOutcome::RefusedDesktop),
        Err(e) => Err(format!("yield request to {url} failed: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moss_home_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir for MOSS_HOME")
    }

    /// Serializes the handful of tests here that set `MOSS_HOME` — the same
    /// process-global hazard `infra::home`'s own test documents.
    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::infra::home::MOSS_HOME_TEST_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The timeout fallback: nobody ever claims the folder (an empty,
    /// freshly-made `MOSS_HOME` has no record for it at all), so the wait
    /// must give up and return `None` rather than hang — the ablation target
    /// for the yield arm's "serve it myself again" fallback.
    #[tokio::test]
    async fn wait_for_owner_to_appear_gives_up_and_returns_none_within_the_timeout() {
        let _guard = env_guard();
        let home = moss_home_dir();
        std::env::set_var("MOSS_HOME", home.path());
        let folder = tempfile::tempdir().expect("tempdir for the unclaimed folder");

        let timeout = std::time::Duration::from_millis(300);
        let started = std::time::Instant::now();
        let result = wait_for_owner_to_appear(folder.path(), timeout).await;
        let elapsed = started.elapsed();

        assert!(result.is_none(), "nobody claimed the folder — the wait must report that, not invent an owner");
        assert!(
            elapsed < timeout + std::time::Duration::from_secs(2),
            "the wait must return at or shortly after its timeout, not hang; took {elapsed:?}"
        );
        std::env::remove_var("MOSS_HOME");
    }

    #[test]
    fn acquire_writes_a_record_that_round_trips_through_the_atomic_writer() {
        let _guard = env_guard();
        let home = moss_home_dir();
        std::env::set_var("MOSS_HOME", home.path());
        let folder = tempfile::tempdir().expect("tempdir for the served folder");

        let owned = acquire(folder.path(), HostKind::Cli, "9.9.9".to_string(), "http://127.0.0.1:1234".to_string())
            .expect("nothing else holds this fresh folder's lock");

        let canonical = std::fs::canonicalize(folder.path()).unwrap();
        let path = record_path(&canonical).unwrap();
        let on_disk: OwnerRecord = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(on_disk.folder, canonical);
        assert_eq!(on_disk.pid, std::process::id());
        assert_eq!(on_disk.kind, HostKind::Cli);
        assert_eq!(on_disk.version, "9.9.9");
        assert_eq!(on_disk.url, "http://127.0.0.1:1234");
        assert!(!on_disk.started_at.is_empty());

        drop(owned);
        assert!(!path.exists(), "dropping the guard must remove the record it wrote");
        std::env::remove_var("MOSS_HOME");
    }

    /// A second `acquire` for a folder THIS process already owns must
    /// succeed, not conflict — `flock` is scoped to an open file
    /// description, so without the in-process fast path a second server in
    /// one process pointed at a vault it is already serving would lose a
    /// lock fight against itself (the regression
    /// `the_seal_tail_leaves_the_served_staging_tree_alone`, which legitimately
    /// runs two servers against one vault mid-lifecycle-swap, caught when
    /// this module first landed).
    #[test]
    fn a_second_acquire_in_the_same_process_succeeds_and_keeps_the_first_record() {
        let _guard = env_guard();
        let home = moss_home_dir();
        std::env::set_var("MOSS_HOME", home.path());
        let folder = tempfile::tempdir().expect("tempdir for the served folder");

        let first =
            acquire(folder.path(), HostKind::Desktop, "1.2.3".to_string(), "http://127.0.0.1:4321".to_string())
                .expect("first acquire must succeed");
        let second = acquire(folder.path(), HostKind::Cli, "1.2.3".to_string(), "http://127.0.0.1:9999".to_string())
            .expect("a second acquire from the SAME process must not conflict with itself");

        let canonical = std::fs::canonicalize(folder.path()).unwrap();
        let on_disk: OwnerRecord =
            serde_json::from_str(&std::fs::read_to_string(record_path(&canonical).unwrap()).unwrap()).unwrap();
        assert_eq!(on_disk.kind, HostKind::Desktop, "the first acquire's record stays canonical while it still holds");

        drop(first);
        assert!(record_path(&canonical).unwrap().exists(), "the record survives while `second` still holds");
        drop(second);
        assert!(!record_path(&canonical).unwrap().exists(), "only the LAST guard removes the record");
        std::env::remove_var("MOSS_HOME");
    }

    /// The real cross-process shape: a lock held by a file THIS process
    /// never opened through `acquire` (standing in for a different `moss`
    /// process) must be reported, not silently granted. This is the ablation
    /// target for the owner-lock try-acquire: skip or short-circuit it and a
    /// second SEPARATE process's `moss build --serve` proceeds instead of
    /// being refused — `folder_owner_record_test.rs`'s real-process test
    /// catches that shape directly; this unit test pins the same contended
    /// branch without needing to spawn one.
    #[test]
    fn acquire_is_refused_by_a_lock_held_outside_this_process_registry() {
        let _guard = env_guard();
        let home = moss_home_dir();
        std::env::set_var("MOSS_HOME", home.path());
        let folder = tempfile::tempdir().expect("tempdir for the served folder");
        let canonical = std::fs::canonicalize(folder.path()).unwrap();

        // Lock the exact file `acquire` would, but through a handle `HELD`
        // never learns about — the in-process fast path must NOT treat this
        // as "already ours."
        let lock_path = owner_lock_path(&canonical).unwrap();
        std::fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
        let foreign = std::fs::OpenOptions::new().read(true).write(true).create(true).open(&lock_path).unwrap();
        foreign.try_lock_exclusive().expect("the foreign handle must win the uncontended lock");
        let foreign_record = OwnerRecord {
            folder: canonical.clone(),
            pid: 777,
            kind: HostKind::Desktop,
            version: "7.0.0".to_string(),
            url: "http://127.0.0.1:7777".to_string(),
            started_at: crate::system::now_iso(),
        };
        atomic_write::write_json_atomic(&record_path(&canonical).unwrap(), &foreign_record).unwrap();

        let result = acquire(folder.path(), HostKind::Cli, "1.2.3".to_string(), "http://127.0.0.1:9999".to_string());
        let ExistingOwner(existing) = result.expect_err("a lock held outside this process's own registry must refuse");
        assert_eq!(existing.kind, HostKind::Desktop);
        assert_eq!(existing.pid, 777);
        assert_eq!(existing.url, "http://127.0.0.1:7777");

        drop(foreign);
        std::env::remove_var("MOSS_HOME");
    }

    /// The ablation target for `find_live_owner`'s folder-identity check:
    /// a record whose advertised URL answers, but for a DIFFERENT folder
    /// than the one the caller asked about, must read as unowned — a fake
    /// local listener stands in for "some other moss process, somewhere
    /// else, happens to be using this same loopback port right now."
    #[tokio::test]
    async fn find_live_owner_is_none_when_the_health_body_names_a_different_folder() {
        let _guard = env_guard();
        let home = moss_home_dir();
        std::env::set_var("MOSS_HOME", home.path());
        let folder = tempfile::tempdir().expect("tempdir for the served folder");
        let canonical = std::fs::canonicalize(folder.path()).unwrap();

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind a fake health server");
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).expect("set_nonblocking");
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_bg = stop.clone();
        let server = std::thread::spawn(move || {
            while !stop_bg.load(std::sync::atomic::Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        use std::io::{Read, Write};
                        let mut buf = [0u8; 1024];
                        let _ = stream.read(&mut buf);
                        // A well-formed schema-2 body, but for a folder id
                        // that can never match a real canonicalized path's
                        // hash: the mismatch this test exists to exercise.
                        let body = "{\"server\":\"moss-preview-server\",\"version\":\"x\",\"preview\":true,\"schema\":2,\"folder_id\":\"not-the-right-folder\",\"pid\":1}";
                        let response = format!(
                            "HTTP/1.0 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(response.as_bytes());
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        let record = OwnerRecord {
            folder: canonical.clone(),
            pid: 4242,
            kind: HostKind::Cli,
            version: "0.0.0".to_string(),
            url: format!("http://127.0.0.1:{port}"),
            started_at: crate::system::now_iso(),
        };
        atomic_write::write_json_atomic(&record_path(&canonical).unwrap(), &record).unwrap();

        let found = find_live_owner(&canonical).await;

        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server.join();
        std::env::remove_var("MOSS_HOME");

        assert!(
            found.is_none(),
            "a health body naming a different folder must not read as this folder's live owner, got {:?}",
            found
        );
    }
}
