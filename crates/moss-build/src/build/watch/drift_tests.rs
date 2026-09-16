//! The sweep's comparison rules, pinned. Each test names the failure mode it
//! forbids — most of them are rebuild loops, which are worse than the
//! staleness the sweep exists to cure.

use std::path::{Path, PathBuf};

use super::*;
use crate::build::types::SourceMetadata;
use crate::types::content::SiteHashes;

fn write(dir: &Path, rel: &str, bytes: &[u8]) -> PathBuf {
    let p = dir.join(rel);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&p, bytes).unwrap();
    p
}

fn meta_for(p: &Path, bytes: &[u8]) -> SourceMetadata {
    use sha2::{Digest, Sha256};
    let md = std::fs::metadata(p).unwrap();
    let mtime = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
    let (ctime, inode) = crate::build::types::stat_identity(&md);
    SourceMetadata {
        hash: format!("{:x}", Sha256::digest(bytes)),
        size: md.len(),
        mtime: mtime.map(|d| d.as_secs()).unwrap_or(0),
        mtime_nanos: mtime.map(|d| d.subsec_nanos()),
        ctime,
        inode,
    }
}

fn walked(rel: &str, abs: PathBuf) -> WalkedFile {
    let meta = std::fs::metadata(&abs).ok();
    WalkedFile { rel: rel.into(), abs, offline: false, meta }
}

fn no_probe(_: &Path) -> MissingProbe {
    panic!("this test expected no deletion probe")
}

/// A quiescent site — walk and manifest agree byte-for-byte — must produce a
/// quiet report. This is the steady state the sweep spends its life in; a
/// false positive here is a rebuild every interval, forever.
#[test]
fn a_matching_tree_is_quiet() {
    let dir = tempfile::tempdir().unwrap();
    let p = write(dir.path(), "index.md", b"# home");
    let mut baseline = SiteHashes::new();
    baseline.sources.insert("index.md".into(), meta_for(&p, b"# home"));

    let report = detect_drift(
        &baseline,
        &[walked("index.md", p)],
        &[],
        dir.path(),
        no_probe,
        None,
        true,
    );
    assert!(report.is_quiet(), "{report:?}");
}

/// An edit the watcher never delivered — the incident's core case.
#[test]
fn changed_bytes_are_drift() {
    let dir = tempfile::tempdir().unwrap();
    let p = write(dir.path(), "index.md", b"# home");
    let mut baseline = SiteHashes::new();
    baseline
        .sources
        .insert("index.md".into(), meta_for(&p, b"# home"));
    std::fs::write(&p, b"# HOME, edited").unwrap();

    let report = detect_drift(
        &baseline,
        &[walked("index.md", p)],
        &[],
        dir.path(),
        no_probe,
        None,
        true,
    );
    assert_eq!(report.drifted.keys().cloned().collect::<Vec<_>>(), vec!["index.md".to_string()]);
    // A plain content edit, so the sweep may dispatch it as `ContentOnly` and
    // reach the same incremental path an event-driven rebuild takes.
    assert!(!report.any_structural(report.drifted.keys()), "an edit is not a structural change");
}

/// A file the manifest has never seen is drift by existence.
#[test]
fn a_new_file_is_drift() {
    let dir = tempfile::tempdir().unwrap();
    let p = write(dir.path(), "new.md", b"fresh");
    let report = detect_drift(
        &SiteHashes::new(),
        &[walked("new.md", p)],
        &[],
        dir.path(),
        no_probe,
        None,
        true,
    );
    assert_eq!(report.drifted.keys().cloned().collect::<Vec<_>>(), vec!["new.md".to_string()]);
    // A CREATE can move the site's structure, so the incremental gates must
    // not be allowed to open on it.
    assert!(report.any_structural(report.drifted.keys()), "a new file is a structural change");
}

/// A manifest entry that is positively gone is drift; one whose path still
/// holds something (walk-scope gap, race) is not.
#[test]
fn a_deletion_is_drift_only_when_proven() {
    let dir = tempfile::tempdir().unwrap();
    let mut baseline = SiteHashes::new();
    baseline
        .sources
        .insert("gone.md".into(), SourceMetadata::default());
    baseline
        .sources
        .insert("still-there.md".into(), SourceMetadata::default());

    let report = detect_drift(
        &baseline,
        &[],
        &[],
        dir.path(),
        |abs| {
            if abs.ends_with("gone.md") {
                MissingProbe::Gone
            } else {
                MissingProbe::Present
            }
        },
        None,
        true,
    );
    assert_eq!(report.drifted.keys().cloned().collect::<Vec<_>>(), vec!["gone.md".to_string()]);
    assert!(report.any_structural(report.drifted.keys()), "a deletion is a structural change");

    // And the subset form, which is what the sweep actually asks. A batch
    // that excludes the deleted path is an ordinary content batch — the
    // whole-report flag this replaced could not express that, so a filtered
    // dispatch inherited `Structural` from a path it no longer carried and
    // closed both incremental gates over an ordinary edit.
    assert!(
        !report.any_structural(std::iter::empty()),
        "triggerability is a property of the batch being sent, not of the report"
    );
}

