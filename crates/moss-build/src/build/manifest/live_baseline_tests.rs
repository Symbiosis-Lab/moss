//! Tests for reading what is live out of the publish records.
//!
//! Two hazards, and every test here is about one of them.
//!
//! - **Absent must never be substituted for unreadable.** A build that cannot
//!   read the record and pretends nothing was published emits no forwarding
//!   stub for a page it renamed (a permanent 404) and rewrites a published
//!   note's identity on a coin flip (no undo). That is the uid-remint bug.
//! - **The pre-fix copy of the whole article map must leave** — but only
//!   once something else holds what it says.

use super::*;
use crate::build::manifest::published_record;

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

/// A record as a landed publish writes it: which source went to which output,
/// and which note ID was serving there.
fn record(target: &str, pages: &[(&str, &str, &str)]) -> PublishedSnapshot {
    PublishedSnapshot {
        generation_id: "g1".into(),
        target: target.into(),
        published_at: "2026-08-20T00:00:00Z".into(),
        sources: pages.iter().map(|(src, _, _)| ((*src).into(), "h".into())).collect(),
        source_to_output: pages
            .iter()
            .map(|(src, out, _)| ((*src).into(), (*out).into()))
            .collect(),
        files: pages.iter().map(|(_, out, _)| ((*out).into(), "h".into())).collect(),
        uids: pages.iter().map(|(src, _, uid)| ((*src).into(), (*uid).into())).collect(),
        triples: None,
    }
}

/// A record as `record_landed` writes it since the triples migration: `triples` populated
/// directly from the same source `record`'s `uids`/`source_to_output` came
/// from, so there is nothing to join.
fn record_with_triples(target: &str, pages: &[(&str, &str, &str)]) -> PublishedSnapshot {
    let mut snap = record(target, pages);
    snap.triples = Some(
        pages
            .iter()
            .map(|(src, out, uid)| LiveEntry {
                uid: (*uid).into(),
                url: crate::build::scan::article_map::to_pretty_url(out),
                source_path: (*src).into(),
                title: String::new(),
            })
            .collect(),
    );
    snap
}

/// The pre-fix file: a serialized `ArticleMap`, keyed by pretty URL.
fn legacy_bytes(pages: &[(&str, &str, &str)]) -> String {
    let mut map = ArticleMap::default();
    for (url, source_path, uid) in pages {
        map.articles.insert(
            (*url).to_string(),
            crate::build::scan::article_map::ArticleInfo {
                source_path: (*source_path).to_string(),
                title: "t".into(),
                content: "the whole body".into(),
                html_content: None,
                frontmatter: HashMap::new(),
                url_path: (*url).to_string(),
                date: None,
                tags: vec![],
                uid: Some((*uid).to_string()),
            },
        );
    }
    serde_json::to_string(&map).unwrap()
}

// ── Reading ─────────────────────────────────────────────────────────────

/// A folder can publish to more than one host, and a redirect stub is emitted
/// into the one output tree they are all served from — so a URL live at either
/// is a URL someone can have linked to.
#[test]
fn every_targets_record_is_read_into_one_baseline() {
    let dir = tmp();
    let paths = MossPaths::new(dir.path());
    published_record::save(&paths, &record("moss:site-1", &[("a.md", "a/index.html", "aaaa1111")]))
        .unwrap();
    published_record::save(
        &paths,
        &record("onionpress:abc.onion", &[("b.md", "b/index.html", "bbbb2222")]),
    )
    .unwrap();

    let Baseline::Present(live) = load(&paths) else { panic!("both records are readable") };
    let by_uid = live.urls_by_uid();
    assert_eq!(by_uid.get("aaaa1111"), Some(&"a/"));
    assert_eq!(by_uid.get("bbbb2222"), Some(&"b/"));
}

/// The corrupt-record shape: bytes that are there and do not parse. The caller must
/// be able to tell that apart from "nothing was ever published", because the
/// two license opposite actions.
#[test]
fn a_record_that_does_not_parse_is_unreadable_not_absent() {
    let dir = tmp();
    let paths = MossPaths::new(dir.path());
    std::fs::create_dir_all(paths.deploy_records_dir()).unwrap();
    std::fs::write(paths.deploy_records_dir().join("moss-deadbeef.json"), "not json").unwrap();

    assert_eq!(load(&paths), Baseline::Unreadable(Unreadable::Corrupt));
}

