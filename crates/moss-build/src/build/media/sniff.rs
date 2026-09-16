//! Header sniffers: bounded reads that classify a source file without
//! decoding it — animated GIF/WebP, CMYK JPEG. Each reads a few KB from the
//! head of the file and nothing more, so asking is cheap and never pulls a
//! whole image into memory.
//!
//! Extracted from `build/media/image.rs` on 2026-09-05, a pure move, per
//! MIGRATION-STATE's image.rs debt row (a feature touching that file extracts
//! before it lands). `should_skip` and the deployed-raster fallback are the
//! consumers.

use std::fs;
use std::path::Path;

/// Read at most `max_bytes` from the head of a file. Used by all header
/// sniffers below so we never read more than a few KB.
fn read_head(path: &Path, max_bytes: usize) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut file = fs::File::open(path).ok()?;
    let mut buf = vec![0u8; max_bytes];
    let n = file.read(&mut buf).ok()?;
    buf.truncate(n);
    Some(buf)
}

/// Detect whether a GIF contains multiple frames (i.e. is animated).
///
/// Reads at most 4 KB and counts "Image Descriptor" blocks (tag 0x2C).
/// Two or more → animated. This is a heuristic — a truly single-frame GIF
/// can technically contain the Netscape application extension but we don't
/// rely on that. For reliability we count frame markers.
pub(crate) fn is_animated_gif(path: &Path) -> bool {
    let buf = match read_head(path, 4096) {
        Some(b) => b,
        None => return false,
    };
    // GIF signature: "GIF87a" or "GIF89a"
    if buf.len() < 6 || &buf[0..3] != b"GIF" {
        return false;
    }
    // Count Image Descriptor introducer bytes (0x2C). This is a simple
    // heuristic and is sufficient for our purposes because a single-frame
    // GIF has exactly one 0x2C, and animated GIFs have one per frame.
    let count = buf.iter().filter(|&&b| b == 0x2C).count();
    count >= 2
}

/// Detect whether a WebP contains an ANIM chunk (animated WebP).
///
/// Reads at most 4 KB, validates RIFF + WEBP header, then scans for the
/// ASCII bytes "ANIM" which mark the start of the animation parameters
/// chunk. Static VP8/VP8L files do not contain this chunk.
pub(crate) fn is_animated_webp(path: &Path) -> bool {
    let buf = match read_head(path, 4096) {
        Some(b) => b,
        None => return false,
    };
    if buf.len() < 12 {
        return false;
    }
    if &buf[0..4] != b"RIFF" || &buf[8..12] != b"WEBP" {
        return false;
    }
    // Scan the remainder for the ANIM chunk ID. Safe substring search.
    buf.windows(4).any(|w| w == b"ANIM")
}

/// Detect whether a JPEG is CMYK (4-component color).
///
/// Reads at most 4 KB and walks JPEG markers looking for a Start-of-Frame
/// (SOF0/SOF1/SOF2, markers 0xFFC0–0xFFC2). The byte following the marker
/// length tells us the component count; 4 indicates CMYK (or YCCK).
pub(crate) fn is_cmyk_jpeg(path: &Path) -> bool {
    let buf = match read_head(path, 4096) {
        Some(b) => b,
        None => return false,
    };
    if buf.len() < 4 || buf[0] != 0xFF || buf[1] != 0xD8 {
        return false; // Not a JPEG
    }
    // Walk markers starting at offset 2.
    let mut i = 2usize;
    while i + 3 < buf.len() {
        if buf[i] != 0xFF {
            return false;
        }
        let marker = buf[i + 1];
        // Skip padding fill bytes (0xFF 0xFF...)
        if marker == 0xFF {
            i += 1;
            continue;
        }
        // Stand-alone markers (no length): SOI, EOI, RST0–RST7.
        if matches!(marker, 0xD0..=0xD9) {
            i += 2;
            continue;
        }
        // All other markers have a 2-byte length field after them.
        if i + 3 >= buf.len() {
            return false;
        }
        let len = ((buf[i + 2] as usize) << 8) | (buf[i + 3] as usize);
        // SOF0 (0xC0), SOF1 (0xC1), SOF2 (0xC2) — others rare / progressive extensions.
        if matches!(marker, 0xC0..=0xC2) {
            // SOF layout: marker(2) + length(2) + precision(1) + height(2) + width(2) + components(1) + ...
            let comp_idx = i + 2 + 5 + 2; // = i + 9
            if comp_idx < buf.len() {
                return buf[comp_idx] == 4;
            }
            return false;
        }
        i += 2 + len;
    }
    false
}

#[cfg(test)]
#[path = "sniff_tests.rs"]
mod tests;
