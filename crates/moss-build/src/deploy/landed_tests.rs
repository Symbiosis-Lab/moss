//! What a landed publish writes down.
//!
//! The record is what rename detection diffs against on the next publish, so
//! these cover what it is written from: the seal of the generation that went
//! live, whatever the article map on disk says by then.
//!
//! The other half of `record_landed` — recomputing the publish read model — is
//! `AppState`-shaped and its tests stayed with it in `deploy::app_seam`.

use crate::build::manifest::change_set::PublishedSnapshot;
use crate::build::manifest::{published_record, PendingManifest, SealedManifest};
use crate::build::manifest::live_baseline::{self, Baseline};
use crate::build::served_path::ServedPath;
use crate::moss_paths::MossPaths;
use crate::build::stat::FileStat;
use crate::build::types::SourceMetadata;
use crate::types::content::SiteHashes;

const TARGET: &str = "moss:site-1";

fn sealed(pages: &[(&str, &str, &str, &[u8])]) -> SealedManifest {
    sealed_with_uids(pages, &[])
}

/// A seal whose pages in `uids` (source, uid) were built under those uids.
fn sealed_with_uids(pages: &[(&str, &str, &str, &[u8])], uids: &[(&str, &str)]) -> SealedManifest {
    let mut pending = PendingManifest::new(SiteHashes::default());
    for (src, out, hash, bytes) in pages {
        let sp = ServedPath::from_source(out).unwrap();
        pending.register(&sp, bytes, crate::build::manifest::HashBucket::Files);
        pending.register_source_mapping(src.to_string(), &sp);
        pending.register_page_source_hash(
            src.to_string(),
            SourceMetadata { hash: hash.to_string(), stat: FileStat::whole_second(1, 1) },
        );
    }
    for (src, uid) in uids {
        pending.register_page_meta(src.to_string(), built_under(uid));
    }
    pending.seal()
}

fn built_under(uid: &str) -> crate::types::content::PageMeta {
    crate::types::content::PageMeta { uid: Some(uid.into()), ..Default::default() }
}

/// The record half of a landing, with no uid stamp.
async fn record_what_is_live(
    mp: &MossPaths,
    sealed: &SealedManifest,
    target: &str,
    history: &crate::deploy::history::HistoryStore,
) -> crate::deploy::change_record::PageChangeSummary {
    let live = super::read_live_article_map(mp).await;
    let published = live_baseline::from_sealed(sealed);
    let live = live.map(|map| super::pinned_to(map, &published));
    super::record_what_is_live(mp, sealed, target, history, live.as_ref(), published, &std::collections::HashMap::new()).await
}

// ── The record of what is live ──────────────────────────────────────────

/// Rename detection diffs the current article map against what was live at the
/// last publish, so the publish is where the note IDs are captured — whoever
/// shipped it.
///
/// What lands is `{source path → uid}` inside the record that was already being
/// written, and nothing else. The article map carries every page's markdown and
/// rendered HTML, and copying it put a second copy of the user's whole site in
/// their Drive.
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
    let manifest = sealed_with_uids(&[("posts/hello.md", "posts/hello/index.html", "h-a", b"A")], &[("posts/hello.md", "aabbccdd")]);
    let history = crate::deploy::history::HistoryStore::at(history_dir.path().to_path_buf());
    record_what_is_live(&mp, &manifest, TARGET, &history).await;

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

