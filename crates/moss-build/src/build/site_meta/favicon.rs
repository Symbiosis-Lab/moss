//! Rasterize an SVG favicon to PNGs at standard sizes (16, 32, 180).

use std::path::{Path, PathBuf};

const SIZES: &[u32] = &[16, 32, 180];

/// moss's paper: the value `site.css` uses and the one the app-icon plate is
/// filled with, so the mark meets one background wherever it is given one.
/// Every size is given one, because a raster has no say over the ground it is
/// painted on.
///
/// The 180 has always needed it — iOS flattens an icon's alpha against black,
/// so a dark mark on transparency reaches the home screen as a solid black tile
/// (moss's own default favicon measured mean luma 0). The 16 and 32 were left
/// transparent until 2026-08-29, on the reasoning that a plate would sit in a
/// dark tab strip as a lit square. It does. What settled it was measuring the
/// alternative: against Chrome's dark strip (#35363a) the black ink is 1.74:1
/// (unchanged by the leaf swap — colour, not shape, drives contrast) while the
/// green satellite beside it is 2.12:1 (re-measured for the leaf mark's
/// #4f7031, WCAG relative luminance; was 3.22:1 against the ink-drop mark's
/// old #6b8e4e, already stale before this swap), so the mark's second voice
/// outranks its first and the splash reads as a smudge. On the paper both sit
/// at 16:1 on either strip (measured on the ink-drop mark; unverified for the
/// leaf mark — the paper/strip pair this derives from could not be
/// reproduced from the values on hand).
///
/// A colour cannot fix that from one raster. The SVG's dark rule is an `@media`
/// rule and `strip_media_queries` takes it out before rasterizing, because
/// resvg cannot evaluate one — the light arm is what survives by construction.
/// A second, white raster behind `media="(prefers-color-scheme: dark)"` could,
/// but only in browsers that honour `media` on a `rel=icon` link, and the ones
/// that read these files at all are Safari before 26.0, which is exactly where
/// that is undocumented.
fn paper() -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(0xfa, 0xf8, 0xf5, 0xff)
}


pub struct FaviconAssets {
    pub png_16: PathBuf,
    pub png_32: PathBuf,
    pub png_180: PathBuf,                // apple-touch-icon
}

/// The `viewBox` `icon.svg` ships with on disk. Never rescaled there — see
/// that file's own header — because `mark.css` carries a hand-measured
/// proportion table keyed to this exact box for every OTHER consumer of the
/// mark (the app UI, the inline small-mark copies). This constant exists so
/// [`tighten_default_favicon_viewbox`] can find-and-replace only the
/// `viewBox`, never the path data, in the copy it emits — `icon.svg` on disk
/// is untouched.
const DEFAULT_FAVICON_VIEWBOX_ATTR: &str = r#"viewBox="-647 -373 3145 3145""#;

/// The tab-only crop. Measured 2026-09-14 by rendering `icon.svg` at 1:1
/// scale (3145x3145) and scanning alpha for the ink's pixel bounding box:
/// x=[9,1776] y=[8,2111] within the shipped box — about 56% of the width and
/// 67% of the height, which read as a barely-there speck in a browser tab
/// (reported on a real site, 2026-09-14). This
/// box centers that bbox with an even ~12%-of-side margin on all four edges:
/// `x0 = cx - side/2 - margin`, `side = max(bbox_w, bbox_h)`, so the ink
/// fills roughly 68-80% of the canvas instead of ~38% of its area.
///
/// Every other consumer of the mark (app UI, inline small-mark copies) sizes
/// it explicitly and already compensates for the padded box via `mark.css`'s
/// proportion table — re-fitting the shared `icon.svg` would silently
/// invalidate that table with nothing to catch it. The favicon is the only
/// consumer that cannot compensate, because the browser paints the whole box
/// into the tab strip verbatim; that is the real distinction, not a
/// workaround around it.
const DEFAULT_FAVICON_TIGHT_VIEWBOX_ATTR: &str = r#"viewBox="-411 -244 2608 2608""#;

