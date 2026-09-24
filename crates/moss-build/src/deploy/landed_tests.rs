//! What a landed publish writes down.
//!
//! The record is what rename detection diffs against on the next publish, so
//! these cover the two states it can be left in: written whole from the seal,
//! or written from the seal with the standing note IDs carried forward because
//! this build's article map could not be read. There was a third — a sealless
//! plugin publish — until track P slice P3 made every publish carry a seal;
//! its two tests went with the branch, one of them subsumed by the unreadable
//! -article-map case it had been asserting a second time.
//!
//! The other half of `record_landed` — recomputing the publish read model — is
//! `AppState`-shaped and its tests stayed with it in `deploy::app_seam`.

use crate::build::manifest::change_set::PublishedSnapshot;
use crate::build::manifest::{published_record, PendingManifest, SealedManifest};
use crate::build::manifest::live_baseline::{self, Baseline};
use crate::build::served_path::ServedPath;
use crate::moss_paths::MossPaths;
use crate::build::types::SourceMetadata;
use crate::types::content::SiteHashes;

const TARGET: &str = "moss:site-1";

fn sealed(pages: &[(&str, &str, &str, &[u8])]) -> SealedManifest {
    let mut pending = PendingManifest::new(SiteHashes::default());
    for (src, out, hash, bytes) in pages {
        let sp = ServedPath::from_source(out).unwrap();
        pending.register(&sp, bytes, crate::build::manifest::HashBucket::Files);
        pending.register_source_mapping(src.to_string(), &sp);
        pending.register_page_source_hash(
            src.to_string(),
            SourceMetadata { hash: hash.to_string(), size: 1, mtime: 1, mtime_nanos: None, ctime: None, inode: None },
        );
    }
    pending.seal()
}

// ── The record of what is live ──────────────────────────────────────────

/// Rename detection diffs the current article map against what was live at the
/// last publish, so the publish is where the note IDs are captured — whoever
/// shipped it.
///
/// What lands is `{source path → uid}` inside the record that was already being
/// written, and nothing else. The article map carries every page's markdown and
/// rendered HTML, and copying it put a second copy of the user's whole site in
/// their Drive (moss#1079).
#[tokio::test]
async fn a_landed_publish_records_what_went_live() {
    let dir = tempfile::tempdir().unwrap();
    let mp = MossPaths::new(dir.path());
    std::fs::create_dir_all(mp.build_dir()).unwrap();
    std::fs::write(
        mp.article_map(),
        br##"{"articles":{"posts/hello/":{"source_path":"posts/hello.md","title":"Hello","content":"# the whole body","html_content":"<p>and its HTML</p>","url_path":"posts/hello/","uid":"aabbccdd"}}}"##,
    )
    .unwrap();

    let history_dir = tempfile::tempdir().unwrap();
    let manifest = sealed(&[("posts/hello.md", "posts/hello/index.html", "h-a", b"A")]);
    let history = crate::deploy::history::HistoryStore::at(history_dir.path().to_path_buf());
    super::record_what_is_live(&mp, &manifest, TARGET, &history).await;

    let Baseline::Present(live) = live_baseline::load(&mp) else {
        panic!("a landed publish must leave a readable record")
    };
    assert_eq!(live.urls_by_uid().get("aabbccdd"), Some(&"posts/hello/"));
    assert_eq!(
        live.entries[0].source_path, "posts/hello.md",
        "the source path is what duplicate-uid resolution joins on"
    );
    let raw =
        std::fs::read_to_string(published_record::path_for(&mp, TARGET)).unwrap();
    assert!(
        !raw.contains("the whole body") && !raw.contains("and its HTML"),
        "no page content may be copied into the record: {raw}"
    );
}

/// The article map is regenerable build output, so a build can legitimately
/// reach the record with none to read. That must not CLEAR what is live: an
/// empty map is read as "nothing was ever published", which is the whole
/// moss#1079 failure arrived at from the writing side.
#[tokio::test]
async fn an_unreadable_article_map_does_not_erase_the_note_ids() {
    let dir = tempfile::tempdir().unwrap();
    let mp = MossPaths::new(dir.path());
    let manifest = sealed(&[("kept.md", "kept/index.html", "h-kept", b"K")]);
    let mut standing =
        PublishedSnapshot::from_sealed(&manifest, TARGET, "2026-08-09T00:00:00Z".into());
    standing.uids.insert("kept.md".into(), "aabbccdd".into());
    published_record::save(&mp, &standing).unwrap();

    // A publish that has a seal, and an article map that does not parse.
    std::fs::create_dir_all(mp.build_dir()).unwrap();
    std::fs::write(mp.article_map(), b"not json").unwrap();

    let history_dir = tempfile::tempdir().unwrap();
    let history = crate::deploy::history::HistoryStore::at(history_dir.path().to_path_buf());
    super::record_what_is_live(&mp, &manifest, TARGET, &history).await;

    let Baseline::Present(live) = live_baseline::load(&mp) else {
        panic!("the standing note IDs must survive")
    };
    assert_eq!(live.urls_by_uid().get("aabbccdd"), Some(&"kept/"));
}

