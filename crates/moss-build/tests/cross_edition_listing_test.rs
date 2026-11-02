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

/// How many rows of `html` link to `href`.
fn rows_to(html: &str, href: &str) -> usize {
    html.matches(&format!(r#"<a href="{href}" class="moss-card""#)).count()
}

#[test]
fn a_translated_piece_is_listed_once_in_the_listing_pages_own_edition() {
    let site = bilingual();

    let archive = site.html("everything/index.html");
    assert_eq!(rows_to(&archive, "/song/"), 1, "the English Song stays: {archive}");
    assert_eq!(rows_to(&archive, "/de/lied/"), 0, "its German twin is folded into it: {archive}");
    assert_eq!(rows_to(&archive, "/essay/"), 1);
    assert_eq!(rows_to(&archive, "/de/aufsatz/"), 1, "a piece with no twin is still listed");

    let german = site.html("de/alles/index.html");
    assert_eq!(rows_to(&german, "/de/lied/"), 1, "the German Lied stays: {german}");
    assert_eq!(rows_to(&german, "/song/"), 0, "its English twin is folded into it: {german}");
    assert_eq!(rows_to(&german, "/essay/"), 1);
    assert_eq!(rows_to(&german, "/de/aufsatz/"), 1);
}

#[test]
fn the_limit_counts_after_translations_are_folded() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Home\n---\n\nText.\n"),
        ("Everything.md", "---\ntitle: Archive\nchildren: \"[[/]]\"\nchildren_depth: all\nchildren_limit: 2\n---\n\nAll.\n"),
        ("Song.md", "---\ntitle: Song\ndate: 2026-01-05\ntranslationKey: song\n---\n\nText.\n"),
        ("Essay.md", "---\ntitle: Essay\ndate: 2026-01-02\n---\n\nText.\n"),
        ("de/index.md", "---\ntitle: Start\n---\n\nWillkommen.\n"),
        ("de/Lied.md", "---\ntitle: Lied\ndate: 2026-01-06\ntranslationKey: song\n---\n\nText.\n"),
        ("de/Aufsatz.md", "---\ntitle: Aufsatz\ndate: 2026-01-03\n---\n\nText.\n"),
    ]);
    let archive = site.html("everything/index.html");
    // Without the fold the two newest rows would be Lied and Song, and Essay would be cut.
    assert_eq!(rows_to(&archive, "/song/"), 1, "{archive}");
    assert_eq!(rows_to(&archive, "/de/aufsatz/"), 1, "{archive}");
    assert_eq!(rows_to(&archive, "/de/lied/") + rows_to(&archive, "/essay/"), 0, "{archive}");
}
