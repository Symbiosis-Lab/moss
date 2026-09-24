use super::*;
use crate::build::folder_embed::{folder_latest_date, generate_children, resolve_children_config};

/// Helper function to create a minimal ParsedDocument for testing
fn make_test_doc(title: &str, url_path: &str) -> ParsedDocument {
    make_test_doc_with_nav(title, url_path, None, false)
}

/// A folder index — what `index.md` / `readme.md` / a folder note becomes, and
/// what the render synthesizes for a directory with no page of its own.
///
/// The kind is not decoration: every listing filter in the build asks
/// `PageKind`, so a test built only from `make_test_doc` (Article) cannot see
/// whether a folder is excluded — which is how a childless folder index stayed
/// listed as an article for as long as it did.
fn make_folder_doc(title: &str, url_path: &str) -> ParsedDocument {
    ParsedDocument {
        kind: PageKind::Folder,
        ..make_test_doc(title, url_path)
    }
}

/// Helper with nav and is_root_level parameters
fn make_test_doc_with_nav(
    title: &str,
    url_path: &str,
    nav: Option<bool>,
    is_root_level: bool,
) -> ParsedDocument {
    ParsedDocument {
        title: title.to_string(),
        label: title.to_string(),
        url_path: url_path.to_string(),
        reading_time: 1,
        slug: title.to_lowercase().replace(' ', "-"),
        permalink: format!("/{}", url_path), // allow:served-path-url-construct (test fixture — permalink field, not HTML-emitted URL)
        nav,
        is_root_level,
        kind: PageKind::Article,
        ..Default::default()
    }
}

#[test]
fn test_generate_year_grouped_article_list_filters_index_files() {
    // Homepage and folder index pages are filtered out. `Asset Folder Index`
    // is the regression case: a synthetic index for a directory that holds
    // only images, so it has no document descendants and no source file. The
    // structural "does anything live under this prefix?" test this function
    // used to run let it through, and it rendered as an article dated the
    // literal word `Unknown`.
    let docs = vec![
        make_folder_doc("Root Index", "index.html"),
        make_test_doc("Article One", "post-one/index.html"),
        make_folder_doc("Folder Index", "游记/index.html"),
        make_test_doc("Article Two", "游记/post-two/index.html"),
        make_folder_doc("Nested Index", "docs/guide/index.html"),
        make_test_doc("Article Three", "docs/guide/setup/index.html"),
        make_folder_doc("Asset Folder Index", "assets/gallery/index.html"),
    ];

    let project = ProjectStructure {
        root_path: String::new(),
        markdown_files: vec![],
        html_files: vec![],
        image_files: vec![],
        video_files: vec![],
        notebook_files: vec![],
        other_files: vec![],
        total_files: 0,
        homepage_file: None,
        ffmpeg_bin_path: None,
        evicted_count: 0,
        evicted_paths: Vec::new(),
        has_content_folders: false,
        has_language_trees: false,
        passthrough_roots: std::collections::HashSet::new(),
        dirs: Vec::new(),
    };

    let result = generate_year_grouped_article_list(&docs, &project, true, Language::En, None);

    // Should include articles but NOT any index.html files
    assert!(result.contains("Article One"), "Should include Article One");
    assert!(result.contains("Article Two"), "Should include Article Two");
    assert!(
        result.contains("Article Three"),
        "Should include Article Three"
    );

    // Should NOT include any index pages
    assert!(
        !result.contains("Root Index"),
        "Should NOT include Root Index"
    );
    assert!(
        !result.contains("Folder Index"),
        "Should NOT include Folder Index"
    );
    assert!(
        !result.contains("Nested Index"),
        "Should NOT include Nested Index"
    );
    assert!(
        !result.contains("Asset Folder Index"),
        "A folder index with no descendants is still a folder, not an article"
    );
}

#[test]
fn test_generate_year_grouped_article_list_excludes_nav_items() {
    // Nav items should be excluded from article list (they appear in navigation)
    let docs = vec![
        // Root-level auto-nav items (should be excluded)
        make_test_doc_with_nav("Research", "research/index.html", None, true),
        make_test_doc_with_nav("People", "people/index.html", None, true),
        // Explicit nav:true (should be excluded)
        make_test_doc_with_nav("Articles", "articles/index.html", Some(true), false),
        // Explicit nav:false on root-level (should be included - opted out of nav)
        make_test_doc_with_nav("Hidden Page", "hidden/index.html", Some(false), true),
        // Regular nested articles (should be included)
        make_test_doc_with_nav("Blog Post", "posts/blog-post/index.html", None, false),
        // Homepage (always excluded)
        make_test_doc_with_nav("Home", "index.html", None, true),
    ];

    let project = ProjectStructure {
        root_path: String::new(),
        markdown_files: vec![],
        html_files: vec![],
        image_files: vec![],
        video_files: vec![],
        notebook_files: vec![],
        other_files: vec![],
        total_files: 0,
        homepage_file: None,
        ffmpeg_bin_path: None,
        evicted_count: 0,
        evicted_paths: Vec::new(),
        has_content_folders: true,
        has_language_trees: false,
        passthrough_roots: std::collections::HashSet::new(),
        dirs: Vec::new(),
    };

    let result = generate_year_grouped_article_list(&docs, &project, true, Language::En, None);

    // Should include: Hidden Page (nav:false), Blog Post (nested)
    assert!(
        result.contains("Hidden Page"),
        "Should include nav:false page"
    );
    assert!(
        result.contains("Blog Post"),
        "Should include nested article"
    );

    // Should NOT include: nav items (Research, People, Articles) or Home
    assert!(
        !result.contains("Research"),
        "Should NOT include auto-nav item Research"
    );
    assert!(
        !result.contains("People"),
        "Should NOT include auto-nav item People"
    );
    assert!(
        !result.contains("Articles"),
        "Should NOT include explicit nav:true Articles"
    );
    assert!(!result.contains(">Home<"), "Should NOT include homepage");
}

