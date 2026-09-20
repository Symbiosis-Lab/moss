use super::*;
use crate::build::types::SourceMetadata;
use std::path::PathBuf;
use tempfile::tempdir;

fn empty_manifest() -> PendingManifest {
    PendingManifest::new(SiteHashes::default())
}

// -----------------------------------------------------------------------
// Bucket registration semantics
// -----------------------------------------------------------------------

#[test]
fn register_files_inserts_into_files_and_blocking_keys() {
    let mut m = empty_manifest();
    m.register(
        &crate::build::served_path::ServedPath::from_source("index.html").unwrap(),
        b"<html/>",
        HashBucket::Files,
    );

    assert!(
        m.inner.files.contains_key("index.html"),
        "files map must contain the path"
    );
    let entry = m.inner.files.get("index.html").unwrap();
    assert!(entry.starts_with("100644:"), "entry must be mode-tagged");
    assert!(
        m.blocking_keys.contains("index.html"),
        "blocking_keys must contain the path"
    );

    // Must NOT land in any other bucket
    assert!(m.inner.image_outputs.is_empty());
    assert!(m.inner.video_outputs.is_empty());
    assert!(m.inner.notebook_outputs.is_empty());
}

#[test]
fn register_image_outputs_inserts_into_image_outputs_and_blocking_keys() {
    let mut m = empty_manifest();
    m.register(
        &crate::build::served_path::ServedPath::from_source("og/home.png").unwrap(),
        b"\x89PNG",
        HashBucket::ImageOutputs,
    );

    // Must be in image_outputs (preserves from stale-cleanup)
    assert!(m.inner.image_outputs.contains("og/home.png"));
    // Must be in blocking_keys (OG cards: Sites 4, 14b insert into both)
    assert!(
        m.blocking_keys.contains("og/home.png"),
        "OG card must be in blocking_keys"
    );

    // Must be in files (2026-05-15: enables deploy upload — the wire manifest
    // at deploy.rs:213 is built from sealed.files(), so OG cards previously
    // never reached the seta server and 404'd on the live site).
    assert!(
        m.inner.files.contains_key("og/home.png"),
        "OG card must be in files for deploy upload"
    );
    assert!(m.inner.video_outputs.is_empty());
    assert!(m.inner.notebook_outputs.is_empty());
}

#[test]
fn register_video_outputs_inserts_into_files_and_video_outputs() {
    let mut m = empty_manifest();
    m.register(
        &crate::build::served_path::ServedPath::from_source("videos/talk.mp4").unwrap(),
        b"ftyp",
        HashBucket::VideoOutputs,
    );

    // Must be in files (deploy manifest — 2026-06-11 fix: previously omitted, causing 404s)
    assert!(
        m.inner.files.contains_key("videos/talk.mp4"),
        "must be in files for deploy upload"
    );
    // Must be in video_outputs (stale-cleanup tracking)
    assert!(m.inner.video_outputs.contains("videos/talk.mp4"));
    // Must NOT be in blocking_keys — stale-cleanup must not treat video files as HTML pages
    assert!(!m.blocking_keys.contains("videos/talk.mp4"));
    assert!(m.inner.image_outputs.is_empty());
    assert!(m.inner.notebook_outputs.is_empty());
}

#[test]
fn register_notebook_outputs_inserts_into_files_and_notebook_outputs() {
    let mut m = empty_manifest();
    m.register(
        &crate::build::served_path::ServedPath::from_source("notebooks/report.html").unwrap(),
        b"<html>",
        HashBucket::NotebookOutputs,
    );

    // Must be in both files and notebook_outputs
    assert!(
        m.inner.files.contains_key("notebooks/report.html"),
        "must be in files"
    );
    assert!(
        m.inner.notebook_outputs.contains("notebooks/report.html"),
        "must be in notebook_outputs"
    );
    // Must NOT be in blocking_keys (audit Site 15: intentional omission)
    assert!(!m.blocking_keys.contains("notebooks/report.html"));
    assert!(m.inner.image_outputs.is_empty());
    assert!(m.inner.video_outputs.is_empty());
}

// -----------------------------------------------------------------------
// Seal invariant
// -----------------------------------------------------------------------

#[test]
fn seal_succeeds_when_invariant_holds() {
    let mut m = empty_manifest();
    m.register(
        &crate::build::served_path::ServedPath::from_source("style.css").unwrap(),
        b"body{}",
        HashBucket::Files,
    );
    // blocking_keys ⊆ files is maintained by register(Files) — seal must succeed
    let sealed = m.seal();
    assert!(sealed.files().contains_key("style.css"));
    assert!(sealed.blocking_keys().contains("style.css"));
}

#[test]
#[should_panic(
    expected = "blocking_keys ⊆ (files ∪ image_outputs) invariant violated at seal time"
)]
fn seal_panics_when_blocking_keys_has_no_files_entry() {
    let mut m = empty_manifest();
    // Insert a key into blocking_keys without a matching files entry
    m.force_blocking_key_for_test("orphan.html".to_string());
    m.seal(); // must panic in debug
}

#[test]
fn seal_with_empty_manifest_succeeds() {
    let m = empty_manifest();
    let sealed = m.seal();
    assert!(sealed.files().is_empty());
    assert!(sealed.blocking_keys().is_empty());
}

