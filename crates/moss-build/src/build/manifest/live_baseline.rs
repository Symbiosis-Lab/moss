//! What is live right now — uid, URL, source path — read from the publish
//! records, with "could not read it" kept apart from "there isn't one".
//!
//! # Why this is a read over the publish record rather than a file of its own
//!
//! Two decisions in the build turn on knowing what is LIVE, and neither can be
//! recomputed from anything else:
//!
//! - **Rename detection** ([`crate::build::feeds::redirects::detect_renames`])
//!   joins live URLs to current URLs by uid. A rename it misses emits no
//!   redirect stub, and the old URL 404s permanently rather than recovering on
//!   the next build.
//! - **Duplicate-uid resolution** ([`crate::build::render::uid_dedup`]) uses
//!   published identity as its first tiebreak. Without it, a collision falls to
//!   date/birth-time heuristics that a synced vault systematically reverses,
//!   and the loser's frontmatter is rewritten with a fresh uid — which cannot
//!   be undone, because a uid is random and joins live comment threads and
//!   signed moderation history.
//!
//! Until moss#1079 that came from a second file, `deployed-article-map.json`: a
//! byte copy of the working `ArticleMap`, carrying every article's full markdown
//! *and* its rendered HTML. On the vault that reported the incident, 4.83 MB
//! across 108 articles — 98.7% of it prose — to convey the `{uid, url,
//! source_path}` triples its two readers actually use. A second copy of the
//! user's site, parked in their Drive, and the one file a provider never
//! finished handing back.
//!
//! It was also redundant. `deploy::landed::record_landed` wrote it in the same
//! breath as [`crate::build::manifest::published_record`], for the same reason,
//! about the same publish. So the fix is not a smaller second file: the publish
//! record carries the `{uid, url, source_path}` triples this module needs,
//! computed once by the writer, as `PublishedSnapshot::triples`.
//!
//! `uids` and `source_to_output` — the two fields `triples` replaced as this
//! module's source (moss#1093) — are kept on the record for one release as the
//! fallback a legacy record (written before `triples` existed) reads through.
//! They come from different sources (the article map, the sealed manifest)
//! that can advance independently, so joining them at read time can be
//! half-updated; `triples` cannot, because it is written whole by one read.
//! See `docs/archive/2026-08-20-publish-record-triples-migration-design.md`.
//!
//! # Why unreadable is a value here, not a log line
//!
//! `.moss/deploy/` is gitignored but **synced** (ADR-062): what is live at a
//! target is a fact about the SITE, so a second machine that publishes after
//! this one renamed a page still has the baseline that lets it leave a
//! forwarding link. The price of that decision is paid here — a provider may
//! decline to hand the record back, so [`Baseline`] keeps `Absent` and
//! `Unreadable` apart all the way to the two decisions. "Nothing was ever
//! published" and "I could not read what was published" license opposite
//! actions, and the code used to collapse them into one empty map.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::build::manifest::change_set::PublishedSnapshot;
use crate::build::scan::article_map::{to_pretty_url, ArticleMap};
use crate::moss_paths::MossPaths;

// `LiveEntry` is persisted as `PublishedSnapshot::triples` (moss#1093), so it
// lives on the record type in `change_set` — re-exported here so every
// existing `live_baseline::LiveEntry` reference keeps resolving.
pub use crate::build::manifest::change_set::LiveEntry;

/// What is live, across every target this folder publishes to.
///
/// The union rather than one target's record: a redirect stub is emitted into
/// the one output tree that every target is served from, so a URL that is live
/// anywhere is a URL someone can have linked to.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LiveBaseline {
    /// Sorted by `url`, so the indexes below are deterministic regardless of
    /// which record was read first.
    pub entries: Vec<LiveEntry>,
}

