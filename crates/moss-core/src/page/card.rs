//! One page as a listing draws it: the facts every card and row form reads,
//! filled by [`props_for`] from any [`SitePage`], so the build's documents and
//! a host's page facts give the same card.

use super::site_page::SitePage;
use crate::event::when::{self, EventWhen};
use crate::frontmatter_union::ChildrenCount;
use crate::i18n::{dates, Language};
use crate::media::html_escape;
use std::borrow::Borrow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverType {
    Image,
    Video,
    Iframe,
}

impl CoverType {
    pub fn resolve(cover: Option<&str>, explicit: Option<CoverType>) -> Option<Self> {
        cover.map(|_| explicit.unwrap_or(CoverType::Image))
    }
}

/// A cover's media type: the page's `cover_type:` when set, else the file's
/// extension, else an image.
pub fn detect_cover_type(url: &str, explicit: Option<&str>) -> CoverType {
    if let Some(e) = explicit {
        return match e.to_ascii_lowercase().as_str() {
            "video" => CoverType::Video,
            "iframe" => CoverType::Iframe,
            _ => CoverType::Image,
        };
    }
    let path = url.split(['?', '#']).next().unwrap_or(url);
    match path.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "mp4" | "webm" | "mov" | "m4v" => CoverType::Video,
        "html" | "htm" => CoverType::Iframe,
        _ => CoverType::Image,
    }
}

/// The accent a folder's `color:` gives the pages under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionAccent {
    /// The validated colour, as written (`#rgb` or `#rrggbb`).
    pub color: String,
    /// The same hue as a text colour on the default light page, `#rrggbb`.
    pub text_light: String,
    /// The same hue as a text colour on the default dark page, `#rrggbb`.
    pub text_dark: String,
    /// The coloured folder's own url path segment; empty for the site root.
    pub segment: String,
}

impl SectionAccent {
    /// The custom properties a page or card in the section carries.
    pub fn style(&self) -> String {
        format!("--moss-section-color: {}; --moss-section-text-light: {}; --moss-section-text-dark: {}", self.color, self.text_light, self.text_dark)
    }
}

/// The language of a listed page that differs from the listing page's: the page's
/// BCP 47 tag for `lang=` on its title and description, and the mark naming it in
/// the listing page's interface language.
#[derive(Debug, Clone, PartialEq)]
pub struct RowLang {
    pub tag: String,
    /// Language name in the listing page's interface language; empty for an unknown tag.
    pub name: &'static str,
    /// The listing page's tag, which the mark's text is in.
    pub listing_tag: String,
}

/// A page's own facts a card reads, from its frontmatter.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct CardFacts {
    /// `date:` as posted, which a folder's card shows over an event's start.
    pub posted_date: Option<String>,
    /// `description:`, else the first `byline:` row, trimmed.
    pub text: Option<String>,
    /// `cover:` as written.
    pub cover: Option<String>,
    pub cover_type: Option<String>,
    /// Where the card links instead of the page: a valid `external_url:`, or
    /// for a page-less entry also a path on the site.
    pub outlet: Option<String>,
    /// `publisher:`, shown above the title.
    pub kicker: Option<String>,
    pub location: Vec<String>,
    pub tickets: Option<String>,
    pub online: Option<String>,
    /// `subtitle:`, trimmed.
    pub subtitle: Option<String>,
}

