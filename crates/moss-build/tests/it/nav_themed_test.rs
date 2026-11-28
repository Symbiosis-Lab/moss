//! A page that links a stylesheet of its own marks its masthead
//! `data-nav-themed`, which keeps the flex row that stylesheet was written
//! against out of the plain name's float layout.

use crate::site::Site;

fn page(title: &str) -> String {
    format!("---\ntitle: {title}\n---\n\nBody of {title}.\n")
}

fn themed(html: &str) -> bool {
    html.contains(r#"<div class="nav-content" data-nav-themed>"#)
}

#[test]
fn a_theme_stylesheet_marks_every_masthead_themed() {
    let site = Site::new(&[(".moss/theme/style.css", ".nav-content { gap: 24px }"), ("index.md", &page("Home")), ("about.md", &page("About"))]);
    for path in ["index.html", "about/index.html"] {
        let html = site.html(path);
        assert!(themed(&html), "{path}: {html}");
    }
}

#[test]
fn a_page_stylesheet_marks_only_its_own_masthead_themed() {
    let site = Site::new(&[("index.md", &page("Home")), ("about.md", &page("About")), ("about.css", ".nav-content { gap: 24px }")]);
    assert!(themed(&site.html("about/index.html")));
    let home = site.html("index.html");
    assert!(home.contains(r#"<div class="nav-content">"#) && !themed(&home), "{home}");
}

#[test]
fn a_site_without_a_stylesheet_keeps_the_plain_masthead() {
    let site = Site::new(&[("index.md", &page("Home")), ("about.md", &page("About"))]);
    for path in ["index.html", "about/index.html"] {
        let html = site.html(path);
        assert!(html.contains(r#"<div class="nav-content">"#) && !html.contains("data-nav-themed"), "{path}: {html}");
    }
}

/// The masthead moss wrote for this site before the float layout, when its
/// markup did not depend on the theme. A themed page keeps it byte for byte.
const FLEX_MASTHEAD: &str = r##"<div class="nav-left"><a href="/" class="site-name">Basic Site Homepage</a></div><div class="nav-right"><button class="mobile-menu-button" onclick="toggleMobileMenu()" aria-label="Menu" aria-expanded="false" aria-controls="nav-links"><svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="3" y1="12" x2="21" y2="12"/><line x1="3" y1="6" x2="21" y2="6"/><line x1="3" y1="18" x2="21" y2="18"/></svg></button><div class="nav-links" id="nav-links"><a href="/about/" class="active" aria-current="page">about</a></div><div class="nav-icons"><button class="nav-theme-btn" type="button" aria-label="Toggle theme" onclick="toggleTheme()"><svg class="theme-toggle-icon" xmlns="http://www.w3.org/2000/svg" aria-hidden="true" width="1em" height="1em" fill="currentColor" stroke-linecap="round" viewBox="0 0 32 32"><clipPath id="theme-toggle__classic__cutout"><path d="M0-5h30a1 1 0 0 0 9 13v24H0Z"/></clipPath><g clip-path="url(#theme-toggle__classic__cutout)"><circle cx="16" cy="16" r="9.34"/><g stroke="currentColor" stroke-width="1"><path d="M16 5.5v-4"/><path d="M16 30.5v-4"/><path d="M1.5 16h4"/><path d="M26.5 16h4"/><path d="m23.4 8.6 2.8-2.8"/><path d="m5.7 26.3 2.9-2.9"/><path d="m5.8 5.8 2.8 2.8"/><path d="m23.4 23.4 2.9 2.9"/></g></g></svg></button></div></div>"##;

#[test]
fn a_themed_masthead_is_the_flex_masthead_themes_were_written_against() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/snapshot-sites/basic-site/input/");
    let read = |name: &str| std::fs::read_to_string(format!("{fixture}{name}")).unwrap();
    let (index, about, contact) = (read("index.md"), read("about.md"), read("contact.md"));
    let site = Site::new(&[
        (".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n"),
        (".moss/theme/style.css", ".nav-content { gap: 24px }"),
        ("index.md", &index),
        ("about.md", &about),
        ("contact.md", &contact),
    ]);
    let html = site.html("about/index.html");
    let (_, after) = html.split_once(r#"<div class="nav-content" data-nav-themed>"#).expect(&html);
    let (masthead, _) = after.split_once("\n            </div>\n        </nav>").expect(&html);
    // This build is the editor's, which tags the nav entry with its source.
    let masthead = masthead.trim().replace(r#" data-source-file="about.md" data-source-fm="nav""#, "");
    assert_eq!(masthead, FLEX_MASTHEAD);
}
