use super::*;
use crate::syndication::channel::Counting;
use std::collections::HashMap;

fn shape(post: &ArticleInfo, spec: &ChannelSpec, site: &str) -> Result<Shaped, String> {
    shape_with(post, spec, site, &ShapeOptions::default())
}

const SITE: &str = "https://example.org";
const PAGE: &str = "https://example.org/notes/2026-11-06-1432/";

fn post(title: &str, content: &str) -> ArticleInfo {
    ArticleInfo {
        source_path: "notes/2026-11-06 1432.md".into(),
        title: title.into(),
        content: content.into(),
        html_content: Some("<p>html</p>".into()),
        frontmatter: HashMap::new(),
        url_path: "notes/2026-11-06-1432/".into(),
        date: None,
        tags: vec![],
        uid: Some("ab12cd34".into()),
        syndicate: None,
        untitled: false,
    }
}

fn spec(forms: &[Form], max: u32) -> ChannelSpec {
    ChannelSpec { forms: forms.to_vec(), max_chars: Some(max), counting: Counting::Graphemes, ..Default::default() }
}

#[test]
fn a_short_post_is_its_own_text_with_links_spelled_out() {
    let p = post("", "Autumn's **first** hotpot, after [the walk](/notes/walk/) and https://example.com.\n");
    let s = shape(&p, &spec(&[Form::Short, Form::Teaser], 300), SITE).unwrap();
    assert_eq!(s.form, Form::Short);
    assert_eq!(s.parts[0].text, format!("Autumn's first hotpot, after the walk (https://example.org/notes/walk/) and https://example.com.\n\n{PAGE}"));
    assert_eq!(s.parts[0].links, ["https://example.org/notes/walk/", "https://example.com", PAGE]);
}

#[test]
fn a_wiki_link_keeps_its_words_without_brackets() {
    let p = post("", "After [[The walk]] and [[Hotpot|the hotpot]].\n");
    let s = shape(&p, &spec(&[Form::Short], 300), SITE).unwrap();
    assert_eq!(s.parts[0].text, format!("After The walk and the hotpot.\n\n{PAGE}"));
}

#[test]
fn pictures_are_left_out_of_the_text() {
    let p = post("", "Look.\n\n![a red maple](maple.jpg)\n\nThat was all.\n");
    let s = shape(&p, &spec(&[Form::Short], 300), SITE).unwrap();
    assert_eq!(s.parts[0].text, format!("Look.\n\nThat was all.\n\n{PAGE}"));
}

#[test]
fn a_post_written_in_parts_is_a_thread_where_the_network_takes_one() {
    let p = post("", "One.\n\n---\n\nTwo.\n\n---\n\nThree.\n");
    let thread = shape(&p, &spec(&[Form::Short, Form::Thread], (PAGE.len() + 10) as u32), SITE).unwrap();
    assert_eq!(thread.form, Form::Thread);
    assert_eq!(thread.parts.iter().map(|p| p.text.clone()).collect::<Vec<_>>(), ["One.".to_string(), "Two.".to_string(), format!("Three.\n\n{PAGE}")]);

    let joined = shape(&p, &spec(&[Form::Short], 300), SITE).unwrap();
    assert_eq!(joined.form, Form::Short);
    assert_eq!(joined.parts[0].text, format!("One.\n\nTwo.\n\nThree.\n\n{PAGE}"));
}

#[test]
fn a_rule_without_a_blank_line_above_is_not_a_break() {
    let p = post("", "Heading\n---\n\nbody\n");
    let s = shape(&p, &spec(&[Form::Short, Form::Thread], 300), SITE).unwrap();
    assert_eq!(s.parts.len(), 1);
}

#[test]
fn too_long_for_the_network_becomes_a_teaser_cut_at_a_sentence() {
    let body = "First sentence here. Second sentence is longer than the limit allows for sure.\n";
    let p = post("", body);
    let limit = (PAGE.chars().count() + 2 + "First sentence here.".len()) as u32;
    let s = shape(&p, &spec(&[Form::Short, Form::Teaser], limit), SITE).unwrap();
    assert_eq!(s.form, Form::Teaser);
    assert_eq!(s.parts[0].text, format!("First sentence here.\n\n{PAGE}"));
    assert_eq!(s.parts[0].links, [PAGE]);
    assert!(spec(&[], limit).fits(&s.parts[0].text));
}

