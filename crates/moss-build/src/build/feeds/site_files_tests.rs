use super::*;
use crate::build::background::BackgroundHandle;
use crate::build::manifest::{SealedManifest, RenderOutcome};
use crate::moss_paths::MossPaths;
use crate::types::content::SiteHashes;

fn declared(folder: &Path) -> SiteFiles {
    let mut files = SiteFiles::new(
        folder,
        "https://example.org",
        crate::i18n::Language::En,
        &HashMap::new(),
    );
    files.feeds();
    files.sitemap();
    files.list_in_sitemap(BTreeSet::from(["index.html".to_string()]));
    files.keep_copy(ServedPath::from_source("feed.xml").unwrap(), "rss.xml");
    files
}

fn promised(files: &SiteFiles) -> PendingManifest {
    let mut pending = PendingManifest::new(SiteHashes::default());
    files.promise(&mut pending);
    pending
}

/// Seal a render-phase manifest that promised `files`, with or without the worker.
async fn seal(files: SiteFiles, output: &Path, render: Option<crate::build::lifecycle::Render>) -> SealedManifest {
    let pending = promised(&files);
    let output = output.to_path_buf();
    let handle = BackgroundHandle::spawn_with_pending(pending, move |tx, workers| {
        if let Some(render) = render {
            let job = files.after_paint(&Arc::new(Vec::new()), &output, None);
            job.expect("files are declared").spawn(workers, tx, render);
        }
    });
    handle.await_completion().await.expect("the background phase seals").0
}

#[tokio::test]
async fn the_worker_keeps_every_promise_of_the_declaration() {
    let dir = tempfile::tempdir().unwrap();
    let mp = MossPaths::new(dir.path());
    let output = dir.path().join("stage");
    let (render, _) = crate::build::lifecycle::show_render(&mp, false);
    let sealed = seal(declared(dir.path()), &output, Some(render)).await;
    assert!(sealed.unverified().is_empty(), "{:?}", sealed.unverified());
    for key in ["rss.xml", "feed.json", "sitemap.xml", "feed.xml"] {
        assert!(sealed.files().contains_key(key), "{key} is not in the generation");
        assert!(output.join(key).is_file(), "{key} is not in the stage");
    }
    assert_eq!(std::fs::read(output.join("feed.xml")).unwrap(), std::fs::read(output.join("rss.xml")).unwrap());
}

#[tokio::test]
async fn a_generation_without_the_worker_is_withheld_for_its_promises() {
    let dir = tempfile::tempdir().unwrap();
    let sealed = seal(declared(dir.path()), &dir.path().join("stage"), None).await;
    let unkept: Vec<&str> = sealed.unverified().keys().map(String::as_str).collect();
    assert_eq!(unkept, ["feed.json", "feed.xml", "rss.xml", "sitemap.xml"]);
}

/// The folder's own file is copied in by the asset walk; generating one too
/// would race it for the same address.
#[tokio::test]
async fn a_file_the_folder_provides_is_neither_generated_nor_promised() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("llms.txt"), "the author's own").unwrap();
    let mut files = declared(dir.path());
    files.llms();
    assert!(!files.addresses().any(|a| a == "llms.txt"));
    let sealed = seal(files, &dir.path().join("stage"), None).await;
    assert!(!sealed.unverified().contains_key("llms.txt"));
    assert!(sealed.unverified().contains_key("rss.xml"), "the rest is still promised");
}

#[tokio::test]
async fn a_worker_whose_render_was_superseded_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mp = MossPaths::new(dir.path());
    let output = dir.path().join("stage");
    let (older, _) = crate::build::lifecycle::show_render(&mp, false);
    // A save lands while the older render's worker is still running.
    let (_newer, _) = crate::build::lifecycle::show_render(&mp, false);
    let sealed = seal(declared(dir.path()), &output, Some(older)).await;
    assert_eq!(sealed.outcome(), RenderOutcome::Superseded, "the older render must not be promoted");
    assert!(sealed.unverified().contains_key("rss.xml"), "a tail that seals it anyway must withhold it");
    assert!(!output.join("rss.xml").exists(), "a superseded worker wrote into the stage");
    assert!(!sealed.files().contains_key("rss.xml"));
}

/// The next save's build holds the stage write lock through its render and is
/// shown before it lets go. A worker that asked "am I newest?" before taking
/// the lock would answer yes and then write over that build's stage.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_newest_check_is_made_under_the_stage_write_lock() {
    let dir = tempfile::tempdir().unwrap();
    let mp = MossPaths::new(dir.path());
    let output = dir.path().join("stage");
    let session = FolderSession::new(dir.path().to_path_buf());
    let (older, _) = crate::build::lifecycle::show_render(&mp, false);
    let next_build = session.lock_stage_write().await;
    let files = declared(dir.path());
    let pending = promised(&files);
    let job = files.after_paint(&Arc::new(Vec::new()), &output, Some(session.clone()));
    let handle = BackgroundHandle::spawn_with_pending(pending, move |tx, workers| {
        job.expect("files are declared").spawn(workers, tx, older);
    });
    // Time for the worker to make its bytes and reach the lock; only the
    // ordering of a worker that checks too early depends on it.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(session.has_ui_bound(), "a publish must wait for the worker");
    let (_newer, _) = crate::build::lifecycle::show_render(&mp, false);
    drop(next_build);
    let sealed = handle.await_completion().await.expect("the background phase seals").0;
    assert_eq!(sealed.outcome(), RenderOutcome::Superseded);
    assert!(!output.join("rss.xml").exists(), "the older worker wrote over the newer build's stage");
}
