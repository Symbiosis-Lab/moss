//! `:::hero {from="shows/|when:upcoming"}` draws the first page of that
//! listing; the fence's own image and words are what shows when the listing is
//! empty.

#[path = "support/site.rs"]
mod site;

use site::{hero_of, Site};

/// 1x1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49,
    0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0xF0, 0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x89, 0x99, 0x3D,
    0x1D, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

const GONE: &str = "---\ntitle: Gone Show\nstart: 2001-05-05 10:00\n---\n\nBody.\n";
const SOONER: &str = "---\ntitle: Sooner Show\nstart: 2099-03-01 19:30\nlocation: Example Hall\ndescription: A play about bridges.\n---\n\n![bridge](sooner.png)\n\nBody.\n";
const LATER: &str = "---\ntitle: Later Show\nstart: 2099-09-01 19:30\n---\n\nBody.\n";

fn home(hero: &str) -> String {
    format!("---\ntitle: Example Company\n---\n\n{hero}\n\n## Work\n\nText below.\n")
}

fn build(files: &[(&str, &str)], png: &[&str]) -> Site {
    let site = Site::new(files);
    for p in png {
        std::fs::create_dir_all(site.root.join(p).parent().unwrap()).unwrap();
        std::fs::write(site.root.join(p), PNG).unwrap();
    }
    if png.is_empty() {
        return site;
    }
    site.rebuild();
    site
}

fn shows() -> Vec<(&'static str, &'static str)> {
    vec![
        ("shows/index.md", "---\ntitle: Shows\nchildren: false\n---\n"),
        ("shows/gone.md", GONE),
        ("shows/sooner.md", SOONER),
        ("shows/later.md", LATER),
    ]
}

fn with_home<'a>(hero: &'a str, extra: &[(&'a str, &'a str)]) -> Vec<(&'a str, String)> {
    let mut files: Vec<(&str, String)> = vec![("index.md", home(hero))];
    files.extend(shows().into_iter().map(|(p, b)| (p, b.to_string())));
    files.extend(extra.iter().map(|(p, b)| (*p, b.to_string())));
    files
}

fn site_with(hero: &str, extra: &[(&str, &str)], png: &[&str]) -> Site {
    let files = with_home(hero, extra);
    let refs: Vec<(&str, &str)> = files.iter().map(|(p, b)| (*p, b.as_str())).collect();
    build(&refs, png)
}

#[test]
fn hero_from_draws_the_next_upcoming_event() {
    let site = site_with(
        ":::hero {from=\"shows/|when:upcoming\"}\n# Between seasons\n:::",
        &[],
        &["shows/sooner.png"],
    );
    let html = site.html("index.html");
    let hero = hero_of(&html);
    assert!(hero.contains("Sooner Show"), "the soonest upcoming event is drawn: {hero}");
    assert!(!hero.contains("Later Show") && !hero.contains("Gone Show"), "only the first page: {hero}");
    assert!(hero.contains("moss-event-meta") && hero.contains("data-when=\"upcoming\""), "event line: {hero}");
    assert!(hero.contains("Example Hall"), "its place: {hero}");
    assert!(hero.contains("A play about bridges."), "its description: {hero}");
    assert!(hero.contains("href=\"/shows/sooner/\"") && hero.contains("More about Sooner Show"), "link: {hero}");
    assert!(hero.contains("<picture>") && hero.contains("/shows/sooner."), "its picture, with the build's sources: {hero}");
    assert!(!hero.contains("Between seasons"), "the fence words are only the fallback: {hero}");
    assert!(!html.contains("MOSS_MARKER"), "no raw marker reaches the page");
}

#[test]
fn hero_from_falls_back_when_nothing_is_upcoming() {
    let site = Site::new(&[
        ("index.md", &home(":::hero {from=\"shows/|when:upcoming\" image=photos/co.png}\n# Between seasons\n:::")),
        ("photos/co.png", ""),
        ("shows/index.md", "---\ntitle: Shows\nchildren: false\n---\n"),
        ("shows/gone.md", GONE),
    ]);
    std::fs::write(site.root.join("photos/co.png"), PNG).unwrap();
    site.rebuild();
    let hero = hero_of(&site.html("index.html"));
    assert!(hero.contains("Between seasons"), "the fence words show: {hero}");
    assert!(hero.contains("src=\"/photos/co."), "the fence image shows: {hero}");
    assert!(!hero.contains("Gone Show"), "a past page is not drawn: {hero}");
}

