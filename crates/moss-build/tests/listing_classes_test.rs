//! The classes a folder listing emits only when it has entries are the ones
//! `is_listing_class` names, so `moss doctor --surface` can tell a rule for an
//! empty listing from a dead one.

#[path = "support/site.rs"]
mod site;

use std::collections::BTreeSet;

use moss_build::build::folder_embed::listing_markup::is_listing_name;
use site::Site;

fn classes_of_lab(style: &str, event_start: &str) -> BTreeSet<String> {
    let event = format!("---\ntitle: Spring fair\nstart: {event_start}\n---\n\nBody.\n");
    let lab = format!("---\ntitle: Lab\n---\n\n![[/events/|when:upcoming,limit:4,style:{style}]]\n");
    let site = Site::new(&[
        ("index.md", "---\ntitle: Example Hall\n---\n\nWelcome.\n"),
        ("events/index.md", "---\ntitle: Events\nchildren: false\n---\n"),
        ("events/fair.md", &event),
        ("lab.md", &lab),
    ]);
    let page = scraper::Html::parse_document(&site.html("lab/index.html"));
    let mut found = BTreeSet::new();
    for el in page.select(&scraper::Selector::parse("*").unwrap()) {
        let el = el.value();
        found.extend(el.classes().map(|c| format!(".{c}")));
        found.extend(el.id().map(|i| format!("#{i}")));
        found.extend(el.attrs().map(|(k, _)| format!("[{k}")));
    }
    found
}

#[test]
fn every_class_an_upcoming_listing_adds_is_a_listing_class() {
    for style in ["summary", "list", "grid", "minimal"] {
        let with_entries = classes_of_lab(style, "2099-03-01 19:30");
        let empty = classes_of_lab(style, "2001-03-01 19:30");
        let added: Vec<&String> = with_entries.difference(&empty).collect();
        assert!(added.iter().any(|c| c.as_str() == ".moss-cards-container"), "{style}: {added:?}");
        let missed: Vec<&&String> = added.iter().filter(|n| !is_listing_name(n.chars().next().unwrap(), &n[1..])).collect();
        assert!(missed.is_empty(), "{style}: names only a non-empty listing emits, not named by is_listing_name: {missed:?}");
    }
}

/// A `partial: true` file with `listed: true` is an entry in its folder's
/// listings with no page of its own: the listing draws its title and card
/// fields without a link (or links out through an `https://` `external_url:`),
/// and nothing else publishes it.
mod listed_partial {
    use super::site::{hrefs_in, Site};

    const CONFIG: &str = "\n[fields.sector]\ntype = \"text\"\ncard = true\nin = [\"work\"]\n";

    fn site(extra: &[(&'static str, &'static str)]) -> Site {
        let mut files: Vec<(&str, &str)> = vec![
            ("index.md", "---\ntitle: Home\nhome: true\nchildren: false\n---\n\n![[work/|style:summary,sort:weight]]\n"),
            ("work/index.md", "---\ntitle: Work\n---\n"),
            ("work/harbour.md", "---\ntitle: Harbour\nweight: 1\nsector: shipping\n---\n\nThe case study.\n"),
            ("work/lantern.md", "---\ntitle: Lantern\nweight: 2\nsector: lighting\npartial: true\nlisted: true\n---\n"),
            ("work/snippet.md", "---\ntitle: Snippet\npartial: true\n---\n\nA block other pages embed.\n"),
        ];
        files.extend_from_slice(extra);
        Site::with_config(CONFIG, &files)
    }