/// Evicted is offline, not changed — a dataless FILE must never read as a
/// mismatch. The iCloud rebuild loop this forbids: provider evicts → sweep
/// calls it drift → rebuild reads it → provider re-evicts → forever.
#[test]
fn an_offline_file_is_never_drift() {
    let dir = tempfile::tempdir().unwrap();
    let mut baseline = SiteHashes::new();
    baseline
        .sources
        .insert("away.md".into(), SourceMetadata::default());

    let report = detect_drift(
        &baseline,
        &[WalkedFile { rel: "away.md".into(), abs: dir.path().join("away.md"), offline: true, meta: None }],
        &[],
        dir.path(),
        no_probe,
        None,
        true,
    );
    assert!(report.is_quiet(), "{report:?}");
    assert_eq!(report.offline, 1);
}

/// Evicted is offline, not changed — for DIRECTORIES too. An unenumerable
/// dataless directory must read as "offline subtree", never as "everything
/// under it was deleted": a deletion verdict here removes pages.
#[test]
fn an_offline_directory_shields_its_subtree_from_the_deletion_check() {
    let dir = tempfile::tempdir().unwrap();
    let mut baseline = SiteHashes::new();
    baseline
        .sources
        .insert("posts/one.md".into(), SourceMetadata::default());
    baseline
        .sources
        .insert("posts/two.md".into(), SourceMetadata::default());
    // A sibling that merely shares the prefix STRING is not shielded.
    baseline
        .sources
        .insert("posts-archive/old.md".into(), SourceMetadata::default());

    let report = detect_drift(
        &baseline,
        &[],
        &["posts".to_string()],
        dir.path(),
        |abs| {
            assert!(
                abs.ends_with("posts-archive/old.md"),
                "only the unshielded entry may be probed, got {abs:?}"
            );
            MissingProbe::Gone
        },
        None,
        true,
    );
    assert_eq!(report.drifted.keys().cloned().collect::<Vec<_>>(), vec!["posts-archive/old.md".to_string()]);
    assert_eq!(report.offline, 2);
}

/// A manifest entry whose probe says "offline" (dataless stub, fail-fast
/// read error) is not a deletion. Waiting may help; deleting pages will not.
#[test]
fn an_offline_probe_is_not_a_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let mut baseline = SiteHashes::new();
    baseline
        .sources
        .insert("cloudy.md".into(), SourceMetadata::default());

    let report = detect_drift(&baseline, &[], &[], dir.path(), |_| MissingProbe::Offline, None, true);
    assert!(report.is_quiet(), "{report:?}");
}

/// A per-file judgment error (Unknown) fails QUIET, not open: the sweep
/// re-runs every interval, so a transient error self-heals, while failing
/// open on a persistent one (permissions) would rebuild in a loop forever.
#[test]
fn an_unjudgeable_file_is_skipped_not_rebuilt() {
    let dir = tempfile::tempdir().unwrap();
    let mut baseline = SiteHashes::new();
    // The manifest promises a file the disk cannot stat (never created):
    // source_metadata_matches_at returns Unknown.
    baseline
        .sources
        .insert("odd.md".into(), SourceMetadata::default());

    let report = detect_drift(
        &baseline,
        &[walked("odd.md", dir.path().join("odd.md"))],
        &[],
        dir.path(),
        no_probe,
        None,
        true,
    );
    assert!(report.is_quiet(), "{report:?}");
}

/// The racily-clean clock rides the baseline into the compare: an entry
/// whose mtime coincides with the manifest's capture must be hashed, so
/// same-second same-size rewrites (exFAT, SMB) cannot hide.
#[test]
fn the_baselines_capture_clock_arms_the_racy_guard() {
    let dir = tempfile::tempdir().unwrap();
    let p = write(dir.path(), "index.md", b"aaaa");
    let mut meta = meta_for(&p, b"aaaa");
    // Same size, same recorded mtime — but different bytes, as a racy
    // same-second rewrite would leave them. Blind the nanosecond tier and
    // the identity tier so only the racy guard can catch it.
    std::fs::write(&p, b"bbbb").unwrap();
    let md = std::fs::metadata(&p).unwrap();
    let mtime = md.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap();
    meta.size = md.len();
    meta.mtime = mtime.as_secs();
    meta.mtime_nanos = Some(mtime.subsec_nanos());
    let (ctime, inode) = crate::build::types::stat_identity(&md);
    meta.ctime = ctime;
    meta.inode = inode;

    let mut baseline = SiteHashes::new();
    baseline.sources.insert("index.md".into(), meta);

    // No clock: the forged fast path wins and the rewrite hides.
    baseline.captured_at = None;
    let quiet = detect_drift(
        &baseline,
        &[walked("index.md", p.clone())],
        &[],
        dir.path(),
        no_probe,
        None,
        true,
    );
    assert!(quiet.is_quiet(), "premise: without the clock this rewrite is invisible");

    // With the clock at the write instant, the entry is suspect → hash tier → drift.
    baseline.captured_at = Some(mtime.as_secs());
    let caught = detect_drift(
        &baseline,
        &[walked("index.md", p)],
        &[],
        dir.path(),
        no_probe,
        None,
        true,
    );
    assert_eq!(caught.drifted.keys().cloned().collect::<Vec<_>>(), vec!["index.md".to_string()]);
}

