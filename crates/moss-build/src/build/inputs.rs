//! What a build is as of beyond its source files: the moment it runs and the
//! site's time zone, which together fix the build day every past/upcoming
//! decision and edition date reads. Each is read once per build and handed
//! down, so two machines that build the same source on the same day publish
//! the same bytes.
//!
//! Also the record of what else a build read, [`BuildInputs`]: the `.moss/`
//! files, the fetched link cards, the environment variables and the build's
//! own options that reach a page. A build that
//! saw no file events compares them with the last seal to prove nothing
//! outside the page tree moved.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

use crate::build::types::{ParsedDocument, SourceMetadata};

/// What a sealed build read besides its source tree.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BuildInputs {
    /// Every `.moss/` file the path registry marks materialized (the bytes
    /// moss must read and cannot regenerate), keyed like `sources`, and the
    /// link cards an older moss kept, keyed `link-meta/<file>`; stat'd before the build
    /// read any of them.
    pub moss: BTreeMap<String, SourceMetadata>,
    /// Each environment variable read through [`env`], and each of the
    /// build's own [`options`], with a hash of its value, or `-` when it was
    /// unset.
    pub env: BTreeMap<String, String>,
    /// When `moss` was measured: a file written in that same moment is hashed
    /// rather than trusted by its stat.
    #[serde(default)]
    pub taken_at: Option<u64>,
    /// The day the render was as of, in the zone it was decided in. The
    /// render verdict compares the day itself; a build that stops before
    /// rendering compares it here.
    #[serde(default)]
    pub day: Option<BuildDay>,
    /// Whether the generation this seal promoted is all a one-shot build
    /// would write: every image placeholder baked and the search index built
    /// from these pages. A build beside a preview leaves both to later passes,
    /// and a later build in that process can read deferred placeholders back,
    /// so this is taken from what the render had, never from its options.
    #[serde(default)]
    pub complete: bool,
    /// Whether the seal found the image and video outputs the render read
    /// from the last seal. Media that settle after the render (a variant
    /// encoded, a video ladder registered) leave pages a later build renders
    /// differently, so no page of that seal may be carried.
    #[serde(default)]
    pub media_settled: bool,
    /// [`config_rest`] as this build measured it. A watch batch that edits
    /// `.moss/config.toml` reuses the last build's pages only while this holds.
    #[serde(default)]
    pub config_rest: Option<String>,
    /// What the publish gate needs to know of this generation, so a build
    /// that ends at its entry can hand it over. `None` when the seal left
    /// something that can clear without the folder changing (a derivation
    /// pending, a promise in flight, an input unread): such a seal is never
    /// handed over, and the next build renders.
    #[serde(default)]
    pub evidence: Option<SealEvidence>,
    /// [`state_rest`] as this build measured it. `.moss/state.toml` is proved
    /// through this digest instead of its bytes, so a publish that rewrites
    /// only its own bookkeeping leaves the last seal standing.
    #[serde(default)]
    pub state_rest: Option<String>,
    /// The published generation each pointer under `.moss/` names: every
    /// publish record (`records/<file>`) and each target's last publish in
    /// `state.toml` (`state/<target>`). Proved by what they name rather than
    /// their bytes; see [`generations_hold`].
    #[serde(default)]
    pub generations: Option<BTreeMap<String, String>>,
    /// Whether the build reported a problem before it sealed. A build that
    /// ends at its entry reports none, so only a seal that reported none
    /// may be kept that way.
    #[serde(default)]
    pub warned: bool,
}

/// [`BuildInputs::evidence`]: the lasting refusals and report lines of a seal
/// with nothing transient left. Only a folder change can clear any of them,
/// and a folder change never stops at entry.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SealEvidence {
    pub missing_references: Vec<crate::build::types::ReferenceOccurrence>,
    pub drafts_linked: Vec<crate::build::types::DraftLinkOccurrence>,
    #[serde(default)]
    pub remote_pictures: Vec<crate::build::types::ReferenceOccurrence>,
    /// Scripts a members folder published to everyone; `None` when the build
    /// recorded no list.
    pub public_scripts: Option<Vec<String>>,
    /// `(old, served)` output paths the seal kept answering.
    pub kept: Vec<(String, String)>,
    /// What the render read and how: the publish refuses a generation without it.
    pub inputs: BTreeMap<String, crate::build::cloud_ledger::InputEntry>,
    /// The folder the seal read: a kept seal answers for it, not for wherever it is read.
    pub root: Option<std::path::PathBuf>,
    pub restricts_pages: bool,
}