/// Why a baseline that exists could not be turned into entries.
///
/// All three arms defer the same decisions; they differ in what the user is
/// told, and in whether waiting would help.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unreadable {
    /// The bytes are not here — evicted, half-synced, permissions. Waiting can
    /// end this, and the next build retries.
    Withheld,
    /// The bytes are here and do not parse. Waiting cannot end this; the next
    /// landed publish rewrites the record, which does.
    Corrupt,
    /// The record is readable and simply predates note-ID recording, and the
    /// file that used to carry them is gone. Nothing can answer for the pages
    /// published before then — but a publish HAS happened, so this is not
    /// absence, and guessing here is what rewrites a live note's identity.
    PredatesUids,
    /// A legacy (pre-`triples`) record whose `uids` and `source_to_output`
    /// disagree: some source path with a live uid has no matching entry in
    /// `source_to_output`. That is the exact shape moss#1089 could produce
    /// before it was fixed — half-updated, not merely absent. Joining it
    /// anyway would drop the uid, source path, and all, from the baseline
    /// entirely, which is the moss#1079 mint reopened. Waiting cannot end
    /// this, like `Corrupt` — the fix is the next publish, which now always
    /// writes `triples` directly and cannot re-diverge.
    Inconsistent,
}

impl Unreadable {
    /// One sentence: what is wrong with the record, and whether waiting helps.
    ///
    /// The single owner of that sentence. It was written out twice — once for
    /// the duplicate-uid pass, once for redirect stubs — and the two copies had
    /// already drifted in wording by the time they were merged here
    /// (2026-08-29). Each caller appends its own consequence.
    pub fn describe(self) -> &'static str {
        match self {
            Unreadable::Withheld => {
                "the record of what is live could not be read right now (sync or \
                 permissions); it usually resolves on its own"
            }
            Unreadable::Corrupt => {
                "the record of what is live is damaged and won't self-heal; publishing \
                 again rewrites it"
            }
            Unreadable::PredatesUids => {
                "the record of what is live predates note-ID tracking; one more publish \
                 writes it"
            }
            Unreadable::Inconsistent => {
                "the record of what is live has a note ID with no matching page mapping \
                 and won't self-heal; publishing again rewrites it whole"
            }
        }
    }
}

/// What the record of what is live says, with "could not read it" kept apart
/// from "there isn't one".
#[derive(Debug, Clone, PartialEq)]
pub enum Baseline {
    /// A baseline was read. Its `entries` may legitimately be empty (a site
    /// whose last publish had no articles).
    Present(LiveBaseline),
    /// Provably nothing published: no record, and no legacy snapshot.
    Absent,
    /// Something is there and could not be used. **Never** substitute an empty
    /// baseline for this — that is the moss#1079 bug.
    Unreadable(Unreadable),
}

impl LiveBaseline {
    /// uid → the URL it was published at. First entry wins, which is stable
    /// because `entries` is sorted; a uid live at two URLs is malformed either
    /// way.
    pub fn urls_by_uid(&self) -> HashMap<&str, &str> {
        let mut index = HashMap::new();
        for e in &self.entries {
            index.entry(e.uid.as_str()).or_insert(e.url.as_str());
        }
        index
    }

    /// uid → every source path recorded live under it.
    ///
    /// A set rather than a single path on purpose: two targets, or a
    /// hand-edited record, can claim two, and `uid_dedup::pick_uid_keeper`
    /// treats that ambiguity as *no* signal rather than picking arbitrarily.
    pub fn paths_by_uid(&self) -> HashMap<String, HashSet<String>> {
        let mut index: HashMap<String, HashSet<String>> = HashMap::new();
        for e in &self.entries {
            if e.source_path.is_empty() {
                continue;
            }
            index.entry(e.uid.clone()).or_default().insert(e.source_path.clone());
        }
        index
    }

}

/// What one record says is live, or why it cannot safely say.
enum RecordFacts {
    /// Either `triples` directly, or a legacy `uids` + `source_to_output` pair
    /// proven complete (below) — both are lossless, so there is nothing to
    /// distinguish once computed.
    Entries(Vec<LiveEntry>),
    /// A legacy pair that is NOT proven complete. See [`Unreadable::Inconsistent`].
    Inconsistent,
}

