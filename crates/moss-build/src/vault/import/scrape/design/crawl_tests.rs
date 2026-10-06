//! The chrome pass as a user meets it: a crawl against a mock site, then the
//! files the crawl left behind.

use std::path::Path;

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
    let mut server = mockito::Server::new_async().await;
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
