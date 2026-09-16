use super::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_article_map_is_article() {
    let mut map = ArticleMap::new();
    map.articles.insert(
        "posts/test-article.html".to_string(),
        ArticleInfo {
            source_path: "posts/test-article.md".to_string(),
            title: "Test Article".to_string(),
            content: "# Test".to_string(),
            html_content: None,
            frontmatter: HashMap::new(),
            url_path: "posts/test-article.html".to_string(),
            date: None,
            tags: vec![],
            uid: None,
        },
    );

    // Should find article with exact match
    assert!(map.is_article("posts/test-article.html"));

    // Should find article with leading slash (normalized)
    assert!(map.is_article("/posts/test-article.html"));

    // Should not find non-existent article
    assert!(!map.is_article("about.html"));
    assert!(!map.is_article("index.html"));
}

#[test]
fn test_article_map_save_and_load() {
    let temp_dir = TempDir::new().unwrap();
    let moss_dir = temp_dir.path();
    fs::create_dir_all(moss_dir.join("build")).unwrap();

    let mut map = ArticleMap::new();
    map.articles.insert(
        "posts/test.html".to_string(),
        ArticleInfo {
            source_path: "posts/test.md".to_string(),
            title: "Test".to_string(),
            content: "Content".to_string(),
            html_content: None,
            frontmatter: HashMap::new(),
            url_path: "posts/test.html".to_string(),
            date: None,
            tags: vec![],
            uid: None,
        },
    );

    // Save
    map.save(moss_dir).unwrap();
    assert!(moss_dir.join("build").join("article-map.json").exists());

    // Load
    let loaded = ArticleMap::load(moss_dir).unwrap();
    assert!(loaded.is_article("posts/test.html"));
    assert_eq!(
        loaded.articles.get("posts/test.html").unwrap().title,
        "Test"
    );
}

#[test]
fn test_article_map_load_nonexistent() {
    let temp_dir = TempDir::new().unwrap();
    let moss_dir = temp_dir.path();

    // Should return empty map when file doesn't exist
    let map = ArticleMap::load(moss_dir).unwrap();
    assert!(map.articles.is_empty());
}

#[test]
fn test_article_map_save_is_atomic_no_temp_leftover() {
    // #820: save writes via a temp file + atomic rename so concurrent readers
    // never catch a half-written file. Assert the rename consumed the temp
    // (no `.json.tmp` sibling left behind) and the final file is complete.
    let temp_dir = TempDir::new().unwrap();
    let moss_dir = temp_dir.path();
    fs::create_dir_all(moss_dir.join("build")).unwrap();

    let map = ArticleMap::new();
    map.save(moss_dir).unwrap();

    let build = moss_dir.join("build");
    let leftovers: Vec<_> = fs::read_dir(&build)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.contains(".tmp") || n.ends_with('~'))
        .collect();
    assert!(
        leftovers.is_empty(),
        "atomic save must leave no temp file behind, found: {:?}",
        leftovers,
    );
    // Final file parses cleanly (round-trip).
    ArticleMap::load(moss_dir).unwrap();
}

// ===== Tests for to_pretty_url() =====
// These must match frontend extractDisplayPath() behavior exactly

#[test]
fn test_to_pretty_url_directory_style() {
    // "foo/bar/index.html" -> "foo/bar/" (WITH trailing slash)
    assert_eq!(to_pretty_url("news/article/index.html"), "news/article/");
    assert_eq!(to_pretty_url("posts/my-post/index.html"), "posts/my-post/");
    assert_eq!(to_pretty_url("a/b/c/index.html"), "a/b/c/");
}

#[test]
fn test_to_pretty_url_file_style() {
    // "foo/bar.html" -> "foo/bar" (NO trailing slash - matches frontend!)
    assert_eq!(to_pretty_url("posts/hello.html"), "posts/hello");
    assert_eq!(to_pretty_url("news/update.html"), "news/update");
}

#[test]
fn test_to_pretty_url_strips_leading_slash() {
    assert_eq!(to_pretty_url("/news/article/index.html"), "news/article/");
    assert_eq!(to_pretty_url("/posts/hello.html"), "posts/hello");
}

#[test]
fn test_to_pretty_url_preserves_non_html() {
    // Non-HTML paths should be returned as-is (minus leading slash)
    assert_eq!(to_pretty_url("rss.xml"), "rss.xml");
    assert_eq!(to_pretty_url("/rss.xml"), "rss.xml");
}