/// A record written before moss recorded note IDs answers nothing, and the file
/// that used to answer is gone. That is not absence — a publish HAS happened —
/// and guessing here is what rewrites a live note's identity.
#[test]
fn a_record_without_note_ids_is_not_absence() {
    let dir = tmp();
    let paths = MossPaths::new(dir.path());
    let mut old = record("moss:site-1", &[("a.md", "a/index.html", "aaaa1111")]);
    old.uids.clear();
    published_record::save(&paths, &old).unwrap();

    assert_eq!(load(&paths), Baseline::Unreadable(Unreadable::PredatesUids));
}

/// The ordinary state of a folder nobody has published: provably nothing, which
/// is the one state where the heuristics are allowed to run.
#[test]
fn a_folder_that_never_published_reads_as_absent() {
    let dir = tmp();
    assert_eq!(load(&MossPaths::new(dir.path())), Baseline::Absent);
}

/// A legacy (no-`triples`) record whose `uids` names a source path with no
/// matching `source_to_output` entry is the half-updated shape.
/// Joining it anyway would drop that uid — source path and all — out of the
/// baseline entirely, which is exactly what lets a later duplicate-uid
/// collision fall through to the heuristic and mint a fresh uid (the
/// uid-remint bug, reopened). Since the triples migration, the record defers
/// instead, and the OTHER record's
/// entries do not paper over it — an inconsistent record is unusable for
/// everyone, the same rule `read_legacy`'s `Unreadable` already applies.
#[test]
fn a_half_updated_legacy_record_defers_instead_of_dropping() {
    let dir = tmp();
    let paths = MossPaths::new(dir.path());
    let mut rec = record("moss:site-1", &[("a.md", "a/index.html", "aaaa1111")]);
    rec.uids.insert("ghost.md".into(), "cccc3333".into());
    published_record::save(&paths, &rec).unwrap();

    assert_eq!(load(&paths), Baseline::Unreadable(Unreadable::Inconsistent));
}

/// A record with `triples` needs no join at all — not even when its legacy
/// `uids`/`source_to_output` pair, if it were consulted, would be incomplete.
/// `triples` is the sole source once it is present; the legacy pair is not
/// read.
#[test]
fn a_record_with_triples_needs_no_join() {
    let dir = tmp();
    let paths = MossPaths::new(dir.path());
    let mut rec = record_with_triples("moss:site-1", &[("a.md", "a/index.html", "aaaa1111")]);
    // Deliberately make the legacy pair incomplete — proves it is never
    // consulted once `triples` is `Some`.
    rec.uids.insert("ghost.md".into(), "cccc3333".into());
    published_record::save(&paths, &rec).unwrap();

    let Baseline::Present(live) = load(&paths) else {
        panic!("triples answer directly, with no join to fail")
    };
    assert_eq!(live.entries.len(), 1);
    assert_eq!(live.urls_by_uid().get("aaaa1111"), Some(&"a/"));
}

/// Write triples, read them back: no loss, and the value that comes back out
/// is exactly what was written in.
#[test]
fn triples_round_trip_through_the_record() {
    let dir = tmp();
    let paths = MossPaths::new(dir.path());
    let rec = record_with_triples(
        "moss:site-1",
        &[("a.md", "a/index.html", "aaaa1111"), ("b.md", "b/index.html", "bbbb2222")],
    );
    published_record::save(&paths, &rec).unwrap();

    let reloaded = published_record::load_for(&paths, Some("moss:site-1")).unwrap();
    assert_eq!(reloaded.triples, rec.triples);

    let Baseline::Present(live) = load(&paths) else { panic!("readable") };
    let by_uid = live.urls_by_uid();
    assert_eq!(by_uid.get("aaaa1111"), Some(&"a/"));
    assert_eq!(by_uid.get("bbbb2222"), Some(&"b/"));
}

