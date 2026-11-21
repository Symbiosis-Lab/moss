//! Admission, enforced: the process-wide verdict `load_plugin` consults.
//!
//! The other three layers are stateless — [`super::index`] decides whether a
//! fetched document may replace the cached one, [`super::cache`] reads and
//! writes it, [`super::fetch`] goes and gets it. This one holds a snapshot,
//! and it is the only place in the client that does, because `load_plugin`
//! runs on every discovery and cannot afford to re-read and re-validate two
//! files per plugin.
//!
//! The badge in the catalog, the refusal at install and the refusal at the
//! loader all end up here, so no two of them can disagree about whether a
//! plugin may run or about why not. The user's answer for each folder
//! ([`super::trust`]) is held here too, for the same reason.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, RwLock};
use std::time::SystemTime;

use crate::plugins::types::PluginManifest;
use serde::{Deserialize, Serialize};
use specta::Type;
use moss_core::untrusted_text::{bounded, MAX_NAME, MAX_SENTENCE};

use super::trust::{self, TrustRecord};
use super::cache::{self, CachedRegistry, RevocationVerdict};

static SNAPSHOT: RwLock<Option<CachedRegistry>> = RwLock::new(None);

/// The folder answers in force.
///
/// `None`: trust is not enforced. That is every test binary that installs
/// none, and an app with no app-data directory, where nothing is refused for
/// want of an answer (the same posture as a kill list moss never fetched).
static TRUST: RwLock<Option<Trust>> = RwLock::new(None);

struct Trust {
    /// Where answers are written. `None` for a headless build, which reads
    /// the app's answers and never writes them.
    writes_to: Option<PathBuf>,
    record: TrustRecord,
    /// The per-plugin approvals an earlier moss kept, and what they settled
    /// for each folder asked about since.
    legacy: HashMap<PathBuf, String>,
    settled: Mutex<HashMap<PathBuf, bool>>,
}

impl Trust {
    fn read(root: Option<&Path>, writes: bool) -> Self {
        Trust {
            writes_to: root.filter(|_| writes).map(Path::to_path_buf),
            record: root.map(trust::load).unwrap_or_default(),
            legacy: root.map(|root| trust::load_legacy(root, &std::env::temp_dir())).unwrap_or_default(),
            settled: Mutex::default(),
        }
    }

    /// The recorded answer; for a folder never asked, trusted when the
    /// earlier approvals covered every plugin in it at its current code.
    fn answer(&self, folder: &Path) -> Option<bool> {
        self.record.answer(folder).or_else(|| self.settled_by_legacy(folder).then_some(true))
    }

    fn settled_by_legacy(&self, folder: &Path) -> bool {
        if self.legacy.is_empty() {
            return false;
        }
        let mut settled = self.settled.lock().unwrap_or_else(|e| e.into_inner());
        *settled
            .entry(folder.to_path_buf())
            .or_insert_with(|| trust::legacy_covers(&self.legacy, &carried(folder)))
    }
}

/// What [`verdict`] learned about the code in a directory, kept until that
/// code changes.
///
/// `load_plugin` runs on every hook dispatch, so hashing the entry file per
/// call would put a SHA-256 of the plugin's whole bundle on the hot path,
/// per plugin, per dispatch. Keyed by the size, mtime and ctime of the two
/// files the hash covers, and by which file the manifest names as the entry.
/// Size and mtime alone would not do: a sync client preserves the source's
/// mtime, so a shared folder can deliver a same-length entry file carrying
/// different code with the old stamp — the exact case this gate is for.
/// ctime is set by the kernel on every write and no userland process can
/// choose it, which closes that door on every platform that has it.
static CODE: OnceLock<Mutex<HashMap<PathBuf, (Fingerprint, Option<Code>)>>> = OnceLock::new();

/// The entry file's name, then (size, mtime, ctime) of the manifest, the
/// entry and each block asset.
type Fingerprint = (String, Vec<(u64, Option<SystemTime>, (i64, i64))>);

#[derive(Clone)]
struct Code {
    hash: String,
    /// The bytes are this binary's own: the same code hash as the bundled
    /// copy, under the directory name the bundle installs to. The name is
    /// part of it because everything else keys on the directory, and a copy
    /// of github's code under another name is not github.
    shipped: bool,
}

/// The kill list cannot be read and there is no binary vouching for these
/// bytes. Worth being precise about what this is for: an attacker who can
/// corrupt the cached kill list can also delete the cache directory and reach
/// `NotRevoked`, so this is not an adversarial control. It is a guard against
/// ACCIDENTAL corruption — a half-written file, a bad disk — where refusing
/// is free and running is not.
const KILL_LIST_UNREADABLE: &str =
    "moss cannot read the plugin registry's kill list, so it will not run a plugin it did not ship";

/// One refresh in flight at a time. `fetch`'s own lock serializes them
/// correctly, but without this every catalog open would park another thread
/// waiting on it — and a user clicking between tabs would queue a dozen
/// network round-trips to learn the same thing.
static REFRESHING: AtomicBool = AtomicBool::new(false);