#[test]
fn test_to_pretty_url_cjk_paths() {
    // CJK characters should be preserved
    assert_eq!(
        to_pretty_url("posts/中文文章/index.html"),
        "posts/中文文章/"
    );
    assert_eq!(to_pretty_url("posts/日本語.html"), "posts/日本語");
}

#[test]
fn test_to_pretty_url_root_homepage() {
    // Root homepage should become empty string (nav prepends / to make /)
    assert_eq!(to_pretty_url("index.html"), "");
    assert_eq!(to_pretty_url("/index.html"), "");
}

#[test]
fn test_to_pretty_url_lang_prefix_homepage() {
    // Language-prefixed homepage: zh-hans/index.html -> zh-hans/
    assert_eq!(to_pretty_url("zh-hans/index.html"), "zh-hans/");
}

// ===== Tests for is_article() with pretty URL keys =====

#[test]
fn test_is_article_matches_pretty_url_directory_style() {
    let mut map = ArticleMap::new();
    map.articles.insert(
        "news/2023-05/".to_string(), // Pretty URL key (directory-style)
        ArticleInfo {
            source_path: "news/2023-05.md".to_string(),
            title: "May 2023".to_string(),
            content: "Content".to_string(),
            html_content: None,
            frontmatter: HashMap::new(),
            url_path: "news/2023-05/".to_string(),
            date: None,
            tags: vec![],
            uid: None,
        },
    );

    // Should match with and without leading slash
    assert!(map.is_article("news/2023-05/"));
    assert!(map.is_article("/news/2023-05/"));
}

#[test]
fn test_is_article_matches_pretty_url_file_style() {
    let mut map = ArticleMap::new();
    map.articles.insert(
        "posts/hello".to_string(), // Pretty URL key (file-style, no trailing slash)
        ArticleInfo {
            source_path: "posts/hello.md".to_string(),
            title: "Hello".to_string(),
            content: "Content".to_string(),
            html_content: None,
            frontmatter: HashMap::new(),
            url_path: "posts/hello".to_string(),
            date: None,
            tags: vec![],
            uid: None,
        },
    );

    // Should match with and without leading slash
    assert!(map.is_article("posts/hello"));
    assert!(map.is_article("/posts/hello"));
}

#[test]
fn test_build_article_map_filters_correctly() {
    let temp_dir = TempDir::new().unwrap();
    let source_path = temp_dir.path();

    // Create test markdown files
    let posts_dir = source_path.join("posts");
    fs::create_dir_all(&posts_dir).unwrap();

    // Article in posts folder
    fs::write(
        posts_dir.join("my-article.md"),
        r#"---
title: "My Article"
date: "2024-01-15"
tags:
  - test
---

# Hello World"#,
    )
    .unwrap();

    // Index page (should be excluded)
    fs::write(
        source_path.join("index.md"),
        r#"---
title: "Home"
---

Welcome"#,
    )
    .unwrap();

    // About page (should be excluded)
    fs::write(
        source_path.join("about.md"),
        r#"---
title: "About"
---

About me"#,
    )
    .unwrap();

    // Collection index (should be excluded)
    fs::write(
        posts_dir.join("index.md"),
        r#"---
title: "Posts"
---

All posts"#,
    )
    .unwrap();

    // Mock parsed documents using the helper
    let documents = vec![
        make_doc("My Article", "posts/my-article/index.html", false),
        make_doc("Home", "index.html", true), // homepage (is_index=true)
        make_doc("About", "about/index.html", false), // regular page (not a folder note)
        make_doc("Posts", "posts/index.html", true), // folder index (is_index=true)
    ];

    let map = build_article_map(&documents, &HashMap::new(), &[], &[], &Default::default());

    // Should include article with PRETTY URL key (not file path!)
    // "posts/my-article/index.html" -> key is "posts/my-article/"
    assert!(map.is_article("posts/my-article/"));

    // Should also match when frontend sends with leading slash
    assert!(map.is_article("/posts/my-article/"));

    // About page (not a folder note) is now included — users can disable
    // comments per-page via frontmatter `comments: false` if needed
    assert!(map.is_article("about/"));

    // Should exclude index/section pages
    assert!(!map.is_article("index.html"));
    assert!(!map.is_article("posts/"));

    // Should have exactly 2 articles (My Article + About)
    assert_eq!(map.articles.len(), 2);
}

