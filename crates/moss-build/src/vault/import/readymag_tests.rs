//! Tests for the Readymag dialect. Sibling file (not inline) because the
//! fixture-heavy module exceeds the ~150-line inline budget.

use super::*;

const VIEWER: &str = r#"<html><head><script>
window.ServerData = {"mags":{"m1":{"title":"Michael Okagaki","uri":"4612303","pages":[
 {"title":"Home","uri":"home","num":1,"htmlUrl":"https://c-p.rmcdn.net/acc/4612303/Snip-home.html"},
 {"title":"Music","uri":"music","num":3,"htmlUrl":"https://c-p.rmcdn.net/acc/4612303/Snip-music.html"}
]}}};
</script></head><body></body></html>"#;

const SNAPSHOT: &str = r#"<html><body>
<div class="rmwidget widget-text-v3" style="left: 50%; top: 50%; width: 434px;">
  <div class="text-viewer"><p><a href="/4612303/music/">Music</a> <a href="/4612303/home/">Home</a></p></div>
</div>
<div class="rmwidget widget-text-v3" style="left: 0px; top: 0px; width: 495px; height: 400px;">
  <div class="text-viewer"><p>First paragraph about the song.</p><p>Second paragraph with more text.</p></div>
</div>
<div class="rmwidget widget-picture" style="left: 0px; top: 150px; width: 300px; height: 100px;">
  <img src="https://i-p.rmcdn.net/acc/4612303/pic.jpg?w=960" alt="cover art">
</div>
<div class="rmwidget widget-iframe simple-iframe" style="left: 0px; top: 390px; height: 43px;">
  <iframe src="https://drive.google.com/file/d/FILE1/preview"></iframe>
</div>
<div class="rmwidget widget-picture" style="left: 0px; top: 500px; height: 200px;">
  <img src="https://i.ytimg.com/vi/abc123DEF-_/maxresdefault.jpg">
</div>
<div class="rmwidget widget-shape full-width" style="left: 0px; top: 40px;"></div>
</body></html>"#;

#[test]
fn detects_readymag_by_content_markers() {
    assert!(is_readymag(VIEWER));
    // The word alone is not evidence — both markers are required.
    assert!(!is_readymag(
        "<p>I built my site on Readymag and window.ServerData was neat</p>"
    ));
    assert!(!is_readymag("<img src=\"https://i-p.rmcdn.net/x.jpg\">"));
}

#[test]
fn manifest_lists_sibling_pages_excluding_the_current_one() {
    // The mag root IS the home page under another name — enqueuing
    // `/home/` from it would import the same page twice.
    let urls = manifest_urls(VIEWER, "https://readymag.website/u3067627634/4612303/");
    assert_eq!(
        urls,
        vec!["https://readymag.website/u3067627634/4612303/music/"]
    );
    // From a non-home page, home is listed in its CANONICAL mag-root form
    // so the crawler's visited set can dedupe it against the start URL.
    let urls = manifest_urls(VIEWER, "https://readymag.website/u3067627634/4612303/music/");
    assert_eq!(urls, vec!["https://readymag.website/u3067627634/4612303/"]);
    // Not a Readymag page → no manifest.
    assert!(manifest_urls("<html></html>", "https://example.com/").is_empty());
}

#[test]
fn snapshot_request_selects_current_page_within_account_scope() {
    assert_eq!(
        snapshot_request(VIEWER, "https://readymag.website/u3067627634/4612303/music/").as_deref(),
        Some("https://c-p.rmcdn.net/acc/4612303/Snip-music.html")
    );
    // A URL ending at the mag segment is the home page (num == 1).
    assert_eq!(
        snapshot_request(VIEWER, "https://readymag.website/u3067627634/4612303/").as_deref(),
        Some("https://c-p.rmcdn.net/acc/4612303/Snip-home.html")
    );
    // Unknown page → None (never guess).
    assert!(snapshot_request(VIEWER, "https://readymag.website/u3067627634/4612303/nope/").is_none());
}

#[test]
fn snapshot_outside_account_scope_is_refused() {
    // A foreign host is refused EVEN when it carries the mag segment —
    // the JSON is attacker-controlled, so host scope is the real gate
    // (page host or the platform CDN only).
    let foreign = VIEWER.replace(
        "https://c-p.rmcdn.net/acc/4612303/Snip-music.html",
        "https://evil.example.com/4612303/steal.html",
    );
    assert!(
        snapshot_request(&foreign, "https://readymag.website/u3067627634/4612303/music/").is_none()
    );
    // And the platform CDN without the mag segment is refused too.
    let unsegmented = VIEWER.replace(
        "https://c-p.rmcdn.net/acc/4612303/Snip-music.html",
        "https://c-p.rmcdn.net/other/Snip.html",
    );
    assert!(
        snapshot_request(&unsegmented, "https://readymag.website/u3067627634/4612303/music/")
            .is_none()
    );
}

#[test]
fn style_px_requires_property_boundaries() {
    assert_eq!(style_px("margin-top: 20px; top: 40px", "top"), Some(40.0));
    assert_eq!(style_px("line-height: 1.5; height: 400px", "height"), Some(400.0));
    assert_eq!(style_px("padding-left: 8px", "left"), None);
    // Percent on the real property means "not px-positioned" — no fallback.
    assert_eq!(style_px("top: 50%", "top"), None);
}

