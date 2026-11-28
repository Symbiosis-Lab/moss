use super::*;
use crate::build::site_url::SiteUrl;
use crate::build::page::meta::OgCover;
use crate::i18n::Language;

fn test_site_url() -> SiteUrl {
    SiteUrl::parse("https://example.com").unwrap()
}

#[test]
fn test_build_og_tags_basic() {
    let site_url = test_site_url();
    let result = build_og_tags(
        "Test Article",
        "A test description",
        "https://example.com/posts/test/",
        "My Site",
        Some("2024-01-15"),
        None,
        &[],
        "en_US",
        &site_url,
    );

    assert!(result.contains(r#"og:type" content="article"#));
    assert!(result.contains(r#"og:title" content="Test Article"#));
    assert!(result.contains(r#"og:url" content="https://example.com/posts/test/"#));
    assert!(result.contains(r#"og:site_name" content="My Site"#));
    assert!(result.contains(r#"og:description" content="A test description"#));
    assert!(result.contains(r#"article:published_time" content="2024-01-15"#));
}

#[test]
fn test_build_og_tags_with_image_and_tags() {
    let site_url = test_site_url();
    let cover = CoverRef::External("https://example.com/image.jpg".to_string());
    let result = build_og_tags(
        "Test",
        "Desc",
        "https://example.com/",
        "Site",
        None,
        Some(&OgCover { cover: cover.clone(), dims: None, alt: None }),
        &["rust".to_string(), "programming".to_string()],
        "en_US",
        &site_url,
    );

    assert!(result.contains(r#"og:image" content="https://example.com/image.jpg"#));
    assert!(result.contains(r#"article:tag" content="rust"#));
    assert!(result.contains(r#"article:tag" content="programming"#));
}

#[test]
fn test_build_og_tags_escapes_special_chars() {
    let site_url = test_site_url();
    let result = build_og_tags(
        "Title with \"quotes\" & <tags>",
        "Description",
        "https://example.com/",
        "Site",
        None,
        None,
        &[],
        "en_US",
        &site_url,
    );

    assert!(result.contains("&quot;quotes&quot;"));
    assert!(result.contains("&amp;"));
    assert!(result.contains("&lt;tags&gt;"));
}

#[test]
fn test_build_og_tags_emits_locale() {
    let site_url = test_site_url();
    let result = build_og_tags(
        "Title",
        "Desc",
        "https://example.com/",
        "Site",
        None,
        None,
        &[],
        "en_US",
        &site_url,
    );
    assert!(
        result.contains(r#"<meta property="og:locale" content="en_US">"#),
        "expected og:locale tag, got: {result}"
    );
}

#[test]
fn test_build_og_tags_skips_locale_when_empty() {
    let site_url = test_site_url();
    let result = build_og_tags(
        "Title",
        "Desc",
        "https://example.com/",
        "Site",
        None,
        None,
        &[],
        "",
        &site_url,
    );
    assert!(
        !result.contains("og:locale"),
        "should not emit og:locale when locale is empty"
    );
}

#[test]
fn test_build_og_tags_website_skips_locale_when_empty() {
    let site_url = test_site_url();
    let result = build_og_tags_website(
        "Title",
        "Test Site",
        "Desc",
        "https://example.com/",
        None,
        "",
        &site_url,
    );
    assert!(
        !result.contains("og:locale"),
        "should not emit og:locale when locale is empty"
    );
}

#[test]
fn test_build_og_tags_emits_image_dimensions_when_image_present() {
    let site_url = test_site_url();
    let cover = CoverRef::External("https://example.com/card.png".to_string());
    let result = build_og_tags(
        "Title",
        "Desc",
        "https://example.com/",
        "Site",
        None,
        Some(&OgCover { cover: cover.clone(), dims: Some((1200, 630)), alt: None }),
        &[],
        "en_US",
        &site_url,
    );
    assert!(result.contains(r#"og:image" content="https://example.com/card.png"#));
    assert!(result.contains(r#"<meta property="og:image:width" content="1200">"#));
    assert!(result.contains(r#"<meta property="og:image:height" content="630">"#));
    assert!(result.contains(r#"<meta property="og:image:alt" content="Title">"#));
}

#[test]
fn test_build_twitter_tags_uses_summary_large_image() {
    let site_url = test_site_url();
    let cover = CoverRef::External("https://example.com/card.png".to_string());
    let result = build_twitter_tags("Title", "Desc", Some(&cover), Some("alt text"), &site_url);
    assert!(result.contains(r#"<meta name="twitter:card" content="summary_large_image">"#));
    assert!(result.contains(r#"<meta name="twitter:title" content="Title">"#));
    assert!(result.contains(r#"<meta name="twitter:description" content="Desc">"#));
    assert!(
        result.contains(r#"<meta name="twitter:image" content="https://example.com/card.png">"#)
    );
    assert!(result.contains(r#"<meta name="twitter:image:alt" content="alt text">"#));
}

#[test]
fn test_build_twitter_tags_uses_summary_when_no_image() {
    let site_url = test_site_url();
    let result = build_twitter_tags("Title", "Desc", None, None, &site_url);
    assert!(result.contains(r#"<meta name="twitter:card" content="summary">"#));
    assert!(result.contains(r#"<meta name="twitter:title" content="Title">"#));
    assert!(result.contains(r#"<meta name="twitter:description" content="Desc">"#));
    assert!(!result.contains("twitter:image"));
}

#[test]
fn test_build_schema_json_ld_basic() {
    let site_url = test_site_url();
    let result = build_schema_json_ld(
        "Test Article",
        "A test description",
        "https://example.com/posts/test/",
        "My Site",
        Some("2024-01-15"),
        None,
        &[],
        &site_url, &[],
    );

    assert!(result.contains(r#""@context": "https://schema.org""#));
    assert!(result.contains(r#""@type": "Article""#));
    assert!(result.contains(r#""headline": "Test Article""#));
    assert!(result.contains(r#""url": "https://example.com/posts/test/""#));
    assert!(result.contains(r#""datePublished": "2024-01-15""#));
    assert!(result.contains(r#""publisher":"#));
    assert!(result.contains(r#""name": "My Site""#));
}

#[test]
fn test_build_schema_json_ld_with_keywords() {
    let site_url = test_site_url();
    let result = build_schema_json_ld(
        "Test",
        "Desc",
        "https://example.com/",
        "Site",
        None,
        None,
        &["rust".to_string(), "wasm".to_string()],
        &site_url, &[],
    );

    let inner = result
        .strip_prefix(r#"<script type="application/ld+json">"#)
        .and_then(|b| b.strip_suffix("</script>"))
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(inner).unwrap();
    assert_eq!(v["keywords"], serde_json::json!(["rust", "wasm"]));
}

#[test]
fn test_build_schema_website_basic() {
    let result = build_schema_website(
        "My Site",
        "A site description",
        "https://example.com/",
        "en",
    );

    // Wrapped in the ld+json script tag
    assert!(result.starts_with(r#"<script type="application/ld+json">"#));
    assert!(result.ends_with("</script>"));

    assert!(result.contains(r#""@context": "https://schema.org""#));
    assert!(result.contains(r#""@type": "WebSite""#));
    assert!(result.contains(r#""name": "My Site""#));
    assert!(result.contains(r#""url": "https://example.com/""#));
    // description + inLanguage present when provided
    assert!(result.contains(r#""description": "A site description""#));
    assert!(result.contains(r#""inLanguage": "en""#));
    // publisher block: Organization with the site name
    assert!(result.contains(r#""publisher":"#));
    assert!(result.contains(r#""@type": "Organization""#));
}

#[test]
fn test_build_schema_website_omits_empty_description_and_lang() {
    let result = build_schema_website("My Site", "", "https://example.com/", "");

    assert!(result.contains(r#""@type": "WebSite""#));
    assert!(result.contains(r#""name": "My Site""#));
    assert!(result.contains(r#""url": "https://example.com/""#));
    // Empty description / lang lines are skipped entirely
    assert!(!result.contains("description"));
    assert!(!result.contains("inLanguage"));
    // publisher Organization always present
    assert!(result.contains(r#""@type": "Organization""#));
}

#[test]
fn test_escape_html_attr() {
    assert_eq!(
        escape_html_attr(r#"a "b" <c>"#),
        r#"a &quot;b&quot; &lt;c&gt;"#
    );
}

#[test]
fn test_build_og_tags_website_basic() {
    let site_url = test_site_url();
    let result = build_og_tags_website(
        "Research",
        "My Site",
        "A personal blog about stars and life.",
        "/",
        None,
        "en_US",
        &site_url,
    );

    assert!(result.contains(r#"og:type" content="website"#));
    // og:title is the page label, og:site_name is the (distinct) site name.
    assert!(result.contains(r#"og:title" content="Research"#));
    assert!(result.contains(r#"og:url" content="/"#));
    assert!(result.contains(r#"og:site_name" content="My Site"#));
    assert!(result.contains(r#"og:description" content="A personal blog about stars and life."#));
    // Should NOT contain article-specific tags
    assert!(!result.contains("article:published_time"));
    assert!(!result.contains("article:tag"));
}

#[test]
fn test_build_og_tags_website_with_cover() {
    let site_url = test_site_url();
    let cover = CoverRef::Local(ServedPath::from_source("images/cover.jpg").unwrap());
    let result = build_og_tags_website(
        "My Site",
        "My Site",
        "Description",
        "/",
        Some(&OgCover { cover: cover.clone(), dims: None, alt: None }),
        "en_US",
        &site_url,
    );

    // Local cover resolved via site_url
    assert!(result.contains(r#"og:image" content="https://example.com/images/cover.jpg"#));
}

fn make_translation_link(lang: Language, url_path: &str) -> TranslationLink {
    TranslationLink {
        lang_tag: lang.as_bcp47_attr().to_string(),
        url_path: url_path.to_string(),
        display_name: lang.display_name(),
    }
}

#[test]
fn an_unshipped_language_advertises_its_own_tag_not_the_nearest_shipped_one() {
    // `Language` has three variants, so a `fr` page resolves to `En` for the
    // interface. Its `<html lang>` says `fr` and its hreflang used to
    // say `en` — the two contradicting each other on the same page — while a
    // second `fr` page deduped away against the first as "already seen en".
    let fr = |url: &str| TranslationLink {
        lang_tag: "fr".to_string(),
        url_path: url.to_string(),
        display_name: "EN",
    };
    let html = build_hreflang_link_tags(
        "de",
        "de/about/index.html",
        &[fr("fr/about/index.html"), fr("fr-ca/about/index.html")],
        "example.com",
        "de",
    )
    .expect("expected hreflang block");

    assert!(html.contains(r#"hreflang="de" href="https://example.com/de/about/""#), "{html}");
    assert!(html.contains(r#"hreflang="fr" href="https://example.com/fr/about/""#), "{html}");
    // Same declared tag on both alternates: still deduped, first one wins.
    assert!(!html.contains("fr-ca"), "{html}");
    // x-default resolves against the tag too, not against a `Language`.
    assert!(html.contains(r#"hreflang="x-default" href="https://example.com/de/about/""#), "{html}");
}

#[test]
fn x_default_follows_the_edition_not_the_exact_tag() {
    // Retyping this function from `Language` to declared tags nearly dropped
    // x-default for a site whose declaration carries a region subtag.
    // See `i18n::same_language_edition`.
    let alt = |tag: &str, url: &str| TranslationLink {
        lang_tag: tag.to_string(),
        url_path: url.to_string(),
        display_name: "EN",
    };
    let x_default_of = |site: &str, alt_tag: &str| {
        build_hreflang_link_tags("fr", "fr/about/index.html", &[alt(alt_tag, "about/index.html")], "example.com", site)
            .expect("expected hreflang block")
    };

    // Region subtag on the site. `en-gb` is an allowed declaration but
    // `Language::from_code` rejects region subtags, so it arrives here as
    // `en-GB` and only prefix matching pairs it with an `en` page.
    assert!(
        x_default_of("en-GB", "en").contains(r#"hreflang="x-default" href="https://example.com/about/""#),
        "an `en` page is the x-default of an `en-GB` site"
    );

    // Same primary subtag, different script: two editions, so no x-default at
    // all rather than the wrong one.
    assert!(!x_default_of("zh-Hant", "zh-Hans").contains("x-default"));
}

#[test]
fn hreflang_emits_self_plus_alternates_plus_x_default() {
    let translations = vec![make_translation_link(
        Language::ZhHans,
        "zh-hans/about/index.html",
    )];
    let out = build_hreflang_link_tags(
        Language::En.as_bcp47_attr(),
        "about/index.html",
        &translations,
        "example.com",
        Language::En.as_bcp47_attr(),
    );
    let html = out.expect("expected hreflang block");
    assert!(
        html.contains(r#"<link rel="alternate" hreflang="en" href="https://example.com/about/">"#)
    );
    assert!(html.contains(
        r#"<link rel="alternate" hreflang="zh-Hans" href="https://example.com/zh-hans/about/">"#
    ));
    assert!(html.contains(
        r#"<link rel="alternate" hreflang="x-default" href="https://example.com/about/">"#
    ));
}

#[test]
fn hreflang_stays_per_article_only_no_homepage_root_fallback() {
    // Locks the intentional divergence from the nav switcher: the switcher
    // gains homepage-root fallbacks for translation-less pages, but hreflang
    // must NOT — a homepage root is not a same-content translation, and
    // emitting it as an hreflang alternate would be an invalid-hreflang SEO
    // bug. An EN page with NO authored translations must produce no hreflang
    // block at all (and certainly no fabricated zh-Hans alternate).
    let out = build_hreflang_link_tags(
        Language::En.as_bcp47_attr(),
        "en/writing/post/index.html",
        &[], // no per-article translations
        "example.com",
        Language::ZhHans.as_bcp47_attr(), // site default is ZH
    );
    assert!(
        out.is_none(),
        "no per-article translations => no hreflang block"
    );
    if let Some(s) = out {
        assert!(
            !s.contains("zh-Hans"),
            "hreflang must not fabricate a ZH alternate"
        );
    }
}

#[test]
fn hreflang_x_default_points_to_site_default_lang() {
    let translations = vec![make_translation_link(Language::En, "about/index.html")];
    let out = build_hreflang_link_tags(
        Language::ZhHans.as_bcp47_attr(),
        "zh-hans/about/index.html",
        &translations,
        "example.com",
        Language::En.as_bcp47_attr(),
    );
    let html = out.expect("expected hreflang block");
    assert!(html.contains(
        r#"<link rel="alternate" hreflang="zh-Hans" href="https://example.com/zh-hans/about/">"#
    ));
    assert!(
        html.contains(r#"<link rel="alternate" hreflang="en" href="https://example.com/about/">"#)
    );
    assert!(html.contains(
        r#"<link rel="alternate" hreflang="x-default" href="https://example.com/about/">"#
    ));
}

#[test]
fn hreflang_skips_x_default_when_no_site_default_translation() {
    let translations = vec![make_translation_link(
        Language::ZhHant,
        "zh-hant/about/index.html",
    )];
    let out = build_hreflang_link_tags(
        Language::ZhHans.as_bcp47_attr(),
        "zh-hans/about/index.html",
        &translations,
        "example.com",
        Language::En.as_bcp47_attr(),
    );
    let html = out.expect("expected hreflang block");
    assert!(html.contains(r#"hreflang="zh-Hans""#));
    assert!(html.contains(r#"hreflang="zh-Hant""#));
    assert!(!html.contains(r#"hreflang="x-default""#));
}

#[test]
fn hreflang_returns_none_when_no_translations() {
    let out = build_hreflang_link_tags(
        Language::En.as_bcp47_attr(),
        "about/index.html",
        &[],
        "example.com",
        Language::En.as_bcp47_attr(),
    );
    assert!(
        out.is_none(),
        "single-language pages should emit no hreflang"
    );
}

#[test]
fn hreflang_strips_index_html_suffix() {
    let translations = vec![make_translation_link(
        Language::ZhHans,
        "zh-hans/about/index.html",
    )];
    let out = build_hreflang_link_tags(
        Language::En.as_bcp47_attr(),
        "about/index.html",
        &translations,
        "example.com",
        Language::En.as_bcp47_attr(),
    )
    .unwrap();
    assert!(out.contains(r#"href="https://example.com/about/""#));
    assert!(out.contains(r#"href="https://example.com/zh-hans/about/""#));
    assert!(!out.contains("index.html"));
}

#[test]
fn hreflang_handles_filename_suffix_style() {
    let translations = vec![make_translation_link(
        Language::ZhHans,
        "about.zh-hans.html",
    )];
    let out = build_hreflang_link_tags(
        Language::En.as_bcp47_attr(),
        "about.html",
        &translations,
        "example.com",
        Language::En.as_bcp47_attr(),
    )
    .unwrap();
    assert!(out.contains(r#"href="https://example.com/about.html""#));
    assert!(out.contains(r#"href="https://example.com/about.zh-hans.html""#));
}

#[test]
fn hreflang_three_languages() {
    let translations = vec![
        make_translation_link(Language::ZhHans, "zh-hans/about/index.html"),
        make_translation_link(Language::ZhHant, "zh-hant/about/index.html"),
    ];
    let out = build_hreflang_link_tags(
        Language::En.as_bcp47_attr(),
        "about/index.html",
        &translations,
        "example.com",
        Language::En.as_bcp47_attr(),
    )
    .unwrap();
    assert!(out.contains(r#"hreflang="en""#));
    assert!(out.contains(r#"hreflang="zh-Hans""#));
    assert!(out.contains(r#"hreflang="zh-Hant""#));
    assert!(out.contains(r#"hreflang="x-default""#));
}

#[test]
fn hreflang_strips_trailing_slash_and_scheme_from_host() {
    let translations = vec![make_translation_link(
        Language::ZhHans,
        "zh-hans/about/index.html",
    )];
    let out_with_scheme = build_hreflang_link_tags(
        Language::En.as_bcp47_attr(),
        "about/index.html",
        &translations,
        "https://example.com/",
        Language::En.as_bcp47_attr(),
    )
    .unwrap();
    assert!(out_with_scheme.contains(r#"href="https://example.com/about/""#));
    assert!(!out_with_scheme.contains("https://https://"));
}

#[test]
fn hreflang_strips_leading_slash_from_url_path() {
    let translations = vec![make_translation_link(
        Language::ZhHans,
        "/zh-hans/about/index.html",
    )];
    let out = build_hreflang_link_tags(
        Language::En.as_bcp47_attr(),
        "/about/index.html",
        &translations,
        "example.com",
        Language::En.as_bcp47_attr(),
    )
    .unwrap();
    assert!(
        !out.contains("https://example.com//"),
        "no double-slash after host: {}",
        out
    );
    assert!(out.contains(r#"href="https://example.com/about/""#));
    assert!(out.contains(r#"href="https://example.com/zh-hans/about/""#));
}

#[test]
fn hreflang_escapes_html_unsafe_chars_in_url_path() {
    let translations = vec![make_translation_link(
        Language::ZhHans,
        "zh-hans/posts/q&a.html",
    )];
    let out = build_hreflang_link_tags(
        Language::En.as_bcp47_attr(),
        "posts/q&a.html",
        &translations,
        "example.com",
        Language::En.as_bcp47_attr(),
    )
    .unwrap();
    assert!(
        out.contains("q&amp;a.html"),
        "ampersand should be escaped: {}",
        out
    );
    assert!(
        !out.contains("q&a.html"),
        "raw & must not appear in href: {}",
        out
    );
}

#[test]
fn test_build_og_tags_skips_description_when_empty() {
    let site_url = test_site_url();
    let result = build_og_tags(
        "Title",
        "",
        "https://example.com/",
        "Site",
        None,
        None,
        &[],
        "",
        &site_url,
    );
    assert!(
        !result.contains("og:description"),
        "should skip og:description when empty, got: {result}"
    );
}

#[test]
fn test_build_og_tags_website_skips_description_when_empty() {
    let site_url = test_site_url();
    let result = build_og_tags_website(
        "Title",
        "Test Site",
        "",
        "https://example.com/",
        None,
        "",
        &site_url,
    );
    assert!(
        !result.contains("og:description"),
        "should skip og:description when empty, got: {result}"
    );
}

#[test]
fn test_build_twitter_tags_skips_description_when_empty() {
    let site_url = test_site_url();
    let result = build_twitter_tags("Title", "", None, None, &site_url);
    assert!(
        !result.contains("twitter:description"),
        "should skip twitter:description when empty, got: {result}"
    );
}

// --- New tests for absolute og:image / twitter:image ---

#[test]
fn og_image_becomes_absolute_when_cover_is_local() {
    let site_url = SiteUrl::parse("https://my-site.mosspub.com").unwrap();
    let cover = CoverRef::Local(ServedPath::for_og_card("abc1234567890def").unwrap());
    let tags = build_og_tags(
        "Page Title",
        "Description",
        "https://my-site.mosspub.com/page/",
        "Site",
        None,
        Some(&OgCover { cover: cover.clone(), dims: Some((1200, 630)), alt: None }),
        &[],
        "",
        &site_url,
    );
    assert!(
        tags.contains(
            r#"og:image" content="https://my-site.mosspub.com/_moss/og/abc1234567890def.png"#
        ),
        "og:image must be absolute; got: {}",
        tags
    );
}

#[test]
fn og_image_passes_through_already_absolute_cover() {
    let site_url = SiteUrl::parse("https://my-site.mosspub.com").unwrap();
    let cover = CoverRef::External("https://cdn.example.com/photo.jpg".to_string());
    let tags = build_og_tags(
        "Page Title",
        "Description",
        "https://my-site.mosspub.com/page/",
        "Site",
        None,
        Some(&OgCover { cover: cover.clone(), dims: None, alt: None }),
        &[],
        "",
        &site_url,
    );
    assert!(
        tags.contains(r#"og:image" content="https://cdn.example.com/photo.jpg"#),
        "external absolute URL must pass through unchanged; got: {}",
        tags
    );
}

#[test]
fn twitter_image_becomes_absolute_when_cover_is_local() {
    let site_url = SiteUrl::parse("https://my-site.mosspub.com").unwrap();
    let cover = CoverRef::Local(ServedPath::for_og_card("abc1234567890def").unwrap());
    let tags = build_twitter_tags("Page Title", "Description", Some(&cover), None, &site_url);
    assert!(
        tags.contains(
            r#"twitter:image" content="https://my-site.mosspub.com/_moss/og/abc1234567890def.png"#
        ),
        "twitter:image must be absolute; got: {}",
        tags
    );
}

#[test]
fn og_image_relative_for_localhost_site_url() {
    // Unconfigured / preview builds use http://localhost(:port) — og:image
    // emits a root-relative path so the Vite dev port doesn't bake into
    // static HTML that may later be served by other means. The previous
    // policy (absolute-even-for-localhost) prioritized crawler resolution,
    // but in practice no crawler ingests localhost URLs anyway, and the
    // dev-port leak (e.g. http://localhost:1420/...) was the active bug.
    // Production builds (https SiteUrl) still emit absolute URLs — see
    // og_image_becomes_absolute_when_cover_is_local.
    let site_url = SiteUrl::parse("http://localhost").unwrap();
    let cover = CoverRef::Local(ServedPath::for_og_card("abc1234567890def").unwrap());
    let tags = build_og_tags(
        "Page Title",
        "Description",
        "/page/",
        "Site",
        None,
        Some(&OgCover { cover: cover.clone(), dims: Some((1200, 630)), alt: None }),
        &[],
        "",
        &site_url,
    );
    assert!(
        tags.contains(r#"og:image" content="/_moss/og/abc1234567890def.png"#),
        "og:image must be relative for localhost site URL; got: {}",
        tags
    );
    assert!(
        !tags.contains("http://localhost"),
        "no localhost URL should leak into static og:image; got: {}",
        tags
    );
}

#[test]
fn og_image_is_relative_when_site_url_is_not_deployed() {
    // Regression: preview/dev builds baked http://localhost:1420 into
    // OG/Twitter URLs because resolve_site_url falls back to the Vite
    // dev port and CoverRef::to_absolute_url always absolutizes.
    //
    // NOTE: plan called for ServedPath::from_source("_moss/og/abc123.png")
    // but `_moss/` is a reserved prefix; from_source rejects it. The
    // proper constructor for OG card paths is for_og_card(hex16). Hex
    // value below is the literal "abc123" padded to 16 hex chars.
    let preview = SiteUrl::parse("http://localhost:1420").unwrap();
    let cover_hash = "abc1230000000000";
    let cover =
        CoverRef::Local(crate::build::served_path::ServedPath::for_og_card(cover_hash).unwrap());
    let og_result = build_og_tags(
        "Test Article",
        "A test description",
        "/posts/test/",
        "My Site",
        Some("2024-01-15"),
        Some(&OgCover { cover: cover.clone(), dims: Some((1200, 630)), alt: None }),
        &[],
        "en_US",
        &preview,
    );
    assert!(
        og_result.contains(&format!(
            r#"og:image" content="/_moss/og/{}.png"#,
            cover_hash
        )),
        "non-deployed must emit relative og:image. Got: {}",
        og_result
    );
    assert!(
        !og_result.contains("http://localhost"),
        "no localhost URL should leak into static og:image. Got: {}",
        og_result
    );

    // Same gate applies to twitter:image (emitted by build_twitter_tags).
    let twitter_result = build_twitter_tags(
        "Test Article",
        "A test description",
        Some(&cover),
        None,
        &preview,
    );
    assert!(
        twitter_result.contains(&format!(
            r#"twitter:image" content="/_moss/og/{}.png"#,
            cover_hash
        )),
        "twitter:image must also be relative. Got: {}",
        twitter_result
    );
    assert!(
        !twitter_result.contains("http://localhost"),
        "no localhost URL should leak into static twitter:image. Got: {}",
        twitter_result
    );
}

#[test]
fn og_image_is_absolute_when_site_url_is_deployed() {
    // NOTE: plan called for ServedPath::from_source("_moss/og/abc123.png")
    // but `_moss/` is reserved; use for_og_card(hex16) instead.
    let real = SiteUrl::parse("https://example.com").unwrap();
    let cover_hash = "abc1230000000000";
    let cover =
        CoverRef::Local(crate::build::served_path::ServedPath::for_og_card(cover_hash).unwrap());
    let result = build_og_tags(
        "Test",
        "desc",
        "https://example.com/",
        "Mountain Home",
        None,
        Some(&OgCover { cover: cover.clone(), dims: Some((1200, 630)), alt: None }),
        &[],
        "zh_CN",
        &real,
    );
    assert!(
        result.contains(&format!(
            r#"og:image" content="https://example.com/_moss/og/{}.png"#,
            cover_hash
        )),
        "deployed must emit absolute og:image. Got: {}",
        result
    );
}

#[test]
fn cover_ref_local_resolves_via_site_url() {
    let sp = ServedPath::for_og_card("87ba30f2b3c09ca9").unwrap();
    let cr = CoverRef::Local(sp);
    let site = SiteUrl::parse("https://example.com").unwrap();
    assert_eq!(
        cr.to_absolute_url(&site),
        "https://example.com/_moss/og/87ba30f2b3c09ca9.png"
    );
}

#[test]
fn cover_ref_external_passes_through() {
    let cr = CoverRef::External("https://cdn.example.com/photo.jpg".to_string());
    let site = SiteUrl::parse("https://moss.example.com").unwrap();
    assert_eq!(
        cr.to_absolute_url(&site),
        "https://cdn.example.com/photo.jpg"
    );
}

/// A local cover's `og:image` must keep the source's own raster extension —
/// never be "optimised" to the webp variant. Facebook's scraper has never
/// ingested webp for `og:image`, and moss emits no `og:image:type`, so a
/// scraper that does not sniff has only the extension to go on. This is the
/// one reason the raster fallback cannot be dropped even once every browser
/// takes the `<source>`.
#[test]
fn og_image_keeps_the_raster_extension_and_never_becomes_webp() {
    let site_url = test_site_url();
    for source in ["images/cover.jpg", "images/cover.png", "images/cover.jpeg"] {
        let cover = CoverRef::Local(ServedPath::from_source(source).unwrap());
        let url = cover.to_meta_url(&site_url);
        assert!(
            !url.ends_with(".webp"),
            "og:image for {source} resolved to {url}; social scrapers need the raster"
        );
        assert!(
            url.ends_with(&source[source.rfind('.').unwrap()..]),
            "og:image for {source} resolved to {url}; extension must survive"
        );
    }
}

/// Author text reaches a `<script>` block verbatim, so `</script>` or `<!--`
/// in a title or description must not be able to end or open anything.
#[test]
fn json_ld_blocks_cannot_be_broken_out_of_by_page_text() {
    let hostile = "</script><img src=x onerror=alert(1)><!-- x";
    let article = build_schema_json_ld(
        hostile, hostile, "https://example.com/", hostile, None, None,
        &[hostile.to_string()], &test_site_url(), &[],
    );
    let website = build_schema_website(hostile, hostile, "https://example.com/", "en");
    for block in [article, website] {
        let inner = block
            .strip_prefix(r#"<script type="application/ld+json">"#)
            .and_then(|b| b.strip_suffix("</script>"))
            .expect("one script wrapper");
        assert!(!inner.contains("</script"), "closes the block early: {inner}");
        assert!(!inner.contains("<!--"), "opens an HTML comment: {inner}");
        let v: serde_json::Value = serde_json::from_str(inner).expect("valid JSON");
        assert!(v.to_string().contains(hostile), "the text survives the escaping");
    }
}

#[test]
fn title_only_page_has_no_entry() {
    assert!(preview_entry("Work", None, "").is_none());
    assert!(preview_entry("Work", Some(""), "").is_none());
}

#[test]
fn page_with_description_keeps_entry() {
    let entry = preview_entry("Work", Some("Selected projects"), "").expect("entry");
    assert_eq!(entry["title"], "Work");
    assert_eq!(entry["description"], "Selected projects");
    assert!(!entry.contains_key("preview"));
}

#[test]
fn page_with_excerpt_only_keeps_entry() {
    let entry = preview_entry("Work", None, "Body text").expect("entry");
    assert_eq!(entry["preview"], "Body text");
    assert!(!entry.contains_key("description"));
}

#[test]
fn an_article_lists_its_citations_and_a_page_without_any_has_no_key() {
    let ld = |citations: &[moss_core::ast::quote_source::QuoteSource]| {
        let html = build_schema_json_ld("T", "D", "https://example.com/", "S", None, None, &[], &test_site_url(), citations);
        let inner = html
            .strip_prefix(r#"<script type="application/ld+json">"#)
            .and_then(|b| b.strip_suffix("</script>"))
            .unwrap();
        serde_json::from_str::<serde_json::Value>(inner).expect("valid JSON")
    };
    let source = |name: &str, href: &str| moss_core::ast::quote_source::QuoteSource { href: href.into(), text: name.into() };
    let v = ld(&[source("Say \"hi\" </script>", "https://example.org/a"), source("B", "https://example.org/b")]);
    assert_eq!(
        v["citation"],
        serde_json::json!([
            {"@type": "CreativeWork", "name": "Say \"hi\" </script>", "url": "https://example.org/a"},
            {"@type": "CreativeWork", "name": "B", "url": "https://example.org/b"},
        ])
    );
    assert!(ld(&[]).get("citation").is_none());
}