/// Read the cache under `registry_dir` and the folder answers under
/// `trust_root`, publish both, and become the verdict the loader asks. The
/// app writes its answers under `trust_root`.
///
/// Synchronous and cheap: three small files off local disk. It must finish
/// before any plugin can load, which is why it is not deferred to the refresh
/// thread — an offline launch still enforces the last kill list moss saw.
pub fn install(registry_dir: Option<&Path>, trust_root: Option<&Path>) {
    if let Some(dir) = registry_dir {
        publish(cache::load(dir));
    }
    if trust_root.is_some() {
        *TRUST.write().unwrap_or_else(|e| e.into_inner()) = Some(Trust::read(trust_root, true));
    }
    // Installed even with no cache to read. Only the kill list needs app data;
    // the version floor compares two strings this binary already knows, and
    // tying it to a directory that may not exist would switch it off for a
    // reason that has nothing to do with it.
    crate::plugins::admission::install_check(refusal);
}

/// The headless build's verdict. Reads the cached kill list under
/// `registry_dir` and the folder answers the app wrote under `trust_root` —
/// never fetches, never writes — and
/// installs one of two checks. Without `--allow-plugins`, the full verdict: a
/// plugin in a folder the app was not told to trust is refused, with the two
/// ways out in the sentence. With it, the command itself is the consent for
/// whatever the folder carries, and only the two grounds no user may waive
/// remain — a withdrawal and a version floor.
///
/// Installed only when plugins are going to run; a `--no-plugins` build has
/// nothing to admit, and `install_check` is set-once for the process.
pub fn install_headless(registry_dir: Option<&Path>, trust_root: Option<&Path>, allow_plugins: bool) {
    if let Some(dir) = registry_dir {
        publish(cache::load(dir));
    }
    // With no trust root nothing was ever trusted, which is not the same as
    // trust going unenforced: the record is installed empty, so headless is
    // always the enforced state.
    *TRUST.write().unwrap_or_else(|e| e.into_inner()) = Some(Trust::read(trust_root, false));
    crate::plugins::admission::install_check(if allow_plugins {
        refusal_without_consent
    } else {
        refusal_headless
    });
}

/// [`install_headless`] for the running app: the shared registry cache, and
/// the answers the app this binary belongs to recorded, so a local build of
/// the app reads its own. Unlike [`install_headless`] it may write: the first
/// call copies the approvals an earlier moss kept in the roaming root.
pub fn install_headless_for_running_app(allow_plugins: bool) {
    let trust = crate::infra::app_data::AppRoots::running().map(|roots| trust::root(&roots));
    install_headless(crate::infra::app_data::app_data_dir_early().as_deref(), trust.as_deref(), allow_plugins);
}

/// Install the cached verdict, then refresh it off-thread.
///
/// Called once at setup. The refresh is a separate thread rather than an async
/// task because [`super::fetch`] is blocking and a slow or hostile origin must
/// not hold up the first window.
pub fn install_and_refresh(trust_root: Option<&Path>) {
    let dir = crate::infra::app_data::app_data_dir_early();
    if dir.is_none() {
        log::warn!(
            target: "registry",
            "no app-data directory: the plugin kill list cannot be read, so no plugin will be refused as withdrawn"
        );
    }
    install(dir.as_deref(), trust_root);
    refresh_in_background();
}

/// Re-fetch and re-publish, off-thread. Safe to call as often as a surface
/// opens; at most one runs at a time and each failure mode leaves the cached
/// kill list in force.
///
/// Called from the catalog, because a kill switch whose latency is "restart
/// the app" is a weak one: a session left open for days would otherwise never
/// learn that a plugin it is running has been withdrawn.
pub fn refresh_in_background() {
    let Some(dir) = crate::infra::app_data::app_data_dir_early() else { return };
    if REFRESHING.swap(true, Ordering::SeqCst) {
        return;
    }
    let dir: PathBuf = dir;
    std::thread::spawn(move || {
        let refreshed = super::fetch::refresh(&dir);
        log::info!(
            target: "registry",
            "registry refresh: index {:?}, revoked {:?}",
            refreshed.index, refreshed.revoked
        );
        publish(refreshed.cache);
        REFRESHING.store(false, Ordering::SeqCst);

        // Icons LAST, and after `publish` rather than inside `refresh`, so a
        // withdrawal is in force the moment it arrives. They are cosmetic and
        // they are slow — an unreachable icon host costs the full connect and
        // read timeout, serially, per entry — and running them first would
        // have left `revoked.json` fetched but not published for as long as
        // that took, with the kill list still answering from the old snapshot
        // and `REFRESHING` set so nothing could retry. `cached_entries` reads
        // the snapshot just published, so a rejected index cannot make moss
        // fetch anything.
        // Two refreshes overlapping here is fine and is the price of clearing
        // REFRESHING first: `cache_icons` stages under a name unique to the
        // attempt and renames, so the loser costs a duplicate download.
        super::icon::cache_icons(&dir, &cached_entries());
    });
}

/// Re-fetch on the calling thread when the cached index says what no plugin
/// contributes, at most once a run. Answers whether it fetched.
///
/// Such an index was published before entries named their kinds, so a surface
/// that offers one kind would sort every plugin moss does not ship into the
/// wrong place until the background refresh landed and the surface was opened
/// again. It waits for the network, for as long as the connect and read
/// timeouts allow, so call it off the async runtime. A failed fetch leaves the
/// cache as it was. Without this call nothing breaks: the same rows are
/// classified one open later.
pub fn refresh_once_if_index_names_no_kinds() -> bool {
    static TRIED: AtomicBool = AtomicBool::new(false);
    if !names_no_kinds(&cached_entries()) || TRIED.swap(true, Ordering::SeqCst) {
        return false;
    }
    let Some(dir) = crate::infra::app_data::app_data_dir_early() else { return false };
    publish(super::fetch::refresh(&dir).cache);
    true
}

