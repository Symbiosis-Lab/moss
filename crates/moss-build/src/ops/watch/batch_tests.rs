use super::*;

use notify::event::{MetadataKind, ModifyKind, RenameMode};
use notify::EventKind;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::Poll;

fn recording_relay() -> (EventRelay, tokio::sync::mpsc::UnboundedReceiver<MossEvent>) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let emit: EventRelay = Arc::new(move |event| {
        if matches!(event, MossEvent::SourceContentChanged { .. } | MossEvent::FileChanged(_)) {
            tx.send(event).unwrap();
        }
    });
    (emit, rx)
}

fn event(kind: EventKind, paths: Vec<PathBuf>) -> DebouncedEvent {
    let mut event = notify::Event::new(kind);
    event.paths = paths;
    DebouncedEvent::new(event, std::time::Instant::now())
}

fn assert_source_only(event: MossEvent, paths: &[&str]) {
    assert!(!matches!(event, MossEvent::FileChanged(_)), "an announcement is not a build result");
    let MossEvent::SourceContentChanged { paths: actual } = event else {
        panic!("expected a source announcement, got {event:?}");
    };
    assert_eq!(actual, paths.iter().map(|p| p.to_string()).collect::<Vec<_>>());
}

async fn announce_during_rebuild(kind: EventKind, renamed: bool) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let source = root.join("P.md");
    std::fs::write(&source, "# Changed\n").unwrap();
    let paths = if renamed {
        vec![root.join(".save.tmp"), source]
    } else {
        vec![source]
    };
    let (emit, mut events) = recording_relay();
    let release = Arc::new(tokio::sync::Notify::new());
    let started = Arc::new(AtomicBool::new(false));
    let dispatch: RebuildDispatch = {
        let (release, started) = (release.clone(), started.clone());
        Arc::new(move |_| {
            let (release, started) = (release.clone(), started.clone());
            Box::pin(async move {
                started.store(true, Ordering::SeqCst);
                release.notified().await;
            })
        })
    };
    let batch = handle_debounced_batch(vec![event(kind, paths)], root.to_str().unwrap(), &emit, &dispatch);
    tokio::pin!(batch);
    assert!(matches!(futures::poll!(batch.as_mut()), Poll::Pending));
    assert!(started.load(Ordering::SeqCst), "the rebuild is awaiting release");
    assert_source_only(events.try_recv().expect("source announced while rebuild is unresolved"), &["P.md"]);
    assert!(events.try_recv().is_err(), "one source event per batch");

    release.notify_one();
    batch.await;
    assert!(events.try_recv().is_err(), "the batch does not announce again on completion");
}

#[tokio::test]
async fn a_changed_source_is_announced_before_its_rebuild_finishes() {
    announce_during_rebuild(EventKind::Modify(ModifyKind::Data(notify::event::DataChange::Content)), false).await;
}

#[tokio::test]
async fn an_atomic_save_destination_is_announced_before_its_rebuild_finishes() {
    announce_during_rebuild(EventKind::Modify(ModifyKind::Name(RenameMode::Both)), true).await;
}

#[tokio::test]
async fn a_source_is_announced_even_when_the_content_hash_gate_suppresses_rebuilding() {
    use sha2::{Digest, Sha256};

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let folder = root.to_str().unwrap();
    let source = root.join("P.md");
    let bytes = b"# Unchanged\n";
    std::fs::write(&source, bytes).unwrap();
    let mut baseline = crate::types::content::SiteHashes::new();
    baseline.sources.insert("P.md".into(), crate::build::types::SourceMetadata {
        hash: format!("{:x}", Sha256::digest(bytes)),
        stat: crate::build::stat::FileStat::whole_second(bytes.len() as u64, 0),
    });
    crate::system::build_records::records().record_content_hashes(folder, baseline);
    let (emit, mut events) = recording_relay();
    let dispatch: RebuildDispatch = {
        let folder = folder.to_string();
        Arc::new(move |request| {
            let folder = folder.clone();
            Box::pin(async move {
                assert!(request.gate_paths.is_some());
                assert!(!request.gate_admits(&folder, "test").await);
            })
        })
    };
    handle_debounced_batch(
        vec![event(EventKind::Modify(ModifyKind::Any), vec![source])], folder, &emit, &dispatch,
    ).await;
    crate::system::build_records::records().forget_content_hashes(folder);
    assert_source_only(events.try_recv().unwrap(), &["P.md"]);
    assert!(events.try_recv().is_err());
}

#[tokio::test]
async fn source_announcements_respect_watch_scope_and_ignore_read_noise() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let (emit, mut events) = recording_relay();
    let dispatch: RebuildDispatch = Arc::new(|_| Box::pin(async {}));
    let paths = [
        "P.md", "P.md", "footer.md", ".moss/config.toml", ".moss/places.toml",
        ".moss/theme/style.css", ".moss/theme/script.js", "assets/site.css", "photo.jpg",
        "AGENTS.md", ".git/hidden.md", "node_modules/hidden.md", ".moss/build.nosync/staging/hidden.md",
    ].iter().map(|p| root.join(p)).chain(std::iter::once(root.join("../outside.md"))).collect();
    handle_debounced_batch(
        vec![event(EventKind::Modify(ModifyKind::Any), paths)],
        root.to_str().unwrap(), &emit, &dispatch,
    ).await;
    assert_source_only(events.try_recv().unwrap(), &[
        ".moss/config.toml", ".moss/places.toml", ".moss/theme/script.js",
        ".moss/theme/style.css", "P.md", "footer.md",
    ]);
    let ignored = [
        EventKind::Access(notify::event::AccessKind::Any),
        EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime)),
        EventKind::Remove(notify::event::RemoveKind::File),
        EventKind::Modify(ModifyKind::Name(RenameMode::From)),
    ].into_iter().map(|kind| event(kind, vec![root.join("P.md")])).collect();
    handle_debounced_batch(ignored, root.to_str().unwrap(), &emit, &dispatch).await;
    handle_debounced_batch(
        vec![event(EventKind::Modify(ModifyKind::Any), vec![root.join("photo.jpg"), root.join("assets/site.css")])],
        root.to_str().unwrap(), &emit, &dispatch,
    ).await;
    assert!(events.try_recv().is_err());
}