impl SealEvidence {
    /// The render's half, or `None` when it left something transient; `settled`
    /// is the render's own part of that answer.
    pub(crate) fn of_render(folder: &str, settled: bool, missing: &[crate::build::types::ReferenceOccurrence], drafts: &[crate::build::types::DraftLinkOccurrence], remote_pictures: &[crate::build::types::ReferenceOccurrence]) -> Option<Self> {
        let records = crate::system::build_records::records();
        (settled && records.unverifiable_moderation(folder).is_none_or(|n| n == 0)).then(|| Self {
            missing_references: missing.to_vec(),
            drafts_linked: drafts.to_vec(),
            remote_pictures: remote_pictures.to_vec(),
            public_scripts: records.public_scripts(folder),
            ..Default::default()
        })
    }
}

/// `.moss/config.toml` keys whose effect on each page is in that page's own
/// render fingerprint (`facade::ShellFacts`), so changing one needs no full
/// render. A key earns its place with a test that flips it and compares every
/// page against a full build.
pub(crate) const FINGERPRINTED_KEYS: &[(&str, &str)] = &[("site", "floating_nav")];

/// A digest of `.moss/config.toml` without [`FINGERPRINTED_KEYS`]: equal across
/// two builds when nothing else in the file changed. `None` when the file
/// cannot be read or parsed, which proves nothing.
pub(crate) fn config_rest(folder: &Path) -> Option<String> {
    let path = crate::moss_paths::MossPaths::new(folder).config();
    let mut table: toml::Table = match std::fs::read_to_string(&path) {
        Ok(text) => text.parse().ok()?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
        Err(_) => return None,
    };
    for (section, key) in FINGERPRINTED_KEYS {
        if let Some(toml::Value::Table(t)) = table.get_mut(*section) {
            t.remove(*key);
            if t.is_empty() {
                table.remove(*section);
            }
        }
    }
    Some(value_hash(Some(&serde_json::to_string(&table).ok()?)))
}

/// Keys of `.moss/state.toml` that a publish rewrites and no page reads, by
/// table: the DNS it observed and verified, and when, and each target's address. Each
/// target's `last_deployment_at` counts only as set or unset, which is what the
/// redirect advisory reads, and its `last_deployed_generation_id` is one of
/// [`BuildInputs::generations`]. A CDN status counts only as active or not. A key of the same name in any other table
/// counts.
const OBSERVED_BOOKKEEPING: &[&str] = &["checked_at", "dns_target", "dns_configured"];
const TARGET_BOOKKEEPING: &[&str] = &["last_deployment_url"];

/// A digest of `.moss/state.toml` without the publish's bookkeeping, keyed and
/// read like [`config_rest`], and the generation each target last published.
/// Every other key counts, so a key added later reaches the proof until a test
/// shows no page reads it.
pub(crate) fn state_rest(folder: &Path) -> Option<(String, BTreeMap<String, String>)> {
    fn table<'a>(parent: &'a mut toml::Table, key: &str) -> Option<&'a mut toml::Table> {
        parent.get_mut(key).and_then(toml::Value::as_table_mut)
    }
    fn strip(state: &mut toml::Table, generations: &mut BTreeMap<String, String>) {
        let Some(deployment) = table(state, "deployment") else { return };
        if let Some(observed) = table(deployment, "observed") {
            OBSERVED_BOOKKEEPING.iter().for_each(|key| { observed.remove(*key); });
            // The build reads only whether the CDN is `active` (www or apex in every canonical).
            if let Some(status) = observed.remove("cdn_status") {
                if status.as_str() == Some("active") {
                    observed.insert("cdn_active".into(), toml::Value::Boolean(true));
                }
            }
        }
        for (name, target) in table(deployment, "targets").into_iter().flat_map(|t| t.iter_mut()) {
            let Some(target) = target.as_table_mut() else { continue };
            TARGET_BOOKKEEPING.iter().for_each(|key| { target.remove(*key); });
            if let Some(at) = target.get_mut("last_deployment_at") {
                *at = toml::Value::Boolean(true);
            }
            if let Some(toml::Value::String(generation)) = target.remove("last_deployed_generation_id") {
                generations.insert(format!("state/{name}"), generation);
            }
        }
    }
    let path = crate::moss_paths::MossPaths::new(folder).state();
    let mut table: toml::Table = match std::fs::read_to_string(&path) {
        Ok(text) => text.parse().ok()?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
        Err(_) => return None,
    };
    let mut generations = BTreeMap::new();
    strip(&mut table, &mut generations);
    Some((value_hash(Some(&serde_json::to_string(&table).ok()?)), generations))
}