/// A page's `cover:` as a path from the site root, with any `|attrs` after it;
/// `None` for `cover: false`. Takes the value quoted or not, as `![[embed|attrs]]`,
/// `[[link|alias]]` (the alias dropped), `![alt|attrs](url)` or a plain path.
pub fn frontmatter_cover(raw: &str, url_path: &str) -> Option<String> {
    use crate::resolve::output_url::resolve_to_root_relative;
    let trimmed = raw.trim();
    if trimmed == "false" {
        return None;
    }
    let unquoted = trimmed.strip_prefix('"').and_then(|s| s.strip_suffix('"')).or_else(|| trimmed.strip_prefix('\'').and_then(|s| s.strip_suffix('\''))).unwrap_or(trimmed);
    let with_attrs = |path: String, attrs: &str| if attrs.is_empty() { path } else { format!("{path}|{attrs}") };
    if let Some(inner) = unquoted.strip_prefix("![[").and_then(|s| s.strip_suffix("]]")).filter(|i| !i.is_empty()) {
        let (path, attrs) = crate::media::split_pipe(inner);
        return Some(with_attrs(path.trim_start_matches('/').to_string(), attrs));
    }
    if let Some(inner) = unquoted.strip_prefix("[[").and_then(|s| s.strip_suffix("]]")).filter(|i| !i.is_empty()) {
        return Some(crate::media::split_pipe(inner).0.trim_start_matches('/').to_string());
    }
    if let Some((alt, url)) = unquoted.strip_prefix("![").and_then(|s| s.strip_suffix(')')).and_then(|s| s.rsplit_once("](")) {
        let attrs = alt.split_once('|').map_or("", |(_, attrs)| attrs);
        return Some(with_attrs(resolve_to_root_relative(url, url_path), attrs));
    }
    let (path, attrs) = crate::media::split_pipe(unquoted);
    Some(with_attrs(resolve_to_root_relative(path, url_path), attrs))
}