#[test]
fn hero_from_with_no_fallback_renders_no_hero() {
    let site = Site::new(&[
        ("index.md", &home(":::hero {from=\"shows/|when:upcoming\"}\n:::")),
        ("shows/index.md", "---\ntitle: Shows\nchildren: false\n---\n"),
        ("shows/gone.md", GONE),
    ]);
    let html = site.html("index.html");
    assert!(!html.contains("<section class=\"moss-hero"), "no empty hero box");
    assert!(html.contains("Text below."), "the page starts with its body");
    assert!(!html.contains("MOSS_MARKER"));
}

#[test]
fn hero_from_unknown_folder_warns_and_falls_back() {
    let site = Site::new(&[
        ("index.md", &home(":::hero {from=\"nowhere/|when:upcoming\"}\n# Between seasons\n:::")),
        ("shows/gone.md", GONE),
    ]);
    let hero = hero_of(&site.html("index.html"));
    assert!(hero.contains("Between seasons"), "fallback shown: {hero}");
    let warned = site.logged.iter().any(|l| l.contains("index.md") && l.contains("nowhere"));
    assert!(warned, "a warning names the page and the folder: {:?}", site.logged);
}

#[test]
fn hero_from_unknown_option_warns() {
    let site = site_with(":::hero {from=\"shows/|when:upcoming,shuffle:yes\"}\n# Between seasons\n:::", &[], &[]);
    assert!(site.logged.iter().any(|l| l.contains("shuffle")), "unknown option warns: {:?}", site.logged);
    assert!(hero_of(&site.html("index.html")).contains("Sooner Show"));
}

#[test]
fn hero_from_in_a_language_tree_draws_that_tree() {
    let site = Site::in_language(
        "en",
        "",
        &[
            ("index.md", "---\ntitle: Home\n---\n\nHi.\n"),
            ("shows/index.md", "---\ntitle: Shows\nchildren: false\n---\n"),
            ("shows/sooner.md", SOONER),
            ("zh-hans/index.md", &home(":::hero {from=\"shows/|when:upcoming\"}\n# 休演期\n:::")),
            ("zh-hans/shows/index.md", "---\ntitle: 演出\nchildren: false\n---\n"),
            (
                "zh-hans/shows/zhong.md",
                "---\ntitle: 中文演出\nstart: 2099-04-01 19:30\n---\n\n正文。\n",
            ),
        ],
    );
    let hero = hero_of(&site.html("zh-hans/index.html"));
    assert!(hero.contains("中文演出"), "draws zh-hans/shows: {hero}");
    assert!(!hero.contains("Sooner Show"), "not the English tree: {hero}");
    assert!(hero.contains("了解更多：中文演出"), "link text is translated: {hero}");
    assert!(hero.contains("href=\"/zh-hans/shows/zhong/\""), "links into its own tree: {hero}");
}

#[test]
fn hero_from_page_without_picture_uses_fallback_image_or_words_only() {
    let with_image = {
        let site = Site::new(&[
            ("index.md", &home(":::hero {from=\"shows/|when:upcoming\" image=photos/co.png}\n# Between seasons\n:::")),
            ("photos/co.png", ""),
            ("shows/index.md", "---\ntitle: Shows\nchildren: false\n---\n"),
            ("shows/later.md", LATER),
        ]);
        std::fs::write(site.root.join("photos/co.png"), PNG).unwrap();
        site.rebuild();
        hero_of(&site.html("index.html"))
    };
    assert!(with_image.contains("Later Show"), "words come from the page: {with_image}");
    assert!(with_image.contains("src=\"/photos/co."), "picture from the fence: {with_image}");

    let words_only = {
        let site = Site::new(&[
            ("index.md", &home(":::hero {from=\"shows/|when:upcoming\"}\n:::")),
            ("shows/index.md", "---\ntitle: Shows\nchildren: false\n---\n"),
            ("shows/later.md", LATER),
        ]);
        hero_of(&site.html("index.html"))
    };
    assert!(words_only.contains("Later Show"), "words-only hero: {words_only}");
    assert!(!words_only.contains("<img"), "no picture anywhere: {words_only}");
}

