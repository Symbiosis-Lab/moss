use super::*;
use crate::i18n::Language;
use moss_core::page::card::{props_for, CardHost};
use moss_core::page::site_page::PageFacts;
use moss_core::render::cards::row::{render, render_child};

#[test]
fn test_render_child_article() {
    let props = ChildItemProps {
        title: "My Article".to_string(),
        url: Some("/blog/my-article/".to_string()),
        date_display: dated("2025-11-17T00:50:43.135Z", Language::ZhHans),
        ..Default::default()
    };
    let html = render_child(&props, crate::i18n::Language::ZhHans, None);
    assert!(html.contains(r#"<div class="moss-card">"#));
    // No year heading sits above a flat row, so the prefix carries the year
    // and month, not the month alone.
    assert!(html.contains(r#"<span class="moss-prefix-link-prefix date"><time datetime="2025-11-17">2025年11月</time></span>"#), "{html}");
    assert!(html.contains(r#"<span class="moss-prefix-link-title">My Article</span>"#));
    assert!(html.contains(r#"href="/blog/my-article/""#));
}

/// A dated, located leaf (a lecture: "Cambridge 1924") shows its place next
/// to its date in the flat listing row, not just the date.
#[test]
fn render_child_shows_the_resolved_place_next_to_a_leafs_date() {
    let props = ChildItemProps {
        title: "Lecture".to_string(),
        url: Some("/lectures/1924.html".to_string()),
        date_display: dated("1924-10-01", Language::En),
        place: Some("Cambridge".to_string()),
        ..Default::default()
    };
    let html = render_child(&props, crate::i18n::Language::En, None);
    assert!(
        html.contains(r#"<span class="moss-prefix-link-prefix date"><time datetime="1924-10-01">October 1924</time> · Cambridge</span>"#),
        "{html}"
    );
}

/// A term kind's names on cards come after the date and place, in the same
/// meta slot, and the row's date span carries them.
#[test]
fn render_child_puts_the_listed_names_after_the_date_and_place() {
    let credits = r#"<span class="moss-card-credits"><span class="moss-credit">Ada Example<span class="moss-credit-role">, conductor</span></span></span>"#;
    let props = ChildItemProps {
        credits: Some(credits.to_string()),
        title: "Lecture".to_string(),
        url: Some("/lectures/1924.html".to_string()),
        date_display: dated("1924-10-01", Language::En),
        place: Some("Cambridge".to_string()),
        ..Default::default()
    };
    let html = render_child(&props, crate::i18n::Language::En, None);
    let expected = format!(r#"<span class="moss-prefix-link-prefix date"><time datetime="1924-10-01">October 1924</time> · Cambridge<span class="moss-event-sep"> · </span>{credits}</span>"#);
    assert!(html.contains(&expected), "{html}");
}

/// The bug this exists for: a site's flat `writings/journal`
/// listing printed a bare "06" for a "1793-06" date, with no year anywhere on
/// the page. `render_child` has no year heading above it (unlike the
/// year-grouped rows), so it must never truncate to month-only.
#[test]
fn flat_row_shows_year_for_month_precision_date() {
    let props = ChildItemProps {
        title: "June Entry".to_string(),
        url: Some("/writings/journal/june.html".to_string()),
        date_display: dated("1793-06", Language::En),
        ..Default::default()
    };
    let html = render_child(&props, crate::i18n::Language::En, None);
    assert!(
        html.contains(r#"<span class="moss-prefix-link-prefix date"><time datetime="1793-06">June 1793</time></span>"#),
        "{html}"
    );
}

/// When only `date_display` is set (no raw date), it is already the compact
/// "year · month" form `props_for` produced — `render_child` must
/// pass it through, not re-derive a month-only string from it.
#[test]
fn flat_row_passes_through_display_only_date_as_is() {
    let props = ChildItemProps {
        title: "Display Only".to_string(),
        url: Some("/writings/display-only.html".to_string()),
        date_display: Some("1793 · 06".to_string()),
        ..Default::default()
    };
    let html = render_child(&props, crate::i18n::Language::En, None);
    assert!(
        html.contains(r#"<span class="moss-prefix-link-prefix date">1793 · 06</span>"#),
        "{html}"
    );
}

/// A year-only date has no month to lose — the flat row shows the bare year.
#[test]
fn flat_row_shows_bare_year_for_year_precision_date() {
    let props = ChildItemProps {
        title: "1793 Entry".to_string(),
        url: Some("/writings/journal/year.html".to_string()),
        date_display: dated("1793", Language::En),
        ..Default::default()
    };
    let html = render_child(&props, crate::i18n::Language::En, None);
    assert!(
        html.contains(r#"<span class="moss-prefix-link-prefix date"><time datetime="1793">1793</time></span>"#),
        "{html}"
    );
}

// ── props_for ─────────────────────────────────────────────────
//
// These behaviours all existed before the hoist, reachable only through
// `folder_embed::generate_children`. Testing them directly is half of what the
// hoist bought.

use crate::build::types::ParsedDocument;
use moss_core::PageKind;
use std::collections::HashMap;

fn doc(url_path: &str, label: &str) -> ParsedDocument {
    ParsedDocument {
        title: label.to_string(),
        label: label.to_string(),
        url_path: url_path.to_string(),
        kind: PageKind::Article,
        lang: Language::En,
        ..Default::default()
    }
}

/// The build's pass results on a page, handed to the page's `PageFacts` by url path.
struct FactsHost<'a> {
    docs: &'a [ParsedDocument],
    cards: BuildCards<'a>,
}

impl FactsHost<'_> {
    fn doc(&self, page: &PageFacts) -> &ParsedDocument {
        self.docs.iter().find(|d| d.url_path == page.url_path).unwrap()
    }
}

impl CardHost<PageFacts> for FactsHost<'_> {
    fn resolve_url(&self, path: &str) -> String { self.cards.resolve_url(path) }
    fn typesetting(&self) -> Option<&str> { self.cards.typesetting() }
    fn now(&self) -> &str { self.cards.now() }
    fn term_card(&self, page: &PageFacts) -> Option<moss_core::page::card::TermCardFacts> { self.cards.term_card(self.doc(page)) }
    fn body_cover(&self, page: &PageFacts) -> Option<String> { self.cards.body_cover(self.doc(page)) }
    fn place_names(&self, page: &PageFacts) -> Option<String> { self.cards.place_names(self.doc(page)) }
    fn credits(&self, page: &PageFacts) -> Option<String> { self.cards.credits(self.doc(page)) }
    fn section(&self, page: &PageFacts) -> Option<crate::build::types::SectionAccent> { self.cards.section(self.doc(page)) }
    fn pictures_after_cover(&self, page: &PageFacts, n: usize) -> Vec<String> { self.cards.pictures_after_cover(self.doc(page), n) }
}

/// A card's props read from the build's document and from its `PageFacts`,
/// which must be the same card.
fn props_at(d: &ParsedDocument, all: &[ParsedDocument], overrides: &HashMap<String, String>, now: chrono::NaiveDateTime) -> ChildItemProps {
    let built = props_for(d, all, &BuildCards::new(overrides, None, now), None);
    let facts: Vec<PageFacts> = all.iter().map(PageFacts::from_page).collect();
    let host = FactsHost { docs: all, cards: BuildCards::new(overrides, None, now) };
    let from_facts = props_for(&PageFacts::from_page(d), &facts, &host, None);
    assert_eq!(format!("{built:?}"), format!("{from_facts:?}"));
    built
}

/// A dated leaf's `date_display`, as the build's `props_for` writes it.
fn dated(raw: &str, lang: Language) -> Option<String> {
    let d = ParsedDocument { date: Some(raw.into()), lang, ..doc("p.html", "P") };
    props(&d, std::slice::from_ref(&d)).date_display
}

fn props(d: &ParsedDocument, all: &[ParsedDocument]) -> ChildItemProps {
    props_at(d, all, &HashMap::new(), chrono::NaiveDateTime::default())
}

#[test]
fn a_folder_carries_its_child_count_and_its_latest_child_date() {
    let docs = vec![
        ParsedDocument { kind: PageKind::Folder, ..doc("works/index.html", "Works") },
        ParsedDocument { date: Some("2023-01-05".into()), ..doc("works/one/index.html", "One") },
        ParsedDocument { date: Some("2024-06-30".into()), ..doc("works/two/index.html", "Two") },
    ];
    let p = props(&docs[0], &docs);
    assert_eq!(p.child_count, Some(2));
    assert_eq!(p.date_raw.as_deref(), Some("2024-06-30"));
    // A folder's own row shows the count, never a date display.
    assert_eq!(p.date_display, None);
}

/// The card's count is recursive: a page in a subfolder counts toward the
/// parent folder's card even though the subfolder's own listing (`direct` by
/// default) would not show it. Only the subfolder's own index page — a
/// `Folder`, not an `Article` — is excluded.
#[test]
fn a_folder_card_counts_pages_in_a_subfolder_too() {
    let docs = vec![
        ParsedDocument { kind: PageKind::Folder, ..doc("works/index.html", "Works") },
        ParsedDocument { date: Some("2023-01-05".into()), ..doc("works/one/index.html", "One") },
        ParsedDocument { kind: PageKind::Folder, ..doc("works/letters/index.html", "Letters") },
        ParsedDocument { date: Some("2024-06-30".into()), ..doc("works/letters/first/index.html", "First") },
        ParsedDocument { date: Some("2024-07-01".into()), ..doc("works/letters/second/index.html", "Second") },
    ];
    let p = props(&docs[0], &docs);
    assert_eq!(p.child_count, Some(3), "direct page + both nested letters, subfolder index excluded");
}

#[test]
fn a_leaf_falls_back_from_frontmatter_to_a_filename_date() {
    let docs = vec![doc("posts/2021-04-09-spring.html", "Spring")];
    let p = props(&docs[0], &docs);
    assert_eq!(p.child_count, None);
    // The raw date is the filename stem as written; only the leading
    // year-month run has to parse.
    assert_eq!(p.date_raw.as_deref(), Some("2021-04-09-spring"));
    assert_eq!(p.date_display.as_deref(), Some("<time datetime=\"2021-04-09\">April 2021</time>"));
}

#[test]
fn a_dateless_leaf_gets_no_date_display() {
    let docs = vec![doc("posts/undated/index.html", "Undated")];
    let p = props(&docs[0], &docs);
    // The filesystem rungs find nothing under an empty root, and a
    // non-explicit date must not be displayed as if the author wrote it.
    assert_eq!(p.date_display, None);
}

#[test]
fn an_external_url_takes_the_card_href_and_the_local_one_becomes_the_permalink() {
    let mut d = doc("links/elsewhere/index.html", "Elsewhere");
    d.raw_frontmatter.insert(
        "external_url".to_string(),
        serde_json::Value::String("https://example.com/piece".to_string()),
    );
    d.raw_frontmatter.insert(
        "publisher".to_string(),
        serde_json::Value::String("The Paper".to_string()),
    );
    let docs = vec![d];
    let p = props(&docs[0], &docs);
    assert_eq!(p.url.as_deref(), Some("https://example.com/piece"));
    assert_eq!(p.permalink.as_deref(), Some("/links/elsewhere/"));
    assert_eq!(p.kicker.as_deref(), Some("The Paper"));
}

#[test]
fn an_ordinary_card_has_no_permalink() {
    let docs = vec![doc("posts/local/index.html", "Local")];
    let p = props(&docs[0], &docs);
    assert_eq!(p.url.as_deref(), Some("/posts/local/"));
    assert_eq!(p.permalink, None);
}

#[test]
fn a_pipe_encoded_cover_resolves_only_its_path_and_keeps_its_attrs() {
    let docs = vec![ParsedDocument {
        cover: Some("images/plate.png|crop:top".to_string()),
        ..doc("works/plate/index.html", "Plate")
    }];
    let mut overrides = HashMap::new();
    overrides.insert("images".to_string(), "media".to_string());
    let p = props_at(&docs[0], &docs, &overrides, chrono::NaiveDateTime::default());
    assert_eq!(p.cover.as_deref(), Some("/media/plate.png|crop:top"));
    assert_eq!(p.cover_type, Some(moss_core::page::card::CoverType::Image));
}

#[test]
fn frontmatter_description_only_and_blank_is_none() {
    let docs = vec![
        ParsedDocument { description: Some("  ".into()), ..doc("a/index.html", "A") },
        ParsedDocument { description: Some("Real.".into()), ..doc("b/index.html", "B") },
    ];
    assert_eq!(props(&docs[0], &docs).description, None);
    assert_eq!(props(&docs[1], &docs).description.as_deref(), Some("Real."));
}

#[test]
fn a_card_with_no_description_shows_the_first_byline_row() {
    let docs = vec![ParsedDocument {
        byline: vec!["Riverside Hall, 2024".into(), "Directed by A. Name".into()],
        ..doc("a/index.html", "A")
    }];
    assert_eq!(props(&docs[0], &docs).description.as_deref(), Some("Riverside Hall, 2024"));
}

#[test]
fn an_explicit_description_wins_over_the_byline_on_a_card() {
    let docs = vec![ParsedDocument {
        description: Some("The described text.".into()),
        byline: vec!["Riverside Hall, 2024".into()],
        ..doc("a/index.html", "A")
    }];
    assert_eq!(props(&docs[0], &docs).description.as_deref(), Some("The described text."));
}

#[test]
fn a_byline_beats_the_body_excerpt_in_a_folder_listing() {
    let text = crate::build::components::child_list::explicit_card_text(&ParsedDocument {
        byline: vec!["Riverside Hall, 2024".into()],
        ..doc("a/index.html", "A")
    });
    assert_eq!(
        crate::build::page::meta::resolve_page_description(text.as_deref(), "A long body paragraph.", false).as_deref(),
        Some("Riverside Hall, 2024"),
    );
}

#[test]
fn an_event_card_keeps_its_meta_line_and_takes_the_byline_only_as_description() {
    let make = |byline: Vec<String>| ParsedDocument {
        event: Some(crate::build::types::EventFields {
            start: moss_core::event::EventTime::parse("2024-05-01").unwrap(),
            end: None,
            when: moss_core::event::EventTime::parse("2024-05-01").unwrap().sort_key(),
            timezone: None,
            status: None,
            tickets: None,
            online: None,
        }),
        location: vec!["Riverside Hall".into()],
        byline,
        ..doc("a/index.html", "A")
    };
    let bare = vec![make(vec![])];
    let with_byline = vec![make(vec!["Riverside Hall, 2024".into()])];
    let (a, b) = (props(&bare[0], &bare), props(&with_byline[0], &with_byline));

    assert_eq!(a.description, None);
    assert_eq!(b.description.as_deref(), Some("Riverside Hall, 2024"));
    assert_eq!(a.date_display, b.date_display);
    assert_eq!(a.date_raw, b.date_raw);
    assert_eq!(a.place, b.place);
    assert_eq!(a.when_html, b.when_html);
    assert_eq!(a.child_count, b.child_count);
    assert_eq!(a.title, b.title);
}

/// `props_for` reads the page's resolved place straight off
/// `ParsedDocument::place_names` — the same field `build::terms::set_term_lines`
/// fills — rather than re-deriving it.
#[test]
fn props_for_carries_the_resolved_place() {
    let docs = vec![ParsedDocument {
        place_names: Some("Kyoto, Osaka".to_string()),
        ..doc("posts/a/index.html", "A")
    }];
    assert_eq!(props(&docs[0], &docs).place.as_deref(), Some("Kyoto, Osaka"));
}

#[test]
fn props_for_has_no_place_when_the_page_names_none() {
    let docs = vec![doc("posts/a/index.html", "A")];
    assert_eq!(props(&docs[0], &docs).place, None);
}

#[test]
fn a_past_event_card_carries_no_tickets_link() {
    let d = vec![ParsedDocument {
        event: Some(crate::build::types::EventFields {
            start: moss_core::event::EventTime::parse("2026-11-21").unwrap(),
            end: None,
            when: moss_core::event::EventTime::parse("2026-11-21").unwrap().sort_key(),
            timezone: None,
            status: None,
            tickets: Some("https://example.test/tickets".into()),
            online: None,
        }),
        ..doc("a/index.html", "A")
    }];
    let at = |s: &str| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap();
    let props_at = |now| props_at(&d[0], &d, &HashMap::new(), now);
    assert!(props_at(at("2026-11-21 23:59")).tickets.is_some());
    assert_eq!(props_at(at("2026-11-22 00:00")).tickets, None);
}

// ── data-source-file ───────────────────────────────────────────────────
//
// A card is rendered into the listing page but belongs to another file, so
// its root names that file for click-to-source.

#[test]
fn every_card_style_names_the_file_it_stands_for() {
    use moss_core::render::cards::{grid as grid_card, summary as child_summary};
    let d = ParsedDocument {
        date: Some("2024-03-02".into()),
        source_path: Some("posts/first & last.md".into()),
        ..doc("posts/first/index.html", "First")
    };
    let p = props(&d, std::slice::from_ref(&d));
    assert_eq!(p.source_file.as_deref(), Some("posts/first & last.md"));
    let attr = r#"data-source-file="posts/first &amp; last.md""#;

    let row = render_child(&p, Language::En, None);
    assert!(row.starts_with(&format!(r#"<div class="moss-card" {attr}>"#)), "{row}");
    let summary = child_summary::render_with_sort(&p, Language::En, None, &(), moss_core::sort::SortAxis::Date);
    assert!(summary.contains(&format!(r#"class="moss-card" {attr}>"#)), "{summary}");
    let grid = grid_card::render_item_with_typesetting(&p, &(), Language::En, None, moss_core::render::image::Loading::Lazy, false, None);
    assert!(grid.starts_with("<a ") && grid.contains(&format!(r#" {attr}>"#)), "{grid}");
    let year_row = render(
        &ArticleListItemProps {
            source_file: p.source_file.clone(),
            url: p.url.clone(),
            title: p.title.clone(),
            url_path: p.url_path.clone(),
            ..Default::default()
        },
        true,
        false,
        Language::En,
        None,
    );
    assert!(year_row.starts_with(&format!(r#"<p class="moss-card" {attr}>"#)), "{year_row}");
}

#[test]
fn a_listing_date_rejects_a_filename_month_or_day_out_of_range() {
    use moss_core::page::card::listing_date;
    assert_eq!(listing_date(&doc("posts/2021-99-09-spring.html", "A")), None);
    assert_eq!(listing_date(&doc("posts/2021-04-40-spring.html", "B")), None);
}

#[test]
fn a_listing_date_takes_an_events_start_over_its_date_over_its_filename() {
    use moss_core::page::card::listing_date;
    let mut d = ParsedDocument { date: Some("2020-01-01".into()), ..doc("posts/2021-04-09-spring.html", "A") };
    assert_eq!(listing_date(&d).as_deref(), Some("2020-01-01"));
    let start = moss_core::event::EventTime::parse("2026-11-01 14:00").unwrap();
    d.event = Some(crate::build::types::EventFields { start, end: None, when: "2026-11-01 14:00".into(), timezone: None, status: None, tickets: None, online: None });
    assert_eq!(listing_date(&d).as_deref(), Some("2026-11-01 14:00"));
}

// ── the core renderers drawn with the build's pictures ──

use moss_core::render::cards::grid::{render_item_with_typesetting, render_list_with_typesetting};
use moss_core::render::image::Loading;


/// The first row of a cover grid is on the first screen, and its largest
/// paint is often not the first card: those covers load with the page, and
/// only the first takes the high-priority fetch.
#[test]
fn a_grids_first_covers_load_with_the_page_and_only_the_first_at_high_priority() {
    let lookup = crate::build::media::dimensions::MediaDimensionLookup::new(&[], &[], &std::collections::HashMap::new(), None, &Default::default());
    let cards: Vec<ChildItemProps> = (1..=5)
        .map(|n| ChildItemProps { title: format!("Card {n}"), url: Some(format!("c{n}/")), cover: Some(format!("cover{n}.jpg")), ..ChildItemProps::default() })
        .collect();
    let html = render_list_with_typesetting(&cards, &crate::build::media::cover::BuildPictures { root: None, lookup: Some(&lookup) }, Language::En, None, moss_core::sort::SortAxis::Title, false, &Default::default(), false);
    let img = |n: usize| {
        let at = html.find(&format!(r#"src="cover{n}.jpg""#)).unwrap_or_else(|| panic!("cover{n} missing: {html}"));
        let start = html[..at].rfind("<img").unwrap();
        &html[start..start + html[start..].find('>').unwrap()]
    };
    for n in 1..=4 {
        assert!(img(n).contains(r#"loading="eager""#), "cover{n}: {}", img(n));
    }
    assert!(img(1).contains(r#"fetchpriority="high""#), "{}", img(1));
    assert_eq!(html.matches("fetchpriority").count(), 1, "{html}");
    assert!(img(5).contains(r#"loading="lazy""#), "{}", img(5));
}

/// The band colour is resolved here, where it is consumed, from the cover the
/// props carry — rung 1 of the ladder, the `|color=` override, needs no root
/// and no cache. #0a2a3f is already past 4.5:1 against white, so its lightness
/// survives the contrast step (14%) rather than being darkened further.
#[test]
fn a_card_takes_its_band_color_from_the_covers_color_override() {
    let props = ChildItemProps {
        credits: None,
        tickets: None,
        title: "Research".to_string(),
        url: Some("/research/".to_string()),
        child_count: Some(3),
        cover: Some("research.jpg|color=#0a2a3f".to_string()),
        ..ChildItemProps::default()
    };
    let html = render_item_with_typesetting(&props, &crate::build::media::cover::BuildPictures { root: None, lookup: None }, Language::En, None, Loading::Lazy, false, None);
    // The colour rides on the outer <a>, not on .moss-card-content.
    assert!(
        html.contains(r#"<a href="/research/" class="moss-card" aria-label="Research" data-cover-color style="--item-cover-color: hsla(203, 73%, 14%, 1)">"#),
        "outer <a> must carry the band colour, got: {html}"
    );
    // A cover with no override, no root and no cache yields no colour at all.
    let plain = ChildItemProps { cover: Some("research.jpg".to_string()), ..props };
    let html = render_item_with_typesetting(&plain, &crate::build::media::cover::BuildPictures { root: None, lookup: None }, Language::En, None, Loading::Lazy, false, None);
    assert!(!html.contains("data-cover-color"), "got: {html}");
}

/// A theme lays cards out by each cover's shape, so the card carries it. Only
/// a size moss read counts: an unread picture or a video gets no property,
/// and the theme's fallback applies.
#[test]
fn a_picture_cover_publishes_its_shape() {
    use crate::build::media::dimensions::MediaDimensionLookup;
    let meta = crate::types::content::MediaMetadata {
        path: "prints/wave.jpg".to_string(),
        dimensions: Some((1500, 1000)),
        ..Default::default()
    };
    let flat = crate::types::content::MediaMetadata { path: "prints/flat.jpg".to_string(), dimensions: Some((0, 1000)), ..Default::default() };
    let lookup = MediaDimensionLookup::new(&[meta, flat], &[], &std::collections::HashMap::new(), None, &Default::default());
    let card = |cover: &str| {
        let props = ChildItemProps { title: "Wave".to_string(), url: Some("wave/".to_string()), cover: Some(cover.to_string()), ..ChildItemProps::default() };
        render_item_with_typesetting(&props, &crate::build::media::cover::BuildPictures { root: None, lookup: Some(&lookup) }, Language::En, None, Loading::Lazy, false, None)
    };

    assert!(card("prints/wave.jpg").contains("--item-cover-ratio: 1500 / 1000"), "{}", card("prints/wave.jpg"));
    assert!(card("prints/wave.jpg|fit=contain").contains("--item-cover-ratio: 1500 / 1000"));
    assert!(!card("prints/unread.jpg").contains("--item-cover-ratio"));
    assert!(!card("prints/flat.jpg").contains("--item-cover-ratio"));
    assert!(!card("prints/clip.mp4").contains("--item-cover-ratio"));
}
