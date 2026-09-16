//! Real-world HTML regression suite for the article import pipeline.
//!
//! Fixtures live in `tests/fixtures/scrape/` — saved pages from outlets we
//! expect to keep working: Wire China, China Books Review, China Media
//! Project, plus the journoportfolio aggregator (a JS-rendered site we
//! handle partially).
//!
//! These assertions are intentionally loose — we test invariants we'd
//! notice breaking ("the byline is parsed", "the lede paragraph is
//! present", "site chrome doesn't leak into the body") rather than
//! pinning the exact markdown string. Tight string snapshots break on
//! every htmd / extractor refinement; invariant tests catch regressions
//! without churn.
//!
//! To save a new fixture: `curl -sL -A "moss-import/test" <url> > tests/fixtures/scrape/<label>.html`

use moss_build::vault::import::scrape::converter::extract_article;
use std::fs;
use std::path::PathBuf;

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("scrape")
        .join(format!("{}.html", name));
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

#[test]
fn wirechina_finding_chinas_voice_extracts_metadata_and_body() {
    let html = fixture("wirechina-finding-voice");
    let art = extract_article(
        &html,
        "https://www.thewirechina.com/2024/12/22/finding-chinas-voice-abroad-chinese-diaspora-america/",
    );

    // Metadata — taken from JSON-LD; brittle to upstream content changes only.
    assert!(
        art.metadata.title.as_deref().map_or(false, |t| t.contains("Finding China") && t.contains("Voice")),
        "title missing or wrong: {:?}",
        art.metadata.title
    );
    assert_eq!(art.metadata.date.as_deref(), Some("2024-12-23"));
    assert_eq!(art.metadata.author.as_deref(), Some("Yi Liu"));
    assert_eq!(art.metadata.publisher.as_deref(), Some("The Wire China"));
    assert_eq!(art.metadata.lang.as_deref(), Some("en"));
    assert!(art.metadata.description.is_some());

    // Body — invariants only.
    let md = &art.markdown;
    assert!(md.len() > 5_000, "body too short: {} bytes", md.len());
    assert!(md.contains("JF Books"), "lede content missing");
    assert!(md.contains("Yu Miao"), "key interview subject missing");
    assert!(
        md.contains("Cai Xia"),
        "preserved inline link target ('Cai Xia') missing"
    );

    // Chrome must NOT leak in.
    let chrome_markers = ["Newsletter", "Subscribe to The Wire", "Read more"];
    for m in chrome_markers {
        let leaks = md
            .lines()
            .any(|line| line.trim() == m || line.trim().starts_with(&format!("{} ", m)));
        assert!(!leaks, "chrome marker leaked into body: {:?}", m);
    }

    // At least one image survived and is an absolute URL we'd download.
    assert!(
        art.media_urls
            .iter()
            .any(|u| u.starts_with("https://www.thewirechina.com/wp-content/")),
        "expected at least one Wire China CDN image in media_urls"
    );
}

#[test]
fn china_books_review_xu_zhiyuan_extracts_metadata_and_body() {
    let html = fixture("cbr-xu-zhiyuan");
    let art = extract_article(&html, "https://chinabooksreview.com/2026/05/19/xu-zhiyuan/");

    assert!(
        art.metadata
            .title
            .as_deref()
            .map_or(false, |t| t.contains("Xu Zhiyuan")),
        "title wrong: {:?}",
        art.metadata.title
    );
    assert_eq!(art.metadata.date.as_deref(), Some("2026-05-19"));
    assert_eq!(art.metadata.author.as_deref(), Some("Yi Liu"));
    assert_eq!(art.metadata.publisher.as_deref(), Some("China Books Review"));
    assert_eq!(art.metadata.lang.as_deref(), Some("en"));

    let md = &art.markdown;
    assert!(md.len() > 5_000, "body too short: {} bytes", md.len());
    assert!(md.contains("Liang Qichao"));
    assert!(md.contains("Thirteen Talks"));

    assert!(
        art.media_urls
            .iter()
            .any(|u| u.contains("chinabooksreview.com/wp-content/")),
        "expected at least one CBR CDN image"
    );
}

#[test]
fn china_media_project_real_america_extracts_body() {
    let html = fixture("china-media-real-america");
    let art = extract_article(
        &html,
        "https://chinamediaproject.org/2024/04/15/who-is-seeing-the-real-america/",
    );

    assert!(
        art.metadata
            .title
            .as_deref()
            .map_or(false, |t| t.contains("Real America")),
        "title wrong: {:?}",
        art.metadata.title
    );
    let md = &art.markdown;
    assert!(md.len() > 3_000, "body too short: {} bytes", md.len());
    assert!(md.contains("zero-dollar") || md.contains("Zero-dollar"));
}

#[test]
fn journoportfolio_aggregator_extracts_some_links() {
    // Yi's portfolio page is a JS-rendered aggregator. We don't get her full
    // work list (JS doesn't run), but we DO extract the article links that
    // are present in the server-rendered HTML — useful as a seed list for
    // batch import.
    let html = fixture("portfolio-journoportfolio");
    let art = extract_article(&html, "https://yiliu.journoportfolio.com/");
    let md = &art.markdown;

    assert!(md.len() > 200, "aggregator body too thin");
    // At least one outbound external article link survives in markdown.
    assert!(
        md.contains("https://www.thewirechina.com")
            || md.contains("https://chinabooksreview.com")
            || md.contains("https://thechinaproject.com"),
        "expected at least one outbound article URL in the aggregator import"
    );
}
