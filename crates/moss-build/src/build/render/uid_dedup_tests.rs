//! Tests for duplicate-uid resolution.
//!
//! The hazard under test is not "two files share a uid" — it is "moss picked
//! the wrong one". A published uid is the join key for the live comment thread
//! and every signed moderation event, so handing it to the copy silently
//! orphans the published note.

use super::*;
use crate::build::manifest::live_baseline::{Baseline, LiveBaseline, LiveEntry, Unreadable};
use crate::build::types::ParsedDocument;
use std::collections::HashMap;

/// Build a `ParsedDocument` carrying only the fields the resolver reads.
fn doc(source_path: &str, uid: &str, date: Option<&str>) -> ParsedDocument {
    ParsedDocument {
        source_path: Some(source_path.to_string()),
        uid: Some(uid.to_string()),
        date: date.map(|d| d.to_string()),
        ..Default::default()
    }
}

/// The record of what is live, saying `uid` is published at `source_path`.
fn deployed(entries: &[(&str, &str)]) -> Baseline {
    Baseline::Present(LiveBaseline {
        entries: entries
            .iter()
            .map(|(source_path, uid)| LiveEntry {
                uid: (*uid).to_string(),
                url: format!("{}/", source_path.trim_end_matches(".md")),
                source_path: (*source_path).to_string(),
                title: String::new(),
            })
            .collect(),
    })
}

/// The reassignments alone, for the tests that are about the choice rather
/// than about deferral.
///
/// Every contender is reported as already built: that is the steady state, and
/// it keeps the never-built rule (which only fires against an unreadable
/// record) out of tests that are about the record's own answer.
fn resolve(
    documents: &mut [ParsedDocument],
    load: impl FnOnce() -> Baseline,
) -> Vec<UidReassignment> {
    // The previous build served every path under the uid it carries now.
    let served: HashMap<String, Option<String>> =
        documents.iter().filter_map(|d| Some((d.source_path.clone()?, d.uid.clone()))).collect();
    resolve_fresh(documents, load, |path, uid| served.get(path).is_some_and(|u| u.as_deref() == Some(uid))).reassignments
}

/// The resolver, with `built_under` saying which paths the previous build
/// served under which uid.
fn resolve_fresh(
    documents: &mut [ParsedDocument],
    load: impl FnOnce() -> Baseline,
    built_under: impl Fn(&str, &str) -> bool,
) -> UidResolution {
    let baseline = std::cell::OnceCell::new();
    resolve_duplicate_uids(documents, || baseline.get_or_init(load), built_under)
}

/// Write a markdown file with the given uid + date frontmatter into `root`.
fn write_note(root: &std::path::Path, rel: &str, uid: &str, date: &str) {
    if let Some(parent) = std::path::Path::new(rel).parent() {
        std::fs::create_dir_all(root.join(parent)).unwrap();
    }
    std::fs::write(
        root.join(rel),
        format!("---\ntitle: note\ndate: {date}\nuid: {uid}\n---\n\nbody\n"),
    )
    .unwrap();
}

/// PORTABLE TempDir pattern: artifacts go inside the repo target/ tree.
fn tempdir() -> tempfile::TempDir {
    let test_tmp = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
        .parent()
        .unwrap()
        .join("target")
        .join("test-tmp");
    std::fs::create_dir_all(&test_tmp).unwrap();
    tempfile::TempDir::new_in(&test_tmp).unwrap()
}

// ---------------------------------------------------------------
// pick_uid_keeper — the decision, isolated from I/O
// ---------------------------------------------------------------

#[test]
fn published_file_keeps_uid_against_an_older_looking_duplicate() {
    // The copy looks older by date. Published identity must override it, or
    // the ORIGINAL's uid gets rewritten and its comment thread is orphaned
    // server-side.
    let contenders = vec![
        UidContender {
            source_path: "posts/copy.md".to_string(),
            date: Some("2020-01-01".to_string()),
            published: PublishedIdentity::No,
        },
        UidContender {
            source_path: "posts/original.md".to_string(),
            date: Some("2026-07-01".to_string()),
            published: PublishedIdentity::Exact,
        },
    ];
    assert_eq!(
        pick_uid_keeper(&contenders),
        1,
        "the published file must keep the uid regardless of date"
    );
}

