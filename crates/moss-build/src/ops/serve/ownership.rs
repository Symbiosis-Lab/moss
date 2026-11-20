//! Which process, on this machine, is serving a given folder right now.
//!
//! Two moss processes can be pointed at the same site folder — the desktop
//! app and a CLI build an editor plugin spawns, say. Each mints its own token
//! and binds its own port, so without a shared record the second silently
//! shadows the first. This is the record that lets a second process find the
//! first instead of racing it: one owner per folder per machine.
//!
//! Two files per folder, both named by [`name_for`] (the same hash
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
//!   a client can reach it at. Written on every acquire and removed when
//!   the server stops. A killed holder leaves its record behind until the
//!   next acquire sweeps records whose pid is gone, so a record can still be
//!   stale, which is why [`find_live_owner`] never trusts it alone and always
//!   confirms it against the URL's own health endpoint.
//!
//! [`acquire`] writes both; [`find_live_owner`] reads only the record and
//! verifies it over HTTP. A caller that wants the authoritative answer
//! (can I serve this folder myself?) calls [`acquire`]; a caller that wants
//! to know who else might be serving it (should I show the user a link
//! instead of starting my own server?) calls [`find_live_owner`].

use std::path::{Path, PathBuf};


use crate::infra::atomic_write;
use crate::infra::folder_store::{name_for, pid_alive};
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
/// `folder` is stored canonicalized (the same path [`name_for`] hashed),
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
    Ok(moss_home()?.join("servers").join(format!("{}.json", name_for(folder))))
}

/// The name of the `infra::folder_lock` lock [`acquire`] takes,
/// `<moss_home>/locks/<folder-id>.owner`. Distinct from the build's lock: a
/// build and the server reading its output are different scopes (a build
/// can run against a folder nobody is serving, and the server keeps running
/// long after any one build finishes), so they must not contend with each
/// other.
const OWNER_LOCK: &str = "owner";

/// Contention and storage failure are separate: neither grants ownership.
/// A contended lock's holder is discovered over HTTP, never trusted from its
/// announcement alone.
#[derive(Debug)]
pub enum AcquireError {
    /// Another process holds the kernel lock.
    Held,
    /// Ownership could not be established or recorded; never serve untracked.
    Storage(String),
}

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
/// reference to the lock each [`OwnerGuard`] for that folder shares.
///
/// `flock` is scoped to an open file description, not to a process: opening
/// the SAME lock path a second time and calling `try_lock` on the
/// new handle conflicts with the first even from the SAME process. That is
/// the right behavior for a second OS process, which is what this module
/// exists to catch — but a single process legitimately runs two servers
/// against one vault for a moment during a lifecycle handoff. A second
/// `acquire` for a folder THIS process already holds upgrades the weak
/// reference and reuses the held lock instead of attempting — and losing —
/// a second lock against itself.
static HELD: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, std::sync::Weak<Claim>>>> =
    std::sync::OnceLock::new();

fn held() -> &'static std::sync::Mutex<std::collections::HashMap<String, std::sync::Weak<Claim>>> {
    HELD.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// The owner lock and the record it vouches for. The record goes first, while
/// the lock is still held, so it can never remove a record the next owner
/// wrote.
#[derive(Debug)]
struct Claim {
    record: PathBuf,
    _lock: crate::infra::folder_lock::Held,
}

impl Drop for Claim {
    fn drop(&mut self) {
        // allow:unlink an owner record under <moss_home>/servers, never staging
        let _ = std::fs::remove_file(&self.record);
    }
}

/// The held owner lock. Dropping the last guard a process holds for a folder
/// releases the lock; the kernel does the same on a hard kill.
#[derive(Debug)]
pub struct OwnerGuard {
    _lock: std::sync::Arc<Claim>,
    /// A previous announcement was replaced after acquiring its released lock.
    pub replaced_stale_record: bool,
}

/// Try to become `folder`'s recorded owner, without blocking.
///
/// * **Lock free** (nobody holds it, including a holder that has since
///   crashed): take it and write a fresh [`OwnerRecord`] over whatever the
///   last holder left.
/// * **Lock held by this process**: share it, and write the record again, so
///   it names the server started last. If that server stops before the
///   earlier one, the folder is held and unnamed until the earlier one stops.
/// * **Lock held elsewhere**: [`AcquireError::Held`].
///
/// Resolving `moss_home()` or opening the lock file can fail (an
/// unresolvable home directory, a permissions error) independently of
/// contention. These failures are errors: no guard means no writing authority.
pub fn acquire(folder: &Path, kind: HostKind, version: String, url: String) -> Result<OwnerGuard, AcquireError> {
    acquire_with_ui(folder, kind, version, url, None)
}