/// Adds the generation each publish record names to `generations`, keyed
/// `records/<file>`; a record that names none is keyed by its bytes. `None`
/// when one is in the cloud or cannot be read, which proves nothing.
fn record_generations(folder: &Path, generations: &mut BTreeMap<String, String>) -> Option<()> {
    #[derive(serde::Deserialize)]
    struct Names {
        generation_id: String,
    }
    let dir = crate::moss_paths::MossPaths::new(folder).deploy_records_dir();
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(()),
        Err(_) => return None,
    };
    for entry in entries {
        let path = entry.ok()?.path();
        let name = path.file_name()?.to_string_lossy().into_owned();
        if !is_record_name(&name) {
            continue;
        }
        if crate::build::icloud::is_evicted(&path) {
            return None;
        }
        let bytes = std::fs::read(&path).ok()?;
        let named = serde_json::from_slice::<Names>(&bytes).map(|n| n.generation_id)
            .or_else(|_| crate::build::cache::ObjectStore::hash_file(&path).map(|hash| format!("bytes {hash}")))
            .ok()?;
        generations.insert(format!("records/{name}"), named);
    }
    Some(())
}

fn is_record_name(name: &str) -> bool {
    !name.starts_with('.') && !name.contains('/') && name.ends_with(".json")
}

/// Whether every generation pointer still names the generation the seal
/// rendered against, or names the seal's own (`sealed`, just published): the
/// build after a publish renders the generation it published, so either one
/// gives the seal's pages. Any other generation, or a pointer gone, renders.
fn generations_hold(then: &BTreeMap<String, String>, now: &BTreeMap<String, String>, sealed: Option<&str>) -> bool {
    then.keys().all(|key| now.contains_key(key))
        && now.iter().all(|(key, named)| then.get(key) == Some(named) || sealed == Some(named.as_str()))
}

/// [`BuildInputs::day`]: the date [`effective_now`] gave, and the site zone it
/// was given (`None` is UTC).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BuildDay {
    pub date: chrono::NaiveDate,
    pub zone: Option<String>,
}

/// [`BuildInputs::complete`]: this build indexes search now, every raster
/// image of its scan has its dominant colour (what a deferred image lacks
/// until the background fills it in), and whatever it reused from the last
/// build (a carried page, parsed HTML) was complete too. `reused` is the last
/// seal's record when this build reused anything.
pub(crate) fn complete(images: &[crate::types::content::MediaMetadata], indexes_now: bool, reused: Option<Option<&BuildInputs>>) -> bool {
    let baked = images.iter().all(|m| !crate::build::scan::scan::is_raster_image(&m.file_type) || m.dominant_color.is_some());
    indexes_now && baked && reused.is_none_or(|last| last.is_some_and(|r| r.complete))
}

impl BuildDay {
    /// Whether a build starting now would be as of the same day.
    pub(crate) fn is_today(&self) -> bool {
        effective_now(build_instant(), self.zone.as_deref()).date() == self.date
    }
}

/// This build's `.moss/` measurement, and whether it and the environment
/// still hold what the last sealed record saw.
#[derive(Debug, Clone, Default)]
pub struct Measured {
    /// `None` when a file is in the cloud or cannot be read: such a build
    /// records nothing, and the next one proves nothing.
    pub record: Option<BuildInputs>,
    pub holds: bool,
}