#[test]
fn falls_back_to_date_when_nothing_is_published() {
    // Nothing is live, so no comment thread can be orphaned — the old
    // heuristic is a fine tiebreak here.
    let contenders = vec![
        UidContender {
            source_path: "posts/b.md".to_string(),
            date: Some("2026-07-01".to_string()),
            published: PublishedIdentity::No,
        },
        UidContender {
            source_path: "posts/a.md".to_string(),
            date: Some("2020-01-01".to_string()),
            published: PublishedIdentity::No,
        },
    ];
    assert_eq!(pick_uid_keeper(&contenders), 1, "earlier date wins");
}

#[test]
fn falls_back_when_both_files_are_published_under_one_uid() {
    // A corrupt or hand-edited snapshot claims both files. Ambiguity is not
    // identity — fall through to the heuristic rather than pick arbitrarily.
    let contenders = vec![
        UidContender {
            source_path: "posts/b.md".to_string(),
            date: Some("2026-07-01".to_string()),
            published: PublishedIdentity::Exact,
        },
        UidContender {
            source_path: "posts/a.md".to_string(),
            date: Some("2020-01-01".to_string()),
            published: PublishedIdentity::Exact,
        },
    ];
    assert_eq!(pick_uid_keeper(&contenders), 1, "earlier date wins");
}

/// A file's creation time is not part of the source a sync client carries
/// (each device that receives the file gives it a new one), so a date tie is
/// broken by the composed path, the same on every machine.
#[test]
fn a_date_tie_is_broken_by_the_composed_path() {
    let contender = |path: &str| UidContender {
        source_path: path.to_string(),
        date: Some("2026-07-01".to_string()),
        published: PublishedIdentity::No,
    };
    assert_eq!(pick_uid_keeper(&[contender("posts/b.md"), contender("posts/a.md")]), 1);
    // Composed, `é.md` sorts before `éx.md`; by raw bytes the decomposed
    // `e` + accent would sort first.
    assert_eq!(pick_uid_keeper(&[contender("posts/\u{e9}.md"), contender("posts/e\u{301}x.md")]), 0);
    assert_eq!(pick_uid_keeper(&[contender("posts/e\u{301}x.md"), contender("posts/\u{e9}.md")]), 1);
}

#[test]
fn source_path_breaks_a_total_tie_so_the_outcome_is_deterministic() {
    let contenders = vec![
        UidContender {
            source_path: "posts/z.md".to_string(),
            date: None,
            published: PublishedIdentity::No,
        },
        UidContender {
            source_path: "posts/a.md".to_string(),
            date: None,
            published: PublishedIdentity::No,
        },
    ];
    assert_eq!(pick_uid_keeper(&contenders), 1);
}

#[test]
fn empty_contenders_do_not_panic() {
    assert_eq!(pick_uid_keeper(&[]), 0);
}

// ---------------------------------------------------------------
// resolve_duplicate_uids — end to end
// ---------------------------------------------------------------

#[test]
fn published_note_keeps_its_uid_and_the_copy_is_reassigned_without_a_write() {
    let tmp = tempdir();
    let root = tmp.path();
    // The copy carries the original's frontmatter verbatim (Obsidian
    // "Make a copy") AND was created first as far as the filesystem is
    // concerned — the shape a synced vault produces.
    write_note(root, "posts/copy.md", "aabbccdd", "2020-01-01");
    write_note(root, "posts/original.md", "aabbccdd", "2026-07-01");

    let mut documents = vec![
        doc("posts/copy.md", "aabbccdd", Some("2020-01-01")),
        doc("posts/original.md", "aabbccdd", Some("2026-07-01")),
    ];

    let reassignments = resolve(&mut documents, || {
        deployed(&[("posts/original.md", "aabbccdd")])
    });

    assert_eq!(documents[1].uid.as_deref(), Some("aabbccdd"),
        "the published original must keep the uid its comments are keyed to");
    assert_ne!(documents[0].uid.as_deref(), Some("aabbccdd"),
        "the copy must be reassigned");

    // Neither file is written: the copy is stamped when it is published.
    for name in ["posts/copy.md", "posts/original.md"] {
        let src = std::fs::read_to_string(root.join(name)).unwrap();
        assert!(src.contains("uid: aabbccdd"), "{name} was rewritten by a build");
    }

    assert_eq!(reassignments.len(), 1);
    assert_eq!(reassignments[0].keeper_path, "posts/original.md");
    assert_eq!(reassignments[0].reassigned_path, "posts/copy.md");
    assert_eq!(reassignments[0].uid, "aabbccdd");
}