/// A site whose only page is its home has no article to track, so its article
/// map is empty. What the publish wrote must still read as a record of what is
/// live: the next build of that site must not report it missing.
#[tokio::test]
async fn a_publish_of_a_site_with_no_tracked_pages_leaves_a_baseline() {
    let dir = tempfile::tempdir().unwrap();
    let mp = MossPaths::new(dir.path());
    std::fs::create_dir_all(mp.build_dir()).unwrap();
    std::fs::write(mp.article_map(), br#"{"articles":{}}"#).unwrap();

    let history_dir = tempfile::tempdir().unwrap();
    let manifest = sealed(&[("index.md", "index.html", "h-home", b"H")]);
    let history = crate::deploy::history::HistoryStore::at(history_dir.path().to_path_buf());
    record_what_is_live(&mp, &manifest, TARGET, &history).await;

    assert_eq!(live_baseline::load(&mp), Baseline::Present(live_baseline::LiveBaseline::default()));
}

/// The article map is regenerable build output, so a build can legitimately
/// reach the record with none to read. The note IDs come from the seal, so an
/// unreadable map cannot clear what is live: an empty record reads as "nothing
/// was ever published".
#[tokio::test]
async fn an_unreadable_article_map_does_not_erase_the_note_ids() {
    let dir = tempfile::tempdir().unwrap();
    let mp = MossPaths::new(dir.path());
    let manifest = sealed_with_uids(&[("kept.md", "kept/index.html", "h-kept", b"K")], &[("kept.md", "aabbccdd")]);
    std::fs::create_dir_all(mp.build_dir()).unwrap();
    std::fs::write(mp.article_map(), b"not json").unwrap();

    let history_dir = tempfile::tempdir().unwrap();
    let history = crate::deploy::history::HistoryStore::at(history_dir.path().to_path_buf());
    record_what_is_live(&mp, &manifest, TARGET, &history).await;

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
    let manifest_1 = sealed_with_uids(&[("a.md", "a/index.html", "h-a", b"A")], &[("a.md", "u-a")]);
    let history_dir = tempfile::tempdir().unwrap();
    let history = crate::deploy::history::HistoryStore::at(history_dir.path().to_path_buf());
    let first = record_what_is_live(&mp, &manifest_1, TARGET, &history).await;
    assert!(first.records.is_empty(), "a first publish has nothing to have moved or added yet");

    // "a/" renames to "a-new/" (same uid, same source, content unchanged);
    // "b/" is a brand-new page.
    std::fs::write(
        mp.article_map(),
        br##"{"articles":{"a-new/":{"source_path":"a.md","title":"Page A","content":"c","url_path":"a-new/","uid":"u-a"},"b/":{"source_path":"b.md","title":"Page B","content":"c","url_path":"b/","uid":"u-b"}}}"##,
    )
    .unwrap();
    let manifest_2 = sealed_with_uids(
        &[("a.md", "a-new/index.html", "h-a", b"A"), ("b.md", "b/index.html", "h-b", b"B")],
        &[("a.md", "u-a"), ("b.md", "u-b")],
    );
    let second = record_what_is_live(&mp, &manifest_2, TARGET, &history).await;

    assert_eq!(second.pages_moved, 1);
    assert_eq!(second.pages_added, 1);
    assert_eq!(second.pages_removed, 0);
    let moved = second.records.iter().find(|r| r.path == "a-new/").expect("the rename must render as a Moved row");
    assert_eq!(moved.old_path.as_deref(), Some("a/"));
    let added = second.records.iter().find(|r| r.path == "b/").expect("the new page must render as an Added row");
    assert_eq!(added.title, "Page B");
}

/// A file the author deleted is named in the receipt data, not only counted:
/// one address record in served form, the page records untouched.
#[tokio::test]
async fn a_publish_names_a_folder_file_the_author_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let mp = MossPaths::new(dir.path());
    std::fs::create_dir_all(mp.build_dir()).unwrap();
    std::fs::write(
        mp.article_map(),
        br##"{"articles":{"a/":{"source_path":"a.md","title":"Page A","content":"c","url_path":"a/","uid":"u-a"}}}"##,
    )
    .unwrap();
    std::fs::write(dir.path().join("guide.pdf"), b"%PDF").unwrap();
    let history_dir = tempfile::tempdir().unwrap();
    let history = crate::deploy::history::HistoryStore::at(history_dir.path().to_path_buf());

    let mut pending = PendingManifest::new(SiteHashes::default());
    let page = ServedPath::from_source("a/index.html").unwrap();
    pending.register(&page, b"A", crate::build::manifest::HashBucket::Files);
    pending.register_source_mapping("a.md".into(), &page);
    let meta = |h: &str| SourceMetadata { hash: h.into(), stat: FileStat::whole_second(1, 1) };
    pending.register_page_source_hash("a.md".into(), meta("h-a"));
    let pdf = ServedPath::from_source("guide.pdf").unwrap();
    pending.register(&pdf, b"%PDF", crate::build::manifest::HashBucket::Files);
    pending.register_page_source_hash("guide.pdf".into(), meta("h-pdf"));
    record_what_is_live(&mp, &pending.seal(), TARGET, &history).await;

    std::fs::remove_file(dir.path().join("guide.pdf")).unwrap();
    let second = record_what_is_live(&mp, &sealed(&[("a.md", "a/index.html", "h-a", b"A")]), TARGET, &history).await;

    assert_eq!(address_records(&second), [("/guide.pdf".to_string(), crate::build::manifest::change_set::RemovalReason::AuthorRemoved)]);
    assert!(second.records.is_empty(), "a file is not a page row: {:?}", second.records);
}

fn address_records(
    summary: &crate::deploy::change_record::PageChangeSummary,
) -> Vec<(String, crate::build::manifest::change_set::RemovalReason)> {
    summary.removed_addresses.iter().map(|r| (r.path.clone(), r.reason)).collect()
}

