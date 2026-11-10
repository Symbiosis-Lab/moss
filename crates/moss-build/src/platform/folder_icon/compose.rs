//! The icon itself: this Mac's own folder icon with moss's leaf drawn on its
//! front, the way macOS draws its folder symbols (Downloads, Applications):
//! centred, one deeper shade of the folder's blue, with a faint light edge.
//!
//! The folder art is read from the running system and composed on this
//! machine only; moss ships just its own mark. That also keeps the icon in
//! step with the OS: a new macOS draws a new folder, so the cache is keyed by
//! the OS build and the tag changes with it.

use image::{GrayImage, Luma, Rgba, RgbaImage};
use std::path::{Path, PathBuf};

/// Bump when the recipe below changes, so folders carrying the old icon are
/// redrawn.
const RECIPE: &str = "centred-leaf-1";
// Fractions of the icon canvas, measured on the 1024px representation: the
// mark's longer side, its centre's height (the middle of the folder's front,
// below the tab) and the light edge's drop.
const MARK_FRAC: f32 = 400.0 / 1024.0;
const CENTRE_Y_FRAC: f32 = 570.0 / 1024.0;
const EDGE_FRAC: f32 = 3.0 / 1024.0;
/// The folder front's own blue, deepened; white reads as a sticker there.
const GLYPH: [u8; 3] = [36, 132, 196];
const EDGE_ALPHA: f32 = 0.35;
/// Below this the leaf is a smudge, so the plain folder ships.
const MIN_MARK_DIM: u32 = 32;
/// The icns slots and the pixel size each holds.
const SLOTS: [(&[u8; 4], u32); 11] = [
    (b"icp4", 16), (b"icp5", 32), (b"icp6", 64), (b"ic07", 128), (b"ic08", 256), (b"ic09", 512),
    (b"ic10", 1024), (b"ic11", 32), (b"ic12", 64), (b"ic13", 256), (b"ic14", 512),
];

/// Names the icon this OS and recipe produce; `None` off a readable macOS.
pub(super) fn current_tag() -> Option<String> {
    let plist = std::fs::read_to_string("/System/Library/CoreServices/SystemVersion.plist").ok()?;
    let build = plist_string(&plist, "ProductBuildVersion")?;
    use sha2::{Digest, Sha256};
    let recipe = Sha256::digest(format!("{RECIPE}\n{}", crate::build::page::shell::DEFAULT_FAVICON));
    Some(format!("{build}-{}", recipe[..6].iter().map(|b| format!("{b:02x}")).collect::<String>()))
}

fn plist_string(plist: &str, key: &str) -> Option<String> {
    let after = plist.split(&format!("<key>{key}</key>")).nth(1)?;
    let value = after.split("<string>").nth(1)?.split("</string>").next()?;
    Some(value.trim().to_owned())
}

/// The icon for `tag`, from the per-OS cache or composed now.
pub(super) fn folder_icns(tag: &str) -> Option<Vec<u8>> {
    let dir = crate::infra::app_data::app_data_dir_early()?.join("folder-icon");
    let cache = dir.join(format!("{tag}.icns"));
    if let Some(bytes) = std::fs::read(&cache).ok().filter(|b| is_icns(b)) {
        return Some(bytes);
    }
    // Composing waits up to the dump's timeout; after a failure (no window
    // server, no osascript) every build would wait again, so it is remembered.
    let failed = dir.join(format!("{tag}.failed"));
    let recent = |p: &Path| {
        std::fs::metadata(p).and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().is_ok_and(|age| age.as_secs() < 86_400))
    };
    if recent(&failed) {
        return None;
    }
    let _ = std::fs::create_dir_all(&dir);
    let Some(icns) = compose() else {
        // allow:raw_write an app-data cache, not build output
        let _ = std::fs::write(&failed, b"");
        return None;
    };
    // A name per process: two builds composing at once must not share one.
    let tmp = dir.join(format!("{tag}.{}.tmp", std::process::id()));
    // allow:raw_write an app-data cache, not build output; renamed into place
    if std::fs::write(&tmp, &icns).is_ok() && std::fs::rename(&tmp, &cache).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    Some(icns)
}

/// Whole icns framing: the magic and a total length that matches.
fn is_icns(bytes: &[u8]) -> bool {
    bytes.len() > 8
        && bytes.starts_with(b"icns")
        && u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize == bytes.len()
}

fn compose() -> Option<Vec<u8>> {
    let work = tempfile::tempdir().ok()?;
    let plain = work.path().join("plain");
    std::fs::create_dir(&plain).ok()?;
    let reps = dump_folder_icon(work.path(), &plain)?;
    let mark = mark_alpha();
    let largest = reps.values().next_back()?;
    let mut drawn = std::collections::BTreeMap::new();
    for (_, dim) in SLOTS {
        if drawn.contains_key(&dim) {
            continue;
        }
        let base = reps.get(&dim).cloned().unwrap_or_else(|| {
            image::imageops::resize(largest, dim, dim, image::imageops::FilterType::Lanczos3)
        });
        drawn.insert(dim, png(&compose_rep(base, &mark))?);
    }
    let mut icns = Vec::new();
    for (kind, dim) in SLOTS {
        let body = &drawn[&dim];
        icns.extend_from_slice(kind);
        icns.extend_from_slice(&(8 + body.len() as u32).to_be_bytes());
        icns.extend_from_slice(body);
    }
    let mut out = b"icns".to_vec();
    out.extend_from_slice(&(8 + icns.len() as u32).to_be_bytes());
    out.extend_from_slice(&icns);
    Some(out)
}