fn names_no_kinds(entries: &[super::index::IndexEntry]) -> bool {
    !entries.is_empty() && entries.iter().all(|entry| entry.contributes.is_empty())
}

/// Replace the snapshot — after a refresh, or from a test.
pub fn publish(cache: CachedRegistry) {
    if cache.revoked_is_unreadable() {
        log::warn!(
            target: "registry",
            "the cached kill list is unreadable; no plugin will be refused until the next successful refresh"
        );
    }
    if let Ok(mut guard) = SNAPSHOT.write() {
        *guard = Some(cache);
    }
}

/// Why a plugin will not run, and which of the three reasons it is.
///
/// One record rather than three `Option` fields side by side. The precedence
/// — a withdrawal outranks a version floor, which outranks an untrusted folder —
/// then exists once, in [`verdict`], instead of being re-derived by every
/// screen that renders it; and the state no reader knows how to draw, two
/// reasons set at once, cannot be built. The kind is kept because the
/// remedies differ: a withdrawal is permanent and the plugin must go, a floor
/// is met by updating moss, and trust is the folder's answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Refusal {
    /// The registry withdrew this exact version. `reason` is the registry's
    /// own published sentence — a third party's words, which moss quotes
    /// rather than writes, so it crosses the seam bounded and stripped.
    Revoked { reason: String },
    /// The installed copy declares a `min_moss_version` this moss does not
    /// meet. Two version numbers, not a sentence: the sentence around them is
    /// moss's own and therefore belongs to the locale, not to Rust. Sending
    /// prose here printed it twice — once in English from here, once inside
    /// the translated frame that already said the same thing.
    Incompatible { needs: String, host: String },
    /// moss did not ship these bytes and the user has not trusted the folder
    /// they are in. Opening someone else's folder must not run their code
    /// before a single screen is shown, so the folder's plugins wait for its
    /// answer — the remedy is trusting the folder, not an update or a
    /// removal, which is why it is its own kind.
    Untrusted,
    /// The plugin's native program is not published for this computer.
    /// Platform keys rather than a sentence, so the screen words it in the
    /// locale; [`Refusal::sentence`] is the English one-liner.
    Unavailable { platforms: Vec<String> },
    /// The installed copy's `min_moss_version` is not a version moss can
    /// compare. Without a readable floor moss cannot tell whether the plugin
    /// was built for host functions this moss has, so it does not run it.
    UnreadableFloor { floor: String },
}

impl Refusal {
    /// The refusal as one line, for a log and for an install command's error.
    ///
    /// English on purpose, and not a duplicate of the tooltip: neither of
    /// those places has a locale. The screen builds its own sentence from the
    /// same verdict. The plugin's own strings are bounded and stripped on the
    /// way in — a direction override inside the version would otherwise
    /// reverse moss's sentence, at whatever length the manifest chose.
    pub fn sentence(&self, manifest: &PluginManifest) -> String {
        let name = bounded(&manifest.name, MAX_NAME);
        let version = bounded(&manifest.version, MAX_NAME);
        match self {
            Refusal::Revoked { reason } => {
                format!("{name} {version} was withdrawn by the plugin registry: {reason}")
            }
            Refusal::Incompatible { needs, host } => {
                format!("{name} {version} needs moss {needs} or newer, and this is moss {host}")
            }
            Refusal::Untrusted => format!("{name} {version} is off until you trust this folder"),
            Refusal::Unavailable { platforms } => {
                crate::plugins::contributions::platform::sentence(&name, platforms)
            }
            Refusal::UnreadableFloor { floor } => {
                format!("{name} {version}'s manifest names a moss version moss can't read: {floor}")
            }
        }
    }
}

/// Everything that can refuse an installed plugin, in one verdict — what the
/// catalog badges with, what the publish rail disables on, and what
/// [`refusal`] hands the loader.
///
/// What vouches for the plugin decides what is asked. Bytes moss shipped are
/// vouched for by the notarized binary they arrived in, so they are neither
/// held to a floor nor wait for the folder's answer; anything else needs the
/// registry's word that it is not withdrawn, a floor this moss meets, and a
/// folder the user trusts.
pub fn verdict(manifest: &PluginManifest, plugin_dir: &Path) -> Option<Refusal> {
    verdict_when(manifest, plugin_dir, false)
}

/// [`verdict`] with consent already given (`--allow-plugins`): the
/// same two withholding grounds, and the folder's answer not asked.
fn verdict_when(manifest: &PluginManifest, plugin_dir: &Path, consent_given: bool) -> Option<Refusal> {
    let shipped = code_of(manifest, plugin_dir).is_some_and(|code| code.shipped);
    if let Some(refusal) = withheld(manifest, shipped) {
        return Some(refusal);
    }
    (!consent_given && !vouched_for(manifest, plugin_dir)).then_some(Refusal::Untrusted)
}