/// One publish after another, the second dropping `gone` from the output set
/// (with or without a stub left at the address).
async fn second_publish_summary(
    first_outputs: &[&str],
    second_outputs: &[&str],
) -> crate::deploy::change_record::PageChangeSummary {
    let dir = tempfile::tempdir().unwrap();
    let mp = MossPaths::new(dir.path());
    std::fs::create_dir_all(mp.build_dir()).unwrap();
    std::fs::write(
        mp.article_map(),
        br##"{"articles":{"a/":{"source_path":"a.md","title":"Page A","content":"c","url_path":"a/","uid":"u-a"}}}"##,
    )
    .unwrap();
    let history_dir = tempfile::tempdir().unwrap();
    let history = crate::deploy::history::HistoryStore::at(history_dir.path().to_path_buf());
    let build = |extra: &[&str]| {
        let mut pending = PendingManifest::new(SiteHashes::default());
        let page = ServedPath::from_source("a/index.html").unwrap();
        pending.register(&page, b"A", crate::build::manifest::HashBucket::Files);
        pending.register_source_mapping("a.md".into(), &page);
        pending.register_page_source_hash(
            "a.md".into(),
            SourceMetadata { hash: "h".into(), stat: FileStat::whole_second(1, 1) },
        );
        for out in extra {
            pending.register(&ServedPath::from_source(out).unwrap(), b"x", crate::build::manifest::HashBucket::Files);
        }
        pending.seal()
    };
    record_what_is_live(&mp, &build(first_outputs), TARGET, &history).await;
    record_what_is_live(&mp, &build(second_outputs), TARGET, &history).await
}

/// A generated file whose loss the author accepted is named, with its reason.
#[tokio::test]
async fn an_accepted_unexplained_removal_is_named_in_the_receipt_data() {
    let summary = second_publish_summary(&["legacy-feed.xml"], &[]).await;
    assert_eq!(
        address_records(&summary),
        [("/legacy-feed.xml".to_string(), crate::build::manifest::change_set::RemovalReason::Unexplained)]
    );
}

/// An address a stub still answers was not lost, so there is nothing to name.
#[tokio::test]
async fn an_address_that_a_stub_still_answers_yields_no_record() {
    let summary = second_publish_summary(&["old.html"], &["old.html"]).await;
    assert!(address_records(&summary).is_empty(), "{:?}", summary.removed_addresses);
}

// ── Publish history (slice 1) ──────────────────────────────────

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

// ── The uid stamp ───────────────────────────────────────────────────────

/// A vault holding `files`, an article map naming `articles` (source, uid) and
/// `folders` (source, uid), and a seal of the published ones as they are now,
/// built under those uids.
fn stamped_vault(
    files: &[(&str, &str)],
    articles: &[(&str, &str)],
    folders: &[(&str, &str)],
    published: &[&str],
) -> (tempfile::TempDir, SealedManifest) {
    let vault = tempfile::tempdir().unwrap();
    for (path, text) in files {
        let at = vault.path().join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
    }
    let mut map = crate::build::scan::article_map::ArticleMap::default();
    for (src, uid) in articles {
        let info: crate::build::scan::article_map::ArticleInfo = serde_json::from_value(serde_json::json!({
            "source_path": src, "title": "", "content": "", "url_path": format!("{src}/"), "date": null, "uid": uid,
        }))
        .unwrap();
        map.articles.insert(info.url_path.clone(), info);
    }
    for (src, _) in folders {
        map.pages.insert(crate::build::scan::article_map::to_pretty_url(&format!("{}/index.html", src.trim_end_matches(".md"))), src.to_string());
    }
    let mp = MossPaths::new(vault.path());
    std::fs::create_dir_all(mp.build_dir()).unwrap();
    std::fs::write(mp.article_map(), serde_json::to_vec(&map).unwrap()).unwrap();
    let mut pending = PendingManifest::new(SiteHashes::default());
    for src in published {
        let bytes = std::fs::read(vault.path().join(src)).unwrap();
        let sp = ServedPath::from_source(&format!("{}/index.html", src.trim_end_matches(".md"))).unwrap();
        // A uid builds to the same page, so the output stays put.
        pending.register(&sp, src.as_bytes(), crate::build::manifest::HashBucket::Files);
        pending.register_source_mapping(src.to_string(), &sp);
        pending.register_page_source_hash(src.to_string(), crate::build::render::blocking::source_metadata(None, &bytes));
        if let Some((_, uid)) = articles.iter().chain(folders).find(|(path, _)| path == src) {
            let meta = built_under(uid);
            pending.register_page_meta(src.to_string(), meta);
        }
    }
    (vault, pending.seal())
}