#[test]
fn an_article_is_long_where_the_network_has_articles_and_a_teaser_elsewhere() {
    let mut p = post("Autumn", "A long essay.\n");
    p.frontmatter.insert("description".into(), serde_json::json!("About the season."));
    p.frontmatter.insert("cover".into(), serde_json::json!("covers/maple.jpg"));
    let long = shape(&p, &spec(&[Form::Long, Form::Teaser], 300), SITE).unwrap();
    assert_eq!(long.form, Form::Long);
    let article = long.article.unwrap();
    assert_eq!(article.html, format!("<p>html</p>\n<p>Originally published on <a href=\"{PAGE}\">example.org</a>.</p>"));
    assert_eq!(article.description.as_deref(), Some("About the season."));
    assert_eq!(article.cover.as_deref(), Some("https://example.org/covers/maple.jpg"), "a cover path is from the site's root, as the scan resolved it");

    let teaser = shape(&p, &spec(&[Form::Teaser], 300), SITE).unwrap();
    assert_eq!(teaser.parts[0].text, format!("Autumn\n\nAbout the season.\n\n{PAGE}"));
    assert_eq!(teaser.teaser.unwrap().description, "About the season.");

    let mut with_link = post("", "Out by the river: https://example.com/river and on.\n\n---\n\nMore.\n");
    with_link.title = "Walk".into();
    let t = shape(&with_link, &spec(&[Form::Teaser], 300), SITE).unwrap();
    assert_eq!(t.parts[0].links, ["https://example.com/river", PAGE], "an address in the teaser's own words is a link too");
}

#[test]
fn a_post_too_long_with_no_teaser_is_refused() {
    let p = post("", &"word ".repeat(100));
    assert!(shape(&p, &spec(&[Form::Short], 50), SITE).is_err());
}

#[test]
fn chinese_text_is_cut_at_a_full_stop() {
    let p = post("", "今天很冷。我们去吃火锅了，吃了很久很久很久很久。\n");
    let limit = (PAGE.chars().count() + 2 + 5) as u32;
    let s = shape(&p, &spec(&[Form::Teaser], limit), SITE).unwrap();
    assert_eq!(s.teaser.unwrap().description, "今天很冷。");
}

#[test]
fn the_hash_follows_the_copy_and_the_key_follows_only_the_post_and_channel() {
    let a = shape(&post("", "One.\n"), &spec(&[Form::Short], 300), SITE).unwrap();
    let b = shape(&post("", "One.\n\n"), &spec(&[Form::Short], 300), SITE).unwrap();
    let c = shape(&post("", "Two.\n"), &spec(&[Form::Short], 300), SITE).unwrap();
    assert_eq!(a.hash(), b.hash(), "a whitespace edit the copy does not show changes nothing");
    assert_ne!(a.hash(), c.hash());
    assert_eq!(idempotency_key("ab12cd34", "bluesky"), idempotency_key("ab12cd34", "bluesky"));
    assert_ne!(idempotency_key("ab12cd34", "bluesky"), idempotency_key("ab12cd34", "mastodon"));
}

#[test]
fn backlinks_fit_at_the_boundary_for_every_counting_rule() {
    for counting in [Counting::Graphemes, Counting::Chars, Counting::Utf16, Counting::Weighted] {
        for link_chars in [None, Some(23)] {
            for form in [Form::Short, Form::Teaser] {
                let text = format!("秋 🍁\n\n{PAGE}");
                let mut channel = ChannelSpec { forms: if form == Form::Teaser { vec![form] } else { vec![form, Form::Teaser] }, counting, link_chars, ..Default::default() };
                let limit = channel.length(&text) as u32;
                channel.max_chars = Some(limit);
                let p = post("", "秋 🍁");
                let exact = shape(&p, &channel, SITE).unwrap();
                assert_eq!(exact.form, form);
                assert_eq!(exact.parts[0].text, text);
                assert_eq!(exact.parts[0].links, [PAGE]);
                channel.max_chars = Some(limit - 1);
                let fallback = shape(&p, &channel, SITE).unwrap();
                assert!(fallback.teaser.is_some());
                assert!(channel.fits(&fallback.parts[0].text));
                assert!(fallback.parts[0].text.ends_with(PAGE));
                assert!(fallback.parts[0].links.iter().any(|link| link == PAGE));
            }
        }
    }
}

