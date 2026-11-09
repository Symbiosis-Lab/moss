use super::*;
use proptest::prelude::*;

fn alice() -> Mount {
    Mount::of_site_url("https://example.edu/~alice")
}

#[test]
fn the_mount_is_the_path_of_the_site_url() {
    assert_eq!(Mount::of_site_url("https://example.com").as_str(), "/");
    assert_eq!(Mount::of_site_url("https://example.com/").as_str(), "/");
    assert_eq!(alice().as_str(), "/~alice/");
    assert_eq!(Mount::of_site_url("https://example.edu/a/b/").as_str(), "/a/b/");
    assert_eq!(Mount::of_site_url("https://example.edu//a//b?x=1#y").as_str(), "/a/b/");
    // Quotes, parens and spaces cannot break out of an attribute or a url().
    assert_eq!(Mount::of_site_url("https://example.edu/a b(\"c\")").as_str(), "/a%20b%28%22c%22%29/");
}

#[test]
fn apply_and_strip_round_trip_root_relative_urls_only() {
    let m = alice();
    assert_eq!(m.apply("/notes/").as_deref(), Some("/~alice/notes/"));
    assert_eq!(m.apply("/").as_deref(), Some("/~alice/"));
    for untouched in ["//cdn.example/x", "/\\evil", "#top", "notes/", "https://example.com/x", "mailto:a@b", "data:x", ""] {
        assert_eq!(m.apply(untouched), None, "{untouched}");
    }
    assert_eq!(m.strip("/~alice/notes/"), Some("/notes/"));
    assert_eq!(m.strip("/~alice"), Some("/"));
    assert_eq!(m.strip("/~alicex/"), None);
    assert_eq!(m.strip("/~bob/x"), None);
    assert_eq!(Mount::root().strip("/x"), Some("/x"));
}

#[test]
fn the_root_mount_returns_the_bytes_unchanged() {
    let html = r#"<head></head><a href="/x">x</a><style>a{background:url(/a.png)}</style>"#;
    assert!(matches!(Mount::root().rewrite_html(html), Cow::Borrowed(_)));
    assert!(matches!(Mount::root().rewrite_css("a{background:url(/a.png)}"), Cow::Borrowed(_)));
}

#[test]
fn html_url_attributes_are_mounted() {
    let html = concat!(
        r#"<html><head><link rel="stylesheet" href="/style.css?v=1"></head><body>"#,
        r##"<a href="/notes/">n</a><a href="https://example.com/x">e</a><a href="//cdn/x">p</a><a href="#top">f</a><a href="rel/">r</a>"##,
        r#"<img src="/a.png" srcset="/a-1x.png 1x, /a-2x.png 2x, https://cdn/x.png 3x">"#,
        r#"<video poster="/p.jpg"><source src="/v.mp4"></video>"#,
        r#"<form action="/search"><button formaction="/go">go</button></form>"#,
        r#"<object data="/x.svg"></object><svg><use href="/s.svg#i"></use></svg>"#,
        r#"<a href='/single?a=1&amp;b=2'>s</a>"#,
        "</body></html>",
    );
    let out = alice().rewrite_html(html);
    for expected in [
        r#"href="/~alice/style.css?v=1""#,
        r#"href="/~alice/notes/""#,
        r#"href="https://example.com/x""#,
        r#"href="//cdn/x""#,
        r##"href="#top""##,
        r#"href="rel/""#,
        r#"src="/~alice/a.png""#,
        r#"srcset="/~alice/a-1x.png 1x, /~alice/a-2x.png 2x, https://cdn/x.png 3x""#,
        r#"poster="/~alice/p.jpg""#,
        r#"src="/~alice/v.mp4""#,
        r#"action="/~alice/search""#,
        r#"formaction="/~alice/go""#,
        r#"data="/~alice/x.svg""#,
        r##"href="/~alice/s.svg#i""##,
        r#"href="/~alice/single?a=1&amp;b=2""#,
    ] {
        assert!(out.contains(expected), "missing {expected} in {out}");
    }
}

