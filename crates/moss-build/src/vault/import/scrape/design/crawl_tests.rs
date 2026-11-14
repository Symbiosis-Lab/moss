//! The chrome pass as a user meets it: a crawl against a mock site, then the
//! files the crawl left behind.

use std::path::Path;
use crate::vault::import::scrape::fetch::test_server;

use super::super::run::scrape_to_folder;
use super::super::service::ScrapeConfig;

const SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 8 8\"><path d=\"M0 0h8v8z\"/></svg>";
const BODY: &str = "Page body, long enough to be extracted as content by the generic scorer.";

fn article(body: &str) -> String {
    format!("<article><p>{BODY} {body}</p></article>")
}

fn page(title: &str) -> String {
    format!("<html><head><title>{title}</title></head><body>{}</body></html>", article(title))
}

fn home_html(head: &str, header: &str, footer: &str) -> String {
    format!(
        "<html><head><title>Studio Example — Furniture and objects</title>{head}</head><body>\
         {header}{}{footer}</body></html>",
        article("Home. <a href=\"/news\">News</a>")
    )
}

const HEAD_FULL: &str = "<meta property=\"og:site_name\" content=\"Studio Example\">\
                         <link rel=\"icon\" type=\"image/svg+xml\" href=\"/favicon.svg\">";

const HEADER_FULL: &str = "<header><a href=\"/\"><img src=\"/logo.svg\" alt=\"Studio Example\"></a>\
    <nav><ul><li><a href=\"/about\">About</a></li><li><a href=\"/work\">Work</a></li>\
    <li><a href=\"/contact\">Contact</a></li>\
    <li><a href=\"https://other.example/shop\">Shop</a></li></ul></nav></header>";

const FOOTER_FULL: &str = "<footer><p>12 Example Street, Testville</p>\
    <a href=\"https://instagram.com/studioexample\" aria-label=\"Instagram\"><svg></svg></a>\
    <a href=\"https://www.facebook.com/studioexample\" title=\"Facebook\"><i class=\"icon\"></i></a>\
    </footer>";

struct Site {
    server: mockito::ServerGuard,
    _mocks: Vec<mockito::Mock>,
}

async fn site(pages: &[(&str, String)], files: &[(&str, &str)]) -> Site {
    let mut server = test_server().await;
    let mut mocks = Vec::new();
    for (path, body) in pages {
        mocks.push(
            server
                .mock("GET", *path)
                .with_status(200)
                .with_header("content-type", "text/html")
                .with_body(body)
                .create_async()
                .await,
        );
    }
    for (path, body) in files {
        mocks.push(
            server
                .mock("GET", *path)
                .with_status(200)
                .with_header("content-type", "image/svg+xml")
                .with_body(body)
                .create_async()
                .await,
        );
    }
    Site { server, _mocks: mocks }
}

async fn crawl(site: &Site, out: &Path) -> super::super::run::ScrapeResult {
    let mut config = ScrapeConfig::new(format!("{}/", site.server.url()), out);
    config.recursive = true;
    scrape_to_folder(config, |_| {}).await.expect("the crawl itself succeeds")
}

fn read(out: &Path, rel: &str) -> String {
    std::fs::read_to_string(out.join(rel)).unwrap()
}

fn full_pages() -> Vec<(&'static str, String)> {
    vec![
        ("/", home_html(HEAD_FULL, HEADER_FULL, FOOTER_FULL)),
        ("/about", page("About")),
        ("/work", page("Work")),
        ("/contact", page("Contact")),
        ("/news", page("News")),
    ]
}

#[tokio::test]
async fn a_crawl_writes_the_sites_chrome_beside_its_pages() {
    let site = site(&full_pages(), &[("/logo.svg", SVG), ("/favicon.svg", SVG)]).await;
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path();

    let res = crawl(&site, out).await;

    assert_eq!(res.total_pages, 5);
    assert_eq!(
        res.chrome.line().unwrap(),
        "site chrome: nav 3 item(s), logo, footer, favicon"
    );

    let home = read(out, "index.md");
    assert!(home.contains("title: \"Studio Example\"\n"), "{home}");
    assert!(home.contains("description: \"Furniture and objects\"\n"), "{home}");
    assert!(!home.contains("nav:"), "{home}");
    let logo = home
        .lines()
        .find_map(|l| l.strip_prefix("logo: \"./"))
        .and_then(|l| l.strip_suffix('"'))
        .unwrap_or_else(|| panic!("no logo line:\n{home}"));
    assert!(logo.starts_with("assets/imported/") && logo.ends_with(".svg"), "{logo}");
    assert_eq!(read(out, logo), SVG);

    for (rel, weight) in [("about.md", 1), ("work.md", 2), ("contact.md", 3)] {
        let note = read(out, rel);
        assert!(note.contains(&format!("\nnav: true\nweight: {weight}\n---\n")), "{rel}:\n{note}");
    }

    let footer = read(out, "footer.md");
    assert!(footer.contains("12 Example Street, Testville"), "{footer}");
    assert!(footer.contains("- [Instagram](https://instagram.com/studioexample)"), "{footer}");
    assert!(footer.contains("- [Facebook](https://www.facebook.com/studioexample)"), "{footer}");
    assert!(!footer.starts_with("---"), "a slot page needs no frontmatter: {footer}");

    assert_eq!(read(out, "assets/favicon.svg"), SVG);
}