#[test]
fn a_threads_last_post_carries_the_address_and_it_falls_back_when_that_post_overflows() {
    let p = post("", "One.\n\n---\n\nLonger.");
    let mut channel = spec(&[Form::Thread, Form::Teaser], (PAGE.len() + 2 + "Longer.".len()) as u32);
    let thread = shape(&p, &channel, SITE).unwrap();
    assert_eq!(thread.form, Form::Thread);
    assert_eq!(thread.parts[0].text, "One.");
    assert!(thread.parts[0].links.is_empty());
    assert_eq!(thread.parts[1].text, format!("Longer.\n\n{PAGE}"));
    assert_eq!(thread.parts[1].links, [PAGE]);
    assert!(thread.parts.iter().all(|part| channel.fits(&part.text)));
    channel.max_chars = channel.max_chars.map(|n| n - 1);
    let fallback = shape(&p, &channel, SITE).unwrap();
    assert_eq!(fallback.form, Form::Teaser);
    assert!(fallback.parts[0].text.ends_with(PAGE));
    assert!(channel.fits(&fallback.parts[0].text));
    channel.forms = vec![Form::Thread];
    assert!(shape(&p, &channel, SITE).is_err());
}

#[test]
fn a_pages_address_is_percent_encoded_so_a_network_can_link_it() {
    assert_eq!(page_url(SITE, "notes/a b/"), "https://example.org/notes/a%20b/");
    assert_eq!(page_url(SITE, "notes/你好/"), "https://example.org/notes/%E4%BD%A0%E5%A5%BD/");
    assert_eq!(page_url(SITE, "/essay/?a=1&b=2"), "https://example.org/essay/?a=1&b=2");
}

#[test]
fn a_long_copy_ends_with_its_escaped_address_whatever_the_networks_short_limit() {
    let mut p = post("Essay", "Opening.");
    p.url_path = "essay/?a=1&b=\"two\"".into();
    let expected = "<p>html</p>\n<p>Originally published on <a href=\"https://example.org/essay/?a=1&amp;b=&quot;two&quot;\">example.org</a>.</p>";
    let copy = shape(&p, &spec(&[Form::Long, Form::Teaser], 10), SITE).unwrap();
    assert_eq!(copy.form, Form::Long);
    assert_eq!(copy.article.unwrap().html, expected);
}

#[test]
fn an_address_alone_must_fit_and_the_copy_ends_with_it() {
    for form in [Form::Short, Form::Teaser] {
        let channel = spec(&[form], PAGE.len() as u32);
        let copy = shape(&post("", ""), &channel, SITE).unwrap();
        assert_eq!(copy.parts[0].text, PAGE);
        assert_eq!(copy.parts[0].links, [PAGE]);
        assert!(shape(&post("", ""), &spec(&[form], PAGE.len() as u32 - 1), SITE).is_err());
    }
    let p = post("", &format!("[Original]({PAGE})"));
    let copy = shape(&p, &spec(&[Form::Short], 300), SITE).unwrap();
    assert!(copy.parts[0].text.ends_with(PAGE));
    assert_eq!(copy.parts[0].links, [PAGE]);
    assert!(shape(&post("", "x"), &spec(&[Form::Short], PAGE.len() as u32), SITE).is_err());
}

