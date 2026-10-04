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
    let [r, g, b] = PAPER_RGB;
    tiny_skia::Color::from_rgba8(r, g, b, 0xff)
}

/// The one place the paper colour is written; the rasters and the disc's light
/// fill are both built from it.
const PAPER_RGB: [u8; 3] = [0xfa, 0xf8, 0xf5];


pub struct FaviconAssets {
    pub png_16: PathBuf,
    pub png_32: PathBuf,
    pub png_180: PathBuf,                // apple-touch-icon
}

/// The `viewBox` `icon.svg` ships with on disk. Never rescaled there — see
/// that file's own header — because `mark.css` carries a hand-measured
/// proportion table keyed to this exact box for every OTHER consumer of the
/// mark (the app UI, the inline small-mark copies). This constant exists so
/// [`ground_default_favicon`] can find-and-replace only the `viewBox`, never
/// the path data, in the copy it emits — `icon.svg` on disk is untouched.
const DEFAULT_FAVICON_VIEWBOX_ATTR: &str = r#"viewBox="-647 -373 3145 3145""#;

/// The dark rule `icon.svg` carries; the ground's dark colour is added inside it.
const DEFAULT_FAVICON_DARK_RULE: &str = "@media (prefers-color-scheme:dark){";

/// The round ground the tab icon carries under the mark.
///
/// A tab icon cannot learn the colour of the tab strip it lands on, and bare
/// ink has almost no contrast with some strips (the green tips on a mid-blue
/// one, white ink on a pale one). So the icon brings its own ground: a disc,
/// paper when the browser reports light and near-black when it reports dark,
/// and the ink never touches the strip.
///
/// Measured from the two paths in `icon.svg`: the smallest circle enclosing all
/// the ink is centred at (1023.2, 1042.2) with radius 1175.5. The disc is that
/// circle widened to `r = 1469`, so the ink reaches 80% of the radius: the tips
/// stop 20% short of the edge and the mark has room inside the disc. The cost
/// is a mark 20% smaller than a disc drawn tight around the ink's
/// enclosing circle would give. The icon's `viewBox` is the disc's own box,
/// which is also what crops the three rasters. The geometry test below renders
/// the real mark and fails if a future mark outgrows the disc.
struct RoundGround {
    cx: i32,
    cy: i32,
    r: i32,
    /// Fill when the browser reports light; paper, see [`paper`].
    light: [u8; 3],
    /// Fill when the browser reports dark; the dark `--moss-color-bg`.
    dark: &'static str,
}

const DEFAULT_GROUND: RoundGround = RoundGround {
    cx: 1023,
    cy: 1042,
    r: 1469,
    light: PAPER_RGB,
    dark: "#1c1914",
};

impl RoundGround {
    fn view_box_attr(&self) -> String {
        format!(
            r#"viewBox="{} {} {} {}""#,
            self.cx - self.r,
            self.cy - self.r,
            2 * self.r,
            2 * self.r
        )
    }

    fn circle(&self) -> String {
        format!(
            r##"<circle cx="{}" cy="{}" r="{}" fill="#{:02x}{:02x}{:02x}"/>"##,
            self.cx, self.cy, self.r, self.light[0], self.light[1], self.light[2]
        )
    }
}

/// Turn moss's bundled default favicon into the tab icon moss emits: the mark
/// on its own round ground.
///
/// Three things change, and no path coordinate: the `viewBox` becomes the
/// disc's box, a `<circle>` is inserted before the first path, and the dark
/// rule gains `circle{fill:…}` so the disc follows the browser's colour scheme.
/// The mark's ~2000-unit fidelity floor (see `icon.svg`'s header) is never
/// engaged and `mark_sync_test`'s path-string comparison still holds. Applies
/// ONLY to moss's own default mark, written fresh at favicon-emit time —
/// never to a site-supplied `assets/favicon.svg`, whose look is the author's
/// decision, and never to `icon.svg`/`mark-small.svg`/`mark.css` on disk.
///
/// `svg` is expected to be [`crate::build::page::shell::DEFAULT_FAVICON`]
/// verbatim. A missing anchor means the source file changed without this being
/// updated; that is a bug, caught by the unit test below, which runs against
/// the real constant, so here each missing anchor only warns and that one step
/// is skipped. An ungrounded tab icon is cosmetic; refusing to build the site
/// is not.
pub fn ground_default_favicon(svg: &str) -> String {
    let ground = &DEFAULT_GROUND;
    let mut out = svg.to_string();

    let view_box = ground.view_box_attr();
    if out.contains(DEFAULT_FAVICON_VIEWBOX_ATTR) {
        out = out.replacen(DEFAULT_FAVICON_VIEWBOX_ATTR, &view_box, 1);
    } else {
        log::warn!(
            "default favicon viewBox is not {} — shipping it uncropped; \
             update DEFAULT_FAVICON_VIEWBOX_ATTR",
            DEFAULT_FAVICON_VIEWBOX_ATTR
        );
    }

    if let Some(at) = out.find("<path") {
        out.insert_str(at, &ground.circle());
    } else {
        log::warn!("default favicon has no <path> to put the round ground under — shipping it without one");
    }

    if out.contains(DEFAULT_FAVICON_DARK_RULE) {
        let dark = format!("{}circle{{fill:{}}}", DEFAULT_FAVICON_DARK_RULE, ground.dark);
        out = out.replacen(DEFAULT_FAVICON_DARK_RULE, &dark, 1);
    } else {
        log::warn!(
            "default favicon has no `{}` rule — the round ground will stay light in dark mode",
            DEFAULT_FAVICON_DARK_RULE
        );
    }
    out
}

