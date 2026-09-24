use super::*;

/// Helper to create a test ParsedDocument with minimal fields
fn make_doc(url_path: &str, title: &str, weight: Option<i32>, nav: Option<bool>) -> ParsedDocument {
    make_doc_with_root_level(url_path, title, weight, nav, false)
}

/// Helper to create a test ParsedDocument with explicit is_root_level
fn make_doc_with_root_level(
    url_path: &str,
    title: &str,
    weight: Option<i32>,
    nav: Option<bool>,
    is_root_level: bool,
) -> ParsedDocument {
    ParsedDocument {
        url_path: url_path.to_string(),
        title: title.to_string(),
        label: title.to_string(),
        content: format!("{} content", title),
        html_content: format!("<p>{} content</p>", title),
        reading_time: 1,
        slug: title.to_lowercase().replace(' ', "-"),
        permalink: format!("/{}", url_path), // allow:served-path-url-construct (test fixture — permalink field, not HTML-emitted URL)
        weight,
        nav,
        is_root_level,
        kind: moss_core::PageKind::Article,
        ..Default::default()
    }
}

/// Navigation opt-in filtering tests
/// Tests that only documents with nav: Some(true) appear in navigation
#[test]
fn test_navigation_only_shows_nav_true_docs() {
    let documents = vec![
        make_doc("about.html", "About", Some(1), Some(true)),
        make_doc("posts/first-post.html", "First Post", None, None),
        make_doc("posts/second-post.html", "Second Post", None, None),
        make_doc("contact.html", "Contact", Some(2), Some(true)),
        make_doc("index.html", "Home", None, None),
        make_doc("hidden.html", "Hidden Page", None, Some(false)),
    ];

    let nav_builder = NavigationBuilder::new(
        &documents,
        "Test Site",
        None,
        crate::i18n::Language::En,
        false,
    );

    let navigation_html = nav_builder.generate_navigation();

    // Should include pages with nav: Some(true)
    assert!(
        navigation_html.contains("About"),
        "Should include About page (nav: true)"
    );
    assert!(
        navigation_html.contains("Contact"),
        "Should include Contact page (nav: true)"
    );

    // Should exclude pages without nav: Some(true)
    assert!(
        !navigation_html.contains("First Post"),
        "Should exclude posts without nav: true"
    );
    assert!(
        !navigation_html.contains("Second Post"),
        "Should exclude posts without nav: true"
    );
    assert!(
        !navigation_html.contains("Home"),
        "Should exclude index page without nav: true"
    );
    assert!(
        !navigation_html.contains("Hidden Page"),
        "Should exclude page with nav: false"
    );

    // Should have hamburger and nav-links since there are nav items
    assert!(
        navigation_html.contains("mobile-menu-button"),
        "Should have hamburger when nav items exist"
    );
    assert!(
        navigation_html.contains("nav-links"),
        "Should have nav-links when nav items exist"
    );
    // Note: nav-divider removed for cleaner design

    // Hamburger ARIA state (WCAG 4.1.2): aria-expanded starts
    // false (theme.ts keeps it faithful to .mobile-open at runtime), and
    // aria-controls names the id nav-links actually carries.
    assert!(
        navigation_html.contains(r#"aria-expanded="false""#),
        "Hamburger should start with aria-expanded=\"false\""
    );
    assert!(
        navigation_html.contains(r#"aria-controls="nav-links""#),
        "Hamburger should point aria-controls at the nav-links id"
    );
    assert!(
        navigation_html.contains(r#"class="nav-links" id="nav-links""#),
        "nav-links must carry id=\"nav-links\" so aria-controls resolves"
    );
}

#[test]
fn test_navigation_no_nav_items() {
    let documents = vec![
        make_doc("about.html", "About", None, None),
        make_doc("posts/first-post.html", "First Post", None, None),
    ];

    let nav_builder = NavigationBuilder::new(
        &documents,
        "My Blog",
        None,
        crate::i18n::Language::En,
        false,
    );

    let navigation_html = nav_builder.generate_navigation();

    assert!(
        navigation_html.contains("My Blog"),
        "Should include site name"
    );
    assert!(
        navigation_html.contains("nav-theme-btn"),
        "Should include nav theme button"
    );

    // Should NOT have hamburger or nav-links when no nav items
    assert!(
        !navigation_html.contains("mobile-menu-button"),
        "Should NOT have hamburger when no nav items"
    );
    assert!(
        !navigation_html.contains("nav-links"),
        "Should NOT have nav-links when no nav items"
    );
}

#[test]
fn test_navigation_weight_sorting() {
    let documents = vec![
        make_doc("contact.html", "Contact", Some(3), Some(true)),
        make_doc("about.html", "About", Some(1), Some(true)),
        make_doc("services.html", "Services", Some(2), Some(true)),
        make_doc("faq.html", "FAQ", None, Some(true)),
    ];

    let nav_builder = NavigationBuilder::new(
        &documents,
        "Test Site",
        None,
        crate::i18n::Language::En,
        false,
    );

    let navigation_html = nav_builder.generate_navigation();

    // All nav: true docs should appear
    assert!(navigation_html.contains("About"));
    assert!(navigation_html.contains("Services"));
    assert!(navigation_html.contains("Contact"));
    assert!(navigation_html.contains("FAQ"));

    // Weighted items should appear before unweighted (About=1, Services=2, Contact=3, FAQ=none)
    let about_pos = navigation_html.find("About").unwrap();
    let services_pos = navigation_html.find("Services").unwrap();
    let contact_pos = navigation_html.find("Contact").unwrap();
    let faq_pos = navigation_html.find("FAQ").unwrap();
    assert!(
        about_pos < services_pos,
        "About (weight 1) should come before Services (weight 2)"
    );
    assert!(
        services_pos < contact_pos,
        "Services (weight 2) should come before Contact (weight 3)"
    );
    assert!(
        contact_pos < faq_pos,
        "Contact (weight 3) should come before FAQ (no weight)"
    );
}

#[test]
fn test_navigation_home_link_is_root_relative() {
    let documents: Vec<ParsedDocument> = vec![];

    let nav_builder = NavigationBuilder::new(
        &documents,
        "My Site",
        None,
        crate::i18n::Language::En,
        false,
    );

    let nav = nav_builder.generate_navigation();
    assert!(
        nav.contains(r#"href="/""#),
        "Home link should be root-relative /"
    );
}

#[test]
fn test_navigation_site_title() {
    let documents: Vec<ParsedDocument> = vec![];

    let nav_builder = NavigationBuilder::new(
        &documents,
        "Tom & Jerry's Site",
        None,
        crate::i18n::Language::En,
        false,
    );

    let nav = nav_builder.generate_navigation();
    assert!(
        nav.contains("Tom & Jerry's Site"),
        "Should include site title"
    );
    assert!(nav.contains("site-name"), "Should have site-name class");
}

#[test]
fn test_navigation_active_page() {
    let documents = vec![
        make_doc("about.html", "About", Some(1), Some(true)),
        make_doc("contact.html", "Contact", Some(2), Some(true)),
    ];

    let nav_builder = NavigationBuilder::new(
        &documents,
        "Test Site",
        Some("about.html"),
        crate::i18n::Language::En,
        false,
    );

    let navigation_html = nav_builder.generate_navigation();
    assert!(
        navigation_html.contains(r#"class="active""#),
        "Should mark current page as active"
    );
}

#[test]
fn test_navigation_structure() {
    let documents = vec![make_doc("about.html", "About", Some(1), Some(true))];

    let nav_builder = NavigationBuilder::new(
        &documents,
        "My Site",
        None,
        crate::i18n::Language::En,
        false,
    );

    let nav = nav_builder.generate_navigation();

    assert!(nav.contains("nav-left"), "Should have nav-left container");
    assert!(nav.contains("nav-right"), "Should have nav-right container");
    assert!(nav.contains("nav-icons"), "Should have nav-icons container");
    assert!(
        nav.contains("nav-theme-btn"),
        "Should include nav theme button"
    );
}

// ===========================================
// Auto-Navigation Tests (TDD)
// ===========================================
// Rule: Root-level non-index files auto-appear in nav (opt-out model)
// Rule: Nested files require explicit nav: true
// Rule: nav: false opts out any file

#[test]
fn test_auto_nav_root_level_appears() {
    // Root-level file (is_root_level=true, not index) should auto-appear in nav
    // in organized mode (has_content_folders=true)
    let documents = vec![make_doc_with_root_level(
        "about/index.html",
        "About",
        None,
        None,
        true,
    )];

    let nav_builder = NavigationBuilder::new(
        &documents,
        "Test Site",
        None,
        crate::i18n::Language::En,
        true, // organized mode
    );

    let navigation_html = nav_builder.generate_navigation();
    assert!(
        navigation_html.contains("About"),
        "Root-level file should auto-appear in nav"
    );
}

#[test]
fn test_auto_nav_root_index_excluded() {
    // Root index (index.html) should NOT appear in nav even if root-level
    let documents = vec![make_doc_with_root_level(
        "index.html",
        "Home",
        None,
        None,
        true,
    )];

    let nav_builder = NavigationBuilder::new(
        &documents,
        "Test Site",
        None,
        crate::i18n::Language::En,
        false,
    );

    let navigation_html = nav_builder.generate_navigation();
    assert!(
        !navigation_html.contains(">Home<"),
        "Root index should NOT appear in nav"
    );
}

#[test]
fn test_auto_nav_nested_requires_explicit() {
    // Nested file (is_root_level=false) without nav: true should NOT appear
    let documents = vec![make_doc_with_root_level(
        "articles/post/index.html",
        "My Post",
        None,
        None,
        false,
    )];

    let nav_builder = NavigationBuilder::new(
        &documents,
        "Test Site",
        None,
        crate::i18n::Language::En,
        false,
    );

    let navigation_html = nav_builder.generate_navigation();
    assert!(
        !navigation_html.contains("My Post"),
        "Nested file should NOT auto-appear without nav: true"
    );
}

#[test]
fn test_auto_nav_nested_with_explicit_true() {
    // Nested file with explicit nav: true should appear
    let documents = vec![make_doc_with_root_level(
        "articles/index.html",
        "Articles",
        None,
        Some(true),
        false,
    )];

    let nav_builder = NavigationBuilder::new(
        &documents,
        "Test Site",
        None,
        crate::i18n::Language::En,
        false,
    );

    let navigation_html = nav_builder.generate_navigation();
    assert!(
        navigation_html.contains("Articles"),
        "Nested file with nav: true should appear"
    );
}

#[test]
fn test_auto_nav_opt_out_with_nav_false() {
    // Root-level file with nav: false should NOT appear (opt-out)
    let documents = vec![
        make_doc_with_root_level("about/index.html", "About", None, Some(false), true),
        make_doc_with_root_level("contact/index.html", "Contact", None, None, true),
    ];

    let nav_builder = NavigationBuilder::new(
        &documents,
        "Test Site",
        None,
        crate::i18n::Language::En,
        true, // organized mode
    );

    let navigation_html = nav_builder.generate_navigation();
    assert!(
        !navigation_html.contains(">About<"),
        "Root-level with nav: false should be excluded"
    );
    assert!(
        navigation_html.contains("Contact"),
        "Root-level without nav: false should appear"
    );
}

#[test]
fn test_auto_nav_combined_scenario() {
    // Comprehensive test matching a real site's structure (organized mode)
    let documents = vec![
        make_doc_with_root_level("index.html", "Home", None, None, true), // Root index - excluded
        make_doc_with_root_level("research/index.html", "Research", None, None, true), // Root-level - auto
        make_doc_with_root_level("people/index.html", "People", None, None, true), // Root-level - auto
        make_doc_with_root_level("publications/index.html", "Publications", None, None, true), // Root-level - auto
        make_doc_with_root_level("articles/index.html", "Articles", None, Some(true), false), // Nested with nav:true - appears
        make_doc_with_root_level("articles/post1/index.html", "Post 1", None, None, false), // Nested - excluded
    ];

    let nav_builder = NavigationBuilder::new(
        &documents,
        "Test Site",
        None,
        crate::i18n::Language::En,
        true, // organized mode
    );

    let navigation_html = nav_builder.generate_navigation();

    // Should appear: Research, People, Publications (root-level auto), Articles (explicit)
    assert!(
        navigation_html.contains("Research"),
        "Research should appear (root-level auto)"
    );
    assert!(
        navigation_html.contains("People"),
        "People should appear (root-level auto)"
    );
    assert!(
        navigation_html.contains("Publications"),
        "Publications should appear (root-level auto)"
    );
    assert!(
        navigation_html.contains("Articles"),
        "Articles should appear (explicit nav:true)"
    );

    // Should NOT appear: Home (root index), Post 1 (nested without nav:true)
    assert!(
        !navigation_html.contains(">Home<"),
        "Home should NOT appear (root index)"
    );
    assert!(
        !navigation_html.contains("Post 1"),
        "Post 1 should NOT appear (nested, no nav:true)"
    );
}

// ===========================================
// is_navigational_page predicate tests (TDD)
// ===========================================

#[test]
fn is_navigational_page_organized_root_nonindex() {
    assert!(super::is_navigational_page(true, false, "about", true));
}

#[test]
fn is_navigational_page_organized_root_index() {
    // The site home is never a navigational page.
    assert!(!super::is_navigational_page(true, true, "index", true));
}

#[test]
fn is_navigational_page_organized_nested() {
    assert!(!super::is_navigational_page(false, false, "post", true));
}

#[test]
fn is_navigational_page_flat_keyword() {
    assert!(super::is_navigational_page(true, false, "about", false));
}

#[test]
fn is_navigational_page_flat_nonkeyword() {
    assert!(!super::is_navigational_page(true, false, "contact", false));
}

#[test]
fn is_navigational_page_nested_keyword_excluded() {
    // Keyword only counts at the root.
    assert!(!super::is_navigational_page(false, false, "about", false));
}

// ===========================================
// Breadcrumb Navigation Tests
// ===========================================

/// Helper to create a doc with breadcrumb field set
fn make_doc_with_breadcrumb(
    url_path: &str,
    title: &str,
    breadcrumb: Option<bool>,
) -> ParsedDocument {
    let mut doc = make_doc(url_path, title, None, None);
    doc.breadcrumb = breadcrumb;
    doc
}

#[test]
fn test_breadcrumb_renders_when_enabled_on_homepage() {
    // Homepage has breadcrumb: true, nested page should get breadcrumbs
    let documents = vec![
        make_doc_with_breadcrumb("index.html", "Home", Some(true)),
        make_doc_with_breadcrumb("posts/index.html", "Posts", None),
        make_doc_with_breadcrumb("posts/hello/index.html", "Hello World", None),
    ];

    let doc = &documents[2]; // posts/hello/index.html

    let segments = compute_breadcrumb_segments(doc, &documents, "My Site", true);
    assert!(
        segments.is_some(),
        "Breadcrumbs should be computed when homepage has breadcrumb: true"
    );

    let segments = segments.unwrap();
    // Should have: Site Name / Posts / Hello World
    assert_eq!(
        segments.len(),
        3,
        "Should have 3 segments: site, folder, current page"
    );

    // Site name segment
    assert_eq!(segments[0].title, "My Site");
    assert_eq!(segments[0].url, "/");
    assert!(!segments[0].is_current);

    // Posts segment (middle)
    assert_eq!(segments[1].title, "Posts");
    assert_eq!(segments[1].url, "/posts/");
    assert!(!segments[1].is_current);

    // Current page (hello)
    assert_eq!(segments[2].title, "Hello World");
    assert!(segments[2].is_current);

    // Now verify the NavigationBuilder renders it
    let nav_builder = NavigationBuilder::new(
        &documents,
        "My Site",
        Some(doc.url_path.as_str()),
        crate::i18n::Language::En,
        false,
    )
    .with_breadcrumb(segments);

    let nav_html = nav_builder.generate_navigation();
    assert!(
        nav_html.contains("breadcrumb-separator"),
        "Should render breadcrumb separators"
    );
    assert!(
        !nav_html.contains("breadcrumb-current"),
        "Current page should not appear in breadcrumb"
    );
    assert!(
        nav_html.contains("My Site"),
        "Should include site name in breadcrumb"
    );
    assert!(
        !nav_html.contains("Hello World"),
        "Current page title should not appear in breadcrumb"
    );
}

#[test]
fn editor_preview_breadcrumb_and_logo_name_their_fields() {
    // In the editor preview the breadcrumb trail names `breadcrumb:` (the
    // field that toggles it on this page) and the logo names `logo:` — the
    // latter only when the flag says this page owns the field. Default
    // builders (publish, synthetic pages) emit neither.
    let documents = vec![
        make_doc_with_breadcrumb("index.html", "Home", Some(true)),
        make_doc_with_breadcrumb("posts/index.html", "Posts", None),
        make_doc_with_breadcrumb("posts/hello/index.html", "Hello World", None),
    ];
    let doc = &documents[2];

    let build = |fm: bool, logo_fm: bool| {
        let segments = compute_breadcrumb_segments(doc, &documents, "My Site", true)
            .expect("breadcrumbs enabled by homepage");
        NavigationBuilder::new(
            &documents,
            "My Site",
            Some(doc.url_path.as_str()),
            crate::i18n::Language::En,
            false,
        )
        .with_logo("/assets/logo.svg".to_string())
        .with_breadcrumb(segments)
        .with_source_fm(fm, logo_fm)
        .generate_navigation()
    };

    let on = build(true, true);
    assert!(
        on.contains(r#"<div class="nav-left" data-source-fm="breadcrumb">"#),
        "trail must name `breadcrumb`. Got: {on}"
    );
    assert!(
        on.contains(r#"class="site-logo" data-source-fm="logo""#),
        "logo must name `logo`. Got: {on}"
    );

    // Logo flag off (any page that doesn't own the field): trail still
    // annotated, logo bare.
    let no_logo = build(true, false);
    assert!(no_logo.contains(r#"data-source-fm="breadcrumb""#), "Got: {no_logo}");
    assert!(!no_logo.contains(r#"data-source-fm="logo""#), "Got: {no_logo}");

    // Publish shape: nothing.
    let off = build(false, false);
    assert!(!off.contains("data-source-fm"), "Got: {off}");
}

#[test]
fn test_breadcrumb_not_rendered_on_root_page() {
    // Root pages (depth 0) should not get breadcrumbs even with breadcrumb: true on homepage
    let documents = vec![
        make_doc_with_breadcrumb("index.html", "Home", Some(true)),
        make_doc_with_breadcrumb("about.html", "About", None),
    ];

    let doc = &documents[0]; // index.html (homepage)

    let segments = compute_breadcrumb_segments(doc, &documents, "My Site", true);
    assert!(
        segments.is_none(),
        "Root page (depth 0) should NOT get breadcrumbs"
    );

    // Also test non-index root page
    let doc = &documents[1]; // about.html
    let segments = compute_breadcrumb_segments(doc, &documents, "My Site", true);
    assert!(
        segments.is_none(),
        "Root-level page (depth 0) should NOT get breadcrumbs"
    );
}

#[test]
fn test_breadcrumb_per_page_override_false() {
    // Homepage has breadcrumb: true, but specific page has breadcrumb: false
    let documents = vec![
        make_doc_with_breadcrumb("index.html", "Home", Some(true)),
        make_doc_with_breadcrumb("posts/index.html", "Posts", None),
        make_doc_with_breadcrumb("posts/secret/index.html", "Secret Post", Some(false)),
    ];

    let doc = &documents[2]; // posts/secret/index.html

    let segments = compute_breadcrumb_segments(doc, &documents, "My Site", true);
    assert!(
        segments.is_none(),
        "Page with breadcrumb: false should not get breadcrumbs"
    );
}

#[test]
fn test_breadcrumb_not_rendered_when_explicitly_disabled() {
    // Homepage has breadcrumb: false — no breadcrumbs even without nav items
    let documents = vec![
        make_doc_with_breadcrumb("index.html", "Home", Some(false)),
        make_doc_with_breadcrumb("posts/hello/index.html", "Hello World", None),
    ];

    let doc = &documents[1];

    let segments = compute_breadcrumb_segments(doc, &documents, "My Site", true);
    assert!(
        segments.is_none(),
        "No breadcrumbs when homepage has breadcrumb: false"
    );
}

#[test]
fn test_breadcrumb_auto_enable_flat_mode_with_keyword() {
    // Flat mode with "about" keyword file: nav items exist → no auto-breadcrumbs
    let mut about = make_doc_with_breadcrumb("about/index.html", "About", None);
    about.is_root_level = true;
    about.clean_stem = "about".to_string();
    let documents = vec![
        make_doc_with_breadcrumb("index.html", "Home", None),
        about,
        make_doc_with_breadcrumb("posts/hello/index.html", "Hello World", None),
    ];
    let doc = &documents[2];
    let segments = compute_breadcrumb_segments(doc, &documents, "My Site", false);
    assert!(
        segments.is_none(),
        "Flat mode with 'about' → nav item exists → no auto-breadcrumbs"
    );
}

#[test]
fn test_breadcrumb_auto_enable_flat_mode_no_keyword() {
    // Flat mode with no keyword files: no nav items → auto-enable breadcrumbs
    let mut post = make_doc_with_breadcrumb("my-post/index.html", "My Post", None);
    post.is_root_level = true;
    post.clean_stem = "my-post".to_string();
    let documents = vec![
        make_doc_with_breadcrumb("index.html", "Home", None),
        post,
        make_doc_with_breadcrumb("writing/article/index.html", "Article", None),
    ];
    let doc = &documents[2];
    let segments = compute_breadcrumb_segments(doc, &documents, "My Site", false);
    assert!(
        segments.is_some(),
        "Flat mode with no keyword files → no nav items → auto-breadcrumbs"
    );
}

#[test]
fn test_breadcrumb_segment_titles_from_label_vs_titlecase_fallback() {
    // When a folder's index page exists, use its label
    // When it doesn't, fall back to titlecased path segment
    let documents = vec![
        make_doc_with_breadcrumb("index.html", "Home", Some(true)),
        make_doc_with_breadcrumb("blog-posts/index.html", "My Blog Posts", None),
        // "blog-posts/tech-tips" has no index.html — should titlecase to "Tech Tips"
        make_doc_with_breadcrumb(
            "blog-posts/tech-tips/my-article/index.html",
            "My Article",
            None,
        ),
    ];

    let doc = &documents[2]; // blog-posts/tech-tips/my-article/index.html

    let segments = compute_breadcrumb_segments(doc, &documents, "My Site", true);
    assert!(segments.is_some());

    let segments = segments.unwrap();
    // Should have: My Site / My Blog Posts / Tech Tips / My Article
    assert_eq!(segments.len(), 4, "Should have 4 segments");
    assert_eq!(segments[0].title, "My Site");
    assert_eq!(
        segments[1].title, "My Blog Posts",
        "Should use label from existing doc"
    );
    assert_eq!(
        segments[2].title, "Tech Tips",
        "Should titlecase when no matching doc exists"
    );
    assert_eq!(segments[3].title, "My Article");
    assert!(segments[3].is_current);
}

#[test]
fn test_breadcrumb_current_page_is_absent() {
    // Current page should be completely absent from breadcrumb (page identity comes from h1)
    let documents = vec![
        make_doc_with_breadcrumb("index.html", "Home", Some(true)),
        make_doc_with_breadcrumb("posts/hello/index.html", "Hello World", None),
    ];

    let doc = &documents[1];

    let segments = compute_breadcrumb_segments(doc, &documents, "My Site", true).unwrap();

    let nav_builder = NavigationBuilder::new(
        &documents,
        "My Site",
        Some(doc.url_path.as_str()),
        crate::i18n::Language::En,
        false,
    )
    .with_breadcrumb(segments);

    let nav_html = nav_builder.generate_navigation();

    // Current page should not appear at all in the breadcrumb
    assert!(
        !nav_html.contains("Hello World"),
        "Current page should not appear in breadcrumb"
    );
    assert!(
        !nav_html.contains("breadcrumb-current"),
        "breadcrumb-current class should not exist"
    );
    // Ancestors should still be present
    assert!(nav_html.contains("My Site"), "Site name should be present");
    assert!(
        nav_html.contains("breadcrumb-segment"),
        "Ancestor segments should be present"
    );
}

#[test]
fn deep_masthead_trail_ships_fold_controls_shallow_does_not() {
    // The masthead folds like the island (as of 2026-08-09):
    // with three or more crumbs there is a middle to sacrifice, so nav.rs
    // ships the `…` button and the levels panel — hidden, because whether
    // anything actually folds is a measurement only masthead-fold.ts can
    // make. With fewer, the controls could never unhide, so they are not
    // emitted at all.
    let documents = vec![
        make_doc_with_breadcrumb("index.html", "Home", Some(true)),
        make_doc_with_breadcrumb("posts/index.html", "Posts", None),
        make_doc_with_breadcrumb("posts/2024/index.html", "2024", None),
        make_doc_with_breadcrumb("posts/2024/hello/index.html", "Hello World", None),
    ];

    let deep = &documents[3];
    let segments = compute_breadcrumb_segments(deep, &documents, "My Site", true).unwrap();
    let deep_html = NavigationBuilder::new(
        &documents,
        "My Site",
        Some(deep.url_path.as_str()),
        crate::i18n::Language::En,
        false,
    )
    .with_breadcrumb(segments)
    .generate_navigation();

    assert!(
        deep_html.contains(r#"<button type="button" class="moss-breadcrumb-more" hidden"#),
        "a 3-crumb trail (site name / Posts / 2024) has a middle, so the fold \
         controls must ship: {deep_html}"
    );
    assert!(
        deep_html.contains(r#"<div class="moss-breadcrumb-menu" hidden></div>"#),
        "the levels panel ships (hidden, empty) beside the deep trail: {deep_html}"
    );
    assert!(
        deep_html.contains("data-trail-crumb"),
        "every crumb carries the fold script's positional handle: {deep_html}"
    );

    let shallow = &documents[1];
    let segments = compute_breadcrumb_segments(shallow, &documents, "My Site", true).unwrap();
    let shallow_html = NavigationBuilder::new(
        &documents,
        "My Site",
        Some(shallow.url_path.as_str()),
        crate::i18n::Language::En,
        false,
    )
    .with_breadcrumb(segments)
    .generate_navigation();

    assert!(
        !shallow_html.contains("moss-breadcrumb-more")
            && !shallow_html.contains("moss-breadcrumb-menu"),
        "a trail with no middle gets no fold controls: {shallow_html}"
    );
}

#[test]
fn test_breadcrumb_segment_carries_hint_label_and_label_span() {
    // A truncatable segment ships two things:
    //   - `data-hint-label` with the UNtruncated title. NOT `data-tooltip`:
    //     site.css renders that one on hover unconditionally, which showed a
    //     hint even when the label was fully on screen. breadcrumb-hint.ts
    //     promotes this attribute to `data-tooltip` only while the label
    //     measures as truncated.
    //   - a `.breadcrumb-label` span, which owns `overflow: hidden` so the
    //     `<a>` can stay unclipped. Collapse them onto the `<a>` and its own
    //     overflow eats the hint pseudo-element — and the span is also what
    //     the script measures.
    // The home segment is `flex-shrink: 0` and never truncates, so it gets
    // neither — a hint there would just repeat what is already on screen.
    let documents = vec![
        make_doc_with_breadcrumb("index.html", "Home", Some(true)),
        make_doc_with_breadcrumb("posts/index.html", "A Very Long Section Name", None),
        make_doc_with_breadcrumb("posts/hello/index.html", "Hello World", None),
    ];

    let doc = &documents[2];
    let segments = compute_breadcrumb_segments(doc, &documents, "My Site", true).unwrap();

    let nav_html = NavigationBuilder::new(
        &documents,
        "My Site",
        Some(doc.url_path.as_str()),
        crate::i18n::Language::En,
        false,
    )
    .with_breadcrumb(segments)
    .generate_navigation();

    assert!(
        nav_html.contains(
            r#"class="breadcrumb-segment" data-trail-crumb data-hint-label="A Very Long Section Name"><span class="breadcrumb-label">A Very Long Section Name</span>"#
        ),
        "ancestor segment should carry both the hint label and the label span, got: {}",
        nav_html
    );
    assert!(
        !nav_html.contains(r#"class="breadcrumb-segment" data-tooltip"#),
        "no breadcrumb may ship a ready-to-render hint — that is the always-on \
         bug; only breadcrumb-hint.ts adds data-tooltip, and only when the \
         label is measured as truncated. Got: {}",
        nav_html
    );
    assert!(
        !nav_html.contains(r#"class="site-name" data-hint-label"#),
        "home segment should not carry a hint, got: {}",
        nav_html
    );
}

#[test]
fn test_breadcrumb_hint_escapes_quotes_in_title() {
    // The hint lands in an HTML attribute, so a title containing a quote must
    // not be able to close it and inject markup.
    let documents = vec![
        make_doc_with_breadcrumb("index.html", "Home", Some(true)),
        make_doc_with_breadcrumb("posts/index.html", r#"He said "hi" & left"#, None),
        make_doc_with_breadcrumb("posts/hello/index.html", "Hello World", None),
    ];

    let doc = &documents[2];
    let segments = compute_breadcrumb_segments(doc, &documents, "My Site", true).unwrap();

    let nav_html = NavigationBuilder::new(
        &documents,
        "My Site",
        Some(doc.url_path.as_str()),
        crate::i18n::Language::En,
        false,
    )
    .with_breadcrumb(segments)
    .generate_navigation();

    assert!(
        !nav_html.contains(r#"data-hint-label="He said "hi""#),
        "quotes in a title must be escaped in the hint attribute, got: {}",
        nav_html
    );
    assert!(
        nav_html.contains("&quot;hi&quot;"),
        "expected escaped quotes in the hint attribute, got: {}",
        nav_html
    );
}

#[test]
fn test_breadcrumb_single_depth_page() {
    // A page at depth 1 (e.g. "about/index.html") — after filtering out is_current,
    // only site-name remains. Result: just the site name link, no separator.
    let documents = vec![
        make_doc_with_breadcrumb("index.html", "Home", Some(true)),
        make_doc_with_breadcrumb("about/index.html", "About Us", None),
    ];

    let doc = &documents[1]; // about/index.html

    let segments = compute_breadcrumb_segments(doc, &documents, "My Site", true).unwrap();

    // Raw segments still have 2 entries (site + current)
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].title, "My Site");
    assert_eq!(segments[1].title, "About Us");
    assert!(segments[1].is_current);

    // But rendered HTML should skip the current page
    let nav_builder = NavigationBuilder::new(
        &documents,
        "My Site",
        Some(doc.url_path.as_str()),
        crate::i18n::Language::En,
        false,
    )
    .with_breadcrumb(segments);

    let nav_html = nav_builder.generate_navigation();

    // Should have site name but no separator (only 1 part after filtering)
    assert!(nav_html.contains("My Site"), "Should include site name");
    assert!(
        !nav_html.contains("breadcrumb-separator"),
        "No separator when only site name remains"
    );
    assert!(
        !nav_html.contains("About Us"),
        "Current page should not appear"
    );
}

#[test]
fn test_titlecase_segment_function() {
    assert_eq!(titlecase_segment("hello-world"), "Hello World");
    assert_eq!(titlecase_segment("tech-tips"), "Tech Tips");
    assert_eq!(titlecase_segment("simple"), "Simple");
    assert_eq!(titlecase_segment("a-b-c"), "A B C");
    assert_eq!(titlecase_segment(""), "");
}

#[test]
fn test_breadcrumb_empty_segments_fallback() {
    // When with_breadcrumb is called with empty segments, fall back to plain site name
    let documents: Vec<ParsedDocument> = vec![];

    let nav_builder = NavigationBuilder::new(
        &documents,
        "My Site",
        None,
        crate::i18n::Language::En,
        false,
    )
    .with_breadcrumb(vec![]);

    let nav_html = nav_builder.generate_navigation();
    // Should fall back to normal site name rendering
    assert!(
        nav_html.contains(r#"class="site-name""#),
        "Empty segments should fall back to site-name"
    );
    assert!(nav_html.contains("My Site"), "Should still show site name");
    assert!(
        !nav_html.contains("breadcrumb-separator"),
        "Should NOT have breadcrumb separators"
    );
}

// ===========================================
// Translation Root Transparency Tests
// ===========================================

/// Helper to create a doc with translations (for translation root tests)
fn make_doc_with_translations(
    url_path: &str,
    title: &str,
    breadcrumb: Option<bool>,
    translations: Vec<TranslationLink>,
) -> ParsedDocument {
    let mut doc = make_doc_with_breadcrumb(url_path, title, breadcrumb);
    doc.translations = translations;
    doc
}

#[test]
fn test_breadcrumb_translation_root_transparent() {
    // zh-hans/ is a translation root (its index is a translation of the homepage).
    // It should NOT appear as a breadcrumb segment.
    let documents = vec![
        make_doc_with_translations(
            "index.html",
            "moss",
            Some(true),
            vec![TranslationLink {
                lang_tag: Language::ZhHans.as_bcp47_attr().to_string(),
                url_path: "zh-hans/index.html".to_string(),
                display_name: "简",
            }],
        ),
        make_doc_with_translations(
            "zh-hans/index.html",
            "青苔",
            None,
            vec![TranslationLink {
                lang_tag: Language::En.as_bcp47_attr().to_string(),
                url_path: "index.html".to_string(),
                display_name: "EN",
            }],
        ),
        make_doc_with_breadcrumb("zh-hans/docs/index.html", "文档", None),
        make_doc_with_breadcrumb("zh-hans/docs/getting-started/index.html", "快速入门", None),
    ];

    let doc = &documents[3]; // zh-hans/docs/getting-started/index.html

    let segments = compute_breadcrumb_segments(doc, &documents, "青苔", true);
    assert!(
        segments.is_some(),
        "Should compute breadcrumbs for translated page"
    );

    let segments = segments.unwrap();
    // Should have: 青苔 -> /zh-hans/ , 文档 -> current
    // NOT: 青苔 -> / , 青苔 -> /zh-hans/ , 文档 -> current
    assert_eq!(
        segments.len(),
        3,
        "Should have 3 segments: home, docs, current. Got: {:?}",
        segments
            .iter()
            .map(|s| (&s.title, &s.url, s.is_current))
            .collect::<Vec<_>>()
    );

    assert_eq!(segments[0].title, "青苔");
    assert_eq!(segments[0].url, "/zh-hans/");
    assert!(!segments[0].is_current);

    assert_eq!(segments[1].title, "文档");
    assert_eq!(segments[1].url, "/zh-hans/docs/");
    assert!(!segments[1].is_current);

    assert_eq!(segments[2].title, "快速入门");
    assert!(segments[2].is_current);
}

#[test]
fn test_breadcrumb_translation_root_homepage_no_breadcrumbs() {
    // The translation root homepage (zh-hans/index.html) should get NO breadcrumbs,
    // just like the root homepage (index.html).
    let documents = vec![
        make_doc_with_translations(
            "index.html",
            "moss",
            Some(true),
            vec![TranslationLink {
                lang_tag: Language::ZhHans.as_bcp47_attr().to_string(),
                url_path: "zh-hans/index.html".to_string(),
                display_name: "简",
            }],
        ),
        make_doc_with_translations(
            "zh-hans/index.html",
            "青苔",
            None,
            vec![TranslationLink {
                lang_tag: Language::En.as_bcp47_attr().to_string(),
                url_path: "index.html".to_string(),
                display_name: "EN",
            }],
        ),
    ];

    let doc = &documents[1]; // zh-hans/index.html

    let segments = compute_breadcrumb_segments(doc, &documents, "青苔", true);
    assert!(
        segments.is_none(),
        "Translation root homepage should NOT get breadcrumbs"
    );
}

#[test]
fn test_breadcrumb_translation_root_deep_nesting() {
    // Deep nesting under a translation root should produce correct URLs
    let documents = vec![
        make_doc_with_translations(
            "index.html",
            "moss",
            Some(true),
            vec![TranslationLink {
                lang_tag: Language::ZhHans.as_bcp47_attr().to_string(),
                url_path: "zh-hans/index.html".to_string(),
                display_name: "简",
            }],
        ),
        make_doc_with_translations(
            "zh-hans/index.html",
            "青苔",
            None,
            vec![TranslationLink {
                lang_tag: Language::En.as_bcp47_attr().to_string(),
                url_path: "index.html".to_string(),
                display_name: "EN",
            }],
        ),
        make_doc_with_breadcrumb("zh-hans/docs/index.html", "文档", None),
        make_doc_with_breadcrumb("zh-hans/docs/guides/index.html", "指南", None),
        make_doc_with_breadcrumb(
            "zh-hans/docs/guides/quickstart/index.html",
            "快速开始",
            None,
        ),
    ];

    let doc = &documents[4]; // zh-hans/docs/guides/quickstart/index.html

    let segments = compute_breadcrumb_segments(doc, &documents, "青苔", true).unwrap();
    assert_eq!(
        segments.len(),
        4,
        "Should have 4 segments: home, docs, guides, current"
    );

    assert_eq!(segments[0].url, "/zh-hans/");
    assert_eq!(segments[1].title, "文档");
    assert_eq!(segments[1].url, "/zh-hans/docs/");
    assert_eq!(segments[2].title, "指南");
    assert_eq!(segments[2].url, "/zh-hans/docs/guides/");
    assert_eq!(segments[3].title, "快速开始");
    assert!(segments[3].is_current);
}

#[test]
fn test_breadcrumb_translation_root_non_index_file() {
    // A non-index file under a translation root
    let documents = vec![
        make_doc_with_translations(
            "index.html",
            "moss",
            Some(true),
            vec![TranslationLink {
                lang_tag: Language::ZhHans.as_bcp47_attr().to_string(),
                url_path: "zh-hans/index.html".to_string(),
                display_name: "简",
            }],
        ),
        make_doc_with_translations(
            "zh-hans/index.html",
            "青苔",
            None,
            vec![TranslationLink {
                lang_tag: Language::En.as_bcp47_attr().to_string(),
                url_path: "index.html".to_string(),
                display_name: "EN",
            }],
        ),
        make_doc_with_breadcrumb("zh-hans/about/index.html", "关于", None),
        make_doc_with_breadcrumb("zh-hans/about/page.html", "某页面", None),
    ];

    let doc = &documents[3]; // zh-hans/about/page.html

    let segments = compute_breadcrumb_segments(doc, &documents, "青苔", true).unwrap();
    // home -> about (folder) -> page (current)
    assert_eq!(segments.len(), 3);
    assert_eq!(segments[0].url, "/zh-hans/");
    assert_eq!(segments[1].title, "关于");
    assert_eq!(segments[1].url, "/zh-hans/about/");
    assert!(!segments[1].is_current);
    assert_eq!(segments[2].title, "某页面");
    assert!(segments[2].is_current);
}

#[test]
fn test_breadcrumb_translation_root_partial_translation() {
    // Missing middle index (zh-hans/docs/index.html doesn't exist)
    // Should fall back to titlecase for that segment
    let documents = vec![
        make_doc_with_translations(
            "index.html",
            "moss",
            Some(true),
            vec![TranslationLink {
                lang_tag: Language::ZhHans.as_bcp47_attr().to_string(),
                url_path: "zh-hans/index.html".to_string(),
                display_name: "简",
            }],
        ),
        make_doc_with_translations(
            "zh-hans/index.html",
            "青苔",
            None,
            vec![TranslationLink {
                lang_tag: Language::En.as_bcp47_attr().to_string(),
                url_path: "index.html".to_string(),
                display_name: "EN",
            }],
        ),
        // No zh-hans/docs/index.html — should titlecase "docs" -> "Docs"
        make_doc_with_breadcrumb("zh-hans/docs/getting-started/index.html", "快速入门", None),
    ];

    let doc = &documents[2];

    let segments = compute_breadcrumb_segments(doc, &documents, "青苔", true).unwrap();
    assert_eq!(segments.len(), 3);
    assert_eq!(segments[0].url, "/zh-hans/");
    assert_eq!(
        segments[1].title, "Docs",
        "Should titlecase when no index doc exists"
    );
    assert_eq!(segments[1].url, "/zh-hans/docs/");
    assert_eq!(segments[2].title, "快速入门");
    assert!(segments[2].is_current);
}

#[test]
fn test_breadcrumb_arbitrary_folder_name_as_translation_root() {
    // A folder named "chinese" (not a language code) linked via translationKey
    let documents = vec![
        make_doc_with_translations(
            "index.html",
            "moss",
            Some(true),
            vec![TranslationLink {
                lang_tag: Language::ZhHans.as_bcp47_attr().to_string(),
                url_path: "chinese/index.html".to_string(),
                display_name: "简",
            }],
        ),
        make_doc_with_translations(
            "chinese/index.html",
            "青苔",
            None,
            vec![TranslationLink {
                lang_tag: Language::En.as_bcp47_attr().to_string(),
                url_path: "index.html".to_string(),
                display_name: "EN",
            }],
        ),
        make_doc_with_breadcrumb("chinese/docs/index.html", "文档", None),
    ];

    let doc = &documents[2]; // chinese/docs/index.html

    let segments = compute_breadcrumb_segments(doc, &documents, "青苔", true).unwrap();
    // "chinese" should be transparent even though it's not a language code
    assert_eq!(segments.len(), 2, "Should have 2 segments: home, current");
    assert_eq!(segments[0].title, "青苔");
    assert_eq!(segments[0].url, "/chinese/");
    assert_eq!(segments[1].title, "文档");
    assert!(segments[1].is_current);
}

#[test]
fn test_breadcrumb_translation_root_direct_child_no_breadcrumbs() {
    // A non-index file directly under the translation root (effective depth 0)
    // should get no breadcrumbs, same as a root-level page.
    let documents = vec![
        make_doc_with_translations(
            "index.html",
            "moss",
            Some(true),
            vec![TranslationLink {
                lang_tag: Language::ZhHans.as_bcp47_attr().to_string(),
                url_path: "zh-hans/index.html".to_string(),
                display_name: "简",
            }],
        ),
        make_doc_with_translations(
            "zh-hans/index.html",
            "青苔",
            None,
            vec![TranslationLink {
                lang_tag: Language::En.as_bcp47_attr().to_string(),
                url_path: "index.html".to_string(),
                display_name: "EN",
            }],
        ),
        make_doc_with_breadcrumb("zh-hans/page.html", "某页面", None),
    ];

    let doc = &documents[2]; // zh-hans/page.html (effective depth 0 after stripping)

    let segments = compute_breadcrumb_segments(doc, &documents, "青苔", true);
    assert!(segments.is_none(),
            "Non-index file directly under translation root (effective depth 0) should get no breadcrumbs");
}

#[test]
fn test_breadcrumb_non_translation_folder_unchanged() {
    // docs/ is NOT a translation root — should behave normally
    let documents = vec![
        make_doc_with_breadcrumb("index.html", "Home", Some(true)),
        make_doc_with_breadcrumb("docs/index.html", "Docs", None),
        make_doc_with_breadcrumb("docs/getting-started/index.html", "Getting Started", None),
    ];

    let doc = &documents[2];

    let segments = compute_breadcrumb_segments(doc, &documents, "My Site", true).unwrap();
    // Normal breadcrumb: site -> docs -> current
    assert_eq!(segments.len(), 3);
    assert_eq!(segments[0].title, "My Site");
    assert_eq!(segments[0].url, "/");
    assert_eq!(segments[1].title, "Docs");
    assert_eq!(segments[1].url, "/docs/");
    assert_eq!(segments[2].title, "Getting Started");
    assert!(segments[2].is_current);
}

#[test]
fn test_breadcrumb_folder_with_no_index_not_transparent() {
    // foo/ has no index.html — can't be a translation root
    let documents = vec![
        make_doc_with_breadcrumb("index.html", "Home", Some(true)),
        make_doc_with_breadcrumb("foo/page/index.html", "Some Page", None),
    ];

    let doc = &documents[1];

    let segments = compute_breadcrumb_segments(doc, &documents, "My Site", true).unwrap();
    // Normal: site -> foo (titlecased) -> current
    assert_eq!(segments.len(), 3);
    assert_eq!(segments[0].url, "/");
    assert_eq!(segments[1].title, "Foo");
    assert_eq!(segments[1].url, "/foo/");
    assert_eq!(segments[2].title, "Some Page");
    assert!(segments[2].is_current);
}

// ===========================================
// Pretty URL Tests
// ===========================================

#[test]
fn test_nav_links_use_root_relative_pretty_urls() {
    // Nav links should use root-relative pretty URLs
    let documents = vec![
        make_doc_with_root_level("about/index.html", "About", Some(1), Some(true), true),
        make_doc_with_root_level("contact/index.html", "Contact", Some(2), Some(true), true),
    ];

    let nav_builder = NavigationBuilder::new(
        &documents,
        "Test Site",
        None,
        crate::i18n::Language::En,
        false,
    );

    let nav_html = nav_builder.generate_navigation();

    // Should use "/about/" not "about/index.html"
    assert!(
        nav_html.contains(r#"href="/about/""#),
        "Nav should use root-relative pretty URL '/about/'. Got: {}",
        nav_html
    );
    assert!(
        !nav_html.contains(r#"href="about/index.html""#),
        "Nav should NOT contain index.html in href"
    );
    assert!(
        nav_html.contains(r#"href="/contact/""#),
        "Nav should use root-relative pretty URL '/contact/'"
    );
}

#[test]
fn the_switcher_advertises_the_declared_tag_not_the_ui_variant() {
    // A `fr` page resolves to `Language::En` for the interface. The switcher
    // used to label its link `hreflang="en"` — from `code()`, whose own doc
    // says to use the BCP-47 form here — contradicting the `<html lang>` of the
    // page it points at.
    let documents = vec![make_doc("index.html", "Home", None, None)];
    let translations = vec![TranslationLink {
        lang_tag: "fr".to_string(),
        url_path: "fr/index.html".to_string(),
        display_name: "FR",
    }];

    let nav_html = NavigationBuilder::new(&documents, "Test Site", None, crate::i18n::Language::En, false)
        .with_translations(crate::i18n::Language::ZhHant, "zh-Hant", translations)
        .generate_navigation();

    assert!(nav_html.contains(r#"hreflang="fr""#), "{nav_html}");
    assert!(!nav_html.contains(r#"hreflang="en""#), "{nav_html}");
}

#[test]
fn test_navigation_with_translations_shows_lang_toggle() {
    let documents = vec![make_doc("index.html", "Home", None, None)];

    let translations = vec![TranslationLink {
        lang_tag: crate::i18n::Language::ZhHans.as_bcp47_attr().to_string(),
        url_path: "index.zh-hans.html".to_string(),
        display_name: "简",
    }];

    let nav_builder = NavigationBuilder::new(
        &documents,
        "Test Site",
        None,
        crate::i18n::Language::En,
        false,
    )
    .with_translations(crate::i18n::Language::En, "en", translations);

    let nav_html = nav_builder.generate_navigation();

    assert!(
        nav_html.contains("nav-lang-toggle"),
        "Should have language toggle"
    );
    assert!(
        nav_html.contains("nav-lang-current"),
        "Should have current language indicator"
    );
    assert!(
        nav_html.contains("EN"),
        "Should show current language display name"
    );
    assert!(
        nav_html.contains("nav-lang-link"),
        "Should have language link"
    );
    assert!(
        nav_html.contains("简"),
        "Should show Chinese translation link"
    );
}

#[test]
fn test_navigation_no_translations_hides_lang_toggle() {
    let documents = vec![make_doc("index.html", "Home", None, None)];

    let nav_builder = NavigationBuilder::new(
        &documents,
        "Test Site",
        None,
        crate::i18n::Language::En,
        false,
    );

    let nav_html = nav_builder.generate_navigation();

    assert!(
        !nav_html.contains("nav-lang-toggle"),
        "Should NOT have language toggle without translations"
    );
}

#[test]
fn test_navigation_lang_toggle_links_to_translation() {
    let documents = vec![make_doc("about.html", "About", Some(1), Some(true))];

    let translations = vec![TranslationLink {
        lang_tag: crate::i18n::Language::ZhHans.as_bcp47_attr().to_string(),
        url_path: "zh-hans/about/index.html".to_string(),
        display_name: "简",
    }];

    let nav_builder = NavigationBuilder::new(
        &documents,
        "Test Site",
        Some("about.html"),
        crate::i18n::Language::En,
        false,
    )
    .with_translations(crate::i18n::Language::En, "en", translations);

    let nav_html = nav_builder.generate_navigation();

    assert!(
        nav_html.contains(r#"href="/zh-hans/about/""#),
        "Lang toggle should link to root-relative lang-prefix URL. Got: {}",
        nav_html
    );
}

#[test]
fn test_navigation_lang_toggle_multiple_translations() {
    let documents: Vec<ParsedDocument> = vec![];

    let translations = vec![
        TranslationLink {
            lang_tag: crate::i18n::Language::ZhHans.as_bcp47_attr().to_string(),
            url_path: "zh-hans/about/index.html".to_string(),
            display_name: "简",
        },
        TranslationLink {
            lang_tag: crate::i18n::Language::ZhHant.as_bcp47_attr().to_string(),
            url_path: "zh-hant/about/index.html".to_string(),
            display_name: "繁",
        },
    ];

    let nav_builder = NavigationBuilder::new(
        &documents,
        "Test Site",
        None,
        crate::i18n::Language::En,
        false,
    )
    .with_translations(crate::i18n::Language::En, "en", translations);

    let nav_html = nav_builder.generate_navigation();

    assert!(nav_html.contains("EN"), "Should show English as current");
    assert!(
        nav_html.contains("简"),
        "Should show Simplified Chinese link"
    );
    assert!(
        nav_html.contains("繁"),
        "Should show Traditional Chinese link"
    );
    assert!(
        nav_html.contains(" / "),
        "Should have separators between languages"
    );
}

// --- Language scoping of nav items (Task 2.2) ---

/// A page whose CONTENT is detected as a non-site language but which does NOT
/// live under a language tree (e.g. an English-titled translator page at
/// `awards/translation/s2/david-yang/` on a zh-hant site with no `en/` tree)
/// must keep `/` as the site-name home link — `/en/` does not exist and 404s —
/// and must still show the SITE language's nav items rather than an empty nav.
/// Regression: harborweekly port, 2026-07-28.
#[test]
fn detected_lang_without_language_tree_keeps_root_home_and_site_nav() {
    use crate::i18n::Language;
    let mut awards =
        make_doc_with_root_level("awards/index.html", "獎項", None, Some(true), false);
    awards.lang = Language::ZhHant;

    let documents = vec![awards];
    // Site language zh-hant; current page detected En, url NOT under a lang tree.
    let nav_html = NavigationBuilder::new(
        &documents,
        "潮汐 · 週報",
        Some("awards/translation/s2/david-yang/index.html"),
        Language::ZhHant,
        true,
    )
    .with_translations(Language::En, "en", Vec::new())
    .generate_navigation();

    assert!(
        nav_html.contains(r#"<a href="/" class="site-name">"#),
        "home link must stay '/' when no language tree exists for the detected lang. Got: {}",
        nav_html
    );
    assert!(
        !nav_html.contains(r#"href="/en/""#),
        "must not link a nonexistent /en/ tree. Got: {}",
        nav_html
    );
    assert!(
        nav_html.contains(">獎項<"),
        "site-language nav items must still show. Got: {}",
        nav_html
    );
}

/// Control: a page genuinely living under an `en/` language tree keeps the
/// language-prefixed home link.
#[test]
fn page_inside_language_tree_keeps_lang_prefixed_home() {
    use crate::i18n::Language;
    let mut en_about =
        make_doc_with_root_level("en/about/index.html", "About", None, Some(true), false);
    en_about.lang = Language::En;

    let documents = vec![en_about];
    let nav_html = NavigationBuilder::new(
        &documents,
        "Site",
        Some("en/about/index.html"),
        Language::ZhHant,
        true,
    )
    .with_translations(Language::En, "en", Vec::new())
    .generate_navigation();

    assert!(
        nav_html.contains(r#"<a href="/en/" class="site-name">"#),
        "a page under en/ keeps the /en/ home link. Got: {}",
        nav_html
    );
}

/// The other symptom of the same regression: `en-us/` is a language tree `lang_tree_prefix`
/// accepts and `Language::code()` cannot spell, so this link used to be
/// `/en/` — which no build emits, and which 404s from inside the one tree
/// whose readers most need the way out.
#[test]
fn a_region_tagged_tree_links_home_to_the_folder_that_exists() {
    use crate::i18n::Language;
    let mut about =
        make_doc_with_root_level("en-us/about/index.html", "About", None, Some(true), false);
    about.lang = Language::En;

    let documents = vec![about];
    let nav_html = NavigationBuilder::new(
        &documents,
        "Site",
        Some("en-us/about/index.html"),
        Language::ZhHant,
        true,
    )
    .with_translations(Language::En, "en-US", Vec::new())
    .generate_navigation();

    assert!(
        nav_html.contains(r#"<a href="/en-us/" class="site-name">"#),
        "home link must be the real folder. Got: {}",
        nav_html
    );
}

#[test]
fn test_nav_scoped_to_current_language_en() {
    use crate::i18n::Language;
    let mut en_about =
        make_doc_with_root_level("about/index.html", "About", None, Some(true), true);
    en_about.lang = Language::En;

    let mut zh_about =
        make_doc_with_root_level("zh-hans/about/index.html", "关于", None, Some(true), true);
    zh_about.lang = Language::ZhHans;

    let documents = vec![en_about, zh_about];
    let nav_builder =
        NavigationBuilder::new(&documents, "Site", Some("index.html"), Language::En, true);
    let nav_html = nav_builder.generate_navigation();

    assert!(
        nav_html.contains(">About<"),
        "EN nav should show EN 'About'. Got: {}",
        nav_html
    );
    assert!(
        !nav_html.contains("关于"),
        "EN nav should NOT show 中文 '关于'. Got: {}",
        nav_html
    );
}

#[test]
fn test_nav_scoped_to_current_language_zh() {
    use crate::i18n::Language;
    let mut en_about =
        make_doc_with_root_level("about/index.html", "About", None, Some(true), true);
    en_about.lang = Language::En;

    let mut zh_about =
        make_doc_with_root_level("zh-hans/about/index.html", "关于", None, Some(true), true);
    zh_about.lang = Language::ZhHans;

    let documents = vec![en_about, zh_about];
    let nav_builder = NavigationBuilder::new(
        &documents,
        "Site",
        Some("zh-hans/index.html"),
        Language::En,
        true,
    )
    .with_translations(Language::ZhHans, "zh-Hans", vec![]);
    let nav_html = nav_builder.generate_navigation();

    assert!(
        nav_html.contains("关于"),
        "ZH nav should show 中文 '关于'. Got: {}",
        nav_html
    );
    assert!(
        !nav_html.contains(">About<"),
        "ZH nav should NOT show EN 'About'. Got: {}",
        nav_html
    );
}

#[test]
fn test_nav_unchanged_for_monolingual_site() {
    // Regression guard: all docs same lang, filter is a no-op.
    use crate::i18n::Language;
    let mut doc = make_doc_with_root_level("about/index.html", "About", None, Some(true), true);
    doc.lang = Language::En;

    let documents = vec![doc];
    let nav_builder =
        NavigationBuilder::new(&documents, "Site", Some("index.html"), Language::En, true);
    let nav_html = nav_builder.generate_navigation();
    assert!(
        nav_html.contains(">About<"),
        "Monolingual nav should show 'About'. Got: {}",
        nav_html
    );
}

// --- Footer generation tests ---

/// Helper to create a test ParsedDocument with footer fields
fn make_footer_doc(
    title: &str,
    url_path: &str,
    footer: Option<bool>,
    weight: Option<i32>,
) -> ParsedDocument {
    make_footer_doc_with_align(title, url_path, footer, weight, None)
}

fn make_footer_doc_with_align(
    title: &str,
    url_path: &str,
    footer: Option<bool>,
    weight: Option<i32>,
    footer_align: Option<String>,
) -> ParsedDocument {
    ParsedDocument {
        title: title.to_string(),
        label: title.to_string(),
        url_path: url_path.to_string(),
        footer,
        footer_align,
        weight,
        lang: crate::i18n::Language::En,
        kind: moss_core::PageKind::Article,
        ..Default::default()
    }
}

/// Helper to create a NavigationBuilder for footer tests
fn footer_builder<'a>(
    docs: &'a [ParsedDocument],
    current_page_url: Option<&'a str>,
) -> NavigationBuilder<'a> {
    NavigationBuilder::new(
        docs,
        "Test Site",
        current_page_url,
        crate::i18n::Language::En,
        false,
    )
}

#[test]
fn footer_page_appears_as_link() {
    let docs = vec![make_footer_doc("About", "about.html", Some(true), None)];
    let html = footer_builder(&docs, None).generate_footer(true);
    assert!(
        html.contains(r#"class="footer-link">About</a>"#),
        "footer link missing: {html}"
    );
    // Default link list lives in <p class="footer-default"> alongside the
    // footer.md slot marker (verbatim contract — no .footer-left/.footer-right
    // chrome).
    assert!(
        html.contains(r#"<p class="footer-default">"#),
        "default link list missing: {html}"
    );
}

#[test]
fn footer_pages_sorted_by_weight_then_title() {
    let docs = vec![
        make_footer_doc("Zebra", "zebra.html", Some(true), Some(2)),
        make_footer_doc("Alpha", "alpha.html", Some(true), None),
        make_footer_doc("Beta", "beta.html", Some(true), Some(1)),
        make_footer_doc("Gamma", "gamma.html", Some(true), None),
    ];
    let html = footer_builder(&docs, None).generate_footer(true);
    let beta_pos = html.find("Beta").unwrap();
    let zebra_pos = html.find("Zebra").unwrap();
    let alpha_pos = html.find("Alpha").unwrap();
    let gamma_pos = html.find("Gamma").unwrap();
    assert!(
        beta_pos < zebra_pos,
        "Beta (w=1) should come before Zebra (w=2)"
    );
    assert!(
        zebra_pos < alpha_pos,
        "Zebra (w=2) should come before Alpha (no weight)"
    );
    assert!(
        alpha_pos < gamma_pos,
        "Alpha should come before Gamma alphabetically"
    );
}

#[test]
fn non_footer_pages_excluded() {
    let docs = vec![
        make_footer_doc("About", "about.html", Some(true), None),
        make_footer_doc("Secret", "secret.html", None, None),
        make_footer_doc("Hidden", "hidden.html", Some(false), None),
    ];
    let html = footer_builder(&docs, None).generate_footer(true);
    assert!(html.contains("About"));
    assert!(!html.contains("Secret"));
    assert!(!html.contains("Hidden"));
}

#[test]
fn footer_uses_root_relative_urls() {
    let docs = vec![make_footer_doc(
        "Friends",
        "friends/index.html",
        Some(true),
        None,
    )];
    let html = footer_builder(&docs, None).generate_footer(true);
    assert!(
        html.contains(r#"href="/friends/""#),
        "footer should use root-relative URLs: {html}"
    );
}

#[test]
fn rss_link_present_when_enabled() {
    let docs = vec![make_footer_doc("About", "about.html", Some(true), None)];
    let html = footer_builder(&docs, None).generate_footer(true);
    assert!(
        html.contains("data-external"),
        "RSS link should be present: {html}"
    );
    assert!(
        html.contains(r#"href="/rss.xml""#),
        "RSS should use root-relative URL: {html}"
    );
}

#[test]
fn footer_contains_both_slot_markers() {
    // Two slot markers: footer-left (author chrome / footer.md) and
    // footer-end (auto-injected subscribe form on moss-hosted sites
    // without footer.md). Both are always emitted; injection happens at
    // post-processing time.
    let docs = vec![make_footer_doc("About", "about.html", Some(true), None)];
    let html = footer_builder(&docs, None).generate_footer(true);
    assert!(
        html.contains("<!-- slot:footer-left -->"),
        "footer-left marker missing: {html}"
    );
    assert!(
        html.contains("<!-- slot:footer-end -->"),
        "footer-end marker missing: {html}"
    );
}

#[test]
fn footer_default_links_appear_before_footer_end_marker() {
    // Design B (2026-05-06): the default link list must appear ABOVE the
    // footer-end slot, so the auto-injected subscribe form trails the
    // wayfinding rather than leading it. If this fails, the template was
    // rearranged and the case-2 ordering regressed back to "form first".
    let docs = vec![make_footer_doc("About", "about.html", Some(true), None)];
    let html = footer_builder(&docs, None).generate_footer(true);
    let links_pos = html
        .find(r#"<p class="footer-default">"#)
        .expect("default link list must be present");
    let end_pos = html
        .find("<!-- slot:footer-end -->")
        .expect("footer-end marker must be present");
    assert!(
        links_pos < end_pos,
        "default link list must appear before footer-end slot (design B):\n{html}"
    );
}

#[test]
fn footer_left_marker_appears_before_default_links() {
    // footer.md (slot:footer-left) leads the footer chrome — above the
    // auto-generated link list. Case-3 ordering: author content first,
    // then moss-derived wayfinding.
    let docs = vec![make_footer_doc("About", "about.html", Some(true), None)];
    let html = footer_builder(&docs, None).generate_footer(true);
    let left_pos = html
        .find("<!-- slot:footer-left -->")
        .expect("footer-left marker must be present");
    let links_pos = html
        .find(r#"<p class="footer-default">"#)
        .expect("default link list must be present");
    assert!(
        left_pos < links_pos,
        "footer-left slot must appear before default link list:\n{html}"
    );
}

#[test]
fn footer_align_right_field_keeps_link() {
    // Under the verbatim contract, `footer_align: right` no longer affects
    // visual position (the .footer-left/.footer-right chrome is gone). The
    // link still renders in the default link list — sites that want a
    // multi-column footer author it via footer.md instead.
    let docs = vec![
        make_footer_doc("About", "about.html", Some(true), None),
        make_footer_doc_with_align(
            "Contact",
            "contact.html",
            Some(true),
            None,
            Some("right".into()),
        ),
    ];
    let html = footer_builder(&docs, None).generate_footer(true);
    assert!(
        html.contains("Contact"),
        "Contact must still render: {html}"
    );
    assert!(html.contains("About"), "About must still render: {html}");
}

#[test]
fn footer_emits_flat_html_no_inner_wrapper() {
    // The footer is a single `<footer class="container" ...>` with the
    // authored content as direct children — no `.footer-content` /
    // `.footer-left` / `.footer-right` chrome divs. The default
    // visual chrome (divider, padding, muted typography) lives on
    // `footer.container` directly via CSS. Flat HTML keeps the
    // `body > footer.container > selector` design space open for
    // sites with custom footer designs.
    //
    // The opening tag is a plain `<footer class="container">` (no
    // `data-moss-shape` — retired; footer layout keys on the CSS
    // `:has(> .moss-subscribe)` selector). Here we just check the tag opens
    // correctly without locking the exact attribute set.
    let docs = vec![make_footer_doc("About", "about.html", Some(true), None)];
    let html = footer_builder(&docs, None).generate_footer(true);
    assert!(
        html.contains("<footer class=\"container\""),
        "single footer wrapper required: {html}"
    );
    assert!(
        !html.contains("footer-content"),
        "no .footer-content wrapper (chrome lives on footer.container): {html}"
    );
    assert!(
        !html.contains("class=\"footer-left"),
        "no .footer-left flex column: {html}"
    );
    // Note: the slot MARKER `<!-- slot:footer-end -->` IS allowed (and
    // expected — see footer_contains_both_slot_markers). What is NOT
    // allowed is a class="footer-right" or class="footer-end" wrapper
    // div — the legacy flex-column chrome that was stripped in the
    // verbatim-footer refactor.
    assert!(
        !html.contains("class=\"footer-right"),
        "no .footer-right flex column: {html}"
    );
    assert!(
        !html.contains("class=\"footer-end"),
        "no .footer-end wrapper class (slot marker is comment-only): {html}"
    );
}

#[test]
fn footer_default_open_tag_is_parse_safe() {
    // The `<footer>` open tag is a plain `<footer class="container">` with
    // NO slot marker inside the start tag. The `data-moss-shape` attribute
    // (which once toggled the footer flex layout from the build side) is
    // retired — footer layout now keys on the CSS `:has(> .moss-subscribe)`
    // selector, so no build-side marker is needed at all.
    //
    // CRITICAL: the open tag must NEVER contain an HTML comment. A
    // `<!-- … -->` inside a start tag is not a comment per the HTML
    // tokenizer — it parses as bogus attributes and the trailing `>` leaks
    // as literal text (a stray ">" at the top of the footer). A plain
    // `<footer class="container">` is trivially valid HTML.
    let docs = vec![make_footer_doc("About", "about.html", Some(true), None)];
    let html = footer_builder(&docs, None).generate_footer(true);

    // Isolate the `<footer …>` open tag.
    let open_start = html.find("<footer").expect("footer open tag present");
    let open_end = html[open_start..].find('>').expect("open tag closes") + open_start;
    let open_tag = &html[open_start..=open_end];

    assert!(
        !open_tag.contains("<!--"),
        "footer open tag must not contain a parse-breaking HTML comment: {open_tag}"
    );
    assert!(
        !open_tag.contains("data-moss-shape"),
        "footer open tag must NOT carry the retired data-moss-shape attribute: {open_tag}"
    );
    assert_eq!(
        open_tag, r#"<footer class="container">"#,
        "footer open tag must be a plain class-only tag: {open_tag}"
    );
}

#[test]
fn rss_link_hidden_when_disabled() {
    let docs = vec![make_footer_doc("About", "about.html", Some(true), None)];
    let html = footer_builder(&docs, None).generate_footer(false);
    assert!(
        !html.contains("rss.xml"),
        "RSS link should not appear when disabled: {html}"
    );
    assert!(
        !html.contains("data-external"),
        "no external link when RSS disabled: {html}"
    );
}

#[test]
fn rss_link_shown_when_enabled() {
    let docs = vec![make_footer_doc("About", "about.html", Some(true), None)];
    let html = footer_builder(&docs, None).generate_footer(true);
    assert!(
        html.contains("rss.xml"),
        "RSS link should appear when enabled: {html}"
    );
}

#[test]
fn empty_footer_has_slot_but_no_links() {
    let docs = vec![make_footer_doc("Page", "page.html", None, None)];
    let html = footer_builder(&docs, None).generate_footer(false);
    assert!(
        html.contains("slot:footer-left"),
        "Slot marker must be present: {html}"
    );
    assert!(
        !html.contains("footer-link"),
        "No footer links when no pages and RSS off: {html}"
    );
}

#[test]
fn footer_with_only_rss_still_renders() {
    let docs = vec![make_footer_doc("Page", "page.html", None, None)];
    let html = footer_builder(&docs, None).generate_footer(true);
    assert!(
        !html.is_empty(),
        "Footer with RSS should render even without footer pages"
    );
    assert!(html.contains("rss.xml"));
}

// --- Active state tests ---

#[test]
fn footer_link_active_when_current_page_matches() {
    let docs = vec![
        make_footer_doc("News", "news/index.html", Some(true), None),
        make_footer_doc("About", "about.html", Some(true), None),
    ];
    let html = footer_builder(&docs, Some("news/index.html")).generate_footer(false);
    assert!(
        html.contains(r#"class="footer-link active">News</a>"#),
        "News should have active class: {html}"
    );
    assert!(
        html.contains(r#"class="footer-link">About</a>"#),
        "About should NOT have active class: {html}"
    );
}

#[test]
fn footer_link_no_active_when_page_not_in_footer() {
    let docs = vec![
        make_footer_doc("News", "news/index.html", Some(true), None),
        make_footer_doc("About", "about.html", Some(true), None),
    ];
    let html = footer_builder(&docs, Some("projects.html")).generate_footer(false);
    assert!(
        !html.contains("active"),
        "No footer link should be active when current page is not a footer page: {html}"
    );
}

#[test]
fn footer_link_no_active_when_current_url_none() {
    let docs = vec![make_footer_doc("News", "news/index.html", Some(true), None)];
    let html = footer_builder(&docs, None).generate_footer(false);
    assert!(
        !html.contains("active"),
        "No active class when current_page_url is None: {html}"
    );
}

#[test]
fn footer_link_active_with_right_align() {
    let docs = vec![
        make_footer_doc("News", "news/index.html", Some(true), None),
        make_footer_doc_with_align(
            "Contact",
            "contact.html",
            Some(true),
            None,
            Some("right".into()),
        ),
    ];
    let html = footer_builder(&docs, Some("contact.html")).generate_footer(false);
    assert!(
        html.contains(r#"class="footer-link active">Contact</a>"#),
        "Right-aligned footer link should get active class: {html}"
    );
    assert!(
        html.contains(r#"class="footer-link">News</a>"#),
        "News should NOT have active class: {html}"
    );
}

// --- is_nav_bar_item unit tests ---

#[test]
fn is_nav_bar_item_explicit_true() {
    // nav: true wins for any file, any depth.
    assert!(super::is_nav_bar_item(
        Some(true),
        false,
        false,
        false,
        false,
        "post",
        false
    ));
}

#[test]
fn is_nav_bar_item_explicit_false() {
    // nav: false opts out even a root keyword file in organized mode.
    assert!(!super::is_nav_bar_item(
        Some(false),
        false,
        false,
        true,
        false,
        "about",
        true
    ));
}

#[test]
fn is_nav_bar_item_slot_only_never_nav() {
    // Slot files (footer.md) are chrome, never nav items — even nav: true.
    assert!(!super::is_nav_bar_item(
        Some(true),
        false,
        true,
        true,
        false,
        "footer",
        true
    ));
}

#[test]
fn is_nav_bar_item_organized_root_nonindex() {
    // Organized mode: a root-level non-index file auto-appears.
    assert!(super::is_nav_bar_item(
        None, false, false, true, false, "about", true
    ));
}

#[test]
fn is_nav_bar_item_organized_nested_excluded() {
    // Nested file never auto-appears.
    assert!(!super::is_nav_bar_item(
        None, false, false, false, false, "post", true
    ));
}

#[test]
fn is_nav_bar_item_organized_root_index_excluded() {
    // The root index is the home page, not a nav item.
    assert!(!super::is_nav_bar_item(
        None, false, false, true, true, "index", true
    ));
}

#[test]
fn is_nav_bar_item_flat_keyword() {
    // Flat mode: only keyword filenames auto-appear.
    assert!(super::is_nav_bar_item(
        None, false, false, true, false, "about", false
    ));
}

#[test]
fn is_nav_bar_item_flat_nonkeyword_excluded() {
    assert!(!super::is_nav_bar_item(
        None, false, false, true, false, "contact", false
    ));
}

#[test]
fn is_nav_bar_item_draft_never_nav() {
    // draft wins over explicit nav: true and over auto-navigational status
    assert!(!super::is_nav_bar_item(
        Some(true),
        true,
        false,
        true,
        false,
        "about",
        true
    ));
    assert!(!super::is_nav_bar_item(
        None, true, false, true, false, "about", true
    ));
}

fn make_footer_doc_with_lang(
    title: &str,
    url_path: &str,
    lang: crate::i18n::Language,
) -> ParsedDocument {
    ParsedDocument {
        title: title.to_string(),
        label: title.to_string(),
        url_path: url_path.to_string(),
        footer: Some(true),
        lang,
        kind: moss_core::PageKind::Article,
        ..Default::default()
    }
}

#[test]
fn footer_excludes_docs_from_other_language() {
    // A Chinese footer doc must not appear when the nav is built for an English page.
    let docs = vec![
        make_footer_doc_with_lang("友链", "links/index.html", crate::i18n::Language::ZhHans),
        make_footer_doc_with_lang("About", "about/index.html", crate::i18n::Language::En),
    ];
    let html = NavigationBuilder::new(&docs, "Test", None, crate::i18n::Language::En, false)
        .generate_footer(false);
    assert!(
        !html.contains("友链"),
        "Chinese footer link must not appear on English page"
    );
    assert!(
        html.contains("About"),
        "English footer link must appear on English page"
    );
}

#[test]
fn footer_excludes_english_docs_from_chinese_page() {
    // An English footer doc must not appear when the nav is built for a Chinese page.
    let docs = vec![
        make_footer_doc_with_lang("友链", "links/index.html", crate::i18n::Language::ZhHans),
        make_footer_doc_with_lang("About", "about/index.html", crate::i18n::Language::En),
    ];
    let html = NavigationBuilder::new(&docs, "Test", None, crate::i18n::Language::ZhHans, false)
        .generate_footer(false);
    assert!(
        html.contains("友链"),
        "Chinese footer link must appear on Chinese page"
    );
    assert!(
        !html.contains("About"),
        "English footer link must not appear on Chinese page"
    );
}

#[test]
fn switcher_never_repeats_current_language() {
    // Regression for "EN / EN": the consolidated helper feeds the switcher
    // an EN page whose only "other language" link is ZH (the ZH root). The
    // switcher must render exactly one ZH link and exactly one EN label —
    // never a second EN.
    let documents: Vec<ParsedDocument> = vec![];
    let nav = NavigationBuilder::new(&documents, "Site", None, Language::En, false)
        .with_translations(
            Language::En,
            "en",
            vec![TranslationLink {
                lang_tag: Language::ZhHans.as_bcp47_attr().to_string(),
                url_path: "index.html".to_string(),
                display_name: "简",
            }],
        );
    let html = nav.generate_navigation();
    // Exact-markup assertions (NOT a fragile `matches("EN").count()` — "EN"
    // can legitimately appear in a site name, aria-label, etc.). Assert on
    // the switcher's own structure:
    //   - the current-language label is exactly the EN span,
    //   - the ZH alternate link is present,
    //   - there is NO EN alternate link (the bug was a second EN <a>).
    assert!(
        html.contains(r#"<span class="nav-lang-current">EN</span>"#),
        "current label must be the EN span"
    );
    assert!(
        html.contains(r#"hreflang="zh-Hans""#) && html.contains("简"),
        "ZH alternate link present"
    );
    assert!(
        !html.contains(r#"class="nav-lang-link" hreflang="en""#),
        "no EN alternate link — the current language is never a switcher link"
    );
}

// ---------------------------------------------------------------------------
// Search button
// ---------------------------------------------------------------------------

/// The nav search button is emitted ONLY when the resolved search gate is on.
/// The default (`NavigationBuilder::new` without `.with_search(true)`) must be
/// off — a site built while `[site].search`, preview-features, or a real
/// deployment URL was missing has no `_moss/pagefind/` bundle, and a button
/// pointing at a missing index is worse than no button.
#[test]
fn test_search_button_absent_by_default() {
    let documents = vec![make_doc("index.html", "Home", None, None)];
    let html = NavigationBuilder::new(
        &documents,
        "Test Site",
        None,
        crate::i18n::Language::En,
        false,
    )
    .generate_navigation();

    assert!(
        !html.contains("nav-search-btn"),
        "search button must be off by default"
    );
    // The other two icons are unaffected.
    assert!(
        html.contains("nav-theme-btn"),
        "theme toggle should still render"
    );
}

/// The one test that pins the toggle cluster's full order.
///
/// Left to right: search, language, theme — most content-related control to
/// most presentation-related. Search sits nearest the nav links it complements
/// (it is navigation); language changes which content you read; theme is pure
/// presentation and keeps its established right-most anchor. It used to run
/// language / search / theme.
#[test]
fn test_icon_cluster_order_is_search_language_theme() {
    let documents = vec![make_doc("index.html", "Home", None, None)];
    let translations = vec![TranslationLink {
        lang_tag: crate::i18n::Language::ZhHans.as_bcp47_attr().to_string(),
        url_path: "index.zh-hans.html".to_string(),
        display_name: "简",
    }];
    let html = NavigationBuilder::new(
        &documents,
        "Test Site",
        None,
        crate::i18n::Language::En,
        false,
    )
    .with_search(true)
    .with_translations(crate::i18n::Language::En, "en", translations)
    .generate_navigation();

    assert!(
        html.contains(r#"class="nav-search-btn""#),
        "search button should render when the gate is open"
    );
    assert!(
        html.contains(r#"aria-label="Search""#),
        "search button needs an accessible name"
    );

    let icons_at = html.find("nav-icons").expect("nav-icons container");
    let search_at = html.find("nav-search-btn").expect("search button");
    let lang_at = html.find("nav-lang-toggle").expect("language toggle");
    let theme_at = html.find("nav-theme-btn").expect("theme button");
    assert!(
        icons_at < search_at,
        "the cluster must live inside .nav-icons"
    );
    assert!(
        search_at < lang_at && lang_at < theme_at,
        "cluster order must be search → language → theme, got search@{} lang@{} theme@{}",
        search_at,
        lang_at,
        theme_at
    );

    // NO toggle carries a hover hint — `aria-label` only. The glyphs are
    // their own labels, so a pill repeating them was noise, and on touch a
    // tapped toggle's hint stuck on screen (no un-hover). The nav's only
    // hints are truncation hints, JS-promoted onto breadcrumb elements.
    // Asserted here rather than left to the snapshot fixtures, which would
    // report a regression as an opaque byte diff.
    assert!(
        !html.contains("data-tooltip"),
        "nav toggles must not emit data-tooltip — hover hints are reserved \
         for truncated breadcrumbs and are JS-promoted, never emitted. Got: {}",
        html
    );
    assert!(
        html.contains(r#"aria-label="Search""#) && html.contains(r#"aria-label="Toggle theme""#),
        "toggles keep their accessible names via aria-label, got: {}",
        html
    );
}

/// The button's accessible name is localized, like every other nav control.
#[test]
fn test_search_button_label_is_localized() {
    let documents = vec![make_doc("index.html", "首页", None, None)];
    let html = NavigationBuilder::new(
        &documents,
        "测试站点",
        None,
        crate::i18n::Language::ZhHans,
        false,
    )
    .with_search(true)
    .generate_navigation();

    assert!(
        html.contains(r#"aria-label="搜索""#),
        "expected zh-Hans label, got: {}",
        html
    );
}

// =========================================================================
// Floating nav island
//
// The island is a SECOND object, not the masthead re-pinned. These tests pin
// the two properties that make that claim checkable from the emitted text:
// it reuses the masthead's breadcrumb classes exactly, and it ends where the
// masthead stops — on the current page. Everything about how it behaves
// (reveal, folding, panels) is measured, so it is tested in the render gate
// (tests/render-gates/site/nav-island.spec.ts) and in
// crates/moss-build/src/js-src/site/__tests__/nav-island.test.ts, not here.
// =========================================================================

/// Build the island for a page at `posts/hello/index.html` on a 3-level site.
fn island_for_deep_page() -> String {
    let documents = vec![
        make_doc_with_breadcrumb("index.html", "Home", Some(true)),
        make_doc_with_breadcrumb("posts/index.html", "Posts", None),
        make_doc_with_breadcrumb("posts/hello/index.html", "Hello World", None),
    ];
    let doc = &documents[2];
    let segments = compute_breadcrumb_segments(doc, &documents, "My Site", true).unwrap();
    NavigationBuilder::new(
        &documents,
        "My Site",
        Some(doc.url_path.as_str()),
        crate::i18n::Language::En,
        true,
    )
    .with_breadcrumb(segments)
    .generate_nav_island()
}

#[test]
fn island_trail_reuses_the_mastheads_breadcrumb_classes() {
    // "Looks the same as the nav bar" is a property someone has to
    // maintain by eye if the two are separate markup. Sharing the classes makes
    // it true by construction.
    let island = island_for_deep_page();
    assert!(island.contains(r#"class="site-name""#), "got: {island}");
    assert!(island.contains(r#"class="breadcrumb-segment""#), "got: {island}");
    assert!(island.contains(r#"<span class="breadcrumb-label">Posts</span>"#), "got: {island}");
    assert!(island.contains(r#"<span class="breadcrumb-separator">/</span>"#), "got: {island}");
}

#[test]
fn island_appends_the_current_page_the_masthead_skips() {
    // In the masthead the page title is directly below, on screen;
    // in the island it is not, so the island supplies it — as the last crumb,
    // not as a separate bold title field.
    let island = island_for_deep_page();
    assert!(
        island.contains(r#"aria-current="page""#),
        "the current page must be marked, got: {island}"
    );
    assert!(
        island.contains("Hello World"),
        "the current page must be the final crumb, got: {island}"
    );
    // …and it is not a link: you are already there.
    let current_start = island.find("aria-current").expect("marked crumb");
    let before = &island[..current_start];
    assert!(
        before.rfind("<span").unwrap() > before.rfind("<a ").unwrap(),
        "the current crumb must be a <span>, not an <a>, got: {island}"
    );
}

#[test]
fn the_masthead_still_stops_before_the_current_page() {
    // The island's extra crumb must not leak back into the nav bar.
    let documents = vec![
        make_doc_with_breadcrumb("index.html", "Home", Some(true)),
        make_doc_with_breadcrumb("posts/index.html", "Posts", None),
        make_doc_with_breadcrumb("posts/hello/index.html", "Hello World", None),
    ];
    let doc = &documents[2];
    let segments = compute_breadcrumb_segments(doc, &documents, "My Site", true).unwrap();
    let nav_html = NavigationBuilder::new(
        &documents,
        "My Site",
        Some(doc.url_path.as_str()),
        crate::i18n::Language::En,
        true,
    )
    .with_breadcrumb(segments)
    .generate_navigation();

    assert!(!nav_html.contains("Hello World"), "got: {nav_html}");
    assert!(!nav_html.contains("aria-current"), "got: {nav_html}");
}

#[test]
fn island_folds_only_when_there_is_a_middle_to_sacrifice() {
    // Three crumbs (site / Posts / Hello World) have exactly one foldable
    // ancestor, so the "…" ships hidden and the script may unhide it.
    let deep = island_for_deep_page();
    assert!(deep.contains(r#"class="moss-nav-island-more" hidden"#), "got: {deep}");
    assert!(deep.contains(r#"aria-expanded="false""#), "got: {deep}");

    // A top-level page has two crumbs — site name and the page — and neither is
    // ever droppable, so there is no "…" at all. This is the answer to an open
    // question about top-level pages: the island still ships,
    // because "back to the site root" is the one route a long page's reader
    // cannot otherwise take, and an affordance that vanishes at depth 1 reads
    // as a bug.
    let documents = vec![
        make_doc_with_breadcrumb("index.html", "Home", Some(true)),
        make_doc_with_breadcrumb("about/index.html", "About Us", None),
    ];
    let doc = &documents[1];
    let segments = compute_breadcrumb_segments(doc, &documents, "My Site", true).unwrap();
    let shallow = NavigationBuilder::new(
        &documents,
        "My Site",
        Some(doc.url_path.as_str()),
        crate::i18n::Language::En,
        true,
    )
    .with_breadcrumb(segments)
    .generate_nav_island();

    assert!(shallow.contains("My Site"), "got: {shallow}");
    assert!(shallow.contains("About Us"), "got: {shallow}");
    assert!(!shallow.contains("moss-nav-island-more"), "got: {shallow}");
    assert_eq!(
        shallow.matches("breadcrumb-separator").count(),
        1,
        "exactly one separator between two crumbs, got: {shallow}"
    );
}

#[test]
fn no_island_without_a_breadcrumb_trail() {
    // The homepage, and any site that has turned breadcrumbs off. Nowhere to go
    // "up" to, so no island — and no empty bar hanging over the page.
    let documents = vec![make_doc_with_breadcrumb("index.html", "Home", Some(true))];
    let island = NavigationBuilder::new(
        &documents,
        "My Site",
        Some("index.html"),
        crate::i18n::Language::En,
        true,
    )
    .generate_nav_island();
    assert_eq!(island, "");

    let empty = NavigationBuilder::new(
        &documents,
        "My Site",
        Some("index.html"),
        crate::i18n::Language::En,
        true,
    )
    .with_breadcrumb(vec![])
    .generate_nav_island();
    assert_eq!(empty, "");
}

#[test]
fn island_buttons_ship_inert_so_a_scriptless_page_shows_no_dead_chrome() {
    // The base state must work with JavaScript off, and
    // an inert icon in shipped chrome is worse than no icon.
    // The `…` is `hidden` in the emitted HTML because the trail may never need
    // to fold. The sections button is not: since 2026-08-30 an island only
    // ever shows on a page with a contents table, so its button always has
    // something to open — and with no script the whole island is
    // `display: none` anyway, which is what makes the attribute unobservable.
    let island = island_for_deep_page();
    assert!(!island.contains(r#"class="moss-nav-island-sections" hidden"#), "got: {island}");
    assert!(island.contains(r#"class="moss-nav-island-more" hidden"#), "got: {island}");
    // Both menus ship empty and hidden — they are filled in by measurement.
    assert!(
        island.contains(r#"data-island-menu="levels" hidden></div>"#),
        "got: {island}"
    );
    assert!(
        island.contains(r#"data-island-menu="sections" hidden></div>"#),
        "got: {island}"
    );
    // And no search: the masthead has it.
    assert!(!island.contains("nav-search-btn"), "got: {island}");
}

#[test]
fn island_claims_no_interaction_model_it_does_not_implement() {
    // `role="menu"` / `role="menuitem"` / `aria-haspopup` all
    // promise arrow-key roving focus. The panels are a popover of plain links
    // and have no such thing, and a screen-reader user told "menu" will press
    // ArrowDown and find that nothing moves — worse than never claiming it.
    // `aria-expanded` on the button already says the true thing.
    //
    // Asserted on the emitted HTML rather than left to review, because the
    // roles were in the first implementation and reading right past them is
    // exactly what happened the first time.
    let island = island_for_deep_page();
    assert!(!island.contains("role=\"menu\""), "got: {island}");
    assert!(!island.contains("role=\"menuitem\""), "got: {island}");
    assert!(!island.contains("aria-haspopup"), "got: {island}");
}

#[test]
fn island_current_crumb_can_say_what_it_truncated() {
    // The current page is the ONE crumb the island lets ellipsise, which
    // makes it the one that needs a way to reveal the rest. `data-hint-label`
    // is what `breadcrumb-hint.ts` promotes to a real tooltip
    // while — and only while — the label is genuinely cut off.
    let island = island_for_deep_page();
    assert!(
        island.contains(r#"aria-current="page" data-island-crumb data-hint-label="Hello World""#),
        "got: {island}"
    );
}

#[test]
fn island_labels_are_localized_and_attribute_safe() {
    let documents = vec![
        make_doc_with_breadcrumb("index.html", "Home", Some(true)),
        make_doc_with_breadcrumb("posts/index.html", r#"He said "hi""#, None),
        make_doc_with_breadcrumb("posts/hello/index.html", "Hello", None),
    ];
    let doc = &documents[2];
    let segments = compute_breadcrumb_segments(doc, &documents, "潮汐", true).unwrap();
    let island = NavigationBuilder::new(
        &documents,
        "潮汐",
        Some(doc.url_path.as_str()),
        crate::i18n::Language::ZhHant,
        true,
    )
    .with_breadcrumb(segments)
    .generate_nav_island();

    assert!(island.contains(r#"aria-label="本頁章節""#), "got: {island}");
    assert!(island.contains(r#"aria-label="顯示省略的層級""#), "got: {island}");
    assert!(island.contains(r#"aria-label="路徑""#), "got: {island}");
}