#[test]
fn forms_follow_content_in_order_and_include_the_authored_title() {
    let all = spec(&[Form::Long, Form::Thread, Form::Teaser, Form::Short], 300);
    let mut p = post("Title", "A short page.");
    let copy = shape(&p, &all, SITE).unwrap();
    assert_eq!(copy.form, Form::Short);
    assert_eq!(copy.parts[0].text, format!("Title\nA short page.\n\n{PAGE}"));
    p.content = "word ".repeat(100);
    assert_eq!(shape(&p, &all, SITE).unwrap().form, Form::Long);
    p.untitled = true;
    p.title = "Opening words".into();
    assert_eq!(shape(&p, &all, SITE).unwrap().article.unwrap().title, "Opening words");
    p.content = "Small".into();
    assert_eq!(shape(&p, &spec(&[Form::Long], 300), SITE).unwrap().form, Form::Long);
    assert!(!shape(&p, &all, SITE).unwrap().parts[0].text.contains("Opening words"));
}

#[test]
fn code_blocks_never_make_thread_parts() {
    for body in ["```\none\n\n---\n\ntwo\n```", "~~~\none\n\n---\n\ntwo\n~~~", "    one\n\n    ---\n\n    two"] {
        assert_eq!(split_parts(body).len(), 1, "{body}");
        assert!(shape(&post("", body), &spec(&[Form::Thread], 300), SITE).is_err());
    }
    assert_eq!(split_parts("one\n\n---\n\ntwo").len(), 2);
    assert_eq!(split_parts("one\r\n\r\n---\r\n\r\ntwo").len(), 2);
    assert_eq!(split_parts("one\n  \n---\n  \ntwo").len(), 2);
}

#[test]
fn footer_settings_language_and_escaping_are_shared() {
    let mut p = post("Essay", "Body");
    let mut opts = ShapeOptions { site_name: Some("A & <B>".into()), lang: Some("zh-Hans".into()), ..Default::default() };
    let long = spec(&[Form::Long], 300);
    let html = shape_with(&p, &long, SITE, &opts).unwrap().article.unwrap().html;
    assert!(html.ends_with(&format!("<p>本文首发于 <a href=\"{PAGE}\">A &amp; &lt;B&gt;</a>。</p>")), "{html}");
    p.frontmatter.insert("lang".into(), serde_json::json!("en"));
    assert!(shape_with(&p, &long, SITE, &opts).unwrap().article.unwrap().html.contains("Originally published on"));
    opts.link_back = false;
    assert_eq!(shape_with(&p, &long, SITE, &opts).unwrap().article.unwrap().html, "<p>html</p>");
    let copy = shape_with(&p, &spec(&[Form::Short], 300), SITE, &opts).unwrap();
    assert_eq!(copy.parts[0].text, "Essay\nBody");
    assert!(shape_with(&p, &spec(&[Form::Teaser], 300), SITE, &opts).unwrap().parts[0].text.ends_with(PAGE));
}

#[test]
fn settings_override_globally_and_per_channel() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".moss")).unwrap();
    std::fs::write(dir.path().join(".moss/config.toml"), "[site]\ntitle = 'Site'\n[syndication]\nlink_back = false\n[channels.one]\nlink_back = true\n[channels.two]\nlink_back = false\n").unwrap();
    let config = crate::build::site_config::read_project_config(&dir.path().to_string_lossy()).unwrap();
    let p = post("", "Body");
    let mut map = crate::build::scan::article_map::ArticleMap::default();
    map.page_site_names.insert(p.url_path.clone(), "Rendered site".into());
    let options = |channel| ShapeOptions::new(&config, dir.path(), &map, &p, channel, None);
    assert!(options("one").link_back);
    assert_eq!(options("one").site_name.as_deref(), Some("Rendered site"));
    assert!(!options("two").link_back);
    assert!(!options("three").link_back);
    assert!(config.unknown_site_keys()[0].contains("home page"));

}