#[test]
fn falls_back_to_the_heuristic_when_the_article_map_is_absent() {
    let tmp = tempdir();
    let root = tmp.path();
    write_note(root, "posts/older.md", "aabbccdd", "2020-01-01");
    write_note(root, "posts/newer.md", "aabbccdd", "2026-07-01");

    let mut documents = vec![
        doc("posts/newer.md", "aabbccdd", Some("2026-07-01")),
        doc("posts/older.md", "aabbccdd", Some("2020-01-01")),
    ];

    // No deploy has ever happened, and the real loader is the one that has to
    // say so: absent must reach here as `Absent` — provably nothing published —
    // and not as the "could not read it" that defers.
    let paths = crate::moss_paths::MossPaths::from_moss_dir(root.join(".moss"));
    let baseline = crate::build::manifest::live_baseline::load(&paths);
    assert_eq!(baseline, Baseline::Absent, "no record on a never-deployed vault");

    let reassignments = resolve(&mut documents, || baseline);

    assert_eq!(documents[1].uid.as_deref(), Some("aabbccdd"),
        "earlier frontmatter date keeps the uid when nothing is published");
    assert_ne!(documents[0].uid.as_deref(), Some("aabbccdd"));
    assert_eq!(reassignments.len(), 1);
    assert_eq!(reassignments[0].keeper_path, "posts/older.md");
    assert_eq!(reassignments[0].reassigned_path, "posts/newer.md");
}

#[test]
fn a_unique_uid_is_left_alone() {
    let tmp = tempdir();
    let root = tmp.path();
    write_note(root, "posts/a.md", "aabbccdd", "2026-01-01");
    write_note(root, "posts/b.md", "11223344", "2026-01-02");

    let mut documents = vec![
        doc("posts/a.md", "aabbccdd", Some("2026-01-01")),
        doc("posts/b.md", "11223344", Some("2026-01-02")),
    ];

    // Reading the record is a synced-folder file read that can block on a
    // provider; a build with no collision must never pay for it.
    let mut loaded = false;
    let reassignments = resolve(&mut documents, || {
        loaded = true;
        Baseline::Absent
    });

    assert!(!loaded, "the record of what is live must not be read when no uid collides");
    assert!(reassignments.is_empty());
    assert_eq!(documents[0].uid.as_deref(), Some("aabbccdd"));
    assert_eq!(documents[1].uid.as_deref(), Some("11223344"));
}

#[test]
fn three_way_collision_leaves_exactly_one_holder_of_the_published_uid() {
    let tmp = tempdir();
    let root = tmp.path();
    write_note(root, "posts/a.md", "aabbccdd", "2020-01-01");
    write_note(root, "posts/b.md", "aabbccdd", "2021-01-01");
    write_note(root, "posts/live.md", "aabbccdd", "2026-07-01");

    let mut documents = vec![
        doc("posts/a.md", "aabbccdd", Some("2020-01-01")),
        doc("posts/b.md", "aabbccdd", Some("2021-01-01")),
        doc("posts/live.md", "aabbccdd", Some("2026-07-01")),
    ];

    let reassignments = resolve(&mut documents, || {
        deployed(&[("posts/live.md", "aabbccdd")])
    });

    assert_eq!(documents[2].uid.as_deref(), Some("aabbccdd"));
    assert_eq!(reassignments.len(), 2);
    let fresh: Vec<&str> = documents[..2]
        .iter()
        .filter_map(|d| d.uid.as_deref())
        .collect();
    assert!(fresh.iter().all(|u| *u != "aabbccdd"));
    assert_ne!(fresh[0], fresh[1], "each reassignment gets its own uid");
}