async fn land(vault: &std::path::Path, sealed: &SealedManifest) -> PublishedSnapshot {
    let history = tempfile::tempdir().unwrap();
    let history = crate::deploy::history::HistoryStore::at(history.path().to_path_buf());
    super::record_landed(vault, sealed, TARGET, &crate::deploy::one_shot::HeadlessDeployPorts, &history).await;
    published_record::load_for(&MossPaths::new(vault), Some(TARGET)).expect("a record")
}

fn read(vault: &std::path::Path, path: &str) -> String {
    std::fs::read_to_string(vault.join(path)).unwrap()
}

/// A minted uid can read as a YAML number (`753659e7` is a float, `46160604` an
/// integer); stamped unquoted, the next build would read another uid and the
/// page would lose its comment thread.
#[test]
fn a_stamped_uid_reads_back_as_written_whatever_its_digits() {
    let dir = tempfile::tempdir().unwrap();
    for (i, uid) in ["753659e7", "46160604", "12e45678", "abcd1234"].iter().enumerate() {
        let source = format!("p{i}.md");
        let text = "---\ntitle: T\n---\nBody\n";
        std::fs::write(dir.path().join(&source), text).unwrap();
        let published_hash = crate::build::render::blocking::source_metadata(None, text.as_bytes()).hash;
        assert!(super::stamp_one(dir.path(), &source, uid, &published_hash).unwrap().is_some());
        let after = std::fs::read_to_string(dir.path().join(&source)).unwrap();
        assert_eq!(crate::build::markdown::parse_typed_frontmatter(&after).uid.as_deref(), Some(*uid), "{after}");
    }
}

fn uid_in(vault: &std::path::Path, path: &str) -> Option<String> {
    crate::build::markdown::parse_typed_frontmatter(&read(vault, path)).uid
}

/// Every published page gets its uid, folder pages and pages with no
/// frontmatter included, with the rest of its bytes kept; a page that already
/// holds its uid and a page that was not published keep every byte. The record
/// takes the stamped bytes' hash, so a build after the publish has nothing left
/// to publish.
#[tokio::test]
async fn a_landing_writes_each_published_pages_uid_and_nothing_else() {
    let (vault, sealed) = stamped_vault(
        &[
            ("index.md", "---\ntitle: Home\n---\nHi\n"),
            ("posts/a.md", "---\r\ntitle: A\r\n---\r\nBody\r\n"),
            ("posts/held.md", "---\nuid: c0ffee00\n---\nHeld\n"),
            ("posts/bare.md", "Bare\n"),
            ("posts/draft.md", "---\ntitle: D\n---\n"),
        ],
        &[("posts/a.md", "aabbccdd"), ("posts/held.md", "c0ffee00"), ("posts/bare.md", "0b0b0b0b"), ("posts/draft.md", "0d0d0d0d")],
        &[("index.md", "11223344")],
        &["index.md", "posts/a.md", "posts/held.md", "posts/bare.md"],
    );
    let record = land(vault.path(), &sealed).await;

    for (path, uid, body) in [("posts/a.md", "aabbccdd", "Body\r\n"), ("index.md", "11223344", "Hi\n"), ("posts/bare.md", "0b0b0b0b", "Bare\n")] {
        assert_eq!(uid_in(vault.path(), path).as_deref(), Some(uid), "{path}");
        assert!(read(vault.path(), path).ends_with(body), "{path}");
    }
    assert!(read(vault.path(), "posts/a.md").starts_with("---\r\ntitle: A\r\n"));
    assert_eq!(read(vault.path(), "posts/held.md"), "---\nuid: c0ffee00\n---\nHeld\n");
    assert_eq!(read(vault.path(), "posts/draft.md"), "---\ntitle: D\n---\n");

    let now: Vec<(String, String)> = ["index.md", "posts/a.md", "posts/held.md", "posts/bare.md"].iter().map(|p| (p.to_string(), read(vault.path(), p))).collect();
    let now: Vec<(&str, &str)> = now.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
    let next_build = stamped_vault(&now, &[], &[], &["index.md", "posts/a.md", "posts/held.md", "posts/bare.md"]).1;
    let left = crate::build::manifest::change_set::classify(Some(&record), &next_build);
    assert!(left.pages.is_empty(), "the stamp is not a change to publish: {:?}", left.pages);
}