/// Does anything vouch for the code now in `plugin_dir`: did moss ship these
/// exact bytes, or does the user trust the folder they are in?
///
/// The trust half of [`verdict`] — that one returns `Untrusted` exactly when
/// nothing does, so the rule lives here and not in both.
///
/// Asked on its own for a row's IDENTITY, which is not the same question as
/// whether the plugin may run. Catalog rows merge by id, so a folder that
/// carries `.moss/plugins/github/` wears the name, description and origin
/// moss holds for `github` over whatever bytes are actually there; the row is
/// only honest about them once nothing vouches for them. Asked separately
/// from [`verdict`] because that one answers with the refusal of highest
/// precedence: a withdrawal or a version floor outranks an untrusted folder,
/// so a refused row can still be code nobody vouched for.
pub fn vouched_for(manifest: &PluginManifest, plugin_dir: &Path) -> bool {
    code_of(manifest, plugin_dir).is_some_and(|code| code.shipped)
        || folder_of(plugin_dir).is_some_and(|folder| is_trusted(&folder))
}

/// Are these moss's own bytes: shipped in this binary, or exactly the code
/// moss's registry lists for this id? What a host call made on the site's
/// behalf asks, since trusting a folder to run its plugins is not consent to
/// sign for them.
pub fn is_moss_own(manifest: &PluginManifest, plugin_dir: &Path) -> bool {
    code_of(manifest, plugin_dir).is_some_and(|code| code.shipped)
        || matches!(registry_status(manifest, plugin_dir), RegistryStatus::ListedExact { .. })
}

/// The two grounds that refuse a plugin whatever the user has said —
/// withdrawn, or built for a newer moss. [`verdict`] asks them first; an
/// install asks them alone, about the manifest that came out of the archive,
/// since nothing on disk can be trusted before it exists.
///
/// `shipped` is whether the bytes are the binary's own. Those are exempt from
/// the floor, and from the unreadable-kill-list refusal, because the binary
/// they arrived in is what vouches for them and a floor cannot tell it
/// something it does not already know. It is not a theoretical carve-out —
/// github and matters both currently declare `0.11.7` while develop builds
/// `0.11.6`, because the floor is written for the release the plugin ships in
/// and develop is always behind it. Enforcing it there would disable moss's
/// own publishing on every development build. A published withdrawal binds
/// even them: the registry's word outranks the binary's.
pub fn withheld(manifest: &PluginManifest, shipped: bool) -> Option<Refusal> {
    match revocation(&manifest.name, &manifest.version) {
        Revocation::Withdrawn(reason) => return Some(Refusal::Revoked { reason }),
        Revocation::Unreadable if !shipped => {
            return Some(Refusal::Revoked { reason: KILL_LIST_UNREADABLE.to_string() })
        }
        _ => {}
    }
    if shipped {
        return None;
    }
    outran_its_host(manifest)
}

/// [`verdict`] as the loader's one-line refusal, which is the shape
/// `admission::install_check` takes.
pub fn refusal(manifest: &PluginManifest, plugin_dir: &Path) -> Option<String> {
    verdict(manifest, plugin_dir).map(|refusal| refusal.sentence(manifest))
}

/// [`refusal`] for a build with no window: the trust refusal names its two
/// remedies, since there is no question to answer.
fn refusal_headless(manifest: &PluginManifest, plugin_dir: &Path) -> Option<String> {
    verdict(manifest, plugin_dir).map(|refusal| match refusal {
        Refusal::Untrusted => format!(
            "{}; pass --allow-plugins to run what this folder carries, or open the folder in the moss app and trust it",
            refusal.sentence(manifest)
        ),
        other => other.sentence(manifest),
    })
}

/// [`withheld`] alone: the verdict under `--allow-plugins`, where the command
/// is the consent and only the registry's word and the version floor remain.
fn refusal_without_consent(manifest: &PluginManifest, plugin_dir: &Path) -> Option<String> {
    verdict_when(manifest, plugin_dir, true).map(|refusal| refusal.sentence(manifest))
}

/// The code in `plugin_dir`, hashed once and remembered until it changes.
fn code_of(manifest: &PluginManifest, plugin_dir: &Path) -> Option<Code> {
    let mut hashed = vec!["manifest.json", manifest.entry.as_deref().unwrap_or("")];
    hashed.extend(crate::plugins::contributions::blocks::asset_paths(manifest));
    let stamps = hashed.into_iter().map(|name| {
        let meta = std::fs::metadata(plugin_dir.join(name)).ok();
        #[cfg(unix)]
        let ctime = {
            use std::os::unix::fs::MetadataExt;
            meta.as_ref().map_or((0, 0), |m| (m.ctime(), m.ctime_nsec()))
        };
        #[cfg(not(unix))]
        let ctime = (0, 0);
        (meta.as_ref().map_or(0, |m| m.len()), meta.and_then(|m| m.modified().ok()), ctime)
    }).collect();
    let fingerprint: Fingerprint = (manifest.entry.clone().unwrap_or_default(), stamps);
    let memo = CODE.get_or_init(Default::default);
    if let Some((seen, code)) = memo.lock().unwrap_or_else(|e| e.into_inner()).get(plugin_dir) {
        if *seen == fingerprint {
            return code.clone();
        }
    }
    let code = crate::plugins::bundled::code_hash(plugin_dir, manifest).map(|hash| Code {
        shipped: plugin_dir.file_name().is_some_and(|dir| dir == manifest.name.as_str())
            && crate::plugins::bundled::bundled_code_hash(&manifest.name).as_deref() == Some(hash.as_str()),
        hash,
    });
    memo.lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(plugin_dir.to_path_buf(), (fingerprint, code.clone()));
    code
}