#[test]
fn apply_message_routes_to_correct_bucket() {
    // Files bucket: files + blocking_keys
    let mut m = empty_manifest();
    m.apply_message("a.html".to_string(), "aaa", HashBucket::Files, None);
    assert!(
        m.inner.files.contains_key("a.html"),
        "Files: must be in files"
    );
    assert!(
        m.blocking_keys.contains("a.html"),
        "Files: must be in blocking_keys"
    );
    assert!(!m.inner.image_outputs.contains("a.html"));
    assert!(!m.inner.notebook_outputs.contains("a.html"));

    // ImageOutputs bucket: image_outputs + blocking_keys + files
    // (2026-05-15: added files — see register_with_hash comment for why)
    let mut m = empty_manifest();
    m.apply_message("og/card.png".to_string(), "bbb", HashBucket::ImageOutputs, None);
    assert!(
        m.inner.image_outputs.contains("og/card.png"),
        "ImageOutputs: must be in image_outputs"
    );
    assert!(
        m.blocking_keys.contains("og/card.png"),
        "ImageOutputs: must be in blocking_keys"
    );
    assert!(
        m.inner.files.contains_key("og/card.png"),
        "ImageOutputs: must be in files (deploy upload)"
    );

    // VideoOutputs bucket: files + video_outputs, NOT blocking_keys
    // (2026-06-11: added files — same deploy-manifest gap as ImageVariants 2026-05-15)
    let mut m = empty_manifest();
    m.apply_message(
        "videos/talk.mp4".to_string(),
        "ccc",
        HashBucket::VideoOutputs,
        None,
    );
    assert!(
        m.inner.video_outputs.contains("videos/talk.mp4"),
        "VideoOutputs: must be in video_outputs"
    );
    assert!(!m.blocking_keys.contains("videos/talk.mp4"));
    assert!(
        m.inner.files.contains_key("videos/talk.mp4"),
        "VideoOutputs: must be in files (deploy upload)"
    );

    // NotebookOutputs bucket: files + notebook_outputs, NOT blocking_keys
    let mut m = empty_manifest();
    let hash = compute_binary_hash(b"<html>");
    m.apply_message(
        "notebooks/chart.html".to_string(),
        &hash,
        HashBucket::NotebookOutputs,
        None,
    );
    assert!(
        m.inner.files.contains_key("notebooks/chart.html"),
        "NotebookOutputs: must be in files"
    );
    assert!(
        m.inner.notebook_outputs.contains("notebooks/chart.html"),
        "NotebookOutputs: must be in notebook_outputs"
    );
    assert!(
        !m.blocking_keys.contains("notebooks/chart.html"),
        "NotebookOutputs: must NOT be in blocking_keys"
    );
    assert_eq!(
        m.inner.files.get("notebooks/chart.html").unwrap(),
        &file_entry(&hash)
    );
}

// -----------------------------------------------------------------------
// register_with_hash mode-prefix preservation
// -----------------------------------------------------------------------

#[test]
fn register_with_hash_preserves_mode_prefix() {
    // Case 1: bare hash — `register_with_hash` must prepend `100644:`.
    let mut m = empty_manifest();
    m.apply_message("page.html".to_string(), "abc123", HashBucket::Files, None);
    assert_eq!(
        m.inner.files.get("page.html").unwrap(),
        "100644:abc123",
        "bare hash must receive 100644: prefix"
    );

    // Case 2: already-prefixed `100644:` — must NOT double-prefix.
    let mut m = empty_manifest();
    m.apply_message("style.css".to_string(), "100644:def456", HashBucket::Files, None);
    assert_eq!(
        m.inner.files.get("style.css").unwrap(),
        "100644:def456",
        "existing 100644: prefix must be preserved verbatim"
    );

    // Case 3: symlink mode prefix `120000:` — must be preserved verbatim.
    let mut m = empty_manifest();
    m.apply_message(
        "link-target".to_string(),
        "120000:ghi789",
        HashBucket::Files,
        None,
    );
    assert_eq!(
        m.inner.files.get("link-target").unwrap(),
        "120000:ghi789",
        "symlink 120000: prefix must be preserved verbatim"
    );
}

// -----------------------------------------------------------------------
// write_to_disk round-trip
// -----------------------------------------------------------------------

#[test]
fn write_to_disk_round_trips() {
    let mut m = empty_manifest();
    m.register(
        &crate::build::served_path::ServedPath::from_source("index.html").unwrap(),
        b"<html/>",
        HashBucket::Files,
    );
    m.register(
        &crate::build::served_path::ServedPath::from_source("og/card.png").unwrap(),
        b"\x89PNG",
        HashBucket::ImageOutputs,
    );
    m.register(
        &crate::build::served_path::ServedPath::from_source("notebooks/nb.html").unwrap(),
        b"<nb>",
        HashBucket::NotebookOutputs,
    );

    let sealed = m.seal();

    let dir = tempdir().expect("tempdir failed");
    let path: PathBuf = dir.path().join("hashes.json");
    sealed.write_to_disk(&path).expect("write_to_disk failed");

    let content = std::fs::read_to_string(&path).expect("read_to_string failed");
    let parsed: SiteHashes = serde_json::from_str(&content).expect("deserialize failed");

    assert_eq!(parsed.files, *sealed.files());
    assert_eq!(parsed.image_outputs, *sealed.image_outputs());
    assert_eq!(parsed.notebook_outputs, *sealed.notebook_outputs());
}

// -----------------------------------------------------------------------
// carry_forward preservation
// -----------------------------------------------------------------------

#[test]
fn carry_forward_entries_are_preserved_mid_build() {
    let mut prev = SiteHashes::default();
    prev.files
        .insert("old.html".to_string(), file_entry("abc123"));

    let mut m = PendingManifest::new(prev);
    // New artifact — does not touch old.html
    m.register(
        &crate::build::served_path::ServedPath::from_source("new.html").unwrap(),
        b"<html/>",
        HashBucket::Files,
    );

    // Mid-build, the carry-forward entry is still visible so consumers like
    // sitemap generation (which reads `pending.files()` before seal) see a
    // coherent working set. Seal-time mark-and-sweep prunes it — see
    // `seal_prunes_untouched_carry_forward_entries` below.
    assert!(
        m.inner.files.contains_key("old.html"),
        "carry-forward must be visible mid-build"
    );
    assert!(m.inner.files.contains_key("new.html"));
}