/// Measure the `.moss/` inputs once, before anything in the build reads them,
/// and compare the environment and this build's `options` with `previous`,
/// the record of the seal whose generation is `sealed`.
/// `previous` lends its hash to a file its stat still vouches for, so an
/// unchanged file is not reread.
pub(crate) fn measure(folder: &Path, previous: Option<&BuildInputs>, sealed: Option<&str>, options: BTreeMap<String, String>) -> Measured {
    let taken_at = crate::build::stat::recording_clock();
    let Some((moss, unchanged)) = moss_files(folder, previous) else { return Measured::default() };
    let now = |name: &str| if is_option(name) {
        options.get(name).cloned().unwrap_or_else(|| value_hash(None))
    } else {
        value_hash(std::env::var(name).ok().as_deref())
    };
    let env_holds = previous.is_some_and(|p| p.env.iter().all(|(name, seen)| now(name) == *seen));
    let (state_rest, mut generations) = state_rest(folder).unzip();
    if let Some(generations) = generations.as_mut() {
        if record_generations(folder, generations).is_none() {
            return Measured::default();
        }
    }
    let state_holds = state_rest.is_some() && previous.is_some_and(|p| p.state_rest == state_rest);
    let generations_hold = previous.and_then(|p| p.generations.as_ref()).zip(generations.as_ref()).is_some_and(|(then, now)| generations_hold(then, now, sealed));
    Measured { holds: unchanged && env_holds && state_holds && generations_hold, record: Some(BuildInputs { moss, env: options, taken_at, config_rest: config_rest(folder), state_rest, generations, ..Default::default() }) }
}

/// The build's own options that reach a page, hashed like a variable: the
/// site address given on the command line, and whether plugins run. They are
/// kept in [`BuildInputs::env`] under names with a `=` after the first
/// character, which no environment variable's name can hold, since the first
/// such `=` ends the name on every platform.
///
/// Plugins are recorded as off, or on with which of the folder's plugins may
/// run (`enforce::folder_admission`): trusting the folder changes pages with
/// no file in it moving. The blocking, non-blocking and slots-only modes
/// differ only in when hooks run, and what a hook writes is on disk, where
/// the `.moss/` measurement compares it.
pub(crate) fn options(site_url_override: Option<&str>, plugins_skipped: bool, folder: &Path) -> BTreeMap<String, String> {
    let plugins = match plugins_skipped {
        true => "off".to_string(),
        false => format!("on/{}", crate::plugins::install::registry_client::enforce::folder_admission(folder)),
    };
    BTreeMap::from([
        ("moss=site_url".to_string(), value_hash(site_url_override)),
        ("moss=plugins".to_string(), value_hash(Some(&plugins))),
    ])
}

fn is_option(name: &str) -> bool {
    name.contains('=')
}

/// The files of [`BuildInputs::moss`], and whether every one matches
/// `previous` with none added or gone.
fn moss_files(folder: &Path, previous: Option<&BuildInputs>) -> Option<(BTreeMap<String, SourceMetadata>, bool)> {
    use crate::build::watch::{source_metadata_verdict, SourceVerdict};
    let moss = folder.join(".moss");
    // Cards an older moss kept under `build.nosync` sit outside the
    // materialized rows, and a build still moves one into the site folder
    // when it reaches its link, so deleting them, or one arriving, moves the
    // record too. Cards in the site folder are a materialized row.
    let link_meta = crate::moss_paths::MossPaths::new(folder).cache_link_meta();
    let materialized = walkdir::WalkDir::new(&moss).sort_by_file_name().into_iter().filter_entry(|e| {
        let rel = e.path().strip_prefix(folder).map(|r| moss_core::slug::normalize_separators(&r.to_string_lossy())).unwrap_or_default();
        !e.file_type().is_dir() || crate::ops::watch::sweep::walk::should_descend(&rel, &e.file_name().to_string_lossy())
    });
    let cards = walkdir::WalkDir::new(&link_meta).max_depth(1).sort_by_file_name().into_iter();
    let mut files = BTreeMap::new();
    let mut unchanged = true;
    for entry in materialized.chain(cards) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) if e.depth() == 0 && e.io_error().is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound) => continue,
            Err(_) => return None,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(rel) = record_key(folder, &link_meta, entry.path()) else { continue };
        if crate::build::icloud::icloud_stub_target(entry.path()).is_some() || crate::build::icloud::is_evicted(entry.path()) {
            return None;
        }
        let md = entry.metadata().ok()?;
        let known = previous.and_then(|p| p.moss.get(&rel).map(|meta| (meta, p.taken_at)));
        let record = match known.map(|(meta, at)| (meta, source_metadata_verdict(meta, &md, entry.path(), at))) {
            Some((meta, SourceVerdict::Unchanged { refreshed })) => refreshed.unwrap_or_else(|| meta.clone()),
            Some((_, SourceVerdict::Unknown)) => return None,
            Some((_, SourceVerdict::Changed)) | None => {
                unchanged = false;
                let stat = crate::build::stat::FileStat::of(&md);
                SourceMetadata::from_stat(crate::build::cache::ObjectStore::hash_file(entry.path()).ok()?, stat)
            }
        };
        files.insert(rel, record);
    }
    unchanged &= previous.is_some_and(|p| p.moss.len() == files.len());
    Some((files, unchanged))
}

