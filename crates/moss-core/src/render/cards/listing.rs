//! A folder listing's cards put in order and drawn as its style asks: the
//! build and a host's preview both draw a listing through [`rows_html`].

use std::collections::HashSet;

use super::row::{render as render_row, render_child, ArticleListItemProps};
use super::{cards_container, CardPictures, ListHolds};
use crate::i18n::dates;
use crate::i18n::Language;
use crate::page::card::{props_for, CardHost, ChildItemProps};
use crate::page::site_page::SitePage;
use crate::sort::{ResolvedSort, SortAxis};

/// How one listing is ordered and drawn.
pub struct RowsAsk<'a> {
    /// `list`, `summary`, `lead` (summary whose first item leads) or `grid`.
    pub style: &'a str,
    /// `year`, `upcoming` or `none`.
    pub group: &'a str,
    /// The cards arrive in an order a date sort must not undo (upcoming
    /// events soonest first). A sort that is not by date keeps the arrival
    /// order on its own.
    pub pre_ordered: bool,
    /// The folder's resolved sort: its axis fills the card's meta slot, and a
    /// typed list is drawn as typed. `None` lists by date.
    pub sort: Option<&'a ResolvedSort>,
    /// The `url_path`s of the events that have not ended; read only when
    /// `group` is `upcoming`.
    pub upcoming: &'a HashSet<&'a str>,
    /// A body embed's listing, which CSS spaces apart from the page's own.
    pub is_embed: bool,
    /// Width and float the container wears.
    pub placement: &'a crate::media::Placement,
    /// The card grid's covers all load lazily.
    pub lazy_cards: bool,
}

impl RowsAsk<'_> {
    fn axis(&self) -> SortAxis {
        self.sort.map_or(SortAxis::Date, ResolvedSort::presentation_axis)
    }

    fn keeps_order(&self) -> bool {
        self.pre_ordered || !self.sort.is_none_or(ResolvedSort::is_date_order)
    }
}

/// A listed page's excerpt, its card `text` else its body's first paragraph,
/// and whether the page is rich: it has a cover or that excerpt. A listing
/// whose pages are mostly rich defaults to `summary`.
pub fn excerpt_and_rich(cover: bool, text: Option<&str>, body: &str, math: bool) -> (Option<String>, bool) {
    let excerpt = crate::page::excerpt::resolve_page_description(text, body, math);
    let rich = cover || excerpt.is_some();
    (excerpt, rich)
}

/// A listing's card for `page`: [`props_for`], and on every style but the
/// grid, the body's first paragraph when the author wrote no card text. A
/// grid card keeps its hand-picked text, like the `:::grid` card it shares a
/// renderer with. `pictures` is the strip a `summary` row draws.
pub fn row_props<P: SitePage, D: std::borrow::Borrow<P>, H: CardHost<P>>(
    page: &P,
    all: &[D],
    host: &H,
    style: &str,
    pictures: Option<usize>,
    body: &str,
    math: bool,
) -> ChildItemProps {
    let summary = matches!(style, "summary" | "lead");
    let mut props = props_for(page, all, host, pictures.filter(|_| summary));
    if style != "grid" {
        props.description = crate::page::excerpt::resolve_page_description(props.description.as_deref(), body, math);
    }
    props
}

/// The listing's subfolders and its pages, in the order they are drawn.
pub struct Arranged<'a> {
    pub folders: Vec<&'a ChildItemProps>,
    pub articles: Vec<&'a ChildItemProps>,
}

impl<'a> Arranged<'a> {
    /// Folders then pages, as one run.
    pub fn flat(&self) -> Vec<&'a ChildItemProps> {
        self.folders.iter().chain(&self.articles).copied().collect()
    }
}

