//! `sizes=` attribute values for responsive image emission, per render
//! context. Part of the theme-author surface: the values encode the DEFAULT
//! theme's layout (site.css). A theme that changes layout widths gets
//! slightly suboptimal (never broken) fetches; overrides are a future
//! contract extension, not built now (YAGNI).
//!
//! Layout facts these encode (verify against contract/tokens.json
//! (definition) and site.css (overrides) when touching):
//! - content column: `--moss-content-width` = calc(42 * 1.125rem) = 47.25rem
//!   at the DEFAULT reading scale — the reader font-scale control shifts
//!   `--moss-reading-size`, so a scaled page gets slightly suboptimal (never
//!   broken) fetches, same framing as theme overrides above. A page whose
//!   `content_width` widens the column has its values restated once that
//!   width is known — see `sizes_in_page_column`
//! - nav/content breakpoint: 48rem (see .claude/CLAUDE.md § "Navigation
//!   Responsive Breakpoints")
//! - `.moss-grid` runs 1–6 columns via data-columns within the content/wide
//!   column, an author ratio or a `mid` count changing the tracks; a
//!   horizontal site collapses to 1 column below 768px, a vertical one never
//!   does (site.css) — see `sizes_for_grid_cell`
//! - `content_width: screen` widens a page's top-level grids and bands after
//!   their pictures are rendered — see `sizes_in_screen_block`
//! - vertical typesetting: the column is a HEIGHT, not a width — see
//!   [`sizes_vertical_body`]
//!
//! A `sizes=` value is a fetch hint and nothing else. The synthesizer emits
//! the source's `width`/`height` alongside every srcset, and site.css caps
//! images on their logical axes, so the laid-out box comes from those hints
//! and the column, never from the density `sizes=` implies. Overstating the
//! rendered width only over-fetches (a bigger rung than needed); understating
//! it blurs (the browser upscales a smaller rung). Every value here therefore
//! errs on the large side when it cannot be exact.
//!
//! Chromium, Firefox and WebKit all evaluate `min()`, `clamp()` and `calc()`
//! with viewport units in a source size; the render gate `screen-page` pins a
//! `min()` entry in chromium and webkit.

/// Hero images and `data-width="screen|full"` figures: span the viewport
/// (bounded by the 2400px deploy cap).
pub const SIZES_FULL_BLEED: &str = "100vw";

/// `:::hero {.plate}` images (2026-09-11): the plate variant's CSS shows
/// the image whole and never upscales it — render width tracks the
/// image's own delivered resolution, not the viewport. `100vw` would
/// therefore ask the srcset ladder to resolve against viewport width,
/// which under-selects a wide plate (a 7.6:1 handscroll can render near
/// its full deployed width while `sizes=100vw` picks a mid rung — a
/// blurry upscale).
/// So `sizes=` names a fixed value wider than every ladder rung, which
/// always selects the base — the same trade every institutional
/// handscroll viewer makes (serve one high-resolution asset rather than
/// device-tiering the object). The base itself may be wider than 2400px
/// for an elongated image (`asset_paths::deployed_long_edge`); that
/// changes nothing here. `sizes_hero_plate_selects_the_base` pins the
/// value above the top rung.
pub const SIZES_HERO_PLATE: &str = "2400px";

/// A body image tagged `data-aspect="scroll"` (`asset_paths::is_scroll_shape`
/// — a handscroll or hanging scroll): the same trade [`SIZES_HERO_PLATE`]
/// makes, for the same reason. Its CSS (site.css / vertical.css) lets the
/// image render at up to its own delivered resolution — a fixed-height
/// scrolling strip horizontally, the full column height vertically — never
/// fit to the viewport-derived column width [`SIZES_BODY`] assumes. Naming
/// that column width here anyway would ask the srcset ladder to resolve
/// against it, under-selecting a wide scroll (a browser picking the `w800`
/// rung for a source displayed at its own 2400px width is the same blurry-
/// upscale mistake `SIZES_HERO_PLATE`'s doc names). Same value, so the same
/// `sizes_hero_plate_selects_the_base` test covers both.
pub const SIZES_SCROLL: &str = SIZES_HERO_PLATE;

/// `data-width="wide"` figures: the wide band —
/// `min(56 × reading-size, site-max)` = `min(63rem, 1200px)` = 63rem at the
/// default reading scale, clamped to the container (`min(…, 100cqw)` in
/// site.css — declared here as `min(…, 100vw)`, the closest `sizes=` can
/// express; the ≤15px classic-scrollbar delta over-fetches, never blurs).
pub const SIZES_WIDE: &str = "(min-width: 48rem) min(63rem, 100vw), 100vw";