/// Regression: when a page's slug changes, the previous build's output
/// path must not survive into the sealed manifest. The CPHS deploy
/// failure on 2026-05-18 (`projects/biogeochemical-processes/index.html`
/// listed in `hashes.json` while only the new long slug existed on disk)
/// landed because `PendingManifest::new` carried the old key forward and
/// nothing pruned it. Mark-and-sweep at `seal()` is the fix.
#[test]
fn seal_prunes_untouched_carry_forward_entries() {
    let mut carry = SiteHashes::default();
    carry.files.insert(
        "projects/old-slug/index.html".to_string(),
        file_entry("dead"),
    );
    carry
        .image_outputs
        .insert("_moss/og/old-hash.png".to_string());
    carry
        .video_outputs
        .insert("assets/old-video.mp4".to_string());
    carry
        .notebook_outputs
        .insert("notebooks/old/index.html".to_string());

    let mut pending = PendingManifest::new(carry);
    let new_html =
        crate::build::served_path::ServedPath::from_source("projects/new-slug/index.html").unwrap();
    pending.register(&new_html, b"<html>...", HashBucket::Files);

    let sealed = pending.seal();

    // The freshly registered path is in the sealed manifest.
    assert!(sealed.files().contains_key("projects/new-slug/index.html"));

    // Untouched carry-forward entries are pruned across all four output buckets.
    assert!(
        !sealed.files().contains_key("projects/old-slug/index.html"),
        "stale files entry must be swept (got: {:?})",
        sealed.files().keys().collect::<Vec<_>>()
    );
    assert!(
        !sealed.image_outputs().contains("_moss/og/old-hash.png"),
        "stale image_outputs entry must be swept (got: {:?})",
        sealed.image_outputs()
    );
    assert!(
        !sealed.video_outputs().contains("assets/old-video.mp4"),
        "stale video_outputs entry must be swept (got: {:?})",
        sealed.video_outputs()
    );
    assert!(
        !sealed
            .notebook_outputs()
            .contains("notebooks/old/index.html"),
        "stale notebook_outputs entry must be swept (got: {:?})",
        sealed.notebook_outputs()
    );
}

/// Mark-and-sweep does NOT touch the carry-forward cache fields
/// (`sources`, `builder_fingerprint`). These are
/// change-detection state, not output state, and their cross-build
/// consistency is owned by separate paths (out of scope for this struct).
#[test]
fn seal_preserves_carry_forward_cache_fields() {
    let mut carry = SiteHashes::default();
    carry.builder_fingerprint = Some("builder-fp-v1".to_string());
    // sources entries simulate prior-build change-detection metadata.
    carry.sources.insert(
        "Faculty.md".to_string(),
        crate::build::types::SourceMetadata::default(),
    );

    let pending = PendingManifest::new(carry);
    let sealed = pending.seal();

    let inner = sealed.site_hashes_view();
    assert_eq!(inner.builder_fingerprint.as_deref(), Some("builder-fp-v1"));
    assert!(
        inner.sources.contains_key("Faculty.md"),
        "carry-forward sources entries must NOT be swept (they're cache, not manifest)"
    );
}

// -----------------------------------------------------------------------
// Regression: OG cards in blocking_keys must not violate seal invariant
// -----------------------------------------------------------------------

#[test]
fn seal_succeeds_when_image_served_path_is_in_blocking_keys() {
    let mut m = empty_manifest();
    m.register(
        &crate::build::served_path::ServedPath::from_source("og/home.png").unwrap(),
        b"\x89PNG",
        HashBucket::ImageOutputs,
    );
    // path is in image_outputs AND blocking_keys but NOT files
    // seal must succeed because blocking_keys ⊆ (files ∪ image_outputs)
    let sealed = m.seal();
    assert!(sealed.image_outputs().contains("og/home.png"));
    assert!(sealed.blocking_keys().contains("og/home.png"));
}


// -----------------------------------------------------------------------
// site_hashes_view (#621 — deferred stale-cleanup borrow)
// -----------------------------------------------------------------------

#[test]
fn site_hashes_view_exposes_merged_buckets() {
    let mut m = empty_manifest();
    m.register(
        &crate::build::served_path::ServedPath::from_source("index.html").unwrap(),
        b"<html/>",
        HashBucket::Files,
    );
    m.register(
        &crate::build::served_path::ServedPath::from_source("og/home.png").unwrap(),
        b"\x89PNG",
        HashBucket::ImageOutputs,
    );
    m.register(
        &crate::build::served_path::ServedPath::from_source("media/talk.webp").unwrap(),
        b"webp",
        HashBucket::ImageVariants,
    );
    m.register(
        &crate::build::served_path::ServedPath::from_source("videos/talk.mp4").unwrap(),
        b"ftyp",
        HashBucket::VideoOutputs,
    );
    m.register(
        &crate::build::served_path::ServedPath::from_source("notebooks/nb.html").unwrap(),
        b"<nb>",
        HashBucket::NotebookOutputs,
    );

    let sealed = m.seal();
    let view = sealed.site_hashes_view();

    // Borrow exposes every bucket — the same content callers see via the
    // typed accessors but in `&SiteHashes` shape for `remove_stale_files`.
    assert!(view.files.contains_key("index.html"));
    assert!(view.files.contains_key("notebooks/nb.html"));
    // 2026-05-15: ImageOutputs and ImageVariants now also enter `files`
    // so the deploy wire manifest carries them. They still appear in
    // `image_outputs` for stale-cleanup preservation.
    assert!(view.files.contains_key("og/home.png"));
    assert!(view.files.contains_key("media/talk.webp"));
    assert!(view.image_outputs.contains("og/home.png"));
    assert!(view.image_outputs.contains("media/talk.webp"));
    assert!(view.video_outputs.contains("videos/talk.mp4"));
    assert!(view.notebook_outputs.contains("notebooks/nb.html"));
}

/// `source_to_output` must NOT carry forward across builds.
///
/// The HTML write loop re-registers every live document on every build,
/// so the only invariant worth holding is "this build's live docs."
/// Carrying entries forward would let a deleted source's mapping leak
/// into the next manifest, where it could shadow lookup or send a
/// future rename to a 404. Pruning at construction enforces the
/// invariant.
#[test]
fn pending_manifest_new_clears_source_to_output_carry_forward() {
    let mut carry = SiteHashes::default();
    carry
        .source_to_output
        .insert("posts/old.md".into(), "posts/old/index.html".into());
    carry
        .source_to_output
        .insert("readme.md".into(), "index.html".into());
    // Other persistent fields should still carry forward.
    carry
        .files
        .insert("posts/old/index.html".into(), file_entry("h1"));
    carry
        .sources
        .insert("media/x.jpg".into(), SourceMetadata::default());

    let pending = PendingManifest::new(carry);
    let (sealed, _) = pending.as_parts_clone();

    assert!(
        sealed.source_to_output.is_empty(),
        "source_to_output must be cleared on PendingManifest::new"
    );
    // Other carry-forward state survives.
    assert!(sealed.files.contains_key("posts/old/index.html"));
    assert!(sealed.sources.contains_key("media/x.jpg"));
}