#[test]
fn hero_from_feeds_the_share_card_fallback() {
    let site = site_with(
        ":::hero {from=\"shows/|when:upcoming\" image=photos/co.png}\n# Between seasons\nCompany line.\n:::",
        &[("photos/co.png", "")],
        &["shows/sooner.png", "photos/co.png"],
    );
    let html = site.html("index.html");
    let og = |prop: &str| {
        let at = html.find(&format!("property=\"{prop}\"")).unwrap_or_else(|| panic!("no {prop}"));
        let tag = &html[html[..at].rfind('<').unwrap()..];
        tag[..tag.find('>').unwrap()].to_string()
    };
    let cover = html.split("data-share-cover=\"").nth(1).and_then(|r| r.split('"').next()).unwrap_or("");
    assert!(cover.contains("sooner"), "share picture is the drawn page's: {cover:?}");
    assert!(og("og:description").contains("A play about bridges."), "share description: {}", og("og:description"));
}

/// Every `<section class="moss-hero…>` opening tag on the page.
fn hero_tags(html: &str) -> Vec<String> {
    html.match_indices("<section class=\"moss-hero")
        .map(|(at, _)| html[at..at + html[at..].find('>').unwrap()].to_string())
        .collect()
}

/// The `<span>` whose opening tag carries `class_name`, through its matching
/// close, nested spans included.
fn span_of(html: &str, class_name: &str) -> String {
    let at = html[..html.find(class_name).unwrap_or_else(|| panic!("no {class_name}"))].rfind("<span").unwrap();
    let (mut depth, mut end) = (0, at);
    for (i, _) in html[at..].match_indices(['<']) {
        let rest = &html[at + i..];
        if rest.starts_with("<span") {
            depth += 1;
        } else if rest.starts_with("</span>") {
            depth -= 1;
            if depth == 0 {
                end = at + i + "</span>".len();
                break;
            }
        }
    }
    html[at..end].to_string()
}

/// The `<template data-hero-ends>` on the page: its end and what it holds.
fn template_of(html: &str) -> (String, String) {
    let at = html.find("<template data-hero-ends=\"").expect("a template beside the drawn hero");
    let rest = &html[at + "<template data-hero-ends=\"".len()..];
    let (ends, rest) = rest.split_once("\">").unwrap();
    (ends.to_string(), rest[..rest.find("</template>").unwrap()].to_string())
}

#[test]
fn hero_from_an_event_carries_its_end_and_keeps_the_fallback_in_a_template() {
    let site = site_with(
        ":::hero {from=\"shows/|when:upcoming\"}\n# Between seasons\n:::",
        &[],
        &[],
    );
    let html = site.html("index.html");
    let (ends, fallback) = template_of(&html);
    assert_eq!(ends, "2099-03-01T19:30", "end as wall-clock time");
    assert!(fallback.starts_with("<section class=\"moss-hero") && fallback.contains("Between seasons"), "the fallback hero: {fallback}");
    let live = hero_tags(&html[..html.find("<template").unwrap()]);
    assert_eq!(live.len(), 1, "only the drawn hero is drawn: {live:?}");
    let template_tag = &html[html.find("<template data-hero-ends").unwrap()..];
    let template_tag = &template_tag[..template_tag.find('>').unwrap()];
    assert!(
        hero_tags(&html).iter().chain([&template_tag.to_string()]).all(|tag| !tag.contains(" hidden")),
        "no hero or template relies on the hidden attribute"
    );
    assert!(!hero_of(&html).contains("Between seasons"));
    assert!(html.contains("/_moss/js/hero-ends."), "the swap script loads on this site");
}

#[test]
fn hero_from_draws_the_event_line_and_link_as_the_site_does_elsewhere() {
    let site = site_with(":::hero {from=\"shows/|when:upcoming\"}\n# Between seasons\n:::", &[], &[]);
    let hero = hero_of(&site.html("index.html"));
    let meta = span_of(&hero, "moss-event-meta");
    assert!(meta.contains("moss-event-upcoming"), "the Upcoming label sits inside the event line: {meta}");
    assert!(hero.contains("<div class=\"moss-buttons\">") && hero.contains("class=\"moss-btn\""), "the link is a moss button: {hero}");
    assert!(!hero.contains("moss-hero-event") && !hero.contains("moss-hero-more"), "no private classes: {hero}");
}