/// Is a legacy `uids` + `source_to_output` pair safe to join?
///
/// Decidable from the record alone: every source path `uids` names must have a
/// matching entry in `source_to_output`, or the join would drop that uid (and
/// its source path) out of the baseline entirely rather than just its URL —
/// the moss#1089 shape. An empty `uids` trivially passes (nothing to place),
/// which is correct: that case answers zero entries either way, and it is
/// `PredatesUids`'s job — decided in [`load`] from the aggregate result, not
/// here — to name it, not `Inconsistent`'s.
fn legacy_pair_is_complete(record: &PublishedSnapshot) -> bool {
    record.uids.keys().all(|source_path| record.source_to_output.contains_key(source_path))
}

/// The join `legacy_pair_is_complete` already proved safe. Never called
/// otherwise — an incomplete pair is `RecordFacts::Inconsistent`, not a lossy
/// call to this function.
fn join_legacy_pair(record: &PublishedSnapshot) -> Vec<LiveEntry> {
    record
        .uids
        .iter()
        .filter_map(|(source_path, uid)| {
            let output = record.source_to_output.get(source_path)?;
            Some(LiveEntry {
                uid: uid.clone(),
                url: to_pretty_url(output),
                source_path: source_path.clone(),
                // The legacy uids + source_to_output pair carries no title;
                // same as a pre-title triples record loaded straight off disk.
                title: String::new(),
            })
        })
        .collect()
}

/// What one publish record says is live — `triples` directly when the writer
/// left them, the legacy join when it is decidably safe, or `Inconsistent`
/// when it is not.
fn record_facts(record: &PublishedSnapshot) -> RecordFacts {
    match &record.triples {
        Some(triples) => RecordFacts::Entries(triples.clone()),
        None if legacy_pair_is_complete(record) => RecordFacts::Entries(join_legacy_pair(record)),
        None => RecordFacts::Inconsistent,
    }
}

/// Source path → uid, for every article the build placed.
///
/// This is the projection a publish records. It is deliberately tiny: the
/// article map it comes from carries every page's markdown and rendered HTML,
/// and none of that is anyone's business once the page is live.
pub fn uids_of(map: &ArticleMap) -> HashMap<String, String> {
    map.articles
        .values()
        .filter_map(|info| Some((info.source_path.clone(), info.uid.clone()?)))
        .filter(|(source_path, _)| !source_path.is_empty())
        .collect()
}

/// The pre-moss#1079 file, in the two places it has lived.
///
/// `.moss/deploy/` since 2026-08-17, `.moss/data/` before that. A project that
/// last published before this change has its only uid history in one of them,
/// and ignoring it would miss every rename made since — a missed rename emits
/// no stub at all, so the old URL 404s for good rather than recovering on the
/// next build. [`migrate`] imports and removes them; this is what reads them in
/// the meantime.
fn legacy_paths(paths: &MossPaths) -> [PathBuf; 2] {
    [
        paths.deployed_article_map(),
        paths.data_dir().join("deployed-article-map.json"),
    ]
}

/// Read what is live, keeping absent and unreadable apart.
///
/// Records and the pre-moss#1079 snapshot are UNIONED rather than ranked. They
/// disagree only by being written at different publishes, and a URL either of
/// them knows is a URL someone can have linked to — so preferring one would
/// drop live pages for the duration of the migration, which is the permanent
/// 404 this module exists to prevent.
pub fn load(paths: &MossPaths) -> Baseline {
    let records = match read_records(paths) {
        Ok(records) => records,
        Err(why) => return Baseline::Unreadable(why),
    };

    let mut entries: Vec<LiveEntry> = Vec::new();
    for record in &records {
        match record_facts(record) {
            RecordFacts::Entries(mut e) => entries.append(&mut e),
            // A record that contradicts itself is unusable for the same
            // reason an unreadable legacy snapshot is (below): the OTHER
            // records cannot be shown to cover the pages this one dropped, so
            // unioning them in anyway would silently narrow the baseline.
            RecordFacts::Inconsistent => {
                log::warn!(
                    "a publish record has a note ID with no matching page mapping — this is \
                     the half-updated shape moss#1089 fixed the last writer of, so this \
                     build treats what is live as unknown rather than guessing at it. The \
                     next publish rewrites the record whole."
                );
                return Baseline::Unreadable(Unreadable::Inconsistent);
            }
        }
    }

    match read_legacy(paths) {
        Baseline::Present(legacy) => entries.extend(legacy.entries),
        // A snapshot that is there and unusable is unusable for everyone: the
        // records cannot be shown to cover it, so its pages would silently go
        // missing from the baseline.
        Baseline::Unreadable(why) => return Baseline::Unreadable(why),
        Baseline::Absent if entries.is_empty() && !records.is_empty() => {
            // Records, but no note IDs anywhere. Something HAS been published,
            // so this is not absence — and the file that used to answer is
            // gone, so nothing here can.
            log::warn!(
                "the publish records carry no note IDs and the file that used to \
                 hold them is gone — this build will not detect renames or resolve \
                 duplicate note IDs, rather than deciding either against a history \
                 it does not have. The next publish records them."
            );
            return Baseline::Unreadable(Unreadable::PredatesUids);
        }
        Baseline::Absent => {}
    }

    if entries.is_empty() && records.is_empty() {
        return Baseline::Absent;
    }
    Baseline::Present(sorted(entries))
}