#[test]
fn renamed_original_keeps_the_uid_via_basename_when_the_deployed_path_is_stale() {
    // The snapshot joins by exact source path, but a deploy is the ONLY
    // writer of the snapshot — so the most ordinary Obsidian sequence
    // (deploy, move the note to another folder, then duplicate it) leaves
    // every contender exact-unmatched while the uid IS live. The copy
    // carries the original's date verbatim, and on that tie the path
    // tiebreak picks the copy ('a 1.md' < 'a.md': space < dot) — rewriting
    // the PUBLISHED note's uid and orphaning its comment thread. The
    // basename fallback re-attaches published identity across the move.
    let tmp = tempdir();
    let root = tmp.path();
    // Every heuristic signal points at the copy; only published identity
    // saves the original.
    write_note(root, "essays/a 1.md", "aabbccdd", "2026-07-01");
    write_note(root, "essays/a.md", "aabbccdd", "2026-07-01");

    let mut documents = vec![
        doc("essays/a.md", "aabbccdd", Some("2026-07-01")),
        doc("essays/a 1.md", "aabbccdd", Some("2026-07-01")),
    ];

    let reassignments = resolve(&mut documents, || {
        deployed(&[("posts/a.md", "aabbccdd")])
    });

    assert_eq!(
        documents[0].uid.as_deref(),
        Some("aabbccdd"),
        "the moved original must keep the uid its live comments are keyed to"
    );
    assert_ne!(documents[1].uid.as_deref(), Some("aabbccdd"));
    assert_eq!(reassignments.len(), 1);
    assert_eq!(reassignments[0].keeper_path, "essays/a.md");
    assert_eq!(reassignments[0].reassigned_path, "essays/a 1.md");
    assert!(
        reassignments[0].live_thread_at_risk,
        "the basename fallback picked the keeper but did not PROVE it — the \
         same evidence is left by a rename plus a copy holding the old name, \
         so the advisory must still name the stake"
    );
}

#[test]
fn a_basename_decoy_does_not_get_to_claim_the_published_file_is_safe() {
    // The history the basename fallback cannot rule out, and the reason it
    // yields `Inferred` rather than `Exact`: the PUBLISHED note was renamed
    // after its last deploy, and an older duplicate still carries the name it
    // used to have. The deployed basename therefore matches the copy, and
    // matches it uniquely — the fallback's exactly-one-match rule is satisfied
    // by the wrong file.
    //
    // moss cannot tell this apart from an ordinary folder move, so it still
    // hands the copy the uid. What it must NOT do is report that as certainty:
    // the reassigned file here is the one whose live comment thread and signed
    // moderation history are at stake, and a uid is random, so nothing can
    // recover it afterwards. The flag is the author's only chance to catch it.
    let tmp = tempdir();
    let root = tmp.path();
    write_note(root, "posts/a-final.md", "aabbccdd", "2026-07-01");
    write_note(root, "drafts/a.md", "aabbccdd", "2026-07-01");

    let mut documents = vec![
        doc("posts/a-final.md", "aabbccdd", Some("2026-07-01")),
        doc("drafts/a.md", "aabbccdd", Some("2026-07-01")),
    ];

    let reassignments = resolve(&mut documents, || {
        deployed(&[("posts/a.md", "aabbccdd")])
    });

    assert_eq!(reassignments.len(), 1);
    assert_eq!(
        reassignments[0].keeper_path, "drafts/a.md",
        "the basename match still picks the keeper — it beats the date/path \
         heuristic, which cannot tell a copy from its original"
    );
    assert!(
        reassignments[0].live_thread_at_risk,
        "the file being rewritten is plausibly the published one; reporting \
         this as settled is how an unrecoverable identity loss stays silent"
    );
}

