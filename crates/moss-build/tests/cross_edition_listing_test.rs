//! A whole-site archive lists every edition's pages. A row in another language
//! than the archive says so (`lang` on its words, a small mark naming the
//! language), and a piece that exists in two editions shows once.

#[path = "support/site.rs"]
mod site;

use site::Site;

const ARCHIVE: &str = "---\ntitle: Archive\nchildren: \"[[/]]\"\nchildren_depth: all\n---\n\nAll of it.\n";
const ARCHIVE_DE: &str = "---\ntitle: Alles\nchildren: \"[[/]]\"\nchildren_depth: all\n---\n\nAlles.\n";

fn bilingual() -> Site {
    Site::new(&[
        ("index.md", "---\ntitle: Home\n---\n\nText.\n"),
        ("Everything.md", ARCHIVE),
        ("Essay.md", "---\ntitle: Essay\ndate: 2026-01-01\n---\n\nText.\n"),
        ("Song.md", "---\ntitle: Song\ndate: 2026-01-02\ntranslationKey: song\n---\n\nText.\n"),
        ("de/index.md", "---\ntitle: Start\n---\n\nWillkommen.\n"),
        ("de/Alles.md", ARCHIVE_DE),
        ("de/Aufsatz.md", "---\ntitle: Aufsatz\ndate: 2026-01-03\n---\n\nText.\n"),
        ("de/Lied.md", "---\ntitle: Lied\ndate: 2026-01-04\ntranslationKey: song\n---\n\nText.\n"),
    ])
}

/// The listing's row for the page titled `title`: from its opening tag to the next row's.
fn row<'a>(html: &'a str, title: &str) -> &'a str {
    let at = html.find(&format!(">{title}<")).or_else(|| html.find(&format!(">{title}</")))
        .unwrap_or_else(|| panic!("no row titled {title}: {html}"));
    let start = html[..at].rfind("<a ").or_else(|| html[..at].rfind("<h3")).unwrap_or(0);
    let end = html[at..].find("</a>").map_or(html.len(), |e| at + e);
    &html[start..end]
}

#[test]
fn a_row_in_another_language_carries_its_language_and_a_mark() {
    let site = bilingual();
    let archive = site.html("everything/index.html");

    let german = row(&archive, "Aufsatz");
    assert!(german.contains(r#"lang="de""#), "the German row's title is marked de: {german}");
    assert!(
        german.contains(r#"<span class="moss-lang-mark" lang="en">German</span>"#),
        "and names the language in the archive's language: {german}"
    );

    let english = row(&archive, "Essay");
    assert!(!english.contains("lang=") && !english.contains("moss-lang-mark"), "an English row gains nothing: {english}");

    let german_archive = site.html("de/alles/index.html");
    let english_row = row(&german_archive, "Essay");
    assert!(english_row.contains(r#"lang="en""#), "{english_row}");
    assert!(english_row.contains(r#"<span class="moss-lang-mark" lang="de">Englisch</span>"#), "named in German: {english_row}");
    let own = row(&german_archive, "Aufsatz");
    assert!(!own.contains("lang=") && !own.contains("moss-lang-mark"), "{own}");
}
