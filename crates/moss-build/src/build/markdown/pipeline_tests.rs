use super::*;
use crate::i18n::Language;

/// BUG 6: dims are keyed by the RAW source path but probed by the
/// TRANSFORMED output URL (covers arrive slugified). `build_asset_snapshot`
/// must ADDITIVELY index every dims/lqip/color entry a second time under
/// its slugified output-URL key so a slug-form cover lookup HITS the real
/// dimensions instead of firing the 800x600 fallback.
#[test]
fn build_asset_snapshot_additively_indexes_output_url() {
    let meta = crate::types::content::MediaMetadata {
        is_animated: false,
        path: "assets/Europe - A Prophecy/e-006.jpg".to_string(),
        file_type: "jpg".to_string(),
        size: 0,
        modified: None,
        dimensions: Some((4515, 6158)),
        dominant_color: Some("#123456".to_string()),
        lqip_data_uri: Some("data:image/jpeg;base64,zzz".to_string()),
    };
    let lookup =
        crate::build::media::dimensions::MediaDimensionLookup::new(&[meta], &[], &HashMap::new(), None);
    let snap = lookup.asset_snapshot();

    // Source key is preserved (percent-decode probes still rely on it).
    let src_key = std::path::PathBuf::from("assets/Europe - A Prophecy/e-006.jpg");
    assert_eq!(snap.dimensions.get(&src_key), Some(&(4515, 6158)));

    // Slugified output-URL key is additively present under the same value.
    let slug_key = std::path::PathBuf::from("assets/europe-a-prophecy/e-006.jpg");
    assert_eq!(snap.dimensions.get(&slug_key), Some(&(4515, 6158)));
    assert_eq!(
        snap.lqip.get(&slug_key).map(String::as_str),
        Some("data:image/jpeg;base64,zzz")
    );
    assert_eq!(
        snap.dominant_color.get(&slug_key).map(String::as_str),
        Some("#123456")
    );
}

/// Task 10: the scanned `is_animated` flag must ride the snapshot keyed
/// IDENTICALLY to `dimensions` — same raw source key AND the additive
/// slugified output-URL key — so Task 12's ladder gate can probe
/// `is_animated(src)` right beside `lookup_dims(src)`. Missing → false.
#[test]
fn build_asset_snapshot_carries_animated_flag() {
    let animated = crate::types::content::MediaMetadata {
        is_animated: true,
        path: "assets/My Loops/spin.gif".to_string(),
        file_type: "gif".to_string(),
        size: 0,
        modified: None,
        dimensions: Some((320, 240)),
        dominant_color: None,
        lqip_data_uri: None,
    };
    let still = crate::types::content::MediaMetadata {
        is_animated: false,
        path: "assets/photo.jpg".to_string(),
        file_type: "jpg".to_string(),
        size: 0,
        modified: None,
        dimensions: Some((1024, 768)),
        dominant_color: None,
        lqip_data_uri: None,
    };
    let lookup = crate::build::media::dimensions::MediaDimensionLookup::new(
        &[animated, still],
        &[],
        &HashMap::new(),
        None,
    );
    let snap = lookup.asset_snapshot();

    // Animated source: true under BOTH the raw source key and the additive
    // slugified output-URL key (keyed identically to `dimensions`).
    assert!(snap.is_animated(&"assets/My Loops/spin.gif".into()));
    assert!(snap.is_animated(&"assets/my-loops/spin.gif".into()));
    // The dims of that same source resolve under the same slug key — the two
    // maps stay in lockstep.
    assert_eq!(
        snap.dimensions
            .get(&std::path::PathBuf::from("assets/my-loops/spin.gif")),
        Some(&(320, 240))
    );

    // Non-animated source present as false (not merely absent).
    assert!(!snap.is_animated(&"assets/photo.jpg".into()));
    // Unknown source → false.
    assert!(!snap.is_animated(&"assets/missing.png".into()));
}

/// Determinism guard: two DISTINCT raw source keys that slugify to the
/// SAME output-URL key must resolve to a STABLE value across builds. The
/// source is a `HashMap` (random SipHash order), so a naive `.or_insert`
/// over its iterator could emit different dims/lqip/color per build,
/// regressing moss's build-determinism doctrine and potentially flaking CLI
/// snapshot fixtures. `build_asset_snapshot` sorts by source key, so the
/// lexicographically-smallest colliding source deterministically wins. We
/// run the build many times over a freshly-shuffled HashMap and assert the
/// slug key always holds the smaller source's value.
#[test]
fn build_asset_snapshot_slug_collision_is_deterministic() {
    // `assets/My Photos/x.jpg` and `assets/My-Photos/x.jpg` both slugify to
    // `assets/my-photos/x.jpg`. Give them DIFFERENT dims so a wrong winner
    // is observable. Lexicographically, "My Photos" (space, 0x20) sorts
    // before "My-Photos" (hyphen, 0x2D), so the space variant must win.
    let space_variant = crate::types::content::MediaMetadata {
        is_animated: false,
        path: "assets/My Photos/x.jpg".to_string(),
        file_type: "jpg".to_string(),
        size: 0,
        modified: None,
        dimensions: Some((100, 100)),
        dominant_color: Some("#aaaaaa".to_string()),
        lqip_data_uri: Some("data:image/jpeg;base64,AAA".to_string()),
    };
    let hyphen_variant = crate::types::content::MediaMetadata {
        is_animated: false,
        path: "assets/My-Photos/x.jpg".to_string(),
        file_type: "jpg".to_string(),
        size: 0,
        modified: None,
        dimensions: Some((200, 200)),
        dominant_color: Some("#bbbbbb".to_string()),
        lqip_data_uri: Some("data:image/jpeg;base64,BBB".to_string()),
    };
    let slug_key = std::path::PathBuf::from("assets/my-photos/x.jpg");
    let overrides: HashMap<String, String> = HashMap::new();

    // Many independent builds, each over a fresh lookup (fresh HashMap seed
    // / insertion order). The winner must be stable every time.
    for _ in 0..64 {
        let lookup = crate::build::media::dimensions::MediaDimensionLookup::new(
            &[space_variant.clone(), hyphen_variant.clone()],
            &[],
            &HashMap::new(),
            None,
        );
        let snap = lookup.asset_snapshot();
        assert_eq!(
                snap.dimensions.get(&slug_key),
                Some(&(100, 100)),
                "colliding slug key must deterministically hold the lexicographically-smallest source's dims"
            );
        assert_eq!(
            snap.lqip.get(&slug_key).map(String::as_str),
            Some("data:image/jpeg;base64,AAA"),
            "lqip must resolve to the same deterministic winner"
        );
        assert_eq!(
            snap.dominant_color.get(&slug_key).map(String::as_str),
            Some("#aaaaaa"),
            "dominant_color must resolve to the same deterministic winner"
        );
    }
}

/// When a document has no `lang:` frontmatter, no filename suffix, and
/// content too short for `whatlang` to detect, the doc should fall back
/// to the site's default language — not a hard-coded English.
///
/// Regression for issue #545. Mirrors the real-world failure on the 刘果
/// vault: a Chinese-default site with `视频/冬日之歌.md` (~13 CJK chars
/// of body) was rendering with `lang=en`, mislabeling it across the
/// HTML lang attribute, language switcher, and pages-by-lang queries.
#[test]
fn short_doc_with_no_signal_uses_site_default_lang() {
    let md = "---\ntitle: 冬日之歌\n---\n\n短。";
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "videos/winter-song.md",
        md,
        "site",
        &empty_map,
        false,
        Language::ZhHans,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    assert_eq!(doc.lang, Language::ZhHans);
}

/// `author:` already lowers into `ParsedDocument.author`; `editor:`/`jury:`
/// are new fields added beside it in the same struct literal (task A3) and
/// must lower the same way, through the same field-agnostic
/// `deserialize_name_list`/`serialize_name_list` normalizer.
#[test]
fn editor_and_jury_frontmatter_lower_into_parsed_document_beside_author() {
    let md = "---\ntitle: Committee Notes\nauthor: Ada Lin\neditor: Kane\njury:\n  - Kane\n  - Kaneda\n---\n\nBody.\n";
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "posts/committee-notes.md",
        md,
        "site",
        &empty_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    assert_eq!(doc.author, vec!["Ada Lin".to_string()]);
    assert_eq!(doc.editor, vec!["Kane".to_string()]);
    assert_eq!(doc.jury, vec!["Kane".to_string(), "Kaneda".to_string()]);
}

/// Step 5: `transform_events` captures the first markdown-origin
/// `![]()` image and exposes it on the `ParsedDocument` for the
/// cover-resolution chain to consume. Replaces the prior regex
/// scrape (`first_body_image`) over rendered HTML.
#[test]
fn body_cover_path_captures_first_markdown_image() {
    let md = "# Title\n\nIntro paragraph.\n\n![first image](photos/first.jpg)\n\n![second image](photos/second.jpg)\n";
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "posts/article.md",
        md,
        "site",
        &empty_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    assert_eq!(
        doc.body_cover_path.as_deref(),
        Some("photos/first.jpg"),
        "body_cover_path must equal the first markdown image's resolved src"
    );
}

/// Raw HTML `<img>` literally embedded in markdown is `Event::Html`,
/// not `Tag::Image` — intentionally excluded from `body_cover_path`
/// (documented carve-out per `image_render.rs:56-93`).
#[test]
fn body_cover_path_skips_raw_html_img_in_markdown() {
    let md =
        "# Title\n\nIntro.\n\n<img src=\"photos/raw.jpg\">\n\n![real image](photos/real.jpg)\n";
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "posts/article.md",
        md,
        "site",
        &empty_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    assert_eq!(
        doc.body_cover_path.as_deref(),
        Some("photos/real.jpg"),
        "raw HTML <img> is opaque Event::Html and must not feed body_cover_path"
    );
}

// Phase 4 PR7a-fragment (2026-05-28): the unit test
// `strip_resolver_sentinel_drops_all_three_known_prefixes` deleted
// alongside its subject helper. The sentinel-stripping behavior at
// body-cover-path capture time now lives inside
// `find_first_block_image` / `process_markdown_file`'s
// `body_cover_path` derivation, which reads `Url::Resolved` from the
// typed AST — the sentinel is decoded by `classify_url_prod` before
// it ever reaches the cover field. The end-to-end coverage is
// preserved by `body_cover_path_*` integration tests in this module.

/// No images in the document → `body_cover_path` is `None`. The cover
/// chain falls through to whatever rung comes next.
#[test]
fn body_cover_path_none_when_document_has_no_images() {
    let md = "# Title\n\nPure prose, no images.\n";
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "posts/article.md",
        md,
        "site",
        &empty_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    assert_eq!(doc.body_cover_path, None);
}

/// Frontmatter `lang:` still wins over the site default.
#[test]
fn frontmatter_lang_overrides_site_default() {
    let md = "---\ntitle: A note\nlang: en\n---\n\n短文。";
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "notes/note.md",
        md,
        "site",
        &empty_map,
        false,
        Language::ZhHans,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    assert_eq!(doc.lang, Language::En);
}

/// Article with no frontmatter title and no body H1 — the page must render
/// with an injected <h1 class="moss-article-title"> derived from the
/// title-cased filename. This matches the Obsidian convention where the
/// filename IS the document title. See
/// docs/archive/2026-04-28-auto-h1-injection-design.md.
#[test]
fn article_without_h1_or_title_injects_filename_as_h1() {
    let md = "短文章正文。\n";
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "文字/AI 带来写作的黄金时代.md",
        md,
        "site",
        &empty_map,
        false,
        Language::ZhHans,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    assert!(
        doc.html_content
            .contains("<h1 class=\"moss-article-title\">AI 带来写作的黄金时代</h1>"),
        "expected injected article-title H1, got: {}",
        doc.html_content
    );
}

/// 2026-05-06 consolidation: frontmatter `title:` drives the visible
/// H1 (and chrome). Filename is used only when `title:` is missing.
/// This test inverts the prior 2026-04-29 contract.
#[test]
fn article_with_frontmatter_title_injects_title() {
    let md = "---\ntitle: Custom Heading\n---\n\nWhere did we come from?\n";
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "Our Mission.md",
        md,
        "site",
        &empty_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    assert!(
        doc.html_content
            .contains("<h1 class=\"moss-article-title\">Custom Heading</h1>"),
        "expected injected H1 to use frontmatter.title, got: {}",
        doc.html_content
    );
    assert!(
        !doc.html_content
            .contains("<h1 class=\"moss-article-title\">Our Mission</h1>"),
        "filename-derived H1 should NOT appear when title: is set, got: {}",
        doc.html_content
    );
}

/// A root-level non-index file in organized mode is a nav page; the nav bar
/// shows its title, so the auto-injected article title is suppressed.
#[test]
fn nav_page_suppresses_injected_article_title() {
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "about.md",
        "Body paragraph.\n",
        "site",
        &empty_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        true, // has_content_folders → organized → root file is a nav page
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    assert!(
        !doc.html_content.contains("moss-article-title"),
        "nav page must not inject an article title, got: {}",
        doc.html_content
    );
}

/// A nested file is never an auto nav page, so it still injects its title.
#[test]
fn non_nav_article_still_injects_title() {
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "blog/my-post.md",
        "Body paragraph.\n",
        "site",
        &empty_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        true,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    assert!(
        doc.html_content.contains("moss-article-title"),
        "non-nav article must inject its title, got: {}",
        doc.html_content
    );
}

/// `nav: false` opts a root file out of the nav even in organized mode
/// (where every root non-index file would otherwise auto-qualify), so its
/// title injects again. Passing `has_content_folders: true` exercises that
/// opt-out path rather than a flat-mode no-op.
#[test]
fn nav_false_restores_article_title() {
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "about.md",
        "---\nnav: false\n---\nBody.\n",
        "site",
        &empty_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        true, // organized mode; nav: false opts out even though it would auto-qualify
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    assert!(
        doc.html_content.contains("moss-article-title"),
        "nav: false must restore title injection, got: {}",
        doc.html_content
    );
}

/// Suppression drops only the AUTO-injected title; an authored body H1 stays.
#[test]
fn nav_page_keeps_authored_body_h1() {
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "about.md",
        "# My Heading\n\nBody.\n",
        "site",
        &empty_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        true,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    assert!(
        !doc.html_content.contains("moss-article-title"),
        "nav page suppresses the injected title"
    );
    assert!(
        doc.html_content.contains("My Heading"),
        "authored body H1 must survive, got: {}",
        doc.html_content
    );
}

