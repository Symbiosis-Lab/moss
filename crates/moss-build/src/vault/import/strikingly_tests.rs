//! Tests for the Strikingly adapter. Sibling file (not inline) because the
//! fixture-heavy module exceeds the ~150-line inline budget.

use super::*;

const MINI_STRIKINGLY: &str = r#"<html><head><title>T - Site</title></head><body>
<script>//<![CDATA[ window.$S={};$S.conf={"a":1};$S.stores={"pageData":{"pages":[]}}; //]]></script>
<img src="https://custom-images.strikinglycdn.com/res/xyz/image/upload/1/2.png">
</body></html>"#;

#[test]
fn detects_strikingly_by_content_markers() {
    assert!(is_strikingly(MINI_STRIKINGLY));
    // The word alone is not evidence — a blog POST ABOUT Strikingly must not match.
    assert!(!is_strikingly(
        "<html><body><p>I migrated off Strikingly!</p></body></html>"
    ));
    // Bootstrap without the platform CDN/store markers is not enough either.
    assert!(!is_strikingly(
        "<html><script>window.$S={};</script></html>"
    ));
}

#[test]
fn extracts_s_assignment_json() {
    let v = extract_s_assignment(MINI_STRIKINGLY, "stores").expect("stores parses");
    assert!(v.get("pageData").is_some());
    assert!(extract_s_assignment(MINI_STRIKINGLY, "blogPostData").is_none());
}

#[test]
fn s_assignment_stops_at_first_complete_value() {
    // The trailing `;$S.next=...` after the JSON value must be ignored.
    let v = extract_s_assignment(MINI_STRIKINGLY, "conf").expect("conf parses");
    assert_eq!(v.get("a").and_then(|a| a.as_i64()), Some(1));
}

#[test]
fn truncated_s_assignment_returns_none() {
    let html = r#"<script>window.$S={};$S.stores={"pageData":{"pages":[</script>"#;
    assert!(extract_s_assignment(html, "stores").is_none());
}

// ---- section renderer ----------------------------------------------------
//
// Fixture shapes mirror the real $S corpus (潮汐/.port/s-corpus): blog
// sections are `Blog.Section` wrappers with a single `component`; page
// sections are `Slide`s with a `components` map; ALL prose is `RichText`
// with HTML in `value` (no Title/SubTitle/Text types exist).

use serde_json::json;

const RES_ID: &str = "hrscywv4p";

#[test]
fn renders_richtext_value_html_as_markdown() {
    let sections = json!([
        {"type": "Blog.Section", "id": "s1", "component":
            {"type": "RichText", "id": "c1",
             "value": "<p style=\"text-align: justify;\"><span style=\"color: #000000;\">2024年10月，第四季作品出爐。</span></p>"}},
        {"type": "Blog.Section", "id": "s2", "component":
            {"type": "RichText", "id": "c2", "value": "<h2>得獎名單</h2>"}}
    ]);
    let out = render_sections(&sections, RES_ID);
    assert!(out.markdown.contains("2024年10月，第四季作品出爐。"), "got: {}", out.markdown);
    assert!(out.markdown.contains("## 得獎名單"), "got: {}", out.markdown);
    assert!(!out.markdown.contains("<p"), "raw HTML leaked: {}", out.markdown);
    assert!(out.skipped.is_empty(), "skipped: {:?}", out.skipped);
}

#[test]
fn renders_video_component_as_wikilink_embed() {
    // Real videos sit as bare Video components inside BlockComponent items
    // (verified on shortfilmfellowship-1): url is the oEmbed watch URL.
    let sections = json!([
        {"type": "Slide", "id": "s1", "components": {
            "block1": {"type": "BlockComponent", "id": "b1", "items": [
                {"type": "Video", "id": "v1", "html": "<iframe src=\"//cdn.embedly.com/...\"></iframe>",
                 "url": "https://youtu.be/BZ8A7LzN4P0", "thumbnail_url": null, "maxwidth": 700}
            ]}
        }}
    ]);
    let out = render_sections(&sections, RES_ID);
    assert!(
        out.markdown.contains("![[https://youtu.be/BZ8A7LzN4P0]]"),
        "got: {}",
        out.markdown
    );
}

