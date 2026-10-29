//! A file that is never a page (a link-only `partial: true` header entry, a
//! `footer.md`) takes the site's language: outside a language tree its chrome is
//! the site's, so an English page elsewhere in the folder must not move it out of
//! the header of a German site.

#[path = "support/site.rs"]
mod site;

use site::Site;

const ENGLISH_PAGE: &str = "---\ntitle: Notes\nnav: false\nlisted: false\n---\n\n# Notes\n\nThe builder wanted a typeset lockup in the header and looked up the guide, which said the site name is one text link. This is a long English paragraph so that language detection has enough words to decide that the page is written in English rather than German.\n";

const BASE: &[(&str, &str)] = &[
    ("index.md", "---\nhome: true\ntitle: Startseite\n---\n\nHallo.\n\n## Kontakt {#kontakt}\n\nText.\n"),
    ("Termine/Termine.md", "---\ntitle: Termine\nnav: true\nweight: 2\n---\n\nListe.\n"),
    ("contact-link.md", "---\ntitle: Kontakt\npartial: true\nnav: true\nweight: 3\nexternal_url: \"/#kontakt\"\n---\n"),
];

#[test]
fn a_link_only_partial_stays_in_the_header_when_an_english_page_joins_a_german_site() {
    let mut files = BASE.to_vec();
    files.push(("notes.md", ENGLISH_PAGE));
    let site = Site::in_language("de", "", &files);
    let links = site.links("index.html").0;
    assert!(links.contains(&"/#kontakt".to_string()), "{links:?}");
    assert!(links.contains(&"/termine/".to_string()), "{links:?}");
}

#[test]
fn an_english_footer_does_not_move_a_german_root_page_out_of_the_header() {
    let site = Site::in_language(
        "de",
        "",
        &[
            ("index.md", "---\nhome: true\ntitle: Startseite\n---\n\nHallo.\n"),
            ("Kontakt.md", "---\ntitle: Kontakt\nnav: true\n---\n\nText.\n"),
            ("footer.md", ENGLISH_PAGE),
        ],
    );
    let links = site.links("index.html").0;
    assert!(links.contains(&"/kontakt/".to_string()), "{links:?}");
}