/// The production probe classifies a plainly-missing file as Gone and a
/// present one as Present (the cloud arms are platform behavior, exercised
/// by the icloud module's own tests).
#[test]
fn probe_missing_distinguishes_present_from_gone() {
    let dir = tempfile::tempdir().unwrap();
    let p = write(dir.path(), "here.md", b"x");
    assert_eq!(probe_missing(&p), MissingProbe::Present);
    assert_eq!(probe_missing(&dir.path().join("nope.md")), MissingProbe::Gone);
}

/// The compare does real I/O (hash tier, deletion probes), so it honors the
/// same pass deadline as the walk: a spent deadline yields a blown report
/// with no verdicts, never an unbounded pass on the runtime's time.
#[test]
fn a_spent_deadline_blows_the_compare_not_the_caller() {
    let dir = tempfile::tempdir().unwrap();
    let p = write(dir.path(), "index.md", b"# home");
    let mut baseline = SiteHashes::new();
    baseline.sources.insert("index.md".into(), meta_for(&p, b"# home"));
    baseline.sources.insert("gone.md".into(), SourceMetadata::default());

    let report = detect_drift(
        &baseline,
        &[walked("index.md", p)],
        &[],
        dir.path(),
        |_| panic!("a blown pass must not reach the deletion probe"),
        Some(std::time::Instant::now() - std::time::Duration::from_secs(1)),
        true,
    );
    assert!(report.deadline_blown);
    assert!(report.drifted.is_empty(), "no verdicts from a pass that could not finish");
}

/// The absorb-once loop, end to end at the drift level: a re-materialized
/// file (same bytes, new stat identity) is hash-confirmed ONCE and hands
/// back the refreshed record; with the record written into the baseline the
/// next pass is quiet with nothing further to refresh.
#[test]
fn a_rematerialized_file_hashes_once_then_fast_paths() {
    let dir = tempfile::tempdir().unwrap();
    let p = write(dir.path(), "index.md", b"# home");
    let mut meta = meta_for(&p, b"# home");
    // A previous life's identity: same bytes and size, different inode/ctime,
    // and an mtime the nanosecond tier cannot match.
    meta.mtime = 1_000;
    meta.mtime_nanos = Some(0);
    meta.ctime = meta.ctime.map(|c| c - 999);
    meta.inode = meta.inode.map(|i| i + 1);
    let mut baseline = SiteHashes::new();
    baseline.sources.insert("index.md".into(), meta);

    let first = detect_drift(
        &baseline,
        &[walked("index.md", p.clone())],
        &[],
        dir.path(),
        no_probe,
        None,
        true,
    );
    assert!(first.is_quiet(), "same bytes are not drift");
    assert_eq!(first.refreshed.len(), 1, "the hash tier ran and owes a write-back");

    for (rel, fresh) in first.refreshed {
        baseline.sources.insert(rel, fresh);
    }
    let second = detect_drift(
        &baseline,
        &[walked("index.md", p)],
        &[],
        dir.path(),
        no_probe,
        None,
        true,
    );
    assert!(second.is_quiet());
    assert!(second.refreshed.is_empty(), "absorbed: the fast path is back");
}

/// A partial (resumed or blown) walk must not run the deletion loop: its
/// absence set says nothing about deletions, and probing from it would
/// re-probe the whole manifest on every shard of a multi-pass sweep.
#[test]
fn a_partial_pass_never_probes_deletions() {
    let dir = tempfile::tempdir().unwrap();
    let mut baseline = SiteHashes::new();
    baseline.sources.insert("gone.md".into(), SourceMetadata::default());

    let report = detect_drift(
        &baseline,
        &[],
        &[],
        dir.path(),
        |_| panic!("a partial pass must not reach the deletion probe"),
        None,
        false,
    );
    assert!(report.is_quiet(), "{report:?}");
}

