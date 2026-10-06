use super::*;
use crate::vault::import::scrape::converter::extract_article;

const PAGE: &str = "https://site.example/about/";

fn page(footer: &str, main: &str) -> String {
    format!(
        r#"<!doctype html><html lang="en"><head><title>About us</title></head><body>
<header><a href="/">Home</a></header>
<main><article><h1>About us</h1>
<p>We run a small studio that makes books and prints, and this paragraph is long enough to be read as the body of the page rather than as a caption.</p>
{main}
<p>Closing paragraph with enough words to keep the article the best scoring container on the page for the scorer.</p>
</article></main>
<footer>{footer}</footer></body></html>"#
    )
}

const CONTACT_FORM: &str = r#"<form action="/send" method="post"><input type="text" name="name"><input type="email" name="email"><textarea name="message"></textarea><button>Send</button></form>"#;

fn run(html: &str) -> (String, WidgetCount) {
    let art = extract_article(html, PAGE);
    (art.markdown, art.widgets)
}

#[test]
fn video_iframe_becomes_the_remote_embed_form() {
    let html = page("", r#"<iframe src="https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ" title="Talk"></iframe>"#);
    let (md, count) = run(&html);
    assert!(md.contains("![[https://www.youtube.com/watch?v=dQw4w9WgXcQ]]"), "{md}");
    assert_eq!(count, WidgetCount::default());
}

#[test]
fn vimeo_player_becomes_the_vimeo_page_url() {
    let html = page("", r#"<iframe src="https://player.vimeo.com/video/123456789"></iframe>"#);
    assert!(run(&html).0.contains("![[https://vimeo.com/123456789]]"));
}

#[test]
fn other_video_hosts_keep_their_player_url() {
    let html = page("", r#"<iframe src="https://player.bilibili.com/player.html?bvid=BV1xx"></iframe>"#);
    assert!(run(&html).0.contains("![[https://player.bilibili.com/player.html?bvid=BV1xx]]"));
}

#[test]
fn hosted_page_iframe_becomes_a_titled_link() {
    let html = page("", r#"<iframe src="https://pay.example/donate/123" title="Give to the studio"></iframe>"#);
    let (md, count) = run(&html);
    assert!(md.contains("[Give to the studio](https://pay.example/donate/123)"), "{md}");
    assert_eq!(count, WidgetCount { carried: 1, dropped: 0 });
}

#[test]
fn untitled_iframe_is_labelled_by_its_row_then_its_host() {
    let html = page("", r#"<iframe src="https://donorbox.org/embed/x"></iframe><iframe src="https://forms.example/f/1"></iframe>"#);
    let (md, count) = run(&html);
    assert!(md.contains("[Donate](https://donorbox.org/embed/x)"), "{md}");
    assert!(md.contains("[forms.example](https://forms.example/f/1)"), "{md}");
    assert_eq!(count.carried, 2);
}

#[test]
fn contact_form_becomes_the_footer_mailto() {
    let html = page(r#"<a href="mailto:hello@studio.example">Write to us</a>"#, CONTACT_FORM);
    let (md, count) = run(&html);
    assert!(md.contains("[hello@studio.example](mailto:hello@studio.example)"), "{md}");
    assert!(!md.contains("<form"));
    assert_eq!(count, WidgetCount { carried: 1, dropped: 0 });
}

#[test]
fn contact_form_beside_a_body_mailto_adds_no_second_link() {
    let main = format!(r#"<p>Mail <a href="mailto:hi@studio.example">hi@studio.example</a> any time.</p>{CONTACT_FORM}"#);
    let (md, count) = run(&page("", &main));
    assert_eq!(md.matches("mailto:").count(), 1, "{md}");
    assert_eq!(count, WidgetCount::default());
}

#[test]
fn contact_form_falls_back_to_the_json_ld_email() {
    let html = page("", CONTACT_FORM).replace(
        "</head>",
        r#"<script type="application/ld+json">{"@type":"Organization","email":"desk@studio.example"}</script></head>"#,
    );
    assert!(run(&html).0.contains("(mailto:desk@studio.example)"));
}

#[test]
fn contact_form_with_no_route_is_dropped_and_counted() {
    let (md, count) = run(&page("", CONTACT_FORM));
    assert!(!md.contains("<form") && !md.contains("Send") && !md.contains("mailto"), "{md}");
    assert_eq!(count, WidgetCount { carried: 0, dropped: 1 });
}

#[test]
fn hosted_form_action_becomes_a_link() {
    let main = r#"<form action="https://forms.tally.so/r/abc" title="Apply"><input name="a"><textarea></textarea></form>"#;
    let (md, count) = run(&page("", main));
    assert!(md.contains("[Apply](https://forms.tally.so/r/abc)"), "{md}");
    assert_eq!(count.carried, 1);
}

#[test]
fn widgets_in_chrome_regions_count_nothing() {
    let footer = r#"<iframe src="https://pay.example/donate/123"></iframe>"#;
    let (md, count) = run(&page(footer, ""));
    assert!(!md.contains("pay.example"));
    assert_eq!(count, WidgetCount::default());
}

#[test]
fn chat_iframes_and_newsletter_popups_vanish_uncounted() {
    let main = r#"<iframe src="https://widget.intercom.io/chat"></iframe>
<div class="newsletter-popup"><form action="https://x.list-manage.com/subscribe/post"><input type="email" name="EMAIL"></form></div>
<form action="https://x.list-manage.com/subscribe/post"><input type="email" name="EMAIL"><input name="a"><textarea></textarea></form>
<form><input type="email" name="email"><button>Join</button></form>"#;
    let (md, count) = run(&page("", main));
    assert!(!md.contains("intercom") && !md.contains("list-manage") && !md.contains("Join"), "{md}");
    assert_eq!(count, WidgetCount::default());
}

#[test]
fn page_without_widgets_is_returned_untouched() {
    let html = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/scrape/paywalled-news-article.html"
    ))
    .unwrap();
    let base = Url::parse(PAGE).unwrap();
    let (out, count) = carry_widgets(&html, &base);
    assert_eq!(out, html);
    assert_eq!(count, WidgetCount::default());
}

#[test]
fn table_rows_use_only_reachable_marker_role_href_combinations() {
    for r in ROWS {
        assert!(!r.pattern.is_empty() && !r.pattern.contains(char::is_whitespace), "{}", r.pattern);
        let ok = match (r.marker, r.role, r.href) {
            (EmbedHost, Embed, Src) => true,
            (IframeSrcHost, Carry | Chrome, Src) => true,
            (FormActionHost, Carry | Chrome, Action) => true,
            (AttrToken, Carry, Attr(_)) => true,
            _ => false,
        };
        assert!(ok, "row {} uses a combination no code path reads", r.pattern);
        if r.marker != AttrToken {
            assert_eq!(r.pattern, r.pattern.to_ascii_lowercase());
            assert!(!r.pattern.contains(['/', ':']), "host rows are bare hosts: {}", r.pattern);
        }
        if r.role == Carry {
            assert!(!r.label.is_empty(), "a carried widget needs a fallback label: {}", r.pattern);
        }
    }
}

#[test]
fn iframes_with_nothing_to_carry_count_nothing() {
    let main = r#"<iframe src="about:blank"></iframe><iframe src="javascript:void(0)"></iframe><iframe src="data:text/html,x"></iframe><iframe hidden src="https://pay.example/x"></iframe><iframe width="0" height="0" src="https://track.example/p"></iframe>"#;
    let (md, count) = run(&page("", main));
    assert!(!md.contains("pay.example") && !md.contains("track.example"), "{md}");
    assert_eq!(count, WidgetCount::default());
}

#[test]
fn a_relative_iframe_on_a_page_without_a_url_is_not_counted() {
    let html = page("", r#"<iframe src="/embed/form"></iframe>"#);
    let art = crate::vault::import::scrape::converter::extract_article(&html, "");
    assert_eq!(art.widgets, WidgetCount::default());
}

#[test]
fn uppercase_tags_still_reach_the_pre_pass() {
    let html = page("", r#"<IFRAME SRC="https://pay.example/donate/123" TITLE="Give"></IFRAME>"#);
    assert!(run(&html).0.contains("[Give](https://pay.example/donate/123)"));
}

#[test]
fn host_markers_match_by_suffix_only_on_a_label_boundary() {
    assert!(host_matches("player.vimeo.com", "vimeo.com"));
    assert!(host_matches("vimeo.com", "vimeo.com"));
    assert!(!host_matches("notvimeo.com", "vimeo.com"));
}