/// The folder a plugin directory belongs to: the one that holds
/// `.moss/plugins/<id>`. `None` for a directory of any other shape, which no
/// folder answer covers.
pub fn folder_of(plugin_dir: &Path) -> Option<PathBuf> {
    let plugins = plugin_dir.parent()?;
    let moss = plugins.parent()?;
    (plugins.file_name()? == "plugins" && moss.file_name()? == ".moss").then(|| moss.parent().map(Path::to_path_buf))?
}

/// Whether the user trusts `folder`, or nothing enforces trust.
///
/// Keyed by the canonical folder, so a folder reached through two paths is
/// one folder; falls back to the path as given when it cannot be
/// canonicalized, which is also what [`set_folder_trust`] does, so the two
/// agree.
pub fn is_trusted(folder: &Path) -> bool {
    let guard = TRUST.read().unwrap_or_else(|e| e.into_inner());
    guard.as_ref().is_none_or(|trust| trust.answer(&canonical(folder)) == Some(true))
}

/// The user's answer for `folder`, for the app to decide whether to ask:
/// `None` when it was never asked. A folder the earlier per-plugin approvals
/// settle is recorded as trusted here, the first time the app asks about it,
/// so it keeps its answer once that file is gone. `Some(true)` when nothing
/// enforces trust.
pub fn folder_answer(folder: &Path) -> Option<bool> {
    let folder = canonical(folder);
    let (recorded, answer) = {
        let guard = TRUST.read().unwrap_or_else(|e| e.into_inner());
        let Some(trust) = guard.as_ref() else { return Some(true) };
        (trust.record.answer(&folder), trust.answer(&folder))
    };
    if recorded.is_none() && answer == Some(true) {
        if let Err(error) = set_folder_trust(&folder, true) {
            log::warn!(target: "registry", "could not record {} as trusted: {error}", folder.display());
        }
    }
    answer
}

/// Record the user's answer for `folder` and put it in force at once. Only
/// the app records answers: a headless build holds a record it reads and
/// never writes.
pub fn set_folder_trust(folder: &Path, trusted: bool) -> Result<(), String> {
    let mut guard = TRUST.write().unwrap_or_else(|e| e.into_inner());
    // Nothing enforces trust without a record, so there is nothing to keep.
    let Some(trust) = guard.as_mut() else { return Ok(()) };
    let root = trust.writes_to.clone().ok_or("this moss reads folder answers and does not record them")?;
    let mut record = trust.record.clone();
    record.set(canonical(folder), trusted);
    trust::save(&root, &record)?;
    trust.record = record;
    Ok(())
}

/// Each plugin directory in `folder` that moss did not ship, canonical, with
/// the hash of its code now (`None` when it cannot be read).
pub fn carried(folder: &Path) -> Vec<(PathBuf, Option<String>)> {
    let Ok(entries) = std::fs::read_dir(folder.join(".moss").join("plugins")) else { return Vec::new() };
    entries
        .flatten()
        .map(|entry| canonical(&entry.path()))
        .filter_map(|dir| Some((crate::plugins::bundled::read_installed_manifest(&dir)?, dir)))
        .filter_map(|(manifest, dir)| match code_of(&manifest, &dir) {
            Some(code) if code.shipped => None,
            code => Some((dir, code.map(|code| code.hash))),
        })
        .collect()
}

/// What the registry's last accepted index says about a plugin directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RegistryStatus {
    /// The index lists this id with a `code_sha256` equal to this directory's
    /// code hash: the registry vouches for these exact bytes.
    ListedExact { version: String },
    /// The index lists this id at this directory's version, but its entry
    /// carries no code hash to compare or carries one that differs from this
    /// directory's. Only the name and version are vouched for, not the code.
    ListedVersion,
    /// The index does not list this id and version, or moss has no index.
    Unknown,
    /// The registry withdrew this exact version; `reason` is its own sentence,
    /// bounded and stripped.
    Withdrawn { reason: String },
}

/// [`RegistryStatus`] of the plugin in `plugin_dir`, from the snapshot the
/// loader consults. A withdrawal outranks a listing. A directory whose version
/// is older or newer than the one the index lists is `Unknown`. An unreadable kill list
/// is not a withdrawal here; [`verdict`] is where it stops a plugin.
pub fn registry_status(manifest: &PluginManifest, plugin_dir: &Path) -> RegistryStatus {
    let withdrawn = match revocation(&manifest.name, &manifest.version) {
        Revocation::Withdrawn(reason) => Some(reason),
        Revocation::Clear | Revocation::Unreadable => None,
    };
    let hash = code_of(manifest, plugin_dir).map(|code| code.hash);
    registry_status_in(&cached_entries(), withdrawn, manifest, hash.as_deref())
}

