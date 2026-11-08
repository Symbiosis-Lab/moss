//! A picture strip that loops sideways: `motion:marquee` on a `style:summary`
//! listing (and `children_motion:` on a folder's own listing) and
//! `:::gallery marquee`. The strip's pictures are drawn twice in one track, the
//! second set hidden from assistive technology and out of reach of the
//! keyboard, and the page ships the script that sets the loop's pace.

#[path = "support/site.rs"]
mod site;

use site::Site;

fn picture(site: &Site, rel: &str, shade: u8) {
    let path = site.root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbImage::from_fn(96, 64, |x, y| image::Rgb([shade, (x % 200) as u8, (y % 200) as u8])).save(path).unwrap();
}

fn page(title: &str, pictures: &[&str]) -> String {
    let body: Vec<String> = pictures.iter().map(|p| format!("![{title}|](pics/{p}.jpg)")).collect();
    format!("---\ntitle: {title}\n---\n\n{}\n", body.join("\n\n"))
}

fn work_site(home_body: &str) -> (Site, Vec<String>) {
    let site = Site::new(&[
        ("index.md", &format!("---\ntitle: Studio\nchildren: false\n---\n\n{home_body}\n")),
        ("work/index.md", "---\ntitle: Work\nchildren: false\n---\n"),
        ("work/five.md", &page("Five", &["p1", "p2", "p3", "p4", "p5"])),
    ]);
    for (i, p) in ["p1", "p2", "p3", "p4", "p5"].iter().enumerate() {
        picture(&site, &format!("work/pics/{p}.jpg"), 20 + i as u8 * 40);
    }
    let logged = site.rebuild();
    (site, logged)
}

fn count(doc: &scraper::Html, css: &str) -> usize {
    doc.select(&scraper::Selector::parse(css).unwrap()).count()
}

/// The two sets of the only marquee on the page: the real strip and its hidden copy.
fn sets(doc: &scraper::Html) -> (scraper::ElementRef<'_>, scraper::ElementRef<'_>) {
    let found: Vec<_> = doc.select(&scraper::Selector::parse("[data-motion='marquee'] > .moss-marquee-track > .moss-marquee-set").unwrap()).collect();
    assert_eq!(found.len(), 2, "one track of two sets");
    (found[0], found[1])
}

fn imgs(set: scraper::ElementRef<'_>) -> Vec<scraper::ElementRef<'_>> {
    set.select(&scraper::Selector::parse("img").unwrap()).collect()
}

/// The copy repeats the pictures with no name, behind `aria-hidden` and `inert`.
fn assert_hidden_copy(html: &str, pictures: usize) {
    let doc = scraper::Html::parse_document(html);
    let (real, copy) = sets(&doc);
    assert!(real.value().attr("aria-hidden").is_none() && real.value().attr("inert").is_none());
    assert_eq!(copy.value().attr("aria-hidden"), Some("true"));
    assert!(copy.value().attr("inert").is_some(), "the copy cannot take focus");
    let (a, b) = (imgs(real), imgs(copy));
    assert_eq!(a.len(), pictures);
    assert_eq!(b.len(), pictures, "the copy repeats every picture once");
    for (original, twin) in a.iter().zip(&b) {
        assert_eq!(original.value().attr("src"), twin.value().attr("src"), "the copy loads the same file");
        assert_eq!(twin.value().attr("alt"), Some(""), "the copy has no alt text");
    }
}

#[test]
fn a_summary_strip_with_motion_marquee_is_drawn_twice_in_one_track() {
    let (site, logged) = work_site("![[work/|style:summary,pictures:5,motion:marquee]]");
    let html = site.html("index.html");
    assert_hidden_copy(&html, 5);
    assert!(logged.is_empty(), "{logged:?}");
    let scripts = site.output("_moss/js");
    let shipped = std::fs::read_dir(scripts).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().starts_with("marquee"));
    assert!(shipped && html.contains("marquee"), "the page ships the pace script");
}

#[test]
fn a_strip_without_motion_has_no_copy_and_no_script() {
    let (site, _) = work_site("![[work/|style:summary,pictures:5]]");
    let doc = scraper::Html::parse_document(&site.html("index.html"));
    assert_eq!(count(&doc, ".moss-marquee-track"), 0);
    assert_eq!(count(&doc, ".moss-card-pictures img"), 5);
    let ships = std::fs::read_dir(site.output("_moss/js")).map(|d| d.flatten().any(|e| e.file_name().to_string_lossy().starts_with("marquee"))).unwrap_or(false);
    assert!(!ships, "no marquee, no script");
}