/// Orders `items` and splits the folders from the pages.
///
/// Unless the caller already ordered them, every style lists by date through
/// the comparator the folder's series chain uses. It re-sorts here because a
/// card's date can come from somewhere the chain does not look: a folder's
/// newest article, a date in the filename, the file's creation time.
pub fn arrange<'a>(items: &'a [ChildItemProps], ask: &RowsAsk<'_>) -> Arranged<'a> {
    let axis = ask.axis();
    let keep = ask.keeps_order();
    let mut sorted: Vec<&ChildItemProps> = items.iter().collect();
    if !keep {
        let ascending = matches!(axis, SortAxis::DateAsc);
        sorted.sort_by(|a, b| crate::sort::cmp_date_axis(&a.date_sort_key(), &b.date_sort_key(), ascending));
    }
    if ask.group == "upcoming" {
        sorted = upcoming_first(sorted, ask.upcoming, keep);
    }
    // Folders lead the pages in every style; a typed order is drawn as typed
    // unless the author asked for year sections. A dated folder on a date
    // order without year sections takes its place by date, since year
    // buckets hold only pages.
    let typed = ask.sort.is_some_and(|s| s.explicit_order.is_some());
    let folders_lead = !typed || ask.group == "year";
    let interleave_dated = ask.group != "year" && axis.shows_date();
    let (folders, articles) = sorted
        .into_iter()
        .partition(|i| folders_lead && i.subfolder && !(interleave_dated && i.date_display.is_some()));
    Arranged { folders, articles }
}

/// The listing of `items` as its style draws it. `stream`, which draws each
/// page whole, is the build's own and never reaches here.
pub fn rows_html(items: &[ChildItemProps], ask: &RowsAsk<'_>, pictures: &dyn CardPictures, lang: Language, typesetting: Option<&str>) -> String {
    if items.is_empty() {
        return String::new();
    }
    arranged_html(&arrange(items, ask), items, ask, pictures, lang, typesetting)
}

/// [`rows_html`] over cards already [`arrange`]d; `items` are all of them.
fn arranged_html(arranged: &Arranged<'_>, items: &[ChildItemProps], ask: &RowsAsk<'_>, pictures: &dyn CardPictures, lang: Language, typesetting: Option<&str>) -> String {
    let axis = ask.axis();
    if ask.style == "grid" {
        return super::grid::render_list_with_typesetting(&arranged.flat(), pictures, lang, typesetting, axis, ask.is_embed, ask.placement, ask.lazy_cards);
    }
    // `lead` is `summary` markup plus a hook on the container.
    let lead = ask.style == "lead";
    let summary = lead || ask.style == "summary";
    let row = |item: &ChildItemProps| {
        if summary {
            super::summary::render_with_sort(item, lang, typesetting, pictures, axis)
        } else {
            render_child(item, lang, typesetting)
        }
    };
    let (folders, articles) = (&arranged.folders, &arranged.articles);

    let mut html = String::new();
    for folder in folders {
        html.push_str(&row(folder));
        html.push('\n');
    }
    if summary && !folders.is_empty() && !articles.is_empty() {
        html.push_str("<hr class=\"moss-child-section-divider\" />\n");
    }
    if ask.group == "upcoming" {
        html.push_str(&upcoming_sections(articles, ask.upcoming, row));
    } else if ask.group == "year" && !articles.is_empty() {
        if summary {
            let sections: Vec<String> = bucket_by_year(articles)
                .into_iter()
                .map(|(year, arts)| {
                    let heading = year.map(|y| format!("<h2>{}</h2>\n", dates::format_year_heading(y, lang, typesetting))).unwrap_or_default();
                    let rows = arts.iter().map(|a| row(a)).collect::<Vec<_>>().join("\n");
                    format!("<section class=\"moss-cards-minimal-year-group moss-cards-minimal-year-group--summary\">\n{heading}{rows}\n</section>")
                })
                .collect();
            html.push_str(&sections.join("\n"));
        } else if ask.keeps_order() {
            // Not by date: the author's order must survive the year sections.
            let sections: Vec<String> = bucket_by_year(articles)
                .into_iter()
                .map(|(year, arts)| minimal_year_section(year, &arts, lang, typesetting))
                .collect();
            html.push_str(&sections.join("\n"));
        } else {
            // By date: the year-group renderer re-sorts (url-path tiebreak on
            // equal dates, as the series chain) and buckets in one pass.
            let props: Vec<ArticleListItemProps> = articles.iter().map(|item| ArticleListItemProps::from(*item)).collect();
            html.push_str(&super::year_group::render(&props, true, lang, typesetting, matches!(axis, SortAxis::DateAsc)));
        }
    } else {
        for article in articles {
            html.push_str(&row(article));
            html.push('\n');
        }
    }

    // `list` (and any older non-summary name) is the compact date and title
    // index, styled by the CSS keyed off `data-layout="minimal"`; summary
    // rows are `data-layout="list"`.
    let data_layout = if summary { "list" } else { "minimal" };
    let holds = ListHolds::of(items.iter().map(ChildItemProps::is_collection));
    cards_container(data_layout, lead, ask.is_embed, ask.placement, axis, holds, html.trim())
}