fn acquire_with_ui(folder: &Path, kind: HostKind, version: String, url: String, ui_url: Option<String>) -> Result<OwnerGuard, AcquireError> {
    let canonical = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    let id = name_for(&canonical);
    let path = record_path(&canonical).map_err(AcquireError::Storage)?;
    let mut replaced_stale_record = false;

    let mut table = held().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let lock = match table.get(&id).and_then(std::sync::Weak::upgrade) {
        Some(lock) if url.is_empty() => return Ok(OwnerGuard { _lock: lock, replaced_stale_record: false }),
        Some(lock) => lock,
        None => match crate::infra::folder_lock::try_acquire_named(&canonical, OWNER_LOCK) {
            Ok(Some(lock)) => {
                replaced_stale_record = path.try_exists().map_err(|e| AcquireError::Storage(e.to_string()))?;
                let lock = std::sync::Arc::new(Claim { record: path.clone(), _lock: lock });
                table.insert(id, std::sync::Arc::downgrade(&lock));
                lock
            }
            Ok(None) => return Err(AcquireError::Held),
            Err(e) => return Err(AcquireError::Storage(e)),
        },
    };
    drop(table);

    // A build reserves only its own folder. Sweeping unrelated announcements
    // here delays source scanning and can miss a folder's cancellation. Leave
    // collection to server announcements, outside the process-wide mutex.
    if !url.is_empty() {
        if let Some(dir) = path.parent() {
            sweep_dead_records(dir, &path);
        }
    }
    let record = OwnerRecord {
        folder: canonical.clone(),
        pid: std::process::id(),
        kind,
        version,
        url,
        ui_url,
        started_at: crate::system::now_iso(),
    };
    atomic_write::write_json_atomic(&path, &record).map_err(AcquireError::Storage)?;
    Ok(OwnerGuard { _lock: lock, replaced_stale_record })
}

/// Removes every record in `dir` but `keep` whose process is gone (a holder
/// killed before it could remove its own) or that does not parse. A server
/// start is rare, and once the litter is gone the directory holds one record
/// per running server, so the pass is synchronous.
fn sweep_dead_records(dir: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut removed = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if path == keep || path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let live = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<OwnerRecord>(&text).ok())
            .is_some_and(|record| pid_alive(record.pid));
        // allow:unlink a dead owner record under <moss_home>/servers, never staging
        if !live && std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    if removed > 0 {
        log::info!("removed {removed} owner records of servers that are gone from {}", dir.display());
    }
}

/// Whether `folder`'s owner record names a live process other than this one.
/// Unlike [`find_live_owner`] it asks the process nothing, so an older moss,
/// which may not answer this one's token, still counts.
pub(crate) fn live_foreign_owner(folder: &Path) -> bool {
    let canonical = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    read_record(&canonical).ok().flatten().is_some_and(|record| record.pid != std::process::id() && pid_alive(record.pid))
}

/// Write `record` as `folder`'s owner, for tests standing in for another process.
#[cfg(test)]
pub(crate) fn write_record_for_tests(folder: &Path, record: &OwnerRecord) {
    let canonical = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    let path = record_path(&canonical).unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_vec(record).unwrap()).unwrap();
}

fn read_record(folder: &Path) -> Result<Option<OwnerRecord>, String> {
    let path = record_path(folder)?;
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(serde_json::from_str(&text).ok()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("read owner record {}: {e}", path.display())),
    }
}

/// Capabilities observed at the endpoint, independent of product-version order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerCapabilities {
    pub preview: bool,
    /// A token was accepted by the events route. This does not promise editing,
    /// publishing or cooperative yield; those operations still check their responses.
    pub authenticated_events: bool,
}

/// A folder/PID/marker-verified endpoint and its observed capabilities.
/// Credentials stay private and are never included in diagnostics.
pub struct VerifiedOwner {
    pub record: OwnerRecord,
    pub capabilities: OwnerCapabilities,
    /// Legacy health may report the library version instead of the product version.
    pub health_version: String,
    token: Option<String>,
}

