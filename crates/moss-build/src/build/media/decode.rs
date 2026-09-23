//! The one place `image::ImageReader` is opened for a content-sniffed read.
//!
//! `ImageReader::open` alone picks a decoder from the file's PATH EXTENSION,
//! never its bytes — the `image` crate's own docs on `with_guessed_format`
//! say so. A PNG saved with a `.jpg` extension by some exporter then fails to
//! decode as JPEG, and a caller that used the extension-only path treated
//! that as "not an image": no dimensions, no dominant color, no LQIP,
//! falling back to an 800×600 placeholder box in the built HTML even though
//! the browser displays the image fine (it sniffs content). `with_guessed_format`
//! peeks the first bytes for a real signature before deciding, and one call
//! site in this crate (`build/media/rungs.rs`'s `decode_oriented`, before this
//! module existed) already did that. This module gives every other reader of
//! image bytes the same fix from one place instead of reimplementing it.
//!
//! Two shapes, because callers want different amounts of work done:
//! [`sniff_dimensions`] reads only the header (no pixel buffer, ADR-006's
//! ~1ms budget); [`sniff_decode`] fully decodes, capped against a
//! decompression bomb. Neither applies EXIF orientation — that stays a
//! decision each caller makes for itself (some want it, some don't).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use image::ImageReader;

/// Decompression-bomb ceiling for every full pixel decode in this crate,
/// moved here from `rungs.rs`'s former private copy of the same constant.
/// Defense-in-depth against decompression bombs / wrong-dimension headers:
/// the MegapixelBudget elsewhere bounds *concurrent* decoded pixels, but
/// nothing else caps a *single* decode's allocation — a tiny file declaring
/// enormous dimensions would decode to multiple GB and OOM-kill the process
/// regardless of that budget. 1 GiB ≈ 250 MP of RGBA, orders of magnitude
/// above any real photo, so legitimate images are never rejected.
/// `sniff_dimensions` never allocates a pixel buffer, so it doesn't need
/// this.
const DECODE_ALLOC_CEILING: u64 = 1024 * 1024 * 1024;

/// Open `path`, letting the bytes — not the extension — pick the decoder.
/// Warns (once per path per process) when they disagree.
fn open_sniffed(path: &Path) -> std::io::Result<ImageReader<std::io::BufReader<std::fs::File>>> {
    let reader = ImageReader::open(path)?;
    let declared = reader.format();
    let reader = reader.with_guessed_format()?;
    warn_on_format_mismatch(path, declared, reader.format());
    Ok(reader)
}

/// Read just the header: width/height, no pixel decode (~1ms per file,
/// ADR-006) — safe to call once per scanned image.
pub(crate) fn sniff_dimensions(path: &Path) -> image::ImageResult<(u32, u32)> {
    open_sniffed(path)
        .map_err(image::ImageError::IoError)?
        .into_dimensions()
}

/// Fully decode `path`, pixel buffer capped at [`DECODE_ALLOC_CEILING`]. No
/// EXIF orientation applied — callers that want it (`decode_oriented`,
/// `encode_sized_raster`) apply it themselves right after, since not every
/// caller does (LQIP/dominant-color extraction never has).
pub(crate) fn sniff_decode(path: &Path) -> image::ImageResult<image::DynamicImage> {
    let mut reader = open_sniffed(path).map_err(image::ImageError::IoError)?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(DECODE_ALLOC_CEILING);
    reader.limits(limits);
    reader.decode()
}

/// Paths already warned about this process. A build that touches the same
/// mislabeled file from more than one call site (dimension probe, then LQIP,
/// then WebP encode) must not repeat the line once per site.
fn warned_paths() -> &'static Mutex<HashSet<PathBuf>> {
    static WARNED: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    WARNED.get_or_init(|| Mutex::new(HashSet::new()))
}

fn warn_on_format_mismatch(
    path: &Path,
    declared: Option<image::ImageFormat>,
    sniffed: Option<image::ImageFormat>,
) {
    let (Some(declared), Some(sniffed)) = (declared, sniffed) else {
        return;
    };
    if declared == sniffed
        || !crate::infra::warn_once::should_warn_once(path.to_path_buf(), warned_paths())
    {
        return;
    }
    log::warn!(
        "{}: file extension suggests {:?} but the content looks like {:?} — it may be served with the wrong type; consider renaming it to match.",
        path.display(),
        declared,
        sniffed
    );
}
