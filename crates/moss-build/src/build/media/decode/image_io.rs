//! HEIF pictures through the operating system's ImageIO, on macOS and iOS.
//!
//! moss ships no HEVC decoder of its own: Apple's carries the codec licence.
//! The externs are raw (the `objc2` framework crates would add two
//! dependencies to save nothing), and each owned `CFTypeRef` sits in a [`Cf`]
//! guard so every early return releases what it holds.

use std::ffi::c_void;
use std::path::Path;

type Ref = *const c_void;

#[repr(C)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFURLCreateFromFileSystemRepresentation(allocator: Ref, buffer: *const u8, len: isize, is_directory: u8) -> Ref;
    fn CFDictionaryGetValue(dict: Ref, key: Ref) -> Ref;
    fn CFNumberGetValue(number: Ref, number_type: isize, value: *mut c_void) -> u8;
    fn CFRelease(object: Ref);
}

#[link(name = "ImageIO", kind = "framework")]
extern "C" {
    static kCGImagePropertyPixelWidth: Ref;
    static kCGImagePropertyPixelHeight: Ref;
    static kCGImagePropertyOrientation: Ref;
    fn CGImageSourceCreateWithURL(url: Ref, options: Ref) -> Ref;
    fn CGImageSourceGetPrimaryImageIndex(source: Ref) -> usize;
    fn CGImageSourceCopyPropertiesAtIndex(source: Ref, index: usize, options: Ref) -> Ref;
    fn CGImageSourceCreateImageAtIndex(source: Ref, index: usize, options: Ref) -> Ref;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    static kCGColorSpaceSRGB: Ref;
    fn CGColorSpaceCreateWithName(name: Ref) -> Ref;
    fn CGImageGetWidth(image: Ref) -> usize;
    fn CGImageGetHeight(image: Ref) -> usize;
    fn CGImageGetAlphaInfo(image: Ref) -> u32;
    fn CGBitmapContextCreate(data: *mut c_void, w: usize, h: usize, bits: usize, row: usize, space: Ref, info: u32) -> Ref;
    fn CGContextDrawImage(context: Ref, rect: Rect, image: Ref);
}

const K_CF_NUMBER_SINT64: isize = 4;
const ALPHA_NONE: u32 = 0;
const ALPHA_PREMULTIPLIED_LAST: u32 = 1;
const ALPHA_NONE_SKIP_LAST: u32 = 5;
const ALPHA_NONE_SKIP_FIRST: u32 = 6;
const BYTE_ORDER_32_BIG: u32 = 4 << 12;

/// An owned Core Foundation reference, released on drop.
struct Cf(Ref);

impl Cf {
    fn new(r: Ref) -> Option<Cf> {
        // `then`, not `then_some`: an eager `Cf(null)` is dropped at once, and CFRelease(NULL) traps.
        (!r.is_null()).then(|| Cf(r))
    }
}

impl Drop for Cf {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) }
    }
}

fn number(dict: Ref, key: Ref) -> Option<i64> {
    let value = unsafe { CFDictionaryGetValue(dict, key) };
    if value.is_null() {
        return None;
    }
    let mut out = 0i64;
    let ok = unsafe { CFNumberGetValue(value, K_CF_NUMBER_SINT64, (&mut out as *mut i64).cast()) };
    (ok != 0).then_some(out)
}

fn failed(what: &str) -> image::ImageError {
    image::ImageError::IoError(std::io::Error::other(format!("ImageIO could not {what}")))
}

fn too_large() -> image::ImageError {
    image::ImageError::Limits(image::error::LimitError::from_kind(image::error::LimitErrorKind::InsufficientMemory))
}