#[tokio::test]
async fn a_page_the_nav_does_not_link_keeps_exactly_the_frontmatter_the_crawl_gave_it() {
    let site = site(&full_pages(), &[("/logo.svg", SVG), ("/favicon.svg", SVG)]).await;
    let tmp = tempfile::tempdir().unwrap();
    crawl(&site, tmp.path()).await;

    let news = read(tmp.path(), "news.md");
    let keys: Vec<String> = moss_core::frontmatter::frontmatter_map(&news).into_keys().collect();
    let mut keys = keys;
    keys.sort();
    assert_eq!(keys, ["origin", "title"], "{news}");
}

#[tokio::test]
async fn what_the_folder_already_has_is_left_as_it_is() {
    let site = site(&full_pages(), &[("/logo.svg", SVG), ("/favicon.svg", SVG)]).await;
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path();
    std::fs::write(out.join("footer.md"), "mine\n").unwrap();
    std::fs::create_dir_all(out.join("assets")).unwrap();
    std::fs::write(out.join("assets/favicon.png"), b"mine").unwrap();

    let res = crawl(&site, out).await;

    assert_eq!(read(out, "footer.md"), "mine\n");
    assert_eq!(std::fs::read(out.join("assets/favicon.png")).unwrap(), b"mine");
    assert!(!out.join("assets/favicon.svg").exists());
    assert_eq!(
        res.chrome.line().unwrap(),
        "site chrome: nav 3 item(s), logo, footer left unchanged, favicon left unchanged"
    );
}

#[tokio::test]
async fn a_platform_default_icon_writes_no_favicon() {
    let head = "<link rel=\"icon\" href=\"/pfavico.ico\">";
    let site = site(&[("/", home_html(head, "", FOOTER_FULL))], &[]).await;
    let tmp = tempfile::tempdir().unwrap();

    crawl(&site, tmp.path()).await;

    assert!(tmp.path().join("footer.md").exists(), "the chrome pass ran");

    let favicons: Vec<_> = std::fs::read_dir(tmp.path().join("assets"))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("favicon"))
        .collect();
    assert!(favicons.is_empty(), "{favicons:?}");
}

#[tokio::test]
async fn a_page_with_no_header_nav_and_no_footer_writes_no_chrome_and_says_so() {
    let site = site(&[("/", home_html("", "", ""))], &[]).await;
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path();

    let res = crawl(&site, out).await;

    assert_eq!(res.chrome.line().unwrap(), "site chrome: none found");
    assert!(!out.join("footer.md").exists());
    assert_eq!(
        read(out, "index.md").lines().filter(|l| l.starts_with("nav") || l.starts_with("logo")).count(),
        0
    );
}

#[tokio::test]
async fn a_single_file_import_reads_no_chrome() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path();
    let page = home_html(HEAD_FULL, HEADER_FULL, FOOTER_FULL);
    let file = out.join("saved.html");
    std::fs::write(&file, page).unwrap();

    let res = super::super::import_local::import_local_file(&file, out).await.unwrap();

    assert_eq!(res.chrome.line(), None);
    assert!(!out.join("footer.md").exists());
    assert!(!out.join("assets/favicon.svg").exists());
}

/// moss puts every root-level page of a site with content folders in the nav
/// bar unless told otherwise, so a 4-item menu over seven root pages must come
/// out as three `nav: true` pages and three `nav: false` ones.
#[tokio::test]
async fn pages_the_menu_does_not_show_are_kept_out_of_the_nav_bar() {
    let header = "<header><nav><a href=\"/\">Home</a><a href=\"/a\">A</a>\
                  <a href=\"/b\">B</a><a href=\"/c\">C</a></nav></header>";
    let links = "<a href=\"/d\">D</a> <a href=\"/e\">E</a> <a href=\"/f\">F</a> \
                 <a href=\"/blog/post\">Post</a>";
    let mut pages = vec![(
        "/",
        format!(
            "<html><head><title>Studio Example</title></head><body>{header}{}</body></html>",
            article(links)
        ),
    )];
    for p in ["/a", "/b", "/c", "/d", "/e", "/f", "/blog/post"] {
        pages.push((p, page(&p[1..].replace('/', " "))));
    }
    let site = site(&pages, &[]).await;
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path();

    crawl(&site, out).await;

    for (rel, weight) in [("a.md", 1), ("b.md", 2), ("c.md", 3)] {
        assert!(read(out, rel).contains(&format!("\nnav: true\nweight: {weight}\n")), "{rel}");
    }
    for rel in ["d.md", "e.md", "f.md"] {
        assert!(read(out, rel).contains("\nnav: false\n---\n"), "{rel}");
    }
    for rel in ["index.md", "blog/post.md"] {
        assert!(!read(out, rel).contains("nav:"), "{rel} is not a root-level candidate");
    }
}