/// Every representation of the system's generic folder icon, by pixel size.
/// AppKit runs in a separate `osascript` process, so the CLI never links it.
fn dump_folder_icon(out: &Path, plain_folder: &Path) -> Option<std::collections::BTreeMap<u32, RgbaImage>> {
    const SCRIPT: &str = r#"ObjC.import('AppKit');
function run(argv) {
  var reps = $.NSWorkspace.sharedWorkspace.iconForFile(argv[1]).representations;
  for (var i = 0; i < reps.count; i++) {
    var rep = reps.objectAtIndex(i), w = Number(rep.pixelsWide);
    var img = $.NSImage.alloc.initWithSize($.NSMakeSize(w, w));
    img.addRepresentation(rep);
    $.NSBitmapImageRep.imageRepWithData(img.TIFFRepresentation)
      .representationUsingTypeProperties($.NSBitmapImageFileTypePNG, $())
      .writeToFileAtomically(argv[0] + '/' + w + '.png', true);
  }
}"#;
    let mut child = std::process::Command::new("/usr/bin/osascript")
        .args(["-l", "JavaScript", "-e", SCRIPT])
        .arg(out)
        .arg(plain_folder)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let reps: std::collections::BTreeMap<u32, RgbaImage> = std::fs::read_dir(out)
        .ok()?
        .filter_map(|e| {
            let path: PathBuf = e.ok()?.path();
            let dim: u32 = path.file_stem()?.to_str()?.parse().ok()?;
            let rep = image::open(&path).ok()?.to_rgba8();
            (rep.width() == dim).then_some((dim, rep))
        })
        .collect();
    (!reps.is_empty()).then_some(reps)
}

/// moss's mark as coverage, trimmed to its ink.
pub(super) fn mark_alpha() -> GrayImage {
    const RENDER: u32 = 1200;
    let svg = crate::build::svg_util::strip_media_queries(crate::build::page::shell::DEFAULT_FAVICON);
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default()).expect("the bundled mark parses");
    let mut pixmap = tiny_skia::Pixmap::new(RENDER, RENDER).expect("non-zero size");
    let scale = RENDER as f32 / tree.size().width().max(tree.size().height());
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    let alpha = GrayImage::from_fn(RENDER, RENDER, |x, y| Luma([pixmap.pixel(x, y).map_or(0, |p| p.alpha())]));
    let (mut x0, mut y0, mut x1, mut y1) = (RENDER, RENDER, 0, 0);
    for (x, y, p) in alpha.enumerate_pixels() {
        if p[0] > 0 {
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
    }
    image::imageops::crop_imm(&alpha, x0, y0, x1 + 1 - x0, y1 + 1 - y0).to_image()
}

/// One representation: the folder at `base.width()` pixels with the mark on
/// its front.
pub(super) fn compose_rep(mut base: RgbaImage, mark: &GrayImage) -> RgbaImage {
    let dim = base.width();
    if dim < MIN_MARK_DIM {
        return base;
    }
    // The folder's crisp body, without the soft drop shadow around it, whose
    // inflated box would push the mark off centre.
    let (mut bx0, mut bx1) = (dim, 0);
    for (x, _, p) in base.enumerate_pixels() {
        if p[3] >= 128 {
            (bx0, bx1) = (bx0.min(x), bx1.max(x));
        }
    }
    if bx1 < bx0 {
        return base;
    }
    let side = (MARK_FRAC * dim as f32).round() as u32;
    let fit = side as f32 / mark.width().max(mark.height()) as f32;
    let (w, h) = (((mark.width() as f32 * fit).round() as u32).max(1), ((mark.height() as f32 * fit).round() as u32).max(1));
    let m = image::imageops::resize(mark, w, h, image::imageops::FilterType::Lanczos3);
    let edge = ((EDGE_FRAC * dim as f32).round() as i64).max(1);
    let cx = (bx0 + bx1 + 1) as i64 / 2;
    let cy = (CENTRE_Y_FRAC * dim as f32).round() as i64;
    let (x, y) = (cx - w as i64 / 2, cy - h as i64 / 2);
    let tinted = |rgb: [u8; 3], k: f32| RgbaImage::from_fn(w, h, |i, j| {
        let a = m.get_pixel(i, j)[0] as f32 * k;
        Rgba([rgb[0], rgb[1], rgb[2], a.round() as u8])
    });
    image::imageops::overlay(&mut base, &tinted([255, 255, 255], EDGE_ALPHA), x, y + edge);
    image::imageops::overlay(&mut base, &tinted(GLYPH, 1.0), x, y);
    base
}

fn png(img: &RgbaImage) -> Option<Vec<u8>> {
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}
