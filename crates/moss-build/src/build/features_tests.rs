use super::*;

/// What `generate_native_slots` used to read for itself: is the `email`
/// channel installed for this project? The function now takes it as an
/// argument (ADR-050 keeps plugin discovery out of the compiler), so these
/// tests answer it the same way the production caller does — from the
/// `[channels]` table the test wrote to disk.
fn email_installed(project_path: &str) -> bool {
    crate::plugins::discovery::get_channels_config(project_path)
        .unwrap_or_default()
        .is_installed("email")
}

#[test]
fn test_html_escape_single_quotes() {
    assert_eq!(html_escape("it's"), "it&#39;s");
    assert_eq!(html_escape("a'b\"c"), "a&#39;b&quot;c");
    // All five HTML special characters
    assert_eq!(html_escape("&<>\"'"), "&amp;&lt;&gt;&quot;&#39;");
}

fn analytics_fixture(script: &str) -> ServicesConfig {
    ServicesConfig {
        analytics: Some(crate::config::services::AnalyticsService {
            common: crate::config::services::ServiceCommon {
                enabled: None,
                provider: Some("goatcounter".into()),
            },
            script: Some(script.into()),
        }),
        ..Default::default()
    }
}

fn comments_fixture(server_url: &str) -> ServicesConfig {
    ServicesConfig {
        comments: Some(crate::config::services::CommentsService {
            common: crate::config::services::ServiceCommon {
                enabled: None,
                provider: Some("artalk".into()),
            },
            server_url: if server_url.is_empty() {
                None
            } else {
                Some(server_url.into())
            },
        }),
        ..Default::default()
    }
}

// (removed) test_analytics_slot_uses_to_script_tag asserted goatcounter SCRIPT
// emission into head-end. Analytics is now mode-INDEPENDENT: always injected,
// wrapped in <!--moss:no-preview--> markers that the preview server strips — see
// analytics_present_in_both_build_modes below and the beacon tests
// (beacon_injected_for_moss_hosted_build / beacon_injected_during_preview_mode_independent).

fn make_article_map() -> HashMap<String, ArticleInfo> {
    let mut map = HashMap::new();
    map.insert(
        "uid1".to_string(),
        ArticleInfo {
            url_path: "posts/test.html".to_string(),
            title: "Test".to_string(),
            uid: "uid1".to_string(),
            review_of: None,
            rating: None,
            cover: None,
            comments: None,
            lang: None,
            syndicated: Vec::new(),
        },
    );
    map
}

fn moss_hosted_deploy_config(site_id: &str) -> DomainDeploymentConfig {
    DomainDeploymentConfig {
        deploy_method: Some("moss".into()),
        site_id: Some(site_id.into()),
        ..Default::default()
    }
}

/// Returns `(ServicesConfig, DomainDeploymentConfig, TempDir)`.
/// Keep `tmp` bound for the entire test body — it owns the temp directory;
/// derive `project_path` after destructuring (not in the same expression).
fn moss_hosted_fixture() -> (ServicesConfig, DomainDeploymentConfig, tempfile::TempDir) {
    // Test artifacts live inside the repo (target/test-tmp), per project rule —
    // matches the established pattern used by the other tests in this module.
    let test_tmp = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/test-tmp");
    std::fs::create_dir_all(test_tmp).unwrap();
    let tmp = tempfile::TempDir::new_in(test_tmp).unwrap();
    let moss_dir = tmp.path().join(".moss");
    std::fs::create_dir_all(&moss_dir).unwrap();
    std::fs::write(
        moss_dir.join("config.toml"),
        &format!(
            "schema_version = {}\n\n[channels.email]\n",
            crate::config::migrations::CURRENT_VERSION
        ),
    )
    .unwrap();
    let config = ServicesConfig::default();
    let deploy = moss_hosted_deploy_config("test-site");
    (config, deploy, tmp)
}

fn make_pages() -> Vec<crate::build::types::ParsedDocument> {
    vec![crate::build::types::ParsedDocument {
        url_path: "index.html".to_string(),
        ..Default::default()
    }]
}

#[test]
fn beacon_injected_for_moss_hosted_build() {
    let config = ServicesConfig::default();
    let deploy = moss_hosted_deploy_config("landing");
    let slots = generate_native_slots(
        &config,
        "/tmp/nonexistent",
        email_installed("/tmp/nonexistent"),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &HashMap::new(),
        &[],
        "en",
        None,
        Some(deploy),
        false, // build, not preview
        None,  // no media lookup in this test
        None, // [site] comments unset
    );
    let html = slots.get_html("head-end", "index.html").unwrap_or_default();
    assert!(
        html.contains("/api/sites/landing/beacon"),
        "beacon script must be injected on a moss-hosted build"
    );
}

#[test]
fn beacon_injected_during_preview_mode_independent() {
    // Previously this test asserted absence in preview (start_server=true).
    // The CORS issue (protocol-relative URL resolving to http:// under the
    // local preview origin) is now handled by the data-moss-preview self-gate
    // in the beacon script itself (Task A4) — the beacon is in the slot in
    // both modes but no-ops at runtime when data-moss-preview is present.
    // This flips the old beacon_skipped_during_preview assertion to pin the
    // mode-independence fix.
    let config = ServicesConfig::default();
    let deploy = moss_hosted_deploy_config("landing");
    let slots = generate_native_slots(
        &config,
        "/tmp/nonexistent",
        email_installed("/tmp/nonexistent"),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &HashMap::new(),
        &[],
        "en",
        None,
        Some(deploy),
        true, // preview — beacon must still be in the slot
        None, // no media lookup in this test
        None, // [site] comments unset
    );
    let html = slots.get_html("head-end", "index.html").unwrap_or_default();
    assert!(
        html.contains("/api/sites/landing/beacon"),
        "beacon must be injected even in preview mode (self-gates at runtime), got: {html}"
    );
}

#[test]
fn test_comment_form_gets_inactive_class_without_endpoint() {
    let config = comments_fixture("");
    let article_map = make_article_map();
    let slots = generate_native_slots(
        &config,
        "/tmp/nonexistent",
        email_installed("/tmp/nonexistent"),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &article_map,
        &[],
        "en",
        None,
        None,
        false,
        None,
        None, // [site] comments unset
    );
    let html = slots.get_html("after-article", "posts/test.html");
    assert!(html.is_some());
    let html = html.unwrap();
    assert!(
        html.contains("moss-service-inactive"),
        "Form should have inactive class when no endpoint"
    );
    assert!(
        html.contains("aria-label=\"Available after publishing\""),
        "Form should name its inactive state when no endpoint"
    );
}