#[test]
fn test_build_article_map_includes_html_content() {
    let temp_dir = TempDir::new().unwrap();
    let source_path = temp_dir.path();

    // Create source markdown file
    let posts_dir = source_path.join("posts");
    fs::create_dir_all(&posts_dir).unwrap();
    fs::write(
        posts_dir.join("hello.md"),
        "---\ntitle: \"Hello\"\ntags:\n  - test\n---\n\n# Hello World\n\nSome content.",
    )
    .unwrap();

    // ParsedDocument with html_content populated (as pulldown-cmark would produce)
    let documents = vec![ParsedDocument {
        title: "Hello".to_string(),
        label: "Hello".to_string(),
        content: "# Hello World\n\nSome content.".to_string(),
        html_content: "<h1>Hello World</h1>\n<p>Some content.</p>\n".to_string(),
        url_path: "posts/hello/index.html".to_string(),
        reading_time: 1,
        slug: "hello".to_string(),
        permalink: "/posts/hello".to_string(),
        kind: PageKind::Article,
        ..Default::default()
    }];

    let map = build_article_map(&documents, &HashMap::new(), &[], &[], &Default::default());
    let article = map.articles.get("posts/hello/").unwrap();

    // html_content should be populated from ParsedDocument.html_content
    assert!(article.html_content.is_some());
    assert_eq!(
        article.html_content.as_ref().unwrap(),
        "<h1>Hello World</h1>\n<p>Some content.</p>\n"
    );
    // markdown content should also still be present
    assert!(!article.content.is_empty());
}

#[test]
fn test_article_map_html_content_roundtrips_through_json() {
    let temp_dir = TempDir::new().unwrap();
    let moss_dir = temp_dir.path();
    fs::create_dir_all(moss_dir.join("build")).unwrap();

    let mut map = ArticleMap::new();
    map.articles.insert(
        "posts/test/".to_string(),
        ArticleInfo {
            source_path: "posts/test.md".to_string(),
            title: "Test".to_string(),
            content: "# Test\n\nBody.".to_string(),
            html_content: Some("<h1>Test</h1>\n<p>Body.</p>\n".to_string()),
            frontmatter: HashMap::new(),
            url_path: "posts/test/".to_string(),
            date: None,
            tags: vec![],
            uid: None,
        },
    );

    // Save and reload
    map.save(moss_dir).unwrap();
    let loaded = ArticleMap::load(moss_dir).unwrap();
    let article = loaded.articles.get("posts/test/").unwrap();

    // html_content should survive serialization roundtrip
    assert_eq!(
        article.html_content.as_ref().unwrap(),
        "<h1>Test</h1>\n<p>Body.</p>\n"
    );
}

#[test]
fn test_article_map_loads_without_html_content_field() {
    // Old article-map.json files won't have html_content
    // Deserialization should still work (field defaults to None)
    let temp_dir = TempDir::new().unwrap();
    let moss_dir = temp_dir.path();
    fs::create_dir_all(moss_dir.join("build")).unwrap();

    let old_json = r##"{
            "articles": {
                "posts/old/": {
                    "source_path": "posts/old.md",
                    "title": "Old Article",
                    "content": "# Old",
                    "url_path": "posts/old/",
                    "tags": []
                }
            }
        }"##;
    fs::write(moss_dir.join("build").join("article-map.json"), old_json).unwrap();

    let loaded = ArticleMap::load(moss_dir).unwrap();
    let article = loaded.articles.get("posts/old/").unwrap();

    // Should load fine with html_content as None
    assert_eq!(article.title, "Old Article");
    assert!(article.html_content.is_none());
    // uid should also be None (not present in old JSON)
    assert!(article.uid.is_none());
}