fn registry_status_in(
    entries: &[super::index::IndexEntry],
    withdrawn: Option<String>,
    manifest: &PluginManifest,
    hash: Option<&str>,
) -> RegistryStatus {
    if let Some(reason) = withdrawn {
        return RegistryStatus::Withdrawn { reason };
    }
    let Some(entry) = entries.iter().find(|e| e.id == manifest.name) else { return RegistryStatus::Unknown };
    if hash.is_some() && entry.code_sha256.as_deref() == hash {
        RegistryStatus::ListedExact { version: entry.version.clone() }
    } else if entry.version == manifest.version {
        RegistryStatus::ListedVersion
    } else {
        RegistryStatus::Unknown
    }
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// The refusal when a plugin needs a newer moss than this one.
///
/// A plugin built against a moss that has host functions this one lacks fails
/// somewhere deep in a hook dispatch, as a JavaScript error the author sees and
/// the user cannot act on. The floor turns that into one readable sentence at
/// the point of use.
///
/// Not applied to the bytes moss itself ships — [`withheld`] says why and
/// does not ask.
///
/// A floor moss cannot parse refuses too, as its own kind: a plugin whose
/// floor cannot be compared may need host functions this moss lacks, and
/// running it would fail the same deep, unreadable way the floor exists to
/// prevent. The sentence names the unreadable value, so the author can fix it.
fn outran_its_host(manifest: &PluginManifest) -> Option<Refusal> {
    let floor = manifest.min_moss_version.as_deref()?;
    // The floor is publisher-authored inside the artifact (the hash pins
    // provenance, not intent) and it lands in an `aria-label`, so it is
    // bounded like a revocation reason. `host` is moss's own and needs none.
    match crate::system::meets_floor(floor) {
        Some(true) => None,
        Some(false) => Some(Refusal::Incompatible {
            needs: bounded(floor, MAX_NAME),
            host: crate::system::app_version().to_string(),
        }),
        None => Some(Refusal::UnreadableFloor { floor: bounded(floor, MAX_NAME) }),
    }
}

/// What the kill list says about one id+version, with the published reason
/// already bounded and stripped — a third party's string that moss frames in
/// a sentence of its own and puts in `aria-label`, where a direction
/// override reverses the explanation and not just the reason.
enum Revocation {
    Withdrawn(String),
    Clear,
    Unreadable,
}

fn revocation(id: &str, version: &str) -> Revocation {
    // Recover from poisoning rather than propagating it. A panic elsewhere
    // while this lock was held says nothing about the snapshot, and `.ok()?`
    // would answer "nothing is revoked" for the rest of the process — a
    // security gate switching itself off, quietly. Same reasoning and same
    // call as `fetch::REFRESH_LOCK`.
    let guard = SNAPSHOT.read().unwrap_or_else(|e| e.into_inner());
    let Some(cache) = guard.as_ref() else { return Revocation::Clear };
    match cache.revocation_for(id, version) {
        RevocationVerdict::Revoked(revocation) => {
            Revocation::Withdrawn(bounded(&revocation.reason, MAX_SENTENCE))
        }
        RevocationVerdict::NotRevoked => Revocation::Clear,
        RevocationVerdict::Unknown => Revocation::Unreadable,
    }
}

/// Why an OFFER — a published entry nothing on disk can be tested against —
/// would be refused: the registry withdrew it, or moss cannot currently hear
/// the registry. Nothing offered is the binary's own, so there is no
/// exemption to ask about here; [`withheld`] holds the one there is.
pub fn revoked_reason(id: &str, version: &str) -> Option<String> {
    match revocation(id, version) {
        Revocation::Withdrawn(reason) => Some(reason),
        Revocation::Clear => None,
        Revocation::Unreadable => Some(KILL_LIST_UNREADABLE.to_string()),
    }
}

/// The catalog the last accepted index published.
///
/// Empty when moss has never successfully fetched one, which is the first-run
/// and offline-forever state — the bundled list is the whole catalog then, and
/// that is the design's permanent baseline rather than a degraded mode.
///
/// Clones: a handful of small entries, read when a catalog surface opens, and
/// the alternative is holding the snapshot lock across catalog assembly.
pub fn cached_entries() -> Vec<super::index::IndexEntry> {
    let guard = SNAPSHOT.read().unwrap_or_else(|e| e.into_inner());
    guard
        .as_ref()
        .and_then(|cache| cache.index.as_ref())
        .map(|index| index.plugins().into_iter().cloned().collect())
        .unwrap_or_default()
}

/// The catalog's current entry for `plugin_id`, from a catalog that may be
/// installed from (see [`CachedRegistry::index_to_install_from`]).
pub fn installable_entry(plugin_id: &str) -> Result<super::index::IndexEntry, String> {
    let guard = SNAPSHOT.read().unwrap_or_else(|e| e.into_inner());
    let dated = !super::signature::REGISTRY_KEYS.is_empty();
    let index = match guard.as_ref() {
        Some(cache) => cache.index_to_install_from(chrono::Utc::now(), dated)?,
        None => None,
    };
    index
        .and_then(|index| index.find(plugin_id).cloned())
        .ok_or_else(|| format!("Plugin '{plugin_id}' is not in this build or the catalog"))
}

/// Whether a version this catalog offers can actually run here.
///
/// The registry publishes one entry per id — the latest — and that latest can
/// declare a floor above this moss: github's live entry needs 0.11.7 while a
/// 0.11.6 app is asking. Offering it as an update would produce an install
/// that is refused at load, so an unrunnable version is not an update.
pub fn host_meets_floor(min_moss_version: Option<&str>) -> bool {
    min_moss_version.is_none_or(|floor| crate::system::meets_floor(floor) == Some(true))
}

#[cfg(test)]
mod tests {
    /// A block's script runs in readers' browsers, so swapping it after the
    /// user allowed the plugin has to ask again.
    #[test]
    fn a_changed_block_asset_is_different_code() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("manifest.json"), "{}").unwrap();
        std::fs::write(dir.path().join("view.js"), "one").unwrap();
        let manifest = crate::plugins::types::PluginManifest::parse(
            r#"{"name":"cards","version":"1.0.0","contributes":{"blocks":{"card":{"element":"demo-card","script":"view.js"}}}}"#,
        ).unwrap();
        let before = code_of(&manifest, dir.path()).unwrap().hash;
        std::fs::write(dir.path().join("view.js"), "a longer script").unwrap();
        assert_ne!(before, code_of(&manifest, dir.path()).unwrap().hash);
    }

    use super::*;
    use crate::plugins::install::registry_client::cache::Anchor;
    use crate::plugins::install::registry_client::index::accept_revoked;

    fn manifest_needing(floor: Option<&str>) -> PluginManifest {
        let floor = floor.map(|v| format!(r#","min_moss_version":"{v}""#)).unwrap_or_default();
        PluginManifest::parse(&format!(
            r#"{{"name":"not-a-plugin-moss-ships","version":"2.0.0","entry":"index.js"{floor}}}"#
        ))
        .unwrap()
    }

    fn manifest_of(name: &str, version: &str) -> PluginManifest {
        PluginManifest::parse(&format!(
            r#"{{"name":"{name}","version":"{version}","entry":"index.js"}}"#
        ))
        .unwrap()
    }

    /// Only an index in which nothing names a kind is refetched: one named
    /// kind means the registry lists them, and an entry without any is a
    /// plugin that contributes nothing.
    #[test]
    fn an_index_is_refetched_only_when_no_entry_names_a_kind() {
        let entry = |id: &str, kinds: &[&str]| -> crate::plugins::install::registry_client::index::IndexEntry {
            serde_json::from_value(serde_json::json!({
                "type": "plugin", "id": id, "display_name": id, "version": "1.0.0",
                "download_url": "https://example.invalid/x.zip", "sha256": "aa",
                "contributes": kinds,
            }))
            .unwrap()
        };
        assert!(names_no_kinds(&[entry("a", &[]), entry("b", &[])]));
        assert!(!names_no_kinds(&[entry("a", &[]), entry("b", &["channel"])]));
        assert!(!names_no_kinds(&[]), "nothing cached is not a stale index");
    }

    /// Pure — no [`SNAPSHOT`] — so unlike the test below it can be several.
    ///
    /// Both numbers, and NOT a sentence: the user cannot act on a floor
    /// without knowing what they are running, and the prose saying so is the
    /// locale's. An English sentence here would print twice, once from Rust
    /// and once inside the translated frame that already says it.
    #[test]
    fn a_floor_above_this_moss_refuses_and_carries_both_versions() {
        assert_eq!(
            outran_its_host(&manifest_needing(Some("99.0.0"))),
            Some(Refusal::Incompatible {
                needs: "99.0.0".to_string(),
                host: crate::system::app_version().to_string(),
            })
        );
    }

    /// The sentence moss writes around a plugin's own strings.
    ///
    /// The name and version come out of the artifact's manifest — the hash
    /// pins who submitted them, not what they say — and [`refusal`] frames
    /// them for a log and for `install_plugin`'s error. Left raw, a direction
    /// override inside the version reverses moss's own explanation, at
    /// whatever length the manifest chose.
    #[test]
    fn the_plugin_does_not_get_to_write_moss_s_sentence() {
        let hostile = PluginManifest::parse(&format!(
            r#"{{"name":"not-a-plugin-moss-ships","version":"2.0.0\u202e{}","entry":"index.js","min_moss_version":"99.0.0"}}"#,
            "x".repeat(600)
        ))
        .unwrap();
        let sentence = outran_its_host(&hostile).expect("the floor still refuses").sentence(&hostile);
        assert!(!sentence.contains('\u{202e}'), "no direction override: {sentence}");
        assert!(sentence.chars().count() < 200, "bounded, not 600 characters: {sentence}");
        assert!(sentence.contains("needs moss 99.0.0"), "{sentence}");
    }

    /// The branch that keeps moss's own publishing alive on a development
    /// build. github and matters declare `min_moss_version` `0.11.7` today
    /// while develop builds `0.11.6`, so without this carve-out neither
    /// bundled plugin loads at all — every day until the release that bumps
    /// the app. Whether bytes ARE the binary's own is `bundled`'s exact test,
    /// proved there; this proves the exemption hangs on that answer alone.
    #[test]
    fn the_bytes_moss_ships_are_not_held_to_the_floor_they_declare() {
        let too_new = manifest_needing(Some("99.0.0"));
        assert_eq!(withheld(&too_new, true), None, "the binary vouches for its own bytes");
        assert!(withheld(&too_new, false).is_some(), "anything else is held to its floor");
    }

    #[test]
    fn the_floor_this_moss_meets_loads() {
        assert_eq!(outran_its_host(&manifest_needing(Some(crate::system::app_version()))), None, "equal is met");
        assert_eq!(outran_its_host(&manifest_needing(Some("0.0.1"))), None);
        assert_eq!(outran_its_host(&manifest_needing(None)), None, "no floor is no gate");
    }

    #[test]
    fn a_floor_moss_cannot_read_is_refused_and_not_offered() {
        let unreadable = manifest_needing(Some("whenever"));
        let refusal = outran_its_host(&unreadable).expect("an unreadable floor refuses");
        assert_eq!(refusal, Refusal::UnreadableFloor { floor: "whenever".into() });
        assert_eq!(
            refusal.sentence(&unreadable),
            "not-a-plugin-moss-ships 2.0.0's manifest names a moss version moss can't read: whenever"
        );
        assert!(!host_meets_floor(Some("whenever")), "the catalog hides it");
        assert!(host_meets_floor(None));
    }

    /// One test: [`SNAPSHOT`] is process-wide, so two tests publishing
    /// different snapshots would race over what the third one reads.
    #[test]
    fn what_an_unreadable_kill_list_stops_depends_on_what_vouched_for_the_plugin() {
        // The reason carries a direction override: it is a publisher's string
        // that moss frames in a sentence of its own and puts in `aria-label`,
        // where an override reverses the explanation and not just the reason.
        // Length is bounded by the same call, tested where it is pure.
        let list = accept_revoked(
            r#"{"schema_version":1,"serial":2,"revocations":[
                 {"id":"matters","versions":["1.0.0"],"reason":"leaked\u202e reader addresses"}]}"#,
            0,
            0,
        )
        .unwrap();

        publish(CachedRegistry {
            index: None,
            revoked: Some(list),
            anchor: Anchor { highest_revoked_serial: 2, revocation_count: 1, ..Anchor::default() },
        });

        // Cleaned on the way out — the override the fixture carries is gone
        // and the sentence is intact. Length is bounded by the same call,
        // tested where it is pure (`moss_core::untrusted_text`).
        assert_eq!(revoked_reason("matters", "1.0.0").as_deref(), Some("leaked reader addresses"));
        assert_eq!(revoked_reason("matters", "1.0.1"), None, "revocation is per version");
        assert_eq!(revoked_reason("github", "1.0.0"), None);
        assert!(
            matches!(
                withheld(&manifest_of("matters", "1.0.0"), true),
                Some(Refusal::Revoked { ref reason }) if reason == "leaked reader addresses"
            ),
            "a published withdrawal binds even the bytes moss shipped"
        );

        // Now the cached list is gone but the anchor remembers there was one —
        // `Unknown`. A plugin moss ships still loads, because the binary it
        // arrived in is what vouches for it; anything else does not, because
        // the registry was the only thing vouching and moss cannot hear it.
        publish(CachedRegistry {
            index: None,
            revoked: None,
            anchor: Anchor {
                highest_revoked_serial: 2,
                revocation_count: 1,
                revoked_fetched_at: Some("2026-08-30T00:00:00Z".to_string()),
                ..Anchor::default()
            },
        });

        let github = manifest_of("github", "1.0.0");
        assert_eq!(withheld(&github, true), None, "the bytes moss shipped still load");
        assert!(
            withheld(&github, false).is_some(),
            "the same manifest from anywhere else has nothing left vouching for it"
        );
        assert!(revoked_reason("github", "1.0.0").is_some(), "and nothing offered is moss's own");

        // And a cache that never held a kill list is the first-run state, not
        // a damaged one — nothing is refused.
        publish(CachedRegistry::default());
        assert_eq!(revoked_reason("some-third-party-plugin", "1.0.0"), None);
        assert_eq!(withheld(&manifest_of("some-third-party-plugin", "1.0.0"), false), None);
    }

    /// A plugin whose files a cloud app still holds hashes to its code once
    /// they arrive, instead of coming back `None`: the earlier approvals settle
    /// a folder by that hash. Before macOS 14 an
    /// undownloaded file is absent from its own path with a `.name.icloud`
    /// placeholder beside it; the files arrive while the hash waits.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_plugin_with_files_still_in_the_cloud_hashes_to_its_code() {
        let dir = tempfile::TempDir::new().unwrap();
        let plugin_dir = dir.path().join("allowed-demo");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        let manifest_path = plugin_dir.join("manifest.json");
        let entry_path = plugin_dir.join("index.js");
        let manifest_text = r#"{"name":"allowed-demo","version":"1.0.0","description":"d","capabilities":["deploy"],"entry":"index.js"}"#;
        let manifest = PluginManifest::parse(manifest_text).unwrap();
        std::fs::write(&manifest_path, manifest_text).unwrap();
        std::fs::write(&entry_path, "var AllowedDemo = {};").unwrap();

        let before = crate::plugins::bundled::code_hash(&plugin_dir, &manifest).unwrap();

        std::fs::remove_file(&manifest_path).unwrap();
        std::fs::remove_file(&entry_path).unwrap();
        std::fs::write(plugin_dir.join(".manifest.json.icloud"), b"").unwrap();
        std::fs::write(plugin_dir.join(".index.js.icloud"), b"").unwrap();
        let download = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            std::fs::write(manifest_path, manifest_text).unwrap();
            std::fs::write(entry_path, "var AllowedDemo = {};").unwrap();
        });

        let code = code_of(&manifest, &plugin_dir).expect("the files arrive while the hash waits");
        download.join().unwrap();
        assert_eq!(code.hash, before);
    }
}
