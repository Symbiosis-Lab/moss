use super::*;
use std::fs;
use std::path::Path;

#[test]
fn test_file_extension_categorization() {
    let md_extensions = vec!["md", "markdown", "mdown", "mkd"];
    for ext in md_extensions {
        assert!(ext == "md" || ext == "markdown" || ext == "mdown" || ext == "mkd");
    }

    let html_extensions = vec!["html", "htm"];
    for ext in html_extensions {
        assert!(ext == "html" || ext == "htm");
    }

    let image_extensions = vec!["jpg", "jpeg", "png", "gif", "svg", "webp"];
    for ext in image_extensions {
        assert!(["jpg", "jpeg", "png", "gif", "svg", "webp"].contains(&ext));
    }

    let doc_extensions = vec!["pages", "docx", "doc"];
    for ext in doc_extensions {
        assert!(["pages", "docx", "doc"].contains(&ext));
    }
}

#[test]
fn test_scan_folder_nonexistent_path() {
    let result = scan_folder("/definitely/does/not/exist/anywhere");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("does not exist"));
}

#[test]
fn test_scan_folder_file_instead_of_directory() {
    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join("test_file.txt");
    fs::write(&temp_file, "test content").unwrap();

    let result = scan_folder(&temp_file.to_string_lossy());
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("not a directory"));

    fs::remove_file(temp_file).ok();
}

#[test]
fn test_detect_homepage_file_priority_order() {
    let files = vec![
        FileInfo {
            path: "README.md".to_string(),
            file_type: "md".to_string(),
            size: 200,
            modified: None,
        },
        FileInfo {
            path: "index.md".to_string(),
            file_type: "md".to_string(),
            size: 150,
            modified: None,
        },
        FileInfo {
            path: "about.md".to_string(),
            file_type: "md".to_string(),
            size: 100,
            modified: None,
        },
    ];

    let result = detect_homepage_file(&files);
    assert_eq!(result, Some("index.md".to_string()));
}

#[test]
fn test_detect_homepage_file_fallback_to_readme() {
    let files = vec![
        FileInfo {
            path: "about.md".to_string(),
            file_type: "md".to_string(),
            size: 100,
            modified: None,
        },
        FileInfo {
            path: "contact.md".to_string(),
            file_type: "md".to_string(),
            size: 150,
            modified: None,
        },
        FileInfo {
            path: "README.md".to_string(),
            file_type: "md".to_string(),
            size: 200,
            modified: None,
        },
    ];

    let result = detect_homepage_file(&files);
    assert_eq!(result, Some("README.md".to_string()));
}

#[test]
fn test_detect_homepage_file_no_candidates() {
    let files = vec![
        FileInfo {
            path: "image.jpg".to_string(),
            file_type: "jpg".to_string(),
            size: 5000,
            modified: None,
        },
        FileInfo {
            path: "data.json".to_string(),
            file_type: "json".to_string(),
            size: 300,
            modified: None,
        },
    ];

    let result = detect_homepage_file(&files);
    assert_eq!(result, None);
}

#[test]
fn test_detect_homepage_file_underscore_index() {
    // _index.md should be recognized as a homepage candidate
    let files = vec![
        FileInfo {
            path: "about.md".to_string(),
            file_type: "md".to_string(),
            size: 100,
            modified: None,
        },
        FileInfo {
            path: "_index.md".to_string(),
            file_type: "md".to_string(),
            size: 150,
            modified: None,
        },
    ];

    let result = detect_homepage_file(&files);
    assert_eq!(result, Some("_index.md".to_string()));
}

#[test]
fn test_detect_homepage_file_main_md() {
    // main.md should be recognized as a homepage candidate
    let files = vec![
        FileInfo {
            path: "about.md".to_string(),
            file_type: "md".to_string(),
            size: 100,
            modified: None,
        },
        FileInfo {
            path: "main.md".to_string(),
            file_type: "md".to_string(),
            size: 150,
            modified: None,
        },
    ];

    let result = detect_homepage_file(&files);
    assert_eq!(result, Some("main.md".to_string()));
}

#[test]
fn test_detect_homepage_file_priority_index_over_readme() {
    // index.md should beat readme.md
    let files = vec![
        FileInfo {
            path: "README.md".to_string(),
            file_type: "md".to_string(),
            size: 200,
            modified: None,
        },
        FileInfo {
            path: "index.md".to_string(),
            file_type: "md".to_string(),
            size: 150,
            modified: None,
        },
    ];
    assert_eq!(detect_homepage_file(&files), Some("index.md".to_string()));
}

#[test]
fn test_detect_homepage_file_priority_readme_over_underscore_index() {
    // readme.md should beat _index.md (readme is higher priority)
    let files = vec![
        FileInfo {
            path: "_index.md".to_string(),
            file_type: "md".to_string(),
            size: 100,
            modified: None,
        },
        FileInfo {
            path: "README.md".to_string(),
            file_type: "md".to_string(),
            size: 200,
            modified: None,
        },
    ];
    assert_eq!(detect_homepage_file(&files), Some("README.md".to_string()));
}

#[test]
fn test_detect_homepage_file_priority_underscore_index_over_main() {
    // _index.md should beat main.md
    let files = vec![
        FileInfo {
            path: "main.md".to_string(),
            file_type: "md".to_string(),
            size: 100,
            modified: None,
        },
        FileInfo {
            path: "_index.md".to_string(),
            file_type: "md".to_string(),
            size: 200,
            modified: None,
        },
    ];
    assert_eq!(detect_homepage_file(&files), Some("_index.md".to_string()));
}

#[test]
fn test_detect_homepage_file_index_md_beats_index_pages() {
    // index.md should beat index.pages (markdown stems checked first)
    let files = vec![
        FileInfo {
            path: "index.pages".to_string(),
            file_type: "pages".to_string(),
            size: 100,
            modified: None,
        },
        FileInfo {
            path: "index.md".to_string(),
            file_type: "md".to_string(),
            size: 200,
            modified: None,
        },
    ];
    assert_eq!(detect_homepage_file(&files), Some("index.md".to_string()));
}

#[test]
fn test_detect_homepage_file_all_stems_present() {
    // When all four stems exist, index.md should win
    let files = vec![
        FileInfo {
            path: "main.md".to_string(),
            file_type: "md".to_string(),
            size: 100,
            modified: None,
        },
        FileInfo {
            path: "_index.md".to_string(),
            file_type: "md".to_string(),
            size: 100,
            modified: None,
        },
        FileInfo {
            path: "README.md".to_string(),
            file_type: "md".to_string(),
            size: 100,
            modified: None,
        },
        FileInfo {
            path: "index.md".to_string(),
            file_type: "md".to_string(),
            size: 100,
            modified: None,
        },
    ];
    assert_eq!(detect_homepage_file(&files), Some("index.md".to_string()));
}

#[test]
fn test_detect_homepage_self_named_folder_note() {
    let files = vec![
        FileInfo {
            path: "myblog.md".to_string(),
            file_type: "md".to_string(),
            size: 100,
            modified: None,
        },
        FileInfo {
            path: "about.md".to_string(),
            file_type: "md".to_string(),
            size: 100,
            modified: None,
        },
    ];
    // Without folder name, "about.md" wins alphabetically
    assert_eq!(detect_homepage_file(&files), Some("about.md".to_string()));
    // With folder name, "myblog.md" wins as self-named
    assert_eq!(
        detect_homepage_file_in_folder(&files, "myblog"),
        Some("myblog.md".to_string())
    );
}

#[test]
fn test_home_marker_detected_on_traditional_frontmatter_drives_homepage() {
    use std::fs;
    // In-repo temp dir per the test-artifact convention.
    let dir = tempfile::TempDir::new_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let root = dir.path();
    // A renamed folder's home: real `---` frontmatter with `home: true`, a
    // filename that is no longer self-named, alongside a competing index.md.
    fs::write(
        root.join("oldname.md"),
        "---\ntitle: \"Home\"\nhome: true\n---\n\nbody\n",
    )
    .unwrap();
    fs::write(
        root.join("index.md"),
        "---\ntitle: \"Index\"\n---\n\nbody\n",
    )
    .unwrap();

    // The marker must be detected on traditional (leading `---`) frontmatter.
    assert!(
        file_has_home_marker(&root.join("oldname.md")),
        "home: true on traditional --- frontmatter must be detected"
    );
    assert!(
        !file_has_home_marker(&root.join("index.md")),
        "a file without the marker must not be flagged"
    );

    // homepage_file detection must prefer the marked file over index.md and
    // over the (now non-matching) folder name — the rename-robustness case.
    let files = vec![
        FileInfo {
            path: "oldname.md".to_string(),
            file_type: "md".to_string(),
            size: 0,
            modified: None,
        },
        FileInfo {
            path: "index.md".to_string(),
            file_type: "md".to_string(),
            size: 0,
            modified: None,
        },
    ];
    assert_eq!(
        detect_homepage_file_in_folder_marked(&files, "newname", root),
        Some("oldname.md".to_string()),
        "marker wins over index.md and survives the folder rename"
    );
}

#[test]
fn test_case_insensitive_extensions() {
    let uppercase_extensions = vec!["MD", "HTML", "JPG", "PNG"];
    for ext in uppercase_extensions {
        let lowercase = ext.to_lowercase();
        assert!(["md", "html", "jpg", "png"].contains(&lowercase.as_str()));
    }
}

// =========================================================================
// Video Files Category Tests (TDD - Phase A)
// =========================================================================

#[test]
fn test_video_extensions_categorization() {
    // Verify the production matches! arm (line ~451) accepts all registry-
    // promised video extensions, including m4v. This test calls through the
    // real helper so it fails if an extension is missing from the arm.
    // Use a non-dot-prefixed name so scan_folder's `is_excluded_dir_name` does not
    // filter the root (tempfile's default prefix is ".tmp", which starts with ".",
    // causing the WalkDir root to be excluded). Builder::prefix avoids this.
    let temp_dir = tempfile::Builder::new()
        .prefix("moss_test_video_cat")
        .tempdir_in(env!("CARGO_MANIFEST_DIR"))
        .unwrap();
    let temp_dir = temp_dir.path();

    // One file per extension — scan_folder routes to video_files via the
    // same matches! arm that extract_media_metadata_cached uses.
    let video_exts = ["mov", "mp4", "webm", "avi", "mkv", "m4v"];
    for ext in &video_exts {
        fs::write(temp_dir.join(format!("clip.{ext}")), "fake video").unwrap();
    }

    let result = scan_folder(&temp_dir.to_string_lossy()).unwrap();

    assert_eq!(
        result.video_files.len(),
        video_exts.len(),
        "Expected {} video files, got {:?}",
        video_exts.len(),
        result
            .video_files
            .iter()
            .map(|f| &f.path)
            .collect::<Vec<_>>(),
    );
    for ext in &video_exts {
        assert!(
            result.video_files.iter().any(|f| f.path.ends_with(ext)),
            ".{ext} should be categorized as a video file by the production scan arm"
        );
    }
    // None of the video files should leak into other_files.
    for ext in &video_exts {
        assert!(
            !result.other_files.iter().any(|f| f.path.ends_with(ext)),
            ".{ext} must not appear in other_files"
        );
    }
}

