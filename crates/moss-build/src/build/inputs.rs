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
/// and compare the environment and this build's `options` with `previous`.
/// `previous` lends its hash to a file its stat still vouches for, so an
/// unchanged file is not reread.
pub(crate) fn measure(folder: &Path, previous: Option<&BuildInputs>, options: BTreeMap<String, String>) -> Measured {
    let taken_at = crate::build::stat::recording_clock();
    let Some((moss, unchanged)) = moss_files(folder, previous) else { return Measured::default() };
    let now = |name: &str| if is_option(name) {
        options.get(name).cloned().unwrap_or_else(|| value_hash(None))
    } else {
        value_hash(std::env::var(name).ok().as_deref())
    };
    let env_holds = previous.is_some_and(|p| p.env.iter().all(|(name, seen)| now(name) == *seen));
    Measured { holds: unchanged && env_holds, record: Some(BuildInputs { moss, env: options, taken_at }) }
}

/// The build's own options that reach a page, hashed like a variable: the
/// site address given on the command line, and whether plugins run. They are
/// kept in [`BuildInputs::env`] under names with a `=` after the first
/// character, which no environment variable's name can hold, since the first
/// such `=` ends the name on every platform.
///
/// Plugins are recorded as on or off only: the blocking, non-blocking and
/// slots-only modes differ in when hooks run, and what a hook writes is on
/// disk, where the `.moss/` measurement compares it.
pub(crate) fn options(site_url_override: Option<&str>, plugins_skipped: bool) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("moss=site_url".to_string(), value_hash(site_url_override)),
        ("moss=plugins".to_string(), value_hash(Some(if plugins_skipped { "off" } else { "on" }))),
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
        // Cards are keyed under their own directory, which `MossPaths` may
        // resolve to another spelling of the folder (a symlinked temp root).
        let (rel, measured) = match entry.path().strip_prefix(&link_meta) {
            // A card (`<sha256 of the URL>.json`), never what a build writes
            // beside them: the prewarm list, derived covers.
            Ok(card) => (format!("link-meta/{}", card.to_string_lossy()), is_card_name(&entry.file_name().to_string_lossy())),
            Err(_) => {
                let rel = moss_core::slug::normalize_separators(&entry.path().strip_prefix(folder).ok()?.to_string_lossy());
                let measured = crate::moss_paths::is_materialized_rel(&rel);
                (rel, measured)
            }
        };
        if !measured {
            continue;
        }
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

/// [`effective_now`] for a site with no zone, as of [`build_instant`].
#[cfg(test)]
pub(crate) fn now() -> chrono::NaiveDateTime {
    effective_now(build_instant(), None)
}

#[cfg(test)]
mod tests {
    use super::*;

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