/// The key [`BuildInputs::moss`] gives a file, when it records that file.
fn record_key(folder: &Path, link_meta: &Path, path: &Path) -> Option<String> {
    // Cards are keyed under their own directory, which `MossPaths` may
    // resolve to another spelling of the folder (a symlinked temp root).
    // Only a card (`<sha256 of the URL>.json`) is recorded, never what a build
    // writes beside them: an old link list, derived covers.
    if let Ok(card) = path.strip_prefix(link_meta) {
        return is_card_name(&card.to_string_lossy()).then(|| format!("link-meta/{}", card.to_string_lossy()));
    }
    let rel = moss_core::slug::normalize_separators(&path.strip_prefix(folder).ok()?.to_string_lossy());
    // Proved through `state_rest` and `generations` instead.
    let proved_otherwise = rel == ".moss/state.toml" || rel.strip_prefix(".moss/deploy/records/").is_some_and(is_record_name);
    (crate::moss_paths::is_materialized_rel(&rel) && !proved_otherwise).then_some(rel)
}

/// Re-records the `.moss/` files this build wrote or removed after it was
/// measured (a migration, the redirect history): its pages rendered from the
/// files as it left them, so the next build may prove from them. A file that
/// no longer holds what the build left keeps its old record, which the next
/// build sees as moved.
pub(crate) fn restamp(inputs: &mut BuildInputs, folder: &Path, left: &[(std::path::PathBuf, Option<String>)]) {
    let link_meta = crate::moss_paths::MossPaths::new(folder).cache_link_meta();
    for (path, hash) in left {
        let Some(rel) = record_key(folder, &link_meta, path) else { continue };
        match hash {
            Some(hash) => {
                let Ok(md) = std::fs::metadata(path) else { continue };
                if crate::build::cache::ObjectStore::hash_file(path).ok().as_ref() == Some(hash) {
                    inputs.moss.insert(rel, SourceMetadata::from_stat(hash.clone(), crate::build::stat::FileStat::of(&md)));
                }
            }
            None if !path.exists() => {
                inputs.moss.remove(&rel);
            }
            None => {}
        }
    }
}