/// A prebuilt tree names no moss pages, so its record says so rather than
/// carrying the last moss build's page map: the next moss-built publish then
/// counts every page as added, which is what it does to the live site. The
/// note IDs last seen live still carry forward, as they do above.
#[test]
fn a_prebuilt_publish_records_its_tree_and_keeps_the_note_ids() {
    let dir = tempfile::tempdir().unwrap();
    let mp = MossPaths::new(dir.path());
    let manifest = sealed(&[("kept.md", "kept/index.html", "h-kept", b"K")]);
    let mut standing =
        PublishedSnapshot::from_sealed(&manifest, TARGET, "2026-08-09T00:00:00Z".into());
    standing.triples = Some(vec![crate::build::manifest::change_set::LiveEntry {
        uid: "aabbccdd".into(),
        url: "kept/".into(),
        source_path: "kept.md".into(),
        title: "Kept".into(),
    }]);
    published_record::save(&mp, &standing).unwrap();

    let tree = std::collections::HashMap::from([("index.html".to_string(), "100644:abc".to_string())]);
    super::record_prebuilt_landed(dir.path(), "prebuilt-gen", TARGET, &tree);

    let record = published_record::load_for(&mp, Some(TARGET)).expect("a record was written");
    assert_eq!(record.generation_id, "prebuilt-gen");
    assert_eq!(record.files, tree);
    assert!(record.sources.is_empty() && record.source_to_output.is_empty());
    let Baseline::Present(live) = live_baseline::load(&mp) else {
        panic!("the note IDs last seen live must survive a prebuilt publish")
    };
    assert_eq!(live.urls_by_uid().get("aabbccdd"), Some(&"kept/"));
    let next = crate::build::manifest::change_set::classify(Some(&record), &manifest);
    assert!(next.classified && next.added == 1 && next.edited == 0 && next.deleted == 0, "{next:?}");
}

// ── The page-change summary a receipt renders (publish-receipt step 4) ──

/// A rename and a brand-new page between two publishes must both reach the
/// summary `record_what_is_live` returns, and reach it correctly diffed —
/// against what was live an instant BEFORE this second publish, which is the
/// ordering the summary's own doc comment calls out. The rename must land as
/// one Moved row, not an Added+Removed pair, since it never changed the
/// source's content hash — only `detect_renames`'s uid match sees it move.
#[tokio::test]
async fn a_publish_summarizes_a_rename_and_a_new_page_since_the_last_one() {
    let dir = tempfile::tempdir().unwrap();
    let mp = MossPaths::new(dir.path());
    std::fs::create_dir_all(mp.build_dir()).unwrap();

    std::fs::write(
        mp.article_map(),
        br##"{"articles":{"a/":{"source_path":"a.md","title":"Page A","content":"c","url_path":"a/","uid":"u-a"}}}"##,
    )
    .unwrap();
    let manifest_1 = sealed(&[("a.md", "a/index.html", "h-a", b"A")]);
    let history_dir = tempfile::tempdir().unwrap();
    let history = crate::deploy::history::HistoryStore::at(history_dir.path().to_path_buf());
    let first = super::record_what_is_live(&mp, &manifest_1, TARGET, &history).await;
    assert!(first.records.is_empty(), "a first publish has nothing to have moved or added yet");

    // "a/" renames to "a-new/" (same uid, same source, content unchanged);
    // "b/" is a brand-new page.
    std::fs::write(
        mp.article_map(),
        br##"{"articles":{"a-new/":{"source_path":"a.md","title":"Page A","content":"c","url_path":"a-new/","uid":"u-a"},"b/":{"source_path":"b.md","title":"Page B","content":"c","url_path":"b/","uid":"u-b"}}}"##,
    )
    .unwrap();
    let manifest_2 = sealed(&[
        ("a.md", "a-new/index.html", "h-a", b"A"),
        ("b.md", "b/index.html", "h-b", b"B"),
    ]);
    let second = super::record_what_is_live(&mp, &manifest_2, TARGET, &history).await;

    assert_eq!(second.pages_moved, 1);
    assert_eq!(second.pages_added, 1);
    assert_eq!(second.pages_removed, 0);
    let moved = second.records.iter().find(|r| r.path == "a-new/").expect("the rename must render as a Moved row");
    assert_eq!(moved.old_path.as_deref(), Some("a/"));
    let added = second.records.iter().find(|r| r.path == "b/").expect("the new page must render as an Added row");
    assert_eq!(added.title, "Page B");
}

// ── Publish history (ADR-083, slice 1) ──────────────────────────────────

/// The end-to-end wiring: `record_landed` — not just the lower-level
/// `record_what_is_live` the tests above exercise directly — must itself
/// reach `deploy::history::HistoryStore::snapshot`. `record_landed` takes
/// the store as a parameter, so this test hands it a `HistoryStore::at` a
/// tempdir rather than the real machine-local one.
///
/// This is the test the design's required ablation targets: delete the one
/// call to `deploy::history` inside `record_what_is_live` and this goes red.
#[tokio::test]
async fn a_landed_publish_snapshots_into_publish_history() {
    let vault = tempfile::tempdir().unwrap();
    let history_root = tempfile::tempdir().unwrap();
    let mp = MossPaths::new(vault.path());
    std::fs::create_dir_all(mp.build_dir()).unwrap();
    std::fs::write(vault.path().join("hello.md"), b"hello world").unwrap();

    let manifest = sealed(&[("hello.md", "hello/index.html", "h-hello", b"H")]);
    let ports = crate::deploy::one_shot::HeadlessDeployPorts;
    let history = crate::deploy::history::HistoryStore::at(history_root.path().to_path_buf());
    super::record_landed(vault.path(), &manifest, TARGET, &ports, &history).await;

    // `HistoryStore::at` is the explicit-directory test seam, so this vault's
    // data lands directly under `history_root` rather than the real
    // `.moss/history/` a `HistoryStore::in_vault` call would resolve to.
    let publishes: Vec<_> = std::fs::read_dir(history_root.path().join("publishes")).unwrap().flatten().collect();
    assert_eq!(publishes.len(), 1, "one landed publish must write exactly one record");

    let blob_count = crate::deploy::history::count_blob_files_for_tests(&history_root.path().join("objects"));
    assert!(blob_count >= 1, "the published file's bytes must be kept in the object store");
}