/// A folder page's uid is in the record like an article's, so one whose stamp
/// failed keeps its uid through an edit until a later publish writes it.
#[tokio::test]
async fn a_folder_pages_uid_is_recorded_with_the_articles() {
    let (vault, sealed) = stamped_vault(&[("work/index.md", "Work\n"), ("posts/a.md", "A\n")], &[("posts/a.md", "aabbccdd")], &[("work/index.md", "11223344")], &["work/index.md", "posts/a.md"]);
    land(vault.path(), &sealed).await;
    let Baseline::Present(live) = live_baseline::load(&MossPaths::new(vault.path())) else { panic!("a readable record") };
    let uids = live.uid_by_path();
    assert_eq!(uids.get("work/index.md").map(String::as_str), Some("11223344"));
    assert_eq!(uids.get("posts/a.md").map(String::as_str), Some("aabbccdd"));
}

/// A page edited after its build keeps its bytes, uid-less, and the record
/// keeps the sealed hash, so the edit is still there to publish; the record's
/// uid for its path carries it until a later publish stamps it.
#[tokio::test]
async fn a_page_edited_after_its_build_keeps_its_bytes_until_a_later_publish() {
    let (vault, sealed) = stamped_vault(&[("posts/a.md", "---\ntitle: A\n---\nBody\n")], &[("posts/a.md", "aabbccdd")], &[], &["posts/a.md"]);
    std::fs::write(vault.path().join("posts/a.md"), "---\ntitle: A\n---\nEdited\n").unwrap();
    let record = land(vault.path(), &sealed).await;
    assert_eq!(read(vault.path(), "posts/a.md"), "---\ntitle: A\n---\nEdited\n");
    assert_eq!(record.sources.get("posts/a.md"), Some(&sealed.sources()["posts/a.md"].hash));
    assert_eq!(record.uids.get("posts/a.md").map(String::as_str), Some("aabbccdd"));
}

/// A page deleted since its build is not written back into existence.
#[tokio::test]
async fn a_page_deleted_after_its_build_is_not_brought_back() {
    let (vault, sealed) = stamped_vault(&[("posts/a.md", "---\ntitle: A\n---\n")], &[("posts/a.md", "aabbccdd")], &[], &["posts/a.md"]);
    std::fs::remove_file(vault.path().join("posts/a.md")).unwrap();
    land(vault.path(), &sealed).await;
    assert!(!vault.path().join("posts/a.md").exists());
}

/// A preview rebuild can replace the article map while a generation uploads,
/// under other uids. The publish writes and records the uids of the generation
/// that went live, which its pages' comment threads are keyed by.
#[tokio::test]
async fn a_rebuild_during_the_upload_does_not_change_the_uids_it_writes() {
    let (vault, sealed) = stamped_vault(
        &[("posts/a.md", "---\ntitle: A\n---\nBody\n"), ("work/index.md", "---\ntitle: Work\n---\n")],
        &[("posts/a.md", "aabbccdd")],
        &[("work/index.md", "11223344")],
        &["posts/a.md", "work/index.md"],
    );
    let mp = MossPaths::new(vault.path());
    let rebuilt = br##"{"articles":{"posts/a/":{"source_path":"posts/a.md","title":"A","content":"","url_path":"posts/a/","uid":"99999999"}},"pages":{"work/":"work/index.md"}}"##;
    std::fs::write(mp.article_map(), rebuilt).unwrap();

    let record = land(vault.path(), &sealed).await;

    assert_eq!(uid_in(vault.path(), "posts/a.md").as_deref(), Some("aabbccdd"));
    assert_eq!(uid_in(vault.path(), "work/index.md").as_deref(), Some("11223344"));
    let recorded: std::collections::HashMap<_, _> =
        record.triples.unwrap().into_iter().map(|e| (e.source_path, e.uid)).collect();
    assert_eq!(recorded.get("posts/a.md").map(String::as_str), Some("aabbccdd"));
    assert_eq!(recorded.get("work/index.md").map(String::as_str), Some("11223344"));
}