/// `data-width="page"` figures: the `--moss-site-max-width` (1200px) band,
/// clamped to the container exactly like [`SIZES_WIDE`].
pub const SIZES_PAGE: &str = "(min-width: 48rem) min(1200px, 100vw), 100vw";

/// Default body figures/inline images: viewport-wide on small screens, the
/// content column (47.25rem) above the 48rem breakpoint.
///
/// Leads with `auto,` for the reason [`SIZES_CARD`] gives: an image a theme
/// draws small (a cover in a link or a list, an inline icon) is not the column's
/// width, and `auto` reads the laid-out width of a lazy image. The remainder is
/// the full-column fallback for an eager image or an engine without `auto`, so
/// it never understates a picture drawn at the column's width.
pub const SIZES_BODY: &str = "auto, (min-width: 48rem) 47.25rem, 100vw";

/// Folder-card covers and link-preview thumbs: grid cells, ~half column and up.
///
/// Leads with `auto,` (2026-09-15): the media-query fallback below tests
/// viewport WIDTH, but a card cover's rendered width isn't always a
/// function of viewport width at all. Under vertical-rl typesetting the
/// grid's track size comes from `--moss-vertical-column`
/// (`min(38em, 100svh - 4rem)` — viewport HEIGHT), and a summary-card
/// thumbnail (`.moss-cards[data-layout="list"] .moss-card-cover`) stretches
/// to an intrinsic, content-driven cross size that no static formula can
/// name at all. Measured on a real vertical-rl site: section covers ~228px,
/// summary covers ~90–103px, at both 390px and 1280px viewport widths — the
/// media query's condition is simply irrelevant to the actual box.
/// `sizes="auto"` (Chromium 126+, Firefox 150+, per the HTML spec's
/// auto-sizes feature) reads the box's real laid-out width instead of
/// evaluating this list, which is correct in every writing mode and every
/// layout, not just vertical-rl. The comma-separated remainder is the
/// mandatory fallback: `auto` binds only when the `img` carries
/// `loading="lazy"`, so the emitter drops the `auto,` lead from an eager
/// image (Chromium reads an eager `auto,` lead as `100vw` instead of skipping
/// it). WebKit 26.5 honours `auto` on an `<img>` but not on a `<picture>`
/// `<source>`, where the WebP ladder sits, so it reads this remainder too.
/// It stays a viewport-width guess, and stays safe only because a
/// generic <picture> `sizes` overstatement over-fetches rather than
/// blurs — see the module doc's caveat on theme overrides.
pub const SIZES_CARD: &str = "auto, (min-width: 48rem) 24rem, 100vw";

/// Gallery thumbnails: 2–3 across on desktop. Leads with `auto,` for the
/// reason [`SIZES_CARD`] gives: a gallery's track comes from auto-fill,
/// `data-columns` or justified rows, and keeps several across on a phone, so
/// the viewport fallback below overstates a phone thumbnail several times.
pub const SIZES_GALLERY: &str = "auto, (min-width: 48rem) 33vw, 100vw";

/// A gallery with an author count of two or more (`:::gallery N`). Below 48rem
/// site.css keeps it at least two across (tracks floored at 88px), so `50vw`
/// never understates a thumbnail there; it is the value WebKit, which has no
/// `sizes=auto`, reads. A 3-across strip of 110px thumbnails otherwise fetched
/// the top rung on an iPhone.
pub const SIZES_GALLERY_COUNTED: &str = "auto, (min-width: 48rem) 33vw, 50vw";

/// The `sizes=` value for a gallery thumbnail, by the gallery's author count.
pub fn sizes_for_gallery(columns: Option<u32>) -> &'static str {
    match columns {
        Some(n) if n >= 2 => SIZES_GALLERY_COUNTED,
        _ => SIZES_GALLERY,
    }
}

/// The `sizes=` value for a figure carrying a canonical `data-width` token
/// (`body | wide | page | screen`; `full` is canonicalized to `screen` at
/// parse time but accepted here for robustness).
///
/// `body` returns `None` — a body-width figure is the content column, i.e.
/// the caller's context default ([`SIZES_BODY`]), and callers may have a
/// more specific default (e.g. a grid cell) that should win.
pub fn sizes_for_data_width(width: &str) -> Option<&'static str> {
    match width {
        "wide" => Some(SIZES_WIDE),
        "page" => Some(SIZES_PAGE),
        "screen" | "full" => Some(SIZES_FULL_BLEED),
        _ => None,
    }
}