#[test]
fn renders_image_component_via_cdn_url_when_url_is_sentinel() {
    // Corpus Image components usually carry the literal sentinel "!" in
    // url/thumb_url; the real asset is storageKey + format on the CDN.
    let sections = json!([
        {"type": "Blog.Section", "id": "s1", "component":
            {"type": "Image", "id": "i1", "url": "!", "thumb_url": "!",
             "caption": "海報", "storageKey": "5927377/856502_996934",
             "format": "jpg", "h": 4096, "w": 2950}}
    ]);
    let out = render_sections(&sections, RES_ID);
    assert!(
        out.markdown.contains(
            "![海報](https://custom-images.strikinglycdn.com/res/hrscywv4p/image/upload/c_limit,h_2400,w_2400,q_90/5927377/856502_996934.jpg)"
        ),
        "got: {}",
        out.markdown
    );
}

#[test]
fn image_with_real_url_prefers_it_over_cdn_construction() {
    let sections = json!([
        {"type": "Blog.Section", "id": "s1", "component":
            {"type": "Image", "id": "i1",
             "url": "//static-assets.strikinglycdn.com/images/list-D-1.png",
             "storageKey": "5927377/1_2", "format": "png"}}
    ]);
    let out = render_sections(&sections, RES_ID);
    assert!(
        out.markdown
            .contains("![](//static-assets.strikinglycdn.com/images/list-D-1.png)"),
        "got: {}",
        out.markdown
    );
    assert!(!out.markdown.contains("c_limit"), "got: {}", out.markdown);
}

#[test]
fn renders_blog_quote_as_blockquote() {
    let sections = json!([
        {"type": "Blog.Section", "id": "s1", "component":
            {"type": "Blog.Quote", "id": "q1",
             "value": "<p><strong>「進來的，都是我們的家人。」榮奧說。</strong></p>"}}
    ]);
    let out = render_sections(&sections, RES_ID);
    assert!(
        out.markdown.contains("> **「進來的，都是我們的家人。」榮奧說。**"),
        "got: {}",
        out.markdown
    );
}

#[test]
fn renders_separator_and_button() {
    let sections = json!([
        {"type": "Blog.Section", "id": "s1", "component": {"type": "Separator", "id": "x1", "value": null}},
        {"type": "Slide", "id": "s2", "components": {
            "text1": {"type": "Button", "id": "b1", "text": "關注日曆",
                      "url": "https://example.com/cal", "new_target": true},
            // defaultValue placeholder buttons have empty text/url — omitted.
            "text2": {"type": "Button", "id": "b2", "text": "", "url": ""}
        }}
    ]);
    let out = render_sections(&sections, RES_ID);
    assert!(out.markdown.contains("---"), "got: {}", out.markdown);
    assert!(
        out.markdown.contains("[關注日曆](https://example.com/cal)"),
        "got: {}",
        out.markdown
    );
    assert_eq!(out.markdown.matches('[').count(), 1, "placeholder button leaked: {}", out.markdown);
}

#[test]
fn recurses_structural_wrappers_and_skips_chrome_silently() {
    // Slide -> BlockComponent -> BlockComponentItem -> components map, plus
    // Repeatable/RepeatableItem; SlideSettings/Spacer/Background-without-
    // media/EmailForm are chrome — skipped without being counted.
    let sections = json!([
        {"type": "Slide", "id": "s1", "components": {
            "slideSettings": {"type": "SlideSettings", "id": "ss1", "name": "hero"},
            "background1": {"type": "Background", "id": "bg1", "url": "!", "useImage": false},
            "spacer1": {"type": "Spacer", "id": "sp1"},
            "form1": {"type": "EmailForm", "id": "f1"},
            "block1": {"type": "BlockComponent", "id": "b1", "items": [
                {"type": "BlockComponentItem", "id": "bi1", "name": "columnBlock", "components": {
                    "text1": {"type": "RichText", "id": "t1", "value": "<p>深層文本</p>"}
                }}
            ]},
            "repeat1": {"type": "Repeatable", "id": "r1", "list": [
                {"type": "RepeatableItem", "id": "ri1", "components": {
                    "text1": {"type": "RichText", "id": "t2", "value": "<p>重複項</p>"}
                }}
            ]}
        }}
    ]);
    let out = render_sections(&sections, RES_ID);
    assert!(out.markdown.contains("深層文本"), "got: {}", out.markdown);
    assert!(out.markdown.contains("重複項"), "got: {}", out.markdown);
    assert!(out.skipped.is_empty(), "chrome must not count as skipped: {:?}", out.skipped);
}