/// After the clear, register_source_mapping populates the map cleanly.
/// This is the round-trip companion to the clear test above.
#[test]
fn register_source_mapping_populates_after_clear() {
    let mut carry = SiteHashes::default();
    carry
        .source_to_output
        .insert("stale.md".into(), "stale/index.html".into());

    let mut pending = PendingManifest::new(carry);
    pending.register_source_mapping(
        "Posts/Hello World.md".into(),
        &crate::build::served_path::ServedPath::from_source("posts/hello-world/index.html")
            .unwrap(),
    );
    pending.register_source_mapping(
        "readme.md".into(),
        &crate::build::served_path::ServedPath::from_source("index.html").unwrap(),
    );

    let (sealed, _) = pending.as_parts_clone();
    assert_eq!(
        sealed.source_to_output.len(),
        2,
        "fresh entries only — stale was cleared"
    );
    assert_eq!(
        sealed
            .source_to_output
            .get("Posts/Hello World.md")
            .map(String::as_str),
        Some("posts/hello-world/index.html")
    );
    assert_eq!(
        sealed.source_to_output.get("readme.md").map(String::as_str),
        Some("index.html")
    );
    assert!(
        !sealed.source_to_output.contains_key("stale.md"),
        "stale carry-forward entry must not survive"
    );
}

// -----------------------------------------------------------------------
// Structural invariant: every bucket must produce files membership
// -----------------------------------------------------------------------

/// Regression guard. Three separate bugs (ImageOutputs/ImageVariants 2026-05-15,
/// VideoOutputs 2026-06-11) all had the same root cause: a HashBucket variant
/// that forgot to call `inner.files.insert()`, so the path existed locally in
/// `.moss/build/current/` but the deploy wire manifest never included it, and seta
/// never uploaded it.  This test catches any future bucket with the same omission.
#[test]
fn every_bucket_variant_produces_files_membership() {
    use crate::build::served_path::ServedPath;
    let all_buckets = [
        HashBucket::Files,
        HashBucket::ImageOutputs,
        HashBucket::ImageVariants,
        HashBucket::VideoOutputs,
        HashBucket::NotebookOutputs,
    ];
    for (i, bucket) in all_buckets.iter().enumerate() {
        let mut m = empty_manifest();
        let path = format!("output-{i}.bin");
        m.register(&ServedPath::from_source(&path).unwrap(), b"data", *bucket);
        assert!(
            m.inner.files.contains_key(&path),
            "HashBucket::{bucket:?} must insert into inner.files — \
                 omitting this makes the path invisible to the deploy wire manifest \
                 and produces 404s on the live site"
        );
    }
}

// ---------------------------------------------------------------------------
// generation_id on SealedManifest (PR1)
// ---------------------------------------------------------------------------

#[test]
fn seal_sets_generation_id() {
    let mut m = empty_manifest();
    m.register(
        &crate::build::served_path::ServedPath::from_source("index.html").unwrap(),
        b"<html/>",
        HashBucket::Files,
    );
    let sealed = m.seal();
    let id = sealed.generation_id();
    assert_eq!(id.len(), 16, "generation_id must be 16 hex chars");
    assert!(
        id.chars().all(|c| c.is_ascii_hexdigit()),
        "generation_id must be lowercase hex: {id}"
    );
}

#[test]
fn seal_generation_id_is_insertion_order_independent() {
    let mut m1 = empty_manifest();
    m1.register(
        &crate::build::served_path::ServedPath::from_source("index.html").unwrap(),
        b"<html>a</html>",
        HashBucket::Files,
    );
    m1.register(
        &crate::build::served_path::ServedPath::from_source("style.css").unwrap(),
        b"body{}",
        HashBucket::Files,
    );

    let mut m2 = empty_manifest();
    m2.register(
        &crate::build::served_path::ServedPath::from_source("style.css").unwrap(),
        b"body{}",
        HashBucket::Files,
    );
    m2.register(
        &crate::build::served_path::ServedPath::from_source("index.html").unwrap(),
        b"<html>a</html>",
        HashBucket::Files,
    );

    assert_eq!(
        m1.seal().generation_id(),
        m2.seal().generation_id(),
        "same content registered in different order must produce equal generation_id"
    );
}

#[test]
fn seal_generation_id_changes_when_content_changes() {
    let mut m1 = empty_manifest();
    m1.register(
        &crate::build::served_path::ServedPath::from_source("index.html").unwrap(),
        b"<html>v1</html>",
        HashBucket::Files,
    );
    let id1 = m1.seal().generation_id().to_string();

    let mut m2 = empty_manifest();
    m2.register(
        &crate::build::served_path::ServedPath::from_source("index.html").unwrap(),
        b"<html>v2</html>",
        HashBucket::Files,
    );
    let id2 = m2.seal().generation_id().to_string();

    assert_ne!(
        id1, id2,
        "different content must produce different generation_id"
    );
}

// ---------------------------------------------------------------------------
// Determinism regression guard (Stage 0 invariant)
// ---------------------------------------------------------------------------