fn is_card_name(name: &str) -> bool {
    name.strip_suffix(".json").is_some_and(|stem| stem.len() == 64 && stem.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The environment variables builds have read, and what each held. It is one
/// per process, so with two folders open one folder's reads enter the other's
/// seal too, which only costs that folder a full render.
static ENV_READ: Mutex<BTreeMap<&'static str, Option<String>>> = Mutex::new(BTreeMap::new());

/// Read an environment variable that can change what a build writes. Every
/// such read goes through here, so the sealed manifest records it and a later
/// build sees it change.
pub(crate) fn env(name: &'static str) -> Option<String> {
    let value = std::env::var(name).ok();
    ENV_READ.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(name, value.clone());
    value
}

/// Every variable [`env`] has returned in this process, hashed.
pub(crate) fn env_read() -> BTreeMap<String, String> {
    ENV_READ.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .map(|(name, value)| ((*name).to_string(), value_hash(value.as_deref())))
        .collect()
}

fn value_hash(value: Option<&str>) -> String {
    use sha2::{Digest, Sha256};
    value.map_or_else(|| "-".to_string(), |v| format!("{:x}", Sha256::digest(v.as_bytes())))
}

/// The moment a build is as of: `SOURCE_DATE_EPOCH` (seconds since 1970, the
/// reproducible-builds convention) when set, otherwise the clock.
///
/// Read raw, not through [`env`]: what reaches a page is the build day, which
/// the render verdict compares itself, so a new epoch on the same day proves
/// nothing moved.
pub(crate) fn build_instant() -> jiff::Timestamp {
    instant_from(std::env::var("SOURCE_DATE_EPOCH").ok().as_deref())
}

fn instant_from(source_date_epoch: Option<&str>) -> jiff::Timestamp {
    source_date_epoch
        .and_then(|raw| raw.trim().parse::<i64>().ok())
        .and_then(|secs| jiff::Timestamp::from_second(secs).ok())
        .unwrap_or_else(jiff::Timestamp::now)
}

/// The site's time zone: the home page's `cascade: {timezone: …}`, when it
/// names a zone.
pub(crate) fn site_zone(docs: &[ParsedDocument]) -> Option<&str> {
    let home = docs.iter().find(|d| d.url_path == "index.html")?;
    let zone = home.cascade.as_ref()?.get("timezone")?.as_str()?.trim();
    crate::build::feeds::calendar::zone::is_known(zone).then_some(zone)
}

/// The build's "now" for every past/upcoming decision: the start of the day
/// `instant` falls on in `zone` (UTC when the site names none). A day, not the
/// minute, and never the build machine's own zone, so every machine that builds
/// the same source on the same day decides the same. An event hero alone swaps
/// itself out in the browser at the minute its event ends (`hero_from`).
pub(crate) fn effective_now(instant: jiff::Timestamp, zone: Option<&str>) -> chrono::NaiveDateTime {
    let tz = zone.and_then(crate::build::feeds::calendar::zone::load).unwrap_or(jiff::tz::TimeZone::UTC);
    let day = instant.to_zoned(tz).date();
    chrono::NaiveDate::from_ymd_opt(i32::from(day.year()), day.month() as u32, day.day() as u32)
        .unwrap_or_default()
        .and_time(chrono::NaiveTime::MIN)
}

/// [`BuildInputs::media_settled`] for `sealed` against the seal its render read.
/// Sharing cards are left out (see [`is_sharing_card`]): counted, every first
/// build would look unsettled and its next edit would render the whole site.
///
/// [`is_sharing_card`]: crate::build::served_path::is_sharing_card
pub(crate) fn media_settled(previous: Option<&crate::types::content::SiteHashes>, sealed: &crate::types::content::SiteHashes) -> bool {
    fn outputs(hashes: &crate::types::content::SiteHashes) -> std::collections::BTreeMap<&str, Option<&str>> {
        hashes.image_outputs.iter().chain(&hashes.video_outputs)
            .filter(|path| !crate::build::served_path::is_sharing_card(path))
            .map(|path| (path.as_str(), hashes.files.get(path).map(String::as_str)))
            .collect()
    }
    previous.is_some_and(|previous| outputs(previous) == outputs(sealed))
}

/// [`effective_now`] for a site with no zone, as of [`build_instant`].
#[cfg(test)]
pub(crate) fn now() -> chrono::NaiveDateTime {
    effective_now(build_instant(), None)
}

#[cfg(test)]
#[path = "media_settled_tests.rs"]
mod media_settled_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_publish_time_counts_only_as_set() {
        let folder = tempfile::tempdir().unwrap();
        let rest = |text: &str| {
            std::fs::create_dir_all(folder.path().join(".moss")).unwrap();
            std::fs::write(folder.path().join(".moss/state.toml"), text).unwrap();
            state_rest(folder.path()).unwrap().0
        };
        let target = "[deployment.targets.\"moss:site-1\"]\n";
        let first = rest(&format!("{target}last_deployment_at = \"2026-11-01T00:00:00Z\"\n"));
        assert_eq!(first, rest(&format!("{target}last_deployment_at = \"2026-11-02T00:00:00Z\"\n")));
        assert_ne!(first, rest(target), "never published");
    }

    #[test]
    fn a_bookkeeping_name_outside_its_table_still_counts() {
        let folder = tempfile::tempdir().unwrap();
        let rest = |text: &str| {
            std::fs::create_dir_all(folder.path().join(".moss")).unwrap();
            std::fs::write(folder.path().join(".moss/state.toml"), text).unwrap();
            state_rest(folder.path()).unwrap().0
        };
        let observed = "[deployment.observed]\nchecked_at = \"2026-11-01T00:00:00Z\"\n";
        assert_eq!(rest(observed), rest("[deployment.observed]\nchecked_at = \"2026-11-02T00:00:00Z\"\n"));
        for (key, table) in [("checked_at", "accounts.mastodon"), ("dns_target", "deployment"), ("last_deployment_url", "deployment"), ("last_deployment_at", "accounts.mastodon")] {
            let at = |value: &str| rest(&format!("[{table}]\n{key} = \"{value}\"\n"));
            assert_ne!(at("one"), at("two"), "{table}.{key}");
        }
    }

    #[test]
    fn a_publish_pointer_holds_while_it_names_the_rendered_or_the_sealed_generation() {
        let folder = tempfile::tempdir().unwrap();
        let records = folder.path().join(".moss/deploy/records");
        std::fs::create_dir_all(&records).unwrap();
        let point = |generation: &str| {
            std::fs::write(records.join("moss-1.json"), format!("{{\"generation_id\": \"{generation}\", \"files\": {{}}}}")).unwrap();
            std::fs::write(folder.path().join(".moss/state.toml"), format!("[deployment.targets.\"moss:site-1\"]\nlast_deployed_generation_id = \"{generation}\"\n")).unwrap();
        };
        point("g1");
        let seal = measure(folder.path(), None, None, BTreeMap::new()).record.unwrap();
        let holds = || measure(folder.path(), Some(&seal), Some("g2"), BTreeMap::new()).holds;
        assert!(holds(), "nothing published since");
        point("g2");
        assert!(holds(), "the seal itself was published");
        point("g3");
        assert!(!holds(), "another generation was published");
        point("g2");
        std::fs::remove_file(records.join("moss-1.json")).unwrap();
        assert!(!holds(), "a record went");
    }

    #[test]
    fn the_build_day_is_the_sites_not_the_machines() {
        // 2026-03-14 02:00 UTC: still the 13th in New York, already the 14th in Tokyo.
        let instant = jiff::Timestamp::from_second(1_773_453_600).unwrap();
        let day = |y, m, d| chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap().and_time(chrono::NaiveTime::MIN);
        assert_eq!(effective_now(instant, None), day(2026, 3, 14));
        assert_eq!(effective_now(instant, Some("America/New_York")), day(2026, 3, 13));
        assert_eq!(effective_now(instant, Some("Asia/Tokyo")), day(2026, 3, 14));
        assert_eq!(effective_now(instant, Some("Not/A_Zone")), day(2026, 3, 14));
    }

    #[test]
    fn the_build_is_as_of_source_date_epoch_when_one_is_given() {
        assert_eq!(instant_from(Some("1700000000")).as_second(), 1_700_000_000);
        for unset in [None, Some("not a number")] {
            let before = jiff::Timestamp::now();
            let read = instant_from(unset);
            assert!(before <= read && read <= jiff::Timestamp::now(), "{unset:?} reads the clock");
        }
    }

    #[test]
    fn the_site_zone_is_the_home_pages_cascade() {
        let page = |url: &str, cascade: &str| ParsedDocument {
            url_path: url.into(),
            cascade: Some(serde_yaml::from_str(cascade).unwrap()),
            ..Default::default()
        };
        assert_eq!(site_zone(&[page("events/index.html", "timezone: Asia/Tokyo")]), None);
        assert_eq!(site_zone(&[page("index.html", "timezone: ' Asia/Tokyo '")]), Some("Asia/Tokyo"));
        assert_eq!(site_zone(&[page("index.html", "timezone: Nowhere")]), None);
    }
}