impl VerifiedOwner {
    /// The credential accepted by the owner's token-gated events route.
    pub fn token(&self) -> Option<&str> { self.token.as_deref() }
}

/// The shared admission result for any interface. Only `Acquired` authorizes
/// local writes; discovery failure is never evidence that the lock is free.
pub enum Ownership {
    Acquired(OwnerGuard),
    Verified(VerifiedOwner),
    Unresponsive,
}

/// Try the authoritative lock, then discover its holder. Storage errors remain
/// errors, distinct from an owner that is starting, stopping or unreachable.
pub async fn resolve_owner(folder: &Path, kind: HostKind, version: String, url: String, ui_url: Option<String>) -> Result<Ownership, String> {
    match acquire_with_ui(folder, kind, version, url, ui_url) {
        Ok(guard) => Ok(Ownership::Acquired(guard)),
        Err(AcquireError::Storage(e)) => Err(e),
        Err(AcquireError::Held) => Ok(match discover_owner(folder).await? {
            Some(owner) => Ownership::Verified(owner),
            None => Ownership::Unresponsive,
        }),
    }
}

fn local_endpoint(endpoint: &str) -> bool {
    url::Url::parse(endpoint).is_ok_and(|url| {
        url.scheme() == "http" && url.username().is_empty() && url.password().is_none()
            && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
            && url.path() == "/" && url.query().is_none() && url.fragment().is_none()
    })
}

fn credentials(folder: &Path) -> Result<Vec<String>, String> {
    // Current first, but a stale current token must not mask a live legacy one.
    let mut tokens = Vec::new();
    for path in [super::carrier_token::token_path(folder), folder.join(".moss/build.nosync/http-token")] {
        match std::fs::read_to_string(&path) {
            Ok(token) if !token.trim().is_empty() => tokens.push(token.trim().to_owned()),
            Ok(_) => {},
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
            Err(e) => return Err(format!("read owner credential {}: {e}", path.display())),
        }
    }
    Ok(tokens)
}

/// Discover without acquiring. Health establishes endpoint identity, never
/// authentication. The legacy events GET is token-gated even when health is
/// anonymous; checking its headers closes the stream without waiting for events.
pub async fn discover_owner(folder: &Path) -> Result<Option<VerifiedOwner>, String> {
    let canonical = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    let Some(record) = read_record(&canonical)? else { return Ok(None) };
    if record.folder != canonical || !local_endpoint(&record.url)
        || record.ui_url.as_ref().is_some_and(|url| !local_endpoint(url))
    {
        return Ok(None);
    }
    let tokens = credentials(&canonical)?;
    let endpoint = record.url.clone();
    let folder_id = name_for(&canonical);
    let pid = record.pid;
    let verified = tokio::task::spawn_blocking(move || {
        let agent = super::carrier_token::agent(std::time::Duration::from_secs(1));
        let mut health = None;
        for token in std::iter::once(None).chain(tokens.iter().map(|s| Some(s.as_str()))) {
            let mut request = agent.get(&format!("{endpoint}{}", super::port::MOSS_HEALTH_PATH));
            if let Some(token) = token { request = request.set(super::carrier_token::TOKEN_HEADER, token); }
            let response = match request.call() {
                Ok(response) if response.status() == 200 => response,
                // A public owner can reject a stale token but accept the next.
                Err(ureq::Error::Status(401, _)) if token.is_some() => continue,
                _ => return None,
            };
            let body = response.into_json::<serde_json::Value>().ok()?;
            if body["server"] != "moss-preview-server" || body["schema"].as_u64().unwrap_or(0) < 2 {
                return None;
            }
            // Readiness-only health may omit identity until authenticated.
            // Contradictory identity must stop before sending any credential.
            if body.get("folder_id").is_some_and(|id| id.as_str() != Some(&folder_id))
                || body.get("pid").is_some_and(|seen| seen.as_u64() != Some(pid as u64))
            {
                return None;
            }
            if body.get("folder_id").is_some() && body.get("pid").is_some() {
                health = Some(body);
                break;
            }
        }
        let body = health?;
        let version = body["version"].as_str()?.to_owned();
        let token = tokens.into_iter().find(|token| {
            agent.get(&format!("{endpoint}/__moss/events"))
                .set(super::carrier_token::TOKEN_HEADER, token).call().is_ok_and(|response| {
                    response.status() == 200 && response.header("content-type")
                        .is_some_and(|value| value.split(';').next() == Some("text/event-stream"))
                })
        });
        Some((version, body["preview"] == true, token))
    }).await.map_err(|e| format!("owner discovery failed: {e}"))?;
    let Some((version, preview, token)) = verified else { return Ok(None) };
    // An announcement replaced while probing belongs to another session.
    if read_record(&canonical)?.as_ref() != Some(&record) {
        return Ok(None);
    }
    Ok(Some(VerifiedOwner {
        record,
        capabilities: OwnerCapabilities { preview, authenticated_events: token.is_some() },
        health_version: version,
        token,
    }))
}