const DESIGN_HEAD: &str = "<link rel=\"stylesheet\" href=\"/fonts.css?family=Field+Sans\">\
    <link rel=\"stylesheet\" href=\"/screen.css\">\
    <style>:root { --site-accent-color: #b0302a }</style>";

const DESIGN_HOME: &str = "<html><head><title>Studio Example</title>{head}</head>\
    <body><main><h1>Studio Example</h1><article><p>Body text, long enough to be extracted as content by the generic scorer.</p>\
    <h2><a href=\"/notes\">Notes</a></h2></article></main></body></html>";

/// A post whose text holds a link, which the theme draws in its own blue.
const DESIGN_POST: &str = "<html><head><title>Notes</title>{head}</head>\
    <body><main><article class=\"prose\"><h1>Notes</h1><p>Body text, long enough to be extracted as content by the generic scorer, \
    with <a href=\"/\">a link home</a>.</p></article></main></body></html>";

/// A Google-Fonts-shaped stylesheet: two families, only one of them used.
const FONTS_CSS: &str = "@font-face { font-family: 'Field Sans'; font-style: normal; font-weight: 400; \
    src: url(/static/field-sans-latin.woff2) format('woff2'); }\n\
    @font-face { font-family: 'Field Sans'; font-style: italic; src: url(/static/field-sans-italic.woff2) format('woff2'); }\n\
    @font-face { font-family: 'Field Sans'; font-weight: 700; src: url(data:font/woff2;base64,d09GMgABAAA=) format('woff2'); }\n\
    @font-face { font-family: 'Unused Serif'; src: url(/static/unused.woff2) format('woff2'); }";

const SCREEN_CSS: &str = "body { font-family: 'Field Sans', sans-serif; color: #202020; background: #fdfcf8 } .prose a { color: #2050c0 }";

async fn design_site() -> Site {
    let pages = [("/", DESIGN_HOME.replace("{head}", DESIGN_HEAD)), ("/notes", DESIGN_POST.replace("{head}", DESIGN_HEAD))];
    let mut site = site(&pages, &[]).await;
    for (path, body, kind) in [
        ("/fonts.css?family=Field+Sans", FONTS_CSS.as_bytes(), "text/css"),
        ("/screen.css", SCREEN_CSS.as_bytes(), "text/css"),
        ("/static/field-sans-latin.woff2", b"wOF2 field sans".as_slice(), "font/woff2"),
        ("/static/unused.woff2", b"wOF2 unused".as_slice(), "font/woff2"),
        ("/static/field-sans-italic.woff2", b"<html>not found</html>".as_slice(), "text/html"),
    ] {
        site._mocks.push(site.server.mock("GET", path).with_status(200).with_header("content-type", kind).with_body(body).create_async().await);
    }
    site
}

#[tokio::test]
async fn a_crawl_writes_the_sites_look_as_moss_tokens_and_copies_its_font() {
    let site = design_site().await;
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path();

    let res = crawl(&site, out).await;

    let css = read(out, ".moss/theme/style.css");
    assert!(css.contains("@font-face {\n  font-family: \"Field Sans\";\n  src: url(fonts/field-sans-latin.woff2) format(\"woff2\");"), "{css}");
    assert!(css.contains("--moss-font-body: 'Field Sans', -apple-system,"), "{css}");
    assert!(css.contains("--moss-color-text: #202020;") && css.contains("--moss-color-accent: #b0302a;"), "{css}");
    assert!(css.contains("--moss-link-color: #2050c0;"), "the link colour is read from the post: {css}");
    assert!(!css.contains("Unused Serif"), "a family no token names is not copied: {css}");
    assert_eq!(std::fs::read(out.join(".moss/theme/fonts/field-sans-latin.woff2")).unwrap(), b"wOF2 field sans");
    assert!(!out.join(".moss/theme/fonts/unused.woff2").exists());
    assert!(!css.contains("field-sans-italic") && !out.join(".moss/theme/fonts/field-sans-italic.woff2").exists(), "an error page is not a font: {css}");
    assert!(read(out, ".moss/config.toml").contains("color_scheme = \"light\""), "no dark scheme in the source");
    assert!(!css.contains("data:"), "{css}");
    assert!(res.chrome.residuals.iter().any(|r| r == "font Field Sans is served as a data: address, which is not copied"), "{:?}", res.chrome.residuals);
    let line = res.chrome.line().unwrap();
    assert!(line.ends_with(", font Field Sans") && line.contains("design "), "{line}");
}