/// Regression: an article whose body opens with a section-number H1
/// (`# 1.`) MUST inject the filename heading above the body. An earlier
/// 2026-04-28 inference rule suppressed injection when the body's lead
/// block was an H1; the 2026-04-29 contract removed that inference and
/// always injects (gated only on `heading: false` and hero-H1).
///
/// Original screenshot: 不要去自动化你爱的事 — opened with `# 1.`
/// and got no visible heading.
#[test]
fn strict_contract_section_number_h1_does_not_suppress_injection() {
    let md = "---\nuid: x\n---\n\n# 1.\n\nIntro text.\n";
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "writings/Automate With Care.md",
        md,
        "site",
        &empty_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    // Title must be injected from the filename (verbatim case).
    assert!(
        doc.html_content
            .contains("<h1 class=\"moss-article-title\">Automate With Care</h1>"),
        "expected injected title from filename, got: {}",
        doc.html_content
    );
    // The body section H1 must still be present.
    assert!(
            doc.html_content.contains(
                "<h1 id=\"1.\">1.<a class=\"moss-heading-anchor\" href=\"#1.\" aria-label=\"Permalink to this section\"></a></h1>"
            ),
            "body section H1 must be preserved, got: {}",
            doc.html_content
        );
}

/// Strict-contract regression: an article with a leading blockquote
/// (epigraph) followed by a section-number H1 must inject the title
/// AND keep the blockquote as authored. This is the bluegrass-hammer
/// case from the second 2026-04-29 screenshot.
#[test]
fn strict_contract_leading_blockquote_then_section_h1_injects_title() {
    let md = "---\nuid: x\ntitle: Folk Songs\n---\n\n> Epigraph quote.\n\n# 1.\n\nIntro text.\n";
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "writings/Folk Songs.md",
        md,
        "site",
        &empty_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    assert!(
        doc.html_content
            .contains("<h1 class=\"moss-article-title\">Folk Songs</h1>"),
        "expected injected title above the blockquote, got: {}",
        doc.html_content
    );
    // Blockquote preserved.
    assert!(
        doc.html_content.contains("<blockquote>"),
        "leading blockquote must be preserved, got: {}",
        doc.html_content
    );
    // The injected title must come BEFORE the blockquote in document order.
    let title_pos = doc.html_content.find("moss-article-title").unwrap();
    let bq_pos = doc.html_content.find("<blockquote").unwrap();
    assert!(
        title_pos < bq_pos,
        "injected title must precede the blockquote, got title at {} vs blockquote at {}",
        title_pos,
        bq_pos
    );
}

/// Obsidian-match: an article whose body opens with `# Title` matching the
/// resolved title (frontmatter or filename) renders BOTH the injected
/// `moss-article-title` and the authored body H1 — no dedup. (Replaces the
/// pre-2026-05-30 `strict_contract_dedup_strips_matching_leading_body_h1`.)
#[test]
fn matching_leading_body_h1_is_kept_alongside_injected_title() {
    let md = "---\ntitle: Architecture\n---\n\n# Architecture\n\nBody content.\n";
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "writings/Architecture.md",
        md,
        "site",
        &empty_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    // Two H1s now: the injected article title AND the authored body H1.
    let h1_count = doc.html_content.matches("<h1").count();
    assert_eq!(
        h1_count, 2,
        "expected injected title + body H1 (no dedup), got {} in: {}",
        h1_count, doc.html_content
    );
    assert!(
        doc.html_content
            .contains("<h1 class=\"moss-article-title\">Architecture</h1>"),
        "the injected article title must be present, got: {}",
        doc.html_content
    );
}

/// Body H1s are never stripped (Obsidian-match). An article whose body's
/// leading H1 differs from the injected title keeps both the injected
/// `moss-article-title` and the authored body H1.
#[test]
fn dedup_does_not_strip_when_text_differs() {
    let md = "---\ntitle: Outer\n---\n\n# Inner\n\nBody.\n";
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "Page.md",
        md,
        "site",
        &empty_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    assert!(
        doc.html_content
            .contains("<h1 class=\"moss-article-title\">Outer</h1>"),
        "expected injected heading from title: ('Outer'); got: {}",
        doc.html_content
    );
    assert!(
            doc.html_content.contains(
                "<h1 id=\"inner\">Inner<a class=\"moss-heading-anchor\" href=\"#inner\" aria-label=\"Permalink to this section\"></a></h1>"
            ),
            "body H1 'Inner' must be preserved (differs from resolved heading), got: {}",
            doc.html_content
        );
}

/// A buried H1 matching the filename is a section header and is kept
/// (body H1s are never stripped).
#[test]
fn dedup_does_not_strip_buried_h1_matching_filename() {
    let md = "Intro paragraph.\n\n# Page\n\nMore.\n";
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "Page.md",
        md,
        "site",
        &empty_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    assert!(
        doc.html_content
            .contains("<h1 class=\"moss-article-title\">Page</h1>"),
        "expected injected heading 'Page' from filename, got: {}",
        doc.html_content
    );
    // Buried 'Page' H1 still present (matches filename but not at lead position).
    // The body heading carries the slug id + permalink anchor; the injected
    // title H1 (above) carries the class and no anchor.
    assert!(
            doc.html_content.contains(
                "<h1 id=\"page\">Page<a class=\"moss-heading-anchor\" href=\"#page\" aria-label=\"Permalink to this section\"></a></h1>"
            ),
            "buried matching H1 must NOT be stripped (it's a section header), got: {}",
            doc.html_content
        );
}

/// Article with a body H1 whose text differs from `title:`. Under the
/// 2026-05-06 consolidated rule, the resolved heading (title: value)
/// is injected, and the differing body H1 survives as a section header.
#[test]
fn article_with_differing_body_h1_renders_both() {
    let md = "---\ntitle: Resolved Title\n---\n\n# Body Heading\n\nContent.\n";
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "Page.md",
        md,
        "site",
        &empty_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    assert!(
        doc.html_content
            .contains("<h1 class=\"moss-article-title\">Resolved Title</h1>"),
        "injected heading must be from title:, got: {}",
        doc.html_content
    );
    assert!(
            doc.html_content.contains(
                "<h1 id=\"body-heading\">Body Heading<a class=\"moss-heading-anchor\" href=\"#body-heading\" aria-label=\"Permalink to this section\"></a></h1>"
            ),
            "differing body H1 must be preserved as a section header, got: {}",
            doc.html_content
        );
    assert_eq!(
        doc.title, "Resolved Title",
        "doc.title also reflects the consolidated value; got: {}",
        doc.title
    );
}

/// Explicit opt-out: `title: ""` on an article suppresses the
/// auto-injection. This replaces the prior `heading: false` mechanism.
#[test]
fn article_with_empty_title_suppresses_injection() {
    let md = "---\ntitle: \"\"\n---\n\nIntentionally headerless.\n";
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "untitled.md",
        md,
        "site",
        &empty_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    assert!(
        !doc.html_content.contains("moss-article-title"),
        "title: \"\" must suppress injection, got: {}",
        doc.html_content
    );
}

/// Index page (folder index) with no body H1 — moss must NOT inject.
/// Index pages typically open with a hero or designed landing layout
/// where a stacked text H1 would compete with the visual header.
/// See docs/archive/2026-04-28-auto-h1-injection-design.md.
#[test]
fn index_page_without_h1_does_not_inject() {
    let md = "---\ntitle: Yin Lab @ NYU\n---\n\nWelcome.\n";
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "main.md",
        md,
        "site",
        &empty_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    assert!(
        !doc.html_content.contains("moss-article-title"),
        "index page must not inject, got: {}",
        doc.html_content
    );
}

/// Self-named folder index (`刘果/刘果.md`) — recognized as an index by
/// moss_core::home::is_home_file. Must not inject.
#[test]
fn self_named_folder_index_does_not_inject() {
    let md = "欢迎。\n";
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "刘果/刘果.md",
        md,
        "site",
        &empty_map,
        false,
        Language::ZhHans,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    assert!(
        !doc.html_content.contains("moss-article-title"),
        "self-named folder index must not inject, got: {}",
        doc.html_content
    );
}

// ---- Obsidian-match title behavior (2026-05-30) ----
// Titles come from `title:` else filename (folder name for index/self-named
// notes), NEVER from the body's first `# H1`. Authored body H1s are kept
// verbatim — no dedup on article, index, or homepage pages.