/// Decode the primary picture as stored, unrotated, in sRGB, together with the
/// orientation (1–8, EXIF numbering) ImageIO reads from the file's `irot` and
/// `imir`. A HEIF's EXIF orientation repeats that turn, so the caller applies
/// this one and never reads EXIF as well.
///
/// The size is checked against `ceiling` from the header before any pixel
/// buffer exists, and again from the decoded image, in case the header lied.
pub(super) fn decode(path: &Path, ceiling: u64) -> image::ImageResult<(image::DynamicImage, u32)> {
    use std::os::unix::ffi::OsStrExt;
    let bytes = path.as_os_str().as_bytes();
    let url = Cf::new(unsafe { CFURLCreateFromFileSystemRepresentation(std::ptr::null(), bytes.as_ptr(), bytes.len() as isize, 0) })
        .ok_or_else(|| failed("open the file"))?;
    let source = Cf::new(unsafe { CGImageSourceCreateWithURL(url.0, std::ptr::null()) }).ok_or_else(|| failed("open the file"))?;
    // A HEIF may hold several pictures; the page shows the primary one, as `imagesize` reads.
    let primary = unsafe { CGImageSourceGetPrimaryImageIndex(source.0) };
    let properties =
        Cf::new(unsafe { CGImageSourceCopyPropertiesAtIndex(source.0, primary, std::ptr::null()) }).ok_or_else(|| failed("read the header"))?;
    let size = unsafe { (number(properties.0, kCGImagePropertyPixelWidth), number(properties.0, kCGImagePropertyPixelHeight)) };
    let (Some(w), Some(h)) = size else {
        return Err(failed("read the size"));
    };
    let fits = |w: u64, h: u64| w.saturating_mul(h).saturating_mul(4) <= ceiling;
    if w <= 0 || h <= 0 || !fits(w as u64, h as u64) {
        return Err(too_large());
    }
    let orientation = unsafe { number(properties.0, kCGImagePropertyOrientation) }
        .and_then(|o| u32::try_from(o).ok())
        .filter(|o| (1..=8).contains(o))
        .unwrap_or(1);

    let image = Cf::new(unsafe { CGImageSourceCreateImageAtIndex(source.0, primary, std::ptr::null()) }).ok_or_else(|| failed("decode"))?;
    let (w, h) = unsafe { (CGImageGetWidth(image.0), CGImageGetHeight(image.0)) };
    if w == 0 || h == 0 || !fits(w as u64, h as u64) {
        return Err(too_large());
    }
    // A bitmap context cannot hold straight alpha, so a picture with alpha is
    // drawn premultiplied and divided back out below.
    let opaque = matches!(unsafe { CGImageGetAlphaInfo(image.0) }, ALPHA_NONE | ALPHA_NONE_SKIP_LAST | ALPHA_NONE_SKIP_FIRST);
    let info = if opaque { ALPHA_NONE_SKIP_LAST } else { ALPHA_PREMULTIPLIED_LAST } | BYTE_ORDER_32_BIG;
    let mut pixels = vec![0u8; w * h * 4];
    {
        let space = Cf::new(unsafe { CGColorSpaceCreateWithName(kCGColorSpaceSRGB) }).ok_or_else(|| failed("make an sRGB space"))?;
        let context = Cf::new(unsafe { CGBitmapContextCreate(pixels.as_mut_ptr().cast(), w, h, 8, w * 4, space.0, info) })
            .ok_or_else(|| failed("make a bitmap"))?;
        unsafe { CGContextDrawImage(context.0, Rect { x: 0.0, y: 0.0, w: w as f64, h: h as f64 }, image.0) };
    }
    let (w32, h32) = (w as u32, h as u32);
    let decoded = if opaque {
        let rgb: Vec<u8> = pixels.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
        image::RgbImage::from_raw(w32, h32, rgb).map(image::DynamicImage::ImageRgb8)
    } else {
        unpremultiply(&mut pixels);
        image::RgbaImage::from_raw(w32, h32, pixels).map(image::DynamicImage::ImageRgba8)
    };
    Ok((decoded.ok_or_else(|| failed("decode"))?, orientation))
}

fn unpremultiply(rgba: &mut [u8]) {
    for p in rgba.chunks_exact_mut(4) {
        let a = u32::from(p[3]);
        if a == 0 || a == 255 {
            continue;
        }
        for c in &mut p[..3] {
            *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
        }
    }
}