/// Every publish record on disk, or why none of them could be trusted.
///
/// Deliberately NOT [`crate::build::manifest::published_record::load_for`],
/// which swallows every failure into `None` — correct for the change set, where
/// absent and unreadable both mean "show a flat count", and wrong here, where
/// they mean opposite things. Every target's record is read, because the
/// redirect stubs go into the one tree they are all served from.
fn read_records(paths: &MossPaths) -> Result<Vec<PublishedSnapshot>, Unreadable> {
    let dir = paths.deploy_records_dir();
    let mut records = Vec::new();

    // The pre-2026-08-17 single-slot record, which `published_record` still
    // reads for the change set.
    let single = paths.deploy_dir().join("last-published.json");
    if let Some(record) = read_record(&single)? {
        records.push(record);
    }

    match std::fs::read_dir(&dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "json") {
                    if let Some(record) = read_record(&path)? {
                        records.push(record);
                    }
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            log::warn!(
                "the publish records at {} could not be listed ({e}) — treating what is \
                 live as unknown rather than as nothing",
                dir.display()
            );
            return Err(Unreadable::Withheld);
        }
    }
    Ok(records)
}

/// One record file. `Ok(None)` is reserved for a file the platform can *prove*
/// is gone.
fn read_record(path: &Path) -> Result<Option<PublishedSnapshot>, Unreadable> {
    match crate::build::cloud_readiness::read_input_if_present(path) {
        Ok(Some(text)) => serde_json::from_str(&text).map(Some).map_err(|e| {
            log::warn!("the publish record at {} does not parse ({e})", path.display());
            Unreadable::Corrupt
        }),
        Ok(None) => Ok(None),
        Err(e) => {
            log::warn!(
                "the publish record at {} could not be read ({e}) — this build will not \
                 detect renames or resolve duplicate note IDs, rather than deciding \
                 either against a record it does not have",
                path.display()
            );
            Err(Unreadable::Withheld)
        }
    }
}

/// The legacy full-`ArticleMap` snapshot, merged across both places it lives.
///
/// Both copies are read, not the first one found: [`migrate`] deletes every
/// copy it retires, and deleting one on the evidence of another is precisely
/// the move this module forbids. One unreadable copy therefore makes the whole
/// answer unreadable — conservative in the direction that keeps files.
fn read_legacy(paths: &MossPaths) -> Baseline {
    let mut entries: Vec<LiveEntry> = Vec::new();
    let mut found = false;
    for path in legacy_paths(paths) {
        match crate::build::cloud_readiness::read_input_if_present(&path) {
            Ok(Some(text)) => match serde_json::from_str::<ArticleMap>(&text) {
                Ok(map) => {
                    found = true;
                    entries.extend(from_article_map(&map).entries);
                }
                Err(e) => {
                    log::warn!(
                        "the pre-moss#1079 record at {} does not parse ({e}) — treating \
                         it as unreadable; the next publish replaces it",
                        path.display()
                    );
                    return Baseline::Unreadable(Unreadable::Corrupt);
                }
            },
            Ok(None) => continue,
            Err(e) => {
                log::warn!(
                    "the pre-moss#1079 record at {} could not be read ({e}) — treating what \
                     is live as unknown rather than as nothing",
                    path.display()
                );
                return Baseline::Unreadable(Unreadable::Withheld);
            }
        }
    }
    if found {
        Baseline::Present(sorted(entries))
    } else {
        Baseline::Absent
    }
}