#[tokio::test]
async fn a_theme_the_author_already_shaped_stays() {
    let site = design_site().await;
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path();
    std::fs::create_dir_all(out.join(".moss/theme")).unwrap();
    std::fs::write(out.join(".moss/theme/style.css"), ":root { --moss-color-accent: #123456; }\n").unwrap();

    let res = crawl(&site, out).await;

    assert_eq!(read(out, ".moss/theme/style.css"), ":root { --moss-color-accent: #123456; }\n");
    assert!(!out.join(".moss/theme/fonts").exists());
    assert!(res.chrome.line().unwrap().ends_with("design left unchanged"), "{:?}", res.chrome.line());
}

#[tokio::test]
async fn a_stylesheet_that_cannot_be_read_or_is_imported_is_reported_and_its_host_slowed() {
    let home = "<html><head><title>Studio Example</title>\
        <link rel=\"stylesheet\" href=\"/busy.css\">\
        <style>@import url(\"/print.css\"); body { color: #202020 }</style></head>\
        <body><main><article><p>Body text, long enough to be extracted as content by the generic scorer.</p></article></main></body></html>";
    let mut site = site(&[("/", home.to_string())], &[]).await;
    site._mocks.push(site.server.mock("GET", "/busy.css").with_status(429).create_async().await);
    let tmp = tempfile::tempdir().unwrap();

    let res = crawl(&site, tmp.path()).await;

    let busy = format!("{}/busy.css", site.server.url());
    assert!(res.chrome.residuals.iter().any(|r| r.starts_with(&format!("stylesheet {busy} could not be read (429 "))), "{:?}", res.chrome.residuals);
    assert!(res.chrome.residuals.iter().any(|r| r.ends_with("imports /print.css, which is not read")), "{:?}", res.chrome.residuals);
    assert!(!res.rate_limited_hosts.is_empty(), "a 429 on a stylesheet slows its host as on a page");
}

#[tokio::test]
async fn a_colour_scheme_the_folder_already_sets_stays() {
    let site = design_site().await;
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path();
    std::fs::create_dir_all(out.join(".moss")).unwrap();
    std::fs::write(out.join(".moss/config.toml"), "[site]\ncolor_scheme = \"auto\"\n").unwrap();

    crawl(&site, out).await;

    assert!(read(out, ".moss/theme/style.css").contains("--moss-color-text"), "the design is written");
    assert_eq!(read(out, ".moss/config.toml"), "[site]\ncolor_scheme = \"auto\"\n");
}

#[tokio::test]
async fn a_stylesheet_on_a_local_or_private_address_is_not_requested() {
    let home = "<html><head><title>Studio Example</title>\
        <link rel=\"stylesheet\" href=\"http://127.0.0.1:9/theme.css\">\
        <link rel=\"stylesheet\" href=\"http://169.254.169.254/latest.css\">\
        <link rel=\"stylesheet\" href=\"http://[fe80::1]/x.css\">\
        <style>body { color: #202020 }</style></head>\
        <body><main><article><p>Body text, long enough to be extracted as content by the generic scorer.</p></article></main></body></html>";
    let site = site(&[("/", home.to_string())], &[]).await;
    let tmp = tempfile::tempdir().unwrap();

    let res = crawl(&site, tmp.path()).await;

    for sheet in ["http://127.0.0.1:9/theme.css", "http://169.254.169.254/latest.css", "http://[fe80::1]/x.css"] {
        assert!(res.chrome.residuals.contains(&format!("stylesheet {sheet}: address not allowed")), "{sheet}: {:?}", res.chrome.residuals);
    }
}

#[tokio::test]
async fn a_token_named_only_in_a_comment_is_no_override_of_the_author_s() {
    let site = design_site().await;
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path();
    std::fs::create_dir_all(out.join(".moss/theme")).unwrap();
    let own = "/* --moss-color-accent: #123456; */\nbody { margin: 0; }\n";
    std::fs::write(out.join(".moss/theme/style.css"), own).unwrap();

    crawl(&site, out).await;

    let css = read(out, ".moss/theme/style.css");
    assert!(css.starts_with(own.trim_end()) && css.contains("--moss-color-text: #202020;"), "{css}");
}