#[test]
fn head_gets_the_base_meta_first() {
    let out = alice().rewrite_html(r#"<html><head><meta charset="utf-8"></head><body></body></html>"#);
    assert!(out.contains(r#"<head><meta name="moss:base" content="/~alice/"><meta charset="utf-8">"#), "{out}");
    // A fragment with no head gets none.
    assert!(!alice().rewrite_html(r#"<p><a href="/x">x</a></p>"#).contains("moss:base"));
}

#[test]
fn meta_refresh_inline_style_and_style_elements_are_mounted() {
    let html = concat!(
        r#"<head><meta http-equiv="refresh" content="0; url=/new/"><meta http-equiv="Refresh" content="0;URL='/q/'">"#,
        r#"<style>.a{background:url("/bg.png")} /* url(/comment.png) */ .b{content:"url(/text)"}</style></head>"#,
        r#"<div style="background-image:url(/inline.png)"></div>"#,
    );
    let out = alice().rewrite_html(html);
    assert!(out.contains(r#"content="0; url=/~alice/new/""#), "{out}");
    assert!(out.contains(r#"content="0;URL='/~alice/q/'""#), "{out}");
    assert!(out.contains(r#"url("/~alice/bg.png")"#), "{out}");
    assert!(out.contains("/* url(/comment.png) */"), "a comment is not a URL: {out}");
    assert!(out.contains(r#""url(/text)""#), "a string is not a URL: {out}");
    assert!(out.contains(r#"style="background-image:url(/~alice/inline.png)""#), "{out}");
}

#[test]
fn css_urls_are_mounted_token_by_token() {
    let css = concat!(
        "@import \"/theme.css\";\n@import url(/print.css) print;\n",
        "@font-face{src:url( /f.woff2 ) format(\"woff2\"),url('//cdn/f.woff')}\n",
        ".a{background:image-set(\"/a.png\" 1x, url(/a2.png) 2x)}\n",
        ".b{background:URL(/up.png)} .c{mask:url(data:image/svg+xml;x)} .d{background:url(rel.png)}\n",
        "@media (min-width:1px){.e{background:url(/nested.png)}}\n",
    );
    let out = alice().rewrite_css(css);
    for expected in [
        "@import \"/~alice/theme.css\"",
        "@import url(/~alice/print.css)",
        "url( /~alice/f.woff2 )",
        "format(\"woff2\")",
        "url('//cdn/f.woff')",
        "image-set(\"/~alice/a.png\" 1x, url(/~alice/a2.png) 2x)",
        "URL(/~alice/up.png)",
        "url(data:image/svg+xml;x)",
        "url(rel.png)",
        "url(/~alice/nested.png)",
    ] {
        assert!(out.contains(expected), "missing {expected} in {out}");
    }
    // Nothing else moved.
    assert_eq!(out.replace("/~alice/", "/"), css);
}

proptest! {
    /// Every URL the pass touched strips back to what the site wrote, and no
    /// URL is prefixed twice: the pass inserts the mount exactly once per URL.
    #[test]
    fn mounting_strips_back_to_the_site_urls(
        paths in proptest::collection::vec("/[a-z~]{1,6}(/[a-z0-9.]{1,6}){0,3}/?", 1..6),
        mount in "(/[a-z~]{1,5}){1,3}",
    ) {
        let mount = Mount::of_site_url(&format!("https://example.edu{mount}"));
        let html = |f: &dyn Fn(&str) -> String| -> String {
            paths.iter().map(|p| format!(r#"<a href="{0}">x</a><img src="{0}">"#, f(p))).collect()
        };
        let css = |f: &dyn Fn(&str) -> String| -> String {
            paths.iter().map(|p| format!("a{{background:url({})}}", f(p))).collect()
        };
        let site = |p: &str| p.to_string();
        let shipped = |p: &str| mount.apply(p).unwrap();
        prop_assert_eq!(mount.rewrite_html(&html(&site)).into_owned(), html(&shipped));
        prop_assert_eq!(mount.rewrite_css(&css(&site)).into_owned(), css(&shipped));
        for p in &paths {
            let mounted = mount.apply(p).unwrap();
            prop_assert_eq!(mount.strip(&mounted), Some(p.as_str()));
        }
    }
}

#[test]
fn moss_data_urls_are_mounted_and_page_identities_are_not() {
    let html = r#"<body data-path="/notes/"><figure data-tiles="/_moss/map.1/tiles.json" data-hydrate-url="/places/?article=/notes/&amp;embed=1"></figure><form data-page-key="/notes/"></form><img data-placeholder-src="/a.png"></body>"#;
    let out = alice().rewrite_html(html);
    assert!(out.contains(r#"data-path="/notes/""#), "{out}");
    assert!(out.contains(r#"data-page-key="/notes/""#), "{out}");
    assert!(out.contains(r#"data-tiles="/~alice/_moss/map.1/tiles.json""#), "{out}");
    // The query names a site path the places page looks up; only the leading path moves.
    assert!(out.contains(r#"data-hydrate-url="/~alice/places/?article=/notes/&amp;embed=1""#), "{out}");
    assert!(out.contains(r#"data-placeholder-src="/~alice/a.png""#), "{out}");
}

#[test]
fn a_form_lands_on_a_mounted_page() {
    let out = alice().rewrite_html(r#"<form data-next="/thanks/"><input type="hidden" name="_next" value="/thanks/"><input name="q" value="/not-a-url"></form>"#);
    assert!(out.contains(r#"data-next="/~alice/thanks/""#), "{out}");
    assert!(out.contains(r#"name="_next" value="/~alice/thanks/""#), "{out}");
    assert!(out.contains(r#"name="q" value="/not-a-url""#), "{out}");
}