    #[test]
    fn a_listed_partial_is_a_row_in_its_folder_listing_without_a_link_or_a_page() {
        let site = site(&[]);
        let home = site.html("index.html");
        assert!(home.contains("Lantern"), "the page-less entry is listed on the home embed");
        assert!(home.contains(r#"data-field="sector" data-value="lighting""#), "its card field is drawn: {home}");
        assert!(!hrefs_in(&home).iter().any(|h| h.contains("lantern")), "nothing links to a page it does not have");
        assert!(home.contains("Harbour") && hrefs_in(&home).iter().any(|h| h.contains("harbour")), "the ordinary page keeps its link");
        assert!(!home.contains("Snippet"), "a partial without `listed: true` stays out, as before");

        let folder = site.html("work/index.html");
        assert!(folder.contains("Lantern"), "the folder's own listing carries it too");
        assert!(!site.output("work/lantern/index.html").exists(), "no page is built for it");
    }

    #[test]
    fn a_listed_partial_appears_only_in_listings_and_never_as_an_address() {
        let site = site(&[]);
        let mut seen = Vec::new();
        let staging = site.output("");
        for entry in walk(&staging) {
            let rel = entry.strip_prefix(&staging).unwrap().to_string_lossy().replace('\\', "/");
            let Ok(text) = std::fs::read_to_string(&entry) else { continue };
            assert!(!text.contains("/work/lantern/"), "{rel} names an address the row does not have");
            if text.contains("Lantern") {
                seen.push(rel);
            }
        }
        seen.sort();
        // llms.txt writes out the home page's listing, so the row is a line of text there.
        assert_eq!(seen, ["index.html", "llms.txt", "work/index.html"], "only the listings carry it");
    }

    fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() { out.extend(walk(&path)) } else { out.push(path) }
        }
        out
    }

    #[test]
    fn a_dated_row_in_a_year_grouped_listing_keeps_its_card_whole() {
        let site = site(&[
            ("work/kiln.md", "---\ntitle: Kiln\ndate: 2024-05-01\npartial: true\nlisted: true\n---\n"),
            ("work/harbour.md", "---\ntitle: Harbour\ndate: 2024-03-01\n---\n\nThe case study.\n"),
            ("dated.md", "---\ntitle: Dated\n---\n\n![[work/|sort:date,style:list]]\n"),
        ]);
        for page in ["dated/index.html", "work/index.html"] {
            let html = site.html(page);
            assert!(html.contains("Kiln"), "{page} lists the dated row");
            assert!(!html.contains("<div class=\"moss-prefix-link"), "{page}: a row inside a <p> card is never a <div>: {html}");
        }
    }

    #[test]
    fn a_listed_partial_is_not_listed_on_a_term_page() {
        let site = Site::with_config(
            &format!("{CONFIG}\n[terms.tags]\nfields = [\"tags\"]\ntitle = \"Tags\"\n"),
            &[
                ("index.md", "---\ntitle: Home\nhome: true\n---\n"),
                ("work/harbour.md", "---\ntitle: Harbour\ntags: [light]\n---\n\nThe case study.\n"),
                ("work/lantern.md", "---\ntitle: Lantern\ntags: [light]\npartial: true\nlisted: true\n---\n"),
            ],
        );
        let term = site.html("tags/light/index.html");
        assert!(term.contains("Harbour"), "the term page exists and lists the page");
        assert!(!term.contains("Lantern"), "a row with no page belongs to its own folder only");
    }

    #[test]
    fn a_folder_of_only_rows_still_gets_its_listing_page() {
        let site = Site::with_config(CONFIG, &[
            ("index.md", "---\ntitle: Home\nhome: true\n---\n"),
            ("work/lantern.md", "---\ntitle: Lantern\npartial: true\nlisted: true\n---\n"),
        ]);
        assert!(site.html("work/index.html").contains("Lantern"));
    }

    #[test]
    fn a_listed_partial_with_an_external_url_links_out() {
        let site = site(&[(
            "work/beacon.md",
            "---\ntitle: Beacon\nweight: 3\npartial: true\nlisted: true\nexternal_url: https://beacon.example/\n---\n",
        )]);
        let home = site.html("index.html");
        assert!(hrefs_in(&home).contains(&"https://beacon.example/"), "the row links to its outlet: {home}");
        assert!(!site.output("work/beacon/index.html").exists());
    }

    #[test]
    fn a_draft_listed_partial_is_not_listed() {
        let site = site(&[("work/ember.md", "---\ntitle: Ember\npartial: true\nlisted: true\ndraft: true\n---\n")]);
        assert!(!site.html("index.html").contains("Ember"));
    }
}