#[test]
fn edits_keep_form_and_thread_count_or_retain_the_prior_version() {
    use crate::syndication::record::{CopyRecord, RemoteRef};
    let mut record = CopyRecord::due("u", "c", "page.md", "page/");
    record.remote = Some(RemoteRef { id: "1".into(), url: "https://example.social/1".into() });
    record.form = Some(Form::Short);
    record.part_count = Some(1);
    let channel = spec(&[Form::Short, Form::Thread, Form::Long, Form::Teaser], 100);
    let opts = ShapeOptions::default();
    assert!(shape_record(&post("", &"word ".repeat(40)), &channel, SITE, &opts, &record).unwrap_err().contains("retaining"));
    record.form = Some(Form::Long);
    assert_eq!(shape_record(&post("", "tiny"), &channel, SITE, &opts, &record).unwrap().form, Form::Long);
    record.form = Some(Form::Thread);
    record.parts = vec![RemoteRef { id: "2".into(), url: "https://example.social/2".into() }];
    assert_eq!(shape_record(&post("", "one\n\n---\n\ntwo"), &channel, SITE, &opts, &record).unwrap().form, Form::Thread);
    assert!(shape_record(&post("", "one\n\n---\n\ntwo\n\n---\n\nthree"), &channel, SITE, &opts, &record).unwrap_err().contains("number of parts"));
    record.form = None;
    assert!(shape_record(&post("", "tiny"), &channel, SITE, &opts, &record).unwrap_err().contains("not recorded"));
}

#[test]
fn copies_reuse_emitted_math_pngs_and_keep_picture_alt_and_limits() {
    use crate::build::emit::math_png::{email_math_img, emit_math_pngs};
    let dir = tempfile::tempdir().unwrap();
    let mut doc = crate::build::types::ParsedDocument::default();
    doc.content = "A $x$ and $$y$$".into();
    let mut pending = crate::build::manifest::PendingManifest::new(Default::default());
    emit_math_pngs(&[doc], true, dir.path(), &mut pending).unwrap();
    let mut p = post("", "A $x$ and $$y$$");
    p.html_content = Some(format!("<p>{}</p>{}<img src=\"/photo.jpg\" alt=\"A &amp; B\">", crate::build::markdown::math::render_math("x", false).unwrap(), crate::build::markdown::math::render_math("y", true).unwrap()));
    let opts = ShapeOptions { output: Some(dir.path().into()), ..Default::default() };
    let mut channel = spec(&[Form::Short], 300);
    channel.images = Some(super::super::channel::ImageLimits { max: 3, alt: true });
    let copy = shape_with(&p, &channel, SITE, &opts).unwrap();
    assert!(copy.parts[0].text.contains("$x$") && copy.parts[0].text.contains("$$y$$"));
    assert_eq!(copy.pictures.len(), 3);
    assert_eq!(copy.pictures[0].url, page_url(SITE, email_math_img("x", false).unwrap().served_path().as_str()));
    assert_eq!(copy.pictures[0].alt, "$x$");
    assert_eq!(copy.pictures[1].alt, "$$y$$");
    assert_eq!(copy.pictures[2].alt, "A & B");
    channel.forms = vec![Form::Long];
    let html = shape_with(&p, &channel, SITE, &opts).unwrap().article.unwrap().html;
    assert!(!html.contains("<svg"));
    assert!(html.contains("https://example.org/_moss/math/"));
    channel.images.as_mut().unwrap().max = 1;
    let limited = shape_with(&p, &channel, SITE, &opts).unwrap();
    assert!(limited.pictures.is_empty(), "long images belong only to HTML");
    assert_eq!(limited.included_pictures, 1);
    assert_eq!(limited.omitted_pictures, 2);
    assert_eq!(limited.article.unwrap().html.matches("<img").count(), 1);
    channel.images = None;
    let unlimited = shape_with(&p, &channel, SITE, &opts).unwrap();
    assert_eq!(unlimited.article.unwrap().html.matches("<img").count(), 3, "an article keeps its pictures on a channel that states no limit");
    assert_eq!((unlimited.included_pictures, unlimited.omitted_pictures), (3, 0));
    channel.images = Some(super::super::channel::ImageLimits { max: 1, alt: true });
    let missing = shape_with(&p, &channel, SITE, &ShapeOptions::default()).unwrap();
    assert!(!missing.article.unwrap().html.contains("/_moss/math/"));
    assert!(missing.pictures.iter().all(|p| !p.url.contains("/_moss/math/")));
}