/// A whole `ArticleMap`'s own view of what is live. Its keys ARE the pretty
/// URLs, so no join is needed.
///
/// `pub(crate)` for the rename tests, which describe a baseline the way the
/// build already has one in hand.
pub fn from_article_map(map: &ArticleMap) -> LiveBaseline {
    sorted(
        map.articles
            .iter()
            .filter_map(|(url, info)| {
                Some(LiveEntry {
                    uid: info.uid.clone()?,
                    url: url.clone(),
                    source_path: info.source_path.clone(),
                    title: info.title.clone(),
                })
            })
            .collect(),
    )
}

fn sorted(mut entries: Vec<LiveEntry>) -> LiveBaseline {
    entries.sort_by(|a, b| a.url.cmp(&b.url).then_with(|| a.uid.cmp(&b.uid)));
    LiveBaseline { entries }
}

/// Two independent one-time upgrades, both run on every build because neither
/// `.moss/data/` nor `.moss/deploy/` is watched — a file or a fixable record
/// that lands mid-session would otherwise sit there until the next folder
/// open.
///
/// - [`backfill_triples`]: freeze a legacy record's `uids` + `source_to_output`
///   pair into `triples`, but only when the pair is decidably complete — see
///   its own doc for why that is safe and when it is not attempted at all.
/// - the pre-moss#1079 snapshot retirement, below: unrelated to triples, and
///   unaffected by this change (`load` unions its entries in exactly as
///   before).
pub fn migrate(paths: &MossPaths) {
    backfill_triples(paths);
    retire_legacy_snapshot(paths);
}

/// Freeze a legacy record's `uids` + `source_to_output` pair into `triples`,
/// once, so [`load`] never has to re-run the join for this record again.
///
/// Only attempted when [`legacy_pair_is_complete`] holds: that is the one
/// condition decidable from the record alone that proves the join drops
/// nothing, so persisting its result changes nothing about what the record
/// answers — it only stops answering it by re-deriving it. A record whose pair
/// is NOT complete is left with `triples: None`; [`load`] then reads it as
/// [`Unreadable::Inconsistent`] until an actual publish writes real triples via
/// `deploy::landed::record_landed`. Freezing an incomplete join would persist
/// a drop as if it were authoritative, which is worse than re-deriving it: a
/// later, better-informed read (e.g. once the pre-moss#1079 snapshot migration
/// below has run) could still improve on it, and a frozen bad join cannot.
fn backfill_triples(paths: &MossPaths) {
    let Ok(records) = read_records(paths) else { return };
    for record in records {
        if record.triples.is_some() || record.uids.is_empty() {
            continue;
        }
        if !legacy_pair_is_complete(&record) {
            continue;
        }
        let mut updated = record.clone();
        updated.triples = Some(join_legacy_pair(&record));
        if let Err(e) = crate::build::manifest::published_record::save(paths, &updated) {
            log::warn!(
                "could not backfill live-page triples into the publish record ({e}) — the \
                 join runs again next build"
            );
        }
    }
}