/// Events that have not ended first (soonest first), then the rest (newest
/// first). With `keep_order` each part keeps its given order.
fn upcoming_first<'a>(items: Vec<&'a ChildItemProps>, upcoming: &HashSet<&str>, keep_order: bool) -> Vec<&'a ChildItemProps> {
    let (mut soon, mut rest): (Vec<_>, Vec<_>) = items.into_iter().partition(|i| upcoming.contains(i.url_path.as_str()));
    if !keep_order {
        soon.sort_by(|a, b| crate::sort::cmp_date_axis(&a.date_sort_key(), &b.date_sort_key(), true));
        rest.sort_by(|a, b| crate::sort::cmp_date_axis(&a.date_sort_key(), &b.date_sort_key(), false));
    }
    soon.into_iter().chain(rest).collect()
}

/// The two runs as `<section class="moss-cards-group" data-group="…">`; an
/// empty run is left out.
fn upcoming_sections(articles: &[&ChildItemProps], upcoming: &HashSet<&str>, row: impl Fn(&ChildItemProps) -> String) -> String {
    let mut html = String::new();
    for (name, in_group) in [("upcoming", true), ("earlier", false)] {
        let items: Vec<&&ChildItemProps> = articles.iter().filter(|a| upcoming.contains(a.url_path.as_str()) == in_group).collect();
        if items.is_empty() {
            continue;
        }
        html.push_str(&format!("<section class=\"moss-cards-group\" data-group=\"{name}\">\n"));
        for article in items {
            html.push_str(&row(article));
            html.push('\n');
        }
        html.push_str("</section>\n");
    }
    html
}

/// One section per year in first-appearance order, never re-sorted, so an
/// order that is not by date survives; the undated run goes last, as the
/// year-group renderer puts it.
fn bucket_by_year<'a>(articles: &[&'a ChildItemProps]) -> Vec<(Option<i32>, Vec<&'a ChildItemProps>)> {
    let mut buckets: Vec<(Option<i32>, Vec<&ChildItemProps>)> = Vec::new();
    for article in articles {
        let year = article.date_raw.as_ref().and_then(|d| dates::extract_year(d));
        match buckets.iter_mut().find(|(y, _)| *y == year) {
            Some((_, bucket)) => bucket.push(*article),
            None => buckets.push((year, vec![*article])),
        }
    }
    if let Some(idx) = buckets.iter().position(|(y, _)| y.is_none()) {
        let undated = buckets.remove(idx);
        buckets.push(undated);
    }
    buckets
}

/// One `list` year section in the year-group renderer's markup, so the
/// minimal row CSS applies; the undated run has no heading.
fn minimal_year_section(year: Option<i32>, arts: &[&ChildItemProps], lang: Language, typesetting: Option<&str>) -> String {
    let rows = arts
        .iter()
        .map(|a| render_row(&ArticleListItemProps::from(*a), true, false, lang, typesetting))
        .collect::<Vec<_>>()
        .join("\n");
    match year {
        Some(y) => format!(
            "<section class=\"moss-cards-minimal-year-group minimal\">\n<h2>{}</h2>\n{rows}\n</section>",
            dates::format_year_heading(y, lang, typesetting)
        ),
        None => format!("<section class=\"moss-cards-minimal-year-group minimal\">\n{rows}\n</section>"),
    }
}

#[cfg(test)]
#[path = "listing_tests.rs"]
mod tests;
