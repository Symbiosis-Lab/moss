//! Tests for duplicate-uid resolution.
//!
//! The hazard under test is not "two files share a uid" — it is "moss picked
//! the wrong one". A rewritten uid is unrecoverable (uids are random, not
//! derived) and it is the join key for the live comment thread and every
//! signed moderation event, so handing it to the copy silently orphans the
//! published note.

use super::*;
use crate::build::manifest::live_baseline::{Baseline, LiveBaseline, LiveEntry, Unreadable};
use crate::build::types::ParsedDocument;

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
    root: &std::path::Path,
    load: impl FnOnce() -> Baseline,
) -> Vec<UidReassignment> {
    resolve_duplicate_uids(documents, root, load, |_| true).reassignments
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
    // The regression. iCloud / Obsidian Sync reset btime, and duplicating a
    // note copies the original's `date` — so the copy can look strictly
    // "older" on both signals the old heuristic used. Published identity must
    // override both, or the ORIGINAL's uid gets rewritten and its comment
    // thread is orphaned server-side.
    let contenders = vec![
        UidContender {
            source_path: "posts/copy.md".to_string(),
            date: Some("2020-01-01".to_string()),
            birth_time: Some(std::time::UNIX_EPOCH),
            published: PublishedIdentity::No,
        },
        UidContender {
            source_path: "posts/original.md".to_string(),
            date: Some("2026-07-01".to_string()),
            birth_time: Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(999_999)),
            published: PublishedIdentity::Exact,
        },
    ];
    assert_eq!(
        pick_uid_keeper(&contenders),
        1,
        "the published file must keep the uid regardless of date/btime"
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
            birth_time: None,
            published: PublishedIdentity::No,
        },
        UidContender {
            source_path: "posts/a.md".to_string(),
            date: Some("2020-01-01".to_string()),
            birth_time: None,
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
            birth_time: None,
            published: PublishedIdentity::Exact,
        },
        UidContender {
            source_path: "posts/a.md".to_string(),
            date: Some("2020-01-01".to_string()),
            birth_time: None,
            published: PublishedIdentity::Exact,
        },
    ];
    assert_eq!(pick_uid_keeper(&contenders), 1, "earlier date wins");
}

#[test]
fn birth_time_breaks_a_date_tie() {
    let contenders = vec![
        UidContender {
            source_path: "posts/b.md".to_string(),
            date: Some("2026-07-01".to_string()),
            birth_time: Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(100)),
            published: PublishedIdentity::No,
        },
        UidContender {
            source_path: "posts/a.md".to_string(),
            date: Some("2026-07-01".to_string()),
            birth_time: Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(10)),
            published: PublishedIdentity::No,
        },
    ];
    assert_eq!(pick_uid_keeper(&contenders), 1);
}