#[test]
fn ambiguous_basename_falls_back_to_the_heuristic_and_flags_the_live_risk() {
    // Two contenders share the deployed file's basename (the note was
    // duplicated into another FOLDER, not renamed). Ambiguity is not
    // identity — the heuristic decides — but the uid IS live, so the
    // reassignment must carry the flag that escalates the advisory.
    let tmp = tempdir();
    let root = tmp.path();
    write_note(root, "essays/a.md", "aabbccdd", "2020-01-01");
    write_note(root, "drafts/a.md", "aabbccdd", "2026-07-01");

    let mut documents = vec![
        doc("essays/a.md", "aabbccdd", Some("2020-01-01")),
        doc("drafts/a.md", "aabbccdd", Some("2026-07-01")),
    ];

    let reassignments = resolve(&mut documents, || {
        deployed(&[("posts/a.md", "aabbccdd")])
    });

    assert_eq!(
        documents[0].uid.as_deref(),
        Some("aabbccdd"),
        "earlier date decides when the basename match is ambiguous"
    );
    assert_eq!(reassignments.len(), 1);
    assert!(
        reassignments[0].live_thread_at_risk,
        "a live deployment exists under this uid and no contender could be \
         identified as the published file — the advisory must say so"
    );
}

// ---------------------------------------------------------------
// The user-facing surface
// ---------------------------------------------------------------

/// Shorthand for advisory-shape tests.
fn reassignment(keeper: &str, dup: &str, live_thread_at_risk: bool) -> UidReassignment {
    UidReassignment {
        uid: "aabbccdd".to_string(),
        keeper_path: keeper.to_string(),
        reassigned_path: dup.to_string(),
        new_uid: "11223344".to_string(),
        live_thread_at_risk,
    }
}

/// A reassignment moss got right is silent. Nothing under that ID was
/// published, or the record named the keeper by exact path — either way the
/// copy simply got its own identity, and the author has nothing to decide.
#[test]
fn a_reassignment_that_risked_nothing_is_not_shown() {
    assert!(
        crate::build::progress::make_duplicate_uid_advisory(&[reassignment(
            "posts/original.md",
            "posts/copy.md",
            false,
        )])
        .is_none()
    );
    assert!(crate::build::progress::make_duplicate_uid_advisory(&[]).is_none());
}

/// The one reassignment she is told about: moss had to guess which file the
/// live comments belong to, so it names both and asks her to look. What it may
/// not do is name the mechanism — "note ID" is a frontmatter field no editor
/// shows her (2026-08-30).
#[test]
fn a_live_at_risk_reassignment_names_both_files_and_asks_her_to_check() {
    let event = crate::build::progress::make_duplicate_uid_advisory(&[reassignment(
        "essays/a.md",
        "drafts/a.md",
        true,
    )])
    .expect("advisory");
    let crate::build::progress::PipelineEvent::BackgroundProgress {
        advisories, message, ..
    } = &event
    else {
        panic!("expected a BackgroundProgress advisory event");
    };
    assert_eq!(advisories.len(), 1);
    assert_eq!(advisories[0].item.as_deref(), Some("drafts/a.md"));
    let what = &advisories[0].what;
    assert!(what.contains("essays/a.md") && what.contains("drafts/a.md"), "{what}");
    assert!(
        what.contains("comments already on your site") && what.contains("check"),
        "the at-risk advisory must name the stake and ask her to look: {what}"
    );
    assert!(!what.contains("note ID"), "no frontmatter jargon: {what}");
    assert!(message.contains("essays/a.md"));
}

// ---------------------------------------------------------------
// An unreadable record defers — it does not fall back
// ---------------------------------------------------------------

#[test]
fn an_unreadable_record_reassigns_nothing() {
    // Every heuristic signal points at the copy here (it carries the
    // original's date verbatim and sorts first), which is exactly what a
    // synced vault produces — so falling back would rewrite the PUBLISHED
    // note's uid and orphan its comment thread, with no undo. The record is
    // withheld rather than absent, and that difference is the whole fix.
    let tmp = tempdir();
    let root = tmp.path();
    write_note(root, "posts/copy.md", "aabbccdd", "2020-01-01");
    write_note(root, "posts/original.md", "aabbccdd", "2026-07-01");

    let mut documents = vec![
        doc("posts/copy.md", "aabbccdd", Some("2020-01-01")),
        doc("posts/original.md", "aabbccdd", Some("2026-07-01")),
    ];

    // Both files are known to a previous build, so the never-built rule has
    // nothing to say and the deferral stands.
    let resolution = resolve_fresh(
        &mut documents,
        || Baseline::Unreadable(Unreadable::Withheld),
        |_, _| true,
    );

    assert!(resolution.reassignments.is_empty(), "no uid may be reassigned");
    // The paths ride along, because the advisory names the two files rather
    // than the uid — the uid is not a thing the author has ever seen.
    assert_eq!(resolution.deferred.len(), 1);
    assert_eq!(resolution.deferred[0].uid, "aabbccdd");
    assert_eq!(
        resolution.deferred[0].paths,
        vec!["posts/copy.md".to_string(), "posts/original.md".to_string()]
    );
    assert_eq!(resolution.deferred_because, Some(Unreadable::Withheld));
    assert_eq!(documents[0].uid.as_deref(), Some("aabbccdd"));
    assert_eq!(documents[1].uid.as_deref(), Some("aabbccdd"));
}

