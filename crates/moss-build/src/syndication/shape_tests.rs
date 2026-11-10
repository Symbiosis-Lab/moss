use super::*;
use crate::syndication::channel::Counting;
use std::collections::HashMap;

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
fn pictures_are_left_out_of_the_text() {
    let p = post("", "Look.\n\n![a red maple](maple.jpg)\n\nThat was all.\n");
    let s = shape(&p, &spec(&[Form::Short], 300), SITE).unwrap();
    assert_eq!(s.parts[0].text, format!("Look.\n\nThat was all.\n\n{PAGE}"));
}

#[test]
fn a_post_written_in_parts_is_a_thread_where_the_network_takes_one() {
    let p = post("", "One.\n\n---\n\nTwo.\n\n---\n\nThree.\n");
    let thread = shape(&p, &spec(&[Form::Short, Form::Thread], 300), SITE).unwrap();
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
    assert_eq!(article.html, format!("<p>html</p>\n<p><a href=\"{PAGE}\">{PAGE}</a></p>"));
    assert_eq!(article.description.as_deref(), Some("About the season."));
    assert_eq!(article.cover.as_deref(), Some("https://example.org/covers/maple.jpg"), "a cover path is from the site's root, as the scan resolved it");

    let teaser = shape(&p, &spec(&[Form::Short, Form::Teaser], 300), SITE).unwrap();
    assert_eq!(teaser.parts[0].text, format!("Autumn\n\nAbout the season.\n\n{PAGE}"));
    assert_eq!(teaser.teaser.unwrap().description, "About the season.");

    let mut with_link = post("", "Out by the river: https://example.com/river and on.\n\n---\n\nMore.\n");
    with_link.title = "Walk".into();
    let t = shape(&with_link, &spec(&[Form::Teaser], 300), SITE).unwrap();
    assert_eq!(t.parts[0].links, ["https://example.com/river", PAGE], "an address in the teaser's own words is a link too");
}

#[test]
fn a_network_with_no_api_gets_the_same_copy_as_a_handoff() {
    let p = post("Autumn", "A long essay.\n");
    let s = shape(&p, &spec(&[Form::Handoff], 3000), SITE).unwrap();
    assert_eq!(s.form, Form::Handoff);
    assert!(s.parts[0].text.ends_with(PAGE));
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
            for form in [Form::Short, Form::Handoff] {
                let text = format!("秋 🍁\n\n{PAGE}");
                let mut channel = ChannelSpec { forms: vec![form, Form::Teaser], counting, link_chars, ..Default::default() };
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
    let expected = "<p>html</p>\n<p><a href=\"https://example.org/essay/?a=1&amp;b=&quot;two&quot;\">https://example.org/essay/?a=1&amp;b=&quot;two&quot;</a></p>";
    let copy = shape(&p, &spec(&[Form::Long, Form::Teaser], 10), SITE).unwrap();
    assert_eq!(copy.form, Form::Long);
    assert_eq!(copy.article.unwrap().html, expected);
}

#[test]
fn an_address_alone_must_fit_and_existing_backlinks_are_not_repeated() {
    for form in [Form::Short, Form::Teaser, Form::Handoff] {
        let channel = spec(&[form], PAGE.len() as u32);
        let copy = shape(&post("", ""), &channel, SITE).unwrap();
        assert_eq!(copy.parts[0].text, PAGE);
        assert_eq!(copy.parts[0].links, [PAGE]);
        assert!(shape(&post("", ""), &spec(&[form], PAGE.len() as u32 - 1), SITE).is_err());
    }
    let p = post("", &format!("[Original]({PAGE})"));
    let copy = shape(&p, &spec(&[Form::Short], 300), SITE).unwrap();
    assert_eq!(copy.parts[0].text.matches(PAGE).count(), 1);
    assert_eq!(copy.parts[0].links, [PAGE]);
    assert!(shape(&post("", "x"), &spec(&[Form::Short], PAGE.len() as u32), SITE).is_err());
}