/// The receipt names the addresses the generation went live at, and the
/// publish then checks those, even when a rebuild during the upload moved a
/// page or added one that did not go out.
#[tokio::test]
async fn a_rebuild_during_the_upload_does_not_change_the_addresses_the_receipt_names() {
    let (vault, first) = stamped_vault(&[("posts/a.md", "---\ntitle: A\n---\n")], &[("posts/a.md", "aabbccdd")], &[], &["posts/a.md"]);
    land(vault.path(), &first).await;
    std::fs::write(vault.path().join("posts/b.md"), "---\ntitle: B\n---\n").unwrap();
    let second = sealed_with_uids(
        &[("posts/a.md", "posts/a/index.html", &first.sources()["posts/a.md"].hash, b"posts/a.md"), ("posts/b.md", "posts/b/index.html", "h-b", b"B")],
        &[("posts/a.md", "aabbccdd"), ("posts/b.md", "bbbbbbbb")],
    );
    let rebuilt = br##"{"articles":{"posts/a/":{"source_path":"posts/a.md","title":"A","content":"","url_path":"posts/a/","uid":"aabbccdd"},"posts/b-moved/":{"source_path":"posts/b.md","title":"B","content":"","url_path":"posts/b-moved/","uid":"bbbbbbbb"},"posts/c/":{"source_path":"posts/c.md","title":"C","content":"","url_path":"posts/c/","uid":"cccccccc"}}}"##;
    std::fs::write(MossPaths::new(vault.path()).article_map(), rebuilt).unwrap();

    let history = tempfile::tempdir().unwrap();
    let history = crate::deploy::history::HistoryStore::at(history.path().to_path_buf());
    let summary = super::record_landed(vault.path(), &second, TARGET, &crate::deploy::one_shot::HeadlessDeployPorts, &history).await;

    let rows: Vec<(&str, crate::deploy::change_record::PageChangeKind)> = summary.records.iter().map(|r| (r.path.as_str(), r.kind)).collect();
    assert_eq!(rows, [("posts/b/", crate::deploy::change_record::PageChangeKind::Added)]);
}

/// A post syndicated to the test channel `notes`, as an article map holds it.
fn syndicated(name: &str, uid: &str, title: &str, words: &str) -> crate::build::scan::article_map::ArticleInfo {
    serde_json::from_value(serde_json::json!({
        "source_path": format!("posts/{name}.md"), "title": title, "content": words, "url_path": format!("posts/{name}/"),
        "date": null, "uid": uid, "syndicate": ["notes"],
    }))
    .unwrap()
}

/// A vault with the test channel `notes`, which posts and updates each copy
/// at an address that spells out the post's title and shaped text.
fn syndicating_vault() -> tempfile::TempDir {
    let vault = tempfile::tempdir().unwrap();
    let spell = "async function (job) { return { success: true, copy: { id: job.post.url, url: 'https://example.social/' + encodeURIComponent(job.post.title + ' ' + JSON.stringify(job.copy)) } }; }";
    crate::syndication::test_channel(vault.path(), "notes", &format!("{{ publish_copy: {spell}, update_copy: {spell} }}"));
    vault
}

/// `posts` (name, uid, title, words) written into the vault and sealed, with
/// the generation's article map kept beside it as materializing it does.
fn seal_posts(root: &std::path::Path, posts: &[(&str, &str, &str, &str)]) -> SealedManifest {
    let mut built = crate::build::scan::article_map::ArticleMap::default();
    let mut pending = PendingManifest::new(SiteHashes::default());
    for (name, uid, title, words) in posts {
        let src = format!("posts/{name}.md");
        let text = format!("---\ntitle: {title}\nuid: {uid}\n---\n{words}\n");
        std::fs::create_dir_all(root.join("posts")).unwrap();
        std::fs::write(root.join(&src), &text).unwrap();
        let out = ServedPath::from_source(&format!("posts/{name}/index.html")).unwrap();
        pending.register(&out, text.as_bytes(), crate::build::manifest::HashBucket::Files);
        pending.register_source_mapping(src.clone(), &out);
        pending.register_page_source_hash(src.clone(), crate::build::render::blocking::source_metadata(None, text.as_bytes()));
        pending.register_page_meta(src, built_under(uid));
        built.articles.insert(format!("posts/{name}/"), syndicated(name, uid, title, words));
    }
    pending.record_articles(std::sync::Arc::new(built));
    let sealed = pending.seal();
    let generations = MossPaths::new(root).generations_dir();
    std::fs::create_dir_all(&generations).unwrap();
    sealed.write_publish_view(&generations).unwrap();
    sealed
}

/// Send the due copies; what became of each.
async fn send_all(root: &std::path::Path) -> Vec<crate::syndication::send::Sent> {
    use crate::syndication::send::{after_live, SendOptions, Sent, PER_RUN};
    let cache = crate::plugins::manager::ManagerCache::headless(crate::build::ports::reporter::discard_owned());
    let emit = |_: &Sent| {};
    let opts = SendOptions { site_url: "https://example.org", retry: &[], emit: &emit, only_source: None, only_channel: None, cap: PER_RUN, check_pages: false };
    after_live(root, &opts, || cache.get_or_create(&root.to_string_lossy())).await
}

/// Send the due copies; the addresses they were posted at.
async fn send_copies(root: &std::path::Path) -> Vec<String> {
    use crate::syndication::send::Outcome;
    send_all(root).await.iter().filter_map(|s| match &s.outcome { Outcome::Posted { url } => Some(url.clone()), _ => None }).collect()
}