#[test]
fn math_disabled_never_advertises_formula_assets_even_if_retained_on_disk() {
    use crate::build::emit::math_png::emit_math_pngs;
    let dir = tempfile::tempdir().unwrap();
    let doc = crate::build::types::ParsedDocument { content: "$x$".into(), ..Default::default() };
    let mut pending = crate::build::manifest::PendingManifest::new(Default::default());
    emit_math_pngs(&[doc], true, dir.path(), &mut pending).unwrap();
    let opts = ShapeOptions { output: Some(dir.path().into()), math_enabled: false, ..Default::default() };
    let mut channel = spec(&[Form::Short], 300);
    channel.images = Some(super::super::channel::ImageLimits { max: 4, alt: true });
    let mut p = post("", "$x$");
    p.html_content = Some(crate::build::markdown::math::render_math("x", false).unwrap());
    let copy = shape_with(&p, &channel, SITE, &opts).unwrap();
    assert!(copy.pictures.is_empty());
    assert!(copy.parts[0].text.contains("$x$"));
}

#[test]
fn regular_pictures_keep_alt_text_and_respect_the_declared_limit() {
    let mut p = post("", "![A **red** maple](/maple.jpg)\n\n![Second](two.jpg)");
    p.html_content = Some("<img src=\"/maple.jpg\" alt=\"A red maple\"><img src=\"two.jpg\" alt=\"Second\">".into());
    let mut channel = spec(&[Form::Short], 300);
    channel.images = Some(super::super::channel::ImageLimits { max: 1, alt: false });
    let copy = shape(&p, &channel, SITE).unwrap();
    assert_eq!(copy.pictures.len(), 1);
    assert_eq!(copy.pictures[0].url, "https://example.org/maple.jpg");
    assert_eq!(copy.pictures[0].alt, "A red maple");
    assert_eq!(copy.omitted_pictures, 1);
    assert!(!copy.parts[0].text.contains("maple"));
}


#[test]
fn posted_threads_need_complete_remote_parts_but_attempts_keep_their_count() {
    use crate::syndication::record::{CopyRecord, RemoteRef, Records};
    let dir = tempfile::tempdir().unwrap();
    let records = Records::at(dir.path().into());
    let mut record = CopyRecord::due("u", "c", "page.md", "page/");
    record.form = Some(Form::Thread);
    record.part_count = Some(2);
    let p = post("", "one\n\n---\n\ntwo");
    let channel = spec(&[Form::Thread], 300);
    assert!(shape_record(&p, &channel, SITE, &ShapeOptions::default(), &record).is_ok());
    record.remote = Some(RemoteRef { id: "1".into(), url: "https://example.social/1".into() });
    assert!(shape_record(&p, &channel, SITE, &ShapeOptions::default(), &record).is_err());
    record.parts.push(RemoteRef { id: "2".into(), url: "https://example.social/2".into() });
    records.save(&record).unwrap();
    let saved = records.get("u", "c").unwrap();
    assert_eq!(saved.part_count, None);
    assert!(shape_record(&p, &channel, SITE, &ShapeOptions::default(), &saved).is_ok());
    let mut old = serde_json::to_value(&saved).unwrap();
    old["prior_post"] = serde_json::json!({"obsolete": true});
    assert!(serde_json::from_value::<CopyRecord>(old).is_ok());
}

#[test]
fn published_language_uses_the_builds_pretty_key_and_preserves_precedence() {
    use crate::build::scan::article_map::{build_article_map, to_pretty_url};
    use crate::syndication::record::{effective_language, PublishedPost};
    for path in ["/zh-hans/a/index.html", "zh-hans/a.html", "zh-hans/a/", "/a/index.html"] {
        let doc = crate::build::types::ParsedDocument { url_path: path.into(), source_path: Some("a.md".into()), title: "A".into(), content: "Body".into(), ..Default::default() };
        let editions = moss_core::home::Editions::new("en", ["zh-hans/index.html"]);
        let mut map = build_article_map(&[doc], &[], &HashMap::new(), &[], &[], &Default::default(), &editions);
        let key = to_pretty_url(path);
        map.page_languages.insert(key.clone(), "zh-Hans".into());
        let mut article = map.articles[&key].clone();
        assert_eq!(PublishedPost::new(&article, &map).lang.as_deref(), Some("zh-Hans"));
        article.frontmatter.insert("lang".into(), serde_json::json!("en"));
        assert_eq!(effective_language(&article, Some(&map), Some("fr")).as_deref(), Some("en"));
        article.frontmatter.remove("lang");
        map.page_languages.clear();
        assert_eq!(effective_language(&article, Some(&map), Some("fr")).as_deref(), Some(if key.starts_with("zh-hans/") { "zh-hans" } else { "fr" }));
    }
}