#[test]
fn test_article_map_uid_serialization() {
    // Test that uid field is serialized to JSON and can be loaded back
    let temp_dir = TempDir::new().unwrap();
    let moss_dir = temp_dir.path();
    fs::create_dir_all(moss_dir.join("build")).unwrap();

    let mut map = ArticleMap::new();
    map.articles.insert(
        "posts/test/".to_string(),
        ArticleInfo {
            source_path: "posts/test.md".to_string(),
            title: "Test".to_string(),
            content: "# Test".to_string(),
            html_content: None,
            frontmatter: HashMap::new(),
            url_path: "posts/test/".to_string(),
            date: None,
            tags: vec![],
            uid: Some("a7b3c9d2".to_string()),
        },
    );

    // Save and verify JSON contains uid
    map.save(moss_dir).unwrap();
    let json = fs::read_to_string(moss_dir.join("build").join("article-map.json")).unwrap();
    assert!(json.contains("a7b3c9d2"), "JSON should contain uid value");

    // Load and verify uid roundtrips
    let loaded = ArticleMap::load(moss_dir).unwrap();
    let article = loaded.articles.get("posts/test/").unwrap();
    assert_eq!(article.uid, Some("a7b3c9d2".to_string()));
}

#[test]
fn test_article_map_uid_none_omitted_from_json() {
    // Test that uid: None is omitted from JSON (skip_serializing_if)
    let mut map = ArticleMap::new();
    map.articles.insert(
        "posts/test/".to_string(),
        ArticleInfo {
            source_path: "posts/test.md".to_string(),
            title: "Test".to_string(),
            content: "# Test".to_string(),
            html_content: None,
            frontmatter: HashMap::new(),
            url_path: "posts/test/".to_string(),
            date: None,
            tags: vec![],
            uid: None,
        },
    );

    let json = serde_json::to_string_pretty(&map).unwrap();
    assert!(
        !json.contains("\"uid\""),
        "JSON should not contain uid when None"
    );
}

#[test]
fn test_build_article_map_propagates_uid_from_parsed_document() {
    // Test that uid flows from ParsedDocument to ArticleInfo in the map
    let temp_dir = TempDir::new().unwrap();
    let source_path = temp_dir.path();

    // Create source markdown file (WITHOUT uid in frontmatter)
    let posts_dir = source_path.join("posts");
    fs::create_dir_all(&posts_dir).unwrap();
    fs::write(
        posts_dir.join("my-article.md"),
        "---\ntitle: \"My Article\"\ntags:\n  - test\n---\n\n# Hello World",
    )
    .unwrap();

    // ParsedDocument with uid set (as pipeline would do)
    let documents = vec![ParsedDocument {
        title: "My Article".to_string(),
        label: "My Article".to_string(),
        url_path: "posts/my-article/index.html".to_string(),
        reading_time: 1,
        slug: "my-article".to_string(),
        permalink: "/posts/my-article".to_string(),
        kind: PageKind::Article,
        uid: Some("a1b2c3d4".to_string()),
        ..Default::default()
    }];

    let map = build_article_map(&documents, &HashMap::new(), &[], &[], &Default::default());
    let article = map.articles.get("posts/my-article/").unwrap();

    // uid should be propagated from ParsedDocument
    assert_eq!(article.uid, Some("a1b2c3d4".to_string()));
}

/// A slot file (`footer.md`) must never enter the article map. It emits no
/// page, so an entry here is a URL nothing else believes in: the editor's
/// preview follower navigates to `/footer/` and 404s, and `resolve_page_source`
/// reports `is_article: true`, which arms the syndicate path.
/// See docs/archive/2026-08-02-footer-slot-preview-and-chip-bar.md.
#[test]
fn test_slot_only_doc_excluded_from_article_map() {
    let mut doc = make_doc("Footer", "footer/index.html", false);
    doc.slot_only = true;
    doc.source_path = Some("footer.md".to_string());

    let map = build_article_map(&[doc], &HashMap::new(), &[], &[], &Default::default());

    assert!(
        !map.is_article("footer/"),
        "slot-only footer must not be in the article map: {:?}",
        map.articles.keys().collect::<Vec<_>>()
    );
    assert!(map.articles.is_empty(), "no articles expected");
    assert!(map.pages.is_empty(), "a slot file is not a page either");
}

/// The slot gate must not swallow ordinary pages that merely live near a
/// footer — only `slot_only` docs are excluded.
#[test]
fn test_non_slot_doc_still_included_alongside_slot_file() {
    let mut footer = make_doc("Footer", "footer/index.html", false);
    footer.slot_only = true;
    footer.source_path = Some("footer.md".to_string());
    let about = make_doc("About", "about/index.html", false);

    let map = build_article_map(&[footer, about], &HashMap::new(), &[], &[], &Default::default());

    assert!(map.is_article("about/"), "normal page must survive the gate");
    assert!(!map.is_article("footer/"), "slot file must not");
    assert_eq!(map.articles.len(), 1);
}