/// The latest build's article map, holding post `a` edited after its landing.
fn rebuild_with_an_edit(root: &std::path::Path) {
    let mut rebuilt = crate::build::scan::article_map::ArticleMap::default();
    rebuilt.articles.insert("posts/a/".into(), syndicated("a", "aaaa0001", "Edited", "Edited words."));
    let map = MossPaths::new(root).article_map();
    std::fs::create_dir_all(map.parent().unwrap()).unwrap();
    std::fs::write(map, serde_json::to_vec(&rebuilt).unwrap()).unwrap();
}

/// Post `a` sealed with `own` as its `syndicate:`, its output the same bytes
/// whatever `own` says: choosing a channel changes no page.
fn seal_post_naming(root: &std::path::Path, own: Option<&[&str]>) -> SealedManifest {
    let line = own.map(|c| format!("syndicate: [{}]\n", c.join(", "))).unwrap_or_default();
    let text = format!("---\ntitle: A\nuid: aaaa0001\n{line}---\nWords.\n");
    std::fs::create_dir_all(root.join("posts")).unwrap();
    std::fs::write(root.join("posts/a.md"), &text).unwrap();
    let out = ServedPath::from_source("posts/a/index.html").unwrap();
    let mut pending = PendingManifest::new(SiteHashes::default());
    pending.register(&out, b"<p>Words.</p>", crate::build::manifest::HashBucket::Files);
    pending.register_source_mapping("posts/a.md".into(), &out);
    pending.register_page_source_hash("posts/a.md".into(), crate::build::render::blocking::source_metadata(None, text.as_bytes()));
    pending.register_page_meta("posts/a.md".into(), built_under("aaaa0001"));
    let mut info = syndicated("a", "aaaa0001", "A", "Words.");
    info.syndicate = own.map(|c| c.iter().map(|s| s.to_string()).collect());
    if let Some(c) = own {
        info.frontmatter.insert("syndicate".into(), serde_json::json!(c));
    }
    let mut built = crate::build::scan::article_map::ArticleMap::default();
    built.articles.insert("posts/a/".into(), info);
    pending.record_articles(std::sync::Arc::new(built));
    let sealed = pending.seal();
    let generations = MossPaths::new(root).generations_dir();
    std::fs::create_dir_all(&generations).unwrap();
    sealed.write_publish_view(&generations).unwrap();
    sealed
}

/// Naming a channel on a live post changes no output, so the next publish
/// seals the same generation. Its copy is still planned at that landing and sent.
#[tokio::test]
async fn a_live_post_that_newly_names_a_channel_gets_its_copy_though_no_page_changed() {
    let vault = syndicating_vault();
    let root = vault.path();
    let first = seal_post_naming(root, None);
    land(root, &first).await;
    let records = crate::syndication::record::Records::in_vault(&MossPaths::new(root));
    assert!(!records.exists("aaaa0001", "notes"), "nothing names the channel yet");

    let second = seal_post_naming(root, Some(&["notes"]));
    assert_eq!(second.generation_id(), first.generation_id(), "the test's premise: no output changed");
    land(root, &second).await;

    assert!(records.exists("aaaa0001", "notes"), "the landing plans the copy the post now names");
    assert_eq!(send_copies(root).await.len(), 1);
}

/// A rebuild during the upload can edit a published article and delete
/// another. The copies are planned and sent from the generation that went
/// live: the deleted article still gets its copy, and every copy carries the
/// text that was published, not the rebuild's.
#[tokio::test]
async fn a_rebuild_during_the_upload_does_not_change_what_the_copies_carry() {
    let vault = syndicating_vault();
    let root = vault.path();
    let sealed = seal_posts(root, &[("a", "aaaa0001", "Sealed", "Sealed words."), ("gone", "aaaa0002", "Sealed too", "Sealed words.")]);
    rebuild_with_an_edit(root);

    land(root, &sealed).await;
    let records = crate::syndication::record::Records::in_vault(&MossPaths::new(root));
    assert!(records.exists("aaaa0002", "notes"), "the article the rebuild deleted is still planned a copy");

    let posted = send_copies(root).await;
    assert_eq!(posted.len(), 2, "{posted:?}");
    assert!(posted.iter().all(|url| url.contains("Sealed") && !url.contains("Edited")), "{posted:?}");
}