#[test]
fn an_unknown_publish_address_defers_the_footer_length_check() {
    let p = post("", "tiny");
    let channel = spec(&[Form::Short], 4);
    let record = crate::syndication::record::CopyRecord::due("u", "c", "page.md", "page/");
    let provisional = assess(&p, &channel, None, &ShapeOptions::default(), &record).unwrap();
    assert_eq!(provisional.parts[0].text, "tiny");
    assert!(assess(&p, &channel, Some(SITE), &ShapeOptions::default(), &record).is_err());
}

#[test]
fn provisional_teasers_preserve_known_limits_and_form_order() {
    let mut p = post("A title much longer than allowed", "Long body. ");
    p.url_path = "a/very/long/page/path/whose/final/address/is/not/known/".into();
    let record = crate::syndication::record::CopyRecord::due("u", "c", "page.md", &p.url_path);
    let channel = spec(&[Form::Short, Form::Teaser], 20);
    assert!(assess(&p, &channel, None, &ShapeOptions::default(), &record).is_err(), "an unknown URL cannot exempt the title");
    p.title = "Title".into();
    p.content = "First. More words that cannot all fit. ".repeat(40);
    let teaser = assess(&p, &channel, None, &ShapeOptions::default(), &record).unwrap();
    assert_eq!(teaser.form, Form::Teaser, "the whole body still exceeds the short limit");
    let card = teaser.teaser.unwrap();
    assert_eq!(card.description, "First.");
    assert!(channel.fits(&format!("{}\n\n{}", card.title, card.description)));
    assert_eq!(card.url, page_url("", &p.url_path));
    assert!(teaser.parts[0].text.ends_with(&card.url), "a provisional teaser still carries its link");
    assert!(assess(&p, &channel, Some(SITE), &ShapeOptions::default(), &record).is_err());
    assert!(assess(&p, &spec(&[Form::Short], 20), None, &ShapeOptions::default(), &record).is_err());
}

#[test]
fn pictures_use_only_the_rendered_addresses_and_absent_html_has_none() {
    let mut p = post("", "![Authored](/source.jpg)");
    let mut channel = spec(&[Form::Short], 300);
    channel.images = Some(super::super::channel::ImageLimits { max: 4, alt: true });
    p.html_content = Some("<img src=\"/_moss/media/rendered.webp\" alt=\"Rendered alt\">".into());
    let rendered = shape(&p, &channel, SITE).unwrap();
    assert_eq!(rendered.pictures[0].url, "https://example.org/_moss/media/rendered.webp");
    assert_eq!(rendered.pictures[0].alt, "Rendered alt");
    p.html_content = None;
    assert!(shape(&p, &channel, SITE).unwrap().pictures.is_empty());
}

#[test]
fn old_maps_without_page_names_use_the_hostname() {
    let map: crate::build::scan::article_map::ArticleMap = serde_json::from_value(serde_json::json!({"articles": {}, "site_names": {"": "Stale default"}})).unwrap();
    let config = crate::config::ConfigFile::parse("").unwrap();
    let p = post("", "Body");
    let options = ShapeOptions::new(&config, std::path::Path::new("."), &map, &p, "one", None);
    assert!(options.site_name.is_none());
    let html = shape_with(&p, &spec(&[Form::Long], 300), SITE, &options).unwrap().article.unwrap().html;
    assert!(html.contains("example.org</a>"));
    assert!(!html.contains("Stale default"));
}