/// [`backfill_triples`] freezes a legacy record's join into `triples`, once,
/// when the pair is decidably complete — so a later read never has to re-join
/// this record.
#[test]
fn a_decidably_complete_legacy_record_is_backfilled_once() {
    let dir = tmp();
    let paths = MossPaths::new(dir.path());
    published_record::save(
        &paths,
        &record("moss:site-1", &[("a.md", "a/index.html", "aaaa1111")]),
    )
    .unwrap();

    migrate(&paths);

    let backfilled = published_record::load_for(&paths, Some("moss:site-1")).unwrap();
    assert_eq!(
        backfilled.triples,
        Some(vec![LiveEntry {
            uid: "aaaa1111".into(),
            url: "a/".into(),
            source_path: "a.md".into(),
            title: String::new(),
        }]),
        "the join's result is frozen into triples, not left to re-derive every load"
    );
}

/// The other half of that rule: a legacy record whose pair is NOT decidably
/// complete is never backfilled — there is nothing safe to freeze, and
/// persisting the lossy join would make a guess permanent.
#[test]
fn an_inconsistent_legacy_record_is_never_backfilled() {
    let dir = tmp();
    let paths = MossPaths::new(dir.path());
    let mut rec = record("moss:site-1", &[("a.md", "a/index.html", "aaaa1111")]);
    rec.uids.insert("ghost.md".into(), "cccc3333".into());
    published_record::save(&paths, &rec).unwrap();

    migrate(&paths);

    let still = published_record::load_for(&paths, Some("moss:site-1")).unwrap();
    assert!(still.triples.is_none(), "nothing here was safe to freeze");
    assert_eq!(load(&paths), Baseline::Unreadable(Unreadable::Inconsistent));
}

/// `paths_by_uid` keeps every claim rather than picking one: two targets that
/// disagree are ambiguity, and `uid_dedup` treats ambiguity as no signal at all
/// rather than as a reason to rewrite a note's identity.
#[test]
fn two_targets_disagreeing_about_a_note_yield_both_paths() {
    let dir = tmp();
    let paths = MossPaths::new(dir.path());
    published_record::save(&paths, &record("moss:site-1", &[("a.md", "a/index.html", "aaaa1111")]))
        .unwrap();
    published_record::save(
        &paths,
        &record("onionpress:abc.onion", &[("moved.md", "a/index.html", "aaaa1111")]),
    )
    .unwrap();

    let Baseline::Present(live) = load(&paths) else { panic!("readable") };
    let claims = live.paths_by_uid();
    assert_eq!(claims["aaaa1111"].len(), 2);
}

/// Until the migration has run, the old file is the only baseline a folder
/// published by an earlier moss has — and ignoring it would miss every rename
/// made since, which 404s for good rather than recovering on the next build.
#[test]
fn the_pre_1079_snapshot_answers_while_it_is_still_the_only_baseline() {
    for which in ["deploy", "data"] {
        let dir = tmp();
        let paths = MossPaths::new(dir.path());
        let at = dir
            .path()
            .join(".moss")
            .join(which)
            .join("deployed-article-map.json");
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(&at, legacy_bytes(&[("old/page/", "old.md", "aaaa1111")])).unwrap();

        let Baseline::Present(live) = load(&paths) else {
            panic!("the legacy snapshot under .moss/{which}/ must answer")
        };
        assert_eq!(live.urls_by_uid().get("aaaa1111"), Some(&"old/page/"));
    }
}

/// The projection a publish records, and the two things it drops: a page with
/// no note ID (nothing to join on) and one with no source path (nothing to join
/// it to).
#[test]
fn the_recorded_projection_is_note_ids_and_nothing_else() {
    let map: ArticleMap = serde_json::from_str(&legacy_bytes(&[
        ("a/", "a.md", "aaaa1111"),
        ("b/", "", "bbbb2222"),
    ]))
    .unwrap();

    let uids = uids_of(&map);
    assert_eq!(uids.get("a.md").map(String::as_str), Some("aaaa1111"));
    assert_eq!(uids.len(), 1, "a page with no source path joins to nothing");
}

// ── Migration ───────────────────────────────────────────────────────────

