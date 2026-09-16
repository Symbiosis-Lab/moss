use super::*;
use crate::build::assets::paths::compute_binary_hash;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::served_path::ServedPath;
use crate::types::content::SiteHashes;

/// A project root under `target/test-tmp` with `state.toml` pointing at
/// `gen_id`. Mirrors the real layout: the pointer lives in the machine-managed
/// state file under the active target's record (`site_id` alone selects moss
/// hosting, so `moss:site-1` is the slot the flat view reads), the tree under
/// `.moss/build/generations/<id>/`.
fn project_with_pointer(gen_id: &str) -> (tempfile::TempDir, MossPaths) {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
    std::fs::create_dir_all(&base).expect("create target/test-tmp");
    let dir = tempfile::TempDir::new_in(&base).unwrap();
    let moss = dir.path().join(".moss");
    std::fs::create_dir_all(&moss).unwrap();
    std::fs::write(
        moss.join("state.toml"),
        format!(
            "[deployment]\nsite_id = \"site-1\"\n\
             [deployment.targets.\"moss:site-1\"]\n\
             last_deployed_generation_id = \"{gen_id}\"\n"
        ),
    )
    .unwrap();
    let mp = MossPaths::new(dir.path());
    (dir, mp)
}

fn write_generation(mp: &MossPaths, gen_id: &str, files: &[(&str, &[u8])]) {
    let gen = mp.generation_dir(gen_id);
    for (rel, bytes) in files {
        let path = gen.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
    }
}

fn sealed_of(files: &[(&str, &[u8])]) -> SealedManifest {
    let mut pending = PendingManifest::new(SiteHashes::default());
    for (rel, bytes) in files {
        pending.register(&ServedPath::from_source(rel).unwrap(), bytes, HashBucket::Files);
    }
    pending.seal()
}

/// The whole feasibility claim of moss#993's local backfill, and the
/// non-obvious half of it: the manifest entry for a page is in the GENERATION
/// domain, not staging's. Staging carries `data-source-*` annotations that
/// `ship_phase` strips on the way to the generation directory, and the manifest
/// agrees only because `emit::slots` re-registers each page it injects with the
/// `apply_transform`-ed bytes.
///
/// So this runs the real ship path over a real annotated page and re-hashes
/// what lands. Registering the same bytes it wrote would prove only that the
/// `<mode>:<hash>` string round-trips — which is not the claim at risk. If the
/// domains ever drift, every page reads as changed and the count silently
/// becomes "everything".
#[test]
fn a_manifest_entry_hashes_the_shipped_page_not_the_staged_one() {
    use crate::build::ship::{apply_transform, ship_phase, transform_for};

    let (dir, mp) = project_with_pointer("gen-shipped");
    let staged: &[u8] = br#"<p data-source-line="3">home</p>"#;
    let stage_dir = dir.path().join(".moss/build/staging");
    std::fs::create_dir_all(&stage_dir).unwrap();
    std::fs::write(stage_dir.join("index.html"), staged).unwrap();

    // The manifest as a build actually builds it: the render registers the
    // staged bytes, then slot injection re-registers the shipped ones.
    let mut pending = PendingManifest::new(SiteHashes::default());
    let served = ServedPath::from_source("index.html").unwrap();
    pending.register(&served, staged, HashBucket::Files);
    let shipped = apply_transform(transform_for("index.html"), staged);
    pending.register(&served, &shipped, HashBucket::Files);
    let sealed = pending.seal();

    ship_phase(&stage_dir, &mp.generation_dir("gen-shipped"), &sealed, None)
        .expect("ship_phase must materialize the generation");
    let files = published_files(&mp).expect("generation on disk must reconstruct");

    assert_eq!(&files, sealed.files());
    assert_ne!(
        sealed.files().get("index.html"),
        Some(&file_entry(&compute_binary_hash(staged))),
        "the test is vacuous unless the transform actually changed the bytes"
    );
}

/// The memo's contract: a hit is by generation DIRECTORY, the single slot means
/// a second generation evicts the first, and a FAILED walk is remembered as
/// such — otherwise a partially-evicted generation re-walks and re-fails on
/// every seal, which on a watch loop is every keystroke.
#[test]
fn the_memo_remembers_failure_too_and_one_generation_evicts_the_other() {
    let memo = Memo::default();
    let a = std::path::PathBuf::from("/gen/a");
    let b = std::path::PathBuf::from("/gen/b");
    let files: FileSet = [("index.html".to_string(), "100644:abc".to_string())].into();

    assert!(memo.get(&a).is_none(), "an empty memo answers for nothing");

    memo.put(&a, &Some(files.clone()));
    assert_eq!(memo.get(&a), Some(Some(files)));
    assert!(memo.get(&b).is_none(), "a hit is keyed by directory, never unconditional");

    memo.put(&b, &None);
    assert_eq!(
        memo.get(&b),
        Some(None),
        "a failed walk is an answer — 'tried and could not', not 'never tried'"
    );
    assert!(
        memo.get(&a).is_none(),
        "one slot: the second generation evicts the first, it does not join it"
    );
    assert!(
        memo.get(&std::path::PathBuf::from("/gen/c")).is_none(),
        "and a remembered failure is scoped to ITS generation — the next id, \
         which the next publish mints, is unanswered and gets a fresh walk"
    );
}

