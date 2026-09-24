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
//!   `--moss-reading-size`, and `content_width: wide` pages use
//!   calc(50 × reading-size). A scaled or wide page therefore gets slightly
//!   suboptimal (never broken) fetches, same framing as theme overrides above.
//! - nav/content breakpoint: 48rem (see .claude/CLAUDE.md § "Navigation
//!   Responsive Breakpoints")
//! - `.moss-grid` runs 1–4 columns via data-columns within the content/wide
//!   column; a horizontal site collapses to 1 column below 768px, a
//!   vertical one never does (site.css) — see `sizes_for_grid_cell`'s
//!   `auto,` lead, which reads the real width instead of guessing it
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
//! Engine support constrains the spelling. WebKit (26, 2026-09) drops a
//! source-size entry that uses `min()` or `clamp()` and falls through to the
//! next one — ultimately `100vw` — while Chromium and Firefox evaluate them;
//! `calc()` with viewport units works in all three. The `min()` values below
//! degrade to `100vw` there, an over-fetch; a new value must not rely on
//! `min()`/`clamp()` to avoid an understatement.

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
pub const SIZES_BODY: &str = "(min-width: 48rem) 47.25rem, 100vw";

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
/// `loading="lazy"` (the HTML spec's own constraint — an eager, high-
/// fetchpriority first card falls through to it unconditionally, same as
/// an engine that doesn't support `auto` yet, e.g. WebKit as of 2026-09).
/// It stays a viewport-width guess, and stays safe only because a
/// generic <picture> `sizes` overstatement over-fetches rather than
/// blurs — see the module doc's caveat on theme overrides.
pub const SIZES_CARD: &str = "auto, (min-width: 48rem) 24rem, 100vw";

/// Gallery thumbnails: 2–3 across on desktop.
pub const SIZES_GALLERY: &str = "(min-width: 48rem) 33vw, 100vw";

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

/// The `sizes=` value for an image inside a `.moss-grid` cell: the cell
/// track is the grid's band width divided by its column count (gaps are
/// ignored — a slight, safe over-declaration). The band is the content
/// column, or the escape band when the grid carries `data-width`.
///
/// Leads with `auto,` for the same reason as [`SIZES_CARD`]: `:::grid N`
/// keeps N tracks at every width on a vertical site but collapses to 1
/// below 768px on a horizontal one (site.css), so no static formula fits
/// both without knowing the page's typesetting — which this contract layer
/// doesn't carry. `auto` sidesteps that by reading the cell's real
/// laid-out width instead. The trailing static value is the pre-`auto`
/// fallback: it only fires for an eager image or an engine without
/// `sizes="auto"` support, so it stays the old always-N-tracks guess,
/// suboptimal on a collapsed horizontal-mobile cell but never broken.
///
/// This exists because the pre-escape mapping declared the CONTENT COLUMN
/// for grid-cell images: a 3-across featured wall emitted
/// `sizes="(min-width: 48rem) 47.25rem, 100vw"`, so the browser fetched the
/// 800w candidate for a ~215px slot — ~12× the pixels needed, per tile.
pub fn sizes_for_grid_cell(columns: u32, data_width: Option<&str>) -> String {
    let band: &str = match data_width {
        Some("wide") => "min(63rem, 100vw)",
        Some("page") => "min(1200px, 100vw)",
        Some("screen") | Some("full") => "100vw",
        // No data-width (or `body`): the content column.
        _ => "min(47.25rem, 100vw)",
    };
    let cols = columns.max(1);
    let fallback = if cols == 1 {
        band.to_string()
    } else {
        format!("calc({band} / {cols})")
    };
    format!("auto, {fallback}")
}

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
/// common viewports; see the module doc for why no `min()` is used. The
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
                    .map(move |w| sizes_for_grid_cell(n, w))
            })
            .collect();
        for s in [SIZES_FULL_BLEED, SIZES_BODY, SIZES_CARD, SIZES_GALLERY, SIZES_WIDE, SIZES_PAGE]
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
            sizes_for_grid_cell(3, None),
            "auto, calc(min(47.25rem, 100vw) / 3)"
        );
        assert_eq!(
            sizes_for_grid_cell(3, Some("page")),
            "auto, calc(min(1200px, 100vw) / 3)"
        );
        // Single column: no calc wrapper.
        assert_eq!(sizes_for_grid_cell(1, Some("screen")), "auto, 100vw");
        // Zero-column defensive clamp.
        assert_eq!(sizes_for_grid_cell(0, None), "auto, min(47.25rem, 100vw)");
    }

    #[test]
    fn vertical_body_names_the_column_height_times_the_aspect() {
        assert_eq!(sizes_vertical_body(2400, 1771), "calc(1.356 * (100vh - 4rem))");
        assert_eq!(sizes_vertical_body(1000, 4000), "calc(0.250 * (100vh - 4rem))");
        assert_eq!(sizes_vertical_body(21969, 950), "calc(23.126 * (100vh - 4rem))");
        // WebKit drops a source-size entry using min()/clamp() (module doc).
        let s = sizes_vertical_body(2400, 1771);
        assert!(!s.contains("min(") && !s.contains("clamp(") && !s.contains("max("), "{s}");
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
}