/// Helper to create a minimal ParsedDocument for testing build_article_map filtering.
fn make_doc(title: &str, url_path: &str, is_index: bool) -> ParsedDocument {
    let kind = if is_index {
        PageKind::Folder
    } else {
        PageKind::Article
    };
    ParsedDocument {
        title: title.to_string(),
        label: title.to_string(),
        url_path: url_path.to_string(),
        reading_time: 1,
        kind,
        ..Default::default()
    }
}

#[test]
fn test_root_level_article_included_in_article_map() {
    // 友链.md builds to 友链/index.html with is_index=false (it's an article, not a folder note)
    let documents = vec![make_doc("友链", "友链/index.html", false)];
    let map = build_article_map(&documents, &HashMap::new(), &[], &[], &Default::default());
    assert!(
        map.is_article("友链/"),
        "Root-level article should be included in article-map"
    );
}

#[test]
fn test_folder_index_excluded_from_article_map() {
    // 文字/纽约诸法门/index.md is a folder note (is_index=true)
    let documents = vec![make_doc("纽约诸法门", "文字/纽约诸法门/index.html", true)];
    let map = build_article_map(&documents, &HashMap::new(), &[], &[], &Default::default());
    assert!(
        !map.is_article("文字/纽约诸法门/"),
        "Folder index page should NOT be in article-map"
    );
}

#[test]
fn test_top_level_folder_index_excluded() {
    // 文字/index.md is a top-level folder index (is_index=true)
    let documents = vec![make_doc("文字", "文字/index.html", true)];
    let map = build_article_map(&documents, &HashMap::new(), &[], &[], &Default::default());
    assert!(
        !map.is_article("文字/"),
        "Top-level folder index should NOT be in article-map"
    );
}

#[test]
fn test_deep_article_included() {
    // A deep article like 文字/游记/某篇/index.html with is_index=false
    let documents = vec![make_doc("某篇", "文字/游记/某篇/index.html", false)];
    let map = build_article_map(&documents, &HashMap::new(), &[], &[], &Default::default());
    assert!(
        map.is_article("文字/游记/某篇/"),
        "Deep article should be included in article-map"
    );
}

#[test]
fn test_build_article_map_special_character_filenames() {
    // Regression test: filenames with special characters (fullwidth colon,
    // spaces, fullwidth comma) get slugified in URL paths, so the
    // reconstructed source path won't match the actual file on disk.
    // build_article_map must NOT rely on re-reading the source file.
    let temp_dir = TempDir::new().unwrap();
    let source_path = temp_dir.path();

    // Create directory structure but DO NOT create the source .md files
    // that the slug-based path reconstruction would expect. The real files
    // have special characters that don't match the slugified URL path.
    let articles_dir = source_path.join("articles");
    fs::create_dir_all(&articles_dir).unwrap();

    // Real file: "蓝海螺：一个童话.md" but slug becomes "蓝海螺-一个童话"
    // Real file: "病毒算法 ，全民基本收入，与可编程的世界观.md" but slug converts spaces/commas
    // Real file: "通过哈伯格税治理 Matters 标签.md" but slug converts spaces
    // We intentionally do NOT create these files — that's the bug scenario.

    let documents = vec![
        ParsedDocument {
            title: "蓝海螺：一个童话".to_string(),
            label: "蓝海螺：一个童话".to_string(),
            content: "# 蓝海螺：一个童话\n\n故事内容".to_string(),
            html_content: "<h1>蓝海螺：一个童话</h1><p>故事内容</p>".to_string(),
            url_path: "articles/蓝海螺-一个童话/index.html".to_string(),
            date: Some("2024-03-01".to_string()),
            reading_time: 5,
            slug: "蓝海螺-一个童话".to_string(),
            permalink: "/articles/蓝海螺-一个童话".to_string(),
            kind: PageKind::Article,
            uid: Some("abc12345".to_string()),
            ..Default::default()
        },
        ParsedDocument {
            title: "通过哈伯格税治理 Matters 标签".to_string(),
            label: "通过哈伯格税治理 Matters 标签".to_string(),
            content: "# 通过哈伯格税治理\n\n内容".to_string(),
            html_content: "<h1>通过哈伯格税治理</h1><p>内容</p>".to_string(),
            url_path: "articles/通过哈伯格税治理-matters-标签/index.html".to_string(),
            date: Some("2024-02-15".to_string()),
            reading_time: 8,
            slug: "通过哈伯格税治理-matters-标签".to_string(),
            permalink: "/articles/通过哈伯格税治理-matters-标签".to_string(),
            kind: PageKind::Article,
            uid: Some("def67890".to_string()),
            ..Default::default()
        },
    ];

    let map = build_article_map(&documents, &HashMap::new(), &[], &[], &Default::default());

    // Both articles MUST be in the map even though no source .md files
    // exist at the slug-derived paths
    assert_eq!(
        map.articles.len(),
        2,
        "Both special-character articles should be in the map"
    );

    // Check first article
    let article1 = map
        .articles
        .get("articles/蓝海螺-一个童话/")
        .expect("Article with fullwidth colon in original filename should be in map");
    assert_eq!(article1.title, "蓝海螺：一个童话");
    assert_eq!(
        article1.html_content.as_deref(),
        Some("<h1>蓝海螺：一个童话</h1><p>故事内容</p>")
    );
    assert_eq!(article1.date, Some("2024-03-01".to_string()));
    assert_eq!(article1.uid, Some("abc12345".to_string()));

    // Check second article
    let article2 = map
        .articles
        .get("articles/通过哈伯格税治理-matters-标签/")
        .expect("Article with spaces in original filename should be in map");
    assert_eq!(article2.title, "通过哈伯格税治理 Matters 标签");
    assert_eq!(article2.uid, Some("def67890".to_string()));
}

