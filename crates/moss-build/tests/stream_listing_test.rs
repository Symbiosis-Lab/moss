//! `style:stream` draws each page whole: its title when it has one of its own,
//! its body, then its date as a link to the page.

#[path = "support/site.rs"]
mod site;

use site::Site;
use std::collections::HashSet;

const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
    0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0,
    0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99, 0x3d, 0x1d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

fn posts() -> Vec<(&'static str, &'static str)> {
    vec![
        ("index.md", "---\ntitle: Home\n---\n\nWelcome.\n"),
        ("posts/index.md", "---\ntitle: Posts\nchildren_style: stream\n---\n\nAll posts.\n"),
        ("posts/2026-01-02 0910.md", "First thought.\n\nSecond paragraph of it.\n\n---\n\nAfter the break.\n"),
        ("posts/2026-01-03 1200.md", "A note[^1] with a footnote.\n\n[^1]: The first source.\n"),
        ("posts/2026-01-04 0800.md", "Another note[^1].\n\n[^1]: The second source.\n"),
        ("posts/2026-01-01 Winter list.md", "# Deep heading\n\nWhat to pack.\n"),
    ]
}

fn stream(html: &str) -> &str {
    let start = html.find("class=\"moss-stream").expect("a stream container");
    &html[start..]
}

fn ids(html: &str) -> Vec<String> {
    html.split(" id=\"").skip(1).filter_map(|r| r.split('"').next()).map(str::to_string).collect()
}

