//! `pictures:N` on a `style:summary` listing draws each row's own pictures as
//! one strip, in reading order, in place of the single cover.

#[path = "support/site.rs"]
mod site;

use site::Site;

fn picture(site: &Site, rel: &str, shade: u8) {
    let path = site.root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbImage::from_fn(96, 64, |x, y| image::Rgb([shade, (x % 200) as u8, (y % 200) as u8])).save(path).unwrap();
}

fn page(title: &str, front: &str, pictures: &[&str]) -> String {
    let body: Vec<String> = pictures.iter().map(|p| format!("![{title}|](pics/{p}.jpg)")).collect();
    format!("---\ntitle: {title}\n{front}---\n\n{}\n", body.join("\n\n"))
}

/// The site after its pictures exist, and what that second build logged.
fn work_site(home_body: &str) -> (Site, Vec<String>) {
    let site = Site::new(&[
        ("index.md", &format!("---\ntitle: Studio\nchildren: false\n---\n\n{home_body}\n")),
        ("work/index.md", "---\ntitle: Work\nchildren: false\n---\n"),
        ("work/five.md", &page("Five", "cover: pics/p2.jpg\n", &["p1", "p2", "p3", "p4", "p5"])),
        ("work/one.md", &page("One", "", &["p1"])),
        ("work/none.md", "---\ntitle: None\n---\n\nWords only.\n"),
    ]);
    for (i, p) in ["p1", "p2", "p3", "p4", "p5"].iter().enumerate() {
        picture(&site, &format!("work/pics/{p}.jpg"), 20 + i as u8 * 40);
    }
    let logged = site.rebuild();
    (site, logged)
}

/// Each card's pictures by the page's last URL segment: `None` when the row draws no strip.
fn strips(html: &str) -> Vec<(String, Option<Vec<String>>)> {
    let doc = scraper::Html::parse_document(html);
    let card = scraper::Selector::parse("a.moss-card").unwrap();
    let strip = scraper::Selector::parse(".moss-card-pictures").unwrap();
    let img = scraper::Selector::parse("img").unwrap();
    doc.select(&card)
        .map(|c| {
            let slug = c.value().attr("href").unwrap_or("").trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string();
            let srcs = c.select(&strip).next().map(|s| s.select(&img).map(|i| i.value().attr("src").unwrap_or("").rsplit('/').next().unwrap().to_string()).collect());
            (slug, srcs)
        })
        .collect()
}

fn strip_of(rows: &[(String, Option<Vec<String>>)], slug: &str) -> Option<Vec<String>> {
    rows.iter().find(|(s, _)| s == slug).unwrap_or_else(|| panic!("no row for {slug}: {rows:?}")).1.clone()
}

#[test]
fn a_summary_row_draws_the_pages_pictures_cover_first_without_repeats() {
    let (site, logged) = work_site("![[work/|style:summary,pictures:3]]");
    let rows = strips(&site.html("index.html"));
    assert_eq!(strip_of(&rows, "five"), Some(vec!["p2.jpg".into(), "p1.jpg".into(), "p3.jpg".into()]));
    assert!(logged.is_empty(), "{logged:?}");
}

#[test]
fn a_page_with_one_or_no_picture_draws_no_strip() {
    let (site, _) = work_site("![[work/|style:summary,pictures:3]]");
    let html = site.html("index.html");
    let rows = strips(&html);
    assert_eq!(strip_of(&rows, "one"), None);
    assert_eq!(strip_of(&rows, "none"), None);
    // The page with one picture keeps its single cover.
    let one = html.split("href=\"/work/one/\"").nth(1).unwrap().split("</a>").next().unwrap();
    assert!(one.contains("moss-card-cover"), "{one}");
}

#[test]
fn a_listing_without_the_option_keeps_one_cover() {
    let (site, _) = work_site("![[work/|style:summary]]");
    let html = site.html("index.html");
    assert!(!html.contains("moss-card-pictures"));
    assert!(html.contains("moss-card-cover"));
}

#[test]
fn a_folders_own_listing_takes_children_pictures() {
    let (site, _) = work_site("Hello.");
    site.write("reel/index.md", "---\ntitle: Reel\nchildren_style: summary\nchildren_pictures: 2\n---\n");
    site.write("reel/five.md", &page("Five", "", &["p1", "p2", "p3"]));
    picture(&site, "reel/pics/p1.jpg", 10);
    picture(&site, "reel/pics/p2.jpg", 60);
    picture(&site, "reel/pics/p3.jpg", 110);
    site.rebuild();
    let rows = strips(&site.html("reel/index.html"));
    assert_eq!(strip_of(&rows, "five"), Some(vec!["p1.jpg".into(), "p2.jpg".into()]));
}

#[test]
fn a_picture_count_out_of_range_or_not_a_number_warns() {
    for bad in ["0", "1", "99", "many"] {
        let (site, logged) = work_site(&format!("![[work/|style:summary,pictures:{bad}]]"));
        assert!(logged.iter().any(|l| l.contains(&format!("pictures:{bad}"))), "pictures:{bad} warns: {logged:?}");
        assert!(!site.html("index.html").contains("moss-card-pictures"), "pictures:{bad} draws no strip");
    }
}

#[test]
fn a_folders_picture_count_out_of_range_warns_and_is_ignored() {
    let (site, _) = work_site("Hello.");
    site.write("reel/index.md", "---\ntitle: Reel\nchildren_style: summary\nchildren_pictures: 99\n---\n");
    site.write("reel/five.md", &page("Five", "", &["p1", "p2", "p3"]));
    for p in ["p1", "p2", "p3"] {
        picture(&site, &format!("reel/pics/{p}.jpg"), 40);
    }
    let logged = site.rebuild();
    assert!(logged.iter().any(|l| l.contains("children_pictures") && l.contains("99")), "{logged:?}");
    assert!(!site.html("reel/index.html").contains("moss-card-pictures"));
}
