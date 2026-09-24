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
//! plugin may run or about why not. The user's consent record
//! ([`super::approval`]) is held here too, for the same reason.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, RwLock};
use std::time::SystemTime;

use crate::plugins::types::PluginManifest;
use serde::{Deserialize, Serialize};
use specta::Type;
use moss_core::untrusted_text::{bounded, MAX_NAME, MAX_SENTENCE};
use semver::Version;

use super::approval::{self, Approvals};
use super::cache::{self, CachedRegistry, RevocationVerdict};

static SNAPSHOT: RwLock<Option<CachedRegistry>> = RwLock::new(None);

/// The approvals in force and the directory they were read from.
///
/// `None`: consent is not enforced — every test binary, and an app with no
/// app-data directory, where nothing is refused for want of it (the same
/// posture as a kill list moss never fetched). `Some((None, _))`: enforced
/// with nothing on disk — a headless build on a box with no app data, where
/// nothing was ever approved because nothing was ever asked.
/// `Some((Some(dir), _))`: the app's record, read from and written to `dir`.
static CONSENT: RwLock<Option<(Option<PathBuf>, Approvals)>> = RwLock::new(None);

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

/// The entry file's name, then (size, mtime, ctime) of the manifest and of
/// the entry.
type Fingerprint = (String, [(u64, Option<SystemTime>, (i64, i64)); 2]);

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

/// Read the cache, publish it, and become the verdict the loader asks.
///
/// Synchronous and cheap: two small files off local disk. It must finish
/// before any plugin can load, which is why it is not deferred to the refresh
/// thread — an offline launch still enforces the last kill list moss saw.
pub fn install(app_data_dir: Option<&Path>) {
    put_in_force(app_data_dir);
    // Installed even with no cache to read. Only the kill list needs app data;
    // the version floor compares two strings this binary already knows, and
    // tying it to a directory that may not exist would switch it off for a
    // reason that has nothing to do with it.
    crate::plugins::admission::install_check(refusal);
}

/// The headless build's verdict. Reads the same cached kill list
/// and the same approvals the app wrote — never fetches — and installs one of
/// two checks. Without `--allow-plugins`, the full verdict: a plugin the app
/// has not been told to allow is refused, with the two ways out in the
/// sentence. With it, the command itself is the consent for whatever the
/// folder carries, and only the two grounds no user may waive remain — a
/// withdrawal and a version floor.
///
/// Installed only when plugins are going to run; a `--no-plugins` build has
/// nothing to admit, and `install_check` is set-once for the process.
pub fn install_headless(allow_plugins: bool) {
    let dir = crate::infra::app_data::app_data_dir_early();
    put_in_force(dir.as_deref());
    if dir.is_none() {
        // No app data means no approvals were ever recorded, not that
        // everything is approved: with `CONSENT` left `None` this build would
        // behave as if the flag were passed, silently. Seed an empty record
        // instead, so headless is always the enforced state.
        *CONSENT.write().unwrap_or_else(|e| e.into_inner()) = Some((None, Approvals::default()));
    }
    crate::plugins::admission::install_check(if allow_plugins {
        refusal_without_consent
    } else {
        refusal_headless
    });
}

/// Read the cache and the approvals under `app_data_dir` and publish both.
fn put_in_force(app_data_dir: Option<&Path>) {
    if let Some(dir) = app_data_dir {
        publish(cache::load(dir));
        *CONSENT.write().unwrap_or_else(|e| e.into_inner()) =
            Some((Some(dir.to_path_buf()), approval::load(dir)));
    }
}