/// The record already says everything the old file said, so the old file is
/// superseded — and an unreadable superseded file is still superseded. Leaving
/// it means ~5 MB keeps syncing, and keeps being asked for, forever.
#[test]
fn a_superseded_snapshot_goes_even_when_it_cannot_be_read() {
    let dir = tmp();
    let paths = MossPaths::new(dir.path());
    published_record::save(&paths, &record("moss:site-1", &[("a.md", "a/index.html", "aaaa1111")]))
        .unwrap();
    std::fs::create_dir_all(paths.deploy_dir()).unwrap();
    std::fs::write(paths.deployed_article_map(), "not json").unwrap();

    migrate(&paths);

    assert!(!paths.deployed_article_map().exists());
}

/// The other half of that rule, and the one that matters: an unreadable file
/// that is still the ONLY baseline stays exactly where it is. Deleting it
/// destroys the thing this module exists to protect; the next build imports it
/// if the provider hands it back.
#[test]
fn an_unreadable_snapshot_survives_while_it_is_the_only_baseline() {
    let dir = tmp();
    let paths = MossPaths::new(dir.path());
    std::fs::create_dir_all(paths.deploy_dir()).unwrap();
    std::fs::write(paths.deployed_article_map(), "not json").unwrap();

    migrate(&paths);

    assert!(paths.deployed_article_map().exists(), "the only baseline is never deleted");
    assert_eq!(load(&paths), Baseline::Unreadable(Unreadable::Corrupt));
}

/// A folder published by a moss that wrote records but no note IDs: the note
/// IDs move into every record, and only then does the old file go. This is the
/// arm that keeps the rename baseline across the move.
#[test]
fn note_ids_move_into_the_record_before_the_old_file_goes() {
    let dir = tmp();
    let paths = MossPaths::new(dir.path());
    let mut old = record("moss:site-1", &[("old.md", "old/page/index.html", "unused")]);
    old.uids.clear();
    published_record::save(&paths, &old).unwrap();
    std::fs::create_dir_all(paths.deploy_dir()).unwrap();
    std::fs::write(
        paths.deployed_article_map(),
        legacy_bytes(&[("old/page/", "old.md", "aaaa1111")]),
    )
    .unwrap();

    migrate(&paths);

    assert!(!paths.deployed_article_map().exists(), "imported, so superseded");
    let Baseline::Present(live) = load(&paths) else { panic!("the record now answers") };
    assert_eq!(live.urls_by_uid().get("aaaa1111"), Some(&"old/page/"));
}

/// Nothing to import INTO. The snapshot is still the whole baseline, `load`
/// reads it directly, and the first landed publish is what retires it.
#[test]
fn with_no_record_at_all_the_snapshot_stays() {
    let dir = tmp();
    let paths = MossPaths::new(dir.path());
    std::fs::create_dir_all(paths.deploy_dir()).unwrap();
    std::fs::write(
        paths.deployed_article_map(),
        legacy_bytes(&[("old/page/", "old.md", "aaaa1111")]),
    )
    .unwrap();

    migrate(&paths);

    assert!(paths.deployed_article_map().exists());
    let Baseline::Present(live) = load(&paths) else { panic!("it still answers") };
    assert_eq!(live.urls_by_uid().get("aaaa1111"), Some(&"old/page/"));
}

/// A record file the platform hands back as an error rather than as bytes —
/// the withheld case, which `Corrupt` does not stand in for: waiting ends one
/// and never the other, and the two say different things to the user.
#[test]
fn a_record_that_cannot_be_opened_is_withheld_not_absent() {
    let dir = tmp();
    let paths = MossPaths::new(dir.path());
    // A directory where a record should be: the read fails without the file
    // being missing, which is the shape an evicted file arrives in.
    std::fs::create_dir_all(paths.deploy_records_dir().join("moss-abc.json")).unwrap();

    assert_eq!(load(&paths), Baseline::Unreadable(Unreadable::Withheld));
}

/// The hazardous migration shape, and the one the two non-negotiables are
/// about: records exist, none carries a note ID, and the snapshot that does
/// cannot be read. Nothing may be deleted — the snapshot is still the only
/// baseline this vault has.
#[test]
fn records_without_note_ids_do_not_license_deleting_an_unreadable_snapshot() {
    let dir = tmp();
    let paths = MossPaths::new(dir.path());
    let mut old = record("moss:site-1", &[("a.md", "a/index.html", "unused")]);
    old.uids.clear();
    published_record::save(&paths, &old).unwrap();
    std::fs::create_dir_all(paths.deploy_dir()).unwrap();
    std::fs::write(paths.deployed_article_map(), "not json").unwrap();

    migrate(&paths);

    assert!(paths.deployed_article_map().exists(), "the only baseline is never deleted");
}

