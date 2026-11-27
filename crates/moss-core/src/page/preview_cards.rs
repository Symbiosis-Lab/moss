//! A folder home's own listing as a host's preview draws it: the build's
//! selection, card reader and renderers over the texts the host sent. A
//! listed page's body is parsed only when its card asks for a picture.
//!
//! Not drawn yet, since moss-core has no pass for them: a term's card,
//! places, credits and the section accent, a cover's colour band, the
//! listing-style wrapper, a row's language mark, the `more` link and
//! `style: stream`. The preview has no clock, so
//! every event reads as not yet ended.

use std::cell::RefCell;
use std::collections::HashMap;

use super::card::{CardHost, SectionAccent, TermCardFacts};
use super::card_pictures::{card_pictures_after_cover, PagePictures};
use super::site_page::{PageFacts, SitePages};
use super::SiteInputs;
use crate::content_graph::ContentGraph;
use crate::render::cards::listing;

/// The pages a preview listing shows, with their texts.
struct PreviewCards<'a> {
    texts: &'a HashMap<String, String>,
    site: &'a SiteInputs,
    graph: ContentGraph,
    typesetting: Option<&'a str>,
    /// Each listed page's pictures and listing cover, once read.
    pictures: RefCell<HashMap<String, (PagePictures, Option<String>)>>,
}

impl PreviewCards<'_> {
    fn text(&self, page: &PageFacts) -> Option<&str> {
        page.source_path.as_deref().and_then(|p| self.texts.get(p)).map(String::as_str)
    }

    fn body(&self, page: &PageFacts) -> &str {
        self.text(page).map_or("", crate::frontmatter::body)
    }

    /// `read` over the page's pictures and its body cover.
    fn with_pictures<T>(&self, page: &PageFacts, read: impl FnOnce(&PagePictures, Option<&str>) -> T) -> Option<T> {
        let path = page.source_path.as_deref()?;
        let mut cache = self.pictures.borrow_mut();
        if !cache.contains_key(path) {
            let text = self.texts.get(path)?;
            let pictures = super::page_pictures(path, text, self.site, &self.graph);
            let cover = pictures.card_cover(crate::frontmatter::read_page(text).0.cover.as_deref());
            cache.insert(path.to_string(), (pictures, cover));
        }
        cache.get(path).map(|(pictures, cover)| read(pictures, cover.as_deref()))
    }
}

impl CardHost<PageFacts> for PreviewCards<'_> {
    /// Relative to the site root, which is the preview's base.
    fn resolve_url(&self, path: &str) -> String {
        if path.starts_with("http://") || path.starts_with("https://") {
            return path.to_string();
        }
        crate::resolve::fuzzy_path::percent_encode_path_segments(path.trim_start_matches('/'))
    }
    fn typesetting(&self) -> Option<&str> { self.typesetting }
    fn now(&self) -> &str { "" }
    fn term_card(&self, _: &PageFacts) -> Option<TermCardFacts> { None }
    fn body_cover(&self, page: &PageFacts) -> Option<String> {
        self.with_pictures(page, |_, cover| cover.map(str::to_string)).flatten()
    }
    fn place_names(&self, _: &PageFacts) -> Option<String> { None }
    fn credits(&self, _: &PageFacts) -> Option<String> { None }
    fn section(&self, _: &PageFacts) -> Option<SectionAccent> { None }
    fn pictures_after_cover(&self, page: &PageFacts, n: usize) -> Vec<String> {
        let own = page.card.cover.clone();
        self.with_pictures(page, |pictures, body_cover| {
            card_pictures_after_cover(own.as_deref().or(body_cover), pictures.hero.as_deref(), &pictures.body, n)
        })
        .unwrap_or_default()
    }
}

impl SitePages {
    /// The listing the page at `path` ends in, drawn from `texts`, every
    /// page's source by path; `None` when it ends in none.
    pub fn own_listing(&self, path: &str, texts: &HashMap<String, String>, site: &SiteInputs, typesetting: Option<&str>) -> Option<String> {
        let host = &self.pages[self.paths.iter().position(|p| p == path)?];
        let cards = PreviewCards { texts, site, graph: super::graph(site), typesetting, pictures: RefCell::default() };
        // The preview parses with math on (`super::parse_config`), so its excerpts read `$…$` the same way.
        let math = true;
        let ordered = super::listing::host_listing(
            host,
            &self.pages,
            self.listing_site(),
            cards.now(),
            |p: &PageFacts| listing::excerpt_and_rich(p.card.cover.is_some(), p.card.text.as_deref(), cards.body(p), math).1,
            |p| p.card.cover.is_some() || cards.body_cover(p).is_some(),
        )?;
        let style = ordered.style.value.as_str();
        let fm = cards.text(host).map(|t| crate::frontmatter::read_page(t).0).unwrap_or_default();
        let items: Vec<_> = ordered
            .limited
            .iter()
            .map(|page| {
                let mut props = listing::row_props(*page, &self.pages, &cards, style, fm.children_pictures.map(|n| n as usize), cards.body(page), math);
                props.marquee = fm.children_motion.is_some();
                props.count_setting = props.count_setting.take().or_else(|| fm.children_count.clone());
                props
            })
            .collect();
        let upcoming = ordered.limited.iter().filter(|p| super::listing::is_upcoming(**p, cards.now())).map(|p| p.url_path.as_str()).collect();
        let ask = listing::RowsAsk {
            style,
            group: &ordered.group,
            pre_ordered: ordered.pre_ordered,
            sort: Some(&ordered.resolved),
            upcoming: &upcoming,
            is_embed: false,
            placement: &Default::default(),
            lazy_cards: false,
        };
        Some(listing::rows_html(&items, &ask, &(), host.lang, typesetting))
    }
}