/// #1013: folder-index pages are deliberately absent from the article map, so
/// the comment page set used to end at the articles and a folder page could
/// never hold a conversation — however its author configured it. Nothing about
/// having children decides that, so the set now includes every folder index,
/// the homepage among them — but unlike an article, a folder page (the
/// homepage included) defaults OFF: `comments: true` is what opts it in.
/// `comments: false` still opts an article out. A prior version of this test
/// asserted `None` on the homepage produced a section — that was the
/// regression (homepage comments with no frontmatter at all), not the spec.
///
/// The second and third calls (below the first `generate_native_slots` call)
/// pin `[site] comments`: it used to reach `resolve_comments_attr`'s
/// `data-comments` JS hint but never `generate_native_slots`, so
/// `[site] comments = false` left the `<section class="moss-comments">`
/// block rendered on every article. The site toggle sets the ARTICLE default
/// only; an explicit page-level `comments:` still wins over it, and a folder
/// page stays opt-in-only even when the site toggle is ON. The first call
/// below passes `None` for the new `site_comments` parameter, so it doubles
/// as the "site unset → unchanged" case.
#[test]
fn folder_index_pages_default_off_articles_default_on() {
    let folder = |url: &str, uid: &str, comments: Option<bool>| {
        crate::build::types::ParsedDocument {
            url_path: url.to_string(),
            title: "Topics".to_string(),
            uid: Some(uid.to_string()),
            kind: moss_core::PageKind::Folder,
            comments,
            ..Default::default()
        }
    };
    let pages = vec![
        folder("index.html", "home-uid", None),
        folder("topics/index.html", "topics-uid", Some(true)),
        folder("quiet/index.html", "quiet-uid", Some(false)),
    ];
    // make_article_map's one entry (posts/test.html, comments: None) doubles
    // as the article default-on case.
    let slots = generate_native_slots(
        &comments_fixture("https://comments.example.com"),
        "/tmp/nonexistent",
        email_installed("/tmp/nonexistent"),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &make_article_map(),
        &pages,
        "en",
        None,
        None,
        false,
        None,
        None, // [site] comments unset
    );
    for (page, expected) in [
        ("index.html", 0),        // homepage, no frontmatter → opt-in default OFF
        ("topics/index.html", 1), // folder, comments: true → opted in
        ("quiet/index.html", 0),  // folder, comments: false → opted out
        ("posts/test.html", 1),   // article, no frontmatter → opt-out default ON
    ] {
        let html = slots.get_html("after-article", page).unwrap_or_default();
        assert_eq!(
            html.matches(r#"<section class="moss-comments""#).count(),
            expected,
            "{page} should carry {expected} comment section(s); got: {html:?}"
        );
    }

    // `[site] comments` is now a caller-resolved parameter (ADR-050 §1 — the
    // same reason `email_installed`/`matters_domain` are parameters rather
    // than re-reads: `pipeline.rs::build_inner` already parses
    // `.moss/config.toml` once for `render::SiteConfig`; a second parse here
    // would be a second ladder that can disagree with the first), so these
    // cases pass it directly instead of writing a `.moss/config.toml` fixture.
    //
    // site comments=false: the article default flips off. A page-level
    // `comments: true` still overrides it (same ladder `resolve_comments_attr`
    // uses for `data-comments`, reused here rather than a second precedence
    // check — see `resolve_comments_pref`).
    let mut site_off_article_map = make_article_map(); // uid1 -> posts/test.html, comments: None
    site_off_article_map.insert(
        "uid2".to_string(),
        ArticleInfo {
            url_path: "posts/opt-in.html".to_string(),
            title: "Opt In".to_string(),
            uid: "uid2".to_string(),
            review_of: None,
            rating: None,
            cover: None,
            comments: Some(true),
            lang: None,
            syndicated: Vec::new(),
        },
    );
    let site_off_slots = generate_native_slots(
        &comments_fixture("https://comments.example.com"),
        "/tmp/nonexistent",
        email_installed("/tmp/nonexistent"),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &site_off_article_map,
        &[],
        "en",
        None,
        None,
        false,
        None,
        Some(false), // [site] comments = false
    );
    for (page, expected) in [
        ("posts/test.html", 0),   // article, no frontmatter, site comments=false → OFF
        ("posts/opt-in.html", 1), // article, comments: true → page overrides site
    ] {
        let html = site_off_slots.get_html("after-article", page).unwrap_or_default();
        assert_eq!(
            html.matches(r#"<section class="moss-comments""#).count(),
            expected,
            "{page} should carry {expected} comment section(s) under [site] comments=false; got: {html:?}"
        );
    }

    // site comments=true — the risky direction: it must NOT leak into folder
    // pages. A naive fix would thread `site_comments` into the folder half of
    // the chained iterator too (instead of features.rs's hardcoded `None` for
    // `folder_targets`), which would light up every folder page's comments
    // the moment an author turned the site-wide toggle on. `pages` here is
    // the same homepage/topics/quiet fixture from the top of this test.
    let site_on_slots = generate_native_slots(
        &comments_fixture("https://comments.example.com"),
        "/tmp/nonexistent",
        email_installed("/tmp/nonexistent"),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &make_article_map(),
        &pages,
        "en",
        None,
        None,
        false,
        None,
        Some(true), // [site] comments = true
    );
    let html = site_on_slots.get_html("after-article", "index.html").unwrap_or_default();
    assert_eq!(
        html.matches(r#"<section class="moss-comments""#).count(),
        0,
        "homepage (folder, no frontmatter) must stay opt-in-only even under [site] comments=true; got: {html:?}"
    );
}

#[test]
fn test_comment_form_inactive_zh_tooltip() {
    let config = comments_fixture("");
    let article_map = make_article_map();
    let slots = generate_native_slots(
        &config,
        "/tmp/nonexistent",
        email_installed("/tmp/nonexistent"),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &article_map,
        &[],
        "zh-hans",
        None,
        None,
        false,
        None,
        None, // [site] comments unset
    );
    let html = slots.get_html("after-article", "posts/test.html").unwrap();
    assert!(
        html.contains("aria-label=\"发布后启用\""),
        "Chinese inactive label expected"
    );
}

#[test]
fn comment_section_uses_per_page_lang_not_site_lang() {
    // An English page (resolved doc.lang = En) inside a Chinese-default
    // site (site lang = zh-hans) must render its comment UI in English.
    // ArticleInfo.lang is deliberately None (the page inherits English
    // from an `en/` folder, carrying no explicit `lang:` frontmatter) so
    // this only passes if the fix consults the resolved per-page language
    // from `pages`, not the frontmatter-only field. Regression guard for
    // the site_lang-instead-of-doc.lang render-path family.
    use crate::i18n::Language;
    let config = comments_fixture(""); // inactive → localized tooltip present
    let mut article_map = HashMap::new();
    article_map.insert(
        "en-uid".to_string(),
        ArticleInfo {
            url_path: "en/post.html".to_string(),
            title: "Post".to_string(),
            uid: "en-uid".to_string(),
            review_of: None,
            rating: None,
            cover: None,
            comments: None,
            lang: None,
            syndicated: Vec::new(),
        },
    );
    let pages = vec![crate::build::types::ParsedDocument {
        url_path: "en/post.html".to_string(),
        uid: Some("en-uid".to_string()),
        lang: Language::En,
        ..Default::default()
    }];
    let slots = generate_native_slots(
        &config,
        "/tmp/nonexistent",
        email_installed("/tmp/nonexistent"),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &article_map,
        &pages,
        "zh-hans",
        None,
        None,
        false,
        None,
        None, // [site] comments unset
    );
    let html = slots
        .get_html("after-article", "en/post.html")
        .expect("comment section should render for the English page");
    assert!(
        html.contains("aria-label=\"Available after publishing\""),
        "English page must get the English inactive label even on a zh-hans site; got: {html}"
    );
    assert!(
        !html.contains("发布后启用"),
        "English page must NOT get the Chinese inactive label; got: {html}"
    );
}

#[test]
fn subscribe_form_uses_per_page_lang_on_scoped_site() {
    // A scoped (multilingual) site with a Chinese default (site lang =
    // zh-hans) and an `en/` language tree: the auto-injected footer
    // subscribe form on the English page must render English copy
    // ("Subscribe"), not the site-default Chinese ("订阅").
    use crate::build::types::ParsedDocument;
    use crate::i18n::Language;
    use tempfile::TempDir;
    let test_tmp = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/test-tmp");
    std::fs::create_dir_all(test_tmp).unwrap();
    let temp_dir = TempDir::new_in(test_tmp).unwrap();
    let project_path = temp_dir.path().to_str().unwrap();
    let moss_dir = temp_dir.path().join(".moss");
    std::fs::create_dir_all(&moss_dir).unwrap();
    std::fs::write(
        moss_dir.join("config.toml"),
        &format!(
            "schema_version = {}\n\n[channels.email]\n",
            crate::config::migrations::CURRENT_VERSION
        ),
    )
    .unwrap();

    // Chinese root home + an English content tree under `en/` mints the
    // "en" scope → the per-page (scoped) form branch.
    let pages = vec![
        ParsedDocument {
            url_path: "index.html".to_string(),
            lang: Language::ZhHans,
            ..Default::default()
        },
        ParsedDocument {
            url_path: "en/post.html".to_string(),
            lang: Language::En,
            ..Default::default()
        },
    ];

    let mut config = ServicesConfig::default();
    config.email = Some(crate::config::services::EmailService {
        common: Default::default(),
        api_key: Some("btd-somekey".to_string()),
    });
    let deploy = DomainDeploymentConfig {
        site_id: Some("mysite".into()),
        deploy_method: Some("moss".into()),
        ..Default::default()
    };
    let slots = generate_native_slots(
        &config,
        project_path,
        email_installed(project_path),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &HashMap::new(),
        &pages,
        "zh-hans",
        None,
        Some(deploy),
        false,
        None,
        None, // [site] comments unset
    );

    let en_form = slots
        .get_html("footer-end", "en/post.html")
        .unwrap_or_default();
    assert!(
        en_form.contains(">Subscribe<") || en_form.contains("Subscribe"),
        "English page's subscribe form must render the English label; got: {en_form}"
    );
    assert!(
        !en_form.contains("订阅"),
        "English page's subscribe form must NOT render the Chinese label; got: {en_form}"
    );
}

#[test]
fn test_comment_form_active_with_endpoint() {
    let config = comments_fixture("https://comments.example.com");
    let article_map = make_article_map();
    let slots = generate_native_slots(
        &config,
        "/tmp/nonexistent",
        email_installed("/tmp/nonexistent"),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &article_map,
        &[],
        "en",
        None,
        None,
        false,
        None,
        None, // [site] comments unset
    );
    let html = slots.get_html("after-article", "posts/test.html");
    assert!(html.is_some());
    let html = html.unwrap();
    assert!(
        !html.contains("moss-service-inactive"),
        "Form should NOT have inactive class when endpoint exists"
    );
}

// Task 3 (v2): Comments form renders from an explicit `[services.comments]`
// section with no wiring — presence = enabled, empty server_url = inactive form.
#[test]
fn comments_form_renders_from_features_toggle_fallback() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let project_path = temp_dir.path().to_str().unwrap();
    let moss_dir = temp_dir.path().join(".moss");
    std::fs::create_dir_all(&moss_dir).unwrap();
    // Services v2: an empty [services.comments] is "enabled by presence"
    // but without wiring — so the form renders in inactive state.
    std::fs::write(
        moss_dir.join("config.toml"),
        "[services.comments]\nprovider = \"artalk\"\n",
    )
    .unwrap();

    // Services config with no comments section; builder logic reads
    // from config.toml via the comments fallback when services.comments is None.
    // Simulate that by building a ServicesConfig with no comments section and
    // relying on the moss-hosted toggle — but this test is self-hosted,
    // so instead we pass a comments fixture with no server_url.
    let config = comments_fixture("");
    let mut article_map = HashMap::new();
    article_map.insert(
        "test-uid".to_string(),
        ArticleInfo {
            url_path: "posts/test.html".to_string(),
            title: "Test Post".to_string(),
            uid: "test-uid".to_string(),
            comments: None,
            review_of: None,
            rating: None,
            cover: None,
            lang: None,
            syndicated: Vec::new(),
        },
    );

    let slots = generate_native_slots(
        &config,
        project_path,
        email_installed(project_path),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &article_map,
        &[],
        "en",
        None,
        None,
        false,
        None,
        None, // [site] comments unset
    );
    let html = slots.get_html("after-article", "posts/test.html");
    assert!(
        html.is_some(),
        "Comment form should render from [services.comments] presence"
    );
    let html = html.unwrap();
    assert!(
        html.contains("moss-service-inactive"),
        "Comment form should be inactive without server_url"
    );
}

/// Helper: services config with `[services.comments] enabled = false`.
/// Mirrors `comments_fixture` but with the disabled flag set.
fn comments_disabled_fixture() -> ServicesConfig {
    ServicesConfig {
        comments: Some(crate::config::services::CommentsService {
            common: crate::config::services::ServiceCommon {
                enabled: Some(false),
                provider: Some("artalk".into()),
            },
            server_url: None,
        }),
        ..Default::default()
    }
}

/// Regression: `[services.comments]` with `enabled = false` must NOT
/// render the comment section. The bug (pre-2026-04-30) was that the
/// explicit-section branch in `generate_native_slots` checked only
/// for section presence, not for `enabled = false`. The fallback path
/// (when services.comments is None) correctly consulted `is_enabled()`,
/// but the explicit-disabled path slipped through.
///
/// A real site's config.toml on 2026-04-30:
///   [services.comments]
///   enabled = false
/// produced `<section class="moss-comments">` markup with "0 comments"
/// visible. After this fix, no such markup is emitted.
#[test]
fn comments_explicit_disabled_does_not_render() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let project_path = temp_dir.path().to_str().unwrap();
    let moss_dir = temp_dir.path().join(".moss");
    std::fs::create_dir_all(&moss_dir).unwrap();
    std::fs::write(
        moss_dir.join("config.toml"),
        "[services.comments]\nenabled = false\nprovider = \"artalk\"\n",
    )
    .unwrap();

    let config = comments_disabled_fixture();
    let mut article_map = HashMap::new();
    article_map.insert(
        "test-uid".to_string(),
        ArticleInfo {
            url_path: "posts/test.html".to_string(),
            title: "Test Post".to_string(),
            uid: "test-uid".to_string(),
            comments: None,
            review_of: None,
            rating: None,
            cover: None,
            lang: None,
            syndicated: Vec::new(),
        },
    );

    let slots = generate_native_slots(
        &config,
        project_path,
        email_installed(project_path),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &article_map,
        &[],
        "en",
        None,
        None,
        false,
        None,
        None, // [site] comments unset
    );

    let after_article_html = slots.get_html("after-article", "posts/test.html");
    assert!(
        after_article_html.is_none()
            || !after_article_html
                .as_ref()
                .unwrap()
                .contains("moss-comments"),
        "no moss-comments section should render when enabled=false; got: {:?}",
        after_article_html
    );

    let head_end_html = slots.get_html("head-end", "posts/test.html");
    assert!(
        head_end_html.is_none()
            || !head_end_html
                .as_ref()
                .unwrap()
                .contains("moss-comments-style"),
        "no comments CSS should render when enabled=false; got: {:?}",
        head_end_html
    );
}

/// Moss-hosted + [channels.email] + no footer.md → auto-form lands in
/// `Slot::FooterEnd` (the trailing slot, post-design-B) rather than the
/// leading slot (`Slot::Footer`, public string `footer-left`), wired to
/// the seta subscribe endpoint. Companion:
/// `email_channel_without_site_id_injects_pending_form` covers the
/// pre-publish (no site_id) state of the same injection.
///
/// Without this test, a regression that drops the auto-injection block
/// in features.rs (or routes the form into the wrong slot) would not
/// be caught — the negative tests would pass either way.
#[test]
fn moss_hosted_email_channel_auto_injects_form_into_footer_end() {
    use crate::build::types::ParsedDocument;
    use tempfile::TempDir;
    let temp_dir = TempDir::new().unwrap();
    let project_path = temp_dir.path().to_str().unwrap();
    let moss_dir = temp_dir.path().join(".moss");
    std::fs::create_dir_all(&moss_dir).unwrap();
    std::fs::write(
        moss_dir.join("config.toml"),
        &format!(
            "schema_version = {}\n\n[channels.email]\n",
            crate::config::migrations::CURRENT_VERSION
        ),
    )
    .unwrap();
    // No footer.md at temp_dir.path() — auto-injection is gated on its
    // absence. If a previous test fixture leaks a footer.md into the
    // tempdir, the assertion below fails fast with a clear message.
    assert!(
        !temp_dir.path().join("footer.md").exists(),
        "tempdir should have no footer.md (auto-injection is gated on absence)"
    );

    // Per-page form emission (Task 1.7) requires at least one document
    // in the pages slice — the slot generator iterates pages to mint a
    // scope-tagged form per URL. Single-page fixture suffices here; the
    // scope-derivation behavior is covered by the dedicated tests
    // `moss_hosted_footer_form_*` above.
    let pages = vec![ParsedDocument {
        url_path: "index.html".to_string(),
        ..Default::default()
    }];

    let config = ServicesConfig::default();
    let deploy = moss_hosted_deploy_config("test-site");
    let slots = generate_native_slots(
        &config,
        project_path,
        email_installed(project_path),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &HashMap::new(),
        &pages,
        "en",
        None,
        Some(deploy),
        false, // build, not preview
        None,  // no media lookup in this test
        None, // [site] comments unset
    );

    let footer_end = slots.get_html("footer-end", "index.html");
    assert!(
        footer_end.is_some(),
        "auto-injected subscribe form must populate footer-end slot \
             on moss-hosted sites with [channels.email] and no footer.md"
    );
    let html = footer_end.unwrap();
    assert!(
        html.contains("class=\"moss-subscribe-form\"")
            && html.contains("data-position=\"inline\"")
            && html.contains("/sites/test-site/subscribe"),
        "footer-end content must be the moss-hosted subscribe form: {html}"
    );

    // Negative: nothing should land in footer-left from auto-injection
    // (footer.md is absent and we have no `slot: footer-left` page in
    // the fixture). If a regression routes the form back to `Slot::Footer`,
    // this assertion catches it.
    assert!(
        slots.get_html("footer-left", "index.html").is_none(),
        "footer-left must remain empty when only the auto-form is in play"
    );
}

/// [channels.email] on, no seta site_id, no footer.md → the seta form
/// injects in PENDING state (action="#", data-moss-pending-site) so
/// preview shows the real footer before first publish. Replaces the
/// pre-2026-06-10 behavior (NoProvider warning + silent dropout).
/// A leaked pending form is hidden from real readers by the email.css rule
/// keyed on `body` without `data-moss-preview` — see
/// `email_no_seta_auth_build`. There is no deploy-time rebuild guard.
#[test]
fn email_channel_without_site_id_injects_pending_form() {
    use crate::build::types::ParsedDocument;
    use tempfile::TempDir;
    let test_tmp = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/test-tmp");
    std::fs::create_dir_all(test_tmp).unwrap();
    let temp_dir = TempDir::new_in(test_tmp).unwrap();
    let project_path = temp_dir.path().to_str().unwrap();
    let moss_dir = temp_dir.path().join(".moss");
    std::fs::create_dir_all(&moss_dir).unwrap();
    std::fs::write(
        moss_dir.join("config.toml"),
        &format!(
            "schema_version = {}\n\n[channels.email]\n",
            crate::config::migrations::CURRENT_VERSION
        ),
    )
    .unwrap();

    let pages = vec![ParsedDocument {
        url_path: "index.html".to_string(),
        ..Default::default()
    }];

    // No services.email at all and no deploy config → no site_id.
    let config = ServicesConfig::default();
    let slots = generate_native_slots(
        &config,
        project_path,
        email_installed(project_path),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &HashMap::new(),
        &pages,
        "en",
        None,
        None,
        false,
        None,
        None, // [site] comments unset
    );

    let html = slots.get_html("footer-end", "index.html").expect(
        "pending subscribe form must populate footer-end when email is on and site_id is absent",
    );
    assert!(
        html.contains("class=\"moss-subscribe-form\"")
            && html.contains("data-position=\"inline\"")
            && html.contains("data-moss-pending-site=\"true\"")
            && html.contains("action=\"#\""),
        "footer-end must carry the pending-variant hosted form: {html}"
    );
}

/// Single-scope sites emit the footer form as STATIC slot content so
/// SYNTHESIZED pages — the auto-generated homepage of a site without
/// an index.md/self-named home note, folder listing pages — get the
/// form too. A PerPage map keyed on parsed url_paths misses them
/// (caught during Phase 1 visual verification: the built homepage of
/// a home-note-less site had no form while every parsed page did,
/// which also made the deploy guard force a rebuild on every publish
/// for such sites). Scoped sites still use PerPage — their
/// synthesized-page gap is a documented Follow-up.
#[test]
fn single_scope_form_covers_synthesized_pages() {
    use crate::build::types::ParsedDocument;
    use tempfile::TempDir;
    let test_tmp = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/test-tmp");
    std::fs::create_dir_all(test_tmp).unwrap();
    let temp_dir = TempDir::new_in(test_tmp).unwrap();
    let project_path = temp_dir.path().to_str().unwrap();
    let moss_dir = temp_dir.path().join(".moss");
    std::fs::create_dir_all(&moss_dir).unwrap();
    std::fs::write(
        moss_dir.join("config.toml"),
        &format!(
            "schema_version = {}\n\n[channels.email]\n",
            crate::config::migrations::CURRENT_VERSION
        ),
    )
    .unwrap();

    // Only a non-root page is parsed; the homepage is synthesized and
    // therefore absent from `pages`.
    let pages = vec![ParsedDocument {
        url_path: "hello/index.html".to_string(),
        ..Default::default()
    }];
    let slots = generate_native_slots(
        &ServicesConfig::default(),
        project_path,
        email_installed(project_path),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &HashMap::new(),
        &pages,
        "en",
        None,
        None,
        false,
        None,
        None, // [site] comments unset
    );

    // The synthesized homepage (NOT in `pages`) must still get the form.
    let html = slots
        .get_html("footer-end", "index.html")
        .expect("single-scope form must be Static so synthesized pages get it");
    assert!(
        html.contains("data-moss-pending-site=\"true\""),
        "synthesized homepage must carry the (pending) subscribe form: {html}"
    );
}

/// `site_id = ""` in state must render the PENDING form, never a
/// "wired" form with action `/sites//subscribe`. Re-establishes
/// the invariant the deleted `get_email_policy_warnings` command held
/// (it defended against an empty site_id in state.toml).
#[test]
fn empty_site_id_renders_pending_form() {
    use crate::build::types::ParsedDocument;
    use tempfile::TempDir;
    let test_tmp = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/test-tmp");
    std::fs::create_dir_all(test_tmp).unwrap();
    let temp_dir = TempDir::new_in(test_tmp).unwrap();
    let project_path = temp_dir.path().to_str().unwrap();
    let moss_dir = temp_dir.path().join(".moss");
    std::fs::create_dir_all(&moss_dir).unwrap();
    std::fs::write(
        moss_dir.join("config.toml"),
        &format!(
            "schema_version = {}\n\n[channels.email]\n",
            crate::config::migrations::CURRENT_VERSION
        ),
    )
    .unwrap();

    let pages = vec![ParsedDocument {
        url_path: "index.html".to_string(),
        ..Default::default()
    }];

    let deploy = DomainDeploymentConfig {
        site_id: Some(String::new()),
        ..Default::default()
    };
    let slots = generate_native_slots(
        &ServicesConfig::default(),
        project_path,
        email_installed(project_path),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &HashMap::new(),
        &pages,
        "en",
        None,
        Some(deploy),
        false,
        None,
        None, // [site] comments unset
    );

    let html = slots
        .get_html("footer-end", "index.html")
        .expect("subscribe form must still populate footer-end with an empty site_id");
    assert!(
        html.contains("data-moss-pending-site=\"true\"") && html.contains("action=\"#\""),
        "empty site_id must render the PENDING form, not wired-to-nothing: {html}"
    );
}

/// footer.md suppresses the pending form exactly like the wired form —
/// one shared gate (`email_installed && !project_has_footer_file`).
#[test]
fn pending_form_not_injected_when_footer_md_present() {
    use crate::build::types::ParsedDocument;
    use tempfile::TempDir;
    let test_tmp = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/test-tmp");
    std::fs::create_dir_all(test_tmp).unwrap();
    let temp_dir = TempDir::new_in(test_tmp).unwrap();
    let project_path = temp_dir.path().to_str().unwrap();
    let moss_dir = temp_dir.path().join(".moss");
    std::fs::create_dir_all(&moss_dir).unwrap();
    std::fs::write(
        moss_dir.join("config.toml"),
        &format!(
            "schema_version = {}\n\n[channels.email]\n",
            crate::config::migrations::CURRENT_VERSION
        ),
    )
    .unwrap();
    std::fs::write(temp_dir.path().join("footer.md"), "my footer\n").unwrap();

    let pages = vec![ParsedDocument {
        url_path: "index.html".to_string(),
        ..Default::default()
    }];
    let slots = generate_native_slots(
        &ServicesConfig::default(),
        project_path,
        email_installed(project_path),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &HashMap::new(),
        &pages,
        "en",
        None,
        None,
        false,
        None,
        None, // [site] comments unset
    );
    assert!(
        slots.get_html("footer-end", "index.html").is_none(),
        "footer.md must suppress the pending auto-form"
    );
}

/// Email slots must be byte-identical whether or not this build starts
/// the preview server. Guards against reintroducing the start_server-flag
/// flip-flop bug (2026-06-10 root cause: folder-open build passed
/// start_server=true, watch rebuilds start_server=false, and the footer
/// toggled on disk between the two).
///
/// The wired case deliberately keeps `deploy_method: None`: the subscribe
/// form gates on `site_id` alone, while the moss-hosted analytics beacon
/// and pageview beacon are handled by separate tests
/// (`beacon_present_in_both_build_modes_for_moss_hosted`,
/// `head_end_byte_identical_across_build_modes`). This test owns the
/// EMAIL invariant only.
#[test]
fn email_slots_identical_across_build_modes() {
    use crate::build::types::ParsedDocument;
    use tempfile::TempDir;
    let test_tmp = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/test-tmp");
    std::fs::create_dir_all(test_tmp).unwrap();
    let temp_dir = TempDir::new_in(test_tmp).unwrap();
    let project_path = temp_dir.path().to_str().unwrap();
    let moss_dir = temp_dir.path().join(".moss");
    std::fs::create_dir_all(&moss_dir).unwrap();
    std::fs::write(
        moss_dir.join("config.toml"),
        &format!(
            "schema_version = {}\n\n[channels.email]\n",
            crate::config::migrations::CURRENT_VERSION
        ),
    )
    .unwrap();
    let pages = vec![ParsedDocument {
        url_path: "index.html".to_string(),
        ..Default::default()
    }];

    // Both wiring states: published (site_id) and pre-publish (None).
    let wired = DomainDeploymentConfig {
        site_id: Some("test-site".to_string()),
        deploy_method: None,
        ..Default::default()
    };
    let deploy_configs = [Some(wired), None];
    for deploy in deploy_configs {
        let per_mode: Vec<_> = [true, false]
            .iter()
            .map(|&serving| {
                let slots = generate_native_slots(
                    &ServicesConfig::default(),
                    project_path,
                    email_installed(project_path),
                    crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
                    &HashMap::new(),
                    &pages,
                    "en",
                    None,
                    deploy.clone(),
                    serving,
                    None,
                    None, // [site] comments unset
                );
                (
                    slots.get_html("footer-end", "index.html"),
                    slots.get_html("head-end", "index.html"),
                )
            })
            .collect();
        assert_eq!(
            per_mode[0],
            per_mode[1],
            "email slot output must not depend on the start-server flag (deploy={:?})",
            deploy.is_some()
        );
    }
}

/// Robustness: seta form injects whenever site_id is present, even when
/// deploy_method is absent or stale. This covers a real site's class of
/// bug where state.toml was never written with deploy_method but site_id
/// was present from a previous publish.
#[test]
fn seta_form_injects_with_site_id_but_no_deploy_method() {
    use crate::build::types::ParsedDocument;
    use tempfile::TempDir;
    let test_tmp = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/test-tmp");
    std::fs::create_dir_all(test_tmp).unwrap();
    let temp_dir = TempDir::new_in(test_tmp).unwrap();
    let project_path = temp_dir.path().to_str().unwrap();
    let moss_dir = temp_dir.path().join(".moss");
    std::fs::create_dir_all(&moss_dir).unwrap();
    std::fs::write(
        moss_dir.join("config.toml"),
        &format!(
            "schema_version = {}\n\n[channels.email]\n",
            crate::config::migrations::CURRENT_VERSION
        ),
    )
    .unwrap();

    let pages = vec![ParsedDocument {
        url_path: "index.html".to_string(),
        ..Default::default()
    }];

    let config = ServicesConfig::default();
    // site_id present but deploy_method deliberately absent — simulates
    // a site whose state.toml was written before deploy_method was added
    // or whose state.toml is missing entirely.
    let deploy = DomainDeploymentConfig {
        site_id: Some("my-lab".into()),
        deploy_method: None,
        ..Default::default()
    };
    let slots = generate_native_slots(
        &config,
        project_path,
        email_installed(project_path),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &HashMap::new(),
        &pages,
        "en",
        None,
        Some(deploy),
        false,
        None,
        None, // [site] comments unset
    );

    let footer_end = slots.get_html("footer-end", "index.html");
    assert!(
        footer_end.is_some(),
        "seta form must inject when site_id is present even without deploy_method"
    );
    let html = footer_end.unwrap();
    assert!(
        html.contains("/sites/my-lab/subscribe"),
        "form must POST to the seta endpoint for the site: {html}"
    );
}

/// site_id present → wired seta form, regardless of leftover third-party
/// email config: a site with both a seta site_id and stale `[services.email]`
/// content from before the 2026-06-10 redesign still gets the wired seta
/// form, and no third-party form ever injects alongside it.
#[test]
fn wired_form_renders_when_site_id_present_regardless_of_provider_config() {
    use crate::build::types::ParsedDocument;
    use tempfile::TempDir;
    let test_tmp = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/test-tmp");
    std::fs::create_dir_all(test_tmp).unwrap();
    let temp_dir = TempDir::new_in(test_tmp).unwrap();
    let project_path = temp_dir.path().to_str().unwrap();
    let moss_dir = temp_dir.path().join(".moss");
    std::fs::create_dir_all(&moss_dir).unwrap();
    std::fs::write(
        moss_dir.join("config.toml"),
        &format!(
            "schema_version = {}\n\n[channels.email]\n",
            crate::config::migrations::CURRENT_VERSION
        ),
    )
    .unwrap();

    let pages = vec![ParsedDocument {
        url_path: "index.html".to_string(),
        ..Default::default()
    }];

    // Both site_id AND leftover 3rd-party provider config present —
    // the wired seta form renders.
    let mut config = ServicesConfig::default();
    config.email = Some(crate::config::services::EmailService {
        common: Default::default(),
        api_key: Some("btd-somekey".to_string()),
    });
    let deploy = DomainDeploymentConfig {
        site_id: Some("mysite".into()),
        deploy_method: Some("moss".into()),
        ..Default::default()
    };
    let slots = generate_native_slots(
        &config,
        project_path,
        email_installed(project_path),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &HashMap::new(),
        &pages,
        "en",
        None,
        Some(deploy),
        false,
        None,
        None, // [site] comments unset
    );

    let footer_end = slots
        .get_html("footer-end", "index.html")
        .unwrap_or_default();
    assert!(
        footer_end.contains("/sites/mysite/subscribe"),
        "seta form must inject when site_id is present: {footer_end}"
    );
    assert!(
        !footer_end.contains("buttondown.com"),
        "Buttondown form must not inject when site_id is present: {footer_end}"
    );
}

/// Task 1.7: the moss-hosted footer form is emitted PER PAGE with a
/// scope derived from the page's url_path. Bilingual sites get
/// different scopes for `/en/...` and `/zh/...`; single-lang sites
/// (no scope folders) get scope="" for every page.
#[test]
fn moss_hosted_footer_form_scope_matches_page_top_folder() {
    use crate::build::types::ParsedDocument;
    use tempfile::TempDir;
    let temp_dir = TempDir::new().unwrap();
    let project_path = temp_dir.path().to_str().unwrap();
    let moss_dir = temp_dir.path().join(".moss");
    std::fs::create_dir_all(&moss_dir).unwrap();
    std::fs::write(
        moss_dir.join("config.toml"),
        &format!(
            "schema_version = {}\n\n[channels.email]\n",
            crate::config::migrations::CURRENT_VERSION
        ),
    )
    .unwrap();

    // Bilingual site: top-level /en/index.html and /zh/index.html mint
    // both "en" and "zh" as supported scopes. Posts under each pick up
    // the matching scope; a page outside both buckets gets "".
    let pages = vec![
        ParsedDocument {
            url_path: "en/index.html".to_string(),
            ..Default::default()
        },
        ParsedDocument {
            url_path: "zh/index.html".to_string(),
            ..Default::default()
        },
        ParsedDocument {
            url_path: "en/posts/hello.html".to_string(),
            ..Default::default()
        },
        ParsedDocument {
            url_path: "zh/posts/ni-hao.html".to_string(),
            ..Default::default()
        },
        // Top-level non-language folder must NOT mint a scope:
        // /about/index.html exists but "about" is not in the ISO
        // allowlist, so the page below ends up with scope="".
        ParsedDocument {
            url_path: "about/index.html".to_string(),
            ..Default::default()
        },
    ];

    let config = ServicesConfig::default();
    let deploy = moss_hosted_deploy_config("test-site");
    let slots = generate_native_slots(
        &config,
        project_path,
        email_installed(project_path),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &HashMap::new(),
        &pages,
        "en",
        None,
        Some(deploy),
        false,
        None,
        None, // [site] comments unset
    );

    let en_post = slots
        .get_html("footer-end", "en/posts/hello.html")
        .expect("footer form must populate /en/ post");
    assert!(
        en_post.contains(r#"name="scope" value="en""#),
        "/en/posts/hello.html must get scope=en, got: {en_post}"
    );

    let zh_post = slots
        .get_html("footer-end", "zh/posts/ni-hao.html")
        .expect("footer form must populate /zh/ post");
    assert!(
        zh_post.contains(r#"name="scope" value="zh""#),
        "/zh/posts/ni-hao.html must get scope=zh, got: {zh_post}"
    );

    // /about/index.html: "about" is NOT in the ISO-639 allowlist, so
    // it does NOT become a supported_scope. The about page gets scope="".
    let about = slots
        .get_html("footer-end", "about/index.html")
        .expect("footer form must populate /about/ page");
    assert!(
        about.contains(r#"name="scope" value="""#),
        "/about/index.html must get empty scope (about is not a language), got: {about}"
    );
}

/// Single-lang site (no language top-folders): every page gets
/// scope="". This is the legacy v0 behavior — no audience
/// segmentation.
#[test]
fn moss_hosted_footer_form_single_lang_site_emits_empty_scope() {
    use crate::build::types::ParsedDocument;
    use tempfile::TempDir;
    let temp_dir = TempDir::new().unwrap();
    let project_path = temp_dir.path().to_str().unwrap();
    let moss_dir = temp_dir.path().join(".moss");
    std::fs::create_dir_all(&moss_dir).unwrap();
    std::fs::write(
        moss_dir.join("config.toml"),
        &format!(
            "schema_version = {}\n\n[channels.email]\n",
            crate::config::migrations::CURRENT_VERSION
        ),
    )
    .unwrap();

    // No /en/ or /zh/ folder — only /posts/ (which is NOT a language).
    let pages = vec![
        ParsedDocument {
            url_path: "index.html".to_string(),
            ..Default::default()
        },
        ParsedDocument {
            url_path: "posts/hello.html".to_string(),
            ..Default::default()
        },
    ];

    let config = ServicesConfig::default();
    let deploy = moss_hosted_deploy_config("test-site");
    let slots = generate_native_slots(
        &config,
        project_path,
        email_installed(project_path),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &HashMap::new(),
        &pages,
        "en",
        None,
        Some(deploy),
        false,
        None,
        None, // [site] comments unset
    );

    let root = slots
        .get_html("footer-end", "index.html")
        .expect("footer form must populate root index");
    assert!(
        root.contains(r#"name="scope" value="""#),
        "single-lang site root must get empty scope, got: {root}"
    );
    let post = slots
        .get_html("footer-end", "posts/hello.html")
        .expect("footer form must populate post");
    assert!(
        post.contains(r#"name="scope" value="""#),
        "single-lang site post must get empty scope, got: {post}"
    );
}

/// Part D parity lock (2026-05-22 policy): moss-hosted sites must
/// auto-inject the seta subscribe form purely from `[channels.email]` —
/// no `[services.email]` api_key check, no provider-fetch check. Seta
/// owns the audience.
///
/// Companion to `moss_hosted_email_channel_auto_injects_form_into_footer_end`:
/// that test verifies the form lands in the right slot; THIS test names
/// the explicit "no api_key required" parity that a future refactor
/// must preserve.
#[test]
fn moss_hosted_email_channel_injects_form_without_api_key() {
    use crate::build::types::ParsedDocument;
    use tempfile::TempDir;
    let temp_dir = TempDir::new().unwrap();
    let project_path = temp_dir.path().to_str().unwrap();
    let moss_dir = temp_dir.path().join(".moss");
    std::fs::create_dir_all(&moss_dir).unwrap();
    std::fs::write(
        moss_dir.join("config.toml"),
        &format!(
            "schema_version = {}\n\n[channels.email]\n",
            crate::config::migrations::CURRENT_VERSION
        ),
    )
    .unwrap();

    // No [services.email] section at all — pure-default ServicesConfig.
    // This is the Yi-shape config on moss-hosted: channel presence plus
    // a site_id must light up the wired form on their own.
    let config = ServicesConfig::default();
    assert!(
        config.email.is_none(),
        "fixture sanity: no [services.email]"
    );

    // Per-page form emission (Task 1.7) needs at least one page.
    let pages = vec![ParsedDocument {
        url_path: "index.html".to_string(),
        ..Default::default()
    }];

    let deploy = moss_hosted_deploy_config("yi-moss");
    let slots = generate_native_slots(
        &config,
        project_path,
        email_installed(project_path),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &HashMap::new(),
        &pages,
        "en",
        None,
        Some(deploy),
        false,
        None,
        None, // [site] comments unset
    );

    // Form must land — moss-hosted policy says channel-presence alone
    // is enough.
    let footer_end = slots.get_html("footer-end", "index.html");
    assert!(
        footer_end.is_some(),
        "moss-hosted + [channels.email] without api_key must auto-inject \
             the seta subscribe form (Part D parity lock for 2026-05-22 policy)"
    );
    let html = footer_end.unwrap();
    assert!(
        html.contains("/sites/yi-moss/subscribe"),
        "moss-hosted form must POST to the seta endpoint, got: {html}"
    );
    assert!(
        !html.contains("buttondown.com"),
        "moss-hosted branch must not touch Buttondown, got: {html}"
    );
}

// Subscribe CSS+JS bundle: injected whenever [channels.email] is
// installed, regardless of wiring state, so the footer form (wired or
// pending) and inline :::subscribe shortcodes are always styled.
// The form itself in the no-site_id state is covered by
// `email_channel_without_site_id_injects_pending_form`.
#[test]
fn email_channel_install_injects_subscribe_assets() {
    use tempfile::TempDir;
    let test_tmp = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/test-tmp");
    std::fs::create_dir_all(test_tmp).unwrap();
    let temp_dir = TempDir::new_in(test_tmp).unwrap();
    let project_path = temp_dir.path().to_str().unwrap();
    let moss_dir = temp_dir.path().join(".moss");
    std::fs::create_dir_all(&moss_dir).unwrap();
    std::fs::write(
        moss_dir.join("config.toml"),
        &format!(
            "schema_version = {}\n\n[channels.email]\n",
            crate::config::migrations::CURRENT_VERSION
        ),
    )
    .unwrap();

    let config = ServicesConfig::default();
    let slots = generate_native_slots(
        &config,
        project_path,
        email_installed(project_path),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &HashMap::new(),
        &[],
        "en",
        None,
        None,
        false,
        None,
        None, // [site] comments unset
    );

    let head_end = slots.get_html("head-end", "index.html").unwrap_or_default();
    // A <link> to the emitted file, not an inline <style> — see
    // `build::emit::feature_styles`. Asserting on the emitted URL rather than
    // the old `moss-email-style` class keeps the test on the contract that
    // matters: the page references the sheet.
    assert!(
        head_end.contains("/_moss/css/email."),
        "email CSS must inject when [channels.email] is installed: {head_end}"
    );
    assert!(
        slots.feature_styles().contains(&"email"),
        "linking email.css must also register it for emission, or the link 404s"
    );
}

/// Regression guard: email.css is injected inside `@layer
/// plugins.shortcodes`, which OUTRANKS site.css's `shortcodes` layer
/// regardless of selector specificity. Its ≤32rem stacking rules force a
/// `.moss-btn` width (100% in-page, auto in footer); if those rules also
/// apply in the success state they beat site.css's `width: 2.25rem`
/// circle and the checkmark button renders as a full-width oval.
#[test]
fn email_css_mobile_btn_width_rules_skip_success_state() {
    let css = include_str!("../assets/css/email.css");
    assert!(
        css.contains(r#"[data-position="inline"]:not([data-state="success"]) .moss-btn"#),
        "mobile in-page .moss-btn width rule must exclude the success state"
    );
    assert!(
        !css.contains(r#"[data-position="inline"] .moss-btn"#),
        "an unscoped `[data-position=\"inline\"] .moss-btn` rule in email.css \
             overrides the success circle through the plugins layer — scope it \
             with :not([data-state=\"success\"])"
    );
}

/// Regression guard for the stacked (≤32rem, in-page) success layout:
/// site.css absolutely positions `.moss-subscribe-status` over the WHOLE
/// form (`inset: 0`), which is right for the single-row desktop form but
/// centers the copy across both stacked rows on mobile — the text lands
/// on top of the success circle. email.css must re-anchor the status to
/// the input row for the stacked card, and restore full-form overlay for
/// the footer form (which stays single-row on mobile).
#[test]
fn email_css_mobile_status_anchors_to_input_row() {
    let css = include_str!("../assets/css/email.css");
    assert!(
        css.contains(r#"[data-position="inline"] .moss-subscribe-status"#),
        "email.css must re-anchor the stacked form's status to the input row"
    );
    assert!(
            css.contains(r#"footer .moss-subscribe .moss-subscribe-form[data-position="inline"] .moss-subscribe-status"#),
            "footer forms stay single-row on mobile — email.css must restore \
             their full-form status overlay"
        );
}

/// Slot-merge contract test: the review-colophon path in
/// `generate_native_slots` honors a non-`None` `media_lookup` and
/// routes the cover image through the synthesizer. Arch review of
/// the 2026-05-16 colophon migration asked for this — the unit test
/// in `review::tests` covers the `render_colophon` shape, but only
/// this fixture exercises the slot-merge seam: native-slots gate
/// trips → article-map article carries `review_of` + `cover` →
/// review.json carries a matching uid → `render_colophon` receives
/// the lookup and emits `<picture>` for covers with a
/// manifest-confirmed WebP variant. A regression that reverts the
/// `media_lookup` argument to `None` at the call site would slip
/// past the unit test silently; this test fails fast.
///
/// Not a full end-to-end test (does NOT exercise the staging slot
/// injection pass, the `article-map.json` disk write, or
/// `MediaDimensionLookup` construction at `run_pipeline` time).
/// The "slot_merge" name reflects the actual seam under test.
#[test]
fn colophon_slot_merge_routes_cover_through_synthesizer_when_lookup_provided() {
    use crate::build::media::dimensions::MediaDimensionLookup;
    use crate::types::content::MediaMetadata;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let project_path = temp_dir.path().to_str().unwrap();
    let moss_dir = temp_dir.path().join(".moss");
    std::fs::create_dir_all(&moss_dir).unwrap();

    // [services.comments] presence trips the native-slots gate so the
    // colophon emission path runs.
    std::fs::write(
        moss_dir.join("config.toml"),
        "[services.comments]\nprovider = \"artalk\"\n",
    )
    .unwrap();

    // Persist a review.json keyed by the article's uid so
    // `load_review_data` returns Some(_).
    let social_dir = moss_dir.join("data").join("social");
    std::fs::create_dir_all(&social_dir).unwrap();
    let review_item = crate::build::features::review::ReviewItem {
        source_url: "https://neodb.social/book/sample".into(),
        source: "neodb".into(),
        category: Some("book".into()),
        title: "Sample Title".into(),
        subtitle: None,
        creator: Some(vec!["An Author".into()]),
        year: Some(2024),
        publisher: None,
        pages: None,
        isbn: None,
        community_rating: None,
        community_rating_count: None,
        writer_rating: None,
        external_urls: None,
        fetched_at: Some("2026-05-17T00:00:00Z".into()),
    };
    let review_data = crate::build::features::review::ReviewData {
        schema_version: "2.0.0".into(),
        updated_at: Some("2026-05-17T00:00:00Z".into()),
        articles: HashMap::from([("uid-review-1".to_string(), review_item)]),
    };
    let review_json = serde_json::to_string_pretty(&review_data).unwrap();
    std::fs::write(social_dir.join("review.json"), &review_json).unwrap();

    // Article that points at the review and carries a manifested cover.
    // `cover` is what `info.cover` returns — already resolved through
    // dir_overrides at article-map build time.
    let mut article_map = HashMap::new();
    article_map.insert(
        "uid-review-1".to_string(),
        ArticleInfo {
            url_path: "posts/review.html".to_string(),
            title: "My Review".to_string(),
            uid: "uid-review-1".to_string(),
            review_of: Some("https://neodb.social/book/sample".to_string()),
            rating: Some(4),
            cover: Some("image/cover.jpg".to_string()),
            comments: None,
            lang: None,
            syndicated: Vec::new(),
        },
    );

    // Manifest carrying a WebP variant for the cover; without dir_overrides
    // since the cover string is already in resolved form.
    let images = vec![MediaMetadata {
        is_animated: false,
        path: "image/cover.jpg".to_string(),
        file_type: "jpg".to_string(),
        size: 50_000,
        modified: None,
        dimensions: Some((1200, 800)),
        dominant_color: None,
        lqip_data_uri: None,
    }];
    let lookup = MediaDimensionLookup::new(&images, &[], &std::collections::HashMap::new(), None);

    let config = comments_fixture("https://example.com/comments");
    let slots = generate_native_slots(
        &config,
        project_path,
        email_installed(project_path),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &article_map,
        &[],
        "en",
        None,
        None,
        false,
        Some(&lookup),
        None, // [site] comments unset
    );

    let after_title = slots
        .get_html("after-title", "posts/review.html")
        .expect("colophon slot must populate for the review article");
    assert!(
        after_title.contains("<picture>"),
        "manifest-confirmed WebP variant must produce <picture> wrap, got: {}",
        after_title
    );
    // 1200px-wide fixture → srcset ladder (responsive-image-variants
    // Task 3); rung URLs inherit the leading-slash-prefixed form.
    assert!(
            after_title.contains(
                r#"<source srcset="/image/cover.w800.webp 800w, /image/cover.webp 1200w" type="image/webp" sizes="auto, (min-width: 48rem) 24rem, 100vw">"#
            ),
            "<source> must use the leading-slash-prefixed absolute URL, got: {}",
            after_title
        );
    assert!(
        after_title.contains(r#"class="review-colophon-cover""#),
        "structural CSS hook must survive the synthesizer, got: {}",
        after_title
    );
    assert!(
        after_title.contains(r#"width="1200""#),
        "manifest dims must reach the inner <img>, got: {}",
        after_title
    );
}

/// Companion to the e2e test above — when `media_lookup` is `None`, the
/// colophon path emits the synthesizer's bare-img fragment (still with
/// the structural class). Guards against an accidental flip that would
/// have callers pass `None` and silently lose the class identifier.
#[test]
fn colophon_slot_merge_emits_class_even_without_lookup() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let project_path = temp_dir.path().to_str().unwrap();
    let moss_dir = temp_dir.path().join(".moss");
    std::fs::create_dir_all(&moss_dir).unwrap();
    std::fs::write(
        moss_dir.join("config.toml"),
        "[services.comments]\nprovider = \"artalk\"\n",
    )
    .unwrap();
    let social_dir = moss_dir.join("data").join("social");
    std::fs::create_dir_all(&social_dir).unwrap();
    let review_item = crate::build::features::review::ReviewItem {
        source_url: "https://neodb.social/book/sample".into(),
        source: "neodb".into(),
        category: Some("book".into()),
        title: "Sample Title".into(),
        subtitle: None,
        creator: None,
        year: None,
        publisher: None,
        pages: None,
        isbn: None,
        community_rating: None,
        community_rating_count: None,
        writer_rating: None,
        external_urls: None,
        fetched_at: Some("2026-05-17T00:00:00Z".into()),
    };
    let review_data = crate::build::features::review::ReviewData {
        schema_version: "2.0.0".into(),
        updated_at: Some("2026-05-17T00:00:00Z".into()),
        articles: HashMap::from([("uid-r".to_string(), review_item)]),
    };
    std::fs::write(
        social_dir.join("review.json"),
        serde_json::to_string_pretty(&review_data).unwrap(),
    )
    .unwrap();
    let mut article_map = HashMap::new();
    article_map.insert(
        "uid-r".to_string(),
        ArticleInfo {
            url_path: "posts/r.html".to_string(),
            title: "T".to_string(),
            uid: "uid-r".to_string(),
            review_of: Some("https://neodb.social/book/sample".to_string()),
            rating: Some(3),
            cover: Some("image/cover.jpg".to_string()),
            comments: None,
            lang: None,
            syndicated: Vec::new(),
        },
    );

    let config = comments_fixture("https://example.com/comments");
    let slots = generate_native_slots(
        &config,
        project_path,
        email_installed(project_path),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &article_map,
        &[],
        "en",
        None,
        None,
        false,
        None, // no manifest → empty AssetSnapshot; dims fall back to 800×600
        None, // [site] comments unset
    );
    let after_title = slots
        .get_html("after-title", "posts/r.html")
        .expect("colophon slot must populate even without manifest");
    assert!(
        after_title.contains(r#"class="review-colophon-cover""#),
        "class identifier survives the no-manifest fallback, got: {}",
        after_title
    );
    // Phase 1 B1 (2026-05-25): the synthesizer always wraps raster
    // originals in `<picture>` regardless of manifest presence per
    // ADR-013's "honest mirror" invariant — the preview server fills
    // in placeholder bytes when the variant hasn't landed yet. The
    // prior "no manifest → no `<picture>`" expectation was a quirk of
    // the old MediaDimensionLookup-gated emission. Falls back to
    // 800×600 dims when the snapshot has no entry for the path.
    assert!(
        after_title.contains("<picture>"),
        "raster cover always wraps in <picture> post-B1; got: {}",
        after_title
    );
    assert!(
        after_title.contains(r#"width="800" height="600""#),
        "no-manifest dim fallback should land 800×600; got: {}",
        after_title
    );
}

// ===== should_inject_subscribe_assets helper =====
//
// The helper is the gate for the moss-hosted subscribe JS+CSS bundle.
// Either trigger (email channel install OR any page using :::subscribe)
// must cause injection; the slot-merge key dedups when both fire.

use crate::build::types::{PageFeatures, ParsedDocument};

fn page_with_features(features: PageFeatures) -> ParsedDocument {
    ParsedDocument {
        features,
        ..Default::default()
    }
}

#[test]
fn test_should_inject_neither_trigger_present() {
    let pages = vec![page_with_features(PageFeatures::default())];
    assert!(!should_inject_subscribe_assets(&pages, false));
}

#[test]
fn test_should_inject_email_channel_alone() {
    let pages = vec![page_with_features(PageFeatures::default())];
    assert!(should_inject_subscribe_assets(&pages, true));
}

#[test]
fn test_should_inject_inline_subscribe_alone() {
    let pages = vec![page_with_features(PageFeatures {
        inline_subscribe: true,
        inline_apply: false,
        ..Default::default()
    })];
    assert!(
            should_inject_subscribe_assets(&pages, false),
            "inline shortcode without channel must still trigger injection (regression for moss-releases)"
        );
}

#[test]
fn test_should_inject_both_triggers() {
    let pages = vec![page_with_features(PageFeatures {
        inline_subscribe: true,
        inline_apply: false,
        ..Default::default()
    })];
    assert!(should_inject_subscribe_assets(&pages, true));
}

#[test]
fn test_should_inject_inline_apply_alone() {
    let pages = vec![page_with_features(PageFeatures {
        inline_subscribe: false,
        inline_apply: true,
        ..Default::default()
    })];
    assert!(
        should_inject_subscribe_assets(&pages, false),
        "inline apply shortcode must trigger subscribe-asset injection"
    );
}

/// Regression: a page slice with ONLY `:::apply` (no subscribe shortcode,
/// no email channel, no comments, no analytics, not moss-hosted) must still
/// produce a true `should_inject_subscribe_assets` result.
///
/// If this returns false, `collect_native_slots_for_documents` hits its
/// early-return and emits empty slots — the apply form renders without the
/// subscribe CSS + JS that hydrate its submit button and state machine.
/// This test pins the fix that added `has_inline_apply` to both the
/// `should_inject_subscribe_assets` gate (features.rs:54) and the
/// `collect_native_slots_for_documents` early-return guard (build.rs:526).
#[test]
fn apply_only_page_slice_does_not_hit_early_return() {
    let pages = vec![page_with_features(PageFeatures {
        inline_subscribe: false,
        inline_apply: true,
        ..Default::default()
    })];
    // No email channel, no comments, not moss-hosted — all other conditions false.
    assert!(
        should_inject_subscribe_assets(&pages, false),
        "an apply-only page slice must trigger asset injection — \
             without this the subscribe CSS/JS bundle is not emitted and \
             the apply form submit button never gets hydrated"
    );
}

// The `project_has_inline_subscribe` filesystem-scan tests were deleted
// along with the function in PR7b (moss#599). Detection now reads
// `ParsedDocument.features.inline_subscribe` directly from the parsed
// page slice produced by `pipeline::run`; `should_inject_subscribe_assets`
// (above) tests cover the new path.

// ── mode-independence regression tests ─────────────────────────────────
//
// Invariant: the slot output that becomes the deploy artifact must be
// mode-independent (same config → same bytes regardless of start_server).
// Preview safety is the runtime layer: the beacon self-gates on
// `data-moss-preview` (Task A4); analytics is stripped from served
// preview responses by the preview server (Task A5). Subscribe forms
// have always been mode-independent (see email_slots_identical_across_
// build_modes). These tests pin the oscillation fix: injected in both
// modes, deterministic on every watch-rebuild.

/// Analytics must be injected REGARDLESS of start_server — the published
/// artifact must be mode-independent (preview safety is handled at serve
/// time, not by omitting the tag). Pins the oscillation fix.
#[test]
fn analytics_present_in_both_build_modes() {
    let config = analytics_fixture("https://mysite.goatcounter.com/count");
    for start_server in [true, false] {
        let slots = generate_native_slots(
            &config,
            "/tmp/nonexistent",
            email_installed("/tmp/nonexistent"),
            crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
            &HashMap::new(),
            &[],
            "en",
            None,
            None,
            start_server,
            None,
            None, // [site] comments unset
        );
        let head_end = slots.get_html("head-end", "index.html").unwrap_or_default();
        assert!(
            head_end.contains("goatcounter"),
            "analytics must be injected when start_server={start_server}, got: {head_end}"
        );
        // Regression guard for the load-bearing preview-strip invariant: the
        // analytics script MUST be wrapped in <!--moss:no-preview--> markers so
        // the preview server can strip it (Task A5). Without this, dropping the
        // marker wrapping would silently fire foreign analytics on every preview
        // reload — the exact bug this change prevents.
        let open = head_end.find("<!--moss:no-preview-->");
        let close = head_end.find("<!--/moss:no-preview-->");
        let gc = head_end.find("goatcounter");
        assert!(
            open.is_some() && close.is_some(),
            "analytics must be wrapped in moss:no-preview markers, got: {head_end}"
        );
        assert!(
            open < gc && gc < close,
            "analytics script must sit between the no-preview markers, got: {head_end}"
        );
    }
}

/// Design §7: the native comments section MUST be injected even when start_server=true.
/// The baked artifact is mode-independent; the preview shim (injected at
/// serve time by iframe_bridge.rs, never written to disk) handles the
/// preview-specific behavior — the generate_native_slots output does not
/// change based on start_server for the comments block.
#[test]
fn comments_section_present_during_preview() {
    let config = comments_fixture("https://comments.example.com");
    let article_map = make_article_map();
    let slots = generate_native_slots(
        &config,
        "/tmp/nonexistent",
        email_installed("/tmp/nonexistent"),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &article_map,
        &[],
        "en",
        None,
        None,
        true, // start_server = preview mode
        None,
        None, // [site] comments unset
    );
    let head_end = slots.get_html("head-end", "index.html").unwrap_or_default();
    assert!(
        head_end.contains("/_moss/css/comments."),
        "comments CSS MUST be injected even during preview (design §7), got: {head_end}"
    );
    assert!(
        slots.feature_styles().contains(&"comments"),
        "linking comments.css must also register it for emission, or the link 404s"
    );
    let after_article = slots
        .get_html("after-article", "posts/test.html")
        .unwrap_or_default();
    assert!(
        after_article.contains("moss-comments"),
        "comments widget MUST be injected even during preview (design §7), got: {after_article}"
    );
}

/// Native comments remain mode-independent when start_server=true,
/// and the seta subscribe form MUST still render.
/// The old Artalk CDN widget must stay out of preview/build output.
#[test]
fn moss_hosted_production_injections_absent_during_preview() {
    use crate::build::types::ParsedDocument;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let project_path = temp_dir.path().to_str().unwrap();
    let moss_dir = temp_dir.path().join(".moss");
    std::fs::create_dir_all(&moss_dir).unwrap();
    std::fs::write(
        moss_dir.join("config.toml"),
        &format!(
            "schema_version = {}\n\n[channels.email]\n",
            crate::config::migrations::CURRENT_VERSION
        ),
    )
    .unwrap();

    let pages = vec![ParsedDocument {
        url_path: "index.html".to_string(),
        ..Default::default()
    }];
    let config = ServicesConfig::default();
    let deploy = moss_hosted_deploy_config("test-site");
    let slots = generate_native_slots(
        &config,
        project_path,
        email_installed(project_path),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &HashMap::new(),
        &pages,
        "en",
        None,
        Some(deploy),
        true, // start_server = preview mode
        None,
        None, // [site] comments unset
    );

    let head_end = slots.get_html("head-end", "index.html").unwrap_or_default();
    let after_article = slots
        .get_html("after-article", "posts/test.html")
        .unwrap_or_default();
    assert!(
            !head_end.contains("Artalk.js") && !after_article.contains("artalk-comments"),
            "official Artalk CDN widget must not be injected anymore; got head_end={head_end}, after_article={after_article}"
        );
    // Seta form must be present in preview with identical HTML to build.
    // Submission is blocked by the subscribe.ts preview branch (no network POST;
    // form renders identically to build — the CSS gray-out rule was removed).
    let footer_end = slots
        .get_html("footer-end", "index.html")
        .unwrap_or_default();
    assert!(
        footer_end.contains("moss-subscribe-form"),
        "subscribe form must render in preview, got: {footer_end}"
    );
    assert!(
            !footer_end.contains("moss-service-inactive"),
            "preview form must NOT carry moss-service-inactive — blocking is via subscribe.ts JS only: {footer_end}"
        );
    // Note: beacon presence/absence no longer asserted here — beacon is
    // mode-independent (see beacon_present_in_both_build_modes_for_moss_hosted).
    // Preview safety is the runtime data-moss-preview self-gate in the beacon
    // script itself (Task A4), not omission from the slot output.
}

/// The pageview beacon must be injected for moss-hosted sites REGARDLESS of
/// start_server. Preview safety is the runtime data-moss-preview self-gate
/// (Task A4), not omission. This is the active @guo oscillation driver.
#[test]
fn beacon_present_in_both_build_modes_for_moss_hosted() {
    // moss_hosted_fixture() returns (ServicesConfig, DomainDeploymentConfig, TempDir).
    // Keep `tmp` bound for the whole test — it owns the temp directory on disk.
    let (config, deploy, tmp) = moss_hosted_fixture();
    let project_path = tmp.path().to_str().unwrap();
    for start_server in [true, false] {
        let slots = generate_native_slots(
            &config,
            project_path,
            email_installed(project_path),
            crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
            &HashMap::new(),
            &make_pages(),
            "en",
            None,
            Some(deploy.clone()),
            start_server,
            None,
            None, // [site] comments unset
        );
        let head_end = slots.get_html("head-end", "index.html").unwrap_or_default();
        assert!(
            head_end.contains("/beacon"),
            "beacon must be injected when start_server={start_server}, got: {head_end}"
        );
    }
}

/// The slot output that becomes the deploy artifact must not depend on
/// start_server. Same config → byte-identical head-end in both modes.
/// This is the direct anti-oscillation invariant.
#[test]
fn head_end_byte_identical_across_build_modes() {
    let (config, deploy, tmp) = moss_hosted_fixture();
    let project_path = tmp.path().to_str().unwrap();
    let render = |start_server: bool| {
        generate_native_slots(
            &config,
            project_path,
            email_installed(project_path),
            crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
            &HashMap::new(),
            &make_pages(),
            "en",
            None,
            Some(deploy.clone()),
            start_server,
            None,
            None, // [site] comments unset
        )
        .get_html("head-end", "index.html")
        .unwrap_or_default()
    };
    assert_eq!(
        render(true),
        render(false),
        "head-end must be byte-identical regardless of start_server"
    );
}

/// The beacon must (a) defer to DOMContentLoaded so document.body exists, and
/// (b) no-op under preview (data-moss-preview present on body). Mirrors the
/// intent of subscribe.ts's gate; the deployed artifact has the attribute
/// stripped by ship_phase, so the beacon fires live.
/// NOTE: this is a string-structure check (Rust can't run the JS). The
/// runtime behavior MUST also be verified headlessly in a browser before
/// merge (load a preview page + a stripped page; confirm no console error and
/// correct fire/no-fire) — see moss headless-verify patterns.
#[test]
fn beacon_script_defers_to_domcontentloaded_and_gates_on_preview() {
    let (config, deploy, tmp) = moss_hosted_fixture();
    let project_path = tmp.path().to_str().unwrap();
    let slots = generate_native_slots(
        &config,
        project_path,
        email_installed(project_path),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &HashMap::new(),
        &make_pages(),
        "en",
        None,
        Some(deploy),
        false,
        None,
        None, // [site] comments unset
    );
    let head_end = slots.get_html("head-end", "index.html").unwrap_or_default();
    let dcl = head_end.find("DOMContentLoaded");
    let gate = head_end.find("data-moss-preview");
    assert!(
        dcl.is_some(),
        "beacon must defer to DOMContentLoaded, got: {head_end}"
    );
    assert!(
        gate.is_some(),
        "beacon must gate on data-moss-preview, got: {head_end}"
    );
    assert!(
        dcl < gate,
        "the data-moss-preview check must be INSIDE the DOMContentLoaded handler \
             (document.body is null in <head> at parse time), got: {head_end}"
    );
}

/// The beacon must be wrapped in `<!--moss:no-preview-->` markers so the
/// preview server's serve-time strip drops it from served staging pages,
/// exactly like the analytics pixel. (Frozen generations served during
/// the zero-flicker window have the markers ship-stripped and keep the
/// script — there the serve-time data-moss-preview re-guarantee in
/// iframe_bridge::ensure_preview_body_attr re-arms the runtime self-gate;
/// see frozen_generation_page_regains_preview_gate_attribute.)
#[test]
fn beacon_wrapped_in_no_preview_markers() {
    let (config, deploy, tmp) = moss_hosted_fixture();
    let project_path = tmp.path().to_str().unwrap();
    let slots = generate_native_slots(
        &config,
        project_path,
        email_installed(project_path),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &HashMap::new(),
        &make_pages(),
        "en",
        None,
        Some(deploy),
        false,
        None,
        None, // [site] comments unset
    );
    let head_end = slots.get_html("head-end", "index.html").unwrap_or_default();
    let beacon = head_end.find("moss-beacon").expect("beacon script present");
    let open = head_end.find("<!--moss:no-preview--><script id=\"moss-beacon\"");
    let close = head_end[beacon..].find("</script><!--/moss:no-preview-->");
    assert!(
        open.is_some(),
        "beacon must open with a moss:no-preview marker (serve-time strip), got: {head_end}"
    );
    assert!(
        close.is_some(),
        "beacon must close its moss:no-preview marker region, got: {head_end}"
    );
}

/// The beacon endpoint must be an absolute URL carrying the environment's
/// scheme (https for prod/staging, http for local seta) — a
/// protocol-relative `//host` URL inherits the page's scheme, and any
/// http origin (local preview, misconfigured proxy) then POSTs to
/// http://, where the server's 308 http→https redirect kills the CORS
/// preflight.
#[test]
fn beacon_url_is_absolute_https_and_fetch_failure_is_swallowed() {
    // This test asserts the PRODUCTION host, so it must pin the resolved
    // environment: lock the env mutex and clear the override vars that
    // test_resolve_environment_precedence mutates in-process.
    let _env = crate::ENV_TEST_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("MOSS_ENV");
    std::env::remove_var("MOSS_SETA_URL");

    let (config, deploy, tmp) = moss_hosted_fixture();
    let project_path = tmp.path().to_str().unwrap();
    let slots = generate_native_slots(
        &config,
        project_path,
        email_installed(project_path),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &HashMap::new(),
        &make_pages(),
        "en",
        None,
        Some(deploy),
        false,
        None,
        None, // [site] comments unset
    );
    let head_end = slots.get_html("head-end", "index.html").unwrap_or_default();
    assert!(
        head_end.contains("'https://api.mosspub.com/api/sites/test-site/beacon'"),
        "beacon URL must be absolute https, got: {head_end}"
    );
    // An analytics ping must never surface as an unhandled promise
    // rejection on a reader's console.
    assert!(
        head_end.contains(".catch("),
        "beacon fetch must swallow failures with .catch, got: {head_end}"
    );
}

/// Ship output served outside the preview server (python -m http.server
/// on .moss/build.nosync/current, file:// opens) is ship-stripped AND
/// un-middlewared, so neither the marker strip nor the re-guaranteed
/// data-moss-preview attribute applies — with an absolute https URL the
/// beacon would silently record real pageviews from a dev machine. The
/// script must therefore also gate on loopback/file origins. Local-env
/// deploys live on *.localhost subdomains and are NOT gated.
#[test]
fn beacon_gates_on_loopback_and_file_origins() {
    let (config, deploy, tmp) = moss_hosted_fixture();
    let project_path = tmp.path().to_str().unwrap();
    let slots = generate_native_slots(
        &config,
        project_path,
        email_installed(project_path),
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &HashMap::new(),
        &make_pages(),
        "en",
        None,
        Some(deploy),
        false,
        None,
        None, // [site] comments unset
    );
    let head_end = slots.get_html("head-end", "index.html").unwrap_or_default();
    for gate in ["location.hostname", "'localhost'", "'127.0.0.1'", "'file:'"] {
        assert!(
            head_end.contains(gate),
            "beacon must gate on loopback/file origins (missing {gate}), got: {head_end}"
        );
    }
}

#[test]
fn test_footer_slot_emitted_when_reserved_file_present() {
    use crate::build::types::ParsedDocument;
    use crate::i18n::Language;

    let footer_page = ParsedDocument {
        source_path: Some("footer.md".to_string()),
        html_content: r#"<p><a href="https://symbiosis-lab.org">Symbiosis Lab</a> · 2026</p>"#
            .to_string(),
        lang: Language::En,
        ..Default::default()
    };
    let pages = vec![footer_page];
    let slots = crate::build::footer::collect_footer_slots_by_language(&pages);
    let footer = slots.get("footer-left").expect("footer-left present");
    assert!(footer
        .default
        .as_deref()
        .unwrap_or("")
        .contains("Symbiosis Lab"));
    assert!(footer.by_lang.is_empty());
}

/// The regression this whole fallback mechanism exists for: a `footer.md`
/// still downloading from the cloud must not take the footer off every page
/// on the site. `generate_native_slots` is exercised directly (not through
/// `collect_footer_slots_by_language` alone) because the fallback is wired in
/// at that call site, keyed on `project_path`.
#[test]
fn footer_survives_a_build_that_could_not_read_footer_md() {
    use crate::build::types::ParsedDocument;
    use crate::i18n::Language;

    let project_path = format!("/tmp/moss-footer-fallback-integ-{}", uuid::Uuid::new_v4());
    let config = ServicesConfig::default();

    let footer_page = ParsedDocument {
        source_path: Some("footer.md".to_string()),
        html_content: "<p>real footer</p>".to_string(),
        lang: Language::En,
        ..Default::default()
    };

    // Build 1: footer.md reads fine.
    let slots = generate_native_slots(
        &config,
        &project_path,
        false,
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &HashMap::new(),
        &[footer_page],
        "en",
        None,
        Some(DomainDeploymentConfig::default()),
        false,
        None,
        None,
    );
    assert_eq!(
        slots.get_html("footer-left", "index.html").as_deref(),
        Some("<p>real footer</p>"),
        "sanity: footer.md must inject on the build that can read it"
    );

    // Build 2: footer.md is unreadable this time (still in the cloud) — its
    // `ParsedDocument` never enters `pages` at all, exactly like
    // `read_page_source`'s deferred branch.
    let slots = generate_native_slots(
        &config,
        &project_path,
        false,
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &HashMap::new(),
        &[],
        "en",
        None,
        Some(DomainDeploymentConfig::default()),
        false,
        None,
        None,
    );
    assert_eq!(
        slots.get_html("footer-left", "index.html").as_deref(),
        Some("<p>real footer</p>"),
        "the footer must not go blank site-wide just because this build could not read footer.md"
    );
}
