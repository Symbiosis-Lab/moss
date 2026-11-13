//! A picture directly above a page's only `# Heading` builds the page the
//! `:::hero` fence around that picture builds, byte for byte apart from
//! source positions, as a home page and as an ordinary page.

#[path = "support/site.rs"]
mod site;

use site::{hero_of, Site};

const PICTURE: &str = "![The hall](hall.jpg)";

fn built(home: &str, walk: &str) -> Site {
    let site = Site::new(&[("index.md", home), ("walk.md", walk)]);
    image::RgbImage::from_fn(96, 64, |x, y| image::Rgb([90, x as u8, y as u8])).save(site.root.join("hall.jpg")).unwrap();
    site.rebuild();
    site
}

/// The page with the site's random folder name taken out, in the forms a build writes it.
fn html(site: &Site, rel: &str) -> String {
    let id = site.root.file_name().unwrap().to_string_lossy().replace("moss_inline_site_", "");
    site.html(rel).replace(&id, "ID").replace(&id.replace('-', " "), "ID")
}

/// Without the source positions a preview build writes: the fence adds two lines above everything after it.
fn without_source_positions(html: &str) -> String {
    regex::Regex::new(r#" data-source-(range|line)="[^"]*""#).unwrap().replace_all(html, "").into_owned()
}

fn page(opening: &str, title: &str, rest: &str) -> String {
    format!("{opening}\n\n# {title}\n\n{rest}")
}

#[test]
fn the_plain_form_builds_the_fenced_page() {
    let fence = format!(":::hero\n{PICTURE}\n:::");
    let rest = "Text.\n\n## Part\n\nMore.\n";
    let plain = built(&page(PICTURE, "Example Page", rest), &page(PICTURE, "Walk", rest));
    let fenced = built(&page(&fence, "Example Page", rest), &page(&fence, "Walk", rest));
    for rel in ["index.html", "walk/index.html"] {
        let html = html(&plain, rel);
        assert!(hero_of(&html).contains("hall"), "{rel} opens with the hero: {html}");
        assert_eq!(html.matches("<h1").count(), 1, "{rel}: {html}");
        assert!(hero_of(&html).contains(r#"data-source-range="1-1""#), "{rel}: a click on the hero reaches the picture's line");
        assert_eq!(without_source_positions(&html), without_source_positions(&self::html(&fenced, rel)), "{rel}");
    }
}

#[test]
fn near_misses_build_no_hero() {
    let sections = "Text.\n\n# 2.\n\nMore.\n";
    let captioned = format!("{PICTURE}\n*The hall in 1884*");
    let site = built(&page(PICTURE, "1.", sections), &page(&captioned, "Walk", "Text.\n"));
    for rel in ["index.html", "walk/index.html"] {
        let html = site.html(rel);
        assert_eq!(hero_of(&html), "", "{rel}");
        assert!(html.contains("hall"), "{rel} keeps its picture");
    }
}

/// A footer is not a page body: its opening picture stays in the footer of every page.
#[test]
fn a_footer_keeps_its_opening_picture() {
    let site = Site::new(&[("index.md", "# Home\n\nText.\n"), ("footer.md", &page(PICTURE, "Contact", "Text.\n"))]);
    image::RgbImage::from_fn(96, 64, |x, y| image::Rgb([90, x as u8, y as u8])).save(site.root.join("hall.jpg")).unwrap();
    site.rebuild();
    let html = site.html("index.html");
    assert_eq!(hero_of(&html), "", "{html}");
    let footer = html.split("<footer").nth(1).unwrap_or_default();
    assert!(footer.contains("hall"), "the footer keeps its picture: {html}");
}