/// Two PendingManifests built with the same content but different
/// registration orders must produce equal generation_ids after seal.
///
/// Regression guard for the Stage 0 determinism requirement.
/// If this fails: compute_manifest_generation_id is not sorting before hashing.
#[test]
fn generation_id_determinism_across_registration_orders() {
    use crate::build::served_path::ServedPath;

    // Five distinct user-space paths. Note: _moss/ prefix is FORBIDDEN —
    // ServedPath::from_source rejects it with ReservedMossPrefix.
    let paths_and_bytes: &[(&str, &[u8])] = &[
        ("index.html", b"<html><body>home</body></html>"),
        ("about/index.html", b"<html><body>about</body></html>"),
        ("assets/style.css", b"body { margin: 0; }"),
        ("assets/theme.js", b"(function(){})();"),
        ("assets/photo.jpg", b"\xff\xd8\xff"),
    ];

    // Build A: register in forward order
    let mut m_a = empty_manifest();
    for (path, bytes) in paths_and_bytes {
        m_a.register(
            &ServedPath::from_source(path).unwrap(),
            bytes,
            HashBucket::Files,
        );
    }
    let id_a = m_a.seal().generation_id().to_string();

    // Build B: register in reverse order
    let mut m_b = empty_manifest();
    for (path, bytes) in paths_and_bytes.iter().rev() {
        m_b.register(
            &ServedPath::from_source(path).unwrap(),
            bytes,
            HashBucket::Files,
        );
    }
    let id_b = m_b.seal().generation_id().to_string();

    assert_eq!(
        id_a, id_b,
        "generation_id must be deterministic regardless of registration order.\n\
             id_a={id_a}, id_b={id_b}\n\
             If this fails, compute_manifest_generation_id is not sorting before hashing."
    );

    // Sanity: different content → different id.
    let mut m_c = empty_manifest();
    m_c.register(
        &ServedPath::from_source("index.html").unwrap(),
        b"<html><body>DIFFERENT</body></html>",
        HashBucket::Files,
    );
    assert_ne!(
        id_a,
        m_c.seal().generation_id(),
        "different content must produce a different generation_id (sanity check)"
    );
}

/// Two manifests with the same entries across all HashBucket types but
/// different registration orders must produce equal generation_ids.
#[test]
fn generation_id_determinism_across_bucket_types_and_orders() {
    use crate::build::served_path::ServedPath;

    let mut m1 = empty_manifest();
    m1.register(
        &ServedPath::from_source("index.html").unwrap(),
        b"<html>",
        HashBucket::Files,
    );
    m1.register(
        &ServedPath::from_source("og/card.png").unwrap(),
        b"\x89PNG",
        HashBucket::ImageOutputs,
    );
    m1.register(
        &ServedPath::from_source("assets/photo.webp").unwrap(),
        b"RIFF",
        HashBucket::ImageVariants,
    );
    m1.register(
        &ServedPath::from_source("videos/talk.mp4").unwrap(),
        b"ftyp",
        HashBucket::VideoOutputs,
    );
    m1.register(
        &ServedPath::from_source("notebooks/nb.html").unwrap(),
        b"<nb>",
        HashBucket::NotebookOutputs,
    );

    // Same registrations, reversed order
    let mut m2 = empty_manifest();
    m2.register(
        &ServedPath::from_source("notebooks/nb.html").unwrap(),
        b"<nb>",
        HashBucket::NotebookOutputs,
    );
    m2.register(
        &ServedPath::from_source("videos/talk.mp4").unwrap(),
        b"ftyp",
        HashBucket::VideoOutputs,
    );
    m2.register(
        &ServedPath::from_source("assets/photo.webp").unwrap(),
        b"RIFF",
        HashBucket::ImageVariants,
    );
    m2.register(
        &ServedPath::from_source("og/card.png").unwrap(),
        b"\x89PNG",
        HashBucket::ImageOutputs,
    );
    m2.register(
        &ServedPath::from_source("index.html").unwrap(),
        b"<html>",
        HashBucket::Files,
    );

    assert_eq!(
            m1.seal().generation_id(),
            m2.seal().generation_id(),
            "generation_id must be deterministic across all HashBucket types regardless of registration order"
        );
}

#[test]
fn site_url_flows_from_pending_through_seal() {
    let mut pending = PendingManifest::new(crate::types::content::SiteHashes::default());
    pending.set_site_url("https://liuguo.mosspub.com");
    let sealed = pending.seal();
    assert_eq!(sealed.site_url(), Some("https://liuguo.mosspub.com"));
}

#[test]
fn hashes_json_without_site_url_deserializes_to_none() {
    // Pre-upgrade hashes.json has no site_url key — must load, and the
    // deploy guard treats None as unknown provenance.
    let json = r#"{"files":{"index.html":"100644:abc"}}"#;
    let hashes: crate::types::content::SiteHashes = serde_json::from_str(json).expect("legacy load");
    assert_eq!(hashes.site_url, None);
    let sealed = PendingManifest::new(hashes).seal();
    assert_eq!(sealed.site_url(), None);
}

#[test]
fn site_url_round_trips_through_serde() {
    let mut hashes = crate::types::content::SiteHashes::default();
    hashes.site_url = Some("https://www.liu-guo.com".to_string());
    let json = serde_json::to_string(&hashes).unwrap();
    let back: crate::types::content::SiteHashes = serde_json::from_str(&json).unwrap();
    assert_eq!(back.site_url.as_deref(), Some("https://www.liu-guo.com"));
}

// -----------------------------------------------------------------------
// apply_post_seal_rewrites (moss#867)
// -----------------------------------------------------------------------

#[test]
fn apply_post_seal_rewrites_updates_hash_and_generation_id() {
    let mut pending = empty_manifest();
    let sp = crate::build::served_path::ServedPath::from_source("index.html").unwrap();
    pending.register(&sp, b"<html>original</html>", HashBucket::Files);
    let mut sealed = pending.seal();
    let original_gen_id = sealed.generation_id().to_string();
    let original_hash = sealed.files().get("index.html").cloned().unwrap();

    let new_hash = compute_binary_hash(b"<html>rewritten</html>");
    let mut rewrites = HashMap::new();
    rewrites.insert("index.html".to_string(), new_hash.clone());
    sealed.apply_post_seal_rewrites(rewrites);

    assert_ne!(sealed.files().get("index.html").unwrap(), &original_hash);
    assert_eq!(sealed.files().get("index.html").unwrap(), &file_entry(&new_hash));
    assert_ne!(sealed.generation_id(), original_gen_id, "generation_id must change with content");
}