/// Whether `svg` embeds a raster image via an SVG `<image>` element.
///
/// moss's usvg/resvg are built with `default-features = false, features =
/// ["text", "system-fonts"]` (see `Cargo.toml`) — `resvg`'s default
/// `raster-images` feature (PNG/JPEG/GIF decoding) is deliberately not one
/// of them, so an `<image>` element parses but never draws: the icon
/// rasterizes blank, with only a `usvg::parser::image` warning that
/// [`crate::build::cli_output`]'s renderer-warning policy would otherwise
/// have to attribute after the fact. Checked before parsing, the same way
/// [`crate::build::svg_util::strip_media_queries`] handles the `@media`
/// case: a known cause reported as itself, rather than a symptom muffled
/// into "some usvg warning fired somewhere."
///
/// A tag-boundary match (`<image` followed by whitespace, `>` or `/`), not a
/// bare substring one — `<imageSomething>` is not this element, and no real
/// SVG writes one.
fn embeds_a_raster_image(svg: &str) -> bool {
    let mut rest = svg;
    while let Some(pos) = rest.find("<image") {
        let after = &rest[pos + "<image".len()..];
        match after.as_bytes().first() {
            Some(b) if b.is_ascii_whitespace() || *b == b'>' || *b == b'/' => return true,
            _ => {}
        }
        rest = after;
    }
    false
}

