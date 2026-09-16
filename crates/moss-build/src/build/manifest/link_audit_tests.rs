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

/// `poster=` joined `href=`/`src=` 2026-09-16 so a video's still-frame gets
/// the same dead-link coverage as its `src` — see the module docs.
#[test]
fn poster_is_read_like_href_and_src() {
    let html = r#"<video src="/videos/clip.mp4" poster="/videos/clip.thumb.jpg" data-thumb-src="/videos/clip.thumb.jpg"></video>"#;
    let mut got = url_attr_values(html);
    got.sort();
    assert_eq!(got, vec!["/videos/clip.mp4", "/videos/clip.thumb.jpg"]);
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

// ── dead_links_among_promises: the publish-refusing subset ──────────────────

fn promised(keys: &[&str]) -> HashSet<String> {
    keys.iter().map(|k| k.to_string()).collect()
}

/// The moss#1187-adjacent race this exists for: a video's page seals before
/// its encode lands, so the mp4 and the poster both read as dead links, and
/// both are keys this build itself promised (`AssetRegistry::pending_keys()`
/// for the mp4, its derived poster key for the other).
#[test]
fn a_video_and_its_poster_still_mid_encode_are_both_promised() {
    let dead = dead_links_in_page(
        "clip/index.html",
        r#"<video src="/videos/clip.mp4" poster="/videos/clip.thumb.jpg"></video>"#,
        &satisfied(&[]),
        no_stage(),
    );
    assert_eq!(dead.len(), 2);

    let promises = promised(&["videos/clip.mp4", "videos/clip.thumb.jpg"]);
    let gated = dead_links_among_promises(&dead, &promises);
    assert_eq!(gated.len(), 2, "both the mp4 and its poster must gate publish");
}

/// An external URL never even reaches `dead_links_in_page` (it is not a
/// root-relative candidate at all), so it cannot gate a publish either —
/// pinned here at the layer the publish gate actually reads.
#[test]
fn an_external_url_is_not_promised_because_it_was_never_a_candidate() {
    let dead = dead_links_in_page(
        "clip/index.html",
        r#"<video poster="https://cdn.example.com/clip.thumb.jpg"></video>"#,
        &satisfied(&[]),
        no_stage(),
    );
    assert!(dead.is_empty(), "an external URL must not be scanned as a candidate");
}

/// A deliberately unbuilt draft: the page it links to was never sealed and
/// was never promised by any in-flight encode either. Advisory, not a
/// refusal — the author left it out on purpose.
#[test]
fn a_link_to_an_unbuilt_draft_is_not_promised() {
    let dead = dead_links_in_page(
        "clip/index.html",
        r#"<a href="/drafts/unfinished/">still a draft</a>"#,
        &satisfied(&[]),
        no_stage(),
    );
    assert_eq!(dead.len(), 1);
    let promises = promised(&["videos/clip.mp4"]);
    assert!(dead_links_among_promises(&dead, &promises).is_empty());
}

/// An optional variant the site never dispatched (no `set_pending` ever ran
/// for it) is not this build's promise, however dead the link reads.
#[test]
fn an_optional_variant_never_dispatched_is_not_promised() {
    let dead = dead_links_in_page(
        "clip/index.html",
        r#"<source src="/img/hero.avif">"#,
        &satisfied(&[]),
        no_stage(),
    );
    assert_eq!(dead.len(), 1);
    let promises = promised(&["videos/clip.mp4"]);
    assert!(dead_links_among_promises(&dead, &promises).is_empty());
}

/// A permanently failed encode must never gate publish forever — there is no
/// override, so a stuck refusal would have no way out. The caller is
/// responsible for this by construction: a failed key drops out of
/// `AssetRegistry::pending_keys()` (see `assets.rs`'s own test), so it never
/// reaches `promised` in the first place. Pinned here at the filter itself so
/// the property holds even if a caller passes a stale promise set by mistake
/// — i.e. NOT promised is the only thing that keeps this safe, and that is
/// exactly what this test fixes in place.
#[test]
fn a_key_absent_from_promised_never_gates_however_dead_the_link() {
    let dead = dead_links_in_page(
        "clip/index.html",
        r#"<video src="/videos/clip.mp4" poster="/videos/clip.thumb.jpg"></video>"#,
        &satisfied(&[]),
        no_stage(),
    );
    assert_eq!(dead.len(), 2);
    // The failed video's keys are simply not in the promised set — exactly
    // what a caller sees after `set_failed` has run.
    assert!(dead_links_among_promises(&dead, &promised(&[])).is_empty());
}