#[test]
fn test_year_grouped_list_uses_pretty_urls() {
    let mut post = make_test_doc("My Post", "blog/my-post/index.html");
    post.date = Some("2025-06-15".to_string());

    let docs = vec![post];

    let project = ProjectStructure {
        root_path: String::new(),
        markdown_files: vec![],
        html_files: vec![],
        image_files: vec![],
        video_files: vec![],
        notebook_files: vec![],
        other_files: vec![],
        total_files: 0,
        homepage_file: None,
        ffmpeg_bin_path: None,
        evicted_count: 0,
        evicted_paths: Vec::new(),
        has_content_folders: false,
        has_language_trees: false,
        passthrough_roots: std::collections::HashSet::new(),
        dirs: Vec::new(),
    };

    let result = generate_year_grouped_article_list(&docs, &project, true, Language::En, None);

    assert!(
        result.contains(r#"href="blog/my-post/""#),
        "Year-grouped list should use pretty URL 'blog/my-post/'. Got: {}",
        result
    );
    assert!(
        !result.contains("index.html"),
        "Year-grouped list should NOT contain index.html"
    );
}

// =============================================================================
// generate_children Tests
// =============================================================================

fn make_project() -> ProjectStructure {
    ProjectStructure {
        root_path: String::new(),
        markdown_files: vec![],
        html_files: vec![],
        image_files: vec![],
        video_files: vec![],
        notebook_files: vec![],
        other_files: vec![],
        total_files: 0,
        homepage_file: None,
        ffmpeg_bin_path: None,
        evicted_count: 0,
        evicted_paths: Vec::new(),
        has_content_folders: false,
        has_language_trees: false,
        passthrough_roots: std::collections::HashSet::new(),
        dirs: Vec::new(),
    }
}

#[test]
fn test_generate_children_empty() {
    // An empty folder produces no listing markup at all — no placeholder,
    // no empty card container.
    let project = make_project();
    let result = generate_children(
        &[],
        &[],
        &project,
        "list",
        "none",
        Language::En,
        false,
        &std::collections::HashMap::new(),
        None,
        None,
        None,
        true,
        false,
        &Default::default(),
    );
    assert_eq!(result, "");
}

#[test]
fn test_generate_children_list_no_group() {
    let mut art1 = make_test_doc("Article A", "blog/art-a/index.html");
    art1.date = Some("2025-01-15".to_string());

    let mut art2 = make_test_doc("Article B", "blog/art-b/index.html");
    art2.date = Some("2025-03-10".to_string());

    let docs = vec![&art1, &art2];
    let project = make_project();

    let result = generate_children(
        &docs,
        &docs,
        &project,
        "list",
        "none",
        Language::En,
        false,
        &std::collections::HashMap::new(),
        None,
        None,
        None,
        true,
        false,
        &Default::default(),
    );

    assert!(result.contains("Article A"));
    assert!(result.contains("Article B"));
    // `list` is always the compact index → data-layout="minimal", even with
    // group="none" (flat, no year sections).
    assert!(result.contains(r#"data-layout="minimal""#));
    // Should contain div.moss-card (child_list::render_child uses div)
    assert!(result.contains(r#"<div class="moss-card">"#));
}

#[test]
fn test_generate_children_summary_no_group() {
    let mut art1 = make_test_doc("Article A", "blog/art-a/index.html");
    art1.date = Some("2025-01-15".to_string());

    let docs = vec![&art1];
    let project = make_project();

    let result = generate_children(
        &docs,
        &docs,
        &project,
        "summary",
        "none",
        Language::En,
        false,
        &std::collections::HashMap::new(),
        None,
        None,
        None,
        true,
        false,
        &Default::default(),
    );

    assert!(result.contains("Article A"));
    // `moss-summary-layout` co-class is retired; summary style is now
    // identified by `data-layout="list"` alone.
    assert!(
        result.contains(r#"data-layout="list""#),
        "Summary style should emit data-layout=list. Got: {}",
        result
    );
    assert!(result.contains(r#"class="moss-card""#));
}

#[test]
fn test_generate_children_folders_before_articles() {
    // Create a folder index doc
    let mut folder = make_test_doc("Tutorials", "blog/tutorials/index.html");
    folder.kind = PageKind::Folder;
    folder.date = None;

    // A child inside the folder (so it counts as a folder with children)
    let mut child = make_test_doc("Tutorial One", "blog/tutorials/one/index.html");
    child.date = Some("2025-06-01".to_string());

    // A regular article
    let mut article = make_test_doc("Article Z", "blog/art-z/index.html");
    article.date = Some("2025-12-01".to_string());

    // folder_docs = what we're rendering (folder + article)
    let folder_docs = vec![&folder, &article];
    // all_folder_docs = all docs including children (for counting)
    let all_docs = vec![&folder, &child, &article];
    let project = make_project();

    let result = generate_children(
        &folder_docs,
        &all_docs,
        &project,
        "list",
        "none",
        Language::En,
        false,
        &std::collections::HashMap::new(),
        None,
        None,
        None,
        true,
        false,
        &Default::default(),
    );

    let folder_pos = result.find("Tutorials").expect("Should contain Tutorials");
    let article_pos = result.find("Article Z").expect("Should contain Article Z");
    assert!(
        folder_pos < article_pos,
        "Folders should appear before articles"
    );
}

#[test]
fn test_generate_children_folder_sorted_by_latest_date() {
    // Folder A has newer articles
    let mut folder_a = make_test_doc("Folder A", "blog/folder-a/index.html");
    folder_a.kind = PageKind::Folder;
    let mut child_a = make_test_doc("Child A", "blog/folder-a/child/index.html");
    child_a.date = Some("2025-01-01".to_string());

    // Folder B has older articles
    let mut folder_b = make_test_doc("Folder B", "blog/folder-b/index.html");
    folder_b.kind = PageKind::Folder;
    let mut child_b = make_test_doc("Child B", "blog/folder-b/child/index.html");
    child_b.date = Some("2025-06-15".to_string());

    let folder_docs = vec![&folder_a, &folder_b];
    let all_docs = vec![&folder_a, &child_a, &folder_b, &child_b];
    let project = make_project();

    let result = generate_children(
        &folder_docs,
        &all_docs,
        &project,
        "list",
        "none",
        Language::En,
        false,
        &std::collections::HashMap::new(),
        None,
        None,
        None,
        true,
        false,
        &Default::default(),
    );

    let pos_a = result.find("Folder A").unwrap();
    let pos_b = result.find("Folder B").unwrap();
    // Folder B has newer latest date (2025-06-15) so should come first
    assert!(
        pos_b < pos_a,
        "Folder B (newer) should come before Folder A (older). Got: {}",
        result
    );
}

#[test]
fn test_folder_latest_date() {
    let folder = ParsedDocument {
        url_path: "blog/tutorials/index.html".to_string(),
        ..make_test_doc("Tutorials", "blog/tutorials/index.html")
    };

    let mut child1 = make_test_doc("Child 1", "blog/tutorials/one/index.html");
    child1.date = Some("2025-01-15".to_string());

    let mut child2 = make_test_doc("Child 2", "blog/tutorials/two/index.html");
    child2.date = Some("2025-06-20".to_string());

    let mut unrelated = make_test_doc("Unrelated", "blog/other/index.html");
    unrelated.date = Some("2025-12-31".to_string());

    let all_docs: Vec<&ParsedDocument> = vec![&folder, &child1, &child2, &unrelated];
    let project = make_project();

    let result = folder_latest_date(&folder, &all_docs, &project.root_path);
    assert_eq!(result, Some("2025-06-20".to_string()));
}

#[test]
fn test_generate_children_pre_sorted_card_style() {
    // When pre_sorted=true, generate_children should render in given order
    // using card style (proving series/ordered folders respect children_style)
    let mut doc_a = make_test_doc("Chapter 1", "series/ch1/index.html");
    doc_a.clean_stem = "ch1".to_string();
    doc_a.date = Some("2025-01-01".to_string());

    let mut doc_b = make_test_doc("Chapter 2", "series/ch2/index.html");
    doc_b.clean_stem = "ch2".to_string();
    doc_b.date = Some("2025-06-01".to_string());

    // Order: ch1 then ch2 (ch2 has newer date but should come second)
    let docs = vec![&doc_a, &doc_b];
    let project = make_project();

    let result = generate_children(
        &docs,
        &docs,
        &project,
        "summary",
        "none",
        Language::En,
        true,
        &std::collections::HashMap::new(),
        None,
        None,
        None,
        true,
        false,
        &Default::default(),
    );

    // Should use summary rendering — identified by `data-layout="list"`
    // since the `moss-summary-layout` co-class is retired.
    assert!(
        result.contains(r#"data-layout="list""#),
        "Summary style should emit data-layout=list. Got: {}",
        result
    );
    assert!(
        result.contains(r#"class="moss-card""#),
        "Should render child summaries. Got: {}",
        result
    );

    // Should preserve given order (ch1 before ch2)
    let pos1 = result.find("Chapter 1").expect("Should contain Chapter 1");
    let pos2 = result.find("Chapter 2").expect("Should contain Chapter 2");
    assert!(
        pos1 < pos2,
        "Pre-sorted order should be preserved (ch1 before ch2). Got: {}",
        result
    );
}

#[test]
fn test_generate_children_pre_sorted_preserves_order() {
    // Verify pre_sorted=true does NOT re-sort by date
    let mut newer = make_test_doc("Newer Post", "blog/newer/index.html");
    newer.date = Some("2025-12-01".to_string());

    let mut older = make_test_doc("Older Post", "blog/older/index.html");
    older.date = Some("2025-01-01".to_string());

    // Pass older first — with pre_sorted=true, older should stay first
    let docs = vec![&older, &newer];
    let project = make_project();

    let result = generate_children(
        &docs,
        &docs,
        &project,
        "list",
        "none",
        Language::En,
        true,
        &std::collections::HashMap::new(),
        None,
        None,
        None,
        true,
        false,
        &Default::default(),
    );

    let pos_older = result
        .find("Older Post")
        .expect("Should contain Older Post");
    let pos_newer = result
        .find("Newer Post")
        .expect("Should contain Newer Post");
    assert!(
        pos_older < pos_newer,
        "Pre-sorted should preserve order. Got: {}",
        result
    );
}

#[test]
fn test_folder_latest_date_no_children() {
    let folder = make_test_doc("Empty", "blog/empty/index.html");
    let all_docs: Vec<&ParsedDocument> = vec![&folder];
    let project = make_project();
    let result = folder_latest_date(&folder, &all_docs, &project.root_path);
    assert_eq!(result, None);
}

#[test]
fn test_generate_children_summary_has_divider_between_folders_and_articles() {
    let mut folder = make_test_doc("Tutorials", "blog/tutorials/index.html");
    folder.kind = PageKind::Folder;
    let mut child = make_test_doc("Tutorial One", "blog/tutorials/one/index.html");
    child.date = Some("2025-06-01".to_string());
    let mut article = make_test_doc("Article Z", "blog/art-z/index.html");
    article.date = Some("2025-12-01".to_string());

    let folder_docs = vec![&folder, &article];
    let all_docs = vec![&folder, &child, &article];
    let project = make_project();

    let result = generate_children(
        &folder_docs,
        &all_docs,
        &project,
        "summary",
        "none",
        Language::En,
        false,
        &std::collections::HashMap::new(),
        None,
        None,
        None,
        true,
        false,
        &Default::default(),
    );

    // Should have a divider between folder and article sections
    assert!(
        result.contains(r#"class="moss-child-section-divider""#),
        "Should have divider between folders and articles, got: {}",
        result
    );
    let divider_pos = result.find("moss-child-section-divider").unwrap();
    let folder_pos = result.find("Tutorials").unwrap();
    let article_pos = result.find("Article Z").unwrap();
    assert!(
        folder_pos < divider_pos,
        "Divider should come after folders"
    );
    assert!(
        divider_pos < article_pos,
        "Divider should come before articles"
    );
}

#[test]
fn test_generate_children_summary_no_divider_when_only_articles() {
    let mut art = make_test_doc("Solo Article", "blog/solo/index.html");
    art.date = Some("2025-01-15".to_string());

    let docs = vec![&art];
    let project = make_project();

    let result = generate_children(
        &docs,
        &docs,
        &project,
        "summary",
        "none",
        Language::En,
        false,
        &std::collections::HashMap::new(),
        None,
        None,
        None,
        true,
        false,
        &Default::default(),
    );
    assert!(
        !result.contains("moss-child-section-divider"),
        "No divider when only articles"
    );
}

// =============================================================================
// Year-grouping heuristic + summary tests
// =============================================================================

#[test]
fn test_year_group_triggered_by_multi_year() {
    // Create 3 docs with dates in 2016, 2019, 2025 (no covers)
    let mut doc1 = make_test_doc("Old Post", "blog/old/index.html");
    doc1.date = Some("2016-07-21".to_string());

    let mut doc2 = make_test_doc("Mid Post", "blog/mid/index.html");
    doc2.date = Some("2019-03-15".to_string());

    let mut doc3 = make_test_doc("New Post", "blog/new/index.html");
    doc3.date = Some("2025-11-01".to_string());

    let docs = vec![&doc1, &doc2, &doc3];
    let project = make_project();

    let result = generate_children(
        &docs,
        &docs,
        &project,
        "list",
        "year",
        Language::En,
        false,
        &std::collections::HashMap::new(),
        None,
        None,
        None,
        true,
        false,
        &Default::default(),
    );

    // Should contain moss-cards-minimal-year-group sections
    assert!(
        result.contains(r#"class="moss-cards-minimal-year-group"#),
        "Should contain moss-cards-minimal-year-group section class. Got: {}",
        result
    );
    // Should contain year headers
    assert!(
        result.contains("<h2>2025</h2>"),
        "Should contain 2025 header. Got: {}",
        result
    );
    assert!(
        result.contains("<h2>2019</h2>"),
        "Should contain 2019 header. Got: {}",
        result
    );
    assert!(
        result.contains("<h2>2016</h2>"),
        "Should contain 2016 header. Got: {}",
        result
    );
}

#[test]
fn test_no_year_group_for_single_year() {
    // Create 10 docs all from 2025 (no covers)
    let docs_owned: Vec<ParsedDocument> = (0..10)
        .map(|i| {
            let mut d = make_test_doc(
                &format!("Article {}", i),
                &format!("blog/art-{}/index.html", i),
            );
            d.date = Some(format!("2025-{:02}-15", (i % 12) + 1));
            d
        })
        .collect();
    let docs: Vec<&ParsedDocument> = docs_owned.iter().collect();
    let project = make_project();

    let result = generate_children(
        &docs,
        &docs,
        &project,
        "list",
        "none",
        Language::En,
        false,
        &std::collections::HashMap::new(),
        None,
        None,
        None,
        true,
        false,
        &Default::default(),
    );

    // Should NOT contain moss-cards-minimal-year-group
    assert!(
        !result.contains("moss-cards-minimal-year-group"),
        "Single-year docs with group=none should NOT have moss-cards-minimal-year-group. Got: {}",
        result
    );
}

#[test]
fn test_year_group_with_summary_style() {
    // Create docs spanning multiple years WITH covers
    let mut doc1 = make_test_doc("Old Article", "blog/old/index.html");
    doc1.date = Some("2016-07-21".to_string());
    doc1.cover = Some("/images/old.jpg".to_string());
    doc1.description = Some("An old article".to_string());

    let mut doc2 = make_test_doc("New Article", "blog/new/index.html");
    doc2.date = Some("2025-11-01".to_string());
    doc2.cover = Some("/images/new.jpg".to_string());
    doc2.description = Some("A new article".to_string());

    let docs = vec![&doc1, &doc2];
    let project = make_project();

    let result = generate_children(
        &docs,
        &docs,
        &project,
        "summary",
        "year",
        Language::En,
        false,
        &std::collections::HashMap::new(),
        None,
        None,
        None,
        true,
        false,
        &Default::default(),
    );

    // Should contain moss-cards-minimal-year-group--summary sections for ALL year groups (2025 and 2016)
    let summary_count = result
        .matches(r#"class="moss-cards-minimal-year-group moss-cards-minimal-year-group--summary""#)
        .count();
    assert_eq!(summary_count, 2,
            "Expected 2 moss-cards-minimal-year-group--summary sections (one per year), got {}. Result: {}", summary_count, result);
    // No bare moss-cards-minimal-year-group section should exist (every section must carry --summary)
    // Strip the compound class occurrences then check no plain class remains
    let stripped = result.replace(
        r#"class="moss-cards-minimal-year-group moss-cards-minimal-year-group--summary""#,
        "",
    );
    assert!(
        !stripped.contains(r#"class="moss-cards-minimal-year-group""#),
        "Found bare moss-cards-minimal-year-group section without --summary suffix. Result: {}",
        result
    );
    // Should contain year headers
    assert!(
        result.contains("<h2>2025</h2>"),
        "Should contain 2025 header. Got: {}",
        result
    );
    assert!(
        result.contains("<h2>2016</h2>"),
        "Should contain 2016 header. Got: {}",
        result
    );
    // Should contain child-summary elements (summary style cards)
    assert!(
        result.contains(r#"class="moss-card""#),
        "Should contain child-summary cards. Got: {}",
        result
    );
    // The wrapper is plain `.moss-cards` (the `moss-summary-layout`
    // co-class is retired).
    assert!(
        result.contains(r#"class="moss-cards""#),
        "Should have moss-cards wrapper. Got: {}",
        result
    );
}

#[test]
fn resolve_children_config_covers_the_two_by_two() {
    // The complete table over (rich, dated) — one test instead of three,
    // because the cells only mean anything against each other.
    //
    // The first cell changed on 2026-09-06: a listing carrying nothing at
    // all is an index of bare labels, not an archive, and "summary" laid it
    // out one-per-row (56 author names, ~2,400px of scroll on the reference
    // vault). It now selects "grid", whose coverless state site.css styles
    // as a roster.
    let bare_a = make_test_doc("Topic A", "topics/a/index.html");
    let bare_b = make_test_doc("Topic B", "topics/b/index.html");

    let mut dated = make_test_doc("Dated", "topics/d/index.html");
    dated.date = Some("2026-01-01".to_string());

    let mut rich = make_test_doc("Rich", "topics/r/index.html");
    rich.description = Some("A described child".to_string());

    let mut rich_and_dated = make_test_doc("Both", "topics/b2/index.html");
    rich_and_dated.description = Some("A described child".to_string());
    rich_and_dated.date = Some("2026-01-01".to_string());

    for (docs, expected, why) in [
        (vec![&bare_a, &bare_b], "grid", "nothing at all -> an index of labels"),
        (vec![&bare_a, &dated], "list", "a date and nothing rich -> a chronological archive"),
        (vec![&bare_a, &rich], "summary", "rich anywhere -> cards worth showing at card size"),
        (vec![&rich_and_dated], "summary", "rich outranks dated"),
    ] {
        let (style, group) = resolve_children_config(&docs, None, None, true);
        assert_eq!(style.value, expected, "{}", why);
        assert!(!style.is_explicit(), "auto-detected style is never explicit: {}", why);
        // "grid" is only reachable when nothing is dated, so it can never
        // auto-group by year — the year heading needs a date to print.
        if expected == "grid" {
            assert_eq!(group.value, "none", "an undated index has no year groups");
        }
    }
}

#[test]
fn test_resolve_children_config_mixed_date_presence_uses_list() {
    // If AT LEAST ONE child has an explicit date, "list" should be selected
    // (the date-bearing child drives the archive layout; undated siblings
    // fall through to the title-only render branch in render_child).
    let mut doc1 = make_test_doc("Post A", "blog/a/index.html");
    doc1.date = Some("2025-06-01".to_string());
    let doc2 = make_test_doc("Post B", "blog/b/index.html"); // no date
    let docs = vec![&doc1, &doc2];

    let (style, _group) = resolve_children_config(&docs, None, None, true);
    assert_eq!(
        style.value, "list",
        "Mixed presence: at least one dated child should still select 'list'. Got: {:?}",
        style.value
    );
}

#[test]
fn test_generate_children_strips_markdown_from_frontmatter_description() {
    let mut doc = make_test_doc("Article", "blog/article/index.html");
    doc.date = Some("2025-01-15".to_string());
    doc.description = Some("A **bold** claim about [something](https://example.com)".to_string());

    let docs = vec![&doc];
    let project = make_project();

    let result = generate_children(
        &docs,
        &docs,
        &project,
        "summary",
        "none",
        Language::En,
        false,
        &std::collections::HashMap::new(),
        None,
        None,
        None,
        true,
        false,
        &Default::default(),
    );

    // Markdown syntax should be stripped from the description
    assert!(
        !result.contains("**"),
        "Should strip bold markdown: {}",
        result
    );
    assert!(
        !result.contains("[something]"),
        "Should strip link markdown: {}",
        result
    );
    assert!(!result.contains("]("), "Should strip link URL: {}", result);
    assert!(
        result.contains("A bold claim about something"),
        "Should contain plain text description: {}",
        result
    );
}

#[test]
fn test_generate_children_strips_markdown_from_content_extracted_description() {
    let mut doc = make_test_doc("Article", "blog/article/index.html");
    doc.date = Some("2025-01-15".to_string());
    // No frontmatter description — will be extracted from content
    doc.content = "This has **bold** and a [link](https://example.com) in it.".to_string();

    let docs = vec![&doc];
    let project = make_project();

    let result = generate_children(
        &docs,
        &docs,
        &project,
        "summary",
        "none",
        Language::En,
        false,
        &std::collections::HashMap::new(),
        None,
        None,
        None,
        true,
        false,
        &Default::default(),
    );

    // Content-extracted descriptions should also be stripped (already works)
    assert!(
        !result.contains("**"),
        "Should strip bold markdown: {}",
        result
    );
    assert!(
        result.contains("This has bold and a link in it"),
        "Should contain plain text description: {}",
        result
    );
}

#[test]
fn test_generate_children_grid_style_renders_collection_grid() {
    let mut doc1 = make_test_doc("Project Alpha", "projects/alpha/index.html");
    doc1.kind = PageKind::Folder;
    doc1.cover = Some("assets/alpha-cover.jpg".to_string());

    let mut doc2 = make_test_doc("Project Beta", "projects/beta/index.html");
    doc2.kind = PageKind::Folder;
    doc2.cover = Some("assets/beta-cover.jpg".to_string());

    let docs = vec![&doc1, &doc2];
    let project = make_project();

    let result = generate_children(
        &docs,
        &docs,
        &project,
        "grid",
        "none",
        Language::En,
        false,
        &std::collections::HashMap::new(),
        None,
        None,
        None,
        true,
        false,
        &Default::default(),
    );

    // Should render as data-layout="grid", not data-layout="list"
    assert!(
        result.contains(r#"data-layout="grid""#),
        "Should use data-layout=\"grid\" wrapper. Got: {}",
        result
    );
    assert!(
        !result.contains(r#"data-layout="list""#),
        "Should NOT use data-layout=\"list\" wrapper. Got: {}",
        result
    );

    // Should render collection cards
    assert!(
        result.contains(r#"class="moss-card""#),
        "Should contain moss-card elements. Got: {}",
        result
    );
    assert!(
        result.contains("alpha-cover.jpg"),
        "Should include cover image for Project Alpha. Got: {}",
        result
    );
    assert!(
        result.contains("beta-cover.jpg"),
        "Should include cover image for Project Beta. Got: {}",
        result
    );

    // Should include project titles
    assert!(
        result.contains("Project Alpha"),
        "Should contain title Project Alpha. Got: {}",
        result
    );
    assert!(
        result.contains("Project Beta"),
        "Should contain title Project Beta. Got: {}",
        result
    );
}

#[test]
fn test_generate_children_grid_leaf_description_renders_below_title() {
    let mut doc = make_test_doc("Icy Materials", "projects/icy/index.html");
    doc.cover = Some("cover.jpg".to_string());
    doc.description = Some("Cheng Li, Jie Li".to_string());

    let docs = vec![&doc];
    let project = make_project();

    let result = generate_children(
        &docs,
        &docs,
        &project,
        "grid",
        "none",
        Language::En,
        false,
        &std::collections::HashMap::new(),
        None,
        None,
        None,
        true,
        false,
        &Default::default(),
    );

    // Description renders as a paragraph below the title...
    assert!(
        result.contains(r#"<p class="moss-card-description">Cheng Li, Jie Li</p>"#),
        "leaf description should render below the title. Got: {}",
        result
    );
    // ...NOT in the meta/subtitle slot above the title.
    assert!(
        !result.contains(r#"<span class="moss-card-meta">Cheng Li, Jie Li</span>"#),
        "description must not appear in the meta slot. Got: {}",
        result
    );
    assert!(
        !result.contains("0 articles"),
        "Should NOT show '0 articles'. Got: {}",
        result
    );
}

#[test]
fn test_generate_children_grid_folder_shows_count_not_description() {
    // Folder with children AND a description. Under the file-cards-only
    // rule, folder cards keep their article COUNT and do NOT render the
    // description (deliberate change from the old "description overrides
    // count" behavior).
    let mut folder = make_test_doc("Tutorials", "tutorials/index.html");
    folder.kind = PageKind::Folder;
    folder.cover = Some("cover.jpg".to_string());
    folder.description = Some("Learn to build websites".to_string());

    let child = make_test_doc("Getting Started", "tutorials/getting-started/index.html");

    let all_docs = vec![&folder, &child];
    let folder_docs = vec![&folder];
    let project = make_project();

    let result = generate_children(
        &folder_docs,
        &all_docs,
        &project,
        "grid",
        "none",
        Language::En,
        false,
        &std::collections::HashMap::new(),
        None,
        None,
        None,
        true,
        false,
        &Default::default(),
    );

    assert!(
        result.contains("1 article"),
        "folder card should show its count. Got: {}",
        result
    );
    assert!(
        !result.contains("Learn to build websites"),
        "folder card must NOT render a description (file cards only). Got: {}",
        result
    );
}

#[test]
fn test_generate_children_grid_childless_folder_renders_description() {
    // A folder with NO leaf descendants is classified as a file card
    // (child_count == None => article_count == 0), so its frontmatter
    // description renders below the title. Pins the emergent edge.
    let mut folder = make_test_doc("Empty Section", "empty-section/index.html");
    folder.kind = PageKind::Folder;
    folder.description = Some("A section with no entries yet.".to_string());

    let docs = vec![&folder];
    let project = make_project();

    let result = generate_children(
        &docs,
        &docs,
        &project,
        "grid",
        "none",
        Language::En,
        false,
        &std::collections::HashMap::new(),
        None,
        None,
        None,
        true,
        false,
        &Default::default(),
    );

    assert!(
        result.contains(r#"<p class="moss-card-description">A section with no entries yet.</p>"#),
        "childless folder should render its description as a file card. Got: {}",
        result
    );
}

#[test]
fn test_generate_children_grid_sorts_date_descending() {
    // Regression test: grid style was sorting by url_path (alphabetically)
    // instead of by publication date descending, causing older articles to appear
    // first when their URL paths sorted earlier alphabetically.

    // doc_a has an older date but its url_path ("projects/alpha/...") sorts
    // alphabetically BEFORE doc_b ("projects/gamma/..."), so the old url_path sort
    // would put Alpha first — wrong. Date-desc should put Gamma (newer) first.
    let mut doc_a = make_test_doc("Project Alpha", "projects/alpha/index.html");
    doc_a.kind = PageKind::Folder;
    doc_a.date = Some("2024-01-01".to_string()); // older

    let mut doc_b = make_test_doc("Project Gamma", "projects/gamma/index.html");
    doc_b.kind = PageKind::Folder;
    doc_b.date = Some("2025-06-01".to_string()); // newer

    let mut doc_c = make_test_doc("Project Beta", "projects/beta/index.html");
    doc_c.kind = PageKind::Folder;
    doc_c.date = Some("2024-09-15".to_string()); // middle

    // Pass in filesystem order (alpha, gamma, beta) — without date sort this
    // would stay alpha-first; with url_path sort it would be alpha, beta, gamma.
    let docs = vec![&doc_a, &doc_b, &doc_c];
    let project = make_project();

    let result = generate_children(
        &docs,
        &docs,
        &project,
        "grid",
        "none",
        Language::En,
        false,
        &std::collections::HashMap::new(),
        None,
        None,
        None,
        true,
        false,
        &Default::default(),
    );

    // Find the positions of each title in the output — date-desc = Gamma, Beta, Alpha
    let pos_alpha = result
        .find("Project Alpha")
        .expect("Alpha should be in output");
    let pos_beta = result
        .find("Project Beta")
        .expect("Beta should be in output");
    let pos_gamma = result
        .find("Project Gamma")
        .expect("Gamma should be in output");

    assert!(
            pos_gamma < pos_beta && pos_beta < pos_alpha,
            "Grid children must appear date-descending (Gamma 2025 > Beta 2024-09 > Alpha 2024-01). \
             Got positions: gamma={pos_gamma}, beta={pos_beta}, alpha={pos_alpha}\nResult: {result}"
        );
}

// =============================================================================
// count_listable_articles Tests
// =============================================================================

#[test]
fn test_count_listable_articles_recursive() {
    // Folder index at blog/tutorials/
    let mut folder = make_test_doc("Tutorials", "blog/tutorials/index.html");
    folder.kind = PageKind::Folder;

    // Direct child article
    let child1 = make_test_doc("Intro", "blog/tutorials/intro/index.html");

    // Nested child article (two levels deep)
    let child2 = make_test_doc("Advanced Topic", "blog/tutorials/advanced/topic/index.html");

    let all: Vec<&ParsedDocument> = vec![&folder, &child1, &child2];
    let count = crate::build::page::page::count_listable_articles(
        &all,
        "blog/tutorials/",
        "blog/tutorials/index.html",
    );

    assert_eq!(
        count, 2,
        "Should count both direct and nested leaf articles"
    );
}

#[test]
fn test_count_listable_articles_excludes_index_pages() {
    // Folder index
    let mut folder = make_test_doc("Blog", "blog/index.html");
    folder.kind = PageKind::Folder;

    // Sub-folder index (should be excluded from count)
    let mut subfolder = make_test_doc("Tutorials", "blog/tutorials/index.html");
    subfolder.kind = PageKind::Folder;

    // Leaf article inside sub-folder
    let leaf = make_test_doc("Lesson 1", "blog/tutorials/lesson-1/index.html");

    // Another leaf article directly in blog
    let leaf2 = make_test_doc("Post", "blog/my-post/index.html");

    let all: Vec<&ParsedDocument> = vec![&folder, &subfolder, &leaf, &leaf2];
    let count =
        crate::build::page::page::count_listable_articles(&all, "blog/", "blog/index.html");

    // subfolder is_index=true so excluded; leaf and leaf2 counted
    assert_eq!(
        count, 2,
        "Should exclude index pages from count, got {}",
        count
    );
}

#[test]
fn test_count_listable_articles_excludes_self() {
    // The folder's own URL (whether folder-style or index.html) is excluded
    let mut folder = make_test_doc("Blog", "blog/index.html");
    folder.kind = PageKind::Folder;

    let leaf = make_test_doc("Post", "blog/post/index.html");

    let all: Vec<&ParsedDocument> = vec![&folder, &leaf];
    let count =
        crate::build::page::page::count_listable_articles(&all, "blog/", "blog/index.html");

    assert_eq!(count, 1, "Should exclude self_url from count");
}

#[test]
fn test_count_listable_articles_works_with_owned_slice() {
    // Verify it works with &[ParsedDocument] (used by shortcode.rs)
    let mut folder = make_test_doc("Blog", "blog/index.html");
    folder.kind = PageKind::Folder;

    let leaf1 = make_test_doc("Post A", "blog/a/index.html");
    let leaf2 = make_test_doc("Post B", "blog/b/index.html");

    let owned: Vec<ParsedDocument> = vec![folder, leaf1, leaf2];
    let count =
        crate::build::page::page::count_listable_articles(&owned, "blog/", "blog/index.html");

    assert_eq!(count, 2, "Should work with &[ParsedDocument] slice");
}

#[test]
fn test_count_listable_articles_empty() {
    let all: Vec<&ParsedDocument> = vec![];
    let count =
        crate::build::page::page::count_listable_articles(&all, "blog/", "blog/index.html");
    assert_eq!(count, 0, "Empty docs should return 0");
}

#[test]
fn test_count_listable_articles_ignores_unrelated_prefix() {
    let leaf_blog = make_test_doc("Post", "blog/post/index.html");
    let leaf_about = make_test_doc("About", "about/index.html");

    let all: Vec<&ParsedDocument> = vec![&leaf_blog, &leaf_about];
    let count =
        crate::build::page::page::count_listable_articles(&all, "blog/", "blog/index.html");
    assert_eq!(count, 1, "Should not count docs outside prefix");
}

#[test]
fn test_count_listable_articles_folder_style_self_url() {
    let leaf1 = make_test_doc("Post", "blog/post/index.html");
    let folder_self = make_test_doc("Blog", "blog/");

    let all: Vec<&ParsedDocument> = vec![&leaf1, &folder_self];
    let count = crate::build::page::page::count_listable_articles(&all, "blog/", "blog/");
    assert_eq!(
        count, 1,
        "Should exclude folder-style self_url and count only leaf"
    );
}

/// A childless folder note is navigation, not an article — it contributes 0.
/// The card must then make no claim at all rather than say "0 articles".
#[test]
fn test_count_listable_articles_childless_folder_note_counts_zero() {
    let mut parent = make_test_doc("Cards", "cards/dust/index.html");
    parent.kind = PageKind::Folder;
    let mut sub = make_test_doc(
        "Simulation Results",
        "cards/dust/simulation-results/index.html",
    );
    sub.kind = PageKind::Folder;

    let all: Vec<&ParsedDocument> = vec![&parent, &sub];
    let count = crate::build::page::page::count_listable_articles(
        &all,
        "cards/dust/",
        "cards/dust/index.html",
    );
    assert_eq!(count, 0, "a sub-folder is not an article");
}