#[test]
fn media_current_image_renders_image_and_not_placeholder_video() {
    // Every Media in the corpus has current == "image"; its `video` child is
    // Strikingly's stock placeholder (vimeo/18150336) and must NOT embed.
    let sections = json!([
        {"type": "Slide", "id": "s1", "components": {
            "media1": {"type": "Media", "id": "m1", "current": "image",
                "video": {"type": "Video", "id": "v1", "url": "http://vimeo.com/18150336"},
                "image": {"type": "Image", "id": "i1", "url": "!", "thumb_url": "!",
                          "storageKey": "5927377/579505_898578", "format": "png"}}
        }}
    ]);
    let out = render_sections(&sections, RES_ID);
    assert!(!out.markdown.contains("vimeo"), "placeholder video leaked: {}", out.markdown);
    assert!(
        out.markdown.contains("5927377/579505_898578.png"),
        "got: {}",
        out.markdown
    );
}

#[test]
fn gallery_sources_render_as_images() {
    let sections = json!([
        {"type": "Slide", "id": "s1", "components": {
            "gallery1": {"type": "Gallery", "id": "g1", "sources": [
                {"type": "Image", "id": "i1",
                 "thumb_url": "//custom-images.strikinglycdn.com/res/hrscywv4p/image/upload/c_fill,h_200,w_200/5927377/a.jpg",
                 "url": "!", "storageKey": "5927377/938868_525143", "format": "jpg"}
            ]}
        }}
    ]);
    let out = render_sections(&sections, RES_ID);
    assert!(
        out.markdown.contains("5927377/938868_525143.jpg"),
        "got: {}",
        out.markdown
    );
}

#[test]
fn background_with_media_renders_image() {
    let sections = json!([
        {"type": "Slide", "id": "s1", "components": {
            "background1": {"type": "Background", "id": "bg1", "useImage": true,
                            "url": "!", "storageKey": "5927377/853769_759688",
                            "format": "jpg", "h": 3289, "w": 6339}
        }}
    ]);
    let out = render_sections(&sections, RES_ID);
    assert!(
        out.markdown.contains("5927377/853769_759688.jpg"),
        "got: {}",
        out.markdown
    );
}

#[test]
fn html_component_value_is_unescaped_then_converted() {
    // HtmlComponent.value arrives HTML-entity-escaped in the $S JSON.
    let sections = json!([
        {"type": "Blog.Section", "id": "s1", "component":
            {"type": "HtmlComponent", "id": "h1",
             "value": "&lt;p&gt;embedded &amp; escaped&lt;/p&gt;"}}
    ]);
    let out = render_sections(&sections, RES_ID);
    assert!(
        out.markdown.contains("embedded & escaped"),
        "got: {}",
        out.markdown
    );
}

#[test]
fn unknown_component_types_are_skipped_and_counted_without_panic() {
    let sections = json!([
        {"type": "Blog.Section", "id": "s1", "component":
            {"type": "FancyNewWidget", "id": "w1", "data": {"nested": [1, 2, 3]}}},
        {"type": "Blog.Section", "id": "s2", "component":
            {"type": "RichText", "id": "t1", "value": "<p>正文</p>"}},
        // Garbage entries (no type / non-object) must not panic.
        {"foo": "bar"},
        42
    ]);
    let out = render_sections(&sections, RES_ID);
    assert!(out.markdown.contains("正文"), "got: {}", out.markdown);
    assert_eq!(out.skipped, vec!["FancyNewWidget".to_string()]);
}

#[test]
fn empty_or_garbage_sections_render_empty() {
    let out = render_sections(&json!([]), RES_ID);
    assert!(out.markdown.is_empty());
    let out = render_sections(&json!("not an array"), RES_ID);
    assert!(out.markdown.is_empty());
}

#[test]
fn image_without_res_id_and_without_real_url_is_dropped() {
    // No res id readable from the page and only a storageKey — the CDN URL
    // cannot be built; drop the image rather than emit a broken link.
    let sections = json!([
        {"type": "Blog.Section", "id": "s1", "component":
            {"type": "Image", "id": "i1", "url": "!", "storageKey": "5927377/1_2", "format": "jpg"}}
    ]);
    let out = render_sections(&sections, "");
    assert!(!out.markdown.contains("!["), "got: {}", out.markdown);
}

#[test]
fn extracts_res_id_from_raw_html() {
    assert_eq!(
        extract_res_id(MINI_STRIKINGLY).as_deref(),
        Some("xyz"),
        "res id should come from the first strikinglycdn res/ URL"
    );
    assert_eq!(extract_res_id("<html>no cdn here</html>"), None);
}

// ---- adapter seam: extract() ---------------------------------------------

