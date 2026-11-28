//! Series navigation: which pages a folder's reading order holds, and the
//! prev/next block drawn at the bottom of each of them.

use crate::frontmatter_typed::SeriesField;
use crate::i18n::numerals::to_chinese_numeral;
use crate::i18n::use_cjk_numerals;
use crate::media::html_escape;
use crate::page::site_page::SitePage;
use crate::sort::{sort_by_resolved, ResolvedSort, SortAxis};

/// Render series navigation HTML: an empty string when `series_name` is empty.
///
/// Arrow and title are separate spans so the arrow stays fixed when the title wraps.
/// `typesetting` is the page's resolved typesetting, read by [`use_cjk_numerals`]
/// as dates and card counts are, so the position label agrees with both.
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
    // Only pages still in the reading order are counted (`is_sequence_step`
    // below), so a page that stepped out with `series: false` does not inflate
    // the total, and a series still being published reports the length it has
    // rather than the length it is going to have.
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

/// Whether `page` is a step in its folder's reading order. A page that sets
/// `series: false` drops out of the whole chain, so its neighbours become
/// adjacent; a folder home is a step only when it is laid out as an article.
/// Every other page answers as [`SitePage::is_listable_in`] its own address,
/// so a hidden folder holding the page holds the whole chain.
pub fn is_sequence_step<P: SitePage>(page: &P) -> bool {
    page.is_listable_in(page.url_path())
        && (!page.is_folder_index() || page.is_article_layout())
        && !matches!(page.series(), Some(SeriesField::Flag(false)))
}

/// Whether a folder's series chrome is on for its children: only the
/// folder's own `series:` turns it on (`true`, or the list form). How the
/// folder is ordered says nothing about it, since most ordered folders are
/// sections, not sequences to read through.
pub fn chrome_on<P: SitePage>(folder: &P) -> bool {
    matches!(folder.series(), Some(SeriesField::Flag(true) | SeriesField::Ordered(_)))
}

/// The folder whose series chain `page` steps in, when that chain's chrome is
/// on: the home of the folder holding `page`'s own folder
/// (`series/part-1/index.html` belongs to `series/index.html`). The series
/// navigation and the incremental verdict both ask this, so they agree on
/// which pages a chain holds.
pub fn series_parent<'a, P: SitePage>(page: &P, pages: &'a [P]) -> Option<&'a P> {
    if !is_sequence_step(page) {
        return None;
    }
    let folder = parent_folder(page.url_path())?;
    let home = format!("{folder}/index.html");
    let parent = pages.iter().find(|p| p.url_path() == home)?;
    chrome_on(parent).then_some(parent)
}

/// The series navigation drawn on `page`, a step in `parent`'s `chain`, or
/// `None` when it draws nothing. `url` turns a page's url path into its link;
/// labels are chrome, so plain text.
pub fn nav<P: SitePage>(
    page: &P,
    parent: &P,
    chain: &[&P],
    url: &dyn Fn(&str) -> String,
    lang: crate::i18n::Language,
    typesetting: Option<&str>,
) -> Option<String> {
    // By url path: every folder home's clean stem is "index".
    let pos = chain.iter().position(|p| p.url_path() == page.url_path())?;
    let neighbour = |i: usize| (chain[i].label().to_string(), url(chain[i].url_path()));
    let prev = (pos > 0).then(|| neighbour(pos - 1));
    let next = (pos + 1 < chain.len()).then(|| neighbour(pos + 1));
    let html = render(
        parent.label(),
        &url(parent.url_path()),
        prev.as_ref().map(|(t, u)| (t.as_str(), u.as_str())),
        next.as_ref().map(|(t, u)| (t.as_str(), u.as_str())),
        // Where you are, over the same set the arrows walk. A lone page gets
        // no ordinal: "1 of 1" is noise, not orientation.
        (chain.len() > 1).then_some((pos + 1, chain.len())),
        lang, typesetting,
    );
    (!html.is_empty()).then_some(html)
}

/// `"series/part-1/index.html"` -> `"series"`: the path with its last two
/// segments removed, or `None` when it has fewer than three.
fn parent_folder(url_path: &str) -> Option<&str> {
    let head = url_path.get(..url_path.rfind('/')?)?;
    head.get(..head.rfind('/')?)
}

/// The chain under `parent`: its direct children that are steps, in reading
/// order. However a folder lists its pages, a series is read forward in time,
/// so next is the newer page.
pub fn chain<'a, P: SitePage>(pages: &'a [P], parent: &P) -> Vec<&'a P> {
    let home = parent.url_path();
    let prefix = home.strip_suffix("index.html").unwrap_or(home);
    let steps: Vec<&P> = pages.iter()
        .filter(|p| p.url_path() != home && is_direct_child(p.url_path(), prefix) && is_sequence_step(*p))
        .collect();
    let resolved = parent.resolve_for_direct_children();
    let forward = (resolved.axis == SortAxis::Date && resolved.explicit_order.is_none())
        .then(|| ResolvedSort { axis: SortAxis::DateAsc, ..resolved.clone() });
    sort_by_resolved(&steps, forward.as_ref().unwrap_or(&resolved))
}

/// Is `url_path` one segment below `prefix`, in either the `<slug>.html` or
/// the `<slug>/index.html` form?
pub fn is_direct_child(url_path: &str, prefix: &str) -> bool {
    url_path.strip_prefix(prefix).is_some_and(|rem| {
        !rem.trim_end_matches("/index.html").trim_end_matches('/').contains('/')
    })
}

#[cfg(test)]
#[path = "series_tests.rs"]
mod tests;
