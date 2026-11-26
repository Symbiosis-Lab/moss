//! A wide picture directly above a page's only `# Heading` builds the page the
//! `:::hero` fence around that picture builds, byte for byte apart from source
//! positions. A narrow picture or one taller than wide stays in the column
//! with its caption.

use crate::site;

use site::{hero_of, Site};

const PICTURE: &str = "![The hall](hall.jpg)";

/// A wide photograph, a picture wide in shape but too narrow to fill a
/// window, and one wide enough but taller than wide.
const WIDE: (u32, u32) = (1600, 900);
const LOGO: (u32, u32) = (400, 200);
const PORTRAIT: (u32, u32) = (1400, 2000);

fn save_picture(site: &Site, rel: &str, (w, h): (u32, u32)) {
    let path = site.root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbImage::from_fn(w, h, |x, y| image::Rgb([90, x as u8, y as u8])).save(path).unwrap();
}

fn built(files: &[(&str, &str)], picture: &str, size: (u32, u32)) -> Site {
    let site = Site::new(files);
    save_picture(&site, picture, size);
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

const REST: &str = "Text.\n\n## Part\n\nMore.\n";

#[test]
fn a_wide_picture_builds_the_fenced_page() {
    let fence = format!(":::hero\n{PICTURE}\n:::");
    let plain = built(&[("index.md", &page(PICTURE, "Example Page", REST)), ("walk.md", &page(PICTURE, "Walk", REST))], "hall.jpg", WIDE);
    let fenced = built(&[("index.md", &page(&fence, "Example Page", REST)), ("walk.md", &page(&fence, "Walk", REST))], "hall.jpg", WIDE);
    for rel in ["index.html", "walk/index.html"] {
        let html = html(&plain, rel);
        let hero = hero_of(&html);
        assert!(hero.contains("hall"), "{rel} opens with the banner: {html}");
        assert!(hero.contains(r#"alt="The hall""#) && !hero.contains("<figcaption"), "{rel}: the description is for screen readers only: {hero}");
        assert_eq!(html.matches("<h1").count(), 1, "{rel}: {html}");
        assert!(hero.contains(r#"data-source-range="1-1""#), "{rel}: a click on the banner reaches the picture's line");
        assert_eq!(without_source_positions(&html), without_source_positions(&self::html(&fenced, rel)), "{rel}");
    }
}

/// A logo or a portrait is drawn in the column under the title rules a page
/// with a picture above its heading always had, its description its caption.
#[test]
fn a_narrow_or_tall_picture_stays_in_the_column() {
    for (name, size) in [("logo", LOGO), ("portrait", PORTRAIT)] {
        let site = built(&[("index.md", &page(PICTURE, "Example Page", REST)), ("walk.md", &page(PICTURE, "Walk", REST))], "hall.jpg", size);
        for rel in ["index.html", "walk/index.html"] {
            let html = site.html(rel);
            assert_eq!(hero_of(&html), "", "{name} {rel}: {html}");
            let article = html.split("<article").nth(1).unwrap_or_default();
            let figure = article.find("<figure").unwrap_or_else(|| panic!("{name} {rel} keeps its figure: {html}"));
            assert!(article[figure..].contains("<figcaption") && article[figure..].contains("The hall"), "{name} {rel} keeps its caption: {html}");
        }
    }
}

/// The size is looked up under the address the picture resolves to, so a
/// picture named by its bare filename from a page in another folder is found.
#[test]
fn a_picture_in_another_folder_is_measured_where_it_resolves() {
    for opening in [PICTURE, "![[hall.jpg]]"] {
        let site = built(&[("index.md", "# Home\n\nText.\n"), ("notes/walk.md", &page(opening, "Walk", REST))], "photos/hall.jpg", WIDE);
        let html = site.html("notes/walk/index.html");
        assert!(hero_of(&html).contains("hall"), "{opening}: {html}");
        assert_eq!(html.matches("<h1").count(), 1, "{opening}: {html}");
    }
}

#[test]
fn near_misses_build_no_banner() {
    let sections = "Text.\n\n# 2.\n\nMore.\n";
    let captioned = format!("{PICTURE}\n*The hall in 1884*");
    let site = built(&[("index.md", &page(PICTURE, "1.", sections)), ("walk.md", &page(&captioned, "Walk", "Text.\n"))], "hall.jpg", WIDE);
    for rel in ["index.html", "walk/index.html"] {
        let html = site.html(rel);
        assert_eq!(hero_of(&html), "", "{rel}");
        assert!(html.contains("hall"), "{rel} keeps its picture");
    }
}

/// A footer is not a page body: its opening picture stays in the footer of every page.
#[test]
fn a_footer_keeps_its_opening_picture() {
    let site = built(&[("index.md", "# Home\n\nText.\n"), ("footer.md", &page(PICTURE, "Contact", "Text.\n"))], "hall.jpg", WIDE);
    let html = site.html("index.html");
    assert_eq!(hero_of(&html), "", "{html}");
    let footer = html.split("<footer").nth(1).unwrap_or_default();
    assert!(footer.contains("hall"), "the footer keeps its picture: {html}");
}

/// The banner follows the picture's size on the rebuild a changed picture starts,
/// not the size the last build measured.
#[test]
fn the_banner_follows_a_picture_replaced_under_the_same_name() {
    let site = built(&[("index.md", &page(PICTURE, "Example Page", REST))], "hall.jpg", WIDE);
    let picture = site.root.join("hall.jpg");
    let rebuild_after = |size| {
        save_picture(&site, "hall.jpg", size);
        site::build_sync(&site.root.to_string_lossy(), moss_build::build::BuildTrigger::ContentOnly(vec![picture.clone()]))
            .unwrap_or_else(|e| panic!("build failed: {e}"));
        hero_of(&site.html("index.html"))
    };
    assert!(hero_of(&site.html("index.html")).contains("hall"), "the wide picture opens as the banner");
    assert_eq!(rebuild_after(LOGO), "", "a logo put in its place goes back to the column");
    assert!(rebuild_after(WIDE).contains("hall"), "the wide picture put back is the banner again");
}