#[test]
fn a_corrupt_record_defers_the_same_way_a_withheld_one_does() {
    // Waiting will not fix a corrupt record — the next landed publish will —
    // but the cost of guessing is identical, so the decision is identical.
    let tmp = tempdir();
    let root = tmp.path();
    write_note(root, "posts/copy.md", "aabbccdd", "2020-01-01");
    write_note(root, "posts/original.md", "aabbccdd", "2026-07-01");

    let mut documents = vec![
        doc("posts/copy.md", "aabbccdd", Some("2020-01-01")),
        doc("posts/original.md", "aabbccdd", Some("2026-07-01")),
    ];

    let resolution = resolve_fresh(
        &mut documents,
        || Baseline::Unreadable(Unreadable::Corrupt),
        |_, _| true,
    );

    assert!(resolution.reassignments.is_empty());
    assert_eq!(resolution.deferred_because, Some(Unreadable::Corrupt));
}

// ---------------------------------------------------------------
// A triples-bearing record answers the collision directly, and a
// half-updated legacy record defers rather than reaching the heuristic.
// ---------------------------------------------------------------

/// End to end, through a REAL record on disk: `record_landed`'s shape
/// (`triples` populated, no join) resolves a collision by published identity,
/// never falling to the date/path heuristic that would rewrite the
/// published note's uid.
#[test]
fn a_record_with_triples_resolves_the_collision_without_the_heuristic() {
    let tmp = tempdir();
    let root = tmp.path();
    // Heuristic signals point at the WRONG file: the copy carries the
    // original's date verbatim and would win the date/path tiebreak.
    write_note(root, "posts/copy.md", "aabbccdd", "2020-01-01");
    write_note(root, "posts/original.md", "aabbccdd", "2020-01-01");

    let mut documents = vec![
        doc("posts/copy.md", "aabbccdd", Some("2020-01-01")),
        doc("posts/original.md", "aabbccdd", Some("2020-01-01")),
    ];

    let paths = crate::moss_paths::MossPaths::new(root);
    crate::build::manifest::published_record::save(
        &paths,
        &crate::build::manifest::change_set::PublishedSnapshot {
            generation_id: "g1".into(),
            target: "moss:site-1".into(),
            published_at: "2026-08-20T00:00:00Z".into(),
            sources: Default::default(),
            source_to_output: Default::default(),
            files: Default::default(),
            uids: Default::default(),
            triples: Some(vec![LiveEntry {
                uid: "aabbccdd".into(),
                url: "posts/original/".into(),
                source_path: "posts/original.md".into(),
                title: String::new(),
            }]),
            asset_source_to_output: None,
        },
    )
    .unwrap();

    let reassignments = resolve(&mut documents, || {
        crate::build::manifest::live_baseline::load(&paths)
    });

    assert_eq!(reassignments.len(), 1);
    assert_eq!(reassignments[0].keeper_path, "posts/original.md");
    assert_eq!(reassignments[0].reassigned_path, "posts/copy.md");
}