/// The compare reports how far it judged, so a caller whose budget keeps
/// blowing at the same point can resume there next pass instead of
/// re-judging the same head of the tree forever (the incident's file was 8
/// directories deep).
#[test]
fn the_compare_reports_how_far_it_judged() {
    let dir = tempfile::tempdir().unwrap();
    let a = write(dir.path(), "a.md", b"a");
    let b = write(dir.path(), "b.md", b"b");
    let mut baseline = SiteHashes::new();
    baseline.sources.insert("a.md".into(), meta_for(&a, b"a"));
    baseline.sources.insert("b.md".into(), meta_for(&b, b"b"));

    let report = detect_drift(
        &baseline,
        &[walked("a.md", a), walked("b.md", b)],
        &[],
        dir.path(),
        no_probe,
        Some(std::time::Instant::now() + std::time::Duration::from_secs(60)),
        false,
    );
    assert!(!report.deadline_blown);
    assert_eq!(report.judged_through.as_deref(), Some("b.md"));
}

// ── The baseline's generation ─────────────────────────────────────────────

fn generation(captured_at: Option<u64>, keys: &[&str]) -> SiteHashes {
    let mut h = SiteHashes::new();
    h.captured_at = captured_at;
    for k in keys {
        h.sources.insert(
            (*k).into(),
            SourceMetadata { hash: "h".into(), size: 1, mtime: 1, mtime_nanos: Some(0), ctime: None, inode: None },
        );
    }
    h
}

/// The incident: a page created after the last seal is in the stash and on
/// disk, but not in the sealed manifest's key set. Judging that key set would
/// report the page as a CREATE and buy a full rebuild that changes nothing;
/// the rule refuses to judge until the seal lands.
#[test]
fn a_stash_newer_than_the_sealed_manifest_yields_no_verdict() {
    let sealed = generation(Some(1000), &["a.md"]);
    let stash = generation(Some(2000), &["a.md", "new.md"]);
    assert!(select_baseline(Some(&stash), None, || Some(sealed)).is_none());
}

/// A pre-clock manifest cannot prove it is current; it counts as behind.
#[test]
fn a_sealed_manifest_without_a_clock_is_behind_any_stash_with_one() {
    let sealed = generation(None, &["a.md"]);
    let stash = generation(Some(2000), &["a.md"]);
    assert!(select_baseline(Some(&stash), None, || Some(sealed)).is_none());
}

/// Nothing landed since the previous pass: the cache stands and the manifest
/// is not read at all — on a cloud root the sweep walks every 2s, and the
/// manifest is site-sized.
#[test]
fn a_pass_with_nothing_new_keeps_the_cache_without_reading_the_manifest() {
    let stash = generation(Some(2000), &["a.md"]);
    let mut cache = generation(Some(2000), &["a.md"]);
    cache.sources.get_mut("a.md").unwrap().inode = Some(42);
    let chosen = select_baseline(Some(&stash), Some(cache), || unreachable!("the manifest must not be read")).unwrap();
    assert_eq!(chosen.sources["a.md"].inode, Some(42));
}

/// Once the seal has landed the manifest IS the screen: judged as is, with
/// the previous pass's copy preferred so its stat-identity refreshes survive
/// (dropping them re-hashes every re-materialized file every pass).
#[test]
fn an_equal_generation_is_judged_and_the_refreshed_copy_wins() {
    let sealed = generation(Some(2000), &["a.md"]);
    let stash = generation(Some(2000), &["a.md"]);
    let mut cache = generation(Some(1000), &["a.md"]);
    cache.captured_at = Some(2000);
    cache.sources.get_mut("a.md").unwrap().inode = Some(42);
    let stale_cache = generation(Some(1000), &["a.md"]);

    let chosen = select_baseline(None, Some(cache), || Some(sealed.clone())).unwrap();
    assert_eq!(chosen.sources["a.md"].inode, Some(42), "the refreshed copy, not a reread");

    let chosen = select_baseline(Some(&stash), Some(stale_cache), || Some(sealed)).unwrap();
    assert_eq!(chosen.captured_at, Some(2000), "a stale cache yields to the new generation");
}

/// A manifest newer than the stash is one this process never stashed — an app
/// restart, or another moss process building the same vault. It is the newest
/// truth and is judged as is; nothing from the stash is written over it.
#[test]
fn a_sealed_manifest_newer_than_the_stash_is_judged_as_is() {
    let sealed = generation(Some(3000), &["a.md", "b.md"]);
    let stash = generation(Some(2000), &["a.md"]);
    let chosen = select_baseline(Some(&stash), None, || Some(sealed)).unwrap();
    assert_eq!(chosen.captured_at, Some(3000));
    assert!(chosen.sources.contains_key("b.md"));
}