/// Tighten moss's bundled default favicon glyph to its ink bounding box.
///
/// Only the `viewBox` attribute changes; every path coordinate is untouched,
/// so the mark's ~2000-unit fidelity floor (see `icon.svg`'s header) is never
/// engaged and `mark_sync_test`'s path-string comparison still holds. Applies
/// ONLY to moss's own default mark, written fresh at favicon-emit time —
/// never to a vault-supplied `assets/favicon.svg`, whose padding is the
/// author's decision, and never to `icon.svg`/`mark-small.svg`/`mark.css` on
/// disk, which stay exactly as every other mark consumer expects them.
///
/// `svg` is expected to be [`crate::build::page::shell::DEFAULT_FAVICON`]
/// verbatim. A mismatched `viewBox` means the source file changed without
/// this crop being re-measured; that is a bug, but it is caught by the unit
/// test below, which runs against the real constant, so here it only warns
/// and returns the SVG untightened. A padded tab icon is cosmetic; refusing
/// to build the site is not.
pub fn tighten_default_favicon_viewbox(svg: &str) -> String {
    if !svg.contains(DEFAULT_FAVICON_VIEWBOX_ATTR) {
        // Fail open, like the rasterization step below: a padded tab icon is a
        // cosmetic regression, a failed build is the whole site. The unit test
        // runs this against the real DEFAULT_FAVICON, so a drifted icon.svg is
        // caught in CI rather than here.
        log::warn!(
            "default favicon viewBox is not the measured {} — shipping it untightened; \
             re-run the ink-bbox measurement and update DEFAULT_FAVICON_TIGHT_VIEWBOX_ATTR",
            DEFAULT_FAVICON_VIEWBOX_ATTR
        );
        return svg.to_string();
    }
    svg.replacen(DEFAULT_FAVICON_VIEWBOX_ATTR, DEFAULT_FAVICON_TIGHT_VIEWBOX_ATTR, 1)
}

pub fn generate_favicons(source_svg: &Path, output_root: &Path) -> Result<FaviconAssets, FaviconError> {
    let mut paths = Vec::with_capacity(SIZES.len());
    let svg = std::fs::read_to_string(source_svg).map_err(FaviconError::Io)?;

    let opt = usvg::Options::default();
    let raster_svg = crate::build::svg_util::strip_media_queries(&svg);
    let tree =
        usvg::Tree::from_str(&raster_svg, &opt).map_err(|e| FaviconError::Svg(e.to_string()))?;

    for &size in SIZES {
        let mut pixmap = tiny_skia::Pixmap::new(size, size)
            .ok_or_else(|| FaviconError::Encode("pixmap alloc".into()))?;
        pixmap.fill(paper());
        let scale = size as f32 / tree.size().width();
        resvg::render(
            &tree,
            tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );

        // Encode to memory and write through `io_utils`, rather than
        // `pixmap.save_png(&out_path)`. `save_png` does its own `File::create`
        // — an `O_TRUNC` open that a grep for `fs::write` would never find, and
        // that fails `EDEADLK` against a cloud-evicted destination (ADR-043).
        // Third-party APIs that take a path and write it themselves are the
        // blind spot in any call-site-shaped audit.
        let png = pixmap.encode_png().map_err(|e| FaviconError::Encode(e.to_string()))?;
        let out_path = output_root.join(format!("favicon-{}.png", size));
        crate::build::io_utils::write_output(&out_path, &png).map_err(FaviconError::Io)?;
        paths.push(out_path);
    }

    Ok(FaviconAssets {
        png_16: paths[0].clone(),
        png_32: paths[1].clone(),
        png_180: paths[2].clone(),
    })
}

#[derive(Debug)]
pub enum FaviconError {
    Io(std::io::Error),
    Svg(String),
    Encode(String),
}

impl std::fmt::Display for FaviconError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FaviconError::Io(e) => write!(f, "io: {}", e),
            FaviconError::Svg(e) => write!(f, "svg: {}", e),
            FaviconError::Encode(e) => write!(f, "encode: {}", e),
        }
    }
}