/// Install the cached verdict, then refresh it off-thread.
///
/// Called once at setup. The refresh is a separate thread rather than an async
/// task because [`super::fetch`] is blocking and a slow or hostile origin must
/// not hold up the first window.
pub fn install_and_refresh() {
    let dir = crate::infra::app_data::app_data_dir_early();
    if dir.is_none() {
        log::warn!(
            target: "registry",
            "no app-data directory: the plugin kill list and the user's plugin approvals cannot be read, so no plugin will be refused as withdrawn or as unapproved"
        );
    }
    install(dir.as_deref());
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
/// — a withdrawal outranks a version floor, which outranks missing consent —
/// then exists once, in [`verdict`], instead of being re-derived by every
/// screen that renders it; and the state no reader knows how to draw, two
/// reasons set at once, cannot be built. The kind is kept because the
/// remedies differ: a withdrawal is permanent and the plugin must go, a floor
/// is met by updating moss, and consent is one click on the tile.
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
    /// Neither moss nor the user put these bytes here: they came with the
    /// folder, or were edited after they were allowed. Opening someone else's
    /// folder must not run their code before a single screen is shown, so
    /// the plugin waits for the user to allow it — the remedy is a click, not
    /// an update or a removal, which is why it is its own kind.
    Unapproved,
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
            Refusal::Unapproved => {
                format!("{name} {version} came with this folder and has not been allowed to run here")
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
/// held to a floor nor asked for consent; anything else needs the registry's
/// word that it is not withdrawn, a floor this moss meets, and the user's.
pub fn verdict(manifest: &PluginManifest, plugin_dir: &Path) -> Option<Refusal> {
    verdict_when(manifest, plugin_dir, false)
}

/// [`verdict`] with consent already given (`--allow-plugins`): the
/// same two withholding grounds, and the consent step skipped.
fn verdict_when(manifest: &PluginManifest, plugin_dir: &Path, consent_given: bool) -> Option<Refusal> {
    let shipped = code_of(manifest, plugin_dir).is_some_and(|code| code.shipped);
    if let Some(refusal) = withheld(manifest, shipped) {
        return Some(refusal);
    }
    (!consent_given && !vouched_for(manifest, plugin_dir)).then_some(Refusal::Unapproved)
}

/// Does anything vouch for the code now in `plugin_dir` — did moss ship these
/// exact bytes, or has the user allowed them?
///
/// The consent half of [`verdict`] — that one returns `Unapproved` exactly
/// when nothing does, so the rule lives here and not in both.
///
/// Asked on its own for a row's IDENTITY, which is not the same question as
/// whether the plugin may run. Catalog rows merge by id, so a
/// folder that carries `.moss/plugins/github/` wears the name, description and
/// origin moss holds for `github` over whatever bytes are actually there; the
/// row is only honest about them once nothing vouches for them. Asked
/// separately from [`verdict`] because that one answers with the refusal of
/// highest precedence: a withdrawal or a version floor outranks missing
/// consent, so a refused row can still be code nobody vouched for.
pub fn vouched_for(manifest: &PluginManifest, plugin_dir: &Path) -> bool {
    let code = code_of(manifest, plugin_dir);
    code.as_ref().is_some_and(|code| code.shipped)
        || approved(plugin_dir, code.as_ref().map(|code| code.hash.as_str()))
}

/// The two grounds that refuse a plugin whatever the user has said —
/// withdrawn, or built for a newer moss. [`verdict`] asks them first; an
/// install asks them alone, about the manifest that came out of the archive,
/// since nothing can be approved before it exists.
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

/// [`refusal`] for a build with no window: the consent refusal names its two
/// remedies, since there is no tile to click.
fn refusal_headless(manifest: &PluginManifest, plugin_dir: &Path) -> Option<String> {
    verdict(manifest, plugin_dir).map(|refusal| match refusal {
        Refusal::Unapproved => format!(
            "{}; pass --allow-plugins to run what this folder carries, or open the folder in the moss app and allow it there",
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
    let stamps = ["manifest.json", manifest.entry.as_str()].map(|name| {
        let meta = std::fs::metadata(plugin_dir.join(name)).ok();
        #[cfg(unix)]
        let ctime = {
            use std::os::unix::fs::MetadataExt;
            meta.as_ref().map_or((0, 0), |m| (m.ctime(), m.ctime_nsec()))
        };
        #[cfg(not(unix))]
        let ctime = (0, 0);
        (meta.as_ref().map_or(0, |m| m.len()), meta.and_then(|m| m.modified().ok()), ctime)
    });
    let fingerprint: Fingerprint = (manifest.entry.clone(), stamps);
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

/// Whether the user has allowed exactly the code now in `plugin_dir`.
///
/// Keyed by the canonical directory, so a project reached through two paths
/// is one project; falls back to the path as given when it cannot be
/// canonicalized, which is also what [`approve`] does, so the two agree.
fn approved(plugin_dir: &Path, code_hash: Option<&str>) -> bool {
    let guard = CONSENT.read().unwrap_or_else(|e| e.into_inner());
    let Some((_, approvals)) = guard.as_ref() else { return true };
    code_hash.is_some_and(|hash| approvals.allows(&canonical(plugin_dir), hash))
}

/// Record the user's consent for the code now in `plugin_dir`, and put it in
/// force at once.
///
/// Called by the install that put the bytes there — the user asked for them,
/// which is the consent — and by the Allow affordance on a refused tile.
pub fn approve(plugin_dir: &Path) -> Result<(), String> {
    let manifest = crate::plugins::bundled::read_installed_manifest(plugin_dir)
        .ok_or_else(|| format!("{} has no readable manifest", plugin_dir.display()))?;
    let hash = crate::plugins::bundled::code_hash(plugin_dir, &manifest)
        .ok_or_else(|| format!("{} has no readable entry file", plugin_dir.display()))?;
    let mut guard = CONSENT.write().unwrap_or_else(|e| e.into_inner());
    let Some((dir, approvals)) = guard.as_mut() else {
        // Nothing enforces consent without a store, so there is nothing to
        // record; every test binary and a moss with no app data land here.
        return Ok(());
    };
    approvals.grant(canonical(plugin_dir), hash);
    match dir {
        Some(dir) => approval::save(dir, approvals),
        // Enforced without a store: the grant holds for this process only.
        None => Ok(()),
    }
}

fn canonical(plugin_dir: &Path) -> PathBuf {
    plugin_dir.canonicalize().unwrap_or_else(|_| plugin_dir.to_path_buf())
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
/// A floor moss cannot parse is not enforced either. Refusing on it would let
/// one typo in a manifest disable a plugin with no way for the user to tell
/// why, and the registry validates the field on submission — so the failure it
/// would catch cannot reach a published entry, while the plugins it would
/// break are the hand-installed ones.
fn outran_its_host(manifest: &PluginManifest) -> Option<Refusal> {
    let floor = manifest.min_moss_version.as_deref()?;
    (!host_meets_floor(Some(floor))).then(|| Refusal::Incompatible {
        // The floor is publisher-authored inside the artifact — the hash pins
        // provenance, not intent — and it lands in an `aria-label`. Same
        // treatment as a revocation reason, for the same reason. `host` is
        // moss's own and needs none.
        needs: bounded(floor, MAX_NAME),
        host: crate::system::app_version().to_string(),
    })
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

/// Whether a version this catalog offers can actually run here.
///
/// The registry publishes one entry per id — the latest — and that latest can
/// declare a floor above this moss: github's live entry needs 0.11.7 while a
/// 0.11.6 app is asking. Offering it as an update would produce an install
/// that is refused at load, so an unrunnable version is not an update.
pub fn host_meets_floor(min_moss_version: Option<&str>) -> bool {
    let Some(floor) = min_moss_version else { return true };
    match (Version::parse(floor), Version::parse(crate::system::app_version())) {
        (Ok(needs), Ok(host)) => host >= needs,
        _ => {
            // `debug!`, not `warn!`: the refusal path runs inside
            // `load_plugin`, which the QuickJS engine re-enters on every hook
            // dispatch, so one typo'd manifest would otherwise fill the log.
            log::debug!(
                target: "registry",
                "min_moss_version {floor:?} is not a version moss can compare; not enforced"
            );
            true
        }
    }
}

#[cfg(test)]
mod tests {
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
    fn the_floor_this_moss_meets_and_the_floor_it_cannot_read_both_load() {
        assert_eq!(outran_its_host(&manifest_needing(Some(crate::system::app_version()))), None, "equal is met");
        assert_eq!(outran_its_host(&manifest_needing(Some("0.0.1"))), None);
        assert_eq!(outran_its_host(&manifest_needing(None)), None, "no floor is no gate");
        assert_eq!(
            outran_its_host(&manifest_needing(Some("whenever"))),
            None,
            "a floor moss cannot compare is not enforced against the plugin"
        );
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
}
