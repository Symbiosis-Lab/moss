//! Header sniffers: bounded reads that classify a source file without
//! decoding it — animated GIF/WebP, a JPEG's CMYK profile. None reads more
//! than a few KB, so asking is cheap and never pulls a whole image into
//! memory. `should_skip` and the deployed-raster fallback are the consumers.

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

/// Whether a JPEG embeds a CMYK colour profile. A browser colour-manages
/// such a file through its print profile, which a plain CMYK-to-RGB decode
/// does not match, so it ships as the original. A CMYK JPEG without one
/// is decoded by plain inversion, as libjpeg, Chromium and Firefox draw it
/// (Safari assumes Generic CMYK), and is encoded like any other.
///
/// Walks the marker segments by seeking, since a profile segment (or EXIF)
/// can run to hundreds of KB, and stops at the first frame or scan header,
/// which every profile precedes.
pub(crate) fn has_cmyk_profile(path: &Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = fs::File::open(path) else { return false };
    let mut byte = [0u8; 1];
    let mut soi = [0u8; 2];
    if file.read_exact(&mut soi).is_err() || soi != [0xFF, 0xD8] {
        return false;
    }
    loop {
        if file.read_exact(&mut byte).is_err() || byte[0] != 0xFF {
            return false;
        }
        let code = loop {
            if file.read_exact(&mut byte).is_err() {
                return false;
            }
            if byte[0] != 0xFF {
                break byte[0];
            }
        };
        if matches!(code, 0x01 | 0xD0..=0xD7) {
            continue;
        }
        if matches!(code, 0xC0..=0xCF | 0xD9 | 0xDA) && !matches!(code, 0xC4 | 0xC8 | 0xCC) {
            return false;
        }
        let mut len = [0u8; 2];
        if file.read_exact(&mut len).is_err() {
            return false;
        }
        let mut rest = i64::from(u16::from_be_bytes(len)) - 2;
        // APP2 `ICC_PROFILE\0`, chunk number, chunk count, then the profile,
        // whose first chunk holds the header with its colour space at 16.
        let mut icc = [0u8; 34];
        if code == 0xE2 && rest >= 34 {
            if file.read_exact(&mut icc).is_err() {
                return false;
            }
            if &icc[..12] == b"ICC_PROFILE\0" && icc[12] == 1 {
                return &icc[30..34] == b"CMYK";
            }
            rest -= 34;
        }
        if rest < 0 || file.seek(SeekFrom::Current(rest)).is_err() {
            return false;
        }
    }
}

/// Whether the file is a HEIF picture (HEIC and kin), read from its `ftyp`
/// brands rather than its extension. AVIF shares the container and often lists
/// `mif1`/`msf1` among its compatible brands, so an `avif`/`avis` brand
/// anywhere decides first: AVIF stays with the image crate.
pub(crate) fn is_heif(path: &Path) -> bool {
    let Some(head) = read_head(path, 64) else { return false };
    heif_brands(&head)
}

fn heif_brands(head: &[u8]) -> bool {
    if head.len() < 16 || &head[4..8] != b"ftyp" {
        return false;
    }
    let size = (u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize).min(head.len());
    // The major brand, then the compatible brands after the minor version.
    let brands: Vec<&[u8]> = std::iter::once(&head[8..12])
        .chain(head.get(16..size).unwrap_or_default().chunks_exact(4))
        .collect();
    if brands.iter().any(|b| matches!(*b, b"avif" | b"avis")) {
        return false;
    }
    brands.iter().any(|b| matches!(*b, b"heic" | b"heix" | b"heim" | b"heis" | b"hevc" | b"hevx" | b"mif1" | b"msf1"))
}

#[cfg(test)]
#[path = "sniff_tests.rs"]
mod tests;
