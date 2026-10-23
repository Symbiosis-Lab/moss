//! A footer.md that ends in a `:::hero` closes the page with that picture: the
//! hero renders in the footer (not hoisted to the page's hero slot, which a
//! footer has none of), is marked as the closing picture, stays outside the
//! footer's heading columns, and has the colophon after it inside the footer.

#[path = "support/site.rs"]
mod site;

use site::Site;

/// 1x1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49,
    0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0xF0, 0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x89, 0x99, 0x3D,
    0x1D, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

const HERO: &str = ":::hero {image=day.png dark=night.png align=top}\n## The shelf is open.\n\n[Propose a collection](https://example.org/new)\n:::\n";

fn build(footer: &str) -> Site {
    let site = Site::new(&[("index.md", "---\ntitle: Harbour\n---\n\nBody text.\n"), ("footer.md", footer)]);
    for name in ["day.png", "night.png"] {
        std::fs::write(site.root.join(name), PNG).unwrap();
    }
    site.rebuild();
    site
}

fn footer_of(html: &str) -> &str {
    let from = html.find("<footer").expect("a footer");
    &html[from..html[from..].find("</footer>").map(|i| from + i).expect("footer closes")]
}

#[test]
fn a_last_block_hero_is_the_footers_closing_picture() {
    let site = build(HERO);
    let html = site.html("index.html");
    let footer = footer_of(&html);

    let hero_at = footer.find("<section class=\"moss-hero moss-hero-closing\"").expect("the hero renders in the footer");
    assert!(footer.contains("data-mobile=\"overlay\""), "words stay on the picture on a phone: {footer}");
    assert!(footer.contains("data-align=\"top\""), "{footer}");
    assert!(footer.contains("The shelf is open."), "{footer}");
    assert!(
        footer.find("moss-colophon").unwrap() > hero_at,
        "the colophon follows the picture inside the footer"
    );
    assert!(!html[..html.find("<footer").unwrap()].contains("moss-hero"), "not hoisted above the page");
}

#[test]
fn the_hero_pairs_a_light_and_a_dark_picture() {
    let site = build(HERO);
    let html = site.html("index.html");
    let pictures: Vec<&str> = footer_of(&html).split("<picture").skip(1).collect();
    assert_eq!(pictures.len(), 2);
    assert!(pictures[0].contains("/day.") && pictures[0].contains("data-hero-variant=\"light\""));
    assert!(pictures[1].contains("/night.") && pictures[1].contains("data-hero-variant=\"dark\""));
}

#[test]
fn a_closing_hero_stays_outside_the_heading_columns() {
    let site = build(&format!("## Contact\n\nWrite to us.\n\n{HERO}"));
    let html = site.html("index.html");
    let footer = footer_of(&html);
    let columns_end = footer.find("</section>").expect("the Contact column closes");
    assert!(footer[..columns_end].contains("Write to us."));
    assert!(footer[columns_end..].contains("moss-hero-closing"), "the picture follows the columns: {footer}");
    assert!(!footer[..columns_end].contains("moss-hero"), "and is not inside one: {footer}");
}

#[test]
fn a_hero_with_words_after_it_is_an_ordinary_hero() {
    let site = build(&format!("{HERO}\nMade with care.\n"));
    let html = site.html("index.html");
    assert!(!footer_of(&html).contains("moss-hero-closing"));
}