/// A copy carries the post as the landing that made it due published it, even
/// when another target's landing published an edit after that.
#[tokio::test]
async fn a_later_landing_on_another_target_does_not_change_a_due_copys_text() {
    let vault = syndicating_vault();
    let root = vault.path();
    land(root, &seal_posts(root, &[("a", "aaaa0001", "Sealed", "Sealed words.")])).await;
    let edited = seal_posts(root, &[("a", "aaaa0001", "Edited", "Edited words.")]);
    let history = tempfile::tempdir().unwrap();
    let history = crate::deploy::history::HistoryStore::at(history.path().to_path_buf());
    super::record_landed(root, &edited, "onionpress:other", &crate::deploy::one_shot::HeadlessDeployPorts, &history).await;

    let posted = send_copies(root).await;
    assert_eq!(posted.len(), 1, "{posted:?}");
    assert!(posted[0].contains("Sealed") && !posted[0].contains("Edited"), "{posted:?}");
}

/// A copy that waits to be sent while later builds age its generation, and
/// that generation's article map, out of retention still carries the text its
/// landing published, and drops it once sent.
#[tokio::test]
async fn a_copy_that_waits_past_retention_still_carries_its_published_text() {
    let vault = syndicating_vault();
    let root = vault.path();
    let mp = MossPaths::new(root);
    let generations = mp.generations_dir();
    let landed = seal_posts(root, &[("a", "aaaa0001", "Sealed", "Sealed words.")]);
    land(root, &landed).await;
    let mut built = vec![landed.generation_id().to_string()];
    for n in 1..=4 {
        built.push(seal_posts(root, &[("a", "aaaa0001", "Edited", &format!("Edited words {n}."))]).generation_id().to_string());
    }
    rebuild_with_an_edit(root);
    for (age, id) in built.iter().enumerate() {
        std::fs::create_dir_all(generations.join(id)).unwrap();
        filetime::set_file_mtime(generations.join(id), filetime::FileTime::from_unix_time(1_000_000 + age as i64, 0)).unwrap();
    }
    crate::build::collect_build_store(&mp, built.last().unwrap(), &std::collections::HashSet::new());
    assert!(!crate::build::manifest::publish_view::articles_path(&generations, &built[0]).exists(), "the landed generation's map aged out");

    let posted = send_copies(root).await;
    assert_eq!(posted.len(), 1, "{posted:?}");
    assert!(posted[0].contains("Sealed") && !posted[0].contains("Edited"), "{posted:?}");
    let record = crate::syndication::record::Records::in_vault(&mp).get("aaaa0001", "notes").unwrap();
    assert_eq!((record.state, record.post), (crate::syndication::record::State::Posted, None), "a sent copy keeps no post");
}

/// Copy records travel with the folder and build stores do not. A copy made
/// due on one computer and sent from another, which never built the landed
/// generation and whose own build has an edit, carries the published text.
#[tokio::test]
async fn a_due_copy_synced_to_a_computer_that_never_built_its_publish_sends_the_published_text() {
    let publisher = syndicating_vault();
    land(publisher.path(), &seal_posts(publisher.path(), &[("a", "aaaa0001", "Sealed", "Sealed words.")])).await;
    let other = syndicating_vault();
    let copies = |root: &std::path::Path| MossPaths::new(root).deploy_dir().join("copies");
    std::fs::create_dir_all(copies(other.path())).unwrap();
    for entry in std::fs::read_dir(copies(publisher.path())).unwrap().flatten() {
        std::fs::copy(entry.path(), copies(other.path()).join(entry.file_name())).unwrap();
    }
    rebuild_with_an_edit(other.path());

    let posted = send_copies(other.path()).await;
    assert_eq!(posted.len(), 1, "{posted:?}");
    assert!(posted[0].contains("Sealed") && !posted[0].contains("Edited"), "{posted:?}");
}

/// An update carries the post as the landing that marked it published it,
/// even when a later build edits the post again.
#[tokio::test]
async fn an_update_carries_the_text_its_landing_published() {
    use crate::syndication::send::Outcome;
    let vault = syndicating_vault();
    let root = vault.path();
    land(root, &seal_posts(root, &[("a", "aaaa0001", "Sealed", "Sealed words.")])).await;
    assert_eq!(send_copies(root).await.len(), 1);
    land(root, &seal_posts(root, &[("a", "aaaa0001", "Revised", "Revised words.")])).await;
    rebuild_with_an_edit(root);

    let sent = send_all(root).await;
    let updated: Vec<&String> = sent.iter().filter_map(|s| match &s.outcome { Outcome::Updated { url } => Some(url), _ => None }).collect();
    assert_eq!(updated.len(), 1, "{:?}", sent.iter().map(|s| &s.outcome).collect::<Vec<_>>());
    assert!(updated[0].contains("Revised") && !updated[0].contains("Edited"), "{updated:?}");
}