impl std::error::Error for FaviconError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_png(path: &Path, expected_size: u32) {
        let bytes = std::fs::read(path).expect("read png");
        assert_eq!(&bytes[..8], &[137, 80, 78, 71, 13, 10, 26, 10], "PNG signature");
        let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        assert_eq!(width, expected_size);
        assert_eq!(height, expected_size);
    }

    /// A real site's too-small favicon glyph (moss's 2026-09-14 fix):
    /// tightening the default mark's viewBox must touch only that attribute,
    /// never the path data — path coordinates are the ~2000-unit fidelity
    /// floor `mark_sync_test` compares byte-for-byte.
    #[test]
    fn tighten_default_favicon_viewbox_only_changes_the_viewbox() {
        let tightened = tighten_default_favicon_viewbox(
            crate::build::page::shell::DEFAULT_FAVICON,
        );
        assert!(
            tightened.contains(DEFAULT_FAVICON_TIGHT_VIEWBOX_ATTR),
            "expected the tightened viewBox; got: {}",
            &tightened[..200.min(tightened.len())]
        );
        assert!(
            !tightened.contains(DEFAULT_FAVICON_VIEWBOX_ATTR),
            "the padded viewBox must not survive the crop"
        );
        // Every other byte — the whole path data — must be untouched: the
        // two strings must be identical once each one's own viewBox
        // attribute is removed.
        let original_without_viewbox =
            crate::build::page::shell::DEFAULT_FAVICON.replacen(DEFAULT_FAVICON_VIEWBOX_ATTR, "", 1);
        let tightened_without_viewbox =
            tightened.replacen(DEFAULT_FAVICON_TIGHT_VIEWBOX_ATTR, "", 1);
        assert_eq!(
            original_without_viewbox, tightened_without_viewbox,
            "tightening must not touch anything but the viewBox attribute"
        );
    }

    /// A drifted `icon.svg` must not take the build down: the tab icon is
    /// cosmetic, the build is not. CI catches the drift via the test above,
    /// which runs against the real `DEFAULT_FAVICON`.
    #[test]
    fn tighten_default_favicon_viewbox_passes_an_unexpected_source_through() {
        let foreign = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"></svg>"#;
        assert_eq!(
            tighten_default_favicon_viewbox(foreign),
            foreign,
            "an unrecognized viewBox should ship untightened, not panic"
        );
    }

    #[test]
    fn rasterizes_provided_svg() {
        let dir = tempfile::tempdir().expect("tempdir");
        let svg_path = dir.path().join("favicon.svg");
        std::fs::write(
            &svg_path,
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><circle cx="50" cy="50" r="40" fill="red"/></svg>"#,
        )
        .unwrap();

        let assets = generate_favicons(&svg_path, dir.path()).expect("generate");

        assert_png(&assets.png_16, 16);
        assert_png(&assets.png_32, 32);
        assert_png(&assets.png_180, 180);
    }

    /// Every raster is opaque, and the two grounds it can meet are why: iOS
    /// flattens the 180's alpha against black, and a tab strip is light or dark
    /// with no way for the file to know which. A transparent corner here means
    /// the plate stopped being filled, which shows up as a mark that vanishes
    /// into one of the two rather than as a failure.
    #[test]
    fn every_favicon_raster_is_opaque() {
        let dir = tempfile::tempdir().expect("tempdir");
        let svg_path = dir.path().join("favicon.svg");
        // A small mark on a large canvas, so the corners are art-free and any
        // opacity there comes from the plate rather than from the drawing.
        std::fs::write(
            &svg_path,
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><circle cx="50" cy="50" r="20" fill="black"/></svg>"#,
        )
        .unwrap();

        let assets = generate_favicons(&svg_path, dir.path()).expect("generate");

        let read = |p: &Path| {
            tiny_skia::Pixmap::decode_png(&std::fs::read(p).expect("read png")).expect("decode png")
        };
        for (label, path) in [
            ("16", &assets.png_16),
            ("32", &assets.png_32),
            ("180", &assets.png_180),
        ] {
            assert!(
                read(path).pixels().iter().all(|p| p.alpha() == 255),
                "favicon-{} must be fully opaque; a transparent pixel means the paper plate was not filled",
                label
            );
        }
    }
}