#[test]
fn hero_from_that_names_no_folder_warns_and_falls_back() {
    for spec in ["|when:upcoming", "shows", ""] {
        let site = site_with(&format!(":::hero {{from=\"{spec}\"}}\n# Between seasons\n:::"), &[], &[]);
        let hero = hero_of(&site.html("index.html"));
        assert!(hero.contains("Between seasons"), "from=\"{spec}\" falls back: {hero}");
        if !spec.is_empty() {
            assert!(site.logged.iter().any(|l| l.contains("index.md") && l.contains("hero from")), "from=\"{spec}\" warns: {:?}", site.logged);
        }
    }
}

#[test]
fn hero_from_an_all_day_event_ends_when_its_last_day_does() {
    let site = site_with(
        ":::hero {from=\"days/|when:upcoming\"}\n# Between seasons\n:::",
        &[
            ("days/index.md", "---\ntitle: Days\nchildren: false\n---\n"),
            ("days/fair.md", "---\ntitle: Fair Day\nstart: 2099-05-01\nend: 2099-05-02\n---\n\nBody.\n"),
        ],
        &[],
    );
    assert_eq!(template_of(&site.html("index.html")).0, "2099-05-03T00:00");
}

#[test]
fn hero_from_an_event_without_fallback_words_swaps_to_nothing() {
    let site = site_with(":::hero {from=\"shows/|when:upcoming\"}\n:::", &[], &[]);
    let html = site.html("index.html");
    assert_eq!(hero_tags(&html).len(), 1);
    assert_eq!(template_of(&html).1, "", "an empty template: nothing replaces the drawn hero");
}

#[test]
fn hero_from_a_page_that_is_not_an_event_never_swaps() {
    let site = site_with(
        ":::hero {from=\"news/\"}\n# Between seasons\n:::",
        &[
            ("news/index.md", "---\ntitle: News\nchildren: false\n---\n"),
            ("news/hello.md", "---\ntitle: Hello Post\ndate: 2024-01-01\n---\n\nBody.\n"),
        ],
        &[],
    );
    let html = site.html("index.html");
    assert!(html.contains("Hello Post"));
    assert_eq!(hero_tags(&html).len(), 1, "no fallback copy");
    assert!(!html.contains("data-hero-ends") && !html.contains("hero-ends."), "no swap for a post");
}

#[test]
fn a_site_without_an_event_hero_ships_no_swap_script() {
    let site = Site::new(&[("index.md", &home(":::hero\n# Plain\n:::"))]);
    assert!(!site.html("index.html").contains("hero-ends"));
}

fn cover_shows(cover_line: &str, fence_image: &str) -> String {
    let fence = format!(":::hero {{from=\"shows/|when:upcoming\"{fence_image}}}\n# Between seasons\n:::");
    let sooner = format!("---\ntitle: Sooner Show\nstart: 2099-03-01 19:30\n{cover_line}---\n\nBody.\n");
    let site = site_with(&fence, &[("shows/sooner.md", &sooner)], &["shows/sooner.png", "photos/co.png"]);
    hero_of(&site.html("index.html"))
}

#[test]
fn hero_from_carries_the_covers_crop_attrs_to_the_drawn_hero() {
    let hero = cover_shows("cover: sooner.png|contain\n", " image=photos/co.png");
    assert!(hero.contains("object-fit:contain") || hero.contains("object-fit: contain"), "the cover's crop: {hero}");
}

#[test]
fn hero_from_a_cover_without_attrs_clears_the_fences_framing() {
    let hero = cover_shows("cover: sooner.png\n", " image=\"photos/co.png|contain\"");
    assert!(hero.contains("/shows/sooner."), "the cover is drawn: {hero}");
    assert!(!hero.contains("object-fit"), "the fence's framing does not carry over: {hero}");
}

#[test]
fn hero_from_keeps_an_external_cover_external() {
    let hero = cover_shows("cover: https://example.com/stage.png\n", "");
    assert!(hero.contains("src=\"https://example.com/stage.png\""), "the address is untouched: {hero}");
}