/// A 160x70 picture: `top` over the first 70% of its rows, `bottom` below.
fn picture(top: [u8; 3], bottom: [u8; 3], hatched_top: bool) -> Vec<u8> {
    let mut img = image::RgbImage::new(160, 70);
    for (_, y, p) in img.enumerate_pixels_mut() {
        let dark_line = hatched_top && y < 49 && y % 2 == 0;
        *p = image::Rgb(if dark_line { [20, 30, 50] } else if y < 49 { top } else { bottom });
    }
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

const SKY: [u8; 3] = [205, 218, 232];
const GROUND: [u8; 3] = [30, 40, 60];

/// A site whose footer closes on `hero` (a `:::hero` block), over a quiet sky
/// (`day.png`, `night.png`) or a hatched one.
fn site_with(hero: &str, hatched: bool) -> String {
    let site = Site::new(&[("index.md", "---\ntitle: Harbour\n---\n\nBody text.\n"), ("footer.md", hero)]);
    std::fs::write(site.root.join("day.png"), picture(SKY, GROUND, hatched)).unwrap();
    std::fs::write(site.root.join("night.png"), picture([20, 28, 48], [230, 235, 240], hatched)).unwrap();
    site.rebuild();
    footer_of(&site.html("index.html")).to_string()
}

#[test]
fn a_quiet_sky_takes_ink_with_no_panel_and_a_busy_one_keeps_its_panel() {
    let quiet = site_with(HERO, false);
    // A closing picture has a reading for each layout and the page's column and contents lane.
    let everywhere = |ink: &str| {
        let mut tokens = vec![format!("narrow:{ink}")];
        for layout in ["mid", "wide"] {
            for lane in ["", "-lane"] {
                for column in ["default", "wide", "full"] {
                    tokens.push(format!("{layout}-{column}{lane}:{ink}"));
                }
            }
        }
        tokens.sort();
        tokens
    };
    let tokens_of = |footer: &str, attr: &str| {
        let mut tokens: Vec<String> = footer.split(&format!(" {attr}=\"")).nth(1).and_then(|r| r.split('"').next()).unwrap_or("").split(' ').map(str::to_string).collect();
        tokens.sort();
        tokens
    };
    assert_eq!(tokens_of(&quiet, "data-words"), everywhere("dark"), "dark ink on the pale sky: {quiet}");
    assert_eq!(tokens_of(&quiet, "data-words-night"), everywhere("light"), "light ink on the dark picture: {quiet}");
    assert!(quiet.contains(r#"data-foot="light""#), "the foot of the day picture is dark ground: {quiet}");

    let busy = site_with(HERO, true);
    assert!(!busy.contains(r#"data-words=""#), "a hatched sky keeps the panel: {busy}");
}

#[test]
fn panel_forces_the_backing_either_way() {
    let always = site_with(&HERO.replace("align=top", "align=top panel=always"), false);
    assert!(!always.contains("data-words"), "{always}");
    let night_only = site_with(HERO, true);
    assert!(night_only.contains("data-words-night"), "the pairs are measured apart: {night_only}");
    let never = site_with(&HERO.replace("align=top", "align=top panel=none"), true);
    assert!(never.contains("data-words=\""), "ink on a busy sky when asked: {never}");
}

#[test]
fn a_closing_hero_caption_is_its_credit_inside_the_picture() {
    let footer = site_with(&HERO.replace("align=top", "align=top caption=\"Etching, 1643.\""), false);
    let section_end = footer.find("</section>").unwrap();
    let credit = footer.find(r#"<p class="moss-hero-credit""#).expect("the credit renders");
    assert!(footer[credit..].contains(">Etching, 1643.</p>"), "{footer}");
    assert!(credit < section_end, "inside the picture's box: {footer}");
    assert!(!footer.contains("data-captioned") && !footer.contains(r#"class="moss-hero-caption""#), "{footer}");
}

#[test]
fn an_ordinary_hero_keeps_its_panel_unless_it_asks_to_measure() {
    let page = |hero: &str| {
        let site = Site::new(&[("index.md", &format!("---\ntitle: Harbour\n---\n\n{hero}\n"))]);
        std::fs::write(site.root.join("day.png"), picture(SKY, GROUND, false)).unwrap();
        site.rebuild();
        site.html("index.html")
    };
    let plain = page(":::hero {image=day.png align=top}\n## Hello\n:::");
    assert!(!plain.contains("data-words"), "today's heroes are unchanged: {plain}");
    let auto = page(":::hero {image=day.png align=top panel=auto}\n## Hello\n:::");
    assert!(auto.contains(r#"data-words="narrow:dark mid:dark wide:dark""#), "{auto}");
}

#[test]
fn an_ordinary_overlay_heros_caption_still_renders_below_its_picture() {
    let site = Site::new(&[(
        "index.md",
        "---\ntitle: Harbour\n---\n\n:::hero {image=day.png caption=\"Etching, 1643.\"}\n## Hello\n:::\n",
    )]);
    std::fs::write(site.root.join("day.png"), picture(SKY, GROUND, false)).unwrap();
    site.rebuild();
    let html = site.html("index.html");
    assert!(html.contains(r#"</section><p class="moss-hero-caption">Etching, 1643.</p>"#), "{html}");
    assert!(!html.contains("moss-hero-credit"), "{html}");
}

#[test]
fn panel_none_over_a_picture_ink_cannot_read_on_warns_naming_the_hero() {
    let site = Site::new(&[]);
    std::fs::write(site.root.join("mid.png"), picture([120; 3], [120; 3], false)).unwrap();
    site.write("index.md", "---\ntitle: Harbour\n---\n\n:::hero {image=mid.png align=top panel=none}\n## Open all hours today\n:::\n");
    let logged = site.rebuild().join("\n");
    assert!(logged.contains("panel=none") && logged.contains("Open all hours today") && logged.contains("4.5:1"), "{logged}");
    // And a picture ink reads on says nothing.
    std::fs::write(site.root.join("mid.png"), picture(SKY, GROUND, false)).unwrap();
    assert!(!site.rebuild().join("\n").contains("panel=none"));
}

#[test]
fn plain_ink_on_a_closing_picture_is_noted_by_layout_and_theme() {
    let site = Site::new(&[("index.md", "---\ntitle: Harbour\n---\n\nBody text.\n"), ("footer.md", &HERO.replace("align=top", "align=top panel=none"))]);
    std::fs::write(site.root.join("day.png"), picture(SKY, GROUND, true)).unwrap();
    std::fs::write(site.root.join("night.png"), picture([20, 28, 48], [230, 235, 240], true)).unwrap();
    let logged = site.rebuild().join("\n");
    assert!(logged.contains("panel=none") && logged.contains("the words in the light theme") && logged.contains("needs") && logged.contains("80rem wide"), "the note names the layouts with the real ratios: {logged}");
}

/// The footer of a site closing on `hero` over the quiet pair, with `theme` as
/// its stylesheet.
fn themed(hero: &str, theme: &str) -> String {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Harbour\n---\n\n:::hero {image=day.png fade=top}\n# Harbour\n:::\n\nBody text.\n"),
        ("footer.md", hero),
        (".moss/theme/style.css", theme),
    ]);
    std::fs::write(site.root.join("day.png"), picture(SKY, GROUND, false)).unwrap();
    std::fs::write(site.root.join("night.png"), picture([20, 28, 48], [230, 235, 240], false)).unwrap();
    site.rebuild();
    site.html("index.html")
}

const FADED: &str = ":::hero {image=day.png dark=night.png align=top fade=top caption=\"Etching, 1643.\"}\n## The shelf is open.\n\n[Propose a collection](https://example.org/new)\n:::\n";

#[test]
fn a_faded_closing_picture_is_measured_over_the_themes_page_colour() {
    let plain = themed(FADED, "h1 { letter-spacing: 0; }\n");
    let footer = footer_of(&plain);
    assert!(footer.contains(r#"data-fade="top""#), "{footer}");
    assert!(footer.contains(r#"data-words="narrow:dark"#) && footer.contains(r#"data-words-night="narrow:light"#), "{footer}");
    // The credit is measured at the foot: the dark ground takes the page colour in the light theme.
    assert!(footer.contains(r#"data-credit="narrow:page mid:page wide:page""#) && footer.contains("data-credit-night="), "{footer}");
    assert!(!plain[..plain.find("<footer").unwrap()].contains("data-fade"), "only a closing picture fades");

    // A black page under the pale day picture: the faded top is dark, and the words take the
    // theme's own inks there, its dark text and the black page: on a phone, where the words sit wholly in the
    // faded top, neither reads and they keep their panel.
    let black = footer_of(&themed(FADED, ":root { --moss-color-bg: #000; }\n")).to_string();
    let day = black.split(r#"data-words=""#).nth(1).and_then(|rest| rest.split('"').next()).unwrap_or("");
    assert!(!day.contains("narrow:"), "no ink reads over the black page they fade into: {black}");
    assert!(black.contains(r#"data-words-night="narrow:light"#), "{black}");

    // A colour the build can't read keeps the words' panels in both themes, and
    // the credit cannot take it; the foot is unaffected.
    let unread = footer_of(&themed(FADED, ":root { --moss-color-bg: var(--paper); }\n")).to_string();
    assert!(!unread.contains("data-words") && !unread.contains(":page"), "{unread}");
    assert!(unread.contains(r#"data-foot="light""#), "{unread}");
}

#[test]
fn fade_on_a_hero_that_does_not_close_the_page_warns() {
    let site = Site::new(&[("index.md", "---\ntitle: Harbour\n---\n\n:::hero {image=day.png fade=top}\n# Harbour\n:::\n\nBody text.\n")]);
    std::fs::write(site.root.join("day.png"), picture(SKY, GROUND, false)).unwrap();
    let logged = site.rebuild().join("\n");
    assert!(logged.contains("fade=top") && logged.contains("closing picture"), "{logged}");
    // Another value is the parser's to report as unrecognized, not this warning's.
    site.write("index.md", "---\ntitle: Harbour\n---\n\n:::hero {image=day.png fade=bottom}\n# Harbour\n:::\n");
    let logged = site.rebuild().join("\n");
    assert!(!logged.contains("closing picture"), "{logged}");
    site.write("footer.md", FADED);
    site.write("index.md", "---\ntitle: Harbour\n---\n\nBody text.\n");
    assert!(!site.rebuild().join("\n").contains("fade=top"));
}

#[test]
fn a_page_colour_edit_remeasures_the_faded_picture_on_a_content_only_rebuild() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Harbour\n---\n\nBody text.\n"),
        ("footer.md", FADED),
        (".moss/theme/style.css", "h1 { letter-spacing: 0; }\n"),
    ]);
    std::fs::write(site.root.join("day.png"), picture(SKY, GROUND, false)).unwrap();
    std::fs::write(site.root.join("night.png"), picture([20, 28, 48], [230, 235, 240], false)).unwrap();
    site.rebuild();
    assert!(footer_of(&site.html("index.html")).contains("data-words="));
    site.write(".moss/theme/style.css", ":root { --moss-color-bg: var(--paper); }\n");
    let changed = site.root.join(".moss/theme/style.css");
    site::build_sync(&site.root.to_string_lossy(), moss_build::build::BuildTrigger::ContentOnly(vec![changed])).unwrap();
    let footer = footer_of(&site.html("index.html")).to_string();
    assert!(!footer.contains("data-words") && !footer.contains("data-credit"), "a cached parse replayed the old measurement: {footer}");
}
