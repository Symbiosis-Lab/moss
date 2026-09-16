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
//! - `.moss-grid` runs 1–4 columns via data-columns within the content/wide column

/// Hero images and `data-width="screen|full"` figures: span the viewport
/// (bounded by the 2400px deploy cap).
pub const SIZES_FULL_BLEED: &str = "100vw";

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
pub const SIZES_CARD: &str = "(min-width: 48rem) 24rem, 100vw";

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
/// column, or the escape band when the grid carries `data-width`
/// (ADR-021 Corollary 2).
///
/// Below the 768px collapse breakpoint (`.moss-grid[data-columns]` goes
/// single-column in site.css) the cell is the full column → `100vw`.
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
    if cols == 1 {
        format!("(min-width: 48rem) {band}, 100vw")
    } else {
        format!("(min-width: 48rem) calc({band} / {cols}), 100vw")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_strings_are_wellformed() {
        // Every constant must be non-empty and contain no double quotes
        // (they are interpolated into sizes="…"). The HTML spec additionally
        // requires the LAST comma-segment to be unconditional (no media
        // condition — a bare length), and every media condition's
        // parentheses to balance.
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
            let last = s.rsplit(',').next().unwrap();
            assert!(
                !last.contains('('),
                "last sizes entry must be an unconditional length: {s}"
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
        // not the full content column.
        assert_eq!(
            sizes_for_grid_cell(3, None),
            "(min-width: 48rem) calc(min(47.25rem, 100vw) / 3), 100vw"
        );
        assert_eq!(
            sizes_for_grid_cell(3, Some("page")),
            "(min-width: 48rem) calc(min(1200px, 100vw) / 3), 100vw"
        );
        // Single column: no calc wrapper.
        assert_eq!(
            sizes_for_grid_cell(1, Some("screen")),
            "(min-width: 48rem) 100vw, 100vw"
        );
        // Zero-column defensive clamp.
        assert_eq!(
            sizes_for_grid_cell(0, None),
            "(min-width: 48rem) min(47.25rem, 100vw), 100vw"
        );
    }
}