/// A remembered failure has to expire. The only thing that makes a walk fail is
/// an eviction, and the provider is usually already fetching the files — so a
/// permanent negative entry would keep the button blank for the rest of the
/// session over a condition that cleared seconds later.
#[test]
fn a_remembered_failure_expires_so_the_answer_can_come_back() {
    let memo = Memo::default();
    let dir = std::path::PathBuf::from("/gen/evicted");

    memo.put(&dir, &None);
    assert_eq!(memo.get(&dir), Some(None), "fresh failure still counts as tried");

    // Age it past the retry window without sleeping through it.
    {
        let mut guard = memo.slot.lock().unwrap();
        let (k, v, failed_at) = guard.take().unwrap();
        let aged = failed_at.unwrap() - FAILURE_RETRY_AFTER;
        *guard = Some((k, v, Some(aged)));
    }
    assert!(
        memo.get(&dir).is_none(),
        "a stale failure reads as a miss, so the next seal walks the tree the \
         provider has since filled in"
    );

    // A SUCCESS never expires: nothing about a hashed tree goes off.
    let files: FileSet = [("index.html".to_string(), "100644:abc".to_string())].into();
    memo.put(&dir, &Some(files.clone()));
    assert_eq!(memo.get(&dir), Some(Some(files)));
}

/// An evicted or pruned generation must be walked once, not once per seal. The
/// walk is a whole tree of reads and a seal happens on every rebuild, so a
/// success-only memo turns one iCloud eviction into a per-keystroke tax that
/// produces nothing.
#[test]
fn a_failed_walk_is_remembered_rather_than_retried_every_seal() {
    let (_dir, mp) = project_with_pointer("gen-evicted"); // pointer set, no tree
    let memo = Memo::default();

    assert!(published_files_memoized(&memo, &mp).is_none());
    assert_eq!(
        memo.get(&mp.generation_dir("gen-evicted")),
        Some(None),
        "the failure must be recorded, or the next seal re-walks the same tree"
    );
    assert!(
        published_files_memoized(&memo, &mp).is_none(),
        "and the recorded failure still answers, without a second walk"
    );
}

#[cfg(unix)]
#[test]
fn a_symlink_hashes_its_target_string_not_the_bytes_behind_it() {
    let (_dir, mp) = project_with_pointer("gen-link");
    write_generation(&mp, "gen-link", &[("real/index.html", b"<p>real</p>")]);
    let gen = mp.generation_dir("gen-link");
    std::os::unix::fs::symlink("../real/index.html", gen.join("alias.html")).unwrap();

    let files = published_files(&mp).expect("generation on disk must reconstruct");

    assert_eq!(files.get("alias.html"), Some(&symlink_entry("../real/index.html")));
    assert_eq!(
        files.get("real/index.html"),
        Some(&file_entry(&compute_binary_hash(b"<p>real</p>"))),
        "a symlink must not perturb the regular file it points at"
    );
}

/// The defect moss#993 #2 names: with no `last-published.json`, the resting
/// answer used to be blank AND zero. It is now a real file-level count — and
/// still carries no verb, because a file set cannot justify one.
#[tokio::test]
async fn with_no_publish_record_a_seal_reports_the_backfilled_count() {
    let (_dir, mp) = project_with_pointer("gen-b");
    write_generation(
        &mp,
        "gen-b",
        &[("index.html", b"old home"), ("gone/index.html", b"removed page")],
    );

    let current = sealed_of(&[("index.html", b"new home"), ("fresh/index.html", b"added page")]);
    let set = match for_seal(&mp, Some(&current)).await {
        Some(SealVerdict::Ready(set)) => set,
        _ => panic!("a materialized seal with a generation on disk answers locally"),
    };

    assert!(!set.classified, "a file set can never justify a verb");
    assert_eq!(set.flat_upload, 2, "one changed page plus one new page");
    assert_eq!(set.flat_remove, 1);
    assert_eq!((set.added, set.edited, set.deleted, set.restyled), (0, 0, 0, 0));
}

