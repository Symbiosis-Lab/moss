//! Which process, on this machine, is serving a given folder right now.
//!
//! Two moss processes can be pointed at the same site folder — the desktop
//! app and a CLI build an editor plugin spawns, say. Each mints its own token
//! and binds its own port, so without a shared record the second silently
//! shadows the first. This is the record that lets a second process find the
//! first instead of racing it: one owner per folder per machine.
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
//!   current or the last holder: its host kind, pid, version, and the URL
//!   a client can reach it at. Written on every acquire and never deleted,
//!   so it is stale from the moment its writer stops — which is why
//!   [`find_live_owner`] never trusts it alone and always confirms it
//!   against the URL's own health endpoint.
//!
//! [`acquire`] writes both; [`find_live_owner`] reads only the record and
//! verifies it over HTTP. A caller that wants the authoritative answer
//! (can I serve this folder myself?) calls [`acquire`]; a caller that wants
//! to know who else might be serving it (should I show the user a link
//! instead of starting my own server?) calls [`find_live_owner`].

use std::path::{Path, PathBuf};


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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_url: Option<String>,
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

/// The folder's owner lock is held by another process. Who that is comes from
/// [`find_live_owner`], never from the record alone: the record on disk can
/// still be the previous holder's.
#[derive(Debug)]
pub struct Held;

/// What a server start does when another process serves the folder.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OnConflict {
    /// Fail with [`already_served_message`]: a one-shot `moss build --serve`.
    #[default]
    Refuse,
    /// Report the owner's address, wait for it to leave, then serve: a
    /// `--serve --watch` an editor plugin keeps for the life of its pane.
    StandBy,
    /// Keep trying the lock this long, then refuse: a host that has just
    /// asked the owner to yield.
    Wait(std::time::Duration),
}

/// Folders this PROCESS currently holds the owner lock for, as a weak
/// reference to the open file each [`OwnerGuard`] for that folder shares.
///
/// `flock` is scoped to an open file description, not to a process: opening
/// the SAME lock path a second time and calling `try_lock` on the
/// new handle conflicts with the first even from the SAME process. That is
/// the right behavior for a second OS process, which is what this module
/// exists to catch — but a single process legitimately runs two servers
/// against one vault for a moment during a lifecycle handoff. A second
/// `acquire` for a folder THIS process already holds upgrades the weak
/// reference and reuses the held file instead of attempting — and losing —
/// a second lock against itself.
static HELD: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, std::sync::Weak<std::fs::File>>>> =
    std::sync::OnceLock::new();

fn held() -> &'static std::sync::Mutex<std::collections::HashMap<String, std::sync::Weak<std::fs::File>>> {
    HELD.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// The held owner lock. Dropping the last guard a process holds for a folder
/// closes the lock file, which releases the lock; the kernel does the same
/// on a hard kill.
#[derive(Debug)]
pub struct OwnerGuard {
    /// `None` when ownership could not be tracked at all (home directory
    /// unresolvable, lock file unopenable) — see [`acquire`]'s fallback.
    _lock: Option<std::sync::Arc<std::fs::File>>,
}

/// Try to become `folder`'s recorded owner, without blocking.
///
/// * **Lock free** (nobody holds it, including a holder that has since
///   crashed): take it and write a fresh [`OwnerRecord`] over whatever the
///   last holder left.
/// * **Lock held by this process**: share it, and write the record again, so
///   it names the server started last. If that server stops before the
///   earlier one, the folder is held and unnamed until the earlier one stops.
/// * **Lock held elsewhere**: [`Held`].
///
/// Resolving `moss_home()` or opening the lock file can fail (an
/// unresolvable home directory, a permissions error) independently of
/// contention. That is logged and the folder is served untracked, rather
/// than making ownership tracking a reason a folder can't be served at all.
pub fn acquire(folder: &Path, kind: HostKind, version: String, url: String) -> Result<OwnerGuard, Held> {
    acquire_with_ui(folder, kind, version, url, None)
}