/// Retire the pre-moss#1079 snapshot, on every build.
///
/// Three arms, and the asymmetry between the first and the last is the point:
///
/// - **The records already carry uids** → delete the snapshot *whatever its
///   state*. It is superseded, not authoritative, so unreadable costs nothing
///   here — and leaving it means a ~5 MB file keeps syncing, materialized, and
///   requested from the provider forever. `published_record::save` sets the
///   precedent: it unconditionally removes the `last-published.json` it
///   replaced.
/// - **The records carry none and the snapshot reads** → import the uids into
///   every record, then delete it — unless the records cannot place every page
///   the snapshot knows, in which case the import stands and the file stays.
///   The site keeps its rename baseline across the move either way, because
///   [`load`] unions the two.
/// - **The records carry none and the snapshot does not read** → leave it
///   exactly where it is. It is the only baseline this vault has, and deleting
///   an unreadable authoritative file destroys the thing this change exists to
///   protect. The sweep is already asking the provider for it; the next build
///   imports it if it landed.
fn retire_legacy_snapshot(paths: &MossPaths) {
    let present: Vec<PathBuf> =
        legacy_paths(paths).into_iter().filter(|p| p.exists()).collect();
    if present.is_empty() {
        return;
    }

    let Ok(records) = read_records(paths) else {
        // Unreadable records: this build cannot tell whether the snapshot is
        // superseded, so it does not get to delete it.
        return;
    };
    if records.iter().any(|r| !r.uids.is_empty()) {
        present.iter().for_each(|p| remove_superseded(p));
        return;
    }
    if records.is_empty() {
        // Nothing to import INTO. The snapshot is still the whole baseline and
        // `load` reads it directly; the next landed publish writes a record and
        // the first arm collects it.
        return;
    }

    let Baseline::Present(live) = read_legacy(paths) else { return };
    let uids: HashMap<String, String> = live
        .entries
        .iter()
        .filter(|e| !e.source_path.is_empty())
        .map(|e| (e.source_path.clone(), e.uid.clone()))
        .collect();
    if uids.is_empty() {
        // A snapshot with no uids in it says nothing, and keeping it costs the
        // sync it is doing right now.
        present.iter().for_each(|p| remove_superseded(p));
        return;
    }

    // Which of the snapshot's pages the records' `source_to_output` can place —
    // relevant to `uids` (kept for the one-release fallback) and to whether
    // the snapshot file is still needed, below. `triples` needs no such check:
    // the snapshot's own entries already carry `url`, complete, with no join
    // against `source_to_output` at all — importing them can never produce the
    // half-updated shape [`Unreadable::Inconsistent`] exists for.
    let placeable: HashSet<&String> =
        records.iter().flat_map(|r| r.source_to_output.keys()).collect();
    let stranded = uids.keys().filter(|src| !placeable.contains(src)).count();
    let triples: Vec<LiveEntry> =
        live.entries.iter().filter(|e| !e.source_path.is_empty()).cloned().collect();

    for mut record in records.iter().cloned() {
        record.uids.clone_from(&uids);
        record.triples = Some(triples.clone());
        if let Err(e) = crate::build::manifest::published_record::save(paths, &record) {
            log::warn!(
                "could not move the note IDs into the publish record ({e}) — leaving the \
                 pre-moss#1079 file where it is; the next build retries"
            );
            return;
        }
    }
    if stranded > 0 {
        // Imported, but not superseded: `load` unions the snapshot in, so the
        // baseline stays whole while the file stays. The next publish maps
        // every live page and the first arm collects it.
        log::info!(
            "moved the note IDs into the publish record, but {stranded} published page(s) \
             are not in any record's output map — keeping the pre-moss#1079 file until a \
             publish can place them"
        );
        return;
    }
    log::info!(
        "moved the note IDs into the publish record — the pre-moss#1079 copy of the \
         whole article map is retired"
    );
    present.iter().for_each(|p| remove_superseded(p));
}

/// The delete that is safe because something else holds the same information.
fn remove_superseded(legacy: &Path) {
    // allow:unlink a retired record under .moss/data, not staging
    match std::fs::remove_file(legacy) {
        Ok(()) => log::info!(
            "removed the superseded record at {} — the publish record has the live one",
            legacy.display()
        ),
        // Unreadable does not block the unlink: `remove_file` never opens the
        // file, so a dataless entry deletes exactly like a materialized one
        // (ADR-043's measurement). A failure here is a real permissions
        // problem, and costs only that the file keeps syncing.
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => log::warn!(
            "could not remove the superseded record at {} ({e}) — it will keep syncing",
            legacy.display()
        ),
        Err(_) => {}
    }
}

#[cfg(test)]
#[path = "live_baseline_tests.rs"]
mod tests;
