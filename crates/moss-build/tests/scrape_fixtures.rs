//! Whole-page regression suite for the article import pipeline.
//!
//! Fixtures live in `tests/fixtures/scrape/`. Each is a synthetic page that
//! copies the markup of a layout the importer meets in the wild: a paywalled
//! WordPress news article, a Q&A interview with embedded book cards, a
//! page-builder layout with no `<article>` container, and a hosted
//! portfolio's article listing. Names, text and URLs are invented; the
//! structure is what the tests exercise — schema.org graphs, decoy meta
//! tags, site chrome, teaser grids, paywall scripts, lazy images. Do not add
//! saved copies of real pages here: they carry someone's published work,
//! their name, and whatever keys the site embeds.
//!
//! Metadata assertions are exact. Body assertions are invariants ("the lede
//! is present", "site chrome doesn't leak into the body") rather than a
//! pinned markdown string, which would break on every htmd / extractor
//! refinement without catching anything new.

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
fn paywalled_news_article_extracts_metadata_and_body() {
    let html = fixture("paywalled-news-article");
    let art = extract_article(
        &html,
        "https://news.example.com/2024/03/17/night-ferry-quiet-return/",
    );

    // Metadata comes from the JSON-LD graph: its headline beats the
    // suffixed og:title, its Person author beats the editor named in
    // <meta name="author">, and its date beats the one in the URL.
    assert_eq!(
        art.metadata.title.as_deref(),
        Some("The Night Ferry\u{2019}s Quiet Return")
    );
    assert_eq!(art.metadata.date.as_deref(), Some("2024-03-18"));
    assert_eq!(art.metadata.author.as_deref(), Some("Alex Reporter"));
    assert_eq!(art.metadata.publisher.as_deref(), Some("The Example Wire"));
    assert_eq!(art.metadata.lang.as_deref(), Some("en"));
    assert!(art.metadata.description.is_some());

    // Body — invariants only. The related-article teasers sit in
    // `.post-content`, which outranks `.entry-content` in the selector list;
    // only the full body's weight keeps the scorer on the article.
    let md = &art.markdown;
    assert!(md.len() > 2_000, "body too short: {} bytes", md.len());
    assert!(md.contains("Harbor Lights Books"), "lede content missing");
    assert!(md.contains("Robin Sample"), "key interview subject missing");
    assert!(
        md.contains("[Casey Placeholder](https://www.example.org/wiki/Casey_Placeholder)"),
        "inline link not preserved"
    );

    // Site chrome and the paywall script's config must NOT leak in.
    for m in ["Newsletter", "Subscribe to The Example Wire", "Read more", "pk_test_"] {
        assert!(!md.contains(m), "chrome leaked into body: {:?}", m);
    }

    // An in-body photo — not just the og:image cover — is queued as an
    // absolute URL we'd download.
    assert!(
        art.media_urls
            .contains("https://news.example.com/wp-content/uploads/2024/03/robin-sample-1200x800.jpg"),
        "expected the in-body photo in media_urls: {:?}",
        art.media_urls
    );
}

#[test]
fn interview_with_book_cards_extracts_metadata_and_body() {
    let html = fixture("interview-with-book-cards");
    let art = extract_article(&html, "https://books.example.org/2025/06/10/jamie-author/");

    assert_eq!(
        art.metadata.title.as_deref(),
        Some("Jamie Author on Keeping an Almanac")
    );
    assert_eq!(art.metadata.date.as_deref(), Some("2025-06-10"));
    assert_eq!(art.metadata.author.as_deref(), Some("Alex Reporter"));
    assert_eq!(art.metadata.publisher.as_deref(), Some("Example Books Review"));
    assert_eq!(art.metadata.lang.as_deref(), Some("en"));

    let md = &art.markdown;
    assert!(md.len() > 2_000, "body too short: {} bytes", md.len());
    assert!(md.contains("The Lantern Almanac"));
    assert!(md.contains("Twelve Evenings"));

    // A book card wraps its cover image in a link; the cover is still queued.
    assert!(
        art.media_urls
            .contains("https://books.example.org/wp-content/uploads/2025/06/lantern-almanac-cover.jpg"),
        "expected the linked book cover in media_urls: {:?}",
        art.media_urls
    );
}

#[test]
fn page_builder_layout_extracts_body() {
    // No <article> and no .entry-content: the body sits in builder panels
    // under <main>, which the scorer has to fall back to.
    let html = fixture("page-builder-article");
    let art = extract_article(
        &html,
        "https://media.example.org/2024/08/05/moonlight-markets/",
    );

    assert_eq!(
        art.metadata.title.as_deref(),
        Some("Who Is Watching the Moonlight Markets?")
    );
    let md = &art.markdown;
    assert!(md.len() > 3_000, "body too short: {} bytes", md.len());
    assert!(md.contains("moonlight-market video"), "body paragraphs missing");
}

#[test]
fn portfolio_listing_extracts_outbound_article_links() {
    // A hosted portfolio's work list: a grid of cards, each one a single
    // link out to the published article, with images only JS would load.
    // Cards past the first page arrive by JS and are not in the HTML, but
    // the server-rendered ones survive — useful as a seed list for batch
    // import.
    let html = fixture("portfolio-listing");
    let art = extract_article(&html, "https://alexreporter.portfolio.example.com/");
    let md = &art.markdown;

    assert!(md.len() > 200, "listing body too thin");
    // Every card's link survives: the scorer keeps the listing, not one card.
    for url in [
        "https://books.example.org/2025/06/10/jamie-author/",
        "https://media.example.org/2024/08/05/moonlight-markets/",
        "https://www.example.net/2023/05/03/harbour-master-ledger/",
        "https://news.example.com/2023/09/21/lighthouse-keepers/",
        "https://magazine.example.com/2022/11/14/salt-roads/",
        "https://www.example.net/2022/06/02/tide-tables/",
    ] {
        assert!(md.contains(url), "outbound article link missing: {url}");
    }
}