fn acquire_with_ui(folder: &Path, kind: HostKind, version: String, url: String, ui_url: Option<String>) -> Result<OwnerGuard, Held> {
    let canonical = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    let id = folder_id(&canonical);
    let untracked = || Ok(OwnerGuard { _lock: None });

    let mut table = held().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let file = match table.get(&id).and_then(std::sync::Weak::upgrade) {
        Some(file) => file,
        None => {
            let lock_path = match owner_lock_path(&canonical) {
                Ok(p) => p,
                Err(e) => {
                    log::warn!("could not resolve the owner-lock path for {}: {e}", canonical.display());
                    return untracked();
                }
            };
            if let Some(parent) = lock_path.parent() {
                // allow:raw_write ~/.moss/locks is machine state under moss_home, not build output
                if let Err(e) = std::fs::create_dir_all(parent) {
                    log::warn!("could not create {}: {e}", parent.display());
                    return untracked();
                }
            }
            // allow:raw_write the lock file is machine state under ~/.moss/locks, not build output
            let file = match std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&lock_path) {
                Ok(f) => f,
                Err(e) => {
                    log::warn!("could not open {}: {e}", lock_path.display());
                    return untracked();
                }
            };
            match file.try_lock() {
                Ok(()) => {}
                Err(std::fs::TryLockError::WouldBlock) => return Err(Held),
                Err(e) => {
                    log::warn!("could not lock {}: {e}", lock_path.display());
                    return untracked();
                }
            }
            let file = std::sync::Arc::new(file);
            table.insert(id, std::sync::Arc::downgrade(&file));
            file
        }
    };
    drop(table);

    let record = OwnerRecord {
        folder: canonical.clone(),
        pid: std::process::id(),
        kind,
        version,
        url,
        ui_url,
        started_at: crate::system::now_iso(),
    };
    match record_path(&canonical) {
        Ok(path) => {
            if let Err(e) = atomic_write::write_json_atomic(&path, &record) {
                log::warn!("could not persist the owner record at {}: {e}", path.display());
            }
        }
        Err(e) => log::warn!("could not resolve the owner-record path for {}: {e}", canonical.display()),
    }
    Ok(OwnerGuard { _lock: Some(file) })
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
/// The one HTTP check this performs is [`super::port::blocking_get`], the
/// GET [`super::port::verify_server_ready`] makes, with the folder's token,
/// since an owner behind a public URL names its folder only to a token
/// holder. It does not reuse the whole of that function, because the two probes disagree on retry
/// policy as much as on what they're checking: `verify_server_ready` polls
/// for up to a second while a server it already decided to wait for is
/// still binding, where this is a single-shot "does anyone answer at all
/// right now" used for discovery, not for a server this caller is itself
/// starting.
///
/// `None` for every negative: no record, the record's process gone quiet,
/// a 4xx/5xx (a 401 for a token from before the owner published its own),
/// a body with no `folder_id` (an older moss, schema 1 — the schema's own
/// doc comment calls for exactly this `>=` gate rather than a hard equality,
/// so a newer schema 3 reader still accepts this one — or an owner behind a
/// public URL answering a caller without its token), or a
/// `folder_id` that names a different folder (the same owner process, moved
/// on).
pub async fn find_live_owner(folder: &Path) -> Option<OwnerRecord> {
    let canonical = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    let record = read_record(&canonical)?;
    let url = format!("{}{}", record.url, super::port::MOSS_HEALTH_PATH);
    let body = super::port::blocking_get(url, super::carrier_token::read(&canonical)).await.ok()?;
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

/// Whether another process's desktop app answers as the server of the
/// folder `site_dir` resolves to. A CLI holder yields to the app when asked;
/// another desktop app is never asked, so it is still answering after a
/// start it refused.
pub async fn open_in_another_app(site_dir: &std::sync::Arc<std::sync::RwLock<PathBuf>>) -> bool {
    let Some(folder) = resolve_vault_folder(site_dir) else { return false };
    find_live_owner(&folder)
        .await
        .is_some_and(|owner| owner.pid != std::process::id() && owner.kind == HostKind::Desktop)
}

/// The one line that names a folder's owner. `None` is a holder that does
/// not answer as this folder's server: it is stopping, or its record is not
/// its own.
pub fn already_served_message(owner: Option<&OwnerRecord>) -> String {
    match owner {
        Some(owner) => format!(
            "this folder is already served at {} by {:?} (pid {}, moss {})",
            owner.url, owner.kind, owner.pid, owner.version
        ),
        None => "this folder is already served by another moss process".to_string(),
    }
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
/// `start_server` to call once it knows the port it bound.
///
/// `Ok(None)` when `site_dir` resolves to no vault — not every headless case
/// is vault-rooted, and such a server is simply not discoverable by folder.
///
/// A held lock is handled as `on_conflict` says. The wait is a timer, never
/// a blocked thread, and ends when the caller drops this future.
pub async fn acquire_for_site_dir(
    site_dir: &std::sync::Arc<std::sync::RwLock<PathBuf>>,
    kind: HostKind,
    version: String,
    url: String,
    on_conflict: OnConflict,
    ui_url: Option<String>,
) -> Result<Option<OwnerGuard>, String> {
    let Some(folder) = resolve_vault_folder(site_dir) else {
        return Ok(None);
    };
    let started = std::time::Instant::now();
    loop {
        if let Ok(guard) = acquire_with_ui(&folder, kind, version.clone(), url.clone(), ui_url.clone()) {
            return Ok(Some(guard));
        }
        match on_conflict {
            OnConflict::Wait(patience) if started.elapsed() < patience => {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            OnConflict::Refuse | OnConflict::Wait(_) => {
                return Err(already_served_message(find_live_owner(&folder).await.as_ref()));
            }
            OnConflict::StandBy => {
                let interrupted = match find_live_owner(&folder).await {
                    Some(owner) => {
                        crate::cli_eprintln!("{}", preview_ready_line(&owner.url));
                        crate::cli_eprintln!("{}", standing_by_message(&owner));
                        wait_for_owner_to_leave_or_interrupt(&folder).await
                    }
                    // The holder is starting or stopping: look again soon, so
                    // its address is not missed once it answers.
                    None => tokio::select! {
                        _ = tokio::time::sleep(std::time::Duration::from_millis(250)) => false,
                        _ = crate::ops::stop::requested() => true,
                    },
                };
                if interrupted {
                    crate::cli_eprintln!("Stopping...");
                    crate::cli_eprintln!("Stopped");
                    std::process::exit(0);
                }
            }
        }
    }
}

/// The CLI's "build done" line. Under a preview server the pages are served
/// from `staging` and `current` is only promoted after
/// `seal_phase::IDLE` without edits, so naming `current` as the output would
/// be false while a watch is running.
pub(crate) fn generated_line(name: &str, current: &Path, staging: &Path, serving: bool) -> String {
    if serving {
        format!(
            "📁 '{name}': Site built; the preview serves {}. {} follows after {} s without edits.",
            staging.display(),
            current.display(),
            crate::build::seal_phase::IDLE.as_secs()
        )
    } else {
        format!("📁 '{name}': Site generated at {}", current.display())
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

/// [`wait_for_owner_to_leave`], or a stop request, whichever comes first. Returns
/// `true` when interrupted.
///
/// A job backgrounded by a non-interactive shell inherits SIGINT ignored, and
/// once any `ctrl_c` handler exists, an await outside a `select` swallows the
/// signal. Every standby wait goes through here so the signal is never
/// swallowed.
pub async fn wait_for_owner_to_leave_or_interrupt(folder: &Path) -> bool {
    tokio::select! {
        _ = wait_for_owner_to_leave(folder) => false,
        _ = crate::ops::stop::requested() => true,
    }
}

/// Wait up to `timeout` for `folder` to GET a live owner — the reverse wait
/// of [`wait_for_owner_to_leave`], for a process that just honored a yield
/// and must not simply reacquire the lock it just freed. A folder is briefly
/// ownerless right after a yield, and reacquiring immediately (the lock IS
/// free) would make the yielder its own next owner, defeating the whole
/// point: this wait is what leaves the window open for someone else — the
/// caller `/__moss/yield` exists for — to claim it first. A record naming
/// THIS process's own pid is ignored rather than accepted: our own record
/// stays on disk after we stop.
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

/// After honoring a yield: stand by for whoever claims `folder` within
/// `timeout`, until they leave. `true` when a stop was requested meanwhile.
pub async fn stand_by_after_yield(folder: &Path, timeout: std::time::Duration) -> bool {
    let claimed = tokio::select! {
        owner = wait_for_owner_to_appear(folder, timeout) => owner,
        _ = crate::ops::stop::requested() => return true,
    };
    match claimed {
        Some(owner) => {
            crate::cli_eprintln!("{}", preview_ready_line(&owner.url));
            crate::cli_eprintln!("{}", standing_by_message(&owner));
            wait_for_owner_to_leave_or_interrupt(folder).await
        }
        None => {
            crate::cli_eprintln!("no other moss claimed the folder; serving again");
            false
        }
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
    let token = super::carrier_token::read(vault.path()).ok_or_else(|| {
        format!("could not read the HTTP carrier token at {}", super::carrier_token::token_path(vault.path()).display())
    })?;
    let url = format!("{}/__moss/yield", owner.url);
    let post_url = url.clone();
    let result = tokio::task::spawn_blocking(move || {
        super::carrier_token::agent(std::time::Duration::from_secs(5))
            .post(&post_url)
            .set(super::carrier_token::TOKEN_HEADER, &token)
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

    fn moss_home_dir() -> std::path::PathBuf {
        crate::infra::home::scratch_moss_home()
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
        std::env::set_var("MOSS_HOME", &home);
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
        std::env::set_var("MOSS_HOME", &home);
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
        let next = std::fs::File::open(owner_lock_path(&canonical).unwrap()).unwrap();
        next.try_lock().expect("dropping the guard frees the folder");
        std::env::remove_var("MOSS_HOME");
    }

    /// One process runs two servers against a vault for a moment during a
    /// lifecycle handoff, and after a yield nobody claims it serves again on
    /// another port: the record must name the server started last.
    #[test]
    fn a_second_acquire_in_the_same_process_succeeds_and_rewrites_the_record() {
        let _guard = env_guard();
        let home = moss_home_dir();
        std::env::set_var("MOSS_HOME", &home);
        let folder = tempfile::tempdir().expect("tempdir for the served folder");

        let first =
            acquire(folder.path(), HostKind::Cli, "1.2.3".to_string(), "http://127.0.0.1:4321".to_string())
                .expect("first acquire must succeed");
        let second = acquire(folder.path(), HostKind::Cli, "1.2.3".to_string(), "http://127.0.0.1:9999".to_string())
            .expect("a second acquire from the SAME process must not conflict with itself");

        let canonical = std::fs::canonicalize(folder.path()).unwrap();
        let on_disk: OwnerRecord =
            serde_json::from_str(&std::fs::read_to_string(record_path(&canonical).unwrap()).unwrap()).unwrap();
        assert_eq!(on_disk.url, "http://127.0.0.1:9999");

        drop(first);
        let foreign = std::fs::File::open(owner_lock_path(&canonical).unwrap()).unwrap();
        assert!(foreign.try_lock().is_err(), "the folder stays held while `second` lives");
        drop(second);
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
        std::env::set_var("MOSS_HOME", &home);
        let folder = tempfile::tempdir().expect("tempdir for the served folder");
        let canonical = std::fs::canonicalize(folder.path()).unwrap();

        // Lock the exact file `acquire` would, but through a handle `HELD`
        // never learns about — the in-process fast path must NOT treat this
        // as "already ours."
        let lock_path = owner_lock_path(&canonical).unwrap();
        std::fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
        let foreign = std::fs::OpenOptions::new().read(true).write(true).create(true).open(&lock_path).unwrap();
        foreign.try_lock().expect("the foreign handle must win the uncontended lock");
        let foreign_record = OwnerRecord {
            folder: canonical.clone(),
            pid: 777,
            kind: HostKind::Desktop,
            version: "7.0.0".to_string(),
            url: "http://127.0.0.1:7777".to_string(),
            ui_url: None,
            started_at: crate::system::now_iso(),
        };
        atomic_write::write_json_atomic(&record_path(&canonical).unwrap(), &foreign_record).unwrap();

        let result = acquire(folder.path(), HostKind::Cli, "1.2.3".to_string(), "http://127.0.0.1:9999".to_string());
        result.expect_err("a lock held outside this process's own registry must refuse");

        // The record on disk names pid 777 at an address nothing answers on:
        // the refusal must not repeat it as the holder.
        std::fs::create_dir_all(folder.path().join(".moss")).unwrap();
        let cell = std::sync::Arc::new(std::sync::RwLock::new(folder.path().join(".moss")));
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let refusal = runtime
            .block_on(acquire_for_site_dir(&cell, HostKind::Cli, "1.2.3".to_string(), "http://127.0.0.1:9999".to_string(), OnConflict::Refuse, None))
            .expect_err("a held folder refuses");
        assert_eq!(refusal, "this folder is already served by another moss process");

        // And with no record at all.
        std::fs::remove_file(record_path(&canonical).unwrap()).unwrap();
        let refusal = runtime
            .block_on(acquire_for_site_dir(&cell, HostKind::Cli, "1.2.3".to_string(), "http://127.0.0.1:9999".to_string(), OnConflict::Refuse, None))
            .expect_err("a held folder refuses");
        assert_eq!(refusal, "this folder is already served by another moss process");

        drop(foreign);
        std::env::remove_var("MOSS_HOME");
    }

    /// A holder that has been asked to yield lets go a moment later; the
    /// requester's wait is on the lock itself.
    #[test]
    fn wait_gets_a_folder_its_holder_releases_in_time() {
        let _guard = env_guard();
        let home = moss_home_dir();
        std::env::set_var("MOSS_HOME", &home);
        let folder = tempfile::tempdir().expect("tempdir for the served folder");
        let canonical = std::fs::canonicalize(folder.path()).unwrap();
        std::fs::create_dir_all(folder.path().join(".moss")).unwrap();
        let cell = std::sync::Arc::new(std::sync::RwLock::new(folder.path().join(".moss")));

        let lock_path = owner_lock_path(&canonical).unwrap();
        std::fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
        let foreign = std::fs::OpenOptions::new().read(true).write(true).create(true).open(&lock_path).unwrap();
        foreign.try_lock().expect("the foreign handle must win the uncontended lock");
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            drop(foreign);
        });

        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let waited = runtime.block_on(acquire_for_site_dir(
            &cell,
            HostKind::Desktop,
            "1.2.3".to_string(),
            "http://127.0.0.1:9999".to_string(),
            OnConflict::Wait(std::time::Duration::from_secs(5)),
            None,
        ));
        release.join().unwrap();
        std::env::remove_var("MOSS_HOME");

        assert!(matches!(waited, Ok(Some(_))), "the wait must end with the folder: {waited:?}");
    }

    /// A loopback listener that answers every request with a schema-2 health
    /// body for `folder_id`, and a record on disk for `folder` naming it as
    /// `pid`. Call the returned closure to stop the listener.
    fn recorded_holder(folder: &Path, folder_id: String, pid: u32, kind: HostKind) -> impl FnOnce() {
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
                        let body = format!(
                            "{{\"server\":\"moss-preview-server\",\"version\":\"x\",\"preview\":true,\"schema\":2,\"folder_id\":\"{folder_id}\",\"pid\":{pid}}}"
                        );
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
            folder: folder.to_path_buf(),
            pid,
            kind,
            version: "0.0.0".to_string(),
            url: format!("http://127.0.0.1:{port}"),
            ui_url: None,
            started_at: crate::system::now_iso(),
        };
        atomic_write::write_json_atomic(&record_path(folder).unwrap(), &record).unwrap();
        move || {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
            let _ = server.join();
        }
    }

    /// A record whose address answers for a DIFFERENT folder (another moss
    /// process that happens to use the same loopback port) reads as unowned.
    #[tokio::test]
    async fn find_live_owner_is_none_when_the_health_body_names_a_different_folder() {
        let _guard = env_guard();
        std::env::set_var("MOSS_HOME", moss_home_dir());
        let folder = tempfile::tempdir().expect("tempdir for the served folder");
        let canonical = std::fs::canonicalize(folder.path()).unwrap();
        let stop = recorded_holder(&canonical, "not-the-right-folder".to_string(), 4242, HostKind::Cli);

        let found = find_live_owner(&canonical).await;

        stop();
        std::env::remove_var("MOSS_HOME");
        assert!(
            found.is_none(),
            "a health body naming a different folder must not read as this folder's live owner, got {:?}",
            found
        );
    }

    #[tokio::test]
    async fn open_in_another_app_is_another_process_answering_as_the_desktop_app() {
        let _guard = env_guard();
        std::env::set_var("MOSS_HOME", moss_home_dir());
        let folder = tempfile::tempdir().expect("tempdir for the served folder");
        std::fs::create_dir(folder.path().join(".moss")).unwrap();
        let canonical = std::fs::canonicalize(folder.path()).unwrap();
        let site_dir = std::sync::Arc::new(std::sync::RwLock::new(canonical.join(".moss")));
        let other_pid = std::process::id().wrapping_add(1);
        let mut seen = vec![open_in_another_app(&site_dir).await];
        for (pid, kind) in [(other_pid, HostKind::Desktop), (other_pid, HostKind::Cli), (std::process::id(), HostKind::Desktop)] {
            let stop = recorded_holder(&canonical, folder_id(&canonical), pid, kind);
            seen.push(open_in_another_app(&site_dir).await);
            stop();
        }
        seen.push(open_in_another_app(&site_dir).await);
        std::env::remove_var("MOSS_HOME");

        assert_eq!(
            seen,
            [false, true, false, false, false],
            "nobody recorded, another app, another program, this process, a record that no longer answers"
        );
    }
}