/// A frontmatter string as a card shows it: trimmed, `None` when blank.
pub fn trimmed(s: Option<&str>) -> Option<String> {
    s.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// The text a card shows when the author wrote no excerpt: `description:`,
/// else the first `byline:` row.
pub fn card_text(description: Option<&str>, first_byline: Option<&str>) -> Option<String> {
    trimmed(description).or_else(|| trimmed(first_byline))
}

/// Where a card links instead of its page: a valid `external_url:`, or for a
/// page-less entry also a path on the site.
pub fn outlet_href(external_url: Option<&str>, pageless: bool) -> Option<String> {
    let url = if pageless { super::nav::nav_link(external_url)? } else { external_url? };
    let on_site = pageless && url.starts_with('/') && !url.starts_with("//");
    (super::nav::is_valid_external_url(url) || on_site).then(|| url.to_string())
}

/// The count and picture a term's pseudo-folder card shows.
#[derive(Debug, Clone, Default)]
pub struct TermCardFacts {
    pub count: usize,
    pub cover: Option<String>,
    /// The term kind's `count` wording for the card's language; `None` keeps the default.
    pub count_setting: Option<ChildrenCount>,
}

/// What a card needs that is not the page's own: what the host resolves, and
/// what other passes found. The term card, places, credits and section accent
/// come from passes moss-core does not have yet, so every host answers them;
/// a host without one says `None`.
pub trait CardHost<P> {
    /// A site path as the card links or shows it.
    fn resolve_url(&self, path: &str) -> String;
    fn typesetting(&self) -> Option<&str>;
    /// The minute events are compared with, `YYYY-MM-DDTHH:MM`.
    fn now(&self) -> &str;
    /// The card of a term page, from the term's members.
    fn term_card(&self, page: &P) -> Option<TermCardFacts>;
    /// The picture the body gives a page with no `cover:`.
    fn body_cover(&self, _page: &P) -> Option<String> {
        None
    }
    /// The page's resolved places, plain.
    fn place_names(&self, page: &P) -> Option<String>;
    /// The names a term kind with `cards = true` lists, as one span.
    fn credits(&self, page: &P) -> Option<String>;
    fn section(&self, page: &P) -> Option<SectionAccent>;
    /// The page's pictures after its cover, at most `n`, unresolved.
    fn pictures_after_cover(&self, _page: &P, _n: usize) -> Vec<String> {
        Vec::new()
    }
}

/// Properties for rendering a child list item (article or folder)
#[derive(Debug, Clone, Default)]
pub struct ChildItemProps {
    pub title: String,
    /// Where the card goes; `None` for a page-less entry, drawn unlinked.
    pub url: Option<String>,
    /// None for folders
    pub date_display: Option<String>,
    /// Raw ISO date for sorting. For folders: latest article date within
    pub date_raw: Option<String>,
    /// Number of articles inside. None for leaf articles
    pub child_count: Option<usize>,
    /// A folder with pages to count or folders inside: it leads the pages
    /// and sorts as a folder. A term's card shows a count and is not one.
    pub subfolder: bool,
    /// Markdown, not plain text: a renderer prints it with
    /// `page::meta::render_description_html`, never a bare escape.
    pub description: Option<String>,
    pub cover: Option<String>,
    /// The page's own pictures, cover first, resolved like `cover`, when the
    /// listing asked for a strip (`pictures:N`) and the page has more than
    /// one. Empty otherwise: a summary row then draws `cover` alone.
    pub pictures: Vec<String>,
    /// The listing asked for `motion:marquee`: the strip is drawn twice in a
    /// track that slides sideways. Meaningful only with `pictures`.
    pub marquee: bool,
    /// Defaults to Image when cover is Some.
    pub cover_type: Option<CoverType>,
    /// The overline above the title (horizontal) or between title and meta
    /// (vertical); the minimal row has no slot for it. Empty and `None` are
    /// the same.
    pub kicker: Option<String>,
    /// Local archival URL, set only when `url` points out (`external_url:`).
    /// The summary row links it from a `★` mark; the minimal row has no slot
    /// for a second anchor.
    pub permalink: Option<String>,
    /// The page's own `url_path`. Unlike `url`, neither `external_url:` nor a
    /// directory override changes it, so it is what breaks a tie between two
    /// cards on the same date.
    pub url_path: String,
    /// The page's `weight:`, which orders cards on one date.
    pub weight: Option<i32>,
    /// The page's resolved place, plain and unlinked, shown next to a leaf's
    /// date; never beside a folder's count, which is not a date.
    pub place: Option<String>,
    /// An event's written time as a `<time>` with its parts in spans
    /// ([`when::render_when`]); shown in place of `date_display`.
    pub when_html: Option<String>,
    /// Where an event stands, published on the card as `data-when` (and
    /// `data-ends`); `None` for any other page.
    pub when: Option<EventWhen>,
    /// The names a term kind with `cards = true` lists on this page, as one
    /// pre-rendered `moss-card-credits` span. Shown in a leaf's meta after its
    /// date and place; `None` for a folder.
    pub credits: Option<String>,
    /// An event's `tickets:` address while it is open. A card with a web
    /// address here gets its own Tickets link and its title is the page link.
    pub tickets: Option<String>,
    /// An event's `online:` address; like `tickets`, its own link after Tickets.
    pub online: Option<String>,
    /// The accent colour of the section the page sits in, published on the
    /// card as `--moss-section-color` so a theme can tint cards by section.
    pub section_color: Option<SectionAccent>,
    /// Source path of the page this card stands for, published as
    /// `data-source-file` on the card's root so a click in the preview opens
    /// that page rather than the listing.
    pub source_file: Option<String>,
    /// The listing's `children_count:` setting, applied to a folder's count.
    pub count_setting: Option<ChildrenCount>,
    /// The page's `subtitle:`, drawn under the title.
    pub subtitle: Option<String>,
    /// Set when the page is in another language than the listing.
    pub lang: Option<RowLang>,
}

impl ChildItemProps {
    /// A folder or a term's card: it stands for the pages it holds.
    pub fn is_collection(&self) -> bool {
        self.child_count.is_some()
    }

    /// A folder card's count text, or `None` for a leaf or a listing that
    /// hides the count. The one place the setting is applied.
    pub fn count_label(&self, lang: Language, typesetting: Option<&str>) -> Option<String> {
        crate::i18n::card_count_label(lang, self.child_count?, typesetting, self.count_setting.as_ref())
    }

    pub fn root_attrs(&self) -> String {
        root_attrs(self.source_file.as_deref(), self.when.as_ref())
    }

    /// The card's ` style="--moss-section-color: …"` attribute, or nothing for
    /// a page outside any coloured section.
    pub fn section_style_attr(&self) -> String {
        self.section_color.as_ref().map(|a| format!(r#" style="{}""#, html_escape(&a.style()))).unwrap_or_default()
    }

    pub fn date_sort_key(&self) -> crate::sort::DateSortKey<'_> {
        crate::sort::DateSortKey { date: self.date_raw.as_deref(), is_folder: self.subfolder, label: &self.title, url_path: &self.url_path, weight: self.weight }
    }
}

/// The attributes every listing card and row root carries: its source page
/// and an event's upcoming/past state.
pub fn root_attrs(source_file: Option<&str>, when: Option<&EventWhen>) -> String {
    format!("{}{}", source_file_attr(source_file), when.map(EventWhen::attrs).unwrap_or_default())
}

/// ` data-source-file="…"` for an element whose source is another page, or
/// nothing when the path is unknown.
pub fn source_file_attr(path: Option<&str>) -> String {
    path.map(|p| format!(r#" data-source-file="{}""#, html_escape(p))).unwrap_or_default()
}

/// The dot between the parts of a card's meta line.
pub const EVENT_SEP: &str = r#"<span class="moss-event-sep"> · </span>"#;

/// The pages under a folder home that `listing` counts: listable articles at
/// any depth.
pub fn count_listable_articles<P: SitePage, D: Borrow<P>>(all: &[D], prefix: &str, self_url: &str) -> usize {
    all.iter().map(Borrow::borrow).filter(|d| d.url_path() != self_url && d.url_path().starts_with(prefix) && !d.is_folder_index() && d.is_listable_in(prefix)).count()
}

/// A folder card's two facts from the corpus: the listable pages it counts,
/// and whether it leads the pages. It leads with pages to count, or, unless it
/// is a term's card, with a listable folder inside; a folder of pictures does not.
pub fn folder_card_facts<P: SitePage, D: Borrow<P>>(page: &P, all: &[D], is_term_card: bool) -> (Option<usize>, bool) {
    if !page.is_folder_index() {
        return (None, false);
    }
    let prefix = page.url_path().trim_end_matches("index.html");
    let count = Some(count_listable_articles(all, prefix, page.url_path())).filter(|n| *n > 0);
    let leads = count.is_some()
        || (!is_term_card
            && all.iter().map(Borrow::borrow).any(|d| d.url_path() != page.url_path() && d.url_path().starts_with(prefix) && d.is_folder_index() && d.is_listable_in(prefix)));
    (count, leads)
}

/// A page's date as a listing reads it: an event's start, else `date:`, else
/// a date in the file's name. `None` for an undated page; a file's creation
/// time is not a date the author gave it.
pub fn listing_date<P: SitePage + ?Sized>(page: &P) -> Option<String> {
    if let Some(d) = page.date() {
        return Some(d.to_string());
    }
    let name = page.url_path().rsplit('/').next()?.strip_suffix(".html")?;
    crate::date::parse_partial_date(name)?.month?;
    Some(name.to_string())
}

/// The latest date among a folder's direct children, a subfolder by its own
/// latest child: what a folder with no date of its own sorts by.
pub fn folder_latest_date<P: SitePage, D: Borrow<P>>(folder: &P, all: &[D]) -> Option<String> {
    let prefix = folder.url_path().trim_end_matches("index.html");
    all.iter()
        .map(Borrow::borrow)
        .filter(|d| {
            d.url_path() != folder.url_path()
                && d.url_path().strip_prefix(prefix).is_some_and(|rem| !rem.trim_end_matches("/index.html").trim_end_matches('/').contains('/'))
        })
        .filter_map(|d| {
            let inner = d.url_path().trim_end_matches("index.html");
            let is_subfolder = d.is_folder_index() && all.iter().map(Borrow::borrow).any(|o| o.url_path() != d.url_path() && o.url_path().starts_with(inner));
            if is_subfolder { folder_latest_date(d, all) } else { listing_date(d) }
        })
        .max()
}

/// One card's props from the page it describes.
///
/// `description` is the card text (`description:`, else the first byline
/// row); a listing that falls back to the page's excerpt overrides it.
/// `pictures` is the strip a listing asked for (`pictures:N`).
pub fn props_for<P: SitePage, D: Borrow<P>, H: CardHost<P>>(page: &P, all: &[D], host: &H, pictures: Option<usize>) -> ChildItemProps {
    let facts = page.card_facts();
    let lang = page.lang();
    let typesetting = host.typesetting();
    let now = host.now();
    let term = host.term_card(page);
    let compact = |d: &str| dates::time_html(&dates::format_compact_date(d, lang, typesetting), d);

    // The card counts every listable page beneath the folder, at any depth,
    // whatever `children_depth` the folder's own page lists: "this many pages
    // live here". No pages, no claim: the card shows a date instead.
    let (article_count, leads) = folder_card_facts(page, all, term.is_some());
    let (child_count, date_raw, date_display) = match article_count {
        // A folder home's own `date:` is what a reader picks it by; the latest
        // child date only orders a folder that has none, and is never shown.
        Some(count) => {
            let raw = facts.posted_date.clone().or_else(|| folder_latest_date(page, all));
            (Some(count), raw, facts.posted_date.as_deref().map(compact))
        }
        None => {
            let raw = listing_date(page);
            (None, raw.clone(), raw.as_deref().map(compact))
        }
    };

    // The resolver sees only the path; a cover's `|attrs` ride along after it.
    let listing_cover = term.as_ref().and_then(|t| t.cover.clone()).or_else(|| facts.cover.clone()).or_else(|| host.body_cover(page));
    let resolved_cover = listing_cover.as_deref().map(|c| {
        let (path, attrs) = crate::media::split_pipe(c);
        (host.resolve_url(path), attrs)
    });
    let cover = resolved_cover.as_ref().map(|(path, attrs)| if attrs.is_empty() { path.clone() } else { format!("{path}|{attrs}") });

    // A card with an outlet links there; the page is still built at its slug,
    // and its local address is kept as the archive mark's link. A page-less
    // entry has no local address.
    let local_url = (!page.is_pageless_entry()).then(|| host.resolve_url(&crate::resolve::output_url::to_pretty_url(page.url_path())));
    let permalink = facts.outlet.as_ref().and(local_url.clone());

    let event = page.event_span();
    let term_count_shown = child_count.is_none() && term.is_some();
    // A listing's `children_count` applies to a card only where the card has no wording of its own.
    let count_setting = term.as_ref().filter(|_| term_count_shown).and_then(|t| t.count_setting.clone());
    let strip = pictures.zip(cover.as_ref()).map_or_else(Vec::new, |(n, cover)| {
        let rest = host.pictures_after_cover(page, n);
        if rest.is_empty() { rest } else { std::iter::once(cover.clone()).chain(rest.iter().map(|p| host.resolve_url(p))).collect() }
    });

    ChildItemProps {
        title: page.label().to_string(),
        url: facts.outlet.clone().or(local_url),
        date_display,
        date_raw,
        child_count: child_count.or(term.as_ref().map(|t| t.count)),
        subfolder: leads,
        description: facts.text.clone(),
        pictures: strip,
        marquee: false,
        cover_type: resolved_cover.as_ref().map(|(path, _)| detect_cover_type(path, facts.cover_type.as_deref())),
        cover,
        kicker: facts.kicker.clone(),
        permalink,
        url_path: page.url_path().to_string(),
        weight: page.weight(),
        count_setting,
        // An event's place is shown even when it names no place page.
        place: host.place_names(page).or_else(|| event.and(Some(facts.location.join(", "))).filter(|p| !p.is_empty())),
        when_html: event.map(|e| {
            let mut html = when::render_when(e.start, e.end, lang);
            let mark = when::upcoming_mark(e, lang, now);
            if !mark.is_empty() {
                html.push_str(EVENT_SEP);
                html.push_str(&mark);
            }
            html
        }),
        when: event.map(|e| EventWhen::of(e, now)),
        credits: host.credits(page),
        tickets: event.and_then(|e| when::open_tickets(facts.tickets.as_deref(), e, now)).map(str::to_string),
        online: event.and(facts.online.clone()),
        section_color: host.section(page),
        source_file: page.source_path().map(str::to_string),
        subtitle: facts.subtitle.clone(),
        lang: None,
    }
}

#[cfg(test)]
#[path = "card_tests.rs"]
mod tests;