/// A half-updated legacy record defers the collision
/// instead of dropping the uid out of the baseline and falling through to the
/// heuristic — an invariant this pass exists to make structurally true.
#[test]
fn a_half_updated_record_defers_instead_of_reaching_the_heuristic() {
    let tmp = tempdir();
    let root = tmp.path();
    write_note(root, "posts/copy.md", "aabbccdd", "2020-01-01");
    write_note(root, "posts/original.md", "aabbccdd", "2026-07-01");

    let mut documents = vec![
        doc("posts/copy.md", "aabbccdd", Some("2020-01-01")),
        doc("posts/original.md", "aabbccdd", Some("2026-07-01")),
    ];

    let paths = crate::moss_paths::MossPaths::new(root);
    crate::build::manifest::published_record::save(
        &paths,
        &crate::build::manifest::change_set::PublishedSnapshot {
            generation_id: "g1".into(),
            target: "moss:site-1".into(),
            published_at: "2026-08-20T00:00:00Z".into(),
            sources: Default::default(),
            // `source_to_output` never advanced past the last SEALED publish —
            // the source path the live uid maps to has no entry here.
            source_to_output: Default::default(),
            files: Default::default(),
            uids: [("posts/original.md".to_string(), "aabbccdd".to_string())].into(),
            triples: None,
            asset_source_to_output: None,
        },
    )
    .unwrap();

    let baseline = crate::build::manifest::live_baseline::load(&paths);
    assert_eq!(
        baseline,
        Baseline::Unreadable(crate::build::manifest::live_baseline::Unreadable::Inconsistent)
    );

    let resolution = resolve_fresh(&mut documents, || baseline, |_, _| true);

    assert!(
        resolution.reassignments.is_empty(),
        "no uid may be reassigned against an inconsistent record"
    );
    assert_eq!(resolution.deferred.len(), 1);
    assert_eq!(resolution.deferred[0].uid, "aabbccdd");
}

/// The rule that makes a deferral rare: a source path this install has never
/// built cannot be the published page, so its uid is safe to re-mint with no
/// record at all. Duplicating a note makes exactly this shape.
///
/// The heuristic is pointed at the WRONG file here — the copy carries an older
/// date and would win a date tiebreak — so a pass only proves the never-built
/// rule decided it.
#[test]
fn a_duplicate_this_install_has_never_built_is_re_minted_without_the_record() {
    let tmp = tempdir();
    let root = tmp.path();
    write_note(root, "posts/copy.md", "aabbccdd", "2020-01-01");
    write_note(root, "posts/original.md", "aabbccdd", "2026-07-01");

    let mut documents = vec![
        doc("posts/copy.md", "aabbccdd", Some("2020-01-01")),
        doc("posts/original.md", "aabbccdd", Some("2026-07-01")),
    ];

    let resolution = resolve_fresh(
        &mut documents,
        || Baseline::Unreadable(Unreadable::Withheld),
        |src, _| src == "posts/original.md",
    );

    assert!(resolution.deferred.is_empty(), "the author must never hear about this");
    assert_eq!(resolution.deferred_because, None);
    assert_eq!(resolution.reassignments.len(), 1);
    assert_eq!(resolution.reassignments[0].keeper_path, "posts/original.md");
    assert_eq!(resolution.reassignments[0].reassigned_path, "posts/copy.md");
    assert!(
        !resolution.reassignments[0].live_thread_at_risk,
        "a file moss has never built cannot have a live thread, so nothing is at risk"
    );
    assert_eq!(documents[1].uid.as_deref(), Some("aabbccdd"));
    assert_ne!(documents[0].uid.as_deref(), Some("aabbccdd"));
}

/// A vault this install has never built — a fresh clone whose `.moss/deploy`
/// has not synced down yet — singles out nobody. The record may well name one
/// of these as published, so nothing may be re-minted.
#[test]
fn a_collision_where_nothing_was_built_before_still_defers() {
    let tmp = tempdir();
    let root = tmp.path();
    write_note(root, "posts/copy.md", "aabbccdd", "2020-01-01");
    write_note(root, "posts/original.md", "aabbccdd", "2026-07-01");

    let mut documents = vec![
        doc("posts/copy.md", "aabbccdd", Some("2020-01-01")),
        doc("posts/original.md", "aabbccdd", Some("2026-07-01")),
    ];

    let resolution = resolve_fresh(
        &mut documents,
        || Baseline::Unreadable(Unreadable::Withheld),
        |_, _| false,
    );

    assert!(resolution.reassignments.is_empty());
    assert_eq!(resolution.deferred.len(), 1);
}

/// The uid each page is built under before duplicates are resolved.
mod built_under {
    use super::*;

    fn doc(source: &str, uid: Option<&str>) -> ParsedDocument {
        ParsedDocument {
            source_path: Some(source.to_string()),
            uid: uid.map(str::to_string),
            ..Default::default()
        }
    }