#[test]
fn animation_container_offsets_compose_through_nesting() {
    // Animated widgets carry left:0/top:0 themselves; the true offset sits
    // on animation-container wrappers, which compose additively.
    let snap = r#"<html><body>
<div class="rmwidget widget-text-v3" style="left: 0px; top: 0px; height: 400px;">
  <div class="text-viewer"><p>Alpha paragraph text here.</p><p>Beta paragraph text here!</p></div>
</div>
<div class="animation-container" style="left: 100px; top: 300px;"><div class="animation-container" style="left: 0px; top: 60px;"><div class="rmwidget widget-iframe" style="left: 0px; top: 0px; height: 43px;">
  <iframe src="https://drive.google.com/file/d/F2/preview"></iframe>
</div></div></div>
</body></html>"#;
    let content = extract(
        VIEWER,
        Some(snap),
        "https://readymag.website/u3067627634/4612303/music/",
    )
    .expect("extracts");
    let md = match content.body {
        AdapterBody::Markdown(md) => md,
        AdapterBody::Html(_) => panic!("markdown expected"),
    };
    // Effective iframe top = 0 + 60 + 300 = 360 of a 400px column → after
    // both paragraphs.
    assert_eq!(
        md,
        "Alpha paragraph text here.\n\nBeta paragraph text here!\n\n![[https://drive.google.com/uc?export=download&id=F2]]"
    );
}

#[test]
fn extract_walks_snapshot_in_reading_order_with_interleaved_media() {
    let content = extract(
        VIEWER,
        Some(SNAPSHOT),
        "https://readymag.website/u3067627634/4612303/music/",
    )
    .expect("readymag page should extract");
    assert_eq!(content.overrides.title.as_deref(), Some("Music"));
    let md = match content.body {
        AdapterBody::Markdown(md) => md,
        AdapterBody::Html(_) => panic!("readymag returns markdown"),
    };
    // Nav-anchor text widget is chrome — dropped.
    assert!(!md.contains("[Music]"), "nav leaked: {md}");
    // Image at 150/400 of the column interleaves after paragraph 1; the
    // transform query is stripped for the original.
    let expected = "First paragraph about the song.\n\n\
                    ![cover art](https://i-p.rmcdn.net/acc/4612303/pic.jpg)\n\n\
                    Second paragraph with more text.\n\n\
                    ![[https://drive.google.com/uc?export=download&id=FILE1]]\n\n\
                    ![[https://www.youtube.com/watch?v=abc123DEF-_]]";
    assert_eq!(md, expected);
}

#[test]
fn home_page_takes_the_mag_title() {
    let content = extract(
        VIEWER,
        Some(SNAPSHOT),
        "https://readymag.website/u3067627634/4612303/",
    )
    .expect("home should extract");
    assert_eq!(content.overrides.title.as_deref(), Some("Michael Okagaki"));
}

#[test]
fn missing_snapshot_returns_none_for_generic_fallback() {
    assert!(extract(VIEWER, None, "https://readymag.website/u3067627634/4612303/music/").is_none());
}

/// Corpus snapshot harness over the saved Okagaki site (not a CI test).
///
/// `MOSS_READYMAG_CORPUS` holds `viewer.html` + `snapshot-{uri}.html` per
/// page; outputs land in `MOSS_CORPUS_OUT`. Inline assertions pin the
/// acceptance invariants from the Phase 2 plan; interleave positions are
/// verified manually against the live site.
#[test]
#[ignore = "corpus harness: set MOSS_READYMAG_CORPUS and MOSS_CORPUS_OUT"]
fn readymag_corpus_snapshot() {
    let corpus = std::env::var("MOSS_READYMAG_CORPUS").expect("MOSS_READYMAG_CORPUS");
    let out = std::path::PathBuf::from(std::env::var("MOSS_CORPUS_OUT").expect("MOSS_CORPUS_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    let corpus = std::path::Path::new(&corpus);
    let viewer = std::fs::read_to_string(corpus.join("viewer.html")).unwrap();
    let base = "https://readymag.website/u3067627634/4612303";
    let mut rendered: std::collections::BTreeMap<String, String> = Default::default();
    for uri in ["home", "videos", "music", "personalhistory", "finaldays"] {
        let snapshot = std::fs::read_to_string(corpus.join(format!("snapshot-{uri}.html"))).unwrap();
        let url = format!("{base}/{uri}/");
        let content = extract(&viewer, Some(&snapshot), &url)
            .unwrap_or_else(|| panic!("{uri} should extract"));
        let md = match content.body {
            AdapterBody::Markdown(md) => md,
            AdapterBody::Html(_) => panic!("markdown expected"),
        };
        std::fs::write(
            out.join(format!("{uri}.md")),
            format!("title: {:?}\n---\n{md}\n", content.overrides.title),
        )
        .unwrap();
        rendered.insert(uri.to_string(), md);
    }
    let count = |s: &str, needle: &str| s.matches(needle).count();

    let home = &rendered["home"];
    assert!(home.contains("Michael Okagaki"), "home name: {home}");
    assert!(home.contains("May 14, 1957"), "home dates: {home}");
    assert!(!home.contains("[Personal History]"), "nav leaked: {home}");

    let videos = &rendered["videos"];
    assert_eq!(
        count(videos, "![[https://www.youtube.com/watch?v="),
        3,
        "videos page should carry 3 recovered YouTube embeds: {videos}"
    );

    let music = &rendered["music"];
    assert_eq!(
        count(music, "![[https://drive.google.com/uc?export=download"),
        6,
        "music page should carry 6 Drive audio embeds: {music}"
    );
    assert!(music.contains("ORIGINAL SONGS"), "music sections: {music}");

    let finaldays = &rendered["finaldays"];
    for date in [
        "December 2022",
        "September 20, 2023",
        "December 7, 2023",
        "December 14, 2023",
        "December 22, 2023",
    ] {
        assert!(finaldays.contains(date), "missing letter {date}");
    }

    let history = &rendered["personalhistory"];
    assert!(
        history.contains("Michael Bruce Okagaki"),
        "obituary lead: {history}"
    );
}