/// The same rule one step earlier: if the RECORDS cannot be read, this build
/// cannot show that anything supersedes the snapshot, so it does not get to
/// delete it either.
#[test]
fn unreadable_records_license_nothing() {
    let dir = tmp();
    let paths = MossPaths::new(dir.path());
    std::fs::create_dir_all(paths.deploy_records_dir().join("moss-abc.json")).unwrap();
    std::fs::create_dir_all(paths.deploy_dir()).unwrap();
    std::fs::write(
        paths.deployed_article_map(),
        legacy_bytes(&[("old/page/", "old.md", "aaaa1111")]),
    )
    .unwrap();

    migrate(&paths);

    assert!(paths.deployed_article_map().exists());
}

/// The import is only a supersession for pages the records can place. A page
/// the record has never mapped survives the import as a note ID pointing
/// nowhere, so the snapshot stays and `load` unions the two — the alternative
/// is deleting the last copy of where that page is live.
#[test]
fn a_page_no_record_can_place_keeps_the_snapshot_alive() {
    let dir = tmp();
    let paths = MossPaths::new(dir.path());
    let mut old = record("moss:site-1", &[("a.md", "a/index.html", "unused")]);
    old.uids.clear();
    published_record::save(&paths, &old).unwrap();
    std::fs::create_dir_all(paths.deploy_dir()).unwrap();
    std::fs::write(
        paths.deployed_article_map(),
        legacy_bytes(&[("a/", "a.md", "aaaa1111"), ("gone/", "gone.md", "bbbb2222")]),
    )
    .unwrap();

    migrate(&paths);

    assert!(
        paths.deployed_article_map().exists(),
        "a page the records cannot place is not superseded"
    );
    let Baseline::Present(live) = load(&paths) else { panic!("both are readable") };
    let by_uid = live.urls_by_uid();
    assert_eq!(by_uid.get("aaaa1111"), Some(&"a/"));
    assert_eq!(by_uid.get("bbbb2222"), Some(&"gone/"), "the union keeps it reachable");
}

/// Both copies are read before either is deleted. Deleting the one on the
/// evidence of the other is the move this module exists to forbid, and the two
/// files are written by different moss versions, so they need not agree.
#[test]
fn an_unreadable_second_copy_protects_the_readable_first_one() {
    let dir = tmp();
    let paths = MossPaths::new(dir.path());
    let mut old = record("moss:site-1", &[("old.md", "old/page/index.html", "unused")]);
    old.uids.clear();
    published_record::save(&paths, &old).unwrap();
    std::fs::create_dir_all(paths.deploy_dir()).unwrap();
    std::fs::create_dir_all(paths.data_dir()).unwrap();
    std::fs::write(
        paths.deployed_article_map(),
        legacy_bytes(&[("old/page/", "old.md", "aaaa1111")]),
    )
    .unwrap();
    let other = paths.data_dir().join("deployed-article-map.json");
    std::fs::write(&other, "not json").unwrap();

    migrate(&paths);

    assert!(paths.deployed_article_map().exists(), "read only one, deleted neither");
    assert!(other.exists());
}

// ── From the article map ────────────────────────────────────────────────

/// `from_article_map` is the baseline `redirects::detect_renames` diffs
/// against, and step 4's change-record rows need every entry's title, not
/// just its uid/url/source_path — confirm it carries the title through
/// rather than dropping it on the floor.
#[test]
fn from_article_map_carries_the_title_through() {
    let mut map = ArticleMap::default();
    map.articles.insert(
        "a/".to_string(),
        crate::build::scan::article_map::ArticleInfo {
            source_path: "a.md".into(),
            title: "My Page".into(),
            content: String::new(),
            html_content: None,
            frontmatter: HashMap::new(),
            url_path: "a/".into(),
            date: None,
            tags: vec![],
            uid: Some("aaaa1111".into()),
        },
    );

    let baseline = from_article_map(&map);

    assert_eq!(baseline.entries.len(), 1);
    assert_eq!(baseline.entries[0].title, "My Page");
}