#[test]
fn apply_post_seal_rewrites_is_noop_for_empty_map() {
    let mut pending = empty_manifest();
    let sp = crate::build::served_path::ServedPath::from_source("index.html").unwrap();
    pending.register(&sp, b"<html>x</html>", HashBucket::Files);
    let mut sealed = pending.seal();
    let before = sealed.generation_id().to_string();

    sealed.apply_post_seal_rewrites(HashMap::new());

    assert_eq!(sealed.generation_id(), before);
}

// -----------------------------------------------------------------------
// Page source hashes (publish-time change-set classification)
// -----------------------------------------------------------------------

fn meta(hash: &str) -> SourceMetadata {
    SourceMetadata { hash: hash.into(), size: 1, mtime: 1, mtime_nanos: None, ctime: None, inode: None }
}

fn sp(path: &str) -> crate::build::served_path::ServedPath {
    crate::build::served_path::ServedPath::from_source(path).unwrap()
}

/// A page this build never re-read (Stage-5b skip, parse-cache hit, iCloud
/// defer) keeps the hash the previous build recorded. Without the carry the
/// page has an output mapping and no hash, which the classifier can only read
/// as added or deleted — and a watch-loop rebuild skips most of a large site.
#[test]
fn carry_forward_page_source_preserves_hash_for_skipped_page() {
    let mut carry = SiteHashes::default();
    carry.source_to_output.insert("posts/a.md".into(), "posts/a/index.html".into());
    carry.sources.insert("posts/a.md".into(), meta("hash-a"));

    let mut pending = PendingManifest::new(carry);
    // The skip path: mapping re-registered, bytes never read.
    pending.register_source_mapping("posts/a.md".into(), &sp("posts/a/index.html"));
    assert_eq!(pending.carry_forward_page_source("posts/a.md"), Some(()));

    let sealed = pending.seal();
    assert_eq!(
        sealed.sources().get("posts/a.md").map(|m| m.hash.as_str()),
        Some("hash-a"),
        "a skipped page must keep the previous build's hash"
    );
}

/// A SLOT-ONLY source (`footer.md`) has a hash and deliberately no output
/// mapping. It must still carry forward on a build that did not re-read it.
///
/// This is the phantom-drift bug: the carry set used to be `sources` projected
/// through `source_to_output`, so a slot source — the one page kind guaranteed
/// absent from that map — could be registered once and never carried. It then
/// fell out of `sources` on the very next build, and the sweep's disk walk
/// read a file with no baseline entry as a CREATE, dispatching a full rebuild
/// every pass for the life of the session (harbor, 2026-08-20).
#[test]
fn a_slot_only_source_carries_forward_without_an_output_mapping() {
    let mut carry = SiteHashes::default();
    carry.sources.insert("footer.md".into(), meta("hash-footer"));
    carry.sources.insert("en/footer.md".into(), meta("hash-en-footer"));
    // A page with a mapping, to prove the two kinds coexist.
    carry.source_to_output.insert("posts/a.md".into(), "posts/a/index.html".into());
    carry.sources.insert("posts/a.md".into(), meta("hash-a"));

    let mut pending = PendingManifest::new(carry);
    assert_eq!(pending.carry_forward_page_source("footer.md"), Some(()));
    assert_eq!(pending.carry_forward_page_source("en/footer.md"), Some(()));

    let sealed = pending.seal();
    assert_eq!(
        sealed.sources().get("footer.md").map(|m| m.hash.as_str()),
        Some("hash-footer"),
        "a slot source with no output mapping must survive the carry"
    );
    assert_eq!(
        sealed.sources().get("en/footer.md").map(|m| m.hash.as_str()),
        Some("hash-en-footer"),
        "per-language slot siblings carry on the same rule"
    );
}

/// The carry set is the MARKDOWN half of `sources`, not all of it: an asset
/// entry must not be resurrected by a `carry_forward_page_source` call, or the
/// deferred asset walk's own bookkeeping stops being the single writer.
#[test]
fn the_page_carry_set_holds_no_assets() {
    let mut carry = SiteHashes::default();
    carry.sources.insert("assets/photo.jpg".into(), meta("hash-jpg"));
    let mut pending = PendingManifest::new(carry);
    assert_eq!(
        pending.carry_forward_page_source("assets/photo.jpg"),
        None,
        "an asset is not a page source and must not carry through the page path"
    );
}

/// Carrying a page the previous manifest never hashed reports the miss rather
/// than inventing an entry — the classifier's hash-carry valve then reads the
/// page as unknown, never as added or deleted.
#[test]
fn carry_forward_page_source_reports_when_no_previous_hash_exists() {
    let mut pending = empty_manifest();
    assert_eq!(pending.carry_forward_page_source("posts/new.md"), None);
    assert!(pending.seal().sources().is_empty());
}

/// The lockstep invariant, end to end through `seal`: every page with an output
/// mapping has a source hash, and no deleted page's hash lingers.
///
/// Covers all three ways a page reaches the sealed manifest — freshly hashed,
/// carried forward, and deleted — plus the asset half of `sources`, which is
/// change-detection cache with its own prune and must survive untouched.
#[test]
fn sources_and_source_to_output_cover_the_same_page_set() {
    let mut carry = SiteHashes::default();
    for name in ["kept", "skipped", "deleted"] {
        carry.source_to_output.insert(format!("posts/{name}.md"), format!("posts/{name}/index.html"));
        carry.sources.insert(format!("posts/{name}.md"), meta("old"));
    }
    carry.sources.insert("media/x.jpg".into(), meta("asset"));

    let mut pending = PendingManifest::new(carry);
    // Re-rendered: fresh bytes, fresh hash.
    pending.register_source_mapping("posts/kept.md".into(), &sp("posts/kept/index.html"));
    pending.register_page_source_hash("posts/kept.md".into(), meta("new"));
    // Skipped: mapping re-registered, hash carried.
    pending.register_source_mapping("posts/skipped.md".into(), &sp("posts/skipped/index.html"));
    pending.carry_forward_page_source("posts/skipped.md");
    // Added: never in the previous manifest at all.
    pending.register_source_mapping("posts/added.md".into(), &sp("posts/added/index.html"));
    pending.register_page_source_hash("posts/added.md".into(), meta("brand-new"));
    // `posts/deleted.md` is simply never registered.

    let sealed = pending.seal();

    let mapped: std::collections::BTreeSet<&String> = sealed.source_to_output().keys().collect();
    let hashed: std::collections::BTreeSet<&String> = sealed
        .sources()
        .keys()
        .filter(|k| k.ends_with(".md"))
        .collect();
    assert_eq!(mapped, hashed, "every mapped page is hashed and vice versa");

    assert_eq!(sealed.sources().get("posts/kept.md").unwrap().hash, "new");
    assert_eq!(sealed.sources().get("posts/skipped.md").unwrap().hash, "old");
    assert!(
        !sealed.sources().contains_key("posts/deleted.md"),
        "a deleted page's hash must go with it, or it reports deleted on every publish forever"
    );
    assert!(
        sealed.sources().contains_key("media/x.jpg"),
        "the asset half of `sources` is not this prune's business"
    );
}