pub fn generate_favicons(source_svg: &Path, output_root: &Path) -> Result<FaviconAssets, FaviconError> {
    let mut paths = Vec::with_capacity(SIZES.len());
    let svg = std::fs::read_to_string(source_svg).map_err(FaviconError::Io)?;

    let embeds_bitmap = embeds_a_raster_image(&svg);
    if embeds_bitmap {
        crate::build::cli_output::log_warn_problem!(
            "assets/favicon.svg embeds a bitmap; moss's SVG renderer cannot \
             decode it, so the icon renders blank; use a pure-vector SVG or a PNG"
        );
    }

    let opt = usvg::Options::default();
    let raster_svg = crate::build::svg_util::strip_media_queries(&svg);
    let tree =
        usvg::Tree::from_str(&raster_svg, &opt).map_err(|e| FaviconError::Svg(e.to_string()))?;

    // Already reported above, precisely — resvg's own generic "decoding was
    // disabled by a build feature" warning would otherwise also fire, once
    // per rasterized size (three times), for the exact same cause. Any
    // OTHER renderer warning (an invalid font-size, say) is left
    // unsuppressed: this SVG is the author's own, so attributing it to this
    // file — the default when nothing suppresses it, see
    // `cli_output::log_line_for` — is exact.
    let _suppress_known_cause = embeds_bitmap.then(crate::build::cli_output::suppress_renderer_warnings);

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
        // that fails `EDEADLK` against a cloud-evicted destination.
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

    /// The emitted default is the mark on its disc: disc viewBox, the circle
    /// under the first path, a dark rule for the disc, and every other byte —
    /// the path data — untouched (`mark_sync_test` compares it byte-for-byte).
    #[test]
    fn ground_default_favicon_adds_the_disc_and_leaves_the_paths_alone() {
        let src = crate::build::page::shell::DEFAULT_FAVICON;
        let out = ground_default_favicon(src);
        let g = &DEFAULT_GROUND;
        assert!(out.contains(r#"viewBox="-446 -427 2938 2938""#), "disc viewBox missing");
        assert!(!out.contains(DEFAULT_FAVICON_VIEWBOX_ATTR), "padded viewBox must not survive");
        let circle = r##"<circle cx="1023" cy="1042" r="1469" fill="#faf8f5"/>"##;
        assert_eq!(g.circle(), circle);
        assert!(out.contains(&format!("{circle}<path")), "circle must sit right before the first path");
        let dark = "circle{fill:#1c1914}";
        let at = out.find(dark).expect("dark rule for the disc missing");
        assert!(
            at > out.find("@media").unwrap() && at < out.find("</style>").unwrap(),
            "the disc's dark fill must live inside the @media rule"
        );
        let stripped = out
            .replacen(&g.view_box_attr(), "", 1)
            .replacen(circle, "", 1)
            .replacen(dark, "", 1);
        assert_eq!(
            stripped,
            src.replacen(DEFAULT_FAVICON_VIEWBOX_ATTR, "", 1),
            "grounding must not touch anything but the viewBox, the circle and the dark rule"
        );
    }

    /// A drifted `icon.svg` must not take the build down: the tab icon is
    /// cosmetic, the build is not. CI catches the drift via the test above,
    /// which runs against the real `DEFAULT_FAVICON`.
    #[test]
    fn ground_default_favicon_passes_an_unexpected_source_through() {
        let foreign = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"></svg>"#;
        assert_eq!(
            ground_default_favicon(foreign),
            foreign,
            "an unrecognized source should ship as it came, not panic"
        );
    }

    /// The icon is round, and the mark fits inside it. The finished icon is
    /// rendered on a transparent pixmap: its four corners stay clear and the
    /// four rim midpoints are covered. Separately the bare mark is rendered in
    /// its own padded box, so the check does not depend on the disc's box, and
    /// every pixel it paints must lie within the disc's radius. A future mark
    /// that grows past the disc fails here rather than shipping clipped.
    #[test]
    fn default_favicon_is_round_and_holds_all_the_ink() {
        let render = |svg: &str, n: u32| {
            let tree = usvg::Tree::from_str(
                &crate::build::svg_util::strip_media_queries(svg),
                &usvg::Options::default(),
            )
            .expect("parse");
            let mut pm = tiny_skia::Pixmap::new(n, n).unwrap();
            let s = n as f32 / tree.size().width();
            resvg::render(&tree, tiny_skia::Transform::from_scale(s, s), &mut pm.as_mut());
            pm
        };
        let src = crate::build::page::shell::DEFAULT_FAVICON;

        let n = 256;
        let icon = render(&ground_default_favicon(src), n);
        let (mid, last) = (n / 2, n - 1);
        for (x, y) in [(0, 0), (last, 0), (0, last), (last, last)] {
            assert_eq!(icon.pixel(x, y).unwrap().alpha(), 0, "corner ({x},{y}) must be transparent");
        }
        // Inside the disc and outside the ink, so opaque only if the disc is there.
        for (x, y) in [(mid, 2), (mid, last - 2), (2, mid), (last - 2, mid)] {
            assert_eq!(icon.pixel(x, y).unwrap().alpha(), 255, "rim point ({x},{y}) must be inside the disc");
        }

        // The padded box `icon.svg` ships with. Asserted first so a drifted
        // source fails as drift, not as a puzzling distance below.
        assert!(
            src.contains(DEFAULT_FAVICON_VIEWBOX_ATTR),
            "icon.svg's viewBox is no longer {DEFAULT_FAVICON_VIEWBOX_ATTR}; update the constant"
        );
        let [box_x, box_y, box_w, _] = DEFAULT_FAVICON_VIEWBOX_ATTR
            .trim_start_matches(r#"viewBox=""#)
            .trim_end_matches('"')
            .split_whitespace()
            .map(|v| v.parse::<f32>().expect("viewBox number"))
            .collect::<Vec<_>>()[..]
        else {
            panic!("viewBox must hold four numbers")
        };
        let n = 1024;
        let mark = render(src, n);
        let unit = box_w / n as f32;
        let g = &DEFAULT_GROUND;
        let mut painted = 0;
        for (i, p) in mark.pixels().iter().enumerate() {
            if p.alpha() == 0 {
                continue;
            }
            painted += 1;
            let x = box_x + ((i as u32 % n) as f32 + 0.5) * unit;
            let y = box_y + ((i as u32 / n) as f32 + 0.5) * unit;
            let d = ((x - g.cx as f32).powi(2) + (y - g.cy as f32).powi(2)).sqrt();
            assert!(d <= g.r as f32, "ink at ({x:.0},{y:.0}) is {d:.0} from the centre, outside the radius {}", g.r);
        }
        assert!(painted > 0, "the mark alone rendered nothing");
    }

    /// The landing site's own `favicon.svg` is generated by
    /// `scripts/generate-landing-locales.mjs` with this same geometry and only
    /// a different dark ground (`#171816`). The two copies of the numbers agree
    /// exactly when this holds.
    #[test]
    fn landing_site_favicon_matches_the_generated_default() {
        let site = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../site/assets/favicon.svg");
        let on_disk = std::fs::read_to_string(&site)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", site.display()));
        let generated = ground_default_favicon(crate::build::page::shell::DEFAULT_FAVICON)
            .replace(DEFAULT_GROUND.dark, "#171816");
        assert_eq!(generated, on_disk);
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