/// The `sizes=` value for an image inside a `.moss-grid` cell: the grid's
/// band times its widest cell's share of it (gaps are ignored — a slight,
/// safe over-declaration). The band is the content column, or the escape band
/// when the grid carries `data-width`. The share is one column of `columns`,
/// or the largest part of an author `ratio` (`5:7` is 7/12), which every cell
/// of the grid then declares.
///
/// `mid` is the grid's column count from 48rem up to 64rem, where site.css
/// lets it replace both `columns` and the ratio. A collage and a scroll row
/// place their cells their own way: the caller passes `None` for both, and
/// two columns for a collage, whose lead tile is half its band.
///
/// Leads with `auto,` for the same reason as [`SIZES_CARD`]. The rest is
/// for an eager image or an engine without `sizes="auto"`: the track above
/// 48rem, and the window below it, where a horizontal site's grid is one
/// column. A vertical site, a collage and a scroll row keep narrower cells
/// there and over-fetch.
///
/// This exists because the pre-escape mapping declared the CONTENT COLUMN
/// for grid-cell images: a 3-across featured wall emitted
/// `sizes="(min-width: 48rem) 47.25rem, 100vw"`, so the browser fetched the
/// 800w candidate for a ~215px slot — ~12× the pixels needed, per tile.
pub fn sizes_for_grid_cell(columns: u32, data_width: Option<&str>, mid: Option<u32>, ratio: Option<&str>) -> String {
    let band: &str = match data_width {
        Some("wide") => "min(63rem, 100vw)",
        Some("page") => "min(1200px, 100vw)",
        Some("screen") | Some("full") => "100vw",
        // No data-width (or `body`): the content column.
        _ => BODY_COLUMN,
    };
    let even = |columns: u32| match columns.max(1) {
        1 => band.to_string(),
        n => format!("calc({band} / {n})"),
    };
    let wide = match ratio.map(widest_share) {
        Some(Some((part, whole))) => format!("calc({band} * {part} / {whole})"),
        // A ratio the stylesheet cannot read leaves the template undefined.
        Some(None) => band.to_string(),
        None => even(columns),
    };
    let mid = mid.map(even).filter(|mid| *mid != wide);
    match mid {
        Some(mid) => format!("auto, (min-width: 64rem) {wide}, (min-width: 48rem) {mid}, 100vw"),
        None if wide == band => format!("auto, {band}"),
        None => format!("auto, (min-width: 48rem) {wide}, 100vw"),
    }
}

/// The largest part of a `1:2:1` ratio and the sum of its parts, or `None`
/// when a part is not a positive number.
fn widest_share(ratio: &str) -> Option<(f64, f64)> {
    let parts: Vec<f64> = ratio.split(':').map(|n| n.trim().parse().ok().filter(|n: &f64| *n > 0.0 && n.is_finite())).collect::<Option<_>>()?;
    Some((parts.iter().copied().fold(0.0, f64::max), parts.iter().sum()))
}

/// The content column as a grid cell's band, clamped to the window.
const BODY_COLUMN: &str = "min(47.25rem, 100vw)";

/// `sizes` as it should read in a top-level grid or band that
/// `content_width: screen` widened to the window after its pictures were
/// rendered against the content column, or `None` when it does not name that
/// column (a card, a gallery, a figure with a width of its own). Left alone,
/// the value understates the picture, and an eager image, or any image where
/// `auto` is not supported, is enlarged from too small a rung.
pub fn sizes_in_screen_block(sizes: &str) -> Option<String> {
    if let Some(lead) = sizes.strip_suffix(SIZES_BODY.trim_start_matches("auto, ")) {
        return Some(format!("{lead}{SIZES_FULL_BLEED}"));
    }
    sizes.contains(BODY_COLUMN).then(|| sizes.replace(BODY_COLUMN, SIZES_FULL_BLEED))
}

/// `sizes` as it should read on a page whose `content_width` widens the
/// column (`wide`, `full`, or `screen`, whose column is the `full` one), or
/// `None` when it does not name the default column or the page keeps it. The
/// pictures were rendered before the page's width was resolved.
pub fn sizes_in_page_column(sizes: &str, content_width: &str) -> Option<String> {
    let column = match content_width {
        // `calc(50 * var(--moss-reading-size))` at the default reading size.
        "wide" => "min(56.25rem, 100vw)",
        "full" | "screen" => "min(1200px, 100vw)",
        _ => return None,
    };
    sizes.contains(DEFAULT_COLUMN).then(|| sizes.replace(BODY_COLUMN, column).replace(DEFAULT_COLUMN, column))
}