/// Local wrapper around `process_markdown_file` for these tests: takes only
/// the three inputs that vary here, fills the rest with the same defaults
/// the surrounding tests use. `root` is `Option<&str>` for call-site clarity.
fn parse_for_test(file_path: &str, md: &str, root: Option<&str>) -> ParsedDocument {
    let empty_map = HashMap::new();
    process_markdown_file(
        file_path,
        md,
        root.unwrap_or("site"),
        &empty_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("parse should succeed")
}

/// Self-named folder note `Research/Research.md`: a paragraph then `# Method`.
/// Title must be the folder name "Research", NOT the body H1 "Method".
/// The authored `# Method` stays in the body verbatim.
#[test]
fn folder_index_title_uses_folder_name_not_body_h1() {
    let md = "---\nweight: 1\n---\nIntro paragraph.\n\n# Method\n\nbody\n";
    let doc = parse_for_test("Research/Research.md", md, Some("VaultRoot"));
    assert_eq!(
        doc.title, "Research",
        "folder-index title must be the folder name, not the body H1"
    );
    assert!(
            doc.html_content.contains(
                "<h1 id=\"method\">Method<a class=\"moss-heading-anchor\" href=\"#method\" aria-label=\"Permalink to this section\"></a></h1>"
            ),
            "authored body H1 'Method' must be kept verbatim, got: {}",
            doc.html_content
        );
}

/// Self-named folder note whose body STARTS with `# Foo`: title is the
/// folder/filename, and the leading H1 is NOT deduped away.
#[test]
fn folder_index_leading_body_h1_is_not_stripped() {
    let md = "---\n---\n# Foo\n\nbody\n";
    let doc = parse_for_test("notes/notes.md", md, Some("VaultRoot"));
    assert_eq!(doc.title, "notes");
    assert!(
            doc.html_content.contains(
                "<h1 id=\"foo\">Foo<a class=\"moss-heading-anchor\" href=\"#foo\" aria-label=\"Permalink to this section\"></a></h1>"
            ),
            "leading body H1 must be retained (no dedup), got: {}",
            doc.html_content
        );
}

/// Frontmatter `title:` wins; the body H1 is still kept as content.
#[test]
fn folder_index_title_frontmatter_wins_and_body_h1_kept() {
    let md = "---\ntitle: Custom Title\n---\n# Other\n\nbody\n";
    let doc = parse_for_test("section/section.md", md, Some("VaultRoot"));
    assert_eq!(doc.title, "Custom Title");
    assert!(
            doc.html_content.contains(
                "<h1 id=\"other\">Other<a class=\"moss-heading-anchor\" href=\"#other\" aria-label=\"Permalink to this section\"></a></h1>"
            ),
            "body H1 kept, got: {}",
            doc.html_content
        );
}

/// Root homepage `index.md`: injects NO title h1 (compute().visible is false
/// for index files; the is_homepage branch skips the folder-title h1). The
/// author's leading `# Welcome` must survive at the pipeline level (the
/// pipeline never strips it and never injects a filename h1 for index pages).
#[test]
fn homepage_body_h1_is_kept_verbatim_no_injection() {
    let md = "---\n---\n# Welcome\n\nhome body\n";
    let doc = parse_for_test("index.md", md, Some("MySite"));
    assert!(
            doc.html_content.contains(
                "<h1 id=\"welcome\">Welcome<a class=\"moss-heading-anchor\" href=\"#welcome\" aria-label=\"Permalink to this section\"></a></h1>"
            ),
            "homepage body H1 must be kept verbatim, got: {}",
            doc.html_content
        );
    assert!(
        !doc.html_content.contains("moss-article-title"),
        "homepage must NOT inject a filename title h1, got: {}",
        doc.html_content
    );
}

/// Article (non-index): body H1 text equals the filename-derived title.
/// `filename_text("posts/my post.md")` == "my post" (stem verbatim, no
/// title-casing). Both the injected `moss-article-title` and the body H1
/// render — Obsidian-match: no dedup.
#[test]
fn article_body_h1_matching_title_is_not_deduped() {
    let md = "---\n---\n# my post\n\nbody\n";
    let doc = parse_for_test("posts/my post.md", md, Some("VaultRoot"));
    assert!(
        doc.html_content.contains("moss-article-title"),
        "article title still injected, got: {}",
        doc.html_content
    );
    let count = doc.html_content.matches("my post").count();
    assert!(
        count >= 2,
        "expected both injected title and body H1 (no dedup), got {} in: {}",
        count,
        doc.html_content
    );
}

/// Article whose `# Title` lives inside a `:::hero` block — moss must NOT
/// inject. The hero block is extracted out of the markdown body before
/// rendering and placed at template level, so a body-H1 lookup against
/// `html_content` alone returns None. The injection gate must also check
/// `hero_html`. Regression for the SoCiviC daowu page.
/// Filename case is preserved verbatim. An author who writes a stem like
/// `Farewell, and Erase on BroadwayWorld` gets exactly that as the
/// injected H1 — no per-word title-casing that would change `and` to
/// `And` or `on` to `On`. Hyphens and underscores still become spaces.
#[test]
fn filename_title_preserves_case_verbatim() {
    let md = "Body text.\n";
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "news/Farewell, and Erase on BroadwayWorld.md",
        md,
        "site",
        &empty_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    assert!(
        doc.html_content
            .contains("<h1 class=\"moss-article-title\">Farewell, and Erase on BroadwayWorld</h1>"),
        "expected verbatim filename heading, got: {}",
        doc.html_content
    );
}

/// A kebab-case stem like `our-mission` becomes `our mission`, NOT
/// `Our Mission`. Authors who want capitals write the file as
/// `Our Mission.md`. Filename is identity: what the author types is
/// what shows.
#[test]
fn filename_title_no_longer_capitalizes_kebab_case() {
    let md = "Body.\n";
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "our-mission.md",
        md,
        "site",
        &empty_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    assert!(
        doc.html_content
            .contains("<h1 class=\"moss-article-title\">our mission</h1>"),
        "expected lowercase verbatim heading from kebab-case stem, got: {}",
        doc.html_content
    );
}

#[test]
fn article_with_h1_inside_hero_block_does_not_inject() {
    let md = "---\ntitle: A House of Daowu\n---\n\n:::hero\n# A House of Daowu\n:::\n\nBody.\n";
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "work/daowu.md",
        md,
        "site",
        &empty_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    assert!(
        !doc.html_content.contains("moss-article-title"),
        "hero-block H1 must suppress injection, got: {}",
        doc.html_content
    );
}

#[test]
fn article_with_an_image_only_hero_still_renders_its_title() {
    // The companion to the test above, and the one that was missing: a hero
    // with an H1 inside legitimately owns the title slot, but a hero with
    // NOTHING inside renders only its image. moss suppressed the title for
    // both, so a full-bleed cover — the commonest hero there is — produced a
    // page with no visible heading at all. One site adopted covers site-wide
    // and lost the title on 87 pages; nothing failed, because `<title>`, the
    // OG tags and RSS resolve the same text by other paths.
    let md = "---\ntitle: A House of Daowu\n---\n\n:::hero {image=assets/cover.jpg}\n:::\n\nBody.\n";
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "work/daowu.md",
        md,
        "site",
        &empty_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    assert!(
        doc.html_content.contains("moss-article-title"),
        "an empty hero renders no title, so moss must inject one, got: {}",
        doc.html_content
    );
    assert!(
        doc.html_content.contains("A House of Daowu"),
        "and it must be the page's own title, got: {}",
        doc.html_content
    );
}

#[test]
fn split_path_suffix_query_only() {
    assert_eq!(
        split_path_suffix("foo.html?a=1&b=2"),
        ("foo.html", Some("?a=1&b=2"))
    );
}

#[test]
fn split_path_suffix_query_then_fragment() {
    assert_eq!(
        split_path_suffix("foo.html?a=1#sec"),
        ("foo.html", Some("?a=1#sec"))
    );
}

#[test]
fn split_path_suffix_fragment_then_query() {
    // Nonstandard order — first delimiter wins, rest is opaque.
    assert_eq!(
        split_path_suffix("foo.html#sec?a=1"),
        ("foo.html", Some("#sec?a=1"))
    );
}

#[test]
fn split_path_suffix_neither() {
    assert_eq!(split_path_suffix("foo.html"), ("foo.html", None));
}

/// End-to-end: a `[text](file.html?query)` link, after passing through
/// moss-core's markdown_links resolver, arrives here as `moss-resolved:...`
/// with the query attached. The pipeline must look up by path (without
/// the query) and reattach the query to the output href.
///
/// Two cases: target IS in page_map (markdown page) and NOT in page_map
/// (HTML asset — real-world case for the original bug).
#[test]
fn moss_resolved_link_preserves_query_when_target_in_page_map() {
    let md = "---\ntitle: t\n---\n\n[demo](moss-resolved:other.md?a=1&b=2)\n";
    let mut page_map = HashMap::new();
    page_map.insert("other.md".to_string(), "/other.html".to_string());
    page_map.insert("page.md".to_string(), "/page.html".to_string());

    let doc = process_markdown_file(
        "page.md",
        md,
        "site",
        &page_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");

    // `&` is HTML-entity-encoded inside href (correct per HTML spec).
    assert!(
        doc.html_content.contains("?a=1&amp;b=2"),
        "query dropped from in-page-map target. html: {}",
        doc.html_content
    );
}

/// HTML assets are NOT in page_map (only markdown files are). The href
/// must be relative to the page's served URL directory, which is one
/// level deeper than the source for non-index pages. Regression for the
/// 刘果 vault `音阶对比.md` 404: source `交互/音阶对比.md` (url:
/// scale-compare) serves at `interactive/scale-compare/index.html`. The href
/// must reach `assets/scale-compare.html` from there. It used to do that by
/// counting `../` from the source file and then adding one more for pretty-URL
/// nesting; it now emits the asset's pinned URL, which no depth can get wrong.
/// The `?query` still rides along untouched.
#[test]
fn moss_resolved_link_to_html_asset_uses_pinned_url_regardless_of_page_depth() {
    let md = "---\ntitle: t\n---\n\n[demo](moss-resolved:assets/scale-compare.html?a=major_pent&r=major_pent%3AD)\n";
    let mut page_map = HashMap::new();
    page_map.insert(
        "interactive/scale-compare.md".to_string(),
        "interactive/scale-compare/index.html".to_string(),
    );

    let doc = process_markdown_file(
        "interactive/scale-compare.md",
        md,
        "site",
        &page_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");

    assert!(
        doc.html_content
            .contains("/assets/scale-compare.html?a=major_pent&amp;r=major_pent%3AD"),
        "expected the pinned /assets/scale-compare.html?... but got: {}",
        doc.html_content
    );
}

/// Edge cases for the asset-fall-through depth math: covers index source,
/// deeply nested source, same-directory asset, and root-level index.
#[test]
fn moss_resolved_link_to_html_asset_depth_table() {
    // The cases that used to prove the depth math now prove there is none: the
    // href is the asset's pinned URL, identical for an index source, a
    // deeply-nested article, and the root home. Each `expected` below is just
    // `/` + the target — read the pairs and notice the target and the href are
    // the same string. What this replaced: four different `../` counts, derived
    // from the source path and then corrected again by `is_index_file`.
    //
    // (source_path, source_url, target_asset, expected_href_substring, label)
    let cases: &[(&str, &str, &str, &str, &str)] = &[
        (
            "a/b/c/page.md",
            "a/b/c/page/index.html",
            "a/b/assets/x.html",
            "/a/b/assets/x.html",
            "deeply-nested non-index",
        ),
        (
            "posts/index.md",
            "posts/index.html",
            "posts/assets/x.html",
            "/posts/assets/x.html",
            "index source linking to nested asset",
        ),
        (
            "posts/page.md",
            "posts/page/index.html",
            "posts/sibling.html",
            "/posts/sibling.html",
            "asset in same directory as source",
        ),
        (
            "index.md",
            "index.html",
            "assets/x.html",
            "/assets/x.html",
            "root index linking to nested asset",
        ),
    ];

    for (src, url, asset, expected, label) in cases {
        let md = format!("---\ntitle: t\n---\n\n[demo](moss-resolved:{})\n", asset);
        let mut page_map = HashMap::new();
        page_map.insert(src.to_string(), url.to_string());
        let doc = process_markdown_file(
            src,
            &md,
            "site",
            &page_map,
            false,
            Language::En,
            None,
            crate::build::markdown::SiteMarkdown::default(),
            None,
            None,
            None,
            None,
            false,
            None, // seta_url
            None, // folder_lang
        )
        .expect("should parse");
        assert!(
            doc.html_content.contains(expected),
            "{}: expected href to contain '{}', got: {}",
            label,
            expected,
            doc.html_content
        );
    }
}

/// Asset links (non-page-map targets) open in a new tab so the reader
/// doesn't lose their place when clicking through to a tool/PDF/etc.
#[test]
fn moss_resolved_link_to_html_asset_opens_in_new_tab() {
    let md = "---\ntitle: t\n---\n\n[demo](moss-resolved:assets/x.html)\n";
    let mut page_map = HashMap::new();
    page_map.insert("page.md".to_string(), "page/index.html".to_string());

    let doc = process_markdown_file(
        "page.md",
        md,
        "site",
        &page_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");

    assert!(
        doc.html_content.contains("target=\"_blank\""),
        "asset link should have target=_blank. got: {}",
        doc.html_content
    );
    assert!(
        doc.html_content.contains("rel=\"noopener\""),
        "asset link should have rel=noopener. got: {}",
        doc.html_content
    );
    // The href itself must still be intact — the target's pinned URL.
    assert!(
        doc.html_content.contains("href=\"/assets/x.html\""),
        "asset link href missing or wrong. got: {}",
        doc.html_content
    );
    // The sentinel must NOT leak into the final HTML.
    assert!(
        !doc.html_content.contains("moss-newtab:"),
        "moss-newtab: sentinel leaked. got: {}",
        doc.html_content
    );
}

/// Markdown-page links (in page_map) stay in the same tab — they're
/// part of the reading flow.
#[test]
fn moss_resolved_link_to_markdown_page_stays_same_tab() {
    let md = "---\ntitle: t\n---\n\n[other](moss-resolved:other.md)\n";
    let mut page_map = HashMap::new();
    page_map.insert("other.md".to_string(), "/other.html".to_string());
    page_map.insert("page.md".to_string(), "/page.html".to_string());

    let doc = process_markdown_file(
        "page.md",
        md,
        "site",
        &page_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");

    assert!(
        !doc.html_content.contains("target=\"_blank\""),
        "markdown-page link should not have target=_blank. got: {}",
        doc.html_content
    );
}

/// Same target, but from a root-level page (no parent dir): href should
/// be `../assets/scale-compare.html` (one `../` for pretty-URL nesting).
#[test]
fn moss_resolved_link_to_html_asset_from_root_page() {
    let md = "---\ntitle: t\n---\n\n[demo](moss-resolved:assets/scale-compare.html)\n";
    let mut page_map = HashMap::new();
    page_map.insert("page.md".to_string(), "page/index.html".to_string());

    let doc = process_markdown_file(
        "page.md",
        md,
        "site",
        &page_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");

    assert!(
        doc.html_content.contains("/assets/scale-compare.html"),
        "expected ../assets/... but got: {}",
        doc.html_content
    );
}

/// Regression for 刘果 vault `音阶对比.md`:
/// `[![[scale-compare.png]]](scale-compare.html?a=...)` — markdown link
/// wrapping a wikilink-image. After the wikilinks pass converts
/// `![[scale-compare.png]]` to `![alt](path)` and markdown_links rewrites
/// the outer target to `moss-resolved:`, the pulldown-cmark output must
/// keep the query string in the final href.
#[test]
fn nested_image_link_with_query_renders_correctly() {
    let md = "---\ntitle: t\n---\n\n[![scale-compare](assets/scale-compare.png)](moss-resolved:assets/scale-compare.html?a=major_pent&r=major_pent%3AD)\n";
    let mut page_map = HashMap::new();
    page_map.insert(
        "interactive/scale-compare.md".to_string(),
        "interactive/scale-compare/index.html".to_string(),
    );

    let doc = process_markdown_file(
        "interactive/scale-compare.md",
        md,
        "site",
        &page_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");

    assert!(
        doc.html_content
            .contains("/assets/scale-compare.html?a=major_pent&amp;r=major_pent%3AD"),
        "expected the pinned /assets/scale-compare.html?... in href. got: {}",
        doc.html_content
    );
}

/// Filename language suffix still wins over the site default.
#[test]
fn filename_suffix_overrides_site_default() {
    let md = "---\ntitle: A note\n---\n\n短文。";
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "notes/note.en.md",
        md,
        "site",
        &empty_map,
        false,
        Language::ZhHans,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    assert_eq!(doc.lang, Language::En);
}

/// `is_index_file` must stay true for a folder index that has a
/// frontmatter slug override. After the issue-#587 promotion fix, an
/// earlier draft used "page_map URL parent equals source parent" as
/// the only signal, which would silently regress this case:
/// `posts/index.md` with `url: blog` produces page_map URL
/// `blog/index.html`, whose parent is `blog`, not `posts` — would
/// have flipped is_index to false. The current rule keeps the
/// filename-based detection as primary.
#[test]
fn folder_index_with_slug_override_is_still_index() {
    let md = "---\nurl: blog\n---\n\n# Posts\n";
    let mut page_map = HashMap::new();
    page_map.insert("posts/index.md".to_string(), "blog/index.html".to_string());
    let doc = process_markdown_file(
        "posts/index.md",
        md,
        "site",
        &page_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    assert_eq!(
        doc.kind,
        moss_core::PageKind::Folder,
        "posts/index.md is a folder index regardless of slug override"
    );
}

/// The `home: true` marker promotes a non-INDEX_STEM file to be its
/// folder's home — the translated index page lands at
/// `<folder>/index.html` and is treated as a folder index by the
/// pipeline. Issue #587.
#[test]
fn home_override_is_index_via_page_map() {
    let md = "---\ntitle: Liu Guo\nlang: en\nhome: true\n---\n\n# Hello\n";
    let mut page_map = HashMap::new();
    page_map.insert("en/Liu Guo.md".to_string(), "en/index.html".to_string());
    let doc = process_markdown_file(
        "en/Liu Guo.md",
        md,
        "site",
        &page_map,
        false,
        Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    assert_eq!(
        doc.kind,
        moss_core::PageKind::Folder,
        "en/Liu Guo.md should be detected as a folder index after translation-home promotion"
    );
}

#[test]
fn test_pipeline_sets_features_inline_subscribe_when_shortcode_present() {
    let content = "---\ntitle: Test\n---\n\n# Heading\n\n:::subscribe\n:::\n";
    let page_map = std::collections::HashMap::new();
    let doc = process_markdown_file(
        "test.md",
        content,
        "root",
        &page_map,
        false,
        crate::i18n::Language::En,
        Some("test-site"),
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("pipeline should succeed");
    assert!(doc.features.inline_subscribe, "flag must be set");
    assert!(doc.html_content.contains("moss-subscribe"));
    assert!(
        doc.html_content
            .contains(r#"action="https://api.mosspub.com/sites/test-site/subscribe""#),
        "site_id must flow into the form action"
    );
}

#[test]
fn test_pipeline_features_default_false_without_shortcode() {
    let content = "# Just a heading\n\nNo shortcode here.";
    let page_map = std::collections::HashMap::new();
    let doc = process_markdown_file(
        "test.md",
        content,
        "root",
        &page_map,
        false,
        crate::i18n::Language::En,
        Some("test-site"),
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("pipeline should succeed");
    assert!(!doc.features.inline_subscribe);
    assert!(!doc.features.inline_apply);
    assert!(!doc.features.scroll_rows);
}

/// `scroll_rows` gates `scroll-row.js`: set by a `{scroll}` grid, including
/// one nested in a fenced div (the embed/wrapper shape), never by a plain grid.
#[test]
fn test_pipeline_sets_scroll_rows_only_for_a_scrolling_grid() {
    let parse = |content: &str| {
        process_markdown_file(
            "test.md",
            content,
            "root",
            &std::collections::HashMap::new(),
            false,
            crate::i18n::Language::En,
            Some("test-site"),
            crate::build::markdown::SiteMarkdown::default(),
            None,
            None,
            None,
            None,
            false,
            None,
            None,
        )
        .expect("pipeline should succeed")
    };
    let nested = parse("::::{.wrap}\n:::grid 3 {scroll}\na\n+++\nb\n:::\n::::\n");
    assert!(nested.features.scroll_rows, "nested scroll row must set the flag");
    let plain = parse(":::grid 3\na\n+++\nb\n:::\n");
    assert!(!plain.features.scroll_rows, "a wrapping grid must not set it");
}

#[test]
fn test_pipeline_sets_features_inline_apply_when_shortcode_present() {
    let content = "---\ntitle: Apply\n---\n\n# Join\n\n:::apply\n:::\n";
    let page_map = std::collections::HashMap::new();
    let doc = process_markdown_file(
        "apply.md",
        content,
        "root",
        &page_map,
        false,
        crate::i18n::Language::ZhHans,
        Some("test-site"),
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        Some("https://api.mosspub.com"), // seta_url
        None, // folder_lang
    )
    .expect("pipeline should succeed");
    assert!(doc.features.inline_apply, "inline_apply flag must be set");
    assert!(
        doc.html_content.contains("moss-apply-form"),
        "html must contain apply form: {}",
        &doc.html_content[..doc.html_content.len().min(500)]
    );
    assert!(
        doc.html_content
            .contains(r#"action="https://api.mosspub.com/apply?lang=zh-hans""#),
        "action must point to seta /apply: {}",
        &doc.html_content[..doc.html_content.len().min(500)]
    );
}

#[test]
fn test_pipeline_no_site_id_yields_pending_form() {
    // Preview / no-deploy-config case. With no site_id the :::subscribe
    // shortcode renders the PENDING form (action="#" + data-moss-pending-site),
    // consistent with the auto-injected footer form on unpublished sites —
    // instead of the old degraded wired form posting to /sites//subscribe.
    let content = "---\ntitle: T\n---\n\n:::subscribe\n:::\n";
    let page_map = std::collections::HashMap::new();
    let doc = process_markdown_file(
        "test.md",
        content,
        "root",
        &page_map,
        false,
        crate::i18n::Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .unwrap();
    assert!(doc.features.inline_subscribe);
    assert!(
        doc.html_content.contains(r##"action="#""##),
        "unpublished :::subscribe → pending action: {}",
        doc.html_content
    );
    assert!(
        doc.html_content
            .contains(r#"data-moss-pending-site="true""#),
        "unpublished :::subscribe → pending marker: {}",
        doc.html_content
    );
}

/// 2026-05-28 (Phase 4 source-line wiring): when `emit_source_lines=true`,
/// the production `process_markdown_file` path must emit
/// `data-source-line="N"` attributes on top-level blocks. Restores the
/// preview-scroll paragraph-precision feature that PR7a's
/// `transform_events` deletion temporarily disabled.
///
/// The downstream consumer is `frontend/bridge/iframe-bridge.ts`'s
/// `scrollToSourceLine` RPC, which queries
/// `[data-source-line], [data-source-range]` and scrolls to the
/// element whose source line ≤ target line.
#[test]
fn process_markdown_file_emits_data_source_line_when_flag_on() {
    let content = "# Heading\n\nfirst paragraph\n\n## Sub\n\nsecond paragraph\n";
    let page_map = std::collections::HashMap::new();
    let doc = process_markdown_file(
        "test.md",
        content,
        "root",
        &page_map,
        true, // emit_source_lines
        crate::i18n::Language::En,
        None,
        crate::build::markdown::SiteMarkdown { implicit_figure: true, ..Default::default() },
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("pipeline should succeed");
    // Heading (line 1 of the body markdown, which is line 1 of the
    // post-frontmatter content slice).
    assert!(
        doc.html_content.contains(r#"data-source-line="1""#),
        "expected data-source-line=\"1\" on H1 (or injected title), got: {}",
        doc.html_content
    );
    // First paragraph appears at line 3.
    assert!(
        doc.html_content
            .contains(r#"<p data-source-line="3">first paragraph</p>"#),
        "first paragraph should carry data-source-line=3, got: {}",
        doc.html_content
    );
    // Second heading at line 5.
    assert!(
        doc.html_content.contains(r#"data-source-line="5""#),
        "second heading should carry data-source-line=5, got: {}",
        doc.html_content
    );
    // Second paragraph at line 7.
    assert!(
        doc.html_content
            .contains(r#"<p data-source-line="7">second paragraph</p>"#),
        "second paragraph should carry data-source-line=7, got: {}",
        doc.html_content
    );
}

/// BUG 3 regression: a malformed `---...---` YAML block (two keys collapsed
/// onto one line, the 'Europe - A Prophecy.md' corruption) must NOT leak
/// verbatim into the rendered HTML. Before the fix, `parsed.body` held the
/// whole document (delimiters + raw YAML) and flowed into
/// `markdown_content_raw` → HTML. Now the build renders `render_body()`,
/// which excludes the failed block.
#[test]
fn process_markdown_file_malformed_yaml_does_not_leak_block() {
    let content =
            "---\nchildren_style: grid\nseries: true\nweight: 10\nuid: blk-europecover: \"006.jpg\"\n---\n\n\nreal body text\n";
    let page_map = std::collections::HashMap::new();
    let doc = process_markdown_file(
        "europe.md",
        content,
        "root",
        &page_map,
        false, // emit_source_lines
        crate::i18n::Language::En,
        None,
        crate::build::markdown::SiteMarkdown { implicit_figure: true, ..Default::default() },
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("pipeline should succeed on malformed frontmatter");
    assert!(
        !doc.html_content.contains("uid:"),
        "raw YAML must not leak into HTML, got: {}",
        doc.html_content
    );
    assert!(
        !doc.html_content.contains("---"),
        "frontmatter delimiters must not leak into HTML, got: {}",
        doc.html_content
    );
    assert!(
        doc.html_content.contains("real body text"),
        "the actual body must still render, got: {}",
        doc.html_content
    );
}

/// BUG 3 build-warning surface: with the no-leak fix the malformed block
/// silently vanishes from output, so the warning is the ONLY signal a build
/// user gets that content was dropped. Assert the pure warning builder fires
/// for a malformed block (and is silent for a clean one) so a regression that
/// drops the `cli_eprintln!` is caught.
#[test]
fn frontmatter_invalid_yaml_warning_fires_only_on_malformed_block() {
    // Same corruption as 'Europe - A Prophecy.md': two keys collapsed onto
    // one line by a deleted newline.
    let malformed = "---\nuid: blk-europecover: \"006.jpg\"\n---\n\nbody\n";
    let parsed = moss_core::frontmatter::parse(malformed);
    let warning = frontmatter_invalid_yaml_warning("europe.md", &parsed)
        .expect("malformed YAML must produce a build warning");
    assert!(
        warning.contains("invalid YAML"),
        "warning must name the failure, got: {warning}"
    );
    assert!(
        warning.contains("europe.md"),
        "warning must name the file, got: {warning}"
    );

    // A clean block produces no warning (positive control).
    let clean = moss_core::frontmatter::parse("---\ntitle: Hi\n---\n\nbody\n");
    assert!(
        frontmatter_invalid_yaml_warning("ok.md", &clean).is_none(),
        "a valid frontmatter block must not warn"
    );
}

/// #771 edge (b): the source-line-offset search must NOT match a body line
/// against an identical line INSIDE the frontmatter, or the offset is
/// under-counted and every annotation lands too high (into the frontmatter
/// region). Reachable via the simplified-frontmatter path, which silently
/// ignores a bare unknown-flag line — here `Welcome` at column 0 sits in the
/// frontmatter AND opens the body. With the `skip(frontmatter_line_count)`
/// guard the body `Welcome` resolves to its real file line (5), not 2.
#[test]
fn data_source_line_skips_frontmatter_collision() {
    // Simplified frontmatter (no leading `---`, a standalone `---` later).
    // Line 1 `title:` (parsed), line 2 bare `Welcome` (ignored unknown
    // flag), line 3 `---` (end). Body: blank, `Welcome`, blank, `body text`.
    let content = "title: My Post\nWelcome\n---\n\nWelcome\n\nbody text\n";
    let page_map = std::collections::HashMap::new();
    let doc = process_markdown_file(
        "post.md",
        content,
        "root",
        &page_map,
        true, // emit_source_lines
        crate::i18n::Language::En,
        None,
        crate::build::markdown::SiteMarkdown { implicit_figure: true, ..Default::default() },
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("pipeline should succeed");
    // Body `Welcome` is file line 5; `body text` is file line 7. Without the
    // skip, the collision pins the offset to 0 and these would be 2 and 4.
    assert!(
        doc.html_content
            .contains(r#"<p data-source-line="5">Welcome</p>"#),
        "body `Welcome` must map to its real file line 5 (not a frontmatter \
             collision), got: {}",
        doc.html_content
    );
    assert!(
        doc.html_content
            .contains(r#"<p data-source-line="7">body text</p>"#),
        "`body text` must map to its real file line 7, got: {}",
        doc.html_content
    );
}

/// Companion on the TRADITIONAL `---`-fenced path. The editor's
/// `frontmatter::parse` STRIPS the fenced block, so CM6 line 1 is the body's
/// first line — annotations are BODY-relative, not raw-file-relative. The 3
/// frontmatter lines are cancelled out by the editor-strip subtraction, so
/// `first body` (raw line 5) is annotated at its CM6 body line 2 and
/// `second body` (raw line 7) at body line 4. (Superset assertion of the
/// coordinate invariant in `data_source_line_matches_editor_cm6_body_line`.)
#[test]
fn data_source_line_offset_traditional_yaml_frontmatter() {
    // Raw lines: 1 `---`, 2 `title: My Post`, 3 `---`, 4 blank, 5 `first
    // body`, 6 blank, 7 `second body`. Editor CM6 body (fm stripped):
    // 1 blank, 2 `first body`, 3 blank, 4 `second body`.
    let content = "---\ntitle: My Post\n---\n\nfirst body\n\nsecond body\n";
    let page_map = std::collections::HashMap::new();
    let doc = process_markdown_file(
        "post.md",
        content,
        "root",
        &page_map,
        true, // emit_source_lines
        crate::i18n::Language::En,
        None,
        crate::build::markdown::SiteMarkdown { implicit_figure: true, ..Default::default() },
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("pipeline should succeed");
    assert!(
        doc.html_content
            .contains(r#"<p data-source-line="2">first body</p>"#),
        "traditional-YAML body must map to its CM6 body line 2 (frontmatter \
             stripped by the editor), got: {}",
        doc.html_content
    );
    assert!(
        doc.html_content
            .contains(r#"<p data-source-line="4">second body</p>"#),
        "`second body` must map to its CM6 body line 4, got: {}",
        doc.html_content
    );
}

/// 1-based line number of the first occurrence of `needle` in `haystack`,
/// counting `\n`. Test helper for the editor-coordinate invariant below.
fn line_of(haystack: &str, needle: &str) -> usize {
    let idx = haystack.find(needle).expect("needle present");
    haystack[..idx].matches('\n').count() + 1
}

/// THE editor↔preview coordinate invariant: a block's `data-source-line`
/// MUST equal the line the editor's CM6 buffer shows it on. The editor
/// loads `frontmatter::parse(content).body` (frontmatter-stripped) — NOT
/// the raw file — so annotations must be BODY-relative. This is the
/// property the whole scroll-sync + click-to-source depends on; the older
/// "REAL FILE lines" model (commit 52a564963) assumed the editor held the
/// raw file (frontmatter + body), which it never has (the editor has been
/// body-only since its first commit, 7b4c79b30). A raw-relative annotation
/// is off by the frontmatter line count → "preview always lower".
#[test]
fn data_source_line_matches_editor_cm6_body_line() {
    let content = "---\ntitle: My Post\n---\n\nfirst body\n\nsecond body\n";

    // What the EDITOR actually loads into CM6 (see editor-main.ts:
    // `new CmEditor({ content: body })`, body from `parse_frontmatter`).
    let editor_body = moss_core::frontmatter::parse(content).body;
    let editor_line_first = line_of(&editor_body, "first body");
    let editor_line_second = line_of(&editor_body, "second body");

    let page_map = std::collections::HashMap::new();
    let doc = process_markdown_file(
        "post.md",
        content,
        "root",
        &page_map,
        true, // emit_source_lines
        crate::i18n::Language::En,
        None,
        crate::build::markdown::SiteMarkdown { implicit_figure: true, ..Default::default() },
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("pipeline should succeed");

    assert!(
        doc.html_content.contains(&format!(
            r#"<p data-source-line="{editor_line_first}">first body</p>"#
        )),
        "`first body` must be annotated with the editor's CM6 body line \
             {editor_line_first}, got: {}",
        doc.html_content
    );
    assert!(
        doc.html_content.contains(&format!(
            r#"<p data-source-line="{editor_line_second}">second body</p>"#
        )),
        "`second body` must be annotated with the editor's CM6 body line \
             {editor_line_second}, got: {}",
        doc.html_content
    );
}

/// Regression guard for the MALFORMED-YAML frontmatter path (the case a
/// `frontmatter_range`-based strip got wrong). When the delimited block
/// fails to parse, `frontmatter::parse` returns `frontmatter_range = Some`
/// but `body = the WHOLE file` — the editor loads the whole file so the
/// author can repair the bad block, stripping NOTHING. The annotation must
/// therefore stay at the real-file line the editor actually shows, NOT be
/// shifted down by the (would-be) frontmatter length.
#[test]
fn data_source_line_matches_editor_on_malformed_frontmatter() {
    // `uid: blk: "x"` is invalid YAML (mapping value inside a scalar) —
    // mirrors the shipped William-Blake malformed-frontmatter case.
    let content = "---\nuid: blk: \"x\"\n---\n\nHello\n";

    let editor_body = moss_core::frontmatter::parse(content).body;
    // The editor buffer is the WHOLE file → `Hello` sits at real line 5.
    let editor_line = line_of(&editor_body, "Hello");
    assert_eq!(
        editor_line, 5,
        "precondition: editor shows Hello at file line 5"
    );

    let page_map = std::collections::HashMap::new();
    let doc = process_markdown_file(
        "post.md",
        content,
        "root",
        &page_map,
        true,
        crate::i18n::Language::En,
        None,
        crate::build::markdown::SiteMarkdown { implicit_figure: true, ..Default::default() },
        None,
        None,
        None,
        None,
        false,
        None,
        None, // folder_lang
    )
    .expect("pipeline should succeed");

    assert!(
        doc.html_content
            .contains(&format!(r#"data-source-line="{editor_line}">Hello"#)),
        "on malformed frontmatter the editor strips nothing, so `Hello` must \
             stay at its real-file line {editor_line} (not shifted down), got: {}",
        doc.html_content
    );
}

/// Companion invariant on the SIMPLIFIED `key: value` path (no leading
/// `---`). The editor's `frontmatter::parse` recognizes no block there and
/// loads the whole file, so annotations correctly stay real-file-relative
/// (`editor_stripped_lines == 0`) — the asymmetric counterpart to the
/// `---`-fenced `data_source_line_matches_editor_cm6_body_line`.
#[test]
fn data_source_line_matches_editor_on_simplified_frontmatter() {
    let content = "title: My Post\nuid: p1\n---\n\nbody one\n\nbody two\n";

    let editor_body = moss_core::frontmatter::parse(content).body;
    // No leading `---` → parse detects no frontmatter → body is the whole
    // file, so the editor shows these at their real file lines.
    let editor_line_one = line_of(&editor_body, "body one");
    let editor_line_two = line_of(&editor_body, "body two");

    let page_map = std::collections::HashMap::new();
    let doc = process_markdown_file(
        "post.md",
        content,
        "root",
        &page_map,
        true,
        crate::i18n::Language::En,
        None,
        crate::build::markdown::SiteMarkdown { implicit_figure: true, ..Default::default() },
        None,
        None,
        None,
        None,
        false,
        None,
        None, // folder_lang
    )
    .expect("pipeline should succeed");

    assert!(
        doc.html_content.contains(&format!(
            r#"<p data-source-line="{editor_line_one}">body one</p>"#
        )),
        "simplified-frontmatter `body one` should equal the editor's whole-file \
             line {editor_line_one}, got: {}",
        doc.html_content
    );
    assert!(
        doc.html_content.contains(&format!(
            r#"<p data-source-line="{editor_line_two}">body two</p>"#
        )),
        "simplified-frontmatter `body two` should equal the editor's whole-file \
             line {editor_line_two}, got: {}",
        doc.html_content
    );
}

/// Companion to the above: when `emit_source_lines=false` (publish
/// builds), NO `data-source-line` attributes are emitted. The
/// ship-stage strip pass is a defense-in-depth net, not the primary
/// gate — the production page render should already be clean.
#[test]
fn process_markdown_file_omits_data_source_line_when_flag_off() {
    let content = "# H\n\npara\n";
    let page_map = std::collections::HashMap::new();
    let doc = process_markdown_file(
        "test.md",
        content,
        "root",
        &page_map,
        false, // emit_source_lines OFF
        crate::i18n::Language::En,
        None,
        crate::build::markdown::SiteMarkdown { implicit_figure: true, ..Default::default() },
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("pipeline should succeed");
    assert!(
        !doc.html_content.contains("data-source-line"),
        "publish build must not emit data-source-line, got: {}",
        doc.html_content
    );
}

/// Task E1 (source-annotation-completeness): shortcode blocks carry a
/// point `data-source-range="N-N"` on their outermost element when
/// `emit_source_lines=true`, so the editor bridge's `resolveSourceTarget`
/// routes a click on a `:::grid` / `:::hero` back to its source line.
/// Grid renders inline (Hero is hoisted to the heading slot), so the
/// Grid path exercises the body-render annotation end-to-end.
#[test]
fn process_markdown_file_emits_data_source_range_on_shortcode_when_flag_on() {
    let content = "intro\n\n:::grid 2\nleft\nright\n:::\n";
    let page_map = std::collections::HashMap::new();
    let doc = process_markdown_file(
        "test.md",
        content,
        "root",
        &page_map,
        true, // emit_source_lines
        crate::i18n::Language::En,
        None,
        crate::build::markdown::SiteMarkdown { implicit_figure: true, ..Default::default() },
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("pipeline should succeed");
    // The :::grid opener is on body line 3.
    assert!(
        doc.html_content.contains(r#"data-source-range="3-3""#),
        "grid should carry a point source range on body line 3, got: {}",
        doc.html_content
    );
}

/// Companion: when `emit_source_lines=false` (publish builds) no
/// `data-source-range` is emitted on shortcode blocks.
#[test]
fn process_markdown_file_omits_data_source_range_when_flag_off() {
    let content = "intro\n\n:::grid 2\nleft\nright\n:::\n";
    let page_map = std::collections::HashMap::new();
    let doc = process_markdown_file(
        "test.md",
        content,
        "root",
        &page_map,
        false, // emit_source_lines OFF
        crate::i18n::Language::En,
        None,
        crate::build::markdown::SiteMarkdown { implicit_figure: true, ..Default::default() },
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("pipeline should succeed");
    assert!(
        !doc.html_content.contains("data-source-range"),
        "publish build must not emit data-source-range, got: {}",
        doc.html_content
    );
}

/// Regression test for the moss-releases Documentation button bug:
/// `:::buttons` with an internal link `[Documentation](docs/)` emitted
/// `<a href="moss-resolved:docs/index.md">` (broken — preview-only URL
/// scheme that 404s in published output) instead of `<a href="docs/">`.
///
/// Root cause: moss-core's resolve pipeline rewrites markdown link
/// sources to `moss-resolved:<resolved-path>` BEFORE shortcodes run.
/// Shortcodes that emit raw HTML (like :::buttons) bypass the markdown
/// parser and the `resolve_link` closure that would have stripped the
/// prefix. This test pins the post-pass that catches the leakage.
///
/// If this test fails, it likely means a shortcode is emitting a
/// `<a href="moss-resolved:...">` and the `sweep_unresolved_hrefs`
/// post-pass is either disabled or has a regression.
#[test]
fn buttons_internal_link_does_not_leak_moss_resolved_prefix() {
    let content = "---\ntitle: t\n---\n\n:::buttons\n[Documentation](docs/)\n[Releases](https://example.com)\n:::\n";
    let mut page_map = std::collections::HashMap::new();
    // Register `docs/index.md` as a known page so the resolver maps
    // `docs/` → `moss-resolved:docs/index.md` (the shape the bug needs).
    page_map.insert("docs/index.md".to_string(), "docs".to_string());
    let doc = process_markdown_file(
        "index.md",
        content,
        "site",
        &page_map,
        false,
        crate::i18n::Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    assert!(
        !doc.html_content.contains("moss-resolved:"),
        "no `moss-resolved:` prefix should survive into rendered HTML — \
             buttons emit raw HTML and the sweep_unresolved_hrefs post-pass \
             must catch the leakage. Got: {}",
        doc.html_content
    );
    // External link should be untouched by the sweep.
    assert!(
        doc.html_content.contains(r#"href="https://example.com""#),
        "external link should pass through unchanged: {}",
        doc.html_content
    );
}

/// Pipeline-level invariant: no resolver-prefix leaks into the final
/// HTML, regardless of which shortcode introduces the markdown link.
/// This is the "wide net" — if any future shortcode forgets to thread
/// `resolve_link`, this test catches it.
#[test]
fn pipeline_invariant_no_resolver_prefix_leaks() {
    // Mix of shortcodes, all containing internal links that the
    // moss-core resolver rewrites to `moss-resolved:...`.
    let content = r#"---
title: t
---

:::buttons
[Docs](docs/)
:::

Inline link to [extend](docs/extend/) for comparison.
"#;
    let mut page_map = std::collections::HashMap::new();
    page_map.insert("docs/index.md".to_string(), "docs".to_string());
    page_map.insert("docs/extend.md".to_string(), "docs/extend".to_string());
    let doc = process_markdown_file(
        "index.md",
        content,
        "site",
        &page_map,
        false,
        crate::i18n::Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    for prefix in ["moss-resolved:", "wikilink:", "moss-newtab:"] {
        assert!(
            !doc.html_content.contains(prefix),
            "rendered HTML must not contain resolver prefix `{}` — \
                 a shortcode is emitting unresolved hrefs and the sweep \
                 didn't catch it. Got: {}",
            prefix,
            doc.html_content
        );
    }
}

/// `sweep_unresolved_hrefs` is idempotent on already-clean HTML.
/// (Defensive: the function should be a no-op if there's nothing to fix.)
#[test]
fn sweep_unresolved_hrefs_is_idempotent_on_clean_html() {
    let resolve = |href: &str| href.to_string();
    let html = r#"<p>See <a href="docs/">docs</a>.</p>"#;
    assert_eq!(sweep_unresolved_hrefs(html, &resolve), html);
}

/// `sweep_unresolved_hrefs` rewrites `moss-resolved:` hrefs by routing
/// them through the supplied `resolve_link` closure.
#[test]
fn sweep_unresolved_hrefs_routes_moss_resolved_through_resolver() {
    let resolve = |href: &str| {
        if href.starts_with("moss-resolved:") {
            "wikilink:docs/".to_string()
        } else {
            href.to_string()
        }
    };
    let html = r#"<a href="moss-resolved:docs/index.md">Docs</a>"#;
    let out = sweep_unresolved_hrefs(html, &resolve);
    assert!(
        out.contains(r#"class="wikilink" href="docs/""#),
        "wikilink prefix should expand into class+href; got: {}",
        out
    );
    assert!(
        !out.contains("moss-resolved:"),
        "moss-resolved: prefix must be gone; got: {}",
        out
    );
}

/// End-to-end smoke test for the Phase A observation path: a real
/// document goes through process_markdown_file (which now calls
/// observe_typed_ast internally in debug builds) without panicking
/// or producing unexpected diagnostics. Production HTML is unchanged.
#[test]
fn typed_ast_observation_runs_without_breaking_production() {
    let content = r#"---
title: Observation Test
---

# Heading

Para with [link](docs/) and *em* and `code`.

- list item one
- list item two

> a blockquote
"#;
    let mut page_map = std::collections::HashMap::new();
    page_map.insert("docs/index.md".to_string(), "docs".to_string());
    let doc = process_markdown_file(
        "index.md",
        content,
        "site",
        &page_map,
        false,
        crate::i18n::Language::En,
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse cleanly with observation path active");
    // Production HTML must still contain the canonical output —
    // the observation path is observation-only.
    assert!(
        doc.html_content.contains("<h1"),
        "missing H1: {}",
        doc.html_content
    );
    assert!(doc.html_content.contains("<p>"), "missing paragraph");
    assert!(doc.html_content.contains("<ul>"), "missing list");
    assert!(
        doc.html_content.contains("<blockquote>"),
        "missing blockquote"
    );
}

// === Figure-caption tests ===
//
// Cover the three figure detectors plus the new Phase A bare-image rule:
//   1. is_image_then_emphasis        — `![alt](src)*Caption*` same paragraph
//   2. peek_emphasis_paragraph       — `![alt](src)\n\n*Caption*` two paragraphs
//   3. bare_image_paragraph_alt      — `![Caption](src)` alone in paragraph
//                                       (Pandoc-style, gated by implicit_figure)
//
// All three share the empty-alt a11y guard: an image with empty alt does
// not produce `<figure>` even when a caption is supplied, so screen-reader
// users never get an undescribed image with a captioned wrapper.
//
// See docs/archive/2026-05-05-figure-captions-design.md.

/// Render with an explicit `[site].math` answer, everything else at
/// production defaults.
fn render_with_math(md: &str, math: bool) -> String {
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "test.md",
        md,
        "site",
        &empty_map,
        false,
        crate::i18n::Language::En,
        None,
        crate::build::markdown::SiteMarkdown { implicit_figure: true, math, hard_line_breaks: false, ..Default::default() },
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    doc.html_content
}

/// The `math` argument must reach `ParseConfig`, not just exist.
///
/// This is a WIRING test, and it is the one that matters: the moss-core
/// side already proves the parser handles math events, so a broken
/// thread here (param dropped, `math: false` left hardcoded in the
/// `ParseConfig` literal, `site_config.math` never passed at the
/// blocking.rs call site) leaves every moss-core test green while the
/// feature is off for every real build. Mutation check: restore
/// `math: false` in the `ParseConfig` literal and this goes red.
///
/// P2 typesets the equation to an inline `<svg class="moss-math"
/// data-moss-math="inline">` (the P1 `<code>` fallback only fires when
/// the render envelope refuses). Either way the emitted node carries the
/// `moss-math` class + `inline` marker and the TeX source survives — in
/// P2 via the SVG's `aria-label`/`<title>`, in the P1 fallback verbatim.
#[test]
fn site_math_on_renders_equations_through_the_pipeline() {
    let html = render_with_math("Energy $E = mc^2$ here.\n", true);
    assert!(
        html.contains(r#"class="moss-math""#) && html.contains(r#"data-moss-math="inline""#),
        "expected an inline moss-math node, got: {}",
        html
    );
    // The `aria-label` distinguishes math-ON (typeset, carrying the TeX
    // as its accessible name) from the math-OFF `$E = mc^2$` verbatim
    // passthrough — and doubles as the source-survival check: the
    // equation's own text is never dropped to a blank.
    assert!(
        html.contains(r#"aria-label="E = mc^2""#),
        "TeX source lost from accessible name: {}",
        html
    );
    assert!(html.contains("E = mc^2"), "TeX source lost: {}", html);
}

/// The opt-out has to actually opt out: with `[site].math = false`,
/// `$` is an ordinary character again and the text passes through
/// verbatim. Authors reach for this when their prose makes `$` pair up
/// (unspaced CJK price lists are the known false positive).
#[test]
fn site_math_off_leaves_dollar_signs_as_plain_text() {
    let html = render_with_math("Energy $E = mc^2$ here.\n", false);
    assert!(
        !html.contains("moss-math"),
        "math must not be parsed when [site].math is off: {}",
        html
    );
    assert!(
        html.contains("$E = mc^2$"),
        "expected the dollars verbatim, got: {}",
        html
    );
}

/// Render with an explicit `[site].hard_line_breaks` answer, everything
/// else at production defaults.
fn render_with_breaks(md: &str, hard_line_breaks: bool) -> String {
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "test.md",
        md,
        "site",
        &empty_map,
        false,
        crate::i18n::Language::En,
        None,
        crate::build::markdown::SiteMarkdown { implicit_figure: true, math: false, hard_line_breaks, ..Default::default() },
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    doc.html_content
}

/// The `hard_line_breaks` argument must reach `ParseConfig`, not just
/// exist — same wiring hazard as `math` above: a dropped thread (param
/// unused, `hard_line_breaks: false` left hardcoded in the `ParseConfig`
/// literal, `site_config.hard_line_breaks` never passed at the
/// blocking.rs call site) leaves every moss-core test green while every
/// real build stays CommonMark. Mutation check: restore
/// `hard_line_breaks: false` in the `ParseConfig` literal and this goes
/// red.
#[test]
fn site_hard_line_breaks_on_renders_br_through_the_pipeline() {
    let html = render_with_breaks("line one\nline two\n", true);
    assert!(
        html.contains("<br />"),
        "expected a <br> for the single newline (Obsidian parity), got: {}",
        html
    );
}

/// The opt-out has to actually opt out: with
/// `[site].hard_line_breaks = false`, CommonMark applies and the
/// newline is a space.
#[test]
fn site_hard_line_breaks_off_keeps_commonmark_soft_break() {
    let html = render_with_breaks("line one\nline two\n", false);
    assert!(
        !html.contains("<br"),
        "no <br> may appear when [site].hard_line_breaks is off: {}",
        html
    );
}

/// Render with an explicit `[site].heading_anchors` answer, everything
/// else at production defaults.
fn render_with_heading_anchors(heading_anchors: bool) -> String {
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "test.md",
        "# Title\n\nBody text.\n",
        "site",
        &empty_map,
        false,
        crate::i18n::Language::En,
        None,
        crate::build::markdown::SiteMarkdown { implicit_figure: true, heading_anchors, ..Default::default() },
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    doc.html_content
}

/// The `heading_anchors` argument must reach `RenderHooks::emit_heading_anchors`,
/// not just exist — same wiring hazard as `hard_line_breaks` above: a dropped
/// thread (param unused, `PipelineHooks::heading_anchors` never populated
/// from `site_config.heading_anchors` at the blocking.rs call site) leaves
/// every moss-core test green while every real build keeps emitting
/// permalink anchors regardless of the config toggle. Mutation check: hardcode
/// `emit_heading_anchors(&self) -> bool { true }` in `PipelineHooks` and this
/// goes red only for the off case below — this test guards the on case stays
/// unaffected by the wiring itself.
#[test]
fn site_heading_anchors_on_renders_permalink_anchor() {
    let html = render_with_heading_anchors(true);
    assert!(
        html.contains(r#"class="moss-heading-anchor""#),
        "expected the permalink anchor when [site].heading_anchors is on, got: {}",
        html
    );
}

/// The opt-out has to actually opt out: with `[site].heading_anchors = false`,
/// the trailing `#` permalink anchor must not render — but the heading text
/// itself must still be present, proving the heading rendered and only the
/// anchor was suppressed.
#[test]
fn site_heading_anchors_off_omits_permalink_anchor() {
    let html = render_with_heading_anchors(false);
    assert!(
        !html.contains("moss-heading-anchor"),
        "no permalink anchor may appear when [site].heading_anchors is off: {}",
        html
    );
    assert!(
        html.contains("Title"),
        "heading text must still render even without the anchor: {}",
        html
    );
}

/// `render_markdown_to_html_with` is the fragment renderer `Shortcode::Recent`
/// uses for its authored fallback_markdown (moss#915) — it must honor the
/// caller's `heading_anchors` value rather than hardcoding one, or a site
/// with `[site].heading_anchors = false` still leaks anchors from `:::recent`
/// fallback blocks.
#[test]
fn render_markdown_to_html_with_respects_heading_anchors_param() {
    let default_resolver = |href: &str| -> String { href.to_string() };

    let on = render_markdown_to_html_with("# Title\n", &default_resolver, None, true);
    assert!(
        on.contains(r#"class="moss-heading-anchor""#),
        "expected the permalink anchor when heading_anchors=true, got: {}",
        on
    );

    let off = render_markdown_to_html_with("# Title\n", &default_resolver, None, false);
    assert!(
        !off.contains("moss-heading-anchor"),
        "no permalink anchor may appear when heading_anchors=false, got: {}",
        off
    );
}

/// Parse with production defaults and return the doc's merged tag set.
fn tags_of(content: &str) -> Option<Vec<String>> {
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "test.md",
        content,
        "site",
        &empty_map,
        false,
        crate::i18n::Language::En,
        None,
        crate::build::markdown::SiteMarkdown { implicit_figure: true, math: false, hard_line_breaks: false, ..Default::default() },
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    doc.tags
}

/// Inline `#tags` (issue #649 P1) must actually reach `doc.tags` alongside
/// frontmatter `tags:` — frontmatter first, inline appended. The union is
/// made HERE at the document level, deliberately NOT in the folder cascade:
/// cascade's rule stays uniform child-overrides-folder (cascade.rs), so a
/// union there would silently change pages that have frontmatter tags and
/// no inline tags at all. Mutation check: restore `let tags =
/// frontmatter.tags;` in the pipeline and this goes red.
#[test]
fn inline_tags_merge_after_frontmatter_tags() {
    let tags = tags_of("---\ntags: [cooking]\n---\n\nDinner notes. #recipes\n");
    assert_eq!(
        tags,
        Some(vec!["cooking".to_string(), "recipes".to_string()])
    );
}

/// A doc with ONLY inline tags (frontmatter absent) still gets them — and
/// having any tags now correctly blocks folder-cascade tags under the
/// existing `if doc.tags.is_none()` cascade arm.
#[test]
fn inline_only_doc_surfaces_inline_tags() {
    let tags = tags_of("Dinner notes. #recipes\n");
    assert_eq!(tags, Some(vec!["recipes".to_string()]));
}

/// Case-insensitive membership: an inline duplicate of a frontmatter tag
/// is dropped, keeping the frontmatter form.
#[test]
fn frontmatter_form_wins_over_inline_duplicate() {
    let tags = tags_of("---\ntags: [Recipes]\n---\n\nDinner. #recipes #baking\n");
    assert_eq!(
        tags,
        Some(vec!["Recipes".to_string(), "baking".to_string()])
    );
}

/// No tags anywhere stays `None`, so the folder cascade still fills.
#[test]
fn no_tags_anywhere_stays_none_so_cascade_can_fill() {
    assert_eq!(tags_of("Dinner notes.\n"), None);
}

fn render(md: &str, implicit_figure: bool) -> String {
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "test.md",
        md,
        "site",
        &empty_map,
        false,
        crate::i18n::Language::En,
        None,
        crate::build::markdown::SiteMarkdown { implicit_figure, ..Default::default() },
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse");
    doc.html_content
}

#[test]
fn implicit_figure_wraps_bare_image_paragraph() {
    let html = render("![Morning light](photo.jpg)\n", true);
    assert!(
        html.contains("<figure ") && html.contains("<figcaption>Morning light</figcaption>"),
        "expected figure wrap, got: {}",
        html
    );
}

/// `implicit_figure: false` opt-out: bare image-only paragraphs stay
/// as plain `<img>` instead of being promoted to `<figure>`. Wired
/// through `ParseConfig.implicit_figure` (see crates/moss-core/src/
/// ast/parser.rs) — `process_markdown_file` threads the flag in.
/// The implicit-figure caption renders its alt as inline markdown —
/// `<em>`, links, and TYPESET math. Option B, matching Pandoc's
/// implicit-figure model. The moss-core parser builds the caption from the
/// image's parsed inline children; the pipeline's `render_math` hook
/// typesets the math node exactly as it does for body math.
///
/// The `alt=` attribute is empty here on purpose: the caption already says
/// this, so repeating it would make a screen reader read the sentence
/// twice (see `moss_core::ast::render`'s Figure arm). The `alt` STRING's
/// own shape — flat plain text, math as `$…$` — is asserted where it is
/// still observable, on the typed node: `crates/moss-core/tests/
/// math_parsing.rs::image_alt_and_caption_keep_the_equation`.
#[test]
fn implicit_figure_caption_typesets_math_and_leaves_alt_to_the_caption() {
    let html = render_with_math(
        "![before *em* and a [link](/x) and $x^2$ after](img.png)\n",
        true,
    );
    // Caption renders inline markup.
    assert!(
        html.contains("<em>em</em>"),
        "figcaption must render emphasis as <em>, got: {html}"
    );
    assert!(
        html.contains(">link</a>"),
        "figcaption must render the link, got: {html}"
    );
    // Math typesets to an SVG (P2), never the raw `$x^2$` source.
    assert!(
        html.contains(r#"class="moss-math""#) && html.contains("<svg"),
        "figcaption math must typeset to an <svg class=\"moss-math\">, got: {html}"
    );
    // The visible caption must not show the raw math delimiters.
    let figcap = html
        .split("<figcaption>")
        .nth(1)
        .and_then(|s| s.split("</figcaption>").next())
        .expect("a figcaption");
    assert!(
        !figcap.contains("$x^2$"),
        "typeset caption must not show raw `$x^2$`, got: {figcap}"
    );
    // The caption owns the description, so the image is decorative
    // relative to it: no second copy in `alt=`.
    assert!(
        html.contains(r#"alt="""#),
        "a captioned figure must not repeat the caption in alt, got: {html}"
    );
}

/// `implicit_figure = false` has to mean the same thing for both spellings of
/// an image. It did not: the parser's unwrap pass ran before
/// `dispatch_wikilink_embeds`, which mints figures of its own, so `![[x.png]]`
/// kept a `<figure class="moss-image">` on a site that had opted out while
/// `![alt](x.png)` did not. Any theme rule keyed on `.moss-image` then styled
/// whichever images happened to be written as wikilinks — visible on
/// harbor's homepage as one award tile narrower than the three beside it.
#[test]
fn implicit_figure_off_unwraps_wikilink_embeds_too() {
    let html = render_with_graph_cfg("![[photo.jpg]]\n", &["photo.jpg"], false);
    assert!(
        !html.contains("<figure"),
        "a wikilink embed must respect implicit_figure=false, got: {html}"
    );
    assert!(html.contains("<img"), "image still emitted: {html}");
}

/// …including inside a `:::grid` cell, which is where the asymmetry was
/// actually seen. The dispatcher descends into shortcode bodies, so the unwrap
/// pass has to as well.
#[test]
fn implicit_figure_off_unwraps_wikilink_embeds_inside_a_grid_cell() {
    let html = render_with_graph_cfg(
        ":::grid 2 {.no-cards}\n![[photo.jpg]]\n+++\n![Alt](photo.jpg)\n:::\n",
        &["photo.jpg"],
        false,
    );
    assert!(
        !html.contains("<figure"),
        "grid cells must respect implicit_figure=false too, got: {html}"
    );
}

/// The opt-out undoes an INFERENCE, never an instruction. `|wide` can only be
/// carried by the figure (`data-width=`), so a figure that has one is not
/// implicit and must survive — otherwise opting out of automatic captions
/// silently discards every author-set image width on the site.
#[test]
fn implicit_figure_off_keeps_a_figure_that_carries_an_author_width() {
    let html = render_with_graph_cfg("![[photo.jpg|wide]]\n", &["photo.jpg"], false);
    assert!(
        html.contains("<figure") && html.contains(r#"data-width="wide""#),
        "an author-set width needs its figure, got: {html}"
    );
}

#[test]
fn implicit_figure_off_leaves_bare_image_unwrapped() {
    let html = render("![Morning light](photo.jpg)\n", false);
    assert!(
        !html.contains("<figure "),
        "expected no figure when implicit_figure=false, got: {}",
        html
    );
    assert!(html.contains("<img"), "image still emitted: {}", html);
}

#[test]
fn implicit_figure_skips_empty_alt() {
    // a11y guard: empty-alt images stay unwrapped to avoid `<figure>` with
    // no image description but a descriptive caption (worst of both worlds
    // for assistive tech).
    let html = render("![](photo.jpg)\n", true);
    assert!(
        !html.contains("<figure "),
        "empty-alt image must not become a figure, got: {}",
        html
    );
}

#[test]
fn implicit_figure_skips_paragraph_with_extra_content() {
    // `![alt](src) prose` is not a bare-image paragraph — the prose is
    // load-bearing and shouldn't be silently dropped via figure wrapping.
    let html = render("![alt](photo.jpg) prose\n", true);
    assert!(
        !html.contains("<figure "),
        "paragraph with trailing prose must not become a figure, got: {}",
        html
    );
    let html = render("prose ![alt](photo.jpg)\n", true);
    assert!(
        !html.contains("<figure "),
        "paragraph with leading prose must not become a figure, got: {}",
        html
    );
    let html = render("![a](one.jpg) ![b](two.jpg)\n", true);
    assert!(
        !html.contains("<figure "),
        "two-image paragraph must not become a figure, got: {}",
        html
    );
}

#[test]
fn implicit_figure_fires_when_followed_by_text_paragraph() {
    // The bare-image rule must fire on the image paragraph when the next
    // paragraph is plain prose (not emphasis-only). The pre-existing
    // `peek_emphasis_paragraph` lookahead requires emphasis; with this
    // input the new rule should win.
    let html = render("![Morning light](photo.jpg)\n\nNext paragraph.\n", true);
    assert!(
        html.contains("<figcaption>Morning light</figcaption>"),
        "expected figure across blank line, got: {}",
        html
    );
    // The next paragraph still renders as a plain <p>.
    assert!(
        html.contains("<p>Next paragraph.</p>") || html.contains("Next paragraph."),
        "expected next paragraph preserved, got: {}",
        html
    );
}

#[test]
fn wikilink_with_caption_round_trips_to_figure() {
    // `![[photo|Caption]]` is supposed to flow through `ImageRenderer`
    // lowering to `![Caption](photo.jpg)` and then be wrapped by the
    // bare-paragraph rule. The wiki-link parser is upstream of
    // `process_markdown_file` (lives in moss-core's resolve pipeline),
    // so this test simulates the post-resolve markdown directly. The
    // claim is just that ANY `![Caption](src)` paragraph produces a
    // figure; the wiki-link path is invariant under the same rule.
    let lowered = "![Morning light, Yangshuo](photos/_43A2045.jpg)\n";
    let html = render(lowered, true);
    assert!(
        html.contains("<figure ")
            && html.contains("<figcaption>Morning light, Yangshuo</figcaption>"),
        "wiki-link lowering should produce figure via the same rule: {}",
        html
    );
}

#[test]
fn wikilink_no_alias_does_not_wrap_as_figure() {
    // `![[Pasted image 20260505.png]]` (no alias) lowers via
    // `ImageRenderer` to `![](url)` — empty alt by design, see
    // crates/moss-core/src/resolve/embed_renderer.rs. The bare-image
    // paragraph rule's empty-alt guard then skips wrapping. Without this
    // chain, an Obsidian author who pastes an image would see
    // `<figcaption>Pasted image 20260505</figcaption>` auto-generated
    // in their published output — visible junk on every plain embed.
    let lowered = "![](photos/_43A2045.jpg)\n";
    let html = render(lowered, true);
    assert!(
        !html.contains("<figure "),
        "no-alias image must stay a plain <img>, got: {}",
        html
    );
    assert!(html.contains("<img"), "image still emitted: {}", html);
}

#[test]
fn empty_alt_with_emphasis_caption_does_not_wrap_same_paragraph() {
    // a11y guard for the existing same-paragraph detector.
    // Pre-this-PR behavior: emitted <figure> with empty alt + descriptive caption.
    // Phase A: rejects so the empty-alt rule is uniform across all three detectors.
    let html = render("![](photo.jpg)*Caption*\n", false);
    assert!(
        !html.contains("<figure "),
        "same-paragraph empty-alt + caption must not wrap: {}",
        html
    );
}

#[test]
fn empty_alt_with_emphasis_caption_does_not_wrap_split_paragraph() {
    // Same a11y guard for the existing split-paragraph detector.
    let html = render("![](photo.jpg)\n\n*Caption*\n", false);
    assert!(
        !html.contains("<figure "),
        "split-paragraph empty-alt + caption must not wrap: {}",
        html
    );
}

// Phase 4 PR7a-fragment (2026-05-28): the two unit tests that
// exercised `emit_standalone_figure_image` directly — Step-8 finding-2
// raw-HTML media wrap, and HTML-escape boundary — deleted alongside
// their subject helper. The figure synthesizer (`synthesize_image_html`
// with `ImageContext::MarkdownStandalone`) and its caption-escaping
// path are still exercised end-to-end via the `implicit_figure_*`
// tests above (which run through `process_markdown_file` → typed-AST
// `Block::Figure` promotion → `DefaultHooks::render_image`) and via
// moss-core's `synthesize_image_html` unit tests in
// `crates/moss-core/src/render/image.rs`.

/// End-to-end integration: moss-core's wikilink ImageRenderer emits a
/// full `<figure class="moss-image" data-width="...">…</figure>` for
/// width-aliased wikilinks. pulldown-cmark passes that through as an
/// `HtmlBlock`, and the surrounding pipeline must NOT strip the
/// data-width attribute or rewrap the figure.
#[test]
fn wikilink_with_full_alias_emits_data_width_screen_on_figure() {
    let html_event = render(
        r#"<figure class="moss-image" data-width="screen"><img src="../assets/photo.jpg" alt="" /></figure>"#,
        false,
    );
    assert!(
        html_event.contains(r#"<figure class="moss-image" data-width="screen">"#),
        "got: {}",
        html_event
    );
    // No double-wrap — the figure stays single.
    assert_eq!(
        html_event.matches("<figure").count(),
        1,
        "must not nest figures; got: {}",
        html_event
    );
}

#[test]
fn wikilink_with_wide_alias_emits_data_width_wide() {
    let html_event = render(
        r#"<figure class="moss-image" data-width="wide"><img src="x.jpg" alt="" /></figure>"#,
        false,
    );
    assert!(
        html_event.contains(r#"data-width="wide""#),
        "got: {}",
        html_event
    );
}

#[test]
fn wikilink_without_width_alias_omits_data_width() {
    // Standalone `![alt](url)` (or `![[file]]` without width pipe-alias)
    // must not emit data-width on the figure.
    let html_event = render("![](photo.jpg)\n", false);
    assert!(
        !html_event.contains("data-width="),
        "default: no data-width on figure; got: {}",
        html_event
    );
}

/// Image-only paragraph promotion satisfies the gallery-promotion
/// regex at media_collection.rs:75-76 (`<figure[^>]*>.*?<img[^>]+>.*?</figure>`).
/// Step 8 contract: the wrapper class is `moss-image`.
/// Photography auto-promotion separately requires a `<!-- photography -->`
/// marker, which the bare-paragraph rule does NOT emit — see the
/// design doc for the explicit boundary.
#[test]
fn implicit_figure_html_shape_satisfies_gallery_regex() {
    let html = render("![Caption](photo.jpg)\n", true);
    for snippet in &[
        "<figure ",
        "<img ",
        "<figcaption>",
        "</figcaption>",
        "</figure>",
    ] {
        assert!(
            html.contains(snippet),
            "shape missing {:?}: {}",
            snippet,
            html
        );
    }
    assert!(
        html.contains(r#"<figure class="moss-image">"#),
        "Step 8 wrapper class is the structural identity for site.css; got: {}",
        html
    );
}

// ===== Phase 3 PR4 (2026-05-27) — `moss:` title channel retired =====
//
// The Phase 1 C1 Stage 2 dispatcher used to read `moss:K=V` titles
// from `Tag::Image` and `Tag::Link` events and route them to per-kind
// synthesizers (pdf/video/audio/iframe/3d). PR4 retires that channel:
// wikilink-typed events dispatch via
// `crates/moss-core/src/resolve/wikilink_dispatch.rs`, and plain
// CommonMark images/links emit with default typed slots. The Phase 1
// C1 integration tests below are deleted with the channel they pinned;
// their structural-identity coverage moves to:
//
//   - moss-core `wikilink_dispatch::tests` (per-extension routing)
//   - moss-core `embed_renderer::tests` (bare-markdown emission)
//   - snapshot tests (end-to-end HTML shape for wikilink fixtures)
//
// The contract test in `src-tauri/tests/img_contract_test.rs` is the
// regression guard against a `moss:` title reviving on any `<img>` in
// rendered output (planned extension in PR6).

#[test]
fn pipeline_moss_title_no_longer_routes_to_typed_context() {
    // Regression guard: even when an author types a literal `moss:`
    // title into the markdown, the channel is gone — output is the
    // pulldown-cmark default emission, no figure synthesizer dispatch.
    let html = render_markdown_to_html("![A photo](photo.jpg \"moss:align=left width=wide\")\n");
    // No align class is injected from the title attribute.
    assert!(
        !html.contains("moss-align-left"),
        "moss: title channel must NOT route to typed context, got: {html}"
    );
    // No data-width attribute from the title.
    assert!(
        !html.contains(r#"data-width="wide""#),
        "moss: title channel must NOT inject data-width, got: {html}"
    );
}

#[test]
fn pipeline_moss_kind_link_no_longer_dispatches_to_synthesizer() {
    // Regression guard: `[name](url "moss:kind=video")` (the old
    // Stage 1 → Stage 2 markdown link contract) no longer dispatches.
    // It emits the default `<a href>` like any other CommonMark link.
    let html = render_markdown_to_html(r#"[clip.mp4](clip.mp4 "moss:kind=video")"#);
    assert!(
        !html.contains("<video"),
        "video synth must not fire, got: {html}"
    );
    assert!(!html.contains("<iframe"), "got: {html}");
    assert!(!html.contains("<audio"), "got: {html}");
    assert!(!html.contains("<object"), "got: {html}");
    assert!(!html.contains("<model-viewer"), "got: {html}");
    // Default anchor emission survives.
    assert!(
        html.contains(r#"href="clip.mp4""#),
        "expected default anchor emission, got: {html}"
    );
}

#[test]
fn pipeline_native_link_without_moss_title_passes_through() {
    // Plain CommonMark link → existing rewrite path emits <a href>.
    let html = render_markdown_to_html("[Click here](page.html)");
    assert!(html.contains(r#"href="page.html""#), "got: {html}");
    assert!(html.contains("Click here"), "got: {html}");
    assert!(
        !html.contains("moss-embed"),
        "should not embed, got: {html}"
    );
    assert!(!html.contains("<video"), "got: {html}");
    assert!(!html.contains("<iframe"), "got: {html}");
}

#[test]
fn pipeline_native_image_without_moss_title_keeps_default_context() {
    // Plain CommonMark image → no align class. The image-only paragraph
    // is promoted to a figure whose caption is the alt, so the `alt=`
    // attribute is empty (the caption already names the image).
    let html = render_markdown_to_html("![alt](photo.jpg)");
    assert!(html.contains("<figcaption>alt</figcaption>"), "got: {html}");
    assert!(html.contains(r#"alt="""#), "got: {html}");
    assert!(
        !html.contains("moss-align-"),
        "should not add align without moss: title, got: {html}"
    );
}

#[test]
fn pipeline_author_raw_img_html_unchanged() {
    // Author-typed raw HTML lands as Event::Html — opaque pass-through.
    // No moss enhancement (no loading=, no class injection).
    let html = render_markdown_to_html(r#"<img src="raw.jpg" width="100">"#);
    assert!(
        html.contains(r#"<img src="raw.jpg" width="100">"#),
        "raw HTML must pass through unchanged, got: {html}"
    );
    assert!(
        !html.contains("loading=") && !html.contains("class=\"moss"),
        "must not enhance raw HTML, got: {html}"
    );
}

#[test]
fn pipeline_unknown_kind_does_not_dispatch_under_pr4() {
    // Phase 3 PR4: `moss:kind=…` titles no longer dispatch — even
    // unknown values emit the default anchor (because the channel
    // itself is gone, not because of the kind switch's fallthrough).
    let html = render_markdown_to_html(r#"[thing](x.unknown "moss:kind=xenomorph")"#);
    assert!(
        html.contains(r#"href="x.unknown""#),
        "expected default anchor emission, got: {html}"
    );
    assert!(!html.contains("<video"), "got: {html}");
    assert!(!html.contains("<iframe"), "got: {html}");
    assert!(!html.contains("<object"), "got: {html}");
    assert!(!html.contains("<audio"), "got: {html}");
    assert!(!html.contains("<model-viewer"), "got: {html}");
}
// ----- :::recent dispatch (Task 4.4) -----

/// `:::recent` must flow through the typed-AST dispatcher without
/// panicking. The per-page processor doesn't carry the build's post
/// set (pages are produced one-at-a-time), so v1 of the web-HTML
/// dispatch always renders the fallback markdown. The full
/// post-filter / list emission lives in the email path (Task 4.5),
/// where the post slice is naturally in scope.
///
/// This test pins three invariants:
/// 1. `:::recent` does not crash the pipeline.
/// 2. The fallback markdown renders as ordinary content.
/// 3. No `:::recent` placeholder or sentinel leaks into the final HTML.
#[test]
fn recent_shortcode_dispatch_renders_fallback_on_html_path() {
    let content = "---\ntitle: T\n---\n\n:::recent count=3\nSee the [news](news/) page for older posts.\n:::\n";
    let page_map = std::collections::HashMap::new();
    let doc = process_markdown_file(
        "test.md",
        content,
        "root",
        &page_map,
        false,
        crate::i18n::Language::En,
        Some("test-site"),
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("pipeline should succeed (no panic from Recent dispatch)");
    let html = &doc.html_content;
    assert!(
        html.contains("older posts"),
        "fallback markdown must render when no post set is available, got: {html}"
    );
    assert!(
        !html.contains(":::recent"),
        "raw `:::recent` fence must not leak into HTML, got: {html}"
    );
}

#[test]
fn recent_shortcode_dispatch_empty_fallback_yields_no_marker_leak() {
    let content = "---\ntitle: T\n---\n\nBefore.\n\n:::recent\n:::\n\nAfter.\n";
    let page_map = std::collections::HashMap::new();
    let doc = process_markdown_file(
        "test.md",
        content,
        "root",
        &page_map,
        false,
        crate::i18n::Language::En,
        Some("test-site"),
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("pipeline should succeed");
    let html = &doc.html_content;
    assert!(html.contains("Before."), "got: {html}");
    assert!(html.contains("After."), "got: {html}");
    assert!(
        !html.contains(":::recent"),
        "raw `:::recent` fence must not leak into HTML, got: {html}"
    );
}

// ── Task 7: image |NN% percent — end-to-end integration guard ──────────
// These tests validate that Tasks 4 (parser), 5 (wikilink dispatch), and
// 6 (renderer) compose correctly. `implicit_figure: true` so the figure
// is preserved (false always unwraps figures — see unwrap_implicit_figure).

#[test]
fn standard_image_percent_end_to_end() {
    let html = render("![Caption|55%](photo.jpg)\n", true);
    assert!(html.contains(r#"class="moss-image""#), "got: {html}");
    assert!(html.contains(r#"style="width:55%""#), "got: {html}");
    // |55% must NOT appear in the figcaption text
    assert!(
        !html.contains("55%</figcaption>") && !html.contains("|55%"),
        "width leaked into caption: {html}"
    );
}

#[test]
fn wikilink_image_percent_end_to_end() {
    let html = render("![[photo.jpg|55%]]\n", true);
    assert!(html.contains(r#"style="width:55%""#), "got: {html}");
    assert!(
        !html.contains("55%</figcaption>"),
        "width leaked into caption: {html}"
    );
}

// === Video wikilink sizing — with-graph end-to-end ===
//
// Regression guards for the figure-promotion hijack: the parser used to
// promote `![[clip.mov|77%]]` to an image Figure (width bypasses the
// empty-alt guard), and `dispatch_wikilink_embeds` skips Figures, so the
// page shipped `<figure><img src="clip.mov">` — a broken image in both
// preview and published site. Videos must reach the video synthesizer
// regardless of sizing pothole. Needs graph + registry: video synthesis
// happens only in the with-graph dispatch path.

fn render_with_graph(md: &str, files: &[&str]) -> String {
    parse_with_graph(md, files).html_content
}

/// As [`render_with_graph`], with the `[site].implicit_figure` switch exposed.
fn render_with_graph_cfg(md: &str, files: &[&str], implicit_figure: bool) -> String {
    parse_with_graph_cfg(md, files, implicit_figure).html_content
}

/// Same pipeline as [`render_with_graph`], but returns the hoisted hero
/// section instead of the body — a `:::hero` block never appears in
/// `html_content`. A document with several heroes yields the FIRST one.
fn render_hero_with_graph(md: &str, files: &[&str]) -> String {
    parse_with_graph(md, files)
        .hero_html
        .expect("source declares a :::hero block")
}

fn parse_with_graph(md: &str, files: &[&str]) -> crate::build::types::ParsedDocument {
    parse_with_graph_cfg(md, files, true)
}

fn parse_with_graph_cfg(
    md: &str,
    files: &[&str],
    implicit_figure: bool,
) -> crate::build::types::ParsedDocument {
    let empty_map = HashMap::new();
    let mut b = moss_core::content_graph::ContentGraphBuilder::new();
    for p in files {
        let slug = std::path::Path::new(p)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(p);
        b.add_file(p, slug);
    }
    let graph = b.build();
    let registry = moss_core::resolve::registry::RendererRegistry::empty().build();
    process_markdown_file(
        "test.md",
        md,
        "site",
        &empty_map,
        false,
        crate::i18n::Language::En,
        None,
        crate::build::markdown::SiteMarkdown { implicit_figure, ..Default::default() },
        None,
        None,
        Some(&graph),
        Some(&registry),
        false,
        None, // seta_url
        None, // folder_lang
    )
    .expect("should parse")
}

#[test]
fn video_embed_plain_end_to_end() {
    let html = render_with_graph("![[clip.mov]]\n", &["clip.mov"]);
    assert!(html.contains("<video"), "got: {html}");
    assert!(html.contains("clip.mp4"), "mov→mp4 swap missing: {html}");
}

#[test]
fn video_embed_percent_end_to_end() {
    let html = render_with_graph("![[clip.mov|77%]]\n", &["clip.mov"]);
    assert!(html.contains("<video"), "got: {html}");
    // A bare percent is the element's own width and rides inline style;
    // `<video width="77%">` was never valid HTML.
    assert!(
        html.contains(r#"style="width:77%""#),
        "percent width dropped: {html}"
    );
    assert!(
        !html.contains("<img") && !html.contains(r#"class="moss-image""#),
        "video must not render as an image figure: {html}"
    );
}

#[test]
fn video_embed_box_sizing_end_to_end() {
    let html = render_with_graph("![[clip.mov|640x360]]\n", &["clip.mov"]);
    assert!(html.contains("<video"), "got: {html}");
    assert!(html.contains(r#"width="640px""#), "got: {html}");
    assert!(html.contains(r#"height="360px""#), "got: {html}");
    assert!(
        !html.contains("figcaption"),
        "sizing alias must not become a caption: {html}"
    );
}

#[test]
fn wikilink_image_percent_with_graph_still_figure() {
    let html = render_with_graph("![[photo.jpg|55%]]\n", &["photo.jpg"]);
    assert!(
        html.contains(r#"style="width:55%""#),
        "image percent must keep figure width: {html}"
    );
}

/// moss#754: an image embed's sizing tokens must survive inside a `:::hero`
/// overlay, exactly as they do in body prose. The overlay renders through
/// hero-overlay hooks. Those used to be a partially-delegating wrapper that
/// forwarded only the styleless image entry point, so `object-fit`/
/// `object-position` fell off silently at the wrapper boundary.
#[test]
fn wikilink_image_sizing_survives_hero_overlay_end_to_end() {
    // Hero is asserted FIRST: both paths share the no-snapshot fallback, so
    // leading with the body baseline would make any shared regression report
    // as a body failure and never reach the hero assertion at all.
    //
    // The hero is hoisted out of `html_content` into `hero_html`. It needs its
    // own `image=` so the embed stays OVERLAY content rather than being
    // promoted into the hero's own image slot.
    let hero = render_hero_with_graph(
        ":::hero {image=header.png}\n# Title\n\n![[photo.jpg|cover left]]\n:::\n",
        &["photo.jpg", "header.png"],
    );
    assert!(
        hero.contains("object-fit:cover;object-position:left"),
        "hero overlay must carry the sizing style: {hero}"
    );
    let body = render_with_graph("![[photo.jpg|cover left]]\n", &["photo.jpg"]);
    assert!(
        body.contains("object-fit:cover;object-position:left"),
        "body prose must carry the same sizing style as the hero overlay: {body}"
    );
}

// ---- per-page language tests ----

/// A `:::apply` shortcode on a `zh-hans/` page in an English-default site
/// must render in zh-hans (button text "申请", action `?lang=zh-hans`),
/// not in the site default (English). Regression for the site_lang bug
/// where pipeline hooks used `site_lang` instead of `doc_lang`.
#[test]
fn per_page_language_apply_in_zh_hans_subdir() {
    let md = "---\ntitle: 申请\n---\n\n:::apply\n:::\n";
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "zh-hans/index.md",
        md,
        "test-site",
        &empty_map,
        false,
        Language::En, // site default is EN
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        Some("https://api.mosspub.com"),
        None, // folder_lang
    )
    .expect("should parse");
    assert!(
        doc.html_content.contains("申请"),
        ":::apply on a zh-hans page must render zh-hans button label, got:\n{}",
        doc.html_content
    );
    assert!(
        doc.html_content.contains("lang=zh-hans"),
        ":::apply action URL must carry lang=zh-hans, got:\n{}",
        doc.html_content
    );
    assert!(
        !doc.html_content.contains(">Apply<"),
        "English label must NOT appear on a zh-hans page, got:\n{}",
        doc.html_content
    );
}

/// A `:::subscribe` shortcode on a zh-hans page in an En-default site must
/// render zh-hans copy (button text "订阅", placeholder "邮箱") and carry
/// `lang="zh-hans"` on the enclosing element, NOT the EN defaults.
///
/// Mirrors `per_page_language_apply_in_zh_hans_subdir` for the subscribe
/// shortcode — regression guard so the lang derivation applies to both
/// inline shortcodes equally.
#[test]
fn per_page_language_subscribe_in_zh_hans_subdir() {
    let md = "---\ntitle: 订阅\n---\n\n:::subscribe\n:::\n";
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "zh-hans/index.md",
        md,
        "test-site",
        &empty_map,
        false,
        Language::En, // site default is EN
        Some("test-site"),
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        Some("https://api.mosspub.com"),
        None, // folder_lang
    )
    .expect("should parse");
    assert!(
        doc.html_content.contains("订阅"),
        ":::subscribe on a zh-hans page must render zh-hans button label, got:\n{}",
        doc.html_content
    );
    assert!(
        !doc.html_content.contains(">Subscribe<"),
        "English 'Subscribe' label must NOT appear on a zh-hans page, got:\n{}",
        doc.html_content
    );
    assert!(
        doc.html_content.contains("请在邮箱中确认订阅"),
        "zh-hans confirmation copy must appear, got:\n{}",
        doc.html_content
    );
}

/// A `:::apply` shortcode on an English page in a zh-hans-default site
/// must render in English (button text "Apply", action `?lang=en`),
/// not in the site default (zh-hans).
#[test]
fn per_page_language_apply_en_page_in_zh_hans_site() {
    let md = "---\ntitle: Apply\nlang: en\n---\n\n:::apply\n:::\n";
    let empty_map = HashMap::new();
    let doc = process_markdown_file(
        "en/index.md",
        md,
        "test-site",
        &empty_map,
        false,
        Language::ZhHans, // site default is ZhHans
        None,
        crate::build::markdown::SiteMarkdown::default(),
        None,
        None,
        None,
        None,
        false,
        Some("https://api.mosspub.com"),
        None, // folder_lang
    )
    .expect("should parse");
    assert!(
        doc.html_content.contains(">Apply<"),
        ":::apply on an en page must render English button label, got:\n{}",
        doc.html_content
    );
    assert!(
        doc.html_content.contains("lang=en"),
        ":::apply action URL must carry lang=en, got:\n{}",
        doc.html_content
    );
    assert!(
        !doc.html_content.contains("申请"),
        "zh-hans label must NOT appear on an English page, got:\n{}",
        doc.html_content
    );
}

/// A frontmatter field moss doesn't recognize is normally silent by design.
/// `slug:` was silent too — and it is the custom-URL field in Hugo, Jekyll,
/// Zola and Astro, so a writer arriving from any of them pinned a URL, got
/// the filename-derived one, and had nothing to go on. The build never
/// validated frontmatter at all: `validate_frontmatter`'s only caller is the
/// editor, so authoring in Obsidian or vim produced no signal.
///
/// Assert the pure warning builder so a regression that drops the
/// `cli_eprintln!` loop from the pipeline is caught.
#[test]
fn foreign_frontmatter_field_warns_and_names_the_moss_equivalent() {
    let warnings = foreign_frontmatter_warnings("privacy.md", &["title", "slug"]);
    assert_eq!(
        warnings.len(),
        1,
        "only 'slug' is foreign here, got: {warnings:?}"
    );
    let warning = &warnings[0];
    assert!(
        warning.contains("privacy.md"),
        "warning must name the file, got: {warning}"
    );
    assert!(
        warning.contains("'slug'"),
        "warning must name the offending key, got: {warning}"
    );
    assert!(
        warning.contains("did you mean 'url'"),
        "warning must name the moss field that does the job, got: {warning}"
    );
}

/// The other half of the contract: a genuinely custom field stays silent.
/// Plugins and templates read their own keys, and warning on every one would
/// make the signal above worthless.
#[test]
fn ordinary_custom_frontmatter_fields_do_not_warn() {
    let warnings =
        foreign_frontmatter_warnings("post.md", &["title", "date", "my_field", "data", "uid"]);
    assert!(
        warnings.is_empty(),
        "builtin and custom fields must not warn, got: {warnings:?}"
    );
}

/// Build the frontmatter map the way the pipeline does, so these tests exercise
/// the same values `schema_frontmatter_warnings` sees at build time.
#[cfg(test)]
fn fm_of(yaml_body: &str) -> std::collections::HashMap<String, serde_yaml::Value> {
    let doc = format!("---\n{}\n---\nbody\n", yaml_body.trim_matches('\n'));
    moss_core::frontmatter::parse(&doc).frontmatter
}

/// The foreign-field warning closed one hole; this closes the rest of the
/// class. `weight: high` is a type error the schema has always been able to
/// catch, but `validate_frontmatter`'s only caller was the editor — so a site
/// authored in Obsidian or vim built silently with a broken sort order.
#[test]
fn schema_type_mismatch_warns_at_build() {
    let warnings = schema_frontmatter_warnings("post.md", &fm_of("title: Hi\nweight: high"));
    assert_eq!(warnings.len(), 1, "only weight is wrong, got: {warnings:?}");
    assert!(
        warnings[0].contains("post.md") && warnings[0].contains("'weight'"),
        "warning must name the file and the field, got: {}",
        warnings[0]
    );
}

/// Enum violations are `Error`s and reach the build for the same reason.
#[test]
fn schema_enum_violation_warns_at_build() {
    let warnings =
        schema_frontmatter_warnings("index.md", &fm_of("title: Hi\nchildren_style: carousel"));
    assert_eq!(warnings.len(), 1, "got: {warnings:?}");
    assert!(
        warnings[0].contains("'children_style'") && warnings[0].contains("carousel"),
        "warning must name the field and the bad value, got: {}",
        warnings[0]
    );
}

/// Date-format problems are `Warning`, not `Error` — both severities surface.
#[test]
fn schema_bad_date_format_warns_at_build() {
    let warnings = schema_frontmatter_warnings("post.md", &fm_of("title: Hi\ndate: '2026/08/04'"));
    assert_eq!(warnings.len(), 1, "got: {warnings:?}");
    assert!(
        warnings[0].contains("date"),
        "warning must name the date field, got: {}",
        warnings[0]
    );
}

/// The critical negative: `title` is the schema's one required field, and
/// `validate_frontmatter` reports an `Error` when it is absent — but moss
/// deliberately falls back to the filename (docs/reference/title-rendering.md),
/// so most correct pages omit it. Emitting that error would fire on nearly
/// every page and train authors to ignore the whole channel.
#[test]
fn missing_title_does_not_warn_at_build() {
    let warnings = schema_frontmatter_warnings("post.md", &fm_of("date: '2026-08-04'"));
    assert!(
        warnings.is_empty(),
        "title falls back to the filename; a missing-title build warning would fire on most pages, got: {warnings:?}"
    );
}

/// Unknown fields are `Hint`s and stay silent: templates and plugins read their
/// own keys. The actionable subset is handled by `foreign_frontmatter_warnings`,
/// and routing it through here too would print each foreign key twice.
#[test]
fn unknown_and_foreign_fields_do_not_warn_from_the_schema_path() {
    let warnings =
        schema_frontmatter_warnings("post.md", &fm_of("title: Hi\nmy_field: x\nslug: about"));
    assert!(
        warnings.is_empty(),
        "custom keys stay silent and 'slug' belongs to the foreign-field path, got: {warnings:?}"
    );
}

/// Correct frontmatter must produce nothing at all — the whole point of the
/// filters above.
#[test]
fn valid_frontmatter_warns_nothing_at_build() {
    let warnings = schema_frontmatter_warnings(
        "post.md",
        &fm_of("title: Hi\ndate: '2026-08-04'\nweight: 3\nchildren_style: grid\ndraft: true"),
    );
    assert!(warnings.is_empty(), "got: {warnings:?}");
}

/// A body image's `sizes=` follows the page's EFFECTIVE typesetting: the
/// page's own `typesetting:` over `[site].typesetting`. Under vertical-rl
/// the column is a height, so the value is the column height × the aspect.
#[test]
fn body_image_sizes_follow_the_pages_effective_typesetting() {
    let meta = crate::types::content::MediaMetadata {
        is_animated: false,
        path: "photo.jpg".to_string(),
        file_type: "jpg".to_string(),
        size: 0,
        modified: None,
        dimensions: Some((2400, 1771)),
        dominant_color: None,
        lqip_data_uri: None,
    };
    let lookup =
        crate::build::media::dimensions::MediaDimensionLookup::new(&[meta], &[], &HashMap::new(), None);
    let empty_map = HashMap::new();
    let render = |frontmatter: &str, site_typesetting: Option<&str>| {
        let md = format!("---\ntitle: t\n{frontmatter}---\n\n正文。\n\n![](photo.jpg)\n");
        let site = SiteMarkdown { typesetting: site_typesetting, ..Default::default() };
        process_markdown_file(
            "a.md", &md, "site", &empty_map, false, Language::ZhHant, None, site,
            Some(&lookup), None, None, None, false, None, None,
        )
        .expect("should parse")
        .html_content
    };
    let vertical = r#"sizes="calc(1.356 * (100vh - 4rem))""#;
    let horizontal = r#"sizes="(min-width: 48rem) 47.25rem, 100vw""#;

    let site_vertical = render("", Some("vertical"));
    assert!(site_vertical.contains(vertical), "{site_vertical}");
    let page_horizontal = render("typesetting: horizontal\n", Some("vertical"));
    assert!(page_horizontal.contains(horizontal), "{page_horizontal}");
    let page_vertical = render("typesetting: vertical\n", None);
    assert!(page_vertical.contains(vertical), "{page_vertical}");
    let site_default = render("", None);
    assert!(site_default.contains(horizontal), "{site_default}");
}