// ===== Tests for parse_frontmatter() =====

#[test]
fn test_parse_frontmatter_basic() {
    let content = "---\ntitle: Hello\ndate: 2024-01-15\n---\n\n# Content";
    let fm = parse_frontmatter(content);
    assert_eq!(fm.get("title"), Some(&Value::String("Hello".into())));
    assert_eq!(fm.get("date"), Some(&Value::String("2024-01-15".into())));
}

#[test]
fn test_parse_frontmatter_with_tags_array() {
    let content = "---\ntitle: Test\ntags:\n  - rust\n  - wasm\n---\n\nBody";
    let fm = parse_frontmatter(content);
    assert_eq!(
        fm.get("tags"),
        Some(&Value::Array(vec![
            Value::String("rust".into()),
            Value::String("wasm".into()),
        ]))
    );
}

#[test]
fn test_parse_frontmatter_with_syndicated_array() {
    let content = "---\ntitle: My Post\nsyndicated:\n  - https://matters.town/@user/123\n  - https://medium.com/post/456\n---\n\nBody";
    let fm = parse_frontmatter(content);
    let syndicated = fm.get("syndicated").unwrap();
    assert!(syndicated.is_array());
    let arr = syndicated.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(
        arr[0],
        Value::String("https://matters.town/@user/123".into())
    );
}

#[test]
fn test_parse_frontmatter_with_cover_string() {
    let content = "---\ntitle: Photo Post\ncover: https://example.com/photo.jpg\n---\n\nBody";
    let fm = parse_frontmatter(content);
    assert_eq!(
        fm.get("cover"),
        Some(&Value::String("https://example.com/photo.jpg".into()))
    );
}

#[test]
fn test_parse_frontmatter_no_frontmatter() {
    let content = "# Just a heading\n\nNo frontmatter here.";
    let fm = parse_frontmatter(content);
    assert!(fm.is_empty());
}

#[test]
fn test_parse_frontmatter_empty_frontmatter() {
    let content = "---\n---\n\nBody";
    let fm = parse_frontmatter(content);
    assert!(fm.is_empty());
}

#[test]
fn test_parse_frontmatter_malformed_yaml() {
    let content = "---\ntitle: [unclosed bracket\n---\n\nBody";
    let fm = parse_frontmatter(content);
    // Malformed YAML should return empty HashMap, not panic
    assert!(fm.is_empty());
}

#[test]
fn test_parse_frontmatter_with_bom() {
    let content = "\u{feff}---\ntitle: BOM Test\n---\n\nBody";
    let fm = parse_frontmatter(content);
    assert_eq!(fm.get("title"), Some(&Value::String("BOM Test".into())));
}