#[test]
fn an_unknown_motion_is_reported_and_draws_a_still_strip() {
    let (site, logged) = work_site("![[work/|style:summary,pictures:3,motion:bounce]]");
    assert!(logged.iter().any(|l| l.contains("motion:bounce")), "{logged:?}");
    let doc = scraper::Html::parse_document(&site.html("index.html"));
    assert_eq!(count(&doc, ".moss-marquee-track"), 0);
    assert_eq!(count(&doc, ".moss-card-pictures"), 1);
}

#[test]
fn a_folders_own_listing_takes_children_motion() {
    let (site, _) = work_site("Hello.");
    site.write("reel/index.md", "---\ntitle: Reel\nchildren_style: summary\nchildren_pictures: 3\nchildren_motion: marquee\n---\n");
    site.write("reel/five.md", &page("Five", &["p1", "p2", "p3"]));
    for p in ["p1", "p2", "p3"] {
        picture(&site, &format!("reel/pics/{p}.jpg"), 40);
    }
    site.rebuild();
    let html = site.html("reel/index.html");
    assert_hidden_copy(&html, 3);
    let ships = std::fs::read_dir(site.output("_moss/js")).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().starts_with("marquee"));
    assert!(ships, "the folder's own listing ships the pace script");
}

#[test]
fn a_gallery_marquee_is_drawn_twice_in_one_track() {
    let home = ":::gallery marquee\n![A](work/pics/p1.jpg)\n![B](work/pics/p2.jpg)\n![C](work/pics/p3.jpg)\n:::";
    let (site, logged) = work_site(home);
    let html = site.html("index.html");
    assert_hidden_copy(&html, 3);
    assert!(logged.is_empty(), "{logged:?}");
    let doc = scraper::Html::parse_document(&html);
    let (real, copy) = sets(&doc);
    // The real items keep their names; the copy's links are out of the viewer's reach.
    assert_eq!(imgs(real)[0].value().attr("alt"), Some("A"));
    assert!(copy.value().attr("class").unwrap().contains("no-viewer"));
    assert!(real.select(&scraper::Selector::parse("a.moss-gallery-link").unwrap()).all(|a| a.value().attr("data-title").is_some()));
    assert!(copy.select(&scraper::Selector::parse("a.moss-gallery-link").unwrap()).all(|a| a.value().attr("data-title").is_none()));
    let ships = std::fs::read_dir(site.output("_moss/js")).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().starts_with("marquee"));
    assert!(ships, "the gallery ships the pace script");
}

#[test]
fn a_plain_gallery_is_not_a_marquee() {
    let (site, _) = work_site(":::gallery rows\n![A](work/pics/p1.jpg)\n![B](work/pics/p2.jpg)\n:::");
    let doc = scraper::Html::parse_document(&site.html("index.html"));
    assert_eq!(count(&doc, ".moss-marquee-track"), 0);
}

#[test]
fn a_marquee_of_ten_pictures_draws_ten_and_ten() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Studio\nchildren: false\n---\n\n![[work/|style:summary,pictures:12,motion:marquee]]\n"),
        ("work/index.md", "---\ntitle: Work\nchildren: false\n---\n"),
        ("work/ten.md", &page("Ten", &["p0", "p1", "p2", "p3", "p4", "p5", "p6", "p7", "p8", "p9"])),
    ]);
    for i in 0..10 {
        picture(&site, &format!("work/pics/p{i}.jpg"), 10 + i as u8 * 20);
    }
    site.rebuild();
    assert_hidden_copy(&site.html("index.html"), 10);
}

#[test]
fn a_gallery_marquee_copy_keeps_the_captions() {
    let (site, _) = work_site(":::gallery marquee\n![A](work/pics/p1.jpg)\n*Dusk*\n\n![B](work/pics/p2.jpg)\n*Dawn*\n:::");
    let doc = scraper::Html::parse_document(&site.html("index.html"));
    let (real, copy) = sets(&doc);
    let captions = |set: scraper::ElementRef<'_>| set.select(&scraper::Selector::parse("p.moss-gallery-caption").unwrap()).map(|p| p.text().collect::<String>()).collect::<Vec<_>>();
    assert_eq!(captions(real), ["Dusk", "Dawn"]);
    assert_eq!(captions(copy), ["Dusk", "Dawn"], "a caption must not vanish where the loop wraps");
}
