//! `[site].credit` puts the site's own line before moss's mark in the colophon:
//! `<credit> · <mark>`. Without the key the colophon is exactly what it was.

#[path = "support/site.rs"]
mod site;

use site::{problems, Site};
use std::sync::{Mutex, MutexGuard};

/// The problem list is global to the test binary, so a test that reads it holds this from build to read.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

const PAGES: [(&str, &str); 2] = [
    ("index.md", "---\ntitle: Harbour\n---\n\nBody text.\n"),
    ("footer.md", "## Contact\n\nWrite to us.\n"),
];

const CREDIT: &str = "credit = \"Designed by [Example Studio](https://example.org)\"\n";

fn colophon_of(html: &str) -> &str {
    let from = html.find("<div class=\"moss-colophon\"").expect("a colophon");
    &html[from..html[from..].find("</div>").map(|i| from + i).expect("colophon closes")]
}

fn stylesheet(site: &Site) -> String {
    let dir = site.output("_moss");
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("style.") && n.ends_with(".css")))
        .map(|p| std::fs::read_to_string(p).unwrap())
        .collect()
}

#[test]
fn without_the_key_the_colophon_is_what_it_was() {
    let _turn = serial();
    let site = Site::new(&PAGES);
    let html = site.html("index.html");
    assert!(colophon_of(&html).contains(">\n        <a href=\"https://mosspub.com\""), "the link follows the opening tag as before: {}", colophon_of(&html));
    assert!(!html.contains("moss-colophon-credit"));
    assert!(!stylesheet(&site).contains("moss-colophon-credit"), "the credit rules ship only with a credit");
}

#[test]
fn the_credit_stands_before_the_mark_and_the_link_keeps_its_name() {
    let _turn = serial();
    let site = Site::with_config(CREDIT, &PAGES);
    let colophon = colophon_of(&site.html("index.html")).to_string();
    let credit_at = colophon.find("<span class=\"moss-colophon-credit\">Designed by <a").expect(&colophon);
    assert!(colophon.contains("href=\"https://example.org\">Example Studio</a></span>"), "{colophon}");
    assert!(colophon.contains("lang=\"en\"><span class=\"moss-colophon-dot\" aria-hidden=\"true\">\u{b7}</span>"), "the dot leads the mark's link: {colophon}");
    let mark_at = colophon.find("<a href=\"https://mosspub.com\"").unwrap();
    assert!(credit_at < mark_at, "{colophon}");
    assert!(colophon.contains("aria-label=\"Published with moss\""), "{colophon}");
    assert!(stylesheet(&site).contains("moss-colophon-credit"), "the credit rules ship with a credit");
    assert!(problems().is_empty(), "{:?}", problems());
}

#[test]
fn the_credit_is_escaped_like_other_text() {
    let _turn = serial();
    let site = Site::with_config("credit = \"Fish & Chips Studio\"\n", &PAGES);
    let colophon = colophon_of(&site.html("index.html")).to_string();
    assert!(colophon.contains("Fish &amp; Chips Studio"), "{colophon}");
}

#[test]
fn block_markdown_is_a_build_problem_naming_the_key_and_leaves_the_colophon_alone() {
    let _turn = serial();
    let site = Site::with_config("credit = \"# Big\"\n", &PAGES);
    let problems = problems();
    assert!(problems.iter().any(|p| p.contains("[site].credit")), "{problems:?}");
    let html = site.html("index.html");
    assert!(!html.contains("moss-colophon-credit") && !html.contains("<h1>Big"), "{}", colophon_of(&html));
    assert!(html.contains("class=\"moss-colophon\""));
}

#[test]
fn colophon_false_leaves_out_the_credit_too_and_says_so_without_failing_strict() {
    let _turn = serial();
    let site = Site::with_config(&format!("colophon = false\n{CREDIT}"), &PAGES);
    let html = site.html("index.html");
    assert!(!html.contains("moss-colophon") && !html.contains("Example Studio"), "{html}");
    assert!(site.logged.iter().any(|l| l.contains("[site].credit is not shown")), "{:?}", site.logged);
    assert!(problems().is_empty(), "a note, not a problem: {:?}", problems());
}

mod closing_picture {
    use super::*;

    const HERO: &str = ":::hero {image=day.png dark=night.png align=top}\n## The shelf is open.\n\n[Propose a collection](https://example.org/new)\n:::\n";

    /// A 160x70 picture: dark ground below row `ground_from`, with a bright band
    /// between row 50 and it, and sky above.
    fn picture(ground_from: u32) -> Vec<u8> {
        let mut img = image::RgbImage::new(160, 70);
        for (_, y, p) in img.enumerate_pixels_mut() {
            *p = image::Rgb(if y >= ground_from { [30, 40, 60] } else { [235, 240, 245] });
        }
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn foot_of(extra_config: &str) -> String {
        let site = Site::with_config(extra_config, &[("index.md", "---\ntitle: Harbour\n---\n\nBody text.\n"), ("footer.md", HERO)]);
        // The bottom 15% of the picture is dark ground and the rows above it are pale:
        // a one-line colophon row lies on the ground, a taller one reaches the pale rows.
        std::fs::write(site.root.join("day.png"), picture(60)).unwrap();
        std::fs::write(site.root.join("night.png"), picture(60)).unwrap();
        site.rebuild();
        let html = site.html("index.html");
        let from = html.find("<footer").unwrap();
        html[from..html[from..].find("</footer>").map(|i| from + i).unwrap()].to_string()
    }

    #[test]
    fn the_credit_sits_in_the_colophon_after_the_picture() {
        let _turn = serial();
        let footer = foot_of(CREDIT);
        assert!(footer.find("moss-hero-closing").unwrap() < footer.find("moss-colophon-credit").unwrap(), "{footer}");
        assert!(footer.find("moss-colophon-credit").unwrap() < footer.find("https://mosspub.com").unwrap(), "{footer}");
    }

    #[test]
    fn a_credit_widens_the_measured_row_on_a_phone_where_it_may_wrap() {
        let _turn = serial();
        let attr = |footer: &str| footer.split(" data-foot=\"").nth(1).and_then(|r| r.split('"').next()).map(str::to_string);
        let without = foot_of("");
        assert_eq!(attr(&without).as_deref(), Some("light"), "the colophon row lies on dark ground: {without}");
        let with = foot_of(CREDIT);
        assert_ne!(attr(&with).as_deref(), Some("light"), "the taller phone row reaches the pale rows, so no ink is promised: {with}");
    }
}

#[test]
fn raw_html_in_the_credit_is_shown_as_text_and_unsafe_links_are_dropped_with_a_problem() {
    let _turn = serial();
    let site = Site::with_config("credit = \"Hi <script>alert(1)</script> [x](javascript:alert(1)) [y](https://example.org)\"\n", &PAGES);
    let colophon = colophon_of(&site.html("index.html")).to_string();
    assert!(!colophon.contains("<script") && colophon.contains("&lt;script&gt;"), "{colophon}");
    assert!(!colophon.contains("javascript:"), "{colophon}");
    assert!(colophon.contains("href=\"https://example.org\">y</a>"), "{colophon}");
    assert!(problems().iter().any(|p| p.contains("[site].credit") && p.contains("javascript:")), "{:?}", problems());
}
