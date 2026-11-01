use super::keep;
use crate::build::manifest::change_set::{PublishedSnapshot, RemovalReason};
use crate::build::manifest::{backfill, published_record, HashBucket, PendingManifest, SealedManifest};
use crate::build::served_path::ServedPath;
use crate::build::types::SourceMetadata;
use crate::moss_paths::MossPaths;
use crate::types::content::SiteHashes;
use std::path::Path;

const TABLE: &str = r#"{"version":1,"redirects":[{"from":"/old/","to":"/new/","status":301}]}"#;

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
    register(&mut pending, stage, "_moss/redirects.json", TABLE.as_bytes());
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

    let kept = keep(&mp, &stage, &mut sealed);

    assert_eq!(kept, [("photo-2.webp".to_string(), "photo-2.jpg".to_string())]);
    assert_eq!(std::fs::read(stage.join("photo-2.webp")).unwrap(), b"JPEG");
    assert!(sealed.files().contains_key("photo-2.webp"));
    assert!(backfill::removed_for_seal(&mp, &sealed).is_empty());
    let table = std::fs::read_to_string(stage.join("_moss/redirects.json")).unwrap();
    assert!(table.contains(r#""from":"/photo-2.webp","to":"/photo-2.jpg","status":301"#), "{table}");
    assert!(table.contains(r#""from":"/old/""#), "the earlier rows stay: {table}");
}

#[test]
fn an_address_the_build_already_serves_is_left_alone() {
    let (_vault, mp) = setup();
    let stage = mp.staging_dir();
    let mut sealed = build(&stage, &[("photo-2.webp", b"AUTHORS")], true);

    assert!(keep(&mp, &stage, &mut sealed).is_empty());
    assert_eq!(std::fs::read(stage.join("photo-2.webp")).unwrap(), b"AUTHORS");
}

#[test]
fn a_derivative_of_a_deleted_picture_is_not_kept_and_stays_the_authors_removal() {
    let (_vault, mp) = setup();
    let stage = mp.staging_dir();
    let mut sealed = build(&stage, &[], false);

    assert!(keep(&mp, &stage, &mut sealed).is_empty());
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
