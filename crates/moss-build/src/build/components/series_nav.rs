//! Series navigation component for ordered folders
//!
//! Renders prev/next navigation at the bottom of articles that belong to
//! an ordered folder. This creates a "curated journey" experience
//! where readers can navigate through a series in sequence.
//!
//! Two-row layout with unified clickable cards:
//! ```text
//! ┌──────────────────┐  ┌──────────────────┐ <- Row 1: Clickable cards
//! │ ← Previous Title │  │ Next Title →     │
//! └──────────────────┘  └──────────────────┘
//!              Folder Name                  <- Row 2: Folder link
//! ```
//!
//! On mobile (≤600px), cards stack vertically for larger touch targets.

use crate::build::components::date::use_cjk_numerals;
use crate::build::media::cover::html_escape;
use crate::i18n::numerals::to_chinese_numeral;

/// Render series navigation HTML.
///
/// Arrow and title are separate spans so arrow stays fixed when title wraps.
/// The whole `<a>` gets the hover highlight (handled by global CSS).
///
/// # Arguments
/// - `series_name`: the folder's display title
/// - `series_url`: URL to the folder index page
/// - `prev`: optional (title, url) for previous article
/// - `next`: optional (title, url) for next article
/// - `position`: optional (1-based position, total) — where this page sits in
///   the reading order.
/// - `typesetting`: the page's resolved typesetting (`"vertical"` / `"horizontal"`
///   / `None`) — same predicate [`use_cjk_numerals`] uses for dates and
///   [`crate::i18n::article_count_label`] uses for card counts, so the position
///   label agrees with both instead of picking its own rule.
///
/// # Output
/// ```html
/// <nav class="moss-series-nav">
///   <div class="moss-series-nav-links">
///     <a href="prev.html" rel="prev" class="moss-series-nav-link moss-series-nav-prev">
///       <span class="moss-series-nav-arrow" aria-hidden="true">&lt;</span> <span class="visually-hidden">Previous</span> <span class="moss-series-nav-title">Title</span>
///     </a>
///     <a href="next.html" rel="next" class="moss-series-nav-link moss-series-nav-next">
///       <span class="visually-hidden">Next</span> <span class="moss-series-nav-title">Title</span> <span class="moss-series-nav-arrow" aria-hidden="true">&gt;</span>
///     </a>
///   </div>
///   <div class="moss-series-nav-collection-row">
///     <a href="index.html" class="moss-series-nav-collection">Collection Name</a>
///     <span class="moss-series-nav-position">2 of 3</span>
///   </div>
/// </nav>
/// ```
///
/// # Returns
/// - Empty string if series_name is empty (no folder context)
/// - HTML navigation element otherwise
pub fn render(
    series_name: &str,
    series_url: &str,
    prev: Option<(&str, &str)>,
    next: Option<(&str, &str)>,
    position: Option<(usize, usize)>,
    lang: crate::i18n::Language,
    typesetting: Option<&str>,
) -> String {
    // Always render if we have folder info (even without prev/next)
    if series_name.is_empty() {
        return String::new();
    }

    // Row 1: Navigation links
    let prev_link = neighbour_link("prev", prev, lang);
    let next_link = neighbour_link("next", next, lang);

    // Row 2: Folder link (centered), and — when the caller knows it — where in
    // the folder this page sits. Two arrows say what comes before and after;
    // they never say how far in the reader is, which is the one thing a reader
    // arriving mid-series from a shared link most wants to know.
    //
    // Only pages still in the reading order are counted (see `is_sequence_step`
    // in render/html.rs), so a page that stepped out with `series: false` does
    // not inflate the total, and a series still being published reports the
    // length it has rather than the length it is going to have.
    let position_html = position
        .map(|(pos, total)| {
            let template = crate::i18n::t(lang, "series_position");
            let text = if use_cjk_numerals(typesetting, lang) {
                // The template's literal spaces exist to set Arabic digits off
                // from the surrounding CJK glyphs; Chinese numerals are CJK
                // glyphs themselves (see article_count_label), so drop them.
                template
                    .replace("{n}", &to_chinese_numeral(pos))
                    .replace("{total}", &to_chinese_numeral(total))
                    .replace(' ', "")
            } else {
                template
                    .replace("{n}", &pos.to_string())
                    .replace("{total}", &total.to_string())
            };
            format!(
                r#"
    <span class="moss-series-nav-position">{}</span>"#,
                html_escape(&text)
            )
        })
        .unwrap_or_default();

    let folder_row = format!(
        r#"<div class="moss-series-nav-collection-row">
    <a href="{}" class="moss-series-nav-collection">{}</a>{}
  </div>"#,
        html_escape(series_url),
        html_escape(series_name),
        position_html
    );

    // Prefetch hint for the next page — browsers support <link> in body
    let prefetch = next
        .map(|(_, url)| format!(r#"<link rel="prefetch" href="{}">"#, html_escape(url)))
        .unwrap_or_default();

    format!(
        r#"<nav class="moss-series-nav">
  <div class="moss-series-nav-links">
    {}
    {}
  </div>
  {}
</nav>
{}"#,
        prev_link, next_link, folder_row, prefetch
    )
}

/// One previous/next link, or an empty placeholder that keeps the row's layout.
///
/// Arrow and title are separate spans so the arrow doesn't wrap with a
/// multi-line title. Arrow and placement show the direction; a screen reader
/// hears it from the hidden word instead of the arrow glyph.
fn neighbour_link(rel: &str, neighbour: Option<(&str, &str)>, lang: crate::i18n::Language) -> String {
    let Some((title, url)) = neighbour else {
        return format!(r#"<span class="moss-series-nav-link moss-series-nav-{rel} empty"></span>"#);
    };
    let (key, glyph) = if rel == "prev" { ("previous", "&lt;") } else { ("next", "&gt;") };
    let arrow = format!(r#"<span class="moss-series-nav-arrow" aria-hidden="true">{glyph}</span>"#);
    let text = format!(
        r#"<span class="visually-hidden">{}</span> <span class="moss-series-nav-title">{}</span>"#,
        html_escape(crate::i18n::t(lang, key)),
        html_escape(title)
    );
    let body = if rel == "prev" { format!("{arrow} {text}") } else { format!("{text} {arrow}") };
    format!(r#"<a href="{}" rel="{rel}" class="moss-series-nav-link moss-series-nav-{rel}">{body}</a>"#, html_escape(url))
}

#[cfg(test)]
#[path = "series_nav_tests.rs"]
mod tests;