#[test]
fn test_parse_frontmatter_boolean_and_number_values() {
    let content = "---\ndraft: true\nweight: 10\n---\n\nBody";
    let fm = parse_frontmatter(content);
    assert_eq!(fm.get("draft"), Some(&Value::Bool(true)));
    assert_eq!(fm.get("weight"), Some(&Value::Number(10.into())));
}

#[test]
fn test_parse_frontmatter_crlf_line_endings() {
    // Windows-created files use \r\n line endings
    let content = "---\r\ntitle: CRLF Test\r\ntags:\r\n  - one\r\n  - two\r\n---\r\n\r\n# Content";
    let fm = parse_frontmatter(content);
    assert_eq!(fm.get("title"), Some(&Value::String("CRLF Test".into())));
    assert_eq!(
        fm.get("tags"),
        Some(&Value::Array(vec![
            Value::String("one".into()),
            Value::String("two".into()),
        ]))
    );
}

#[test]
fn test_parse_frontmatter_closing_delimiter_with_whitespace() {
    // Closing delimiter may have leading/trailing whitespace
    let content = "---\ntitle: Spaced\n  ---  \n\n# Content";
    let fm = parse_frontmatter(content);
    assert_eq!(fm.get("title"), Some(&Value::String("Spaced".into())));
}

#[test]
fn test_parse_frontmatter_closing_delimiter_with_leading_spaces() {
    // Closing delimiter with only leading spaces (no trailing)
    let content = "---\ntitle: Indented Close\n   ---\n\nBody";
    let fm = parse_frontmatter(content);
    assert_eq!(
        fm.get("title"),
        Some(&Value::String("Indented Close".into()))
    );
}

#[test]
fn test_parse_frontmatter_crlf_with_whitespace_delimiter() {
    // Combine both: CRLF line endings AND whitespace around closing delimiter
    let content = "---\r\ntitle: Both Issues\r\n  ---  \r\n\r\nBody";
    let fm = parse_frontmatter(content);
    assert_eq!(fm.get("title"), Some(&Value::String("Both Issues".into())));
}

// ===== Tests for extract_tags() =====

#[test]
fn test_extract_tags_from_frontmatter() {
    let mut fm = std::collections::BTreeMap::new();
    fm.insert(
        "tags".to_string(),
        Value::Array(vec![
            Value::String("rust".into()),
            Value::String("wasm".into()),
        ]),
    );
    let tags = extract_tags(&fm);
    assert_eq!(tags, vec!["rust".to_string(), "wasm".to_string()]);
}

#[test]
fn test_extract_tags_missing() {
    let fm = std::collections::BTreeMap::new();
    let tags = extract_tags(&fm);
    assert!(tags.is_empty());
}

#[test]
fn test_extract_tags_not_array() {
    let mut fm = std::collections::BTreeMap::new();
    fm.insert("tags".to_string(), Value::String("not-an-array".into()));
    let tags = extract_tags(&fm);
    assert!(tags.is_empty());
}

// ===== Tests for build_article_map field population =====

#[test]
fn test_build_article_map_populates_source_path() {
    let documents = vec![ParsedDocument {
        title: "Test".to_string(),
        label: "Test".to_string(),
        url_path: "posts/test/index.html".to_string(),
        reading_time: 1,
        slug: "test".to_string(),
        permalink: "/posts/test".to_string(),
        kind: PageKind::Article,
        source_path: Some("posts/test-article.md".to_string()),
        ..Default::default()
    }];

    let map = build_article_map(&documents, &HashMap::new(), &[], &[], &Default::default());
    let article = map.articles.get("posts/test/").unwrap();
    assert_eq!(article.source_path, "posts/test-article.md");
}

#[test]
fn test_build_article_map_source_path_defaults_to_empty() {
    // When source_path is None, should default to empty string
    let documents = vec![make_doc("Test", "posts/test/index.html", false)];
    let map = build_article_map(&documents, &HashMap::new(), &[], &[], &Default::default());
    let article = map.articles.get("posts/test/").unwrap();
    assert_eq!(article.source_path, "");
}