#[test]
fn source_path_breaks_a_total_tie_so_the_outcome_is_deterministic() {
    let contenders = vec![
        UidContender {
            source_path: "posts/z.md".to_string(),
            date: None,
            birth_time: None,
            published: PublishedIdentity::No,
        },
        UidContender {
            source_path: "posts/a.md".to_string(),
            date: None,
            birth_time: None,
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
// resolve_duplicate_uids — end to end, including the frontmatter write-back
// ---------------------------------------------------------------

#[test]
fn published_note_keeps_its_uid_and_the_copy_is_rewritten_on_disk() {
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

    let reassignments = resolve(&mut documents, root, || {
        deployed(&[("posts/original.md", "aabbccdd")])
    });

    assert_eq!(documents[1].uid.as_deref(), Some("aabbccdd"),
        "the published original must keep the uid its comments are keyed to");
    assert_ne!(documents[0].uid.as_deref(), Some("aabbccdd"),
        "the copy must be reassigned");

    // The write-back landed in the copy's frontmatter, not the original's.
    let copy_src = std::fs::read_to_string(root.join("posts/copy.md")).unwrap();
    let original_src = std::fs::read_to_string(root.join("posts/original.md")).unwrap();
    assert!(!copy_src.contains("uid: aabbccdd"), "copy still holds the contested uid");
    assert!(original_src.contains("uid: aabbccdd"), "original's uid was rewritten");

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

    let reassignments = resolve(&mut documents, root, || baseline);

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
    let reassignments = resolve(&mut documents, root, || {
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

    let reassignments = resolve(&mut documents, root, || {
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
    // every contender exact-unmatched while the uid IS live. On a synced
    // vault the copy carries the original's date verbatim and btime is
    // reset, and on a full tie the lexicographic tiebreak picks the copy
    // ('a 1.md' < 'a.md': space < dot) — rewriting the PUBLISHED note's
    // uid and orphaning its comment thread. The basename fallback
    // re-attaches published identity across the move.
    let tmp = tempdir();
    let root = tmp.path();
    // The copy is written FIRST: on a synced vault btime is rewritten per
    // device, so the copy can carry the older birth time. Every heuristic
    // signal then points at the copy — only published identity saves the
    // original.
    write_note(root, "essays/a 1.md", "aabbccdd", "2026-07-01");
    write_note(root, "essays/a.md", "aabbccdd", "2026-07-01");

    let mut documents = vec![
        doc("essays/a.md", "aabbccdd", Some("2026-07-01")),
        doc("essays/a 1.md", "aabbccdd", Some("2026-07-01")),
    ];

    let reassignments = resolve(&mut documents, root, || {
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

    let reassignments = resolve(&mut documents, root, || {
        deployed(&[("posts/a.md", "aabbccdd")])
    });

    assert_eq!(reassignments.len(), 1);
    assert_eq!(
        reassignments[0].keeper_path, "drafts/a.md",
        "the basename match still picks the keeper — it beats the timestamp \
         heuristic, which on a synced vault is systematically wrong"
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

    let reassignments = resolve(&mut documents, root, || {
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

#[test]
fn a_rewritten_loser_round_trips_through_the_frontmatter_parser() {
    // The resolver's loser write goes through replace_uid_in_frontmatter.
    // Minted uids are 8 hex chars, so digits-e-digits shapes occur
    // (~1.1% of mints) — unquoted, YAML parses one as a float and the next
    // build reads a 13-digit string: a silent identity flip that orphans
    // the comment thread with no advisory. The quoted write is the fix;
    // prove the misparse is dead at the ONE parser the pipeline uses
    // (parse_typed_frontmatter). The uid is `753659e7` — the shape
    // whose float FITS in f64 (unquoted it parses to "7536590000000");
    // larger exponents like `12e45678` overflow and accidentally round-trip
    // as strings, so they would not detect an unquoted regression.
    let content = "---\ntitle: note\ndate: 2026-01-01\nuid: aabbccdd\n---\n\nbody\n";
    let updated = crate::build::markdown::replace_uid_in_frontmatter(content, "753659e7");
    let fm = crate::build::markdown::parse_typed_frontmatter(&updated);
    assert_eq!(
        fm.uid.as_deref(),
        Some("753659e7"),
        "the written uid must survive the pipeline's frontmatter parser \
         byte-for-byte; got a YAML-float coercion from: {updated}"
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
// The in-memory uid may only move when the disk moved with it
// ---------------------------------------------------------------

#[test]
fn an_unreadable_duplicate_keeps_its_old_uid_in_memory() {
    // A cloud-evicted source fails the read. Reassigning in memory anyway
    // would emit — and sign — this build against a uid no file claims, and
    // the next build would read the old uid and collide all over again.
    let tmp = tempdir();
    let root = tmp.path();
    write_note(root, "posts/original.md", "aabbccdd", "2026-07-01");
    // posts/copy.md is deliberately never written.

    let mut documents = vec![
        doc("posts/copy.md", "aabbccdd", Some("2020-01-01")),
        doc("posts/original.md", "aabbccdd", Some("2026-07-01")),
    ];

    let reassignments = resolve(&mut documents, root, || {
        deployed(&[("posts/original.md", "aabbccdd")])
    });

    assert_eq!(
        documents[0].uid.as_deref(),
        Some("aabbccdd"),
        "an unreadable file must keep the uid its source still carries"
    );
    assert!(
        reassignments.is_empty(),
        "a reassignment that never reached disk must not be reported as one"
    );
}

#[cfg(unix)]
#[test]
fn an_unwritable_duplicate_keeps_its_old_uid_in_memory() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir();
    let root = tmp.path();
    write_note(root, "posts/copy.md", "aabbccdd", "2020-01-01");
    write_note(root, "posts/original.md", "aabbccdd", "2026-07-01");

    let copy = root.join("posts/copy.md");
    std::fs::set_permissions(&copy, std::fs::Permissions::from_mode(0o444)).unwrap();

    let mut documents = vec![
        doc("posts/copy.md", "aabbccdd", Some("2020-01-01")),
        doc("posts/original.md", "aabbccdd", Some("2026-07-01")),
    ];

    let reassignments = resolve(&mut documents, root, || {
        deployed(&[("posts/original.md", "aabbccdd")])
    });

    // Restore write permission so TempDir cleanup succeeds.
    std::fs::set_permissions(&copy, std::fs::Permissions::from_mode(0o644)).unwrap();

    assert_eq!(
        documents[0].uid.as_deref(),
        Some("aabbccdd"),
        "a failed write must leave the in-memory uid matching the file"
    );
    assert!(reassignments.is_empty());
}

#[test]
fn a_duplicate_with_no_uid_line_is_not_treated_as_reassigned() {
    // `replace_uid_in_frontmatter` is a no-op when there is no `uid:` line, so
    // the file would keep whatever it had. That is not a persisted
    // reassignment, and must not be recorded as one.
    let tmp = tempdir();
    let root = tmp.path();
    std::fs::create_dir_all(root.join("posts")).unwrap();
    std::fs::write(
        root.join("posts/copy.md"),
        "---\ntitle: note\ndate: 2020-01-01\n---\n\nbody\n",
    )
    .unwrap();
    write_note(root, "posts/original.md", "aabbccdd", "2026-07-01");

    let mut documents = vec![
        doc("posts/copy.md", "aabbccdd", Some("2020-01-01")),
        doc("posts/original.md", "aabbccdd", Some("2026-07-01")),
    ];

    let reassignments = resolve(&mut documents, root, || {
        deployed(&[("posts/original.md", "aabbccdd")])
    });

    assert_eq!(documents[0].uid.as_deref(), Some("aabbccdd"));
    assert!(reassignments.is_empty());
}

// ---------------------------------------------------------------
// An unreadable record defers — it does not fall back
// ---------------------------------------------------------------

#[test]
fn an_unreadable_record_reassigns_nothing_and_writes_no_frontmatter() {
    // Every heuristic signal points at the copy here (it carries the
    // original's date verbatim and sorts first), which is exactly what a
    // synced vault produces — so falling back would rewrite the PUBLISHED
    // note's uid and orphan its comment thread, with no undo. The record is
    // withheld rather than absent, and that difference is the whole fix.
    let tmp = tempdir();
    let root = tmp.path();
    write_note(root, "posts/copy.md", "aabbccdd", "2020-01-01");
    write_note(root, "posts/original.md", "aabbccdd", "2026-07-01");
    let before = std::fs::read_to_string(root.join("posts/copy.md")).unwrap();

    let mut documents = vec![
        doc("posts/copy.md", "aabbccdd", Some("2020-01-01")),
        doc("posts/original.md", "aabbccdd", Some("2026-07-01")),
    ];

    // Both files are known to a previous build, so the never-built rule has
    // nothing to say and the deferral stands.
    let resolution = resolve_duplicate_uids(
        &mut documents,
        root,
        || Baseline::Unreadable(Unreadable::Withheld),
        |_| true,
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
    assert_eq!(
        std::fs::read_to_string(root.join("posts/copy.md")).unwrap(),
        before,
        "not one byte of the user's frontmatter may be rewritten on a guess"
    );
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

    let resolution = resolve_duplicate_uids(
        &mut documents,
        root,
        || Baseline::Unreadable(Unreadable::Corrupt),
        |_| true,
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
/// never falling to the date/birth-time heuristic that would rewrite the
/// published note's uid.
#[test]
fn a_record_with_triples_resolves_the_collision_without_the_heuristic() {
    let tmp = tempdir();
    let root = tmp.path();
    // Heuristic signals point at the WRONG file: the copy carries the
    // original's date verbatim and would win a date/birth-time tiebreak.
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
        },
    )
    .unwrap();

    let reassignments = resolve(&mut documents, root, || {
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
        },
    )
    .unwrap();

    let baseline = crate::build::manifest::live_baseline::load(&paths);
    assert_eq!(
        baseline,
        Baseline::Unreadable(crate::build::manifest::live_baseline::Unreadable::Inconsistent)
    );

    let resolution = resolve_duplicate_uids(&mut documents, root, || baseline, |_| true);

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

    let resolution = resolve_duplicate_uids(
        &mut documents,
        root,
        || Baseline::Unreadable(Unreadable::Withheld),
        |src| src == "posts/original.md",
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

    let resolution = resolve_duplicate_uids(
        &mut documents,
        root,
        || Baseline::Unreadable(Unreadable::Withheld),
        |_| false,
    );

    assert!(resolution.reassignments.is_empty());
    assert_eq!(resolution.deferred.len(), 1);
}
