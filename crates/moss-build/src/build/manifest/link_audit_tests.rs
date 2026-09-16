use super::*;

fn satisfied<'a>(keys: &[&'a str]) -> HashSet<&'a str> {
    keys.iter().copied().collect()
}

/// `stage_dir` for the pure cases: a path that exists nowhere, so the disk
/// fallback can never accidentally satisfy a candidate.
fn no_stage() -> &'static Path {
    Path::new("/nonexistent-moss-link-audit-stage")
}

fn hrefs(page_html: &str, keys: &[&str]) -> Vec<String> {
    dead_links_in_page("p/index.html", page_html, &satisfied(keys), no_stage())
        .into_iter()
        .map(|d| d.href)
        .collect()
}

// ── candidate_keys: what is and is not this module's business ───────────────

#[test]
fn a_directory_url_also_names_its_index_html() {
    assert_eq!(
        candidate_keys("/authors/ling/"),
        Some(vec!["authors/ling/".into(), "authors/ling/index.html".into()])
    );
    assert_eq!(
        candidate_keys("/authors/ling"),
        Some(vec!["authors/ling".into(), "authors/ling/index.html".into()])
    );
    assert_eq!(candidate_keys("/"), Some(vec!["index.html".into()]));
}

#[test]
fn query_and_fragment_are_not_part_of_the_path() {
    assert_eq!(
        candidate_keys("/search/?q=moss#top"),
        Some(vec!["search/".into(), "search/index.html".into()])
    );
    assert_eq!(candidate_keys("/rss.xml"), Some(vec!["rss.xml".into(), "rss.xml/index.html".into()]));
}

#[test]
fn everything_that_is_not_root_relative_is_left_alone() {
    for href in [
        "https://example.com/a",
        "http://example.com/a",
        "//cdn.example.com/a.js",
        "mailto:someone@example.com",
        "data:image/png;base64,AAAA",
        "moss-source://asset/x.png",
        "#footnote-1",
        "sibling/page/",
        "../up/",
        "",
    ] {
        assert_eq!(candidate_keys(href), None, "must ignore {href}");
    }
}

#[test]
fn a_path_escaping_the_site_root_is_ignored_rather_than_reported() {
    assert_eq!(candidate_keys("/../../etc/passwd"), None);
    assert_eq!(candidate_keys("/a/./b/"), None);
}

/// The CJK case moss#1187 names: moss emits the same file's URL percent-encoded
/// from one code path and literal from another. Decoding the href is what makes
/// the encoded form find the literal manifest key.
#[test]
fn a_percent_encoded_cjk_slug_matches_the_literal_key() {
    assert_eq!(
        candidate_keys("/authors/%E9%BB%83%E5%B1%B1/"),
        Some(vec!["authors/黃山/".into(), "authors/黃山/index.html".into()])
    );
    assert!(hrefs(
        r#"<a href="/authors/%E9%BB%83%E5%B1%B1/">x</a>"#,
        &["authors/黃山/index.html"]
    )
    .is_empty());
}

/// Decoding the href is enough, and only enough because the manifest side is
/// never encoded: a key comes from a source path through `ServedPath`, which
/// lowercases directory segments and touches nothing else. Pinned here so a
/// future encoding step in `ServedPath` shows up as this test failing rather
/// than as an advisory that fires on every CJK link.
#[test]
fn a_manifest_key_keeps_a_cjk_slug_literal() {
    let sp = ServedPath::from_source("Authors/黃山/index.html").unwrap();
    assert_eq!(sp.as_str(), "authors/黃山/index.html");
}

// ── url_attr_values: the scan ──────────────────────────────────────────────

#[test]
fn href_and_src_are_read_and_lookalike_attributes_are_not() {
    let html = r#"<a href="/a/">a</a><img src='/b.png'><img data-src="/c.png">
        <use xlink:href="/d.svg"><img srcset="/e.png 2x"><link href="/f.css">"#;
    let mut got = url_attr_values(html);
    got.sort();
    assert_eq!(got, vec!["/a/", "/b.png", "/f.css"]);
}

#[test]
fn an_unterminated_attribute_ends_the_scan_without_hanging() {
    assert_eq!(url_attr_values(r#"<a href="/never-closed"#), Vec::<String>::new());
}

// ── the comparison ─────────────────────────────────────────────────────────

/// The moss#1187 shape itself: the namespace the links were written against no
/// longer exists, and the one that replaced it does.
#[test]
fn a_link_into_a_renamed_namespace_is_reported() {
    let html = r#"<a href="/author/ling/">Ling</a> <a href="/authors/ling/">Ling</a>"#;
    assert_eq!(hrefs(html, &["authors/ling/index.html"]), vec!["/author/ling/"]);
}

#[test]
fn a_page_repeating_one_broken_link_reports_it_once() {
    let html = r#"<a href="/author/ling/">1</a><a href="/author/ling/">2</a><a href="/author/ling/">3</a>"#;
    assert_eq!(hrefs(html, &[]), vec!["/author/ling/"]);
}

#[test]
fn an_emitted_asset_and_the_site_root_are_satisfied() {
    let html = r#"<a href="/">home</a><link href="/assets/style-ab12.css"><img src="/img/x.webp">"#;
    assert!(hrefs(html, &["index.html", "assets/style-ab12.css", "img/x.webp"]).is_empty());
}

/// A redirect stub is an ordinary emitted file, which is the whole reason this
/// module never reads `.moss/data/redirects.json`: a link to a renamed page's
/// old URL finds the stub in the manifest like any other page.
#[test]
fn a_link_to_a_redirect_stub_is_not_dead() {
    let html = r#"<a href="/old/slug/">still linked</a>"#;
    assert!(hrefs(html, &["old/slug/index.html", "new/slug/index.html"]).is_empty());
}

/// Passthrough: a directory moss copies rather than renders can reach the stage
/// without a manifest entry, and an advisory that cries wolf gets muted.
#[test]
fn a_file_present_in_the_stage_but_absent_from_the_manifest_is_not_dead() {
    let stage = std::env::temp_dir().join(format!("moss_link_audit_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(stage.join("app")).unwrap();
    std::fs::write(stage.join("app/index.html"), b"<html></html>").unwrap();

    let dead = dead_links_in_page(
        "p/index.html",
        r#"<a href="/app/">the passthrough SPA</a><a href="/nope/">gone</a>"#,
        &satisfied(&[]),
        &stage,
    );
    assert_eq!(dead.iter().map(|d| d.href.as_str()).collect::<Vec<_>>(), vec!["/nope/"]);

    let _ = std::fs::remove_dir_all(&stage);
}