    fn hash(source: &str) -> Option<String> {
        Some(format!("hash of {source}"))
    }

    fn assign(documents: &mut [ParsedDocument], published: impl FnOnce() -> HashMap<String, String>) {
        let mut taken: HashSet<String> = documents.iter().filter_map(|doc| doc.uid.clone()).collect();
        assign_missing(documents, &mut taken, published, hash);
    }

    fn uids(documents: &[ParsedDocument]) -> Vec<Option<&str>> {
        documents.iter().map(|doc| doc.uid.as_deref()).collect()
    }

    #[test]
    fn a_page_keeps_its_own_uid_and_the_record_never_loads_when_none_is_missing() {
        let mut documents = vec![doc("a.md", Some("aaaa1111"))];
        assign(&mut documents, || panic!("the record is read only when a page lacks a uid"));
        assert_eq!(uids(&documents), [Some("aaaa1111")]);
    }

    /// A published page with no frontmatter block has its uid only in the record,
    /// and must be built under it on any machine.
    #[test]
    fn a_page_without_a_uid_is_built_under_the_one_published_from_its_path() {
        let mut documents = vec![doc("plain.md", None)];
        assign(&mut documents, || {
            [("plain.md".to_string(), "bbbb2222".to_string())].into()
        });
        assert_eq!(uids(&documents), [Some("bbbb2222")]);
    }

    /// The published page was renamed, carrying its uid in its frontmatter, and a
    /// new note now sits at its old path. The new note must not take the uid: that
    /// would hand it the published page's comment thread.
    #[test]
    fn a_published_uid_another_page_now_carries_is_not_reused_at_the_old_path() {
        let mut documents = vec![doc("renamed.md", Some("cccc3333")), doc("old.md", None)];
        assign(&mut documents, || {
            [("old.md".to_string(), "cccc3333".to_string())].into()
        });
        assert_eq!(documents[0].uid.as_deref(), Some("cccc3333"));
        assert!(documents[1].uid.as_deref().is_some_and(|uid| uid != "cccc3333"), "{:?}", uids(&documents));
    }

    /// Two machines, or two builds in flight at once, give a new page one uid.
    #[test]
    fn a_new_page_is_built_under_the_uid_its_path_and_bytes_give() {
        let mut a = vec![doc("new.md", None), doc("other.md", None)];
        let mut b = vec![doc("new.md", None)];
        assign(&mut a, HashMap::new);
        assign(&mut b, HashMap::new);
        let derived = crate::build::markdown::generate_uid("new.md", "hash of new.md");
        assert_eq!(uids(&a)[0], Some(derived.as_str()));
        assert_eq!(uids(&b), [Some(derived.as_str())]);
        assert_ne!(uids(&a)[1], uids(&a)[0]);
    }

    #[test]
    fn a_slot_file_is_given_no_uid() {
        let mut documents = vec![ParsedDocument { slot_only: true, ..doc("footer.md", None) }];
        assign(&mut documents, HashMap::new);
        assert_eq!(uids(&documents), [None]);
    }

    /// A copy is built under the same new uid on every build until a publish
    /// writes it into the file, and the author is told about it once.
    #[test]
    fn a_duplicate_is_reported_on_the_build_that_resolves_it_only() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("a.md"), "a").unwrap();
        std::fs::write(root.join("b.md"), "b").unwrap();
        let paths = crate::moss_paths::MossPaths::new(root);
        let notes = || vec![doc("a.md", Some("aaaa1111")), doc("b.md", Some("aaaa1111"))];

        let mut first = notes();
        assert_eq!(assign_note_ids(&mut first, || crate::build::manifest::live_baseline::load(&paths), |_, _| false, hash).reassignments.len(), 1);
        let copy_uid = first[1].uid.clone().unwrap();
        let mut second = notes();
        let built_under = |src: &str, uid: &str| src == "a.md" || (src == "b.md" && uid == copy_uid);
        assert!(assign_note_ids(&mut second, || crate::build::manifest::live_baseline::load(&paths), built_under, hash).reassignments.is_empty());
        assert_eq!(uids(&first), uids(&second), "both builds give the copy one uid");
    }
}