use crate::vault::import::scrape::converter::AdapterBody;

const BLOG_HTML: &str = r#"<html><head><title>【潮汐活動】寫作的決心 - 潮汐</title></head><body>
<script>//<![CDATA[
window.$S={};$S.conf={"locale":"zh-TW"};$S.blogPostData={"blogPostMeta":{"publishedAt":"2024-10-13T20:08:41.387-07:00","socialMediaConfig":{"url":"https://www.harborweekly.io/blog/202411event","title":"【潮汐活動】寫作的決心"}},"content":{"type":"Blog.Post","sections":[{"type":"Blog.Section","id":"s1","component":{"type":"RichText","id":"c1","value":"<p>時間：2024/11/9 14:30</p>"}},{"type":"Blog.Section","id":"s2","component":{"type":"Image","id":"i1","url":"!","thumb_url":"!","storageKey":"5927377/250970_432913","format":"jpeg"}}]}};$S.stores={"blogData":{}};
//]]></script>
<img src="https://custom-images.strikinglycdn.com/res/hrscywv4p/image/upload/c_limit/5927377/x.jpeg">
</body></html>"#;

const PAGE_HTML: &str = r#"<html><head><title>為什麼要發起「潮汐」？ - 潮汐</title></head><body>
<script>//<![CDATA[
window.$S={};$S.nav=[{"uid":"home-uid","name":"/4th-test","isHomePage":true},{"uid":"p2-uid","name":"/2","isHomePage":false}];$S.stores={"pageData":{"pages":[{"uid":"home-uid","path":"/4th-test","title":"首頁","sections":[{"type":"Slide","id":"hs1","components":{"text1":{"type":"RichText","id":"ht1","value":"<p>首頁內容</p>"}}}]},{"uid":"p2-uid","path":"/2","title":"為什麼要發起「潮汐」？","sections":[{"type":"Slide","id":"ps1","components":{"text1":{"type":"RichText","id":"pt1","value":"<p>第二頁內容</p>"}}}]}]}};
//]]></script>
<img src="https://custom-images.strikinglycdn.com/res/hrscywv4p/image/upload/c_limit/5927377/y.jpeg">
</body></html>"#;

fn expect_markdown_body(content: crate::vault::import::scrape::converter::SiteContent) -> String {
    match content.body {
        AdapterBody::Markdown(md) => md,
        AdapterBody::Html(_) => panic!("strikingly adapter returns markdown, not HTML"),
    }
}

#[test]
fn strikingly_blog_extract_overrides_title_and_date() {
    let content = extract(BLOG_HTML, "https://www.harborweekly.io/blog/202411event")
        .expect("blog post should extract");
    assert_eq!(
        content.overrides.title.as_deref(),
        Some("【潮汐活動】寫作的決心"),
        "clean socialMediaConfig title, no site suffix"
    );
    assert_eq!(content.overrides.date.as_deref(), Some("2024-10-13"));
    assert_eq!(content.overrides.publisher, None);
    assert_eq!(content.overrides.author, None);
    let md = expect_markdown_body(content);
    assert!(md.contains("時間：2024/11/9 14:30"), "got: {md}");
    assert!(
        md.contains("5927377/250970_432913.jpeg"),
        "CDN image built from page res id: {md}"
    );
}

#[test]
fn strikingly_page_extract_selects_current_page_sections() {
    let content = extract(PAGE_HTML, "https://www.harborweekly.io/2")
        .expect("page /2 should extract");
    assert_eq!(
        content.overrides.title.as_deref(),
        Some("為什麼要發起「潮汐」？")
    );
    let md = expect_markdown_body(content);
    assert!(md.contains("第二頁內容"), "got: {md}");
    assert!(!md.contains("首頁內容"), "sibling page leaked in: {md}");
}

#[test]
fn strikingly_homepage_matched_via_nav_is_home_page_uid() {
    // No page has path "/": the homepage is $S.nav[].isHomePage==true → uid
    // → pageData.pages[].uid (its own path is an arbitrary slug).
    let content = extract(PAGE_HTML, "https://www.harborweekly.io/")
        .expect("homepage should extract via nav uid");
    let md = expect_markdown_body(content);
    assert!(md.contains("首頁內容"), "got: {md}");
    assert!(!md.contains("第二頁內容"), "sibling page leaked in: {md}");
}