/// Generations are collectable, and a site published only from another machine
/// has no pointer at all — the residual gap moss#993 documents. Both surface as
/// `AskServer`, and a caller with no port renders that as the blank default —
/// unclassified and zero, exactly where they landed before the backfill
/// existed. Never a partial count.
#[tokio::test]
async fn a_pruned_or_absent_generation_degrades_to_ask_server_then_blank() {
    let (_pruned, pruned_mp) = project_with_pointer("gen-collected");
    let (_none, no_pointer_mp) = project_with_pointer("gen-x");
    std::fs::remove_file(no_pointer_mp.project_root().join(".moss/state.toml")).unwrap();
    let current = sealed_of(&[("index.html", b"home")]);

    for mp in [&pruned_mp, &no_pointer_mp] {
        assert!(published_files(mp).is_none());
        assert!(
            matches!(for_seal(mp, Some(&current)).await, Some(SealVerdict::AskServer)),
            "both local sources empty defers to the server, never a partial count"
        );
    }
}

/// The record is PREFERRED over the backfill, and it is the only input that can
/// carry a verb. With one on disk the tree is not walked at all — note the
/// pointed-at generation here holds bytes that would produce different flat
/// counts, so a backfill answer would be visibly wrong.
#[tokio::test]
async fn a_publish_record_wins_over_the_generation_on_disk() {
    let (_dir, mp) = project_with_pointer("gen-record");
    write_generation(&mp, "gen-record", &[("index.html", b"unrelated"), ("x.html", b"x")]);
    let published = sealed_of(&[("index.html", b"old home")]);
    published_record::save(
        &mp,
        &change_set::PublishedSnapshot::from_sealed(
            &published,
            "moss:site-1",
            "2026-01-01T00:00:00Z".into(),
        ),
    )
    .unwrap();

    let current = sealed_of(&[("index.html", b"new home")]);
    let set = match for_seal(&mp, Some(&current)).await {
        Some(SealVerdict::Ready(set)) => set,
        _ => panic!("a record on disk answers locally"),
    };

    assert!(set.classified, "a record carries authorship; only a file set does not");
    assert_eq!(
        (set.flat_upload, set.flat_remove),
        (0, 0),
        "flat counts are the backfill's signature — the generation tree was not consulted"
    );
}

/// The server's own diff is the LAST resort — `for_seal` defers to it only
/// when there is no publish record and no generation on disk (the re-cloned
/// vault whose `state.toml` was gitignored), and `from_server` asks at most
/// once per sealed generation: a seal fires on every keystroke-driven
/// rebuild, and this arm costs a network round trip. The counts pass through
/// untouched and stay verbless.
///
/// Uses the process-global `SERVER_MEMO` deliberately (no other test calls
/// `from_server`, so nothing else writes it): the claim under test is that
/// the REAL path memoizes, not that some injected cache would.
#[tokio::test]
async fn the_server_diff_is_the_last_resort_and_is_asked_once_per_generation() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let (_dir, mp) = project_with_pointer("gen-remote"); // pointer set, tree pruned
    let current = sealed_of(&[("remote.html", b"only the server knows")]);
    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let port: ServerDiff = {
        let calls = calls.clone();
        std::sync::Arc::new(move |files, _gen| {
            calls.fetch_add(1, Ordering::SeqCst);
            assert!(files.contains_key("remote.html"), "the port gets the wire manifest");
            Box::pin(async { Some((3usize, 1usize)) })
        })
    };

    assert!(
        matches!(for_seal(&mp, Some(&current)).await, Some(SealVerdict::AskServer)),
        "no record and no tree defers to the server"
    );

    let gen = "gen-remote-ask".to_string();
    let set = from_server(&port, current.files().clone(), gen.clone()).await;
    assert!(!set.classified, "a server count can never justify a verb");
    assert_eq!((set.flat_upload, set.flat_remove), (3, 1), "counts pass through untouched");

    let again = from_server(&port, current.files().clone(), gen).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1, "one ask per sealed generation, not per seal");
    assert_eq!((again.flat_upload, again.flat_remove), (3, 1));
}

/// The step-5b contract (moss#993 4a, and the superseded-tail defect beside it).
/// Both refusals produce the same `None`, and `None` is "say nothing" — the
/// caller leaves the stash standing, because the manifest it describes stands.
#[tokio::test]
async fn a_tail_that_did_not_promote_speaks_for_nothing() {
    let (_dir, mp) = project_with_pointer("gen-b");
    write_generation(&mp, "gen-b", &[("index.html", b"home")]);
    let sealed = sealed_of(&[("index.html", b"home")]);

    assert!(tail_speaks(&sealed, false, true).is_none(), "a failed materialize");
    assert!(tail_speaks(&sealed, true, false).is_none(), "a superseded tail");
    assert!(tail_speaks(&sealed, true, true).is_some(), "the promoting tail does speak");

    assert!(for_seal(&mp, None).await.is_none(), "and a silent tail yields no change set");
}