/// `--moss-content-width` at the default reading size.
const DEFAULT_COLUMN: &str = "47.25rem";

/// Body images on a `typesetting = "vertical"` page: `calc(A * (100vh - 4rem))`
/// with `A` = the source's width/height, rounded UP to three decimals.
///
/// Under vertical-rl the column is a fixed HEIGHT
/// (`--moss-vertical-column: min(38em, 100svh - 4rem)` in vertical.css), the
/// image's inline size (its height) fills it, and its physical width follows
/// from the aspect ratio: roughly column × A. None of the horizontal values
/// describe that — `SIZES_BODY` names a 47.25rem WIDTH, which asked a
/// 2400×1771 plate rendered ~794px wide for the 800w rung, and a 23:1
/// handscroll laid out thousands of px wide for the same.
///
/// `100vh - 4rem` is an upper bound on the column, not the column: it drops
/// the `38em` term (a theme or the reading-scale control can move `em`, and
/// an understated column would blur) and uses `vh`, which is never smaller
/// than `svh`. Rounding `A` up keeps the bound. The cost is ~28% of width on
/// common viewports. The
/// render gate `vertical-sizes` pins the fetch this produces against the CSS
/// column, so a change to `--moss-vertical-column` that outgrows this bound
/// turns it red.
///
/// Applies to `data-width` figures too: width tokens are inert under
/// vertical typesetting, so such a figure sits in the column like any
/// other body image.
pub fn sizes_vertical_body(width: u32, height: u32) -> String {
    let h = u64::from(height.max(1));
    let milli = (u64::from(width) * 1000).div_ceil(h);
    format!("calc({}.{:03} * (100vh - 4rem))", milli / 1000, milli % 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_strings_are_wellformed() {
        // Every constant must be non-empty and contain no double quotes
        // (they are interpolated into sizes="…"). The HTML spec additionally
        // requires the LAST comma-segment to be unconditional — no LEADING
        // media condition, i.e. it must not start with `(` — though it may
        // still be a `calc(…)` expression, which is why the check below is
        // "does not start with", not "does not contain": the grid samples'
        // trailing fallback segment is a `calc(…)` or bare `min(…)`/`100vw`
        // value, never a media condition. Every media condition's
        // parentheses must still balance.
        let grid_samples: Vec<String> = (1..=4)
            .flat_map(|n| {
                [None, Some("wide"), Some("page"), Some("screen")]
                    .into_iter()
                    .map(move |w| sizes_for_grid_cell(n, w, None, None))
            })
            .collect();
        for s in [SIZES_FULL_BLEED, SIZES_BODY, SIZES_CARD, SIZES_GALLERY, SIZES_GALLERY_COUNTED, SIZES_WIDE, SIZES_PAGE]
            .into_iter()
            .chain(grid_samples.iter().map(String::as_str))
        {
            assert!(!s.is_empty());
            assert!(!s.contains('"'));
            let last = s.rsplit(',').next().unwrap().trim();
            assert!(
                !last.starts_with('('),
                "last sizes entry must have no leading media condition: {s}"
            );
            let mut depth: i32 = 0;
            for c in s.chars() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        assert!(depth >= 0, "unbalanced parentheses: {s}");
                    }
                    _ => {}
                }
            }
            assert_eq!(depth, 0, "unbalanced parentheses: {s}");
        }
    }

    #[test]
    fn contexts_whose_width_layout_decides_lead_with_auto() {
        // A lazy image in these contexts has a laid-out width no viewport
        // formula names; without `auto` a 110px gallery thumb on a phone
        // resolves `100vw` and fetches the top rung.
        for s in [SIZES_BODY, SIZES_CARD, SIZES_GALLERY, SIZES_GALLERY_COUNTED] {
            assert!(s.starts_with("auto, "), "{s}");
        }
    }

    #[test]
    fn data_width_mapping() {
        assert_eq!(sizes_for_data_width("wide"), Some(SIZES_WIDE));
        assert_eq!(sizes_for_data_width("page"), Some(SIZES_PAGE));
        assert_eq!(sizes_for_data_width("screen"), Some(SIZES_FULL_BLEED));
        assert_eq!(sizes_for_data_width("full"), Some(SIZES_FULL_BLEED));
        // body = the caller's context default, not a fixed value.
        assert_eq!(sizes_for_data_width("body"), None);
        assert_eq!(sizes_for_data_width("55%"), None);
    }

    #[test]
    fn grid_cell_declares_cell_not_column() {
        // The motivating bug: a 3-across grid cell must declare ~band/3,
        // not the full content column, once past the `auto,` lead — the
        // fallback that fires when `auto` doesn't apply.
        assert_eq!(
            sizes_for_grid_cell(3, None, None, None),
            "auto, (min-width: 48rem) calc(min(47.25rem, 100vw) / 3), 100vw"
        );
        assert_eq!(
            sizes_for_grid_cell(3, Some("page"), None, None),
            "auto, (min-width: 48rem) calc(min(1200px, 100vw) / 3), 100vw"
        );
        // Single column: no calc wrapper.
        assert_eq!(sizes_for_grid_cell(1, Some("screen"), None, None), "auto, 100vw");
        // Zero-column defensive clamp.
        assert_eq!(sizes_for_grid_cell(0, None, None, None), "auto, min(47.25rem, 100vw)");
    }

    #[test]
    fn a_ratio_grid_declares_its_widest_cell() {
        assert_eq!(
            sizes_for_grid_cell(2, Some("screen"), None, Some("5:7")),
            "auto, (min-width: 48rem) calc(100vw * 7 / 12), 100vw"
        );
        assert_eq!(
            sizes_for_grid_cell(3, None, None, Some("1 : 2.5 : 1")),
            "auto, (min-width: 48rem) calc(min(47.25rem, 100vw) * 2.5 / 4.5), 100vw"
        );
        // Unreadable: the whole band, which no cell exceeds.
        for ratio in ["1:x", "0:1", "1:-2", ""] {
            assert_eq!(sizes_for_grid_cell(2, None, None, Some(ratio)), "auto, min(47.25rem, 100vw)", "{ratio}");
        }
    }

    #[test]
    fn a_mid_count_declares_its_cell_up_to_64rem() {
        assert_eq!(
            sizes_for_grid_cell(4, Some("page"), Some(2), None),
            "auto, (min-width: 64rem) calc(min(1200px, 100vw) / 4), (min-width: 48rem) calc(min(1200px, 100vw) / 2), 100vw"
        );
        // `mid` replaces the ratio in its range.
        assert_eq!(
            sizes_for_grid_cell(2, Some("screen"), Some(1), Some("1:3")),
            "auto, (min-width: 64rem) calc(100vw * 3 / 4), (min-width: 48rem) 100vw, 100vw"
        );
        assert_eq!(sizes_for_grid_cell(3, None, Some(3), None), sizes_for_grid_cell(3, None, None, None));
    }

    #[test]
    fn a_wider_page_column_replaces_the_default_one() {
        assert_eq!(
            sizes_in_page_column(SIZES_BODY, "wide").as_deref(),
            Some("auto, (min-width: 48rem) min(56.25rem, 100vw), 100vw")
        );
        assert_eq!(
            sizes_in_page_column(&sizes_for_grid_cell(2, None, Some(1), Some("5:7")), "screen").as_deref(),
            Some("auto, (min-width: 64rem) calc(min(1200px, 100vw) * 7 / 12), (min-width: 48rem) min(1200px, 100vw), 100vw")
        );
        assert_eq!(sizes_in_page_column(SIZES_BODY, "full"), sizes_in_page_column(SIZES_BODY, "screen"));
        assert_eq!(sizes_in_page_column(SIZES_BODY, "body"), None);
        for sizes in [SIZES_CARD, SIZES_GALLERY, SIZES_WIDE, SIZES_PAGE, SIZES_FULL_BLEED] {
            assert_eq!(sizes_in_page_column(sizes, "full"), None, "{sizes}");
        }
    }

    #[test]
    fn vertical_body_names_the_column_height_times_the_aspect() {
        assert_eq!(sizes_vertical_body(2400, 1771), "calc(1.356 * (100vh - 4rem))");
        assert_eq!(sizes_vertical_body(1000, 4000), "calc(0.250 * (100vh - 4rem))");
        assert_eq!(sizes_vertical_body(21969, 950), "calc(23.126 * (100vh - 4rem))");
    }

    #[test]
    fn vertical_body_never_understates_the_aspect() {
        // An understated coefficient fetches a smaller rung than the
        // rendered width needs, i.e. blurs.
        for w in (1..=6000u32).step_by(37) {
            for h in (1..=6000u32).step_by(41) {
                let s = sizes_vertical_body(w, h);
                // Integer thousandths: `9.200` → 9200 (f64 would round
                // an exact 9.2 × 165 below 1518).
                let milli: u64 = s
                    .strip_prefix("calc(")
                    .and_then(|r| r.split(' ').next())
                    .unwrap()
                    .replace('.', "")
                    .parse()
                    .unwrap();
                assert!(milli * u64::from(h) >= u64::from(w) * 1000, "{w}x{h} -> {s}");
            }
        }
    }

    #[test]
    fn sizes_hero_plate_selects_the_base() {
        // SIZES_HERO_PLATE must always select the base srcset candidate:
        // wider than every ladder rung, and at least the long-edge cap so a
        // base at the cap is taken at 1x. A ladder or cap change must move
        // it rather than silently starve a plate hero.
        let px: u32 = SIZES_HERO_PLATE
            .strip_suffix("px")
            .expect("SIZES_HERO_PLATE must be a bare px length, not a media-query list")
            .parse()
            .expect("SIZES_HERO_PLATE must be a plain integer px value");
        assert!(
            px >= crate::asset_paths::DEPLOY_MAX_EDGE,
            "SIZES_HERO_PLATE ({px}px) must be >= DEPLOY_MAX_EDGE ({}px) or a wide plate \
             under-selects its srcset rung",
            crate::asset_paths::DEPLOY_MAX_EDGE,
        );
        let top_rung = *crate::asset_paths::LADDER.last().unwrap();
        assert!(px > top_rung, "SIZES_HERO_PLATE ({px}px) must exceed the top rung ({top_rung}w)");
    }

    #[test]
    fn a_screen_grid_cell_reads_as_if_written_with_screen() {
        for columns in 1..=4 {
            assert_eq!(
                sizes_in_screen_block(&sizes_for_grid_cell(columns, None, None, None)),
                Some(sizes_for_grid_cell(columns, Some("screen"), None, None))
            );
        }
    }

    #[test]
    fn a_picture_at_the_column_reads_the_window_lazy_or_eager() {
        assert_eq!(sizes_in_screen_block(SIZES_BODY).as_deref(), Some("auto, 100vw"));
        assert_eq!(sizes_in_screen_block("(min-width: 48rem) 47.25rem, 100vw").as_deref(), Some("100vw"));
    }

    #[test]
    fn sizes_that_do_not_name_the_content_column_are_left_alone() {
        for sizes in [SIZES_CARD, SIZES_GALLERY, SIZES_WIDE, SIZES_PAGE, SIZES_FULL_BLEED, SIZES_HERO_PLATE] {
            assert_eq!(sizes_in_screen_block(sizes), None, "{sizes}");
        }
        assert_eq!(sizes_in_screen_block(&sizes_for_grid_cell(2, Some("wide"), None, None)), None);
    }

    /// The `screen-page` render gate fetches these against the real CSS in
    /// Chromium and WebKit, as an eager image carries them. It cannot call
    /// Rust, so it reads this golden. Regenerate with `SNAPSHOTS=overwrite`.
    #[test]
    fn screen_page_render_gate_golden_matches() {
        let eager = |sizes: String| sizes.trim_start_matches("auto, ").to_string();
        let screen = |sizes: &str| eager(sizes_in_screen_block(sizes).unwrap());
        let entries = [
            ("grid2", screen(&sizes_for_grid_cell(2, None, None, None))),
            ("grid3", screen(&sizes_for_grid_cell(3, None, None, None))),
            ("band", screen(SIZES_BODY)),
            ("ratio", screen(&sizes_for_grid_cell(2, None, None, Some("5:7")))),
            ("mid", screen(&sizes_for_grid_cell(4, None, Some(2), None))),
            ("wideColumn", eager(sizes_in_page_column(SIZES_BODY, "wide").unwrap())),
            ("fullColumn", eager(sizes_in_page_column(SIZES_BODY, "full").unwrap())),
            ("card", eager(SIZES_CARD.to_string())),
        ];
        let want = format!("{{\n{}\n}}\n", entries.map(|(name, sizes)| format!("  \"{name}\": \"{sizes}\"")).join(",\n"));
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/render-gates/site/screen-page.golden.json");
        if std::env::var("SNAPSHOTS").as_deref() == Ok("overwrite") {
            std::fs::write(&path, &want).unwrap();
        }
        let have = std::fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(have, want, "stale {}: rerun with SNAPSHOTS=overwrite", path.display());
    }
}
