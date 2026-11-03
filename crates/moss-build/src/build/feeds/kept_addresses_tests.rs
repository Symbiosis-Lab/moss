use super::keep;
use crate::build::manifest::change_set::{PublishedSnapshot, RemovalReason};
use crate::build::manifest::{backfill, published_record, HashBucket, PendingManifest, SealedManifest};
use crate::build::served_path::ServedPath;
use crate::build::types::SourceMetadata;
use crate::moss_paths::MossPaths;
use crate::types::content::SiteHashes;
use std::path::Path;

fn register(pending: &mut PendingManifest, stage: &Path, path: &str, bytes: &[u8]) {
    let full = stage.join(path);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(&full, bytes).unwrap();
    let sp = ServedPath::from_cached(path).unwrap();
    pending.register(&sp, bytes, HashBucket::Files);
}

/// The last publish: a picture and the derivative a page's embed produced.
fn publish_with_derivative(mp: &MossPaths) {
    let mut pending = PendingManifest::new(SiteHashes::default());
    for (path, bytes) in [("photo-2.jpg", &b"JPEG"[..]), ("photo-2.webp", &b"WEBP"[..])] {
        pending.register(&ServedPath::from_source(path).unwrap(), bytes, HashBucket::Files);
    }
    let meta = SourceMetadata { hash: "h".into(), size: 1, mtime: 1, mtime_nanos: None, ctime: None, inode: None };
    pending.register_page_source_hash("photo-2.jpg".into(), meta);
    let snap = PublishedSnapshot::from_sealed(&pending.seal(), "moss:site-1", "2026-08-05T00:00:00Z".into());
    published_record::save(mp, &snap).unwrap();
}

/// This build: the redesign no longer embeds the picture.
fn build(stage: &Path, extra: &[(&str, &[u8])], keep_picture: bool) -> SealedManifest {
    let mut pending = PendingManifest::new(SiteHashes::default());
    if keep_picture {
        register(&mut pending, stage, "photo-2.jpg", b"JPEG");
    }
    for (path, bytes) in extra {
        register(&mut pending, stage, path, bytes);
    }
    pending.seal()
}

fn setup() -> (tempfile::TempDir, MossPaths) {
    let vault = tempfile::tempdir().unwrap();
    let mp = MossPaths::new(vault.path());
    publish_with_derivative(&mp);
    (vault, mp)
}

#[test]
fn a_dropped_derivative_of_a_served_picture_keeps_answering_and_the_removal_check_passes() {
    let (_vault, mp) = setup();
    let stage = mp.staging_dir();
    let mut sealed = build(&stage, &[], true);
    assert_eq!(backfill::removed_for_seal(&mp, &sealed).len(), 1, "without the redirect the publish is refused");

    let kept = keep(&mp, &stage, &mut sealed, &[]);

    assert_eq!(kept, [("photo-2.webp".to_string(), "photo-2.jpg".to_string())]);
    assert_eq!(std::fs::read(stage.join("photo-2.webp")).unwrap(), b"JPEG");
    assert!(sealed.files().contains_key("photo-2.webp"));
    assert!(backfill::removed_for_seal(&mp, &sealed).is_empty());
}

#[test]
fn an_address_the_build_already_serves_is_left_alone() {
    let (_vault, mp) = setup();
    let stage = mp.staging_dir();
    let mut sealed = build(&stage, &[("photo-2.webp", b"AUTHORS")], true);

    assert!(keep(&mp, &stage, &mut sealed, &[]).is_empty());
    assert_eq!(std::fs::read(stage.join("photo-2.webp")).unwrap(), b"AUTHORS");
}

#[test]
fn a_derivative_of_a_deleted_picture_is_not_kept_and_stays_the_authors_removal() {
    let (_vault, mp) = setup();
    let stage = mp.staging_dir();
    let mut sealed = build(&stage, &[], false);

    assert!(keep(&mp, &stage, &mut sealed, &[]).is_empty());
    assert!(!stage.join("photo-2.webp").exists());
    let reasons: Vec<_> = backfill::removed_for_seal(&mp, &sealed).into_iter().map(|r| (r.path, r.reason)).collect();
    assert_eq!(
        reasons,
        [
            ("photo-2.jpg".to_string(), RemovalReason::AuthorRemoved),
            ("photo-2.webp".to_string(), RemovalReason::AuthorRemoved),
        ]
    );
}

fn served(paths: &[&str]) -> std::collections::HashMap<String, String> {
    paths.iter().map(|p| (p.to_string(), "h".to_string())).collect()
}

fn mv(from: &str, to: &str) -> super::FolderMove {
    super::FolderMove { from: from.into(), to: to.into(), old_served: false }
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
}

#[test]
fn a_moved_folder_keeps_each_served_file_at_its_old_address() {
    let files = served(&["review/client/old/index.html", "review/client/old/a/b.png", "review/client/other/x.pdf", "_moss/og/1.png"]);
    assert_eq!(
        super::moved_folder_files(&[mv("review/old/", "review/client/old/")], &files),
        pairs(&[("review/old/a/b.png", "review/client/old/a/b.png"), ("review/old/index.html", "review/client/old/index.html")])
    );
}

#[test]
fn an_old_address_the_build_still_serves_is_not_kept() {
    let files = served(&["new/a.pdf", "new/b.pdf", "old/a.pdf"]);
    assert_eq!(super::moved_folder_files(&[mv("old/", "new/")], &files), pairs(&[("old/b.pdf", "new/b.pdf")]));
}

/// Two lines reaching one old address: the line for the deeper folder wins.
#[test]
fn the_most_specific_move_wins_an_address_two_moves_reach() {
    let files = served(&["x/sub/f.pdf", "y/f.pdf"]);
    let kept = super::moved_folder_files(&[mv("a/", "x/"), mv("a/sub/", "y/")], &files);
    assert_eq!(kept, pairs(&[("a/sub/f.pdf", "y/f.pdf")]));
}

/// Into its own subfolder: every file of the subfolder answers one level up.
#[test]
fn a_folder_moved_into_its_own_subfolder_answers_one_level_up() {
    let files = served(&["review/2026/plan.pdf", "review/index.html"]);
    assert_eq!(super::moved_folder_files(&[mv("review/", "review/2026/")], &files), pairs(&[("review/plan.pdf", "review/2026/plan.pdf")]));
}

/// Up into its parent: the parent's files answer under the old folder, but
/// nothing already under the old folder is copied into it again.
#[test]
fn a_folder_moved_up_into_its_parent_does_not_nest_copies_of_itself() {
    let files = served(&["review/plan.pdf", "review/2026/index.html"]);
    assert_eq!(super::moved_folder_files(&[mv("review/2026/", "review/")], &files), pairs(&[("review/2026/plan.pdf", "review/plan.pdf")]));
}