#[test]
fn test_build_article_map_populates_frontmatter_and_tags() {
    // Simulate a ParsedDocument with raw_frontmatter populated
    let mut raw_fm = std::collections::BTreeMap::new();
    raw_fm.insert("title".to_string(), Value::String("My Post".into()));
    raw_fm.insert(
        "tags".to_string(),
        Value::Array(vec![
            Value::String("rust".into()),
            Value::String("web".into()),
        ]),
    );
    raw_fm.insert(
        "syndicated".to_string(),
        Value::Array(vec![Value::String("https://matters.town/@user/123".into())]),
    );
    raw_fm.insert(
        "cover".to_string(),
        Value::String("https://example.com/cover.jpg".into()),
    );

    let documents = vec![ParsedDocument {
        title: "My Post".to_string(),
        label: "My Post".to_string(),
        content: "Body text".to_string(),
        html_content: "<p>Body text</p>".to_string(),
        url_path: "posts/my-post/index.html".to_string(),
        date: Some("2024-06-01".to_string()),
        reading_time: 3,
        slug: "my-post".to_string(),
        permalink: "/posts/my-post".to_string(),
        cover: Some("https://example.com/cover.jpg".to_string()),
        tags: Some(vec!["rust".into(), "web".into()]),
        kind: PageKind::Article,
        uid: Some("abc123".to_string()),
        source_path: Some("posts/my-post.md".to_string()),
        raw_frontmatter: raw_fm,
        ..Default::default()
    }];

    let map = build_article_map(&documents, &HashMap::new(), &[], &[], &Default::default());
    let article = map.articles.get("posts/my-post/").unwrap();

    // source_path should be populated
    assert_eq!(article.source_path, "posts/my-post.md");

    // frontmatter should contain all raw fields including syndicated
    assert_eq!(
        article.frontmatter.get("title"),
        Some(&Value::String("My Post".into()))
    );
    assert_eq!(
        article.frontmatter.get("cover"),
        Some(&Value::String("https://example.com/cover.jpg".into()))
    );
    let syndicated = article.frontmatter.get("syndicated").unwrap();
    assert!(syndicated.is_array());
    assert_eq!(
        syndicated.as_array().unwrap()[0],
        Value::String("https://matters.town/@user/123".into())
    );

    // tags should be extracted from frontmatter
    assert_eq!(article.tags, vec!["rust".to_string(), "web".to_string()]);
}

#[test]
fn test_build_article_map_empty_frontmatter_gives_empty_tags() {
    // When raw_frontmatter is empty, tags should be empty
    let documents = vec![make_doc("Test", "posts/test/index.html", false)];
    let map = build_article_map(&documents, &HashMap::new(), &[], &[], &Default::default());
    let article = map.articles.get("posts/test/").unwrap();
    assert!(article.tags.is_empty());
    assert!(article.frontmatter.is_empty());
}

#[test]
fn test_build_article_map_resolves_cover_paths_through_dir_overrides() {
    // Cover paths in frontmatter should be resolved through dir_overrides
    // so plugins receive URL-ready paths, not source paths with Chinese names.
    let mut doc = make_doc("Review", "writings/reviews/tools/index.html", false);
    doc.raw_frontmatter.insert(
        "cover".to_string(),
        Value::String("图片/配图/286b63.jpg".to_string()),
    );

    let mut dir_overrides = HashMap::new();
    dir_overrides.insert("图片".to_string(), "image".to_string());
    dir_overrides.insert("图片/配图".to_string(), "assets".to_string());

    let documents = vec![doc];
    let map = build_article_map(&documents, &dir_overrides, &[], &[], &Default::default());
    let article = map.articles.get("writings/reviews/tools/").unwrap();

    assert_eq!(
        article.frontmatter.get("cover"),
        Some(&Value::String("image/assets/286b63.jpg".to_string())),
        "Cover path should be resolved through dir_overrides"
    );
}

#[test]
fn test_build_article_map_leaves_http_covers_unchanged() {
    // HTTP/HTTPS cover URLs should not be modified by dir_overrides
    let mut doc = make_doc("Post", "blog/post/index.html", false);
    doc.raw_frontmatter.insert(
        "cover".to_string(),
        Value::String("https://example.com/cover.jpg".to_string()),
    );

    let dir_overrides = HashMap::new();
    let documents = vec![doc];
    let map = build_article_map(&documents, &dir_overrides, &[], &[], &Default::default());
    let article = map.articles.get("blog/post/").unwrap();

    assert_eq!(
        article.frontmatter.get("cover"),
        Some(&Value::String("https://example.com/cover.jpg".to_string())),
        "HTTP cover URLs should remain unchanged"
    );
}