/// Compatibility wrapper for read-only callers. `None` never grants ownership.
pub async fn find_live_owner(folder: &Path) -> Option<OwnerRecord> {
    discover_owner(folder).await.ok().flatten().map(|owner| owner.record)
}

/// Whether another process's desktop app answers as the server of the
/// folder `site_dir` resolves to. A CLI holder yields to the app when asked;
/// another desktop app is never asked, so it is still answering after a
/// start it refused.
pub async fn open_in_another_app(site_dir: &crate::moss_paths::ServedCell) -> bool {
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
fn resolve_vault_folder(site_dir: &crate::moss_paths::ServedCell) -> Option<PathBuf> {
    site_dir
        .read()
        .ok()
        .map(|served| served.folder.path().to_path_buf())
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
    site_dir: &crate::moss_paths::ServedCell,
    kind: HostKind,
    version: String,
    url: String,
    on_conflict: OnConflict,
    ui_url: Option<String>,
) -> Result<Option<OwnerGuard>, String> {
    let Some(folder) = resolve_vault_folder(site_dir) else {
        return Ok(None);
    };
    acquire_for_folder(&folder, kind, version, url, on_conflict, ui_url).await.map(Some)
}

/// Admit a build before it creates or migrates any output. A blank URL reserves
/// ownership until the server announces its bound endpoint using the shared guard.
pub async fn acquire_for_folder(folder: &Path, kind: HostKind, version: String, url: String, on_conflict: OnConflict, ui_url: Option<String>) -> Result<OwnerGuard, String> {
    let started = std::time::Instant::now();
    loop {
        let owner = match resolve_owner(folder, kind, version.clone(), url.clone(), ui_url.clone()).await? {
            Ownership::Acquired(guard) => return Ok(guard),
            Ownership::Verified(owner) => Some(owner.record),
            Ownership::Unresponsive => None,
        };
        match on_conflict {
            OnConflict::Wait(patience) if started.elapsed() < patience => {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            OnConflict::Refuse | OnConflict::Wait(_) => {
                return Err(already_served_message(owner.as_ref()));
            }
            OnConflict::StandBy => {
                let interrupted = match owner {
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
    /// No verified endpoint answered. This does not imply the lock is free.
    NoOwner,
}

/// Ask whichever process currently serves `folder` to give it up over
/// `POST /__moss/yield`, so a fuller engine elsewhere on the machine (the
/// desktop app) can take the folder over. Reads the live owner's record
/// (for its URL), then the vault's
/// the folder's `http-token` record on this computer (for the same bearer token the carrier
/// checks — see [`super::carrier_token`]), and reports what the owner
/// answered rather than assuming the request landed.
pub async fn request_yield(folder: &Path) -> Result<YieldOutcome, String> {
    let canonical = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    let Some(owner) = discover_owner(&canonical).await? else {
        return Ok(YieldOutcome::NoOwner);
    };
    let token = owner.token().ok_or("could not authenticate the live owner")?.to_owned();
    let url = format!("{}/__moss/yield", owner.record.url);
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

    fn owner_lock_path(folder: &Path) -> Result<PathBuf, String> {
        crate::infra::folder_lock::lock_path_named(folder, OWNER_LOCK)
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
        assert!(!path.exists(), "a stopped server leaves no record");
        assert!(!owner_lock_path(&canonical).unwrap().exists(), "a released owner lock leaves no file");
        let next = crate::infra::folder_lock::try_acquire_named(&canonical, OWNER_LOCK).unwrap();
        assert!(next.is_some(), "dropping the guard frees the folder");
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

    #[test]
    fn build_admission_leaves_unrelated_records_for_server_start() {
        crate::infra::home::with_moss_home(|home| {
            let folder = tempfile::tempdir().unwrap();
            let servers = home.join("servers");
            std::fs::create_dir_all(&servers).unwrap();
            let garbage = servers.join("gone.json");
            std::fs::write(&garbage, "not a record").unwrap();

            let build = acquire(folder.path(), HostKind::Cli, "test".into(), String::new()).unwrap();
            assert!(garbage.exists(), "build admission must not scan unrelated owner records");
            assert!(crate::infra::folder_lock::try_acquire_named(folder.path(), OWNER_LOCK).unwrap().is_none());
            let server = acquire(folder.path(), HostKind::Cli, "test".into(), "http://127.0.0.1:4321".into()).unwrap();
            assert!(!garbage.exists(), "server startup still collects stale announcements");
            let rebuild = acquire(folder.path(), HostKind::Cli, "test".into(), String::new()).unwrap();
            assert_eq!(read_record(folder.path()).unwrap().unwrap().url, "http://127.0.0.1:4321");
            drop((build, server, rebuild));
        });
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
        // A live pid, so the start's sweep keeps the record.
        let foreign_record = OwnerRecord {
            folder: canonical.clone(),
            pid: std::process::id(),
            kind: HostKind::Desktop,
            version: "7.0.0".to_string(),
            url: "http://127.0.0.1:7777".to_string(),
            ui_url: None,
            started_at: crate::system::now_iso(),
        };
        atomic_write::write_json_atomic(&record_path(&canonical).unwrap(), &foreign_record).unwrap();

        let result = acquire(folder.path(), HostKind::Cli, "1.2.3".to_string(), "http://127.0.0.1:9999".to_string());
        result.expect_err("a lock held outside this process's own registry must refuse");

        // The record on disk names an address nothing answers on:
        // the refusal must not repeat it as the holder.
        std::fs::create_dir_all(folder.path().join(".moss")).unwrap();
        let cell = crate::moss_paths::Served::test_cell(folder.path().join(".moss"));
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

    /// A killed server cannot remove its record; the next start does, and
    /// keeps the records of servers still running.
    #[test]
    fn a_start_removes_the_records_of_servers_that_are_gone() {
        let _guard = env_guard();
        let home = moss_home_dir();
        std::env::set_var("MOSS_HOME", &home);
        let gone = tempfile::tempdir().unwrap();
        let running = tempfile::tempdir().unwrap();
        let mut exited = std::process::Command::new(std::env::current_exe().unwrap()).arg("--list").stdout(std::process::Stdio::null()).spawn().unwrap();
        let dead_pid = exited.id();
        exited.wait().unwrap();
        let record = |folder: &Path, pid: u32| {
            let canonical = std::fs::canonicalize(folder).unwrap();
            let path = record_path(&canonical).unwrap();
            let record = OwnerRecord {
                folder: canonical,
                pid,
                kind: HostKind::Cli,
                version: "1.0.0".to_string(),
                url: "http://127.0.0.1:1".to_string(),
                ui_url: None,
                started_at: crate::system::now_iso(),
            };
            atomic_write::write_json_atomic(&path, &record).unwrap();
            path
        };
        let gone_record = record(gone.path(), dead_pid);
        let running_record = record(running.path(), std::process::id());
        let garbage = home.join("servers").join("garbage.json");
        std::fs::write(&garbage, "not a record").unwrap();

        let next = tempfile::tempdir().unwrap();
        let owned = acquire(next.path(), HostKind::Cli, "1.0.0".to_string(), "http://127.0.0.1:2".to_string()).unwrap();

        assert!(!gone_record.exists(), "the record of a server that is gone is removed");
        assert!(!garbage.exists(), "a record that does not parse is removed");
        assert!(running_record.exists(), "a running server keeps its record");
        drop(owned);
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
        let cell = crate::moss_paths::Served::test_cell(folder.path().join(".moss"));

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
            version: "x".to_string(),
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
        let site_dir = crate::moss_paths::Served::test_cell(canonical.join(".moss"));
        let other_pid = std::process::id().wrapping_add(1);
        let mut seen = vec![open_in_another_app(&site_dir).await];
        for (pid, kind) in [(other_pid, HostKind::Desktop), (other_pid, HostKind::Cli), (std::process::id(), HostKind::Desktop)] {
            let stop = recorded_holder(&canonical, name_for(&canonical), pid, kind);
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
    struct FixtureServer {
        url: String,
        credential_requests: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
        worker: Option<std::thread::JoinHandle<()>>,
    }

    impl FixtureServer {
        fn new(health: serde_json::Value, redirect: bool) -> Self {
            use std::io::{Read, Write};
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            listener.set_nonblocking(true).unwrap();
            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let stopping = stop.clone();
            let credential_requests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let credentials_seen = credential_requests.clone();
            let worker = std::thread::spawn(move || {
                while !stopping.load(std::sync::atomic::Ordering::Relaxed) {
                    let Ok((mut stream, _)) = listener.accept() else {
                        std::thread::sleep(std::time::Duration::from_millis(2));
                        continue;
                    };
                    stream.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
                    let mut buffer = [0; 4096];
                    let mut size = 0;
                    // TCP can split the request line or headers across reads.
                    while size < buffer.len() && !buffer[..size].windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                        match stream.read(&mut buffer[size..]) {
                            Ok(0) | Err(_) => break,
                            Ok(read) => size += read,
                        }
                    }
                    let request = String::from_utf8_lossy(&buffer[..size]).to_ascii_lowercase();
                    if request.contains("x-moss-token:") {
                        credentials_seen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                    let authenticated = request.contains("x-moss-token: legacy-secret\r\n");
                    let (status, content_type, body) = if redirect {
                        (302, "application/json", health.to_string())
                    } else if request.starts_with("get /__moss_health/") {
                        // Legacy loopback health is anonymous; a public owner
                        // withholds identity until its token is accepted.
                        if health["private"] == true && !authenticated {
                            if request.contains("x-moss-token:") {
                                (401, "application/json", "{}".into())
                            } else {
                                (200, "application/json", serde_json::json!({"server":"moss-preview-server", "schema":2}).to_string())
                            }
                        } else {
                            (200, "application/json", health.to_string())
                        }
                    } else if request.starts_with("get /__moss/events ") && request.contains("x-moss-token: legacy-secret\r\n") {
                        (200, "text/event-stream", String::new())
                    } else {
                        (401, "application/json", "{}".into())
                    };
                    let _ = write!(stream, "HTTP/1.1 {status} Fixture\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nLocation: http://127.0.0.1:1/\r\n\r\n{body}", body.len());
                }
            });
            Self { url, credential_requests, stop, worker: Some(worker) }
        }
    }

    impl Drop for FixtureServer {
        fn drop(&mut self) {
            self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            self.worker.take().unwrap().join().unwrap();
        }
    }

    fn legacy_health(folder: &Path) -> serde_json::Value {
        serde_json::json!({
            "server": "moss-preview-server", "schema": 2, "preview": true,
            "version": "0.8.0", "pid": std::process::id(), "folder_id": name_for(folder),
        })
    }

    fn fixture_record(folder: &Path, url: String) -> OwnerRecord {
        OwnerRecord {
            folder: folder.to_path_buf(), pid: std::process::id(), kind: HostKind::Cli,
            version: "0.15.23".into(), url, ui_url: None, started_at: "fixture-session".into(),
        }
    }

    #[tokio::test]
    async fn legacy_auth_with_missing_machine_token() {
        legacy_auth_fixture(false, false).await;
    }

    #[tokio::test]
    async fn legacy_auth_with_stale_machine_token() {
        legacy_auth_fixture(true, false).await;
    }

    #[tokio::test]
    async fn legacy_auth_with_private_health_and_stale_machine_token() {
        legacy_auth_fixture(true, true).await;
    }

    async fn legacy_auth_fixture(stale: bool, private: bool) {
        let _guard = env_guard();
        std::env::set_var("MOSS_HOME", moss_home_dir());
        let folder = tempfile::tempdir().unwrap();
        let folder = folder.path().canonicalize().unwrap();
        let mut health = legacy_health(&folder);
        health["private"] = private.into();
        let server = FixtureServer::new(health, false);
        atomic_write::write_json_atomic(&record_path(&folder).unwrap(), &fixture_record(&folder, server.url.clone())).unwrap();
        let legacy = folder.join(".moss/build.nosync/http-token");
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::write(&legacy, "legacy-secret\n").unwrap();
        if stale { super::super::carrier_token::publish(&folder, "stale-machine-secret").unwrap(); }
        let owner = discover_owner(&folder).await.unwrap().unwrap();
        assert_eq!(owner.record.version, "0.15.23");
        assert_eq!(owner.health_version, "0.8.0");
        assert!(owner.capabilities.preview);
        assert!(owner.capabilities.authenticated_events, "stale current: {stale}");
        assert_eq!(owner.token(), Some("legacy-secret"));
        std::fs::remove_file(legacy).unwrap();
        super::super::carrier_token::publish(&folder, "legacy-secret").unwrap();
        assert!(discover_owner(&folder).await.unwrap().unwrap().capabilities.authenticated_events, "current token still works without a legacy file");
        super::super::carrier_token::publish(&folder, "stale-machine-secret").unwrap();
        let found = discover_owner(&folder).await.unwrap();
        if private {
            assert!(found.is_none(), "readiness-only health cannot verify folder identity");
        } else {
            let owner = found.unwrap();
            assert!(owner.capabilities.preview, "anonymous health still works");
            assert!(!owner.capabilities.authenticated_events, "health must not prove the stale credential");
            assert!(owner.token().is_none());
        }
        std::env::remove_var("MOSS_HOME");
    }

    #[tokio::test]
    async fn discovery_rejects_wrong_marker_pid_folder_record_and_redirects() {
        let _guard = env_guard();
        std::env::set_var("MOSS_HOME", moss_home_dir());
        let folder = tempfile::tempdir().unwrap();
        let folder = folder.path().canonicalize().unwrap();
        super::super::carrier_token::publish(&folder, "legacy-secret").unwrap();
        for case in ["marker", "pid", "folder", "record", "redirect", "endpoint"] {
            let mut health = legacy_health(&folder);
            match case {
                "marker" => health["server"] = "foreign-server".into(),
                "pid" => health["pid"] = 1.into(),
                "folder" => health["folder_id"] = "another-folder".into(),
                _ => {},
            }
            let server = FixtureServer::new(health, case == "redirect");
            let mut record = fixture_record(&folder, server.url.clone());
            if case == "record" { record.folder = folder.join("elsewhere"); }
            if case == "endpoint" { record.url.push_str("/wrong-path"); }
            atomic_write::write_json_atomic(&record_path(&folder).unwrap(), &record).unwrap();
            assert!(discover_owner(&folder).await.unwrap().is_none(), "{case}");
            assert_eq!(server.credential_requests.load(std::sync::atomic::Ordering::Relaxed), 0, "credentials sent to {case}");
        }
        std::env::remove_var("MOSS_HOME");
    }

    #[tokio::test]
    async fn resolve_owner_distinguishes_stale_replacement_unresponsive_and_storage_failure() {
        let _guard = env_guard();
        let home = moss_home_dir();
        std::env::set_var("MOSS_HOME", &home);
        let folder = tempfile::tempdir().unwrap();
        let folder = folder.path().canonicalize().unwrap();
        let record = fixture_record(&folder, "http://127.0.0.1:1".into());
        atomic_write::write_json_atomic(&record_path(&folder).unwrap(), &record).unwrap();
        let foreign = crate::infra::folder_lock::try_acquire_named(&folder, OWNER_LOCK).unwrap().unwrap();
        let resolve = || resolve_owner(&folder, HostKind::Cli, "new".into(), String::new(), None);
        assert!(matches!(resolve().await.unwrap(), Ownership::Unresponsive));
        assert_eq!(read_record(&folder).unwrap(), Some(record));
        drop(foreign);
        let Ownership::Acquired(guard) = resolve().await.unwrap() else { panic!("released lock must be acquired") };
        assert!(guard.replaced_stale_record);
        let shared = acquire(&folder, HostKind::Cli, "new".into(), String::new()).unwrap();
        assert!(!shared.replaced_stale_record);
        drop(guard);
        drop(shared);
        std::fs::remove_dir_all(home.join("locks")).unwrap();
        std::fs::write(home.join("locks"), "not a directory").unwrap();
        assert!(resolve().await.is_err(), "lock IO failure must never yield a guard");
        std::fs::remove_file(home.join("locks")).unwrap();
        std::fs::remove_dir_all(home.join("servers")).unwrap();
        std::fs::write(home.join("servers"), "not a directory").unwrap();
        assert!(resolve().await.is_err(), "announcement IO failure must remain an error");
        std::env::remove_var("MOSS_HOME");
    }

}
