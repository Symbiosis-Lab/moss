//! Wiring test for the publish/media-race fix (designed 2026-09-16, see
//! `link_audit`'s module docs): a generation can seal before its own video
//! (or that video's poster) has finished encoding, leaving the page's HTML
//! with a dead reference the sealed manifest does not yet cover.
//! `record_promise_gate` is the function that turns exactly that shape of
//! dead link into a publish refusal — this drives the REAL function against
//! a real `stage_dir`, a real `SealedManifest` and a real `AssetRegistry`,
//! all the way through to `deploy::refuse_publish`, rather than only the
//! pure `link_audit::dead_links_among_promises` filter (that test lives in
//! `manifest/link_audit_tests.rs`).

use super::*;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::served_path::ServedPath;
use crate::types::assets::AssetRegistry;
use crate::types::content::SiteHashes;

/// A portable tempdir under `target/test-tmp`, matching
/// `epoch_ordering_tests.rs`'s own fixture: the system tempdir can itself sit
/// under a moss-managed folder on some machines, which would make this test
/// pass or fail for the wrong reason.
fn stage_dir() -> tempfile::TempDir {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-tmp");
    std::fs::create_dir_all(&base).unwrap();
    tempfile::Builder::new()
        .prefix("moss-promise-gate")
        .tempdir_in(&base)
        .unwrap()
}

/// A sealed manifest carrying exactly ONE page, its video/poster URLs
/// deliberately unregistered — mirroring what a real seal produces while a
/// video is still dispatched: the manifest omits it until the follow-up
/// rebuild registers it for real.
fn sealed_with_one_page(stage: &std::path::Path, page_html: &[u8]) -> crate::build::manifest::SealedManifest {
    std::fs::write(stage.join("index.html"), page_html).unwrap();
    let mut pending = PendingManifest::new(SiteHashes::default());
    pending.register(&ServedPath::from_source("index.html").unwrap(), page_html, HashBucket::Files);
    pending.seal()
}

const VIDEO_PAGE: &[u8] =
    br#"<video src="/videos/clip.mp4" poster="/videos/clip.thumb.jpg"></video>"#;

/// The wiring the whole fix exists for: a page referencing a video still
/// mid-encode is recorded as an unfulfilled promise, end to end through to
/// the publish gate.
#[test]
fn a_video_and_poster_still_pending_at_seal_time_gate_publish() {
    let tmp = stage_dir();
    let folder = format!("/promise-gate-test-{}", uuid::Uuid::new_v4());
    let sealed = sealed_with_one_page(tmp.path(), VIDEO_PAGE);

    let assets = std::sync::Arc::new(AssetRegistry::new());
    assets.set_pending("videos/clip.mp4".into(), None, None);

    let unfulfilled = record_promise_gate(tmp.path(), &sealed, Some(&assets), &folder);
    assert_eq!(unfulfilled.len(), 2, "both the mp4 and its derived poster key must be caught");

    let recorded = crate::system::build_records::records()
        .promised_dead_links(&folder)
        .expect("record_promise_gate must always write a verdict");
    assert_eq!(recorded.len(), 2);

    let err = crate::deploy::refuse_publish(&folder)
        .expect_err("a publish landing in the window must be refused, not raced through");
    assert!(err.contains("still being prepared"), "{err}");
}

/// A permanently failed encode must not gate publish forever — there is no
/// override, so a stuck refusal would leave the author with no way out. The
/// registry mirrors what `set_failed` really does: the key drops OUT of
/// `pending_keys()`.
#[test]
fn a_permanently_failed_video_does_not_gate_publish() {
    let tmp = stage_dir();
    let folder = format!("/promise-gate-failed-{}", uuid::Uuid::new_v4());
    let sealed = sealed_with_one_page(tmp.path(), VIDEO_PAGE);

    let assets = std::sync::Arc::new(AssetRegistry::new());
    assets.set_pending("videos/clip.mp4".into(), None, None);
    assets.set_failed("videos/clip.mp4".into(), "ffmpeg exited non-zero".into());

    let unfulfilled = record_promise_gate(tmp.path(), &sealed, Some(&assets), &folder);
    assert!(unfulfilled.is_empty(), "a failed encode's dead link must not be this build's promise");
    assert!(
        crate::deploy::refuse_publish(&folder).is_ok(),
        "a permanently failed video must not block publish"
    );
}

/// A build with no `AssetRegistry` at all (the one-shot CLI's synchronous
/// path can pass `None`) must not panic and must not manufacture a refusal
/// out of an ordinary dead link.
#[test]
fn no_registry_at_all_is_advisory_only() {
    let tmp = stage_dir();
    let folder = format!("/promise-gate-no-registry-{}", uuid::Uuid::new_v4());
    let sealed = sealed_with_one_page(tmp.path(), VIDEO_PAGE);

    let unfulfilled = record_promise_gate(tmp.path(), &sealed, None, &folder);
    assert!(unfulfilled.is_empty());
    assert!(crate::deploy::refuse_publish(&folder).is_ok());
}

/// The follow-up rebuild's clean seal — video now Ready and registered in
/// the manifest — clears the earlier refusal. There is no override, so a
/// verdict that never got replaced would block this folder's publish forever.
#[test]
fn a_follow_up_rebuild_clears_the_refusal() {
    let tmp = stage_dir();
    let folder = format!("/promise-gate-clears-{}", uuid::Uuid::new_v4());

    // First seal: the video is still dispatched, publish is refused.
    let sealed = sealed_with_one_page(tmp.path(), VIDEO_PAGE);
    let assets = std::sync::Arc::new(AssetRegistry::new());
    assets.set_pending("videos/clip.mp4".into(), None, None);
    record_promise_gate(tmp.path(), &sealed, Some(&assets), &folder);
    assert!(crate::deploy::refuse_publish(&folder).is_err());

    // Second seal: the follow-up rebuild's own registry sees the encode
    // finished, and the manifest now carries the real outputs.
    let mut pending = PendingManifest::new(SiteHashes::default());
    pending.register(&ServedPath::from_source("index.html").unwrap(), VIDEO_PAGE, HashBucket::Files);
    pending.register(
        &ServedPath::from_source("videos/clip.mp4").unwrap(),
        b"mp4-bytes",
        HashBucket::VideoOutputs,
    );
    pending.register(
        &ServedPath::from_source("videos/clip.thumb.jpg").unwrap(),
        b"jpg-bytes",
        HashBucket::VideoOutputs,
    );
    let sealed2 = pending.seal();
    let empty_assets = std::sync::Arc::new(AssetRegistry::new());
    record_promise_gate(tmp.path(), &sealed2, Some(&empty_assets), &folder);

    assert!(
        crate::deploy::refuse_publish(&folder).is_ok(),
        "the follow-up rebuild's clean seal must clear the earlier refusal"
    );
}