#[test]
fn strikingly_unmatched_page_returns_none() {
    // Never guess: a request path matching no page falls back to the
    // generic extractor (portfolio items land here by design).
    assert!(extract(PAGE_HTML, "https://www.harborweekly.io/nonexistent").is_none());
}

#[test]
fn non_strikingly_html_returns_none() {
    let html = "<html><body><article><p>plain site</p></article></body></html>";
    assert!(extract(html, "https://example.com/post").is_none());
}

#[test]
fn strikingly_parse_failure_falls_back_to_none() {
    // Detection fires (bootstrap + CDN marker) but the $S JSON is truncated —
    // graceful fallback, not a panic and not a half-rendered body.
    let html = r#"<html><body>
<script>window.$S={};$S.blogPostData={"blogPostMeta":{"publishedAt":"2024-</script>
<img src="https://custom-images.strikinglycdn.com/res/hrscywv4p/image/upload/a/b.png">
</body></html>"#;
    assert!(extract(html, "https://example.com/blog/x").is_none());
}

#[test]
fn extract_article_routes_strikingly_blog_through_adapter() {
    // End to end through the converter seam: overrides land in metadata,
    // the markdown body comes from $S, and the CDN image is queued for
    // download (media_urls) like any remote image.
    let art = crate::vault::import::scrape::converter::extract_article(
        BLOG_HTML,
        "https://www.harborweekly.io/blog/202411event",
    );
    assert_eq!(art.metadata.title.as_deref(), Some("【潮汐活動】寫作的決心"));
    assert_eq!(art.metadata.date.as_deref(), Some("2024-10-13"));
    assert!(art.markdown.contains("時間：2024/11/9 14:30"), "got: {}", art.markdown);
    assert!(
        art.media_urls.iter().any(|u| u.contains("250970_432913")),
        "CDN image queued for download: {:?}",
        art.media_urls
    );
}

/// Corpus snapshot harness (not a CI test — run explicitly).
///
/// Reconstructs minimal Strikingly HTML around each saved `$S` store dict in
/// `MOSS_STRIKINGLY_CORPUS` ({blog,pages}/*.json from the harbor port's
/// fetch_s_corpus.py) and writes `extract()`'s output (title/date overrides +
/// markdown) to `MOSS_CORPUS_OUT`, one `.md` per input. Diffing two runs of
/// this harness across a refactor is the zero-behavior-change gate —
/// reconstruction artifacts affect both runs equally, so the diff stays valid.
#[test]
#[ignore = "corpus harness: set MOSS_STRIKINGLY_CORPUS and MOSS_CORPUS_OUT"]
fn corpus_snapshot() {
    let corpus = std::env::var("MOSS_STRIKINGLY_CORPUS").expect("MOSS_STRIKINGLY_CORPUS");
    let out = std::path::PathBuf::from(std::env::var("MOSS_CORPUS_OUT").expect("MOSS_CORPUS_OUT"));
    for sub in ["pages", "blog"] {
        let dir = std::path::Path::new(&corpus).join(sub);
        let out_dir = out.join(sub);
        std::fs::create_dir_all(&out_dir).unwrap();
        let mut entries: Vec<_> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("read {dir:?}: {e}"))
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        entries.sort();
        for path in entries {
            let stores: serde_json::Map<String, serde_json::Value> =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            // serde_json (no preserve_order) sorts object keys — deterministic.
            let assigns: String = stores
                .iter()
                .map(|(k, v)| format!("$S.{k}={v};"))
                .collect();
            let html = format!(
                r#"<html><head><link href="https://custom-images.strikinglycdn.com/res/hrscywv4p/x/"><script>window.$S={{}};{assigns}</script></head><body></body></html>"#
            );
            let slug = path.file_stem().unwrap().to_string_lossy();
            let url = match (sub, slug.as_ref()) {
                ("pages", "home") => "https://www.harborweekly.io/".to_string(),
                ("pages", s) => format!("https://www.harborweekly.io/{s}"),
                (_, s) => format!("https://www.harborweekly.io/blog/{s}"),
            };
            let rendered = match extract(&html, &url) {
                Some(c) => {
                    let body = match c.body {
                        AdapterBody::Markdown(m) => m,
                        AdapterBody::Html(h) => format!("<HTML BODY>\n{h}"),
                    };
                    format!(
                        "title: {:?}\ndate: {:?}\n---\n{body}\n",
                        c.overrides.title, c.overrides.date
                    )
                }
                None => "<NONE>\n".to_string(),
            };
            std::fs::write(out_dir.join(format!("{slug}.md")), rendered).unwrap();
        }
    }
}