/// The deferred asset walk's bulk `sources` replacement carries no page entries
/// — it `continue`s past markdown before inserting — so a naive overwrite would
/// drop every page hash the blocking phase just wrote.
#[test]
fn replace_sources_preserves_page_hashes() {
    let mut pending = empty_manifest();
    pending.register_source_mapping("posts/a.md".into(), &sp("posts/a/index.html"));
    pending.register_page_source_hash("posts/a.md".into(), meta("hash-a"));

    let mut walk_result = HashMap::new();
    walk_result.insert("media/x.jpg".to_string(), meta("asset"));
    pending.replace_sources(walk_result);

    let sealed = pending.seal();
    assert_eq!(sealed.sources().get("posts/a.md").unwrap().hash, "hash-a");
    assert!(sealed.sources().contains_key("media/x.jpg"));
}

// -----------------------------------------------------------------------
// Cancelled notebook run (moss#618)
// -----------------------------------------------------------------------

/// The previous build's JupyterLite bundle is carried forward but never
/// re-registered, so mark-and-sweep prunes it — and `build.rs`'s post-seal
/// staging sweep then deletes the files, 404ing a live page. This is the
/// layer that owns the sweep, so it is the layer that can see the loss.
fn previous_with_notebook_bundle() -> SiteHashes {
    let mut prev = SiteHashes::default();
    for key in ["jupyter/lab/index.html", "notebooks/report.html"] {
        prev.files.insert(key.to_string(), "100644:0123456789abcdef".to_string());
        prev.notebook_outputs.insert(key.to_string());
    }
    prev
}

#[test]
fn seal_drops_notebook_outputs_no_one_re_registered() {
    let sealed = PendingManifest::new(previous_with_notebook_bundle()).seal();

    assert!(sealed.notebook_outputs().is_empty());
    assert!(!sealed.files().contains_key("jupyter/lab/index.html"));
}

#[test]
fn cancelled_notebook_run_keeps_the_previous_bundle_byte_identical() {
    let prev = previous_with_notebook_bundle();
    let mut m = PendingManifest::new(prev.clone());

    m.carry_forward_notebook_outputs(&prev);
    let sealed = m.seal();

    for key in ["jupyter/lab/index.html", "notebooks/report.html"] {
        assert!(sealed.notebook_outputs().contains(key), "{key} must survive the sweep");
        // Verbatim, not re-prefixed: `100644:100644:…` would be a corrupt entry.
        assert_eq!(sealed.files().get(key), prev.files.get(key), "{key} entry must be byte-identical");
    }
}

// -----------------------------------------------------------------------
// A CAS object id belongs to the (path, hash) it was registered with
// -----------------------------------------------------------------------

/// Notebook viewer pages are written AFTER the slot pass, so on a second build
/// the pass can see the previous viewer and hand it an oid; the notebook step
/// then rewrites the page and registers the new hash without one. If the old
/// oid survived, `ship_phase` would read the old blob under the new hash and
/// deploy would refuse the generation.
#[test]
fn a_later_registration_without_an_oid_drops_the_earlier_cas_source() {
    use crate::build::served_path::ServedPath;
    let sp = ServedPath::from_source("notebooks/analysis.html").unwrap();
    let mut m = empty_manifest();

    m.apply_message(sp.as_str().to_string(), "aaaaaaaaaaaaaaaa", HashBucket::Files, Some("old-blob".to_string()));
    m.register_hashed(&sp, "bbbbbbbbbbbbbbbb", HashBucket::NotebookOutputs);

    let sealed = m.seal();
    assert_eq!(
        sealed.staged_oid("notebooks/analysis.html"),
        None,
        "an oid registered for the OLD hash must not outlive a re-registration under a new one"
    );
}

#[test]
fn a_later_registration_with_an_oid_replaces_the_earlier_one() {
    use crate::build::served_path::ServedPath;
    let sp = ServedPath::from_source("a.html").unwrap();
    let mut m = empty_manifest();

    m.apply_message(sp.as_str().to_string(), "aaaaaaaaaaaaaaaa", HashBucket::Files, Some("first".to_string()));
    m.apply_message(sp.as_str().to_string(), "bbbbbbbbbbbbbbbb", HashBucket::Files, Some("second".to_string()));

    assert_eq!(m.seal().staged_oid("a.html"), Some("second"));
}

// -----------------------------------------------------------------------
// Held bytes: a derived output that ships from memory
// -----------------------------------------------------------------------

mod held {
    use super::*;
    use crate::build::served_path::ServedPath;
    use super::super::ship_source::HELD_BYTES_BUDGET;

    fn served(rel: &str) -> ServedPath {
        ServedPath::from_source(rel).unwrap()
    }