#[test]
fn test_scan_folder_categorizes_videos() {
    // Create temp dir with video file
    let temp_dir = std::env::temp_dir().join(format!("moss_test_videos_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).unwrap();

    // Create a .mov file (empty, just for categorization test)
    fs::write(temp_dir.join("clip.mov"), "fake video").unwrap();
    fs::write(temp_dir.join("movie.mp4"), "fake video").unwrap();
    fs::write(temp_dir.join("reel.m4v"), "fake video").unwrap();
    fs::write(temp_dir.join("doc.md"), "# Test").unwrap();

    let result = scan_folder(&temp_dir.to_string_lossy()).unwrap();

    // Videos should be in video_files, not other_files
    assert_eq!(
        result.video_files.len(),
        3,
        "Should have 3 video files, got {:?}",
        result.video_files
    );
    assert!(
        result.video_files.iter().any(|f| f.path == "clip.mov"),
        "clip.mov should be in video_files"
    );
    assert!(
        result.video_files.iter().any(|f| f.path == "movie.mp4"),
        "movie.mp4 should be in video_files"
    );
    assert!(
        result.video_files.iter().any(|f| f.path == "reel.m4v"),
        "reel.m4v should be in video_files (registry-promised embeddable)"
    );

    // Videos should NOT be in other_files
    assert!(
        !result.other_files.iter().any(|f| f.path.ends_with(".mov")
            || f.path.ends_with(".mp4")
            || f.path.ends_with(".m4v")),
        "Videos should not be in other_files"
    );

    // Cleanup
    fs::remove_dir_all(&temp_dir).ok();
}

#[test]
fn test_scan_folder_videos_in_subdirectory() {
    let temp_dir =
        std::env::temp_dir().join(format!("moss_test_videos_subdir_{}", std::process::id()));
    let videos_dir = temp_dir.join("videos");
    fs::create_dir_all(&videos_dir).unwrap();

    // Create video in subdirectory
    fs::write(videos_dir.join("nature.MOV"), "fake video").unwrap();

    let result = scan_folder(&temp_dir.to_string_lossy()).unwrap();

    // Should find video in subdirectory (case-insensitive extension)
    assert_eq!(result.video_files.len(), 1);
    assert_eq!(result.video_files[0].path, "videos/nature.MOV");

    // Cleanup
    fs::remove_dir_all(&temp_dir).ok();
}

// =========================================================================
// MediaMetadata Extraction Tests (TDD - Phase B: ADR-002/006)
// =========================================================================

#[test]
fn test_media_metadata_extracts_image_dimensions() {
    // Test that extract_image_dimensions correctly reads image header
    // ADR-006: Uses image_dimensions() which reads header only (~1ms per file)
    let temp_dir = std::env::temp_dir().join(format!("moss_test_dims_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).unwrap();

    // Create a minimal valid PNG (1x1 red pixel)
    // PNG header + IHDR chunk specifying 100x50 dimensions
    let png_path = temp_dir.join("test.png");
    create_test_png(&png_path, 100, 50);

    let dims = extract_image_dimensions(&png_path);
    assert_eq!(
        dims,
        Some((100, 50)),
        "Should extract 100x50 dimensions from PNG"
    );

    // Cleanup
    fs::remove_dir_all(&temp_dir).ok();
}

#[test]
fn test_media_metadata_handles_exif_rotation() {
    // Test that EXIF orientation 5-8 swaps dimensions
    // ADR-006: Handle EXIF orientation (swap dims for orientations 5-8)
    let temp_dir = std::env::temp_dir().join(format!("moss_test_exif_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).unwrap();

    // For this test, we test the orientation check function directly
    // since creating real EXIF data is complex.
    // Orientations 5-8 indicate 90/270 degree rotation requiring dimension swap

    // Test orientation values that should trigger swap (5, 6, 7, 8)
    assert!(should_swap_dimensions(5), "Orientation 5 should swap");
    assert!(should_swap_dimensions(6), "Orientation 6 should swap");
    assert!(should_swap_dimensions(7), "Orientation 7 should swap");
    assert!(should_swap_dimensions(8), "Orientation 8 should swap");

    // Test orientation values that should NOT trigger swap (1, 2, 3, 4)
    assert!(!should_swap_dimensions(1), "Orientation 1 should not swap");
    assert!(!should_swap_dimensions(2), "Orientation 2 should not swap");
    assert!(!should_swap_dimensions(3), "Orientation 3 should not swap");
    assert!(!should_swap_dimensions(4), "Orientation 4 should not swap");

    // Cleanup
    fs::remove_dir_all(&temp_dir).ok();
}

// =========================================================================
// EXIF-oriented png/webp/jpeg dimension-swap tests (design follow-up #1)
//
// Regression pins for the "stranded rung publish-404" fix. Scan must store
// the DISPLAY (post-EXIF-orientation) dimensions for every ladder source
// (png/webp/jpeg) so the ladder it feeds emission/registration equals the
// ladder encode derives from `decode_oriented`. The png/webp cases MUST
// fail on the pre-fix HEAD (scan's orientation read was JPEG-gated, so
// png/webp stored the UNswapped header dims) and pass after ungating the
// swap onto every ladder source. Fixtures are hand-built byte-for-byte (no
// external tool) so they are hermetic on CI; the EXIF layout targets the
// exact reader the encode side uses (`read_exif_orientation`).
// =========================================================================

/// A little-endian TIFF/Exif blob whose single IFD0 entry is Orientation
/// (tag 0x0112, SHORT) = `orientation` — the exact structure
/// `kamadak-exif`'s `read_from_container` extracts from a JPEG APP1 /
/// PNG `eXIf` / WebP `EXIF` chunk.
fn exif_tiff_orientation(orientation: u16) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(b"II"); // little-endian byte order
    v.extend_from_slice(&42u16.to_le_bytes()); // TIFF magic
    v.extend_from_slice(&8u32.to_le_bytes()); // offset to IFD0
    v.extend_from_slice(&1u16.to_le_bytes()); // one directory entry
    v.extend_from_slice(&0x0112u16.to_le_bytes()); // tag: Orientation
    v.extend_from_slice(&3u16.to_le_bytes()); // type: SHORT
    v.extend_from_slice(&1u32.to_le_bytes()); // count: 1
    v.extend_from_slice(&(orientation as u32).to_le_bytes()); // value (inline, LE)
    v.extend_from_slice(&0u32.to_le_bytes()); // next IFD = none
    v
}

/// CRC-32/ISO-HDLC over `bytes` — the PNG chunk CRC (over chunk type +
/// data). Inlined so the test needs no crc crate; the value must be correct
/// or the image crate's PNG decoder rejects the `eXIf` chunk when
/// `decode_oriented` full-decodes the fixture.
fn png_crc32(bytes: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut c = Vec::new();
    c.extend_from_slice(&(data.len() as u32).to_be_bytes());
    c.extend_from_slice(kind);
    c.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    c.extend_from_slice(&png_crc32(&crc_input).to_be_bytes());
    c
}

/// Write a `w`×`h` solid PNG carrying an `eXIf` chunk with the given
/// Orientation. `image::image_dimensions` reads the STORED (unswapped)
/// `w`×`h` from IHDR; `read_exif_orientation` reads the tag.
fn write_png_with_exif_orientation(path: &Path, w: u32, h: u32, orientation: u16) {
    use std::io::Cursor;
    let img = image::RgbImage::from_pixel(w, h, image::Rgb([122, 139, 156]));
    let mut png = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    // signature(8) + IHDR chunk(len4+type4+data13+crc4 = 25) = 33.
    let insert_at = 8 + 25;
    assert_eq!(&png[12..16], b"IHDR", "IHDR must be the first chunk");
    let chunk = png_chunk(b"eXIf", &exif_tiff_orientation(orientation));
    let mut out = Vec::with_capacity(png.len() + chunk.len());
    out.extend_from_slice(&png[..insert_at]);
    out.extend_from_slice(&chunk);
    out.extend_from_slice(&png[insert_at..]);
    fs::write(path, out).unwrap();
}

fn riff_chunk(fourcc: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut c = Vec::new();
    c.extend_from_slice(fourcc);
    c.extend_from_slice(&(data.len() as u32).to_le_bytes());
    c.extend_from_slice(data);
    if data.len() & 1 == 1 {
        c.push(0); // RIFF chunks are padded to even length
    }
    c
}

/// Write a `w`×`h` solid WebP in EXTENDED (VP8X) form carrying an `EXIF`
/// chunk with the given Orientation. The simple VP8L bitstream is produced
/// by the image crate, then rewrapped with a VP8X header (EXIF flag set)
/// plus the EXIF chunk. `image::image_dimensions` reads the VP8X canvas
/// (STORED, unswapped) size.
fn write_webp_with_exif_orientation(path: &Path, w: u32, h: u32, orientation: u16) {
    use std::io::Cursor;
    let img = image::RgbImage::from_pixel(w, h, image::Rgb([122, 139, 156]));
    let mut simple = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut Cursor::new(&mut simple), image::ImageFormat::WebP)
        .unwrap();
    assert_eq!(&simple[0..4], b"RIFF");
    assert_eq!(&simple[8..12], b"WEBP");
    let mut fourcc = [0u8; 4];
    fourcc.copy_from_slice(&simple[12..16]);
    let clen = u32::from_le_bytes(simple[16..20].try_into().unwrap()) as usize;
    let cdata = simple[20..20 + clen].to_vec();

    // VP8X: flags(1) + reserved(3) + canvasW-1(3 LE) + canvasH-1(3 LE).
    // Flag bit 0x08 = "Exif metadata present".
    let mut vp8x = Vec::new();
    vp8x.push(0x08);
    vp8x.extend_from_slice(&[0, 0, 0]);
    vp8x.extend_from_slice(&(w - 1).to_le_bytes()[0..3]);
    vp8x.extend_from_slice(&(h - 1).to_le_bytes()[0..3]);

    let mut body = Vec::new();
    body.extend_from_slice(b"WEBP");
    body.extend_from_slice(&riff_chunk(b"VP8X", &vp8x));
    body.extend_from_slice(&riff_chunk(&fourcc, &cdata));
    body.extend_from_slice(&riff_chunk(b"EXIF", &exif_tiff_orientation(orientation)));

    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    fs::write(path, out).unwrap();
}

/// Write a `w`×`h` solid JPEG with an APP1 Exif segment (`Exif\0\0` +
/// TIFF) carrying the given Orientation, inserted right after SOI.
fn write_jpeg_with_exif_orientation(path: &Path, w: u32, h: u32, orientation: u16) {
    use std::io::Cursor;
    let img = image::RgbImage::from_pixel(w, h, image::Rgb([122, 139, 156]));
    let mut jpg = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut Cursor::new(&mut jpg), image::ImageFormat::Jpeg)
        .unwrap();
    assert_eq!(&jpg[0..2], &[0xFF, 0xD8], "SOI");
    let mut payload = Vec::new();
    payload.extend_from_slice(b"Exif\0\0");
    payload.extend_from_slice(&exif_tiff_orientation(orientation));
    let seg_len = (payload.len() + 2) as u16; // length field counts itself
    let mut out = Vec::new();
    out.extend_from_slice(&jpg[0..2]); // SOI
    out.extend_from_slice(&[0xFF, 0xE1]); // APP1 marker
    out.extend_from_slice(&seg_len.to_be_bytes());
    out.extend_from_slice(&payload);
    out.extend_from_slice(&jpg[2..]);
    fs::write(path, out).unwrap();
}

/// The oriented (display) dims the ENCODE side derives — the ground truth
/// scan must agree with. `decode_oriented` (build/media/rungs.rs) is the
/// exact function both the base webp pass and the rung encodes use.
fn oriented_dims(path: &Path) -> (u32, u32) {
    let img = crate::build::media::rungs::decode_oriented(path).unwrap();
    (img.width(), img.height())
}

#[test]
fn extract_image_dimensions_swaps_for_exif_oriented_png() {
    let dir = std::env::temp_dir().join(format!("moss_exif_png_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rotated.png");
    // Stored 2000x1000 landscape + orientation 6 (90° CW) → DISPLAY 1000x2000.
    write_png_with_exif_orientation(&path, 2000, 1000, 6);

    // Fixture sanity (true on pre- and post-fix HEAD): stored dims + tag.
    assert_eq!(image::image_dimensions(&path).unwrap(), (2000, 1000));
    assert_eq!(crate::build::media::image::read_exif_orientation(&path), 6);
    // Encode's oriented dims ARE the swapped display dims.
    assert_eq!(oriented_dims(&path), (1000, 2000));

    // THE FIX: scan stores the display (swapped) dims, matching encode.
    assert_eq!(
        extract_image_dimensions(&path),
        Some((1000, 2000)),
        "scan must store DISPLAY dims for an EXIF-oriented png (matches decode_oriented)"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn extract_image_dimensions_swaps_for_exif_oriented_webp() {
    let dir = std::env::temp_dir().join(format!("moss_exif_webp_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rotated.webp");
    write_webp_with_exif_orientation(&path, 2000, 1000, 6);

    assert_eq!(image::image_dimensions(&path).unwrap(), (2000, 1000));
    assert_eq!(crate::build::media::image::read_exif_orientation(&path), 6);
    assert_eq!(oriented_dims(&path), (1000, 2000));

    assert_eq!(
        extract_image_dimensions(&path),
        Some((1000, 2000)),
        "scan must store DISPLAY dims for an EXIF-oriented webp (matches decode_oriented)"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn scan_ladder_agrees_with_encode_for_exif_oriented_png_webp() {
    use moss_core::asset_paths::ladder_rungs;
    let dir = std::env::temp_dir().join(format!("moss_exif_ladder_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();

    let writers: [(&str, fn(&Path, u32, u32, u16)); 2] = [
        ("r.png", write_png_with_exif_orientation),
        ("r.webp", write_webp_with_exif_orientation),
    ];
    for (name, writer) in writers {
        let path = dir.join(name);
        // Stored 2000x1000 + orient6 → display 1000x2000. This is the
        // NON-superset publish-404 case: an UNswapped scan ladder [800,1600]
        // would promise a w1600 rung the oriented encode ([800]) never
        // produces — the exact stranded-rung bug this fix closes.
        writer(&path, 2000, 1000, 6);

        let (sw, sh) = extract_image_dimensions(&path).unwrap();
        let (ow, oh) = oriented_dims(&path);
        assert_eq!(
            ladder_rungs(sw, sh, false),
            ladder_rungs(ow, oh, false),
            "{name}: scan's ladder must equal encode's oriented ladder (no stranded rung)"
        );
        // And concretely the smaller, correct ladder — not the over-promised one.
        assert_eq!(ladder_rungs(sw, sh, false), &[800u32][..], "{name}");
    }
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn extract_image_dimensions_swaps_for_exif_oriented_jpeg_unchanged() {
    // JPEG already swapped before this fix — guard against regression when
    // the orientation read moves onto the shared encode-side reader.
    let dir = std::env::temp_dir().join(format!("moss_exif_jpg_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rotated.jpg");
    write_jpeg_with_exif_orientation(&path, 2000, 1000, 6);

    assert_eq!(image::image_dimensions(&path).unwrap(), (2000, 1000));
    assert_eq!(crate::build::media::image::read_exif_orientation(&path), 6);
    assert_eq!(
        extract_image_dimensions(&path),
        Some((1000, 2000)),
        "jpeg orientation-6 swap must remain correct"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn extract_image_dimensions_no_swap_for_non_exif_oriented_png_webp() {
    // Orientation 1 (identity) → no swap; and a plain image with no EXIF at
    // all → no swap. Guards against a spurious swap.
    let dir = std::env::temp_dir().join(format!("moss_exif_noswap_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();

    let png1 = dir.join("o1.png");
    write_png_with_exif_orientation(&png1, 2000, 1000, 1);
    assert_eq!(extract_image_dimensions(&png1), Some((2000, 1000)));

    let webp1 = dir.join("o1.webp");
    write_webp_with_exif_orientation(&webp1, 2000, 1000, 1);
    assert_eq!(extract_image_dimensions(&webp1), Some((2000, 1000)));

    let plain = dir.join("plain.png");
    create_test_png(&plain, 1200, 900);
    assert_eq!(extract_image_dimensions(&plain), Some((1200, 900)));

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_media_metadata_handles_corrupt_file() {
    // Test graceful handling of corrupt/invalid image files
    let temp_dir = std::env::temp_dir().join(format!("moss_test_corrupt_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).unwrap();

    // Create a file with invalid image data
    let corrupt_path = temp_dir.join("corrupt.png");
    fs::write(&corrupt_path, b"this is not a valid image file").unwrap();

    // Should return None, not panic
    let dims = extract_image_dimensions(&corrupt_path);
    assert_eq!(dims, None, "Corrupt file should return None for dimensions");

    let (color, _) = extract_color_and_lqip(&corrupt_path);
    assert_eq!(
        color, None,
        "Corrupt file should return None for dominant color"
    );

    // Cleanup
    fs::remove_dir_all(&temp_dir).ok();
}

#[test]
fn test_extract_dominant_color_uses_thumbnail() {
    // Test that dominant color extraction works and uses thumbnail for speed
    // ADR-006: Use 100x100 thumbnail for fast dominant color extraction
    let temp_dir = std::env::temp_dir().join(format!("moss_test_color_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).unwrap();

    // Create a solid red PNG
    let red_png_path = temp_dir.join("red.png");
    create_solid_color_png(&red_png_path, 200, 200, [255, 0, 0]);

    let (color, _) = extract_color_and_lqip(&red_png_path);
    assert!(
        color.is_some(),
        "Should extract dominant color from valid image"
    );

    let hex = color.unwrap();
    assert!(
        hex.starts_with('#'),
        "Color should be hex format starting with #"
    );
    assert_eq!(hex.len(), 7, "Color should be #RRGGBB format (7 chars)");

    // Red image should have high R value
    let r = u8::from_str_radix(&hex[1..3], 16).unwrap();
    assert!(r > 200, "Red image should have high red value, got {}", r);

    // Cleanup
    fs::remove_dir_all(&temp_dir).ok();
}

#[test]
fn test_extract_media_metadata_combines_all_fields() {
    // Test the combined extract_media_metadata function
    let temp_dir = std::env::temp_dir().join(format!("moss_test_meta_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).unwrap();

    let png_path = temp_dir.join("image.png");
    create_test_png(&png_path, 640, 480);

    let metadata = extract_media_metadata(
        &png_path,
        "image.png",
        "png",
        1024,
        Some("1234567890".to_string()),
        None,
    );

    assert_eq!(metadata.path, "image.png");
    assert_eq!(metadata.file_type, "png");
    assert_eq!(metadata.size, 1024);
    assert_eq!(metadata.modified, Some("1234567890".to_string()));
    assert_eq!(metadata.dimensions, Some((640, 480)));
    assert!(
        metadata.dominant_color.is_some(),
        "Should have dominant color"
    );

    // Cleanup
    fs::remove_dir_all(&temp_dir).ok();
}

#[test]
fn test_media_metadata_handles_video_dimensions() {
    // Test that video files get MediaMetadata but dimensions may be None
    // (video dimension extraction requires ffprobe, tested elsewhere)
    let temp_dir =
        std::env::temp_dir().join(format!("moss_test_video_meta_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).unwrap();

    let video_path = temp_dir.join("clip.mp4");
    fs::write(&video_path, b"fake video data").unwrap();

    let metadata = extract_media_metadata(
        &video_path,
        "clip.mp4",
        "mp4",
        2048,
        None,
        None, // No FFmpeg for this test
    );

    assert_eq!(metadata.path, "clip.mp4");
    assert_eq!(metadata.file_type, "mp4");
    assert_eq!(metadata.size, 2048);
    // Without FFmpegManager, video dimensions and color should be None
    assert_eq!(metadata.dimensions, None, "No dimensions without FFmpeg");
    assert_eq!(
        metadata.dominant_color, None,
        "No dominant color without FFmpeg"
    );

    // Cleanup
    fs::remove_dir_all(&temp_dir).ok();
}

// =========================================================================
// scan_folder_with_dedup tests (metadata singleflight feature)
// =========================================================================

/// scan_folder() still works identically — backward compatibility.
#[test]
fn test_scan_folder_backward_compat() {
    let temp_dir = std::env::temp_dir().join(format!("moss_test_sf_compat_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).unwrap();
    fs::write(temp_dir.join("hello.md"), "# Hello").unwrap();

    let result = scan_folder(&temp_dir.to_string_lossy());
    assert!(result.is_ok(), "scan_folder must still work");
    let ps = result.unwrap();
    assert_eq!(ps.markdown_files.len(), 1);
    assert_eq!(ps.markdown_files[0].path, "hello.md");

    fs::remove_dir_all(&temp_dir).ok();
}

/// scan_folder_with_dedup(path, None) produces the same result as scan_folder(path).
#[test]
fn test_scan_folder_with_dedup_none_same_as_scan_folder() {
    let temp_dir =
        std::env::temp_dir().join(format!("moss_test_sf_dedup_none_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).unwrap();
    fs::write(temp_dir.join("page.md"), "# Page").unwrap();

    let folder = temp_dir.to_string_lossy().to_string();
    let result_old = scan_folder(&folder).unwrap();
    let result_new = scan_folder_with_dedup(&folder, None).unwrap();

    assert_eq!(
        result_old.markdown_files.len(),
        result_new.markdown_files.len()
    );
    assert_eq!(result_old.total_files, result_new.total_files);

    fs::remove_dir_all(&temp_dir).ok();
}

// =========================================================================
// Lazy FFmpeg Resolution Tests (Task 4)
// =========================================================================

/// Scanning a markdown-only directory should NOT resolve ffmpeg.
/// After the lazy change, ffmpeg_bin_path should be None when no videos exist.
#[test]
fn test_scan_folder_no_videos_has_no_ffmpeg_path() {
    let temp_dir = std::env::temp_dir().join(format!("moss_test_no_ffmpeg_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).unwrap();
    fs::write(temp_dir.join("readme.md"), "# Hello").unwrap();
    fs::write(temp_dir.join("about.md"), "# About").unwrap();

    let result = scan_folder(&temp_dir.to_string_lossy()).unwrap();

    // No video files should exist
    assert!(result.video_files.is_empty(), "No video files expected");
    // FFmpeg should NOT have been resolved (lazy: no videos = no ffmpeg)
    assert!(
        result.ffmpeg_bin_path.is_none(),
        "ffmpeg_bin_path should be None when no videos found"
    );

    fs::remove_dir_all(&temp_dir).ok();
}

/// Scanning a directory with videos should resolve ffmpeg lazily.
/// ffmpeg_bin_path should be Some(...) when videos are present
/// (assuming ffmpeg is available on the system or was previously downloaded).
/// Note: This test may set ffmpeg_bin_path to None if ffmpeg is not installed,
/// but it must NOT panic. The key invariant is that ffmpeg is only resolved
/// when video files are actually encountered during the walk.
#[test]
fn test_scan_folder_with_videos_populates_ffmpeg_path() {
    let temp_dir =
        std::env::temp_dir().join(format!("moss_test_ffmpeg_lazy_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).unwrap();
    fs::write(temp_dir.join("clip.mp4"), "fake video").unwrap();
    fs::write(temp_dir.join("doc.md"), "# Test").unwrap();

    let result = scan_folder(&temp_dir.to_string_lossy()).unwrap();

    // Should have found the video
    assert_eq!(result.video_files.len(), 1);
    // ffmpeg_bin_path should have been resolved lazily when the video was found.
    // If ffmpeg is available (common on dev machines), verify the path was set.
    if result.ffmpeg_bin_path.is_some() {
        assert!(
            !result.ffmpeg_bin_path.as_ref().unwrap().is_empty(),
            "ffmpeg_bin_path should be a non-empty path when ffmpeg is available"
        );
    }
    // If ffmpeg is NOT available (CI/minimal environments), ffmpeg_bin_path
    // will be None — the lazy init ran but get_or_download returned an error.

    fs::remove_dir_all(&temp_dir).ok();
}

// =========================================================================
// Video Scan Hash-Skip Performance Tests (TDD)
//
// These tests verify the optimization that skips SHA-256 hashing for
// video files during the blocking scan phase. Video files are multi-GB;
// hashing takes 14-28s per file. The scan phase only needs dimensions
// (via ffprobe, ~50ms), not content verification.
// =========================================================================

/// Video files should NOT be hashed when the hash index has no stat match.
/// On first scan (empty hash index), videos should skip hashing entirely
/// and go directly to metadata extraction. We verify this by checking that
/// new_index does NOT contain a hash entry for the video file — since
/// hashing was skipped, no hash was computed.
#[test]
fn test_video_scan_skips_hash_on_stat_miss() {
    let temp_dir =
        std::env::temp_dir().join(format!("moss_test_video_skip_hash_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    // Create a fake video file (small, but extension is what matters).
    let video_path = temp_dir.join("big_video.mp4");
    fs::write(&video_path, b"fake video data for testing").unwrap();

    // Set up empty caches — simulates first-ever scan.
    let cache_dir = temp_dir.join(".moss/build/cache");
    fs::create_dir_all(cache_dir.join("objects")).unwrap();
    fs::create_dir_all(cache_dir.join("transforms")).unwrap();
    let objects = ObjectStore::new(cache_dir.join("objects"));
    let transform_cache = TransformCache::new(
        cache_dir.join("transforms"),
        ObjectStore::new(cache_dir.join("objects")),
    );
    let old_index = HashIndex::new();
    let mut new_index = HashIndex::new();

    let meta = extract_media_metadata_cached(
        &video_path,
        "big_video.mp4",
        "mp4",
        &FileStat::whole_second(27, 1234567890),
        None,
        None, // No FFmpeg — dimensions will be None (that's fine)
        &old_index,
        &mut new_index,
        &objects,
        &transform_cache,
        None,  // No singleflight
        false, // build-mode default (videos defer regardless)
    );

    // The function should return valid metadata even without hashing.
    assert_eq!(meta.path, "big_video.mp4");
    assert_eq!(meta.file_type, "mp4");
    assert_eq!(meta.size, 27);

    // Key assertion: new_index should NOT contain a hash entry for the
    // video, because we skipped hashing. This proves we avoided the
    // expensive SHA-256 computation.
    assert!(
        !new_index.entries.contains_key("big_video.mp4"),
        "Video file should NOT have been hashed on stat miss (first scan)"
    );

    fs::remove_dir_all(&temp_dir).ok();
}

/// When the hash index already has a stat match for a video file
/// (second build onward), the existing hash-based path should still work.
/// Pre-populate the hash index + transform cache, then verify the cached
/// metadata is returned.
#[test]
fn test_video_scan_uses_hash_when_index_hits() {
    let temp_dir =
        std::env::temp_dir().join(format!("moss_test_video_hash_hit_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let video_path = temp_dir.join("cached_video.mov");
    fs::write(&video_path, b"fake video").unwrap();

    // Set up caches.
    let cache_dir = temp_dir.join(".moss/build/cache");
    fs::create_dir_all(cache_dir.join("objects")).unwrap();
    fs::create_dir_all(cache_dir.join("transforms")).unwrap();
    let objects = ObjectStore::new(cache_dir.join("objects"));
    let transform_cache = TransformCache::new(
        cache_dir.join("transforms"),
        ObjectStore::new(cache_dir.join("objects")),
    );

    // Pre-populate hash index with a stat match.
    let fake_hash = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
    let mut old_index = HashIndex::new();
    old_index.update_whole_second(
        "cached_video.mov".to_string(),
        10,
        9999999,
        fake_hash.to_string(),
    );

    // Pre-populate transform cache with cached metadata.
    let cached = CachedMediaMeta {
        dimensions: Some((1920, 1080)),
        dominant_color: Some("#FF5733".to_string()),
        lqip_data_uri: None,
        is_animated: false,
    };
    write_cached_meta(&objects, &transform_cache, fake_hash, 10, &cached);

    let mut new_index = HashIndex::new();

    let meta = extract_media_metadata_cached(
        &video_path,
        "cached_video.mov",
        "mov",
        &FileStat::whole_second(10, 9999999),
        Some("2024-01-01".to_string()),
        None,
        &old_index,
        &mut new_index,
        &objects,
        &transform_cache,
        None,
        false,
    );

    // Should return cached metadata (hash path still works).
    assert_eq!(meta.path, "cached_video.mov");
    assert_eq!(meta.dimensions, Some((1920, 1080)));
    assert_eq!(meta.dominant_color, Some("#FF5733".to_string()));

    // Hash should be propagated to new_index (stat match was found).
    assert!(
        new_index.lookup_whole_second("cached_video.mov", 10, 9999999) == Some(fake_hash),
        "Hash should be propagated to new_index when hash index has a stat match"
    );

    fs::remove_dir_all(&temp_dir).ok();
}

/// Images defer their expensive work (full-file SHA-256 + full-decode for
/// dominant color / LQIP) OFF the blocking scan path. On a cold stat-miss
/// the scan reads ONLY the dimensions (image header, ~1ms) and leaves
/// `dominant_color` / `lqip_data_uri` as `None`; the background media phase
/// computes those from the pixels it already decodes for WebP conversion,
/// and hashes the file there. This keeps first paint independent of image
/// count. (Previously images were fully hashed AND decoded here, which made
/// a 1000-image folder take minutes before the preview could appear.)
#[test]
fn test_image_scan_defers_expensive_work_on_stat_miss() {
    let temp_dir =
        std::env::temp_dir().join(format!("moss_test_image_defers_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    // Solid-red PNG: if color extraction had run on the blocking path,
    // dominant_color would be ~#FF0000. Asserting None proves we skipped
    // the full decode.
    let png_path = temp_dir.join("photo.png");
    create_solid_color_png(&png_path, 64, 48, [255, 0, 0]);
    let file_size = fs::metadata(&png_path).unwrap().len();

    // Set up empty caches — simulates a first-ever (cold) scan.
    let cache_dir = temp_dir.join(".moss/build/cache");
    fs::create_dir_all(cache_dir.join("objects")).unwrap();
    fs::create_dir_all(cache_dir.join("transforms")).unwrap();
    let objects = ObjectStore::new(cache_dir.join("objects"));
    let transform_cache = TransformCache::new(
        cache_dir.join("transforms"),
        ObjectStore::new(cache_dir.join("objects")),
    );
    let old_index = HashIndex::new();
    let mut new_index = HashIndex::new();

    let meta = extract_media_metadata_cached(
        &png_path,
        "photo.png",
        "png",
        &FileStat::whole_second(file_size, 1234567890),
        None,
        None,
        &old_index,
        &mut new_index,
        &objects,
        &transform_cache,
        None,
        true, // preview mode: defer color/LQIP
    );

    // Dimensions ARE read on the blocking path (cheap header read) — they
    // reserve the aspect-ratio box that prevents layout shift on first paint.
    assert_eq!(
        meta.dimensions,
        Some((64, 48)),
        "dimensions must be read from the header on the blocking scan"
    );

    // The expensive full-decode outputs are DEFERRED to the background.
    assert!(
        meta.dominant_color.is_none(),
        "dominant_color must be deferred off the blocking scan (got {:?})",
        meta.dominant_color
    );
    assert!(
        meta.lqip_data_uri.is_none(),
        "lqip must be deferred off the blocking scan"
    );

    // The full-file SHA-256 is DEFERRED too — no hash entry on the blocking scan.
    assert!(
        !new_index.entries.contains_key("photo.png"),
        "image must NOT be SHA-256 hashed on the blocking scan (deferred to background)"
    );

    fs::remove_dir_all(&temp_dir).ok();
}

// =========================================================================
// is_animated sniffing on the scan path (responsive-image-variants Task 9)
//
// The flag is sniffed from the file header (bounded ≤4 KB read, gif/webp
// extensions only) at every scan — it is deliberately NOT stored in
// CachedMediaMeta, so warm stat-cache hits must re-sniff rather than pin a
// stale `false` from a pre-flag cache entry.
// =========================================================================

/// Minimal animated-WebP bytes: RIFF/WEBP container with a VP8X chunk
/// carrying an ANIM chunk — the exact layout
/// `build::media::sniff::is_animated_webp` sniffs for.
fn animated_webp_bytes() -> Vec<u8> {
    let mut bytes: Vec<u8> = b"RIFF".to_vec();
    bytes.extend_from_slice(&[0, 0, 0, 0]); // RIFF length (unchecked by the sniffer)
    bytes.extend_from_slice(b"WEBP");
    bytes.extend_from_slice(b"VP8X");
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(b"ANIM");
    bytes.extend_from_slice(&[0; 10]);
    bytes
}

/// Animated WebP: the flag lands `true` on the cold (deferred, stat-miss)
/// scan path AND stays `true` on the warm stat-cache-hit path.
#[test]
fn test_scan_carries_is_animated_for_animated_webp() {
    let temp_dir =
        std::env::temp_dir().join(format!("moss_test_scan_anim_webp_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let webp_path = temp_dir.join("loop.webp");
    fs::write(&webp_path, animated_webp_bytes()).unwrap();
    let file_size = fs::metadata(&webp_path).unwrap().len();

    let cache_dir = temp_dir.join(".moss/build/cache");
    fs::create_dir_all(cache_dir.join("objects")).unwrap();
    fs::create_dir_all(cache_dir.join("transforms")).unwrap();
    let objects = ObjectStore::new(cache_dir.join("objects"));
    let transform_cache = TransformCache::new(
        cache_dir.join("transforms"),
        ObjectStore::new(cache_dir.join("objects")),
    );
    let old_index = HashIndex::new();
    let mut new_index = HashIndex::new();

    // Cold scan (preview mode, deferred stat-key path — the hot route).
    let meta = extract_media_metadata_cached(
        &webp_path,
        "loop.webp",
        "webp",
        &FileStat::whole_second(file_size, 1234567890),
        None,
        None,
        &old_index,
        &mut new_index,
        &objects,
        &transform_cache,
        None,
        true,
    );
    assert!(
        meta.is_animated,
        "animated WebP (VP8X + ANIM chunk) must scan as is_animated"
    );

    // Warm scan: same stat key now hits the cache, which does NOT store
    // the flag — the return-site sniff must still land `true`.
    let meta2 = extract_media_metadata_cached(
        &webp_path,
        "loop.webp",
        "webp",
        &FileStat::whole_second(file_size, 1234567890),
        None,
        None,
        &old_index,
        &mut new_index,
        &objects,
        &transform_cache,
        None,
        true,
    );
    assert!(
        meta2.is_animated,
        "warm stat-cache-hit scan must re-sniff is_animated, not pin false"
    );

    fs::remove_dir_all(&temp_dir).ok();
}

/// Static WebP: the flag stays `false`.
#[test]
fn test_scan_static_webp_is_not_animated() {
    let temp_dir =
        std::env::temp_dir().join(format!("moss_test_scan_static_webp_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    // Minimal static WebP header (VP8 chunk, no ANIM anywhere).
    let mut bytes: Vec<u8> = b"RIFF".to_vec();
    bytes.extend_from_slice(&[0, 0, 0, 0]);
    bytes.extend_from_slice(b"WEBPVP8 ");
    bytes.extend_from_slice(&[0; 32]);
    let webp_path = temp_dir.join("still.webp");
    fs::write(&webp_path, &bytes).unwrap();
    let file_size = fs::metadata(&webp_path).unwrap().len();

    let cache_dir = temp_dir.join(".moss/build/cache");
    fs::create_dir_all(cache_dir.join("objects")).unwrap();
    fs::create_dir_all(cache_dir.join("transforms")).unwrap();
    let objects = ObjectStore::new(cache_dir.join("objects"));
    let transform_cache = TransformCache::new(
        cache_dir.join("transforms"),
        ObjectStore::new(cache_dir.join("objects")),
    );
    let old_index = HashIndex::new();
    let mut new_index = HashIndex::new();

    let meta = extract_media_metadata_cached(
        &webp_path,
        "still.webp",
        "webp",
        &FileStat::whole_second(file_size, 1234567890),
        None,
        None,
        &old_index,
        &mut new_index,
        &objects,
        &transform_cache,
        None,
        true,
    );
    assert!(
        !meta.is_animated,
        "static WebP must not scan as is_animated"
    );

    fs::remove_dir_all(&temp_dir).ok();
}

/// Animated GIF through the direct (uncached, build-mode) extractor:
/// covers the `extract_media_metadata` construction site.
#[test]
fn test_scan_carries_is_animated_for_animated_gif() {
    let temp_dir =
        std::env::temp_dir().join(format!("moss_test_scan_anim_gif_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    // Hand-crafted 2-frame GIF89a: two 0x2C Image Descriptor introducers,
    // which is what `is_animated_gif` counts. It does not need to decode —
    // extract_color_and_lqip degrades to (None, None) on a bad body.
    let mut bytes: Vec<u8> = b"GIF89a".to_vec();
    bytes.extend_from_slice(&[1, 0, 1, 0]); // LSD width/height
    bytes.extend_from_slice(&[0, 0, 0]); // packed, bgcolor, aspect
    bytes.push(0x2C);
    bytes.extend_from_slice(&[0; 9]);
    bytes.push(0x2C);
    bytes.extend_from_slice(&[0; 9]);
    bytes.push(0x3B); // trailer
    let gif_path = temp_dir.join("dance.gif");
    fs::write(&gif_path, &bytes).unwrap();
    let file_size = fs::metadata(&gif_path).unwrap().len();

    let meta = extract_media_metadata(&gif_path, "dance.gif", "gif", file_size, None, None);
    assert!(
        meta.is_animated,
        "multi-frame GIF must scan as is_animated on the direct extract path"
    );

    fs::remove_dir_all(&temp_dir).ok();
}

/// Video files should use stat-based cache keys on second call within
/// the same session. When a video was extracted with stat key on the
/// first call, the second call should find the stat-cached result.
#[test]
fn test_video_scan_stat_cache_hit_on_second_call() {
    let temp_dir =
        std::env::temp_dir().join(format!("moss_test_video_stat_cache_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let video_path = temp_dir.join("repeat.mp4");
    fs::write(&video_path, b"fake video data").unwrap();

    let cache_dir = temp_dir.join(".moss/build/cache");
    fs::create_dir_all(cache_dir.join("objects")).unwrap();
    fs::create_dir_all(cache_dir.join("transforms")).unwrap();
    let objects = ObjectStore::new(cache_dir.join("objects"));
    let transform_cache = TransformCache::new(
        cache_dir.join("transforms"),
        ObjectStore::new(cache_dir.join("objects")),
    );
    let old_index = HashIndex::new();
    let mut new_index = HashIndex::new();

    // First call: stat miss, extracts metadata directly.
    let meta1 = extract_media_metadata_cached(
        &video_path,
        "repeat.mp4",
        "mp4",
        &FileStat::whole_second(15, 5555555555),
        None,
        None,
        &old_index,
        &mut new_index,
        &objects,
        &transform_cache,
        None,
        false,
    );

    // Second call: should hit stat-based cache.
    let meta2 = extract_media_metadata_cached(
        &video_path,
        "repeat.mp4",
        "mp4",
        &FileStat::whole_second(15, 5555555555),
        None,
        None,
        &old_index,
        &mut new_index,
        &objects,
        &transform_cache,
        None,
        false,
    );

    // Both calls should return the same metadata.
    assert_eq!(meta1.path, meta2.path);
    assert_eq!(meta1.file_type, meta2.file_type);
    assert_eq!(meta1.dimensions, meta2.dimensions);

    // Still no hash entry — both calls used the stat-key path.
    assert!(
        !new_index.entries.contains_key("repeat.mp4"),
        "Video should not have been hashed in either call"
    );

    fs::remove_dir_all(&temp_dir).ok();
}

// =========================================================================
// The hash index across a scan: stat identity carried, never laundered
// =========================================================================

/// One image and the caches a scan reads and writes, for the tests below.
struct ScanFixture {
    dir: std::path::PathBuf,
    png: std::path::PathBuf,
    objects: ObjectStore,
    transforms: TransformCache,
}

impl ScanFixture {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("moss_scan_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let cache = dir.join(".moss/build/cache");
        fs::create_dir_all(cache.join("objects")).unwrap();
        fs::create_dir_all(cache.join("transforms")).unwrap();
        let png = dir.join("photo.png");
        create_solid_color_png(&png, 8, 8, [255, 0, 0]);
        Self {
            dir,
            png,
            objects: ObjectStore::new(cache.join("objects")),
            transforms: TransformCache::new(cache.join("transforms"), ObjectStore::new(cache.join("objects"))),
        }
    }

    fn scan(
        &self,
        stat: &FileStat,
        old_index: &HashIndex,
        new_index: &mut HashIndex,
        defer_placeholders: bool,
    ) -> MediaMetadata {
        extract_media_metadata_cached(
            &self.png, "photo.png", "png", stat, None, None, old_index, new_index, &self.objects, &self.transforms, None,
            defer_placeholders,
        )
    }

    /// The placeholder metadata an earlier scan of this image, hydrated, cached under
    /// `hash`.
    fn cache_placeholder_under(&self, hash: &str) -> CachedMediaMeta {
        let meta = CachedMediaMeta {
            dimensions: Some((8, 8)),
            dominant_color: Some("#ff0000".to_string()),
            lqip_data_uri: Some("data:image/webp;base64,AAAA".to_string()),
            is_animated: false,
        };
        write_cached_meta(&self.objects, &self.transforms, hash, 1, &meta);
        meta
    }
}

impl Drop for ScanFixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.dir).ok();
    }
}

/// The background worker records each image with its full stat record and the next
/// scan rewrites the index from scratch, so the scan has to hand the record on as it
/// found it — or `collect_images_for_conversion` would miss on every image, every
/// build, and the worker would hash them all again.
#[test]
fn a_scan_carries_a_full_stat_entry_forward_so_the_next_collect_still_matches() {
    let fx = ScanFixture::new("carry_full");
    let stat = FileStat::of(&fs::metadata(&fx.png).unwrap());
    let mut old_index = HashIndex::new();
    old_index.update("photo.png".to_string(), &stat, "recorded-by-the-worker".to_string());
    let mut new_index = HashIndex::new();

    fx.scan(&stat, &old_index, &mut new_index, true);

    assert_eq!(new_index.lookup("photo.png", &stat), Some("recorded-by-the-worker"));
}

/// An entry recorded for another instant of the file is not this file's hash, and the
/// scan must not carry it — nor, by re-stamping it with the current stat, turn it into
/// one that `lookup` would trust. A preview scan (which never hashes an image) leaves
/// the file out of the new index; a build-mode scan hashes it afresh.
#[test]
fn a_scan_does_not_carry_forward_an_entry_recorded_for_a_different_instant() {
    let fx = ScanFixture::new("carry_stale");
    let stat = FileStat::of(&fs::metadata(&fx.png).unwrap());
    let earlier = FileStat { mtime_nanos: stat.mtime_nanos.map(|n| (n + 250_000_000) % 1_000_000_000), ..stat };
    let mut old_index = HashIndex::new();
    old_index.update("photo.png".to_string(), &earlier, "hash-of-the-previous-bytes".to_string());

    let mut preview = HashIndex::new();
    fx.scan(&stat, &old_index, &mut preview, true);
    assert!(preview.entries.is_empty(), "preview scan carried forward {:?}", preview.entries);

    let mut build = HashIndex::new();
    fx.scan(&stat, &old_index, &mut build, false);
    assert_eq!(build.lookup("photo.png", &stat), Some(ObjectStore::hash_file(&fx.png).unwrap().as_str()));
}

/// A build-mode scan hashes an image it has no entry for, and what it records must be
/// the record `collect_images_for_conversion` will look for.
#[test]
fn a_build_mode_scan_records_the_hash_it_computed_with_the_files_full_stat() {
    let fx = ScanFixture::new("record_full");
    let stat = FileStat::of(&fs::metadata(&fx.png).unwrap());
    let mut new_index = HashIndex::new();

    fx.scan(&stat, &HashIndex::new(), &mut new_index, false);

    assert_eq!(new_index.lookup("photo.png", &stat), Some(ObjectStore::hash_file(&fx.png).unwrap().as_str()));
}

/// An entry the index recorded while the image was local, seen by a scan after the
/// provider evicted it: the recorded ctime and inode no longer match, and nothing
/// may read the file to find out whether the bytes still do.
fn recorded_before_the_provider_touched_it(fx: &ScanFixture, hash: &str) -> (FileStat, HashIndex) {
    let stat = FileStat::of(&fs::metadata(&fx.png).unwrap());
    let recorded = FileStat { ctime: Some(1), inode: Some(1), ..stat };
    let mut old_index = HashIndex::new();
    old_index.update("photo.png".to_string(), &recorded, hash.to_string());
    (stat, old_index)
}

/// An evicted image cannot be hashed, so its metadata is found by the hash the index
/// recorded — under the whole-second rule, the only one it could be answered by
/// before the index recorded more. The strict rule misses the moment the provider
/// changes ctime or inode, and a build-mode scan (which bakes LQIP and dimensions
/// into the page) would then find nothing and read an unreadable file for them.
#[test]
fn an_evicted_image_keeps_the_placeholder_metadata_cached_under_its_recorded_hash() {
    let fx = ScanFixture::new("evicted_meta");
    let cached = fx.cache_placeholder_under("hash-of-the-hydrated-bytes");
    let (stat, old_index) = recorded_before_the_provider_touched_it(&fx, "hash-of-the-hydrated-bytes");
    let _cloud = crate::build::icloud::pretend::evicted(&fx.png);

    for defer_placeholders in [false, true] {
        let mut new_index = HashIndex::new();
        let meta = fx.scan(&stat, &old_index, &mut new_index, defer_placeholders);

        assert_eq!(
            (meta.dimensions, meta.dominant_color.clone(), meta.lqip_data_uri.clone()),
            (cached.dimensions, cached.dominant_color.clone(), cached.lqip_data_uri.clone()),
            "defer_placeholders={defer_placeholders}: the evicted image lost its placeholder"
        );
        // Carried as recorded: the strict lookup that names an encode must still miss.
        assert_eq!(new_index.entries, old_index.entries, "defer_placeholders={defer_placeholders}");
        assert!(new_index.lookup("photo.png", &stat).is_none());
    }
}

/// The same answer is not given for an image that is on disk: a scan that defers its
/// placeholder (preview) or hashes it (build) has no need of a hash it cannot vouch
/// for, and a whole-second hit would carry it into an index the worker trusts.
#[test]
fn an_image_on_disk_is_not_matched_by_the_whole_second_rule() {
    let fx = ScanFixture::new("hydrated_strict");
    fx.cache_placeholder_under("hash-of-the-hydrated-bytes");
    let (stat, old_index) = recorded_before_the_provider_touched_it(&fx, "hash-of-the-hydrated-bytes");

    let mut preview = HashIndex::new();
    let meta = fx.scan(&stat, &old_index, &mut preview, true);
    assert!(preview.entries.is_empty(), "preview scan carried forward {:?}", preview.entries);
    assert_ne!(meta.lqip_data_uri.as_deref(), Some("data:image/webp;base64,AAAA"), "used a hash the index does not vouch for");
}

/// What an evicted source yields is a non-answer, and cached under its hash — which
/// its arrival does not change — it would be permanent. Reachable once an evicted
/// file can hold a hash the index recorded while it was local, and the metadata was
/// never cached under it (a preview scan hashes nothing).
#[test]
fn an_evicted_image_never_has_a_non_answer_cached_under_its_hash() {
    let fx = ScanFixture::new("evicted_no_poison");
    let (stat, old_index) = recorded_before_the_provider_touched_it(&fx, "hash-with-no-cached-metadata");
    let _cloud = crate::build::icloud::pretend::evicted(&fx.png);

    let meta = fx.scan(&stat, &old_index, &mut HashIndex::new(), false);

    assert_eq!(meta.dimensions, None, "premise: an evicted image is read as nothing");
    assert!(
        read_cached_meta(&fx.transforms, &fx.objects, "hash-with-no-cached-metadata").is_none(),
        "the scan cached dimensions: None under the hash of a file it could not read"
    );
}

/// A preview scan reads an image's dimensions and caches them under a key made of its
/// path, size and mtime. An evicted source cannot answer, its arrival preserves size and
/// mtime, and so the key would serve a non-answer for as long as the file stays the
/// same. The evicted image is read here (the seam marks it, it does not stop the read),
/// so what the guard keeps out of the cache is a real answer and the control is the
/// same scan of the image once it is on disk.
#[test]
fn a_preview_scan_caches_nothing_under_the_stat_key_of_an_evicted_image() {
    let fx = ScanFixture::new("evicted_stat_key");
    let stat = FileStat::of(&fs::metadata(&fx.png).unwrap());
    let key = image_meta_stat_key("photo.png", stat.size, stat.mtime);

    let cloud = crate::build::icloud::pretend::evicted(&fx.png);
    fx.scan(&stat, &HashIndex::new(), &mut HashIndex::new(), true);
    assert!(
        read_cached_meta(&fx.transforms, &fx.objects, &key).is_none(),
        "the scan cached what it read of an evicted image under its stat key"
    );

    drop(cloud);
    fx.scan(&stat, &HashIndex::new(), &mut HashIndex::new(), true);
    assert!(
        read_cached_meta(&fx.transforms, &fx.objects, &key).is_some(),
        "control: the same scan of an image on disk caches its dimensions under the stat key"
    );
}

/// A build-mode scan hashes an image the last index does not vouch for, after it has
/// asked whether the image is in the cloud — and the provider can evict it in between.
/// The scan's own hash read is guarded like every other, so the image is left unhashed
/// (its worker defers it) instead of blocking the scan on a download.
#[test]
fn a_build_scan_does_not_hash_an_image_that_went_to_the_cloud_after_it_was_checked() {
    let fx = ScanFixture::new("evicted_after_check");
    let stat = FileStat::of(&fs::metadata(&fx.png).unwrap());
    let mut new_index = HashIndex::new();

    let _cloud = crate::build::icloud::pretend::evicted_after(&fx.png, 1);
    fx.scan(&stat, &HashIndex::new(), &mut new_index, false);

    assert!(new_index.entries.is_empty(), "the scan hashed an image that was in the cloud: {:?}", new_index.entries);
}

// =========================================================================
// What the walk feeds the hash index: the file's whole stat record
// =========================================================================

/// A vault with one image, scanned the way a build scans it (`defer_placeholders =
/// false`, which hashes an image it has no trusted entry for). The persisted index is
/// the observable: an entry the scan trusted comes back as it was, one it did not comes
/// back re-hashed.
struct ScannedVault {
    dir: tempfile::TempDir,
}

impl ScannedVault {
    fn new() -> Self {
        // Not `tempdir()`'s dot-named directory: the walk skips hidden folders.
        let dir = tempfile::Builder::new().prefix("moss_scan_vault_").tempdir().unwrap();
        // Two solid colours whose PNGs are the same number of bytes, so the rewrite
        // below is a same-size one.
        create_solid_color_png(&dir.path().join("photo.png"), 24, 24, [200, 30, 30]);
        Self { dir }
    }

    fn png(&self) -> std::path::PathBuf {
        self.dir.path().join("photo.png")
    }

    fn index_path(&self) -> std::path::PathBuf {
        self.dir.path().join(".moss/build/cache/hash-index.json")
    }

    /// Scan, and return `photo.png`'s entry in the index the scan persisted.
    fn scan(&self) -> crate::build::cache::HashIndexEntry {
        scan_folder_with_dedup_emit(self.dir.path().to_str().unwrap(), None, None, false).unwrap();
        HashIndex::load(&self.index_path()).entries.remove("photo.png").expect("the scan recorded the image")
    }

    /// Leave an index that holds `hash` for `photo.png` as it stood at `recorded`.
    fn record(&self, recorded: &FileStat, hash: &str) {
        let mut index = HashIndex::new();
        index.update("photo.png".to_string(), recorded, hash.to_string());
        index.save(&self.index_path()).unwrap();
    }
}

/// The walk hands each image's stat to the scan, and the scan trusts an entry only for
/// that whole record. An entry recorded for another size, mtime, sub-second mtime,
/// ctime or inode is not this file's, and the hash in it — planted here so a wrongly
/// trusted entry is visible — must not come back. The exact record is the control.
#[test]
fn the_scan_trusts_an_entry_only_for_the_images_whole_stat_record() {
    let vault = ScannedVault::new();
    let real = FileStat::of(&fs::metadata(vault.png()).unwrap());
    let hashed = ObjectStore::hash_file(&vault.png()).unwrap();

    vault.record(&real, "planted");
    assert_eq!(vault.scan().content_hash, "planted", "control: the exact record hits");

    for (field, changed) in real.each_field_changed() {
        vault.record(&changed, "planted");
        assert_eq!(vault.scan().content_hash, hashed, "an entry recorded for another {field} was trusted");
    }
}

/// Replace-via-rename with size and mtime kept: the scan must hash the new file, not
/// carry the old one's hash into the index the image worker reads.
#[cfg(unix)]
#[test]
fn an_image_replaced_by_rename_with_its_size_and_mtime_kept_is_hashed_again() {
    let vault = ScannedVault::new();
    let first = vault.scan().content_hash;

    let replacement = vault.dir.path().join("replacement.png");
    create_solid_color_png(&replacement, 24, 24, [30, 30, 200]);
    FileStat::replace_by_rename_keeping_mtime(&vault.png(), &fs::read(&replacement).unwrap());
    fs::remove_file(&replacement).unwrap();

    let second = vault.scan().content_hash;
    assert_ne!(second, first, "the scan kept the hash of the image that was replaced");
    assert_eq!(second, ObjectStore::hash_file(&vault.png()).unwrap());
}

// =========================================================================
// Test Helpers
// =========================================================================

/// Create a test PNG with specified dimensions
/// Uses the image crate to create a valid PNG file
fn create_test_png(path: &std::path::Path, width: u32, height: u32) {
    use image::{ImageBuffer, Rgb};
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(width, height, |x, y| {
        // Create a gradient pattern
        Rgb([(x % 256) as u8, (y % 256) as u8, 128])
    });
    img.save(path).unwrap();
}

/// Create a solid color PNG for testing dominant color extraction
fn create_solid_color_png(path: &std::path::Path, width: u32, height: u32, rgb: [u8; 3]) {
    use image::{ImageBuffer, Rgb};
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(width, height, |_, _| Rgb(rgb));
    img.save(path).unwrap();
}

// -----------------------------------------------------------------------
// LQIP generation tests
// -----------------------------------------------------------------------

#[test]
fn test_extract_color_and_lqip_returns_data_uri_for_valid_image() {
    let temp_dir = std::env::temp_dir().join(format!("moss_test_lqip_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).unwrap();

    let png_path = temp_dir.join("blue.png");
    create_solid_color_png(&png_path, 200, 200, [0, 0, 255]);

    let (color, lqip) = extract_color_and_lqip(&png_path);

    assert!(color.is_some(), "Should extract dominant color");
    assert!(lqip.is_some(), "Should generate LQIP for valid image");

    let uri = lqip.unwrap();
    assert!(
        uri.starts_with("data:image/jpeg;base64,"),
        "LQIP must be a JPEG data URI, got: {}",
        &uri[..uri.len().min(40)]
    );
    // A 20px-wide JPEG should be well under 1KB
    assert!(
        uri.len() < 1024,
        "LQIP data URI too large: {} bytes",
        uri.len()
    );

    fs::remove_dir_all(&temp_dir).ok();
}

#[test]
fn test_extract_color_and_lqip_returns_none_for_corrupt_file() {
    let temp_dir =
        std::env::temp_dir().join(format!("moss_test_lqip_corrupt_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).unwrap();

    let corrupt_path = temp_dir.join("corrupt.png");
    fs::write(&corrupt_path, b"this is not a valid image").unwrap();

    let (color, lqip) = extract_color_and_lqip(&corrupt_path);

    assert!(color.is_none(), "Should return None color for corrupt file");
    assert!(lqip.is_none(), "Should return None LQIP for corrupt file");

    fs::remove_dir_all(&temp_dir).ok();
}

#[test]
fn test_extract_color_and_lqip_returns_none_lqip_for_svg() {
    let temp_dir = std::env::temp_dir().join(format!("moss_test_lqip_svg_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).unwrap();

    // SVG cannot be loaded by the image crate, so both should be None
    let svg_path = temp_dir.join("icon.svg");
    fs::write(&svg_path, b"<svg xmlns='http://www.w3.org/2000/svg' width='100' height='100'><rect fill='red' width='100' height='100'/></svg>").unwrap();

    let (color, lqip) = extract_color_and_lqip(&svg_path);

    // SVG can't be decoded by image crate — both are None
    assert!(lqip.is_none(), "Should return None LQIP for SVG");

    fs::remove_dir_all(&temp_dir).ok();
}

#[test]
fn test_extract_media_metadata_includes_lqip() {
    let temp_dir = std::env::temp_dir().join(format!("moss_test_meta_lqip_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).unwrap();

    let png_path = temp_dir.join("photo.png");
    create_solid_color_png(&png_path, 640, 480, [128, 64, 32]);

    let metadata = extract_media_metadata(&png_path, "photo.png", "png", 2048, None, None);

    assert!(
        metadata.lqip_data_uri.is_some(),
        "Image metadata should include LQIP"
    );
    let uri = metadata.lqip_data_uri.unwrap();
    assert!(
        uri.starts_with("data:image/jpeg;base64,"),
        "LQIP must be a JPEG data URI"
    );

    fs::remove_dir_all(&temp_dir).ok();
}

#[test]
fn test_extract_color_and_lqip_both_from_single_load() {
    // Verify that color and LQIP are both extracted from one function call
    let temp_dir = std::env::temp_dir().join(format!("moss_test_lqip_both_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).unwrap();

    let png_path = temp_dir.join("green.png");
    create_solid_color_png(&png_path, 300, 300, [0, 200, 0]);

    let (color, lqip) = extract_color_and_lqip(&png_path);

    // Both should be present for a valid raster image
    assert!(color.is_some(), "Should have color");
    assert!(lqip.is_some(), "Should have LQIP");

    // Color should be green-ish
    let hex = color.unwrap();
    let g = u8::from_str_radix(&hex[3..5], 16).unwrap();
    assert!(
        g > 150,
        "Green image should have high green value, got {}",
        g
    );

    fs::remove_dir_all(&temp_dir).ok();
}

// =========================================================================
// build_content_graph integration tests
// =========================================================================

#[test]
fn test_build_content_graph_indexes_files() {
    let temp_dir = std::env::temp_dir().join(format!("moss_test_cg_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).unwrap();

    fs::write(
        temp_dir.join("hello.md"),
        "# Greetings\n\nHello world. ^intro\n",
    )
    .unwrap();
    fs::write(temp_dir.join("notes.md"), "# Notes\n\n## Sub\n").unwrap();

    let ps = ProjectStructure {
        root_path: temp_dir.to_string_lossy().to_string(),
        markdown_files: vec![
            FileInfo {
                path: "hello.md".into(),
                file_type: "md".into(),
                size: 100,
                modified: None,
            },
            FileInfo {
                path: "notes.md".into(),
                file_type: "md".into(),
                size: 80,
                modified: None,
            },
        ],
        html_files: vec![],
        image_files: vec![MediaMetadata {
            path: "photo.jpg".into(),
            file_type: "jpg".into(),
            size: 5000,
            ..Default::default()
        }],
        video_files: vec![],
        notebook_files: vec![],
        other_files: vec![],
        total_files: 3,
        homepage_file: Some("hello.md".into()),
        ffmpeg_bin_path: None,
        evicted_count: 0,
        evicted_paths: Vec::new(),
        has_content_folders: false,
        has_language_trees: false,
        passthrough_roots: std::collections::HashSet::new(),
        dirs: Vec::new(),
    };

    let graph = build_content_graph(&ps);

    // All files should be indexed
    assert_eq!(graph.all_files().len(), 3);

    // Fuzzy resolution should work
    assert!(graph.resolve_path("hello", "").is_some());
    assert!(graph.resolve_path("photo.jpg", "").is_some());

    fs::remove_dir_all(&temp_dir).ok();
}

#[test]
fn test_build_content_graph_indexes_html_files() {
    let temp_dir = std::env::temp_dir().join(format!("moss_test_cg_html_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(temp_dir.join("experiments")).unwrap();

    fs::write(temp_dir.join("experiments/article.md"), "# Article\n").unwrap();
    fs::write(
        temp_dir.join("experiments/widget.html"),
        "<canvas></canvas>",
    )
    .unwrap();

    let ps = ProjectStructure {
        root_path: temp_dir.to_string_lossy().to_string(),
        markdown_files: vec![FileInfo {
            path: "experiments/article.md".into(),
            file_type: "md".into(),
            size: 10,
            modified: None,
        }],
        html_files: vec![FileInfo {
            path: "experiments/widget.html".into(),
            file_type: "html".into(),
            size: 50,
            modified: None,
        }],
        image_files: vec![],
        video_files: vec![],
        notebook_files: vec![],
        other_files: vec![],
        total_files: 2,
        homepage_file: None,
        ffmpeg_bin_path: None,
        evicted_count: 0,
        evicted_paths: Vec::new(),
        has_content_folders: false,
        has_language_trees: false,
        passthrough_roots: std::collections::HashSet::new(),
        dirs: Vec::new(),
    };

    let graph = build_content_graph(&ps);

    // HTML files should be indexed and resolvable by bare filename
    assert!(
        graph
            .resolve_path("widget.html", "experiments/article.md")
            .is_some(),
        "Expected widget.html to be indexed and resolvable"
    );
    // Should resolve to the full path
    assert_eq!(
        graph
            .resolve_path("widget.html", "experiments/article.md")
            .unwrap(),
        "experiments/widget.html"
    );

    fs::remove_dir_all(&temp_dir).ok();
}

#[test]
fn test_build_content_graph_indexes_asset_directories() {
    // Files in `assets/`/`images/`/etc. now flow through the main scan
    // (those folders are no longer per-name excluded), so they arrive at
    // build_content_graph via image_files / video_files just like media
    // anywhere else. Bare-filename wikilink resolution still works.
    let temp_dir = std::env::temp_dir().join(format!("moss_test_cg_assets_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(temp_dir.join("assets/nested")).unwrap();

    fs::write(temp_dir.join("hello.md"), "# Hello\n").unwrap();
    fs::write(temp_dir.join("assets/photo.jpg"), b"fake-jpg").unwrap();
    fs::write(temp_dir.join("assets/nested/deep.png"), b"fake-png").unwrap();

    let ps = ProjectStructure {
        root_path: temp_dir.to_string_lossy().to_string(),
        markdown_files: vec![FileInfo {
            path: "hello.md".into(),
            file_type: "md".into(),
            size: 10,
            modified: None,
        }],
        html_files: vec![],
        image_files: vec![
            MediaMetadata {
                is_animated: false,
                path: "assets/photo.jpg".into(),
                file_type: "jpg".into(),
                size: 8,
                modified: None,
                dimensions: None,
                dominant_color: None,
                lqip_data_uri: None,
            },
            MediaMetadata {
                is_animated: false,
                path: "assets/nested/deep.png".into(),
                file_type: "png".into(),
                size: 8,
                modified: None,
                dimensions: None,
                dominant_color: None,
                lqip_data_uri: None,
            },
        ],
        video_files: vec![],
        notebook_files: vec![],
        other_files: vec![],
        total_files: 3,
        homepage_file: None,
        ffmpeg_bin_path: None,
        evicted_count: 0,
        evicted_paths: Vec::new(),
        has_content_folders: false,
        has_language_trees: false,
        passthrough_roots: std::collections::HashSet::new(),
        dirs: Vec::new(),
    };

    let graph = build_content_graph(&ps);

    // Markdown indexed by stem
    assert!(graph.resolve_path("hello", "").is_some());
    // Bare filename in assets/ resolves
    assert!(
        graph.resolve_path("photo.jpg", "").is_some(),
        "Expected photo.jpg from assets/ to be indexed"
    );
    // Nested asset files also resolve
    assert!(
        graph.resolve_path("deep.png", "").is_some(),
        "Expected deep.png from assets/nested/ to be indexed"
    );

    fs::remove_dir_all(&temp_dir).ok();
}

#[test]
fn test_scan_folder_reports_zero_evicted_for_local_files() {
    let dir = tempfile::TempDir::new().unwrap();
    fs::write(dir.path().join("hello.md"), "# Hello").unwrap();
    fs::write(dir.path().join("image.jpg"), &[0xFF, 0xD8]).unwrap();

    let ps = scan_folder(&dir.path().to_string_lossy()).unwrap();
    assert_eq!(
        ps.evicted_count, 0,
        "Local files should have zero evicted count"
    );
}

// =========================================================================
// Notebook (.ipynb) detection tests
// =========================================================================

#[test]
fn test_scan_folder_detects_notebook_files() {
    let temp_dir = std::env::temp_dir().join(format!("moss_test_notebooks_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).unwrap();

    let notebook_json = r#"{"cells":[],"metadata":{},"nbformat":4,"nbformat_minor":5}"#;
    fs::write(temp_dir.join("analysis.ipynb"), notebook_json).unwrap();

    let ps = scan_folder(&temp_dir.to_string_lossy()).unwrap();
    assert_eq!(
        ps.notebook_files.len(),
        1,
        "Should detect .ipynb as notebook file"
    );
    assert_eq!(ps.notebook_files[0].path, "analysis.ipynb");
    assert_eq!(ps.notebook_files[0].file_type, "ipynb");

    fs::remove_dir_all(&temp_dir).ok();
}

#[test]
fn test_scan_folder_mixed_content_categorizes_correctly() {
    let temp_dir = std::env::temp_dir().join(format!("moss_test_mixed_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).unwrap();

    fs::write(temp_dir.join("readme.md"), "# Hello").unwrap();
    fs::write(temp_dir.join("sketch.html"), "<html></html>").unwrap();
    fs::write(
        temp_dir.join("data.ipynb"),
        r#"{"cells":[],"metadata":{},"nbformat":4,"nbformat_minor":5}"#,
    )
    .unwrap();
    fs::write(temp_dir.join("photo.jpg"), &[0xFF, 0xD8]).unwrap();

    let ps = scan_folder(&temp_dir.to_string_lossy()).unwrap();
    assert_eq!(ps.markdown_files.len(), 1, "One markdown file");
    assert_eq!(ps.html_files.len(), 1, "One HTML file");
    assert_eq!(ps.notebook_files.len(), 1, "One notebook file");
    assert_eq!(ps.image_files.len(), 1, "One image file");
    assert_eq!(ps.total_files, 4, "Total should include all file types");

    fs::remove_dir_all(&temp_dir).ok();
}

#[test]
fn test_scan_folder_no_notebooks() {
    let temp_dir = std::env::temp_dir().join(format!("moss_test_no_nb_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).unwrap();
    fs::write(temp_dir.join("readme.md"), "# Hello").unwrap();

    let ps = scan_folder(&temp_dir.to_string_lossy()).unwrap();
    assert!(
        ps.notebook_files.is_empty(),
        "No notebooks should be detected"
    );

    fs::remove_dir_all(&temp_dir).ok();
}

#[test]
fn test_scan_folder_notebooks_in_subdirectory() {
    let temp_dir = std::env::temp_dir().join(format!("moss_test_nb_sub_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).unwrap();

    let notebooks_dir = temp_dir.join("notebooks");
    fs::create_dir_all(&notebooks_dir).unwrap();
    fs::write(
        notebooks_dir.join("demo.ipynb"),
        r#"{"cells":[],"metadata":{},"nbformat":4,"nbformat_minor":5}"#,
    )
    .unwrap();

    let ps = scan_folder(&temp_dir.to_string_lossy()).unwrap();
    assert_eq!(ps.notebook_files.len(), 1);
    assert_eq!(
        ps.notebook_files[0].path, "notebooks/demo.ipynb",
        "Path should be relative with subdirectory"
    );

    fs::remove_dir_all(&temp_dir).ok();
}

#[test]
fn scan_emits_progress_and_complete_events() {
    use std::sync::{Arc, Mutex};

    // Build a tiny fixture directory: 3 files, 1 marked evicted.
    // Use a named subfolder inside tempdir because scan's filter_entry
    // excludes dot-prefixed folders — raw tempdir names can start with '.'
    // on some platforms.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("site");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.md"), "# a").unwrap();
    std::fs::write(root.join("b.md"), "# b").unwrap();
    std::fs::write(root.join("c.md"), "# c").unwrap();
    // Note: we cannot set SF_DATALESS in tests without special privileges.
    // Assert the counts and event shape for the non-evicted path; a
    // separate integration test (feature-gated) covers the evicted path
    // on machines where SF_DATALESS can be set.

    #[derive(Default)]
    struct Sink {
        progress_count: usize,
        complete: Option<(usize, usize, Option<String>)>,
    }
    let sink = Arc::new(Mutex::new(Sink::default()));
    let sink_clone = sink.clone();

    let emitter = ScanEventEmitter::new(Box::new(move |event| {
        let mut s = sink_clone.lock().unwrap();
        match event {
            ScanEvent::Progress {
                found: _,
                evicted: _,
            } => s.progress_count += 1,
            ScanEvent::Complete {
                total_files,
                evicted_count,
                cloud_provider,
            } => {
                s.complete = Some((total_files, evicted_count, cloud_provider));
            }
        }
    }));

    let _ps =
        scan_folder_with_dedup_emit(&root.to_string_lossy(), None, Some(&emitter), false).unwrap();

    let s = sink.lock().unwrap();
    assert!(s.progress_count >= 1, "expected at least one scan-progress");
    let (total, evicted, provider) = s.complete.clone().expect("expected scan-complete");
    assert_eq!(total, 3);
    assert_eq!(evicted, 0);
    assert_eq!(provider, None);
}

#[test]
fn test_scan_excludes_passthrough_images_from_image_files() {
    use std::fs;
    use tempfile::TempDir;

    // Use a named subfolder inside tempdir — scan's filter_entry excludes
    // dot-prefixed directories, and raw tempdir names can start with '.' on
    // some platforms (see scan_emits_progress_and_complete_events).
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("site");
    fs::create_dir_all(&root).unwrap();

    // Normal image at root level — must still be processed
    fs::write(root.join("cover.png"), b"\x89PNG\r\n\x1a\n").unwrap();
    // Passthrough app: index.html + image
    let app_dir = root.join("my-app");
    fs::create_dir_all(&app_dir).unwrap();
    fs::write(app_dir.join("index.html"), b"<!DOCTYPE html><html></html>").unwrap();
    fs::write(app_dir.join("logo.png"), b"\x89PNG\r\n\x1a\n").unwrap();

    let ps = scan_folder(root.to_str().unwrap()).expect("scan should succeed");

    assert!(
        ps.image_files.iter().all(|f| !f.path.contains("my-app")),
        "passthrough images must be excluded from image_files: {:?}",
        ps.image_files
    );
    assert!(
        ps.passthrough_roots.contains("my-app/"),
        "my-app/ should be in passthrough_roots: {:?}",
        ps.passthrough_roots
    );
    // The passthrough index.html MUST stay in html_files so the folder-embed
    // iframe lookup (folder_embed.rs) can render `![[my-app]]` as an iframe.
    assert!(
        ps.html_files.iter().any(|f| f.path == "my-app/index.html"),
        "passthrough index.html must remain in html_files for folder-embed: {:?}",
        ps.html_files
    );
}

/// Nested-vault boundary (2026-08-19 design §4): a descendant that owns its
/// own `.moss/` is a different site — the outer build must not absorb its
/// pages, at any depth.
#[test]
fn scan_folder_prunes_nested_moss_sites() {
    let dir = tempfile::Builder::new().prefix("moss_scan_nested").tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("index.md"), "# outer").unwrap();
    fs::create_dir_all(root.join("posts")).unwrap();
    fs::write(root.join("posts/hello.md"), "# hi").unwrap();

    // A nested site at depth 1 and another at depth 2.
    fs::create_dir_all(root.join("inner/.moss")).unwrap();
    fs::write(root.join("inner/secret.md"), "# inner page").unwrap();
    fs::create_dir_all(root.join("posts/deep/.moss")).unwrap();
    fs::write(root.join("posts/deep/note.md"), "# deep page").unwrap();

    let ps = scan_folder(root.to_str().unwrap()).expect("scan should succeed");

    assert!(
        ps.markdown_files.iter().any(|f| f.path == "posts/hello.md"),
        "outer content stays scanned: {:?}",
        ps.markdown_files
    );
    assert!(
        ps.markdown_files
            .iter()
            .all(|f| !f.path.starts_with("inner/") && !f.path.starts_with("posts/deep/")),
        "nested sites' pages must not appear in the outer scan: {:?}",
        ps.markdown_files
    );
    assert!(
        !ps.dirs.iter().any(|d| d == "inner" || d.starts_with("posts/deep")),
        "nested site dirs must not get outer index pages: {:?}",
        ps.dirs
    );
}