#[test]
fn a_stream_home_draws_every_page_whole_with_its_date_as_a_link() {
    let html = Site::new(&posts()).html("posts/index.html");
    let s = stream(&html);
    assert_eq!(s.matches("moss-stream-item").count(), 4, "{s}");
    assert!(s.contains("Second paragraph of it.") && s.contains("After the break."), "the body is drawn whole: {s}");
    assert!(s.contains("<hr"), "the thread break stays a break: {s}");
    assert!(s.contains(r#"datetime="2026-01-02T09:10">January 2, 2026, 09:10</time>"#), "a post stamped with a time shows it: {s}");
    assert!(s.contains(r#"datetime="2026-01-01">January 1, 2026</time>"#), "a post dated by its day alone shows the day: {s}");
    assert!(s.contains("class=\"dt-published\"") && s.contains("class=\"moss-stream-date\"><a class=\"u-url\" href="), "{s}");
    assert_eq!(s.matches("moss-stream-title").count(), 1, "only the titled post has a heading: {s}");
    assert!(s.contains("Winter list"), "{s}");
    assert!(!s.contains("moss:listing") && !s.contains("MOSS_MARKER"), "{s}");
    assert!(!s.contains("also-on") && !s.to_lowercase().contains("also on"), "{s}");
}

#[test]
fn a_page_heading_is_demoted_below_the_listing_page_title() {
    let html = Site::new(&posts()).html("posts/index.html");
    let s = stream(&html);
    assert!(!s.contains("<h1"), "{s}");
    assert!(s.contains("Deep heading"), "{s}");
    assert!(s.contains("Deep heading</h2>"), "{s}");
}

#[test]
fn ids_stay_unique_and_footnote_links_still_land() {
    let html = Site::new(&posts()).html("posts/index.html");
    let all = ids(&html);
    let unique: HashSet<&String> = all.iter().collect();
    assert_eq!(all.len(), unique.len(), "duplicate ids: {all:?}");
    let s = stream(&html);
    let hrefs: Vec<&str> = s.split("href=\"#").skip(1).filter_map(|r| r.split('"').next()).collect();
    assert!(hrefs.len() >= 4, "two footnotes, each with a link and a backlink: {hrefs:?}");
    for href in hrefs {
        assert!(all.iter().any(|id| id == href), "#{href} lands nowhere in {all:?}");
    }
}

#[test]
fn a_page_that_lists_a_folder_is_a_row_whichever_side_of_the_host_it_sorts() {
    for host in ["a-host.md", "zhost.md"] {
        let host_page = format!("{}/index.html", host.trim_end_matches(".md"));
        let files = vec![
            (host, "---\ntitle: Stream\n---\n\n![[posts/|style:stream]]\n"),
            ("other/one.md", "---\ntitle: One\ndate: 2026-01-01\n---\n\nOne.\n"),
            ("posts/2026-01-02 0910.md", "Plain post body.\n"),
            ("posts/2026-01-03 0910.md", "Before.\n\nHidden tail.\n\n![[other/]]\n"),
            ("posts/2026-01-04 0910.md", "Also lists.\n\n![[other/|style:list]]\n"),
        ];
        let site = Site::new(&files);
        let html = site.html(&host_page);
        let s = stream(&html);
        assert!(!html.contains("MOSS_MARKER"), "{host}: no raw marker leaks: {s}");
        assert!(s.contains("Plain post body."), "{host}: {s}");
        assert!(s.matches("moss-stream-row").count() == 2, "{host}: both embedding pages are rows: {s}");
        assert!(!s.contains("Hidden tail."), "{host}: an embedding page's body is not inlined: {s}");
    }
}

#[test]
fn a_stream_host_takes_the_features_of_the_pages_it_draws() {
    let site = Site::new(&[
        ("host.md", "---\ntitle: Host\n---\n\n![[posts/|style:stream]]\n"),
        ("posts/index.md", "---\ntitle: Posts\nchildren_style: stream\n---\n\nAll posts.\n"),
        ("posts/2026-01-02 0910.md", "![[pic.png]]\n\nA picture.\n"),
        ("other/index.md", "---\ntitle: Other\n---\n\nPlain.\n"),
    ]);
    std::fs::write(site.root.join("posts/pic.png"), PNG).unwrap();
    site.rebuild();
    for page in ["host/index.html", "posts/index.html"] {
        assert!(site.html(page).contains("id=\"lightbox\""), "{page} draws a picture, so it carries the viewer");
    }
    assert!(!site.html("other/index.html").contains("id=\"lightbox\""));
}

#[test]
fn pictures_load_lazily_and_the_limit_is_honoured() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Home\n---\n\n![[posts/|style:stream,limit:2]]\n"),
        ("posts/2026-01-02 0910.md", "![[pic.png]]\n\nWith a picture.\n"),
        ("posts/2026-01-03 0910.md", "![[pic.png]]\n\nAnother picture.\n"),
        ("posts/2026-01-04 0910.md", "Newest, no picture.\n"),
    ]);
    std::fs::write(site.root.join("posts/pic.png"), PNG).unwrap();
    site.rebuild();
    let html = site.html("index.html");
    let s = stream(&html);
    assert_eq!(s.matches("moss-stream-item").count(), 2, "{s}");
    assert!(!s.contains("fetchpriority"), "{s}");
    assert!(s.contains("Another picture."), "{s}");
    assert!(!s.contains("eager"), "{s}");
}

/// Every id on the page is unique, and every reference to an id (a fragment
/// link, a label's `for`, an `aria-*` list) names one that exists.
fn assert_ids_hold(html: &str) {
    let all = ids(html);
    let unique: HashSet<&String> = all.iter().collect();
    assert_eq!(all.len(), unique.len(), "duplicate ids: {all:?}");
    let attr = |name: &str| -> Vec<String> {
        html.split(&format!(" {name}=\"")).skip(1).filter_map(|r| r.split('"').next()).flat_map(|v| v.split(' ').map(str::to_string).collect::<Vec<_>>()).collect()
    };
    let mut refs: Vec<String> = attr("for");
    refs.extend(attr("aria-describedby"));
    refs.extend(attr("aria-labelledby"));
    refs.extend(html.split("href=\"#").skip(1).filter_map(|r| r.split('"').next()).filter(|r| !r.is_empty()).map(str::to_string));
    for r in refs {
        assert!(all.contains(&r), "#{r} names no id on the page; ids: {all:?}");
    }
}

fn form_and_footnote_posts() -> Vec<(&'static str, &'static str)> {
    vec![
        ("posts/index.md", "---\ntitle: Posts\nchildren_style: stream\n---\n\n![[/posts/|style:stream,limit:1]]\n"),
        ("posts/2026-01-03 1200.md", "Write to me[^1].\n\n:::form {#ask next=/thanks/}\n- Your name *\n:::\n\n[^1]: Replies are read.\n"),
        ("posts/2026-01-02 1200.md", "An older note[^1].\n\n[^1]: Its source.\n"),
    ]
}

#[test]
fn a_page_drawn_twice_on_one_page_keeps_its_ids_and_their_references_apart() {
    let mut files = form_and_footnote_posts();
    files.push(("index.md", "---\ntitle: Home\nchildren: false\n---\n\n![[posts/|style:stream,limit:1]]\n\nBetween.\n\n![[posts/|style:stream,limit:1]]\n"));
    let site = Site::new(&files);
    let home = site.html("index.html");
    assert_eq!(stream(&home).matches("Write to me").count(), 2, "the newest post is drawn by both embeds");
    assert!(home.contains(" for=\""), "the form draws a label: {home}");
    assert_ids_hold(&home);
    // An embed and the page's own stream listing draw the newest post twice.
    let posts = site.html("posts/index.html");
    assert_eq!(posts.matches("Write to me").count(), 2, "{posts}");
    assert_ids_hold(&posts);
}

#[test]
fn a_stream_on_a_page_elsewhere_rebases_each_body_to_that_page() {
    let site = Site::new(&[
        ("alice.md", "---\ntitle: Alice\nauthor_page: true\nchildren_style: stream\n---\n\nBio.\n"),
        ("notes/one.md", "---\ntitle: One\nauthor: Alice\ndate: 2026-01-01\n---\n\nSee [the next one](two.md).\n"),
        ("notes/two.md", "---\ntitle: Two\ndate: 2026-01-02\n---\n\nText.\n"),
    ]);
    let html = site.html("alice/index.html");
    let s = stream(&html);
    let body = s.split("moss-stream-body").nth(1).unwrap_or_else(|| panic!("no body drawn in {s}"));
    let href = body.split(" href=\"").nth(1).and_then(|r| r.split('"').next()).unwrap();
    assert!(!href.starts_with('/') && !href.contains("://"), "a relative link stays relative: {href}");
    let target = site.output("alice").join(href).join("index.html");
    assert!(target.exists(), "{href} does not resolve from alice/: {}", target.display());
}

#[test]
fn the_first_picture_loads_at_once_unless_the_page_has_its_own() {
    let posts = [("posts/2026-01-02 0910.md", "![A pier](pier.png)\n\nNewest, with a picture.\n")];
    let first_img = |site: &Site| {
        let html = site.html("index.html");
        let s = stream(&html);
        s.split("<img").nth(1).and_then(|t| t.split('>').next()).map(str::to_string).unwrap_or_else(|| panic!("no picture in {s}"))
    };
    let bare = Site::new(&[("index.md", "---\ntitle: Home\nchildren: false\n---\n\n![[posts/|style:stream]]\n"), posts[0]]);
    std::fs::write(bare.root.join("posts/pier.png"), PNG).unwrap();
    bare.rebuild();
    assert!(!first_img(&bare).contains("loading=\"lazy\""), "{}", first_img(&bare));

    let pictured = Site::new(&[("index.md", "---\ntitle: Home\nchildren: false\n---\n\n![The harbour](harbour.png)\n\n![[posts/|style:stream]]\n"), posts[0]]);
    std::fs::write(pictured.root.join("posts/pier.png"), PNG).unwrap();
    std::fs::write(pictured.root.join("harbour.png"), PNG).unwrap();
    pictured.rebuild();
    assert!(first_img(&pictured).contains("loading=\"lazy\""), "the page's own picture takes the eager slot: {}", first_img(&pictured));
}

#[test]
fn a_host_takes_features_only_from_the_pages_it_draws_whole() {
    let site = Site::new(&[
        ("host.md", "---\ntitle: Host\n---\n\n![[posts/|style:stream]]\n"),
        ("posts/2026-01-02 0910.md", "Plain.\n"),
        ("posts/2026-01-03 0910.md", "![A pier](pier.png)\n\nLists too.\n\n![[other/]]\n"),
        ("other/one.md", "---\ntitle: One\ndate: 2026-01-01\n---\n\nOne.\n"),
    ]);
    std::fs::write(site.root.join("posts/pier.png"), PNG).unwrap();
    site.rebuild();
    let html = site.html("host/index.html");
    assert!(stream(&html).contains("moss-stream-row"), "{html}");
    assert!(!html.contains("id=\"lightbox\""), "the picture belongs to a row, which draws no picture");
}

#[test]
fn a_children_source_host_takes_the_features_of_the_pages_it_draws() {
    let site = Site::new(&[
        ("reading.md", "---\ntitle: Reading\nchildren_source: posts\nchildren_style: stream\n---\n\nWhat I read.\n"),
        ("posts/index.md", "---\ntitle: Posts\n---\n\nAll posts.\n"),
        ("posts/2026-01-02 0910.md", "![A pier](pier.png)\n\nA picture.\n"),
    ]);
    std::fs::write(site.root.join("posts/pier.png"), PNG).unwrap();
    site.rebuild();
    let html = site.html("reading/index.html");
    assert!(stream(&html).contains("A picture."), "{html}");
    assert!(html.contains("id=\"lightbox\""), "the drawn post's picture needs the viewer");
}

#[test]
fn a_home_naming_an_edition_home_takes_the_features_of_the_pages_it_draws() {
    let site = Site::in_language("zh-hans", "", &[
        ("Field Notes.md", "---\ntitle: Field Notes\nhome: true\ntranslationKey: home\nchildren: \"[[Notes]]\"\nchildren_style: stream\n---\n\nHome.\n"),
        ("en/Notes.md", "---\ntitle: Notes\nhome: true\ntranslationKey: home\n---\n\nHome.\n"),
        ("en/posts/2026-01-02 0910.md", "![A pier](pier.png)\n\nA picture.\n"),
    ]);
    std::fs::write(site.root.join("en/posts/pier.png"), PNG).unwrap();
    site.rebuild();
    let html = site.html("index.html");
    assert!(stream(&html).contains("A picture."), "{html}");
    assert!(html.contains("id=\"lightbox\""), "the drawn post's picture needs the viewer");
}

#[test]
fn a_subscribe_form_a_stream_draws_carries_its_language_scope() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Home\nlang: en\ntranslationKey: home\n---\n\nHi.\n"),
        ("zh-hans/index.md", "---\ntitle: 首页\ntranslationKey: home\nchildren: false\n---\n\n![[/zh-hans/posts/|style:stream]]\n"),
        ("zh-hans/posts/2026-01-02 0910.md", "订阅我们。\n\n:::subscribe\n:::\n"),
    ]);
    let html = site.html("zh-hans/index.html");
    let s = stream(&html);
    assert!(s.contains(r#"<input type="hidden" name="scope" value="zh-hans""#), "{s}");
}