    #[test]
    fn a_held_output_keeps_its_bytes_and_registers_their_hash() {
        let mut m = empty_manifest();
        m.register_held(&served("sitemap.xml"), b"<urlset/>".to_vec(), HashBucket::Files).unwrap();

        let sealed = m.seal();
        // Also the proof that `register` ran BEFORE the pin was inserted: a
        // registration with no oid removes whatever source is on record, so the
        // reverse order would leave this entry with none.
        assert_eq!(sealed.held_bytes("sitemap.xml"), Some(&b"<urlset/>"[..]));
        assert_eq!(
            sealed.files().get("sitemap.xml"),
            Some(&file_entry(&compute_binary_hash(b"<urlset/>"))),
            "the hash must be of exactly the bytes that are held"
        );
    }

    #[test]
    fn a_later_registration_replaces_the_held_bytes_with_its_own_hash() {
        let mut m = empty_manifest();
        m.register_held(&served("rss.xml"), b"old feed".to_vec(), HashBucket::Files).unwrap();
        // The deferred asset walk re-registering a vault's own `rss.xml`, or any
        // producer that later learns better: last registration wins, source included.
        m.register(&served("rss.xml"), b"new feed", HashBucket::Files);

        let sealed = m.seal();
        assert_eq!(sealed.held_bytes("rss.xml"), None, "held bytes for the OLD hash must not outlive a re-registration");
        assert_eq!(sealed.files().get("rss.xml"), Some(&file_entry(&compute_binary_hash(b"new feed"))));
    }

    #[test]
    fn held_bytes_replace_an_earlier_cas_source() {
        let mut m = empty_manifest();
        m.apply_message("llms.txt".to_string(), "aaaaaaaaaaaaaaaa", HashBucket::Files, Some("blob".to_string()));
        m.register_held(&served("llms.txt"), b"llms".to_vec(), HashBucket::Files).unwrap();

        let sealed = m.seal();
        assert_eq!(sealed.staged_oid("llms.txt"), None);
        assert_eq!(sealed.held_bytes("llms.txt"), Some(&b"llms"[..]));
    }

    /// `.html` is stripped of preview attributes on the way to the generation, so
    /// its manifest hash is of bytes that differ from the registered ones. Held
    /// bytes are shipped as they are: the pair would agree with each other and
    /// publish `data-source-*` annotations. Refused loudly, in release too.
    #[test]
    fn an_output_ship_transforms_cannot_be_held() {
        for rel in ["index.html", "legacy.htm"] {
            let mut m = empty_manifest();
            let err = m
                .register_held(&served(rel), b"<p data-source-line=\"1\">x</p>".to_vec(), HashBucket::Files)
                .unwrap_err();

            assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput, "{rel}");
            assert!(m.seal().files().is_empty(), "{rel}: a refused registration must record nothing");
        }
    }

    #[test]
    fn stamping_fingerprints_leaves_a_held_entry_alone() {
        let stage = tempdir().unwrap();
        // Both files exist, so a stamp COULD succeed on either.
        std::fs::write(stage.path().join("sitemap.xml"), b"<urlset/>").unwrap();
        std::fs::write(stage.path().join("plain.css"), b"a{}").unwrap();
        let mut m = empty_manifest();
        m.register_held(&served("sitemap.xml"), b"<urlset/>".to_vec(), HashBucket::Files).unwrap();
        m.register(&served("plain.css"), b"a{}", HashBucket::Files);
        let mut sealed = m.seal();

        sealed.stamp_all_ship_fingerprints(stage.path());

        assert_eq!(sealed.held_bytes("sitemap.xml"), Some(&b"<urlset/>"[..]), "a pin must not be replaced by a fingerprint");
        assert!(sealed.ship_fingerprint("sitemap.xml").is_none());
        assert!(sealed.ship_fingerprint("plain.css").is_some(), "an entry with no source still gets one");
    }

    #[test]
    fn release_drops_the_bytes_and_nothing_else() {
        let mut m = empty_manifest();
        m.register_held(&served("sitemap.xml"), b"<urlset/>".to_vec(), HashBucket::Files).unwrap();
        m.apply_message("img.webp".to_string(), "bbbbbbbbbbbbbbbb", HashBucket::ImageVariants, Some("blob".to_string()));
        let mut sealed = m.seal();
        let hash_before = sealed.files().get("sitemap.xml").cloned();

        sealed.release_held();

        assert_eq!(sealed.held_bytes("sitemap.xml"), None);
        assert_eq!(sealed.files().get("sitemap.xml"), hash_before.as_ref(), "the entry keeps its place and its hash");
        assert_eq!(sealed.staged_oid("img.webp"), Some("blob"), "only held bytes are released");
    }

    /// The whole point of the byte budget: a manifest never pins more than
    /// `HELD_BYTES_BUDGET`, and what does not fit is registered as an ordinary
    /// stage-read entry rather than refused.
    #[test]
    fn bytes_past_the_budget_ship_from_the_stage_instead_of_being_held() {
        let half_and_a_bit = vec![b'a'; HELD_BYTES_BUDGET / 2 + 1];
        let mut m = empty_manifest();
        m.register_held(&served("rss.xml"), half_and_a_bit.clone(), HashBucket::Files).unwrap();
        m.register_held(&served("llms.txt"), half_and_a_bit.clone(), HashBucket::Files).unwrap();

        let sealed = m.seal();
        assert!(sealed.held_bytes("rss.xml").is_some(), "the first fits");
        assert_eq!(sealed.held_bytes("llms.txt"), None, "the second would take the manifest past the budget");
        assert_eq!(
            sealed.files().get("llms.txt"),
            Some(&file_entry(&compute_binary_hash(&half_and_a_bit))),
            "not holding it must not un-register it"
        );
    }

    /// Replacing a held payload frees it first: re-registering one path with a
    /// payload that alone fits must not be judged against the payload it replaces.
    #[test]
    fn re_holding_one_path_does_not_count_the_payload_it_replaces() {
        let three_fifths = vec![b'a'; HELD_BYTES_BUDGET / 5 * 3];
        let mut m = empty_manifest();
        m.register_held(&served("llms.txt"), three_fifths.clone(), HashBucket::Files).unwrap();
        m.register_held(&served("llms.txt"), three_fifths, HashBucket::Files).unwrap();

        assert!(m.seal().held_bytes("llms.txt").is_some());
    }
}
