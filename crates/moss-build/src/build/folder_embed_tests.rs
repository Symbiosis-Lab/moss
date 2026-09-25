use super::*;

fn make_doc(url_path: &str, label: &str, date: Option<&str>) -> ParsedDocument {
    ParsedDocument {
        url_path: url_path.to_string(),
        label: label.to_string(),
        title: label.to_string(),
        clean_stem: label.to_lowercase().replace(' ', "-"),
        date: date.map(|s| s.to_string()),
        kind: PageKind::Article,
        ..Default::default()
    }
}

pub(crate) fn make_folder_doc(url_path: &str, label: &str) -> ParsedDocument {
    ParsedDocument {
        url_path: url_path.to_string(),
        label: label.to_string(),
        title: label.to_string(),
        clean_stem: label.to_lowercase().replace(' ', "-"),
        kind: PageKind::Folder,
        direct_children_sort: Some(ResolvedSort {
            axis: SortAxis::Date,
            explicit_order: None,
            series_default: false,
        }),
        ..Default::default()
    }
}

#[test]
fn parse_marker_body_round_trips_with_emit() {
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams {
        limit: Some(3),
        sort: Some(SortAxis::Title),
        size: Some("80%".to_string()),
        ..Default::default()
    };
    let m = moss_core::resolve::embed_renderer::folder_list::emit_marker(
        "/journal/",
        "index.md",
        &params,
    );
    let body = m
        .trim_start_matches(MARKER_FOLDER_LIST)
        .trim_end_matches(MARKER_END);
    let parsed = parse_marker_body(body).unwrap();
    assert_eq!(parsed.path, "/journal/");
    assert_eq!(parsed.from, "index.md");
    assert_eq!(parsed.limit, Some(3));
    assert_eq!(parsed.sort, Some(SortAxis::Title));
    assert_eq!(parsed.size, Some("80%".to_string()));
}

#[test]
fn resolve_folder_id_absolute_strips_slashes() {
    assert_eq!(resolve_folder_id("/journal/", "index.md"), "journal");
    assert_eq!(
        resolve_folder_id("/nested/journal/", "any.md"),
        "nested/journal"
    );
}

#[test]
fn resolve_folder_id_relative_uses_from_dir() {
    assert_eq!(resolve_folder_id("journal/", "index.md"), "journal");
    assert_eq!(resolve_folder_id("sub/", "posts/foo.md"), "posts/sub");
    assert_eq!(resolve_folder_id("../news/", "posts/foo.md"), "news");
}

#[test]
fn resolve_folder_id_bare_root_is_root_even_from_a_nested_page() {
    // `/` alone names the site root. From a root-level `from` this already
    // worked by coincidence (trimming "/" to "" and then falling through to
    // the relative branch still lands on "" when `from`'s own directory is
    // also ""). From a NESTED `from` the coincidence breaks: the relative
    // branch anchors on `from`'s directory instead of the root, which is
    // what a body embed `![[/|depth:all]]` written inside a subfolder hits.
    assert_eq!(resolve_folder_id("/", "blog/post.md"), "");
    assert_eq!(resolve_folder_id("/", "index.md"), "");
}

#[test]
fn missing_folder_emits_fallback() {
    let docs: Vec<ParsedDocument> = vec![];
    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams::default();
    let marker = moss_core::resolve::embed_renderer::folder_list::emit_marker(
        "/missing/",
        "index.md",
        &params,
    );
    let out = resolve_markers(
        &marker,
        "index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(out.contains("moss-embed-missing"), "got: {}", out);
    assert!(out.contains("/missing/"), "got: {}", out);
}

#[test]
fn full_listing_renders_all_articles_in_date_order() {
    let folder = make_folder_doc("journal/index.html", "Journal");
    let a = make_doc("journal/a.html", "A", Some("2025-01-01"));
    let b = make_doc("journal/b.html", "B", Some("2025-03-01"));
    let c = make_doc("journal/c.html", "C", Some("2025-02-01"));
    let docs = vec![folder, a, b, c];

    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams::default();
    let marker = moss_core::resolve::embed_renderer::folder_list::emit_marker(
        "/journal/",
        "index.md",
        &params,
    );
    let out = resolve_markers(
        &marker,
        "index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    // All three rendered, no "More →" (no limit), date-desc. The dated
    // children auto-year-group and `list` is the compact index, so the
    // wrapper is data-layout="minimal".
    assert!(out.contains(r#"data-layout="minimal""#), "got: {}", out);
    assert!(
        !out.contains("moss-embed-more"),
        "no more link expected: {}",
        out
    );
    let pos_b = out.find(">B<").expect("B missing");
    let pos_c = out.find(">C<").expect("C missing");
    let pos_a = out.find(">A<").expect("A missing");
    assert!(pos_b < pos_c, "expected B before C (date desc): {}", out);
    assert!(pos_c < pos_a, "expected C before A (date desc): {}", out);
}

#[test]
fn also_in_doc_outside_folder_is_included_in_direct_listing() {
    // A gap-4 sweep changed `select_children_by_slug`'s depth check
    // (folder_embed.rs, the `strip_prefix(folder_prefix).is_none_or(...)`
    // arm): an `also_in` doc that is NOT physically under the folder used to
    // be excluded from a "direct" (non-flatten, the default) listing by the
    // same depth check that limits genuinely-nested docs to one level. It is
    // now included — the `also_in` declaration IS the membership claim, so
    // there is nothing to depth-check, matching `blocking.rs`'s existing
    // `also_in` handling (`None => true`). Pins that behavior.
    let folder = make_folder_doc("journal/index.html", "Journal");
    let elsewhere = ParsedDocument {
        also_in: Some(vec!["journal".to_string()]),
        ..make_doc("essays/deep/dive.html", "Deep Dive", Some("2025-01-01"))
    };
    let docs = vec![folder, elsewhere];

    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams::default();
    let marker = moss_core::resolve::embed_renderer::folder_list::emit_marker(
        "/journal/",
        "index.md",
        &params,
    );
    let out = resolve_markers(
        &marker,
        "index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(
        out.contains("Deep Dive"),
        "also_in doc outside the folder should appear in a direct-depth listing: {}",
        out
    );
}

#[test]
fn limit_emits_more_link_automatically() {
    let folder = make_folder_doc("journal/index.html", "Journal");
    let a = make_doc("journal/a.html", "A", Some("2025-01-01"));
    let b = make_doc("journal/b.html", "B", Some("2025-03-01"));
    let c = make_doc("journal/c.html", "C", Some("2025-02-01"));
    let docs = vec![folder, a, b, c];

    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams {
        limit: Some(2),
        ..Default::default()
    };
    let marker = moss_core::resolve::embed_renderer::folder_list::emit_marker(
        "/journal/",
        "index.md",
        &params,
    );
    let out = resolve_markers(
        &marker,
        "index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(
        out.contains("moss-embed-more"),
        "expected more link: {}",
        out
    );
    assert!(
        out.contains("/journal/"),
        "more href should target /journal/: {}",
        out
    );
}

#[test]
fn explicit_order_listing_omits_card_dates() {
    // A curated `sort: [list]` folder is a manual sequence, not a
    // chronological feed: its summary cards must not carry the per-card
    // date meta even though the inferred axis is Date. Control below pins
    // the ordinary Date-axis behavior (meta present).
    let mut folder = make_folder_doc("journal/index.html", "Journal");
    folder.direct_children_sort = Some(ResolvedSort {
        axis: SortAxis::Date,
        explicit_order: Some(vec!["b".to_string(), "a".to_string()]),
        series_default: true,
    });
    let a = make_doc("journal/a.html", "A", Some("2025-01-01"));
    let b = make_doc("journal/b.html", "B", Some("2025-03-01"));
    let docs = vec![folder, a, b];

    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams {
        style: Some("summary".to_string()),
        ..Default::default()
    };
    let marker = moss_core::resolve::embed_renderer::folder_list::emit_marker(
        "/journal/",
        "index.md",
        &params,
    );
    let out = resolve_markers(
        &marker,
        "index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(
        !out.contains("moss-card-meta"),
        "curated listing must not show card dates: {}",
        out
    );
    let pos_b = out.find(">B<").expect("B missing");
    let pos_a = out.find(">A<").expect("A missing");
    assert!(pos_b < pos_a, "explicit order not honored: {}", out);

    // Control: same folder without the explicit list keeps the date meta.
    let mut docs_ctl = docs.clone();
    docs_ctl[0].direct_children_sort = Some(ResolvedSort {
        axis: SortAxis::Date,
        explicit_order: None,
        series_default: false,
    });
    let out_ctl = resolve_markers(
        &marker,
        "index.md",
        &docs_ctl,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(
        out_ctl.contains("moss-card-meta"),
        "Date-axis listing should keep card dates: {}",
        out_ctl
    );
}

#[test]
fn zh_hant_vertical_listing_shows_card_meta_in_chinese_numerals() {
    // A zh-hant, vertical-typesetting folder page renders its article date
    // line in Chinese numerals (html.rs's `article_date_line_html`); its
    // card meta on the SAME page's folder listing should use the same
    // numerals, not Arabic digits + middle dot. Regression: a vertical CJK
    // site showed "1697 · 09" on folder cards while the article itself read
    // "一六九七年·九月".
    let folder = make_folder_doc("文/index.html", "文");
    let mut article = make_doc("文/a.html", "A", Some("1697-09"));
    article.lang = crate::i18n::Language::ZhHant;
    let docs = vec![folder, article];

    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams {
        style: Some("summary".to_string()),
        ..Default::default()
    };
    let marker = moss_core::resolve::embed_renderer::folder_list::emit_marker(
        "/文/",
        "index.md",
        &params,
    );
    let out = resolve_markers(
        &marker,
        "index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::ZhHant,
        Some("vertical"),
        None,
        true,
    );
    assert!(
        out.contains(r#"<div class="moss-card-meta">一六九七年·九月</div>"#),
        "zh-hant vertical folder card meta should use Chinese numerals: {}",
        out
    );
    assert!(
        !out.contains("1697 · 09"),
        "card meta must not fall back to Arabic digits: {}",
        out
    );
}

/// A body embed's listing follows its host page's EFFECTIVE typesetting —
/// the page's own `typesetting:`, else the site's. It used to read only the
/// page's, so on a site set vertical in config.toml alone, a zh-hant body
/// embed printed Arabic dates beside a page written in Chinese numerals.
#[test]
fn body_embed_listing_follows_the_hosts_effective_typesetting() {
    let folder = make_folder_doc("文/index.html", "文");
    let mut article = make_doc("文/a.html", "A", Some("1697-09"));
    article.lang = crate::i18n::Language::ZhHant;
    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams {
        style: Some("summary".to_string()),
        ..Default::default()
    };
    let marker =
        moss_core::resolve::embed_renderer::folder_list::emit_marker("/文/", "home.md", &params);
    let media_lookup =
        crate::build::media::dimensions::MediaDimensionLookup::new(&[], &[], &dir_overrides, None);
    let expand = |page: Option<&str>, site: Option<&str>| {
        let host = ParsedDocument {
            url_path: "index.html".to_string(),
            source_path: Some("home.md".to_string()),
            html_content: marker.clone(),
            kind: PageKind::Article,
            lang: crate::i18n::Language::ZhHant,
            typesetting: page.map(String::from),
            ..Default::default()
        };
        let mut documents = vec![host, folder.clone(), article.clone()];
        expand_markers_in_documents(&mut documents, &project, &dir_overrides, true, &media_lookup, site);
        documents.swap_remove(0).html_content
    };
    let cjk = "一六九七年·九月";
    let site_only = expand(None, Some("vertical"));
    assert!(site_only.contains(cjk), "site-only vertical must reach the embed: {site_only}");
    let page_override = expand(Some("horizontal"), Some("vertical"));
    assert!(!page_override.contains(cjk), "the page's horizontal must win: {page_override}");
}

#[test]
fn root_homepage_self_listing_suppresses_more_link() {
    // Homepage listing its own root children (no children_source) with a limit
    // should NOT emit More → pointing to /. Synthesize the marker the same way
    // html.rs does for a root folder-index.
    let folder = make_folder_doc("index.html", "Home");
    let a = make_doc("a.html", "A", Some("2025-01-01"));
    let b = make_doc("b.html", "B", Some("2025-03-01"));
    let c = make_doc("c.html", "C", Some("2025-02-01"));
    let docs = vec![folder, a, b, c];
    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams {
        limit: Some(2),
        ..Default::default()
    };
    // from = index.md, path = / — what synthesize_children_marker emits for the root
    let marker =
        moss_core::resolve::embed_renderer::folder_list::emit_marker("/", "index.md", &params);
    let out = resolve_markers(
        &marker,
        "index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(
        !out.contains("moss-embed-more"),
        "root self-listing should not emit More link: {}",
        out
    );
}

/// `children_more:` names a page the truncated "More →" link should point
/// at. Same fixture as `root_homepage_self_listing_suppresses_more_link`
/// (the home listing its own root, truncated) — but with `children_more` set
/// the self-listing suppression must yield: a More link pointing to the page
/// you're reading is meaningless, but `children_more` names a DIFFERENT page.
#[test]
fn children_more_emits_link_even_on_self_listing_when_truncated() {
    let mut home = make_folder_doc("index.html", "Home");
    home.children_limit = Some(2);
    home.children_more = Some("[[Everything]]".to_string());
    let a = make_doc("a.html", "A", Some("2025-01-01"));
    let b = make_doc("b.html", "B", Some("2025-03-01"));
    let c = make_doc("c.html", "C", Some("2025-02-01"));
    let everything = make_doc("everything/index.html", "Everything", None);
    let docs = vec![home.clone(), a, b, c, everything];
    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();

    let marker = synthesize_children_marker(&home, "", "index.md", true);
    let out = resolve_markers(
        &marker,
        "index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(
        out.contains(r#"<p class="moss-embed-more"><a href="/everything/">"#),
        "children_more must point the More link at its target even on a self-listing: {}",
        out
    );
}

/// An unresolvable `children_more:` is a broken wikilink — omit the More
/// link rather than guessing, the same way a dead `[[wikilink]]` anywhere
/// else in a page is left for the author to fix rather than silently routed
/// somewhere plausible.
#[test]
fn children_more_unresolved_target_omits_more_link() {
    let mut home = make_folder_doc("index.html", "Home");
    home.children_limit = Some(2);
    home.children_more = Some("[[NoSuchPage]]".to_string());
    let a = make_doc("a.html", "A", Some("2025-01-01"));
    let b = make_doc("b.html", "B", Some("2025-03-01"));
    let c = make_doc("c.html", "C", Some("2025-02-01"));
    let docs = vec![home.clone(), a, b, c];
    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();

    let marker = synthesize_children_marker(&home, "", "index.md", true);
    let out = resolve_markers(
        &marker,
        "index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(
        !out.contains("moss-embed-more"),
        "an unresolved children_more target must not emit a More link: {}",
        out
    );
}

/// A body embed can now name its own More target via `more:<target>`
/// (design addendum 7) — the embed grammar has no frontmatter to read
/// `children_more` from, so it needs its own keyed param. The link text is
/// the target page's title + " →" rather than the localised "More →", same
/// fixture shape as `children_more_emits_link_even_on_self_listing_when_truncated`
/// but driven through the real `![[/|...]]` pothole parser.
#[test]
fn embed_more_param_names_link_text_and_target() {
    let home = make_folder_doc("index.html", "Home");
    let a = make_doc("a.html", "A", Some("2025-01-01"));
    let b = make_doc("b.html", "B", Some("2025-03-01"));
    let c = make_doc("c.html", "C", Some("2025-02-01"));
    let archive = make_doc("archive/index.html", "Archive", None);
    let docs = vec![home, a, b, c, archive];
    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();

    let params = moss_core::resolve::embed_renderer::folder_list::parse_params(
        "depth:all,limit:2,more:Archive",
    );
    let marker =
        moss_core::resolve::embed_renderer::folder_list::emit_marker("/", "index.md", &params);
    let out = resolve_markers(
        &marker,
        "index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(
        out.contains(r#"<p class="moss-embed-more"><a href="/archive/">Archive →</a></p>"#),
        "embed more: param should link to its target using the target's title: {}",
        out
    );
}

/// `children_more`'s More link takes the same target-title text as the
/// embed `more:` param — the two share one resolution + rendering path, so
/// a frontmatter-named target reads the same as a body-embed-named one.
#[test]
fn children_more_link_text_uses_target_title() {
    let mut home = make_folder_doc("index.html", "Home");
    home.children_limit = Some(2);
    home.children_more = Some("[[Archive]]".to_string());
    let a = make_doc("a.html", "A", Some("2025-01-01"));
    let b = make_doc("b.html", "B", Some("2025-03-01"));
    let c = make_doc("c.html", "C", Some("2025-02-01"));
    let archive = make_doc("archive/index.html", "Archive", None);
    let docs = vec![home.clone(), a, b, c, archive];
    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();

    let marker = synthesize_children_marker(&home, "", "index.md", true);
    let out = resolve_markers(
        &marker,
        "index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(
        out.contains(r#"<p class="moss-embed-more"><a href="/archive/">Archive →</a></p>"#),
        "children_more should render the target page's title, not the localised More →: {}",
        out
    );
}

/// An unresolvable `more:` target is a broken reference like any other —
/// warn and omit the link, rather than falling back to the folder's own
/// (non-self) page, which the embed is NOT sitting on here.
#[test]
fn embed_more_param_unresolved_target_omits_more_link() {
    let folder = make_folder_doc("news/index.html", "News");
    let a = make_doc("news/a.html", "A", Some("2025-01-01"));
    let b = make_doc("news/b.html", "B", Some("2025-03-01"));
    let c = make_doc("news/c.html", "C", Some("2025-02-01"));
    let docs = vec![folder, a, b, c];
    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();

    let params =
        moss_core::resolve::embed_renderer::folder_list::parse_params("limit:2,more:Nowhere");
    let marker = moss_core::resolve::embed_renderer::folder_list::emit_marker(
        "/news/",
        "other.md",
        &params,
    );
    let out = resolve_markers(
        &marker,
        "other.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(
        !out.contains("moss-embed-more"),
        "an unresolved more: target must not emit a More link, not even the folder's own: {}",
        out
    );
}

#[test]
fn root_self_named_home_self_listing_suppresses_more_link() {
    // A root home that is self-named / marker / inherited (NOT literally
    // `index.md`) is still its folder's index, so its own depth listing with a
    // limit must NOT emit a `More →`. Regresses the `is_index_source` root /
    // home-override gap: `is_home_file("山居","")` is false (empty parent), so the
    // filename heuristic alone misses it — the doc lookup (kind == Folder) catches it.
    let mut folder = make_folder_doc("index.html", "Home");
    folder.source_path = Some("山居.md".to_string()); // root self-named home (kind=Folder)
    let a = make_doc("a.html", "A", Some("2025-01-01"));
    let b = make_doc("b.html", "B", Some("2025-03-01"));
    let c = make_doc("c.html", "C", Some("2025-02-01"));
    let docs = vec![folder, a, b, c];
    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams {
        limit: Some(2),
        ..Default::default()
    };
    // from = 山居.md (the root self-named home), path = / (self-listing).
    let marker =
        moss_core::resolve::embed_renderer::folder_list::emit_marker("/", "山居.md", &params);
    let out = resolve_markers(
        &marker,
        "山居.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(
        !out.contains("moss-embed-more"),
        "root self-named home self-listing must not emit More link: {}",
        out
    );
}

#[test]
fn self_listing_suppresses_more_link() {
    // folder-index embedding its own folder should NOT get a More link
    let folder = make_folder_doc("journal/index.html", "Journal");
    let a = make_doc("journal/a.html", "A", Some("2025-01-01"));
    let b = make_doc("journal/b.html", "B", Some("2025-03-01"));
    let c = make_doc("journal/c.html", "C", Some("2025-02-01"));
    let docs = vec![folder, a, b, c];
    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams {
        limit: Some(2),
        ..Default::default()
    };
    // from = journal/index.md — the folder's own index
    let marker = moss_core::resolve::embed_renderer::folder_list::emit_marker(
        "/journal/",
        "journal/index.md",
        &params,
    );
    let out = resolve_markers(
        &marker,
        "journal/index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(
        !out.contains("moss-embed-more"),
        "self-listing should not emit More link: {}",
        out
    );
}

#[test]
fn nested_article_embedding_parent_folder_gets_more_link() {
    // an article inside blog/ embedding ![[blog/|limit:2]] is NOT self-listing;
    // it should still get a More link so readers can navigate to the full listing
    let folder = make_folder_doc("journal/index.html", "Journal");
    let a = make_doc("journal/a.html", "A", Some("2025-01-01"));
    let b = make_doc("journal/b.html", "B", Some("2025-03-01"));
    let c = make_doc("journal/c.html", "C", Some("2025-02-01"));
    let docs = vec![folder, a, b, c];
    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams {
        limit: Some(2),
        ..Default::default()
    };
    // from = journal/some-article.md — a non-index page inside the folder
    let marker = moss_core::resolve::embed_renderer::folder_list::emit_marker(
        "/journal/",
        "journal/some-article.md",
        &params,
    );
    let out = resolve_markers(
        &marker,
        "journal/some-article.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(
        out.contains("moss-embed-more"),
        "non-index embed should emit More link: {}",
        out
    );
}

#[test]
fn sort_override_changes_order() {
    // Folder's direct_children_sort is Date; embed override sets sort=title so
    // alphabetical wins. Title ascending: A, B, C — but date desc would
    // give B, C, A.
    let folder = make_folder_doc("journal/index.html", "Journal");
    let a = make_doc("journal/a.html", "A", Some("2025-01-01"));
    let b = make_doc("journal/b.html", "B", Some("2025-03-01"));
    let c = make_doc("journal/c.html", "C", Some("2025-02-01"));
    let docs = vec![folder, a, b, c];

    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams {
        sort: Some(SortAxis::Title),
        ..Default::default()
    };
    let marker = moss_core::resolve::embed_renderer::folder_list::emit_marker(
        "/journal/",
        "index.md",
        &params,
    );
    let out = resolve_markers(
        &marker,
        "index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    let pos_a = out.find(">A<").expect("A missing");
    let pos_b = out.find(">B<").expect("B missing");
    let pos_c = out.find(">C<").expect("C missing");
    assert!(
        pos_a < pos_b && pos_b < pos_c,
        "expected title-ascending A,B,C: {}",
        out
    );
}

pub(crate) fn test_project() -> ProjectStructure {
    ProjectStructure {
        root_path: "/tmp".into(),
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

pub(crate) fn make_file_info(path: &str) -> crate::types::content::FileInfo {
    crate::types::content::FileInfo {
        path: path.to_string(),
        file_type: path.rsplit('.').next().unwrap_or("").to_lowercase(),
        size: 0,
        modified: None,
    }
}

fn test_project_with_html(paths: &[&str]) -> ProjectStructure {
    let mut p = test_project();
    p.html_files = paths.iter().map(|s| make_file_info(s)).collect();
    p
}

#[test]
fn folder_with_source_index_html_renders_iframe() {
    let docs: Vec<ParsedDocument> = vec![];
    let project = test_project_with_html(&["cities-heat-map-app/index.html"]);
    let dir_overrides = std::collections::HashMap::new();
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams::default();
    let marker = moss_core::resolve::embed_renderer::folder_list::emit_marker(
        "/cities-heat-map-app/",
        "index.md",
        &params,
    );
    let out = resolve_markers(
        &marker,
        "index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(out.contains("<iframe"), "expected iframe, got: {}", out);
    assert!(
        out.contains("class=\"moss-embed\""),
        "missing class: {}",
        out
    );
    assert!(
        out.contains("data-type=\"iframe\""),
        "missing data-type: {}",
        out
    );
    assert!(
        out.contains("src=\"cities-heat-map-app/index.html\""),
        "wrong src: {}",
        out
    );
    assert!(out.contains("loading=\"lazy\""), "missing lazy: {}", out);
    assert!(
        !out.contains("moss-embed-missing"),
        "should not be missing: {}",
        out
    );
}

#[test]
fn folder_with_source_index_htm_also_renders_iframe() {
    let docs: Vec<ParsedDocument> = vec![];
    let project = test_project_with_html(&["legacy-app/index.htm"]);
    let dir_overrides = std::collections::HashMap::new();
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams::default();
    let marker = moss_core::resolve::embed_renderer::folder_list::emit_marker(
        "/legacy-app/",
        "index.md",
        &params,
    );
    let out = resolve_markers(
        &marker,
        "index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(out.contains("src=\"legacy-app/index.htm\""), "got: {}", out);
}

#[test]
fn folder_with_both_index_html_and_htm_prefers_html() {
    let docs: Vec<ParsedDocument> = vec![];
    let project = test_project_with_html(&["dual-app/index.htm", "dual-app/index.html"]);
    let dir_overrides = std::collections::HashMap::new();
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams::default();
    let marker = moss_core::resolve::embed_renderer::folder_list::emit_marker(
        "/dual-app/",
        "index.md",
        &params,
    );
    let out = resolve_markers(
        &marker,
        "index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(
        out.contains("src=\"dual-app/index.html\""),
        "should prefer .html: {}",
        out
    );
    assert!(
        !out.contains("dual-app/index.htm\""),
        "must not pick .htm when .html exists: {}",
        out
    );
}

#[test]
fn nested_folder_id_resolves_against_html_files() {
    let docs: Vec<ParsedDocument> = vec![];
    let project = test_project_with_html(&["Resources/app/index.html"]);
    let dir_overrides = std::collections::HashMap::new();
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams::default();
    let marker = moss_core::resolve::embed_renderer::folder_list::emit_marker(
        "/Resources/app/",
        "Resources/index.md",
        &params,
    );
    let out = resolve_markers(
        &marker,
        "Resources/index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    // From Resources/index.md → Resources/app/index.html: "app/index.html".
    assert!(out.contains("src=\"app/index.html\""), "wrong src: {}", out);
}

#[test]
fn folder_iframe_applies_percent_size() {
    let docs: Vec<ParsedDocument> = vec![];
    let project = test_project_with_html(&["Resources/app/index.html"]);
    let dir_overrides = std::collections::HashMap::new();
    let mut params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams::default();
    params.size = Some("80%".to_string());
    let marker = moss_core::resolve::embed_renderer::folder_list::emit_marker(
        "/Resources/app/",
        "Research.md",
        &params,
    );
    let out = resolve_markers(
        &marker,
        "Research.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(out.contains("<iframe"), "got: {}", out);
    assert!(
        out.contains("width=\"80%\""),
        "expected width=80%; got: {}",
        out
    );
}

#[test]
fn folder_iframe_applies_box_size() {
    let docs: Vec<ParsedDocument> = vec![];
    let project = test_project_with_html(&["Resources/app/index.html"]);
    let dir_overrides = std::collections::HashMap::new();
    let mut params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams::default();
    params.size = Some("800x600".to_string());
    let marker = moss_core::resolve::embed_renderer::folder_list::emit_marker(
        "/Resources/app/",
        "Research.md",
        &params,
    );
    let out = resolve_markers(
        &marker,
        "Research.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(out.contains("<iframe"), "got: {}", out);
    assert!(
        out.contains("width=\"800px\""),
        "expected width=800px; got: {}",
        out
    );
    assert!(
        out.contains("height=\"600px\""),
        "expected height=600px; got: {}",
        out
    );
}

#[test]
fn folder_iframe_without_size_is_unsized() {
    // Default (no size) must not emit width/height — guards that the size
    // path is opt-in and doesn't regress the existing iframe shape.
    let docs: Vec<ParsedDocument> = vec![];
    let project = test_project_with_html(&["Resources/app/index.html"]);
    let dir_overrides = std::collections::HashMap::new();
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams::default();
    let marker = moss_core::resolve::embed_renderer::folder_list::emit_marker(
        "/Resources/app/",
        "Research.md",
        &params,
    );
    let out = resolve_markers(
        &marker,
        "Research.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(out.contains("<iframe"), "got: {}", out);
    assert!(
        !out.contains("width=\""),
        "default iframe must be unsized; got: {}",
        out
    );
    assert!(
        !out.contains("height=\""),
        "default iframe must be unsized; got: {}",
        out
    );
}

#[test]
fn iframe_src_uses_relative_path_from_nested_page() {
    let docs: Vec<ParsedDocument> = vec![];
    let project = test_project_with_html(&["app/index.html"]);
    let dir_overrides = std::collections::HashMap::new();
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams::default();
    let marker = moss_core::resolve::embed_renderer::folder_list::emit_marker(
        "/app/",
        "posts/resources.md",
        &params,
    );
    let out = resolve_markers(
        &marker,
        "posts/resources.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    // From posts/resources.md → app/index.html: relative_asset_path
    // yields "../app/index.html", then the pretty-URL pass adds one more
    // "../" because resources.md is non-index (wraps to /posts/resources/).
    // Final src is "../../app/index.html".
    assert!(
        out.contains("src=\"../../app/index.html\""),
        "wrong src: {}",
        out
    );
}

#[test]
fn iframe_src_is_slugified_lowercase_when_source_lacks_shared_prefix() {
    // Regression (a real site): ![[/Resources/cities-heat-map-app/]] embedded
    // from a ROOT page. moss slugifies output directories to lowercase
    // ("resources/cities-heat-map-app/"), but the iframe src was emitted
    // from the case-preserving folder_id ("Resources/..."), so it 404'd on
    // case-sensitive (Linux/Caddy) servers while resolving fine on
    // case-insensitive macOS APFS. Because the source page (Research.md) is
    // at the root, it shares no path prefix with the target, so the capital
    // "Resources" segment survives into the relative URL — unlike
    // `nested_folder_id_resolves_against_html_files`, where the shared
    // "Resources/" prefix cancels and masks the bug. The emitted src must be
    // lowercase to match the slugified output directory.
    let docs: Vec<ParsedDocument> = vec![];
    let project = test_project_with_html(&["Resources/cities-heat-map-app/index.html"]);
    let dir_overrides = std::collections::HashMap::new();
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams::default();
    let marker = moss_core::resolve::embed_renderer::folder_list::emit_marker(
        "/Resources/cities-heat-map-app/",
        "Research.md",
        &params,
    );
    let out = resolve_markers(
        &marker,
        "Research.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(out.contains("<iframe"), "expected iframe, got: {}", out);
    // Research.md is a non-index root page → wraps to /research/, so the
    // pretty-URL pass adds one "../". The src must be slugified lowercase.
    assert!(
        out.contains("src=\"../resources/cities-heat-map-app/index.html\""),
        "iframe src must be slugified lowercase to match the output dir; got: {}",
        out
    );
    assert!(
        !out.contains("Resources/cities-heat-map-app"),
        "src must not retain the case-preserving 'Resources' segment; got: {}",
        out
    );
}

#[test]
fn content_folder_still_renders_listing() {
    let folder = make_folder_doc("journal/index.html", "Journal");
    let a = make_doc("journal/a.html", "A", Some("2025-01-01"));
    let b = make_doc("journal/b.html", "B", Some("2025-02-01"));
    let docs = vec![folder, a, b];
    let project = test_project_with_html(&[]);
    let dir_overrides = std::collections::HashMap::new();
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams::default();
    let marker = moss_core::resolve::embed_renderer::folder_list::emit_marker(
        "/journal/",
        "index.md",
        &params,
    );
    let out = resolve_markers(
        &marker,
        "index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(!out.contains("<iframe"), "should be listing, got: {}", out);
    assert!(
        out.contains("data-layout=\"minimal\""),
        "expected listing: {}",
        out
    );
}

#[test]
fn folder_without_doc_or_index_html_still_emits_missing() {
    let docs: Vec<ParsedDocument> = vec![];
    let project = test_project_with_html(&[]);
    let dir_overrides = std::collections::HashMap::new();
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams::default();
    let marker = moss_core::resolve::embed_renderer::folder_list::emit_marker(
        "/ghost/", "index.md", &params,
    );
    let out = resolve_markers(
        &marker,
        "index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(
        out.contains("moss-embed-missing"),
        "expected missing fallback: {}",
        out
    );
    assert!(
        out.contains("/ghost/"),
        "expected path in fallback: {}",
        out
    );
    assert!(
        !out.contains("<iframe"),
        "must not iframe when no index.html: {}",
        out
    );
}

#[test]
fn capitalized_folder_id_slugified_for_content_lookup() {
    // Regression: ParsedDocument.url_path is slugified (lowercase,
    // hyphen-normalized) by moss-core's generate_slug, but folder_id from
    // a literal wikilink target preserves case. The lookup must slugify
    // folder_id to find the content-folder doc when source folder names
    // are capitalized. Without this, the listing branch silently misses
    // and falls through to the iframe (or missing) branch.
    let folder = make_folder_doc("resources/cities-heat-map-app/index.html", "App");
    let a = make_doc(
        "resources/cities-heat-map-app/a.html",
        "A",
        Some("2025-01-01"),
    );
    let docs = vec![folder, a];
    let project = test_project_with_html(&[]);
    let dir_overrides = std::collections::HashMap::new();
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams::default();
    // The marker carries the capitalized form, as the wikilink would.
    let marker = moss_core::resolve::embed_renderer::folder_list::emit_marker(
        "/Resources/cities-heat-map-app/",
        "Resources/Resources.md",
        &params,
    );
    let out = resolve_markers(
        &marker,
        "Resources/Resources.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(
        !out.contains("<iframe"),
        "capitalized content folder must render listing, got: {}",
        out
    );
    assert!(
        out.contains("data-layout=\"minimal\""),
        "expected listing: {}",
        out
    );
}

#[test]
fn capitalized_folder_with_readme_flips_iframe_to_listing() {
    // End-to-end documented behavior: a webapp folder that ALSO has a
    // markdown file (README.md, notes.md, etc.) is promoted to a content
    // folder and gets the listing form, not the iframe — even when the
    // source folder name is capitalized.
    let folder = make_folder_doc("resources/cities-heat-map-app/index.html", "App");
    let readme = make_doc(
        "resources/cities-heat-map-app/readme/index.html",
        "Readme",
        Some("2025-01-01"),
    );
    let docs = vec![folder, readme];
    let project = test_project_with_html(&[
        "Resources/cities-heat-map-app/index.html", // source webapp index
    ]);
    let dir_overrides = std::collections::HashMap::new();
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams::default();
    let marker = moss_core::resolve::embed_renderer::folder_list::emit_marker(
        "/Resources/cities-heat-map-app/",
        "Resources/Resources.md",
        &params,
    );
    let out = resolve_markers(
        &marker,
        "Resources/Resources.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(
        !out.contains("<iframe"),
        "README presence must flip to listing, got: {}",
        out
    );
}

#[test]
fn folder_name_with_space_iframes_with_slugified_output_path() {
    // A source folder like "my app" has its on-disk path with a space.
    // The iframe lookup compares against project.html_files which is the
    // raw on-disk path, so the predicate matches. But the OUTPUT path is
    // slugified by `resolve_path_with_overrides` (intermediate dir
    // "my app" → "my-app"), so the iframe src must point at "my-app/",
    // not the raw "my%20app/". (Before the output-space fix this asserted
    // "my%20app/index.html", which 404'd on deploy — the same case/slug
    // bug class as the capitalized-folder regression.)
    let docs: Vec<ParsedDocument> = vec![];
    let project = test_project_with_html(&["my app/index.html"]);
    let dir_overrides = std::collections::HashMap::new();
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams::default();
    let marker = moss_core::resolve::embed_renderer::folder_list::emit_marker(
        "/my app/", "index.md", &params,
    );
    let out = resolve_markers(
        &marker,
        "index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(out.contains("<iframe"), "expected iframe, got: {}", out);
    // Output dir is slugified: "my app" → "my-app".
    assert!(
        out.contains("src=\"my-app/index.html\""),
        "expected slugified src matching the output dir: {}",
        out
    );
    assert!(
        !out.contains("my%20app"),
        "src must not retain the raw spaced path: {}",
        out
    );
}

#[test]
fn synthesize_marker_from_frontmatter() {
    let mut doc = ParsedDocument::default();
    doc.children_style = Some(moss_core::Resolved::frontmatter("grid".to_string()));
    doc.children_limit = Some(5);
    let marker = synthesize_children_marker(&doc, "projects", "index.md", false);
    assert!(marker.contains("path=/projects/"));
    assert!(marker.contains("from=index.md"));
    assert!(marker.contains("style=grid"));
    assert!(marker.contains("limit=5"));
}

#[test]
fn synthesize_marker_homepage_defaults_depth_all() {
    let doc = ParsedDocument::default();
    let marker = synthesize_children_marker(&doc, "", "index.md", true);
    assert!(marker.contains("depth=all"));
}

#[test]
fn synthesize_marker_folder_index_no_depth_default() {
    let doc = ParsedDocument::default();
    let marker = synthesize_children_marker(&doc, "articles", "articles/index.md", false);
    assert!(!marker.contains("depth="));
}

// ---- Multilingual children scoping (location model) ----

fn multilingual_project() -> ProjectStructure {
    let mut p = test_project();
    p.has_language_trees = true;
    p
}

/// KEY: the root homepage (is_homepage) on a multilingual site lists only the
/// default tree — root docs (incl. nested non-language subfolders) appear,
/// language-subtree docs (`en/…`, incl. nested) are excluded.
#[test]
fn root_homepage_multilingual_excludes_language_subtrees() {
    let home = make_folder_doc("index.html", "Home");
    let zh_root = make_doc("alpha.html", "Alpha", Some("2025-01-01"));
    let zh_sub = make_doc("writings/beta.html", "Beta", Some("2025-02-01"));
    let en_top = make_doc("en/yankee.html", "Yankee", Some("2025-03-01"));
    let en_nested = make_doc("en/writing/xray.html", "Xray", Some("2025-04-01"));
    let docs = vec![home, zh_root, zh_sub, en_top, en_nested];
    let project = multilingual_project();
    let dir_overrides = std::collections::HashMap::new();
    // Drive the real root-home path: is_homepage = true (sets scope_default_tree + depth=all).
    let marker = synthesize_children_marker(&docs[0], "", "index.md", true);
    let out = resolve_markers(
        &marker,
        "index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::ZhHans,
        None,
        None,
        true,
    );
    assert!(out.contains(">Alpha<"), "root doc must be listed: {}", out);
    assert!(
        out.contains(">Beta<"),
        "root subfolder doc must be listed: {}",
        out
    );
    assert!(
        !out.contains(">Yankee<"),
        "en/ doc must be excluded: {}",
        out
    );
    assert!(
        !out.contains(">Xray<"),
        "nested en/ doc must be excluded: {}",
        out
    );
}

/// Single-language site (no language trees): the root home lists everything —
/// the scope gate is off, so behavior is unchanged.
#[test]
fn single_language_root_homepage_lists_all() {
    let home = make_folder_doc("index.html", "Home");
    let a = make_doc("alpha.html", "Alpha", Some("2025-01-01"));
    let b = make_doc("posts/beta.html", "Beta", Some("2025-02-01"));
    let docs = vec![home, a, b];
    let project = test_project(); // has_language_trees = false
    let dir_overrides = std::collections::HashMap::new();
    let marker = synthesize_children_marker(&docs[0], "", "index.md", true);
    let out = resolve_markers(
        &marker,
        "index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(out.contains(">Alpha<"), "{}", out);
    assert!(out.contains(">Beta<"), "{}", out);
}

/// Documented (location-model) behavior: a single-language site that happens to
/// have a top-level folder named like a language code is treated as multilingual,
/// so the root home excludes it. Pinned so the false-positive is intentional, and
/// any future stricter gate is a visible, reviewed change.
#[test]
fn coincidental_language_named_folder_excluded_from_root_home() {
    let home = make_folder_doc("index.html", "Home");
    let a = make_doc("alpha.html", "Alpha", Some("2025-01-01"));
    let en = make_doc("en/note.html", "Note", Some("2025-02-01"));
    let docs = vec![home, a, en];
    let project = multilingual_project(); // scan would set this true given en/
    let dir_overrides = std::collections::HashMap::new();
    let marker = synthesize_children_marker(&docs[0], "", "index.md", true);
    let out = resolve_markers(
        &marker,
        "index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(out.contains(">Alpha<"), "{}", out);
    assert!(
        !out.contains(">Note<"),
        "known-language-named folder treated as a tree: {}",
        out
    );
}

/// Regression guard for the EXISTING mechanism: a language-subtree home is a
/// folder index (is_homepage = false) scoped by its folder prefix, not by the
/// new flag. It lists its own subtree and excludes root docs. (Review A: the
/// `/en/` home is folder_prefix-scoped — this drives that real path.)
#[test]
fn language_home_excludes_root_docs_via_folder_prefix() {
    let mut en_home = make_folder_doc("en/index.html", "En Home");
    en_home.children_depth = Some("all".to_string());
    let en_doc = make_doc("en/writing/xray.html", "Xray", Some("2025-03-01"));
    let zh_doc = make_doc("alpha.html", "Alpha", Some("2025-01-01"));
    let docs = vec![en_home.clone(), en_doc, zh_doc];
    let project = multilingual_project();
    let dir_overrides = std::collections::HashMap::new();
    // is_homepage = false → folder-index path (scope_default_tree stays false).
    let marker = synthesize_children_marker(&en_home, "en", "en/index.md", false);
    let out = resolve_markers(
        &marker,
        "en/index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(out.contains(">Xray<"), "en/ doc must be listed: {}", out);
    assert!(
        !out.contains(">Alpha<"),
        "root doc excluded via folder_prefix: {}",
        out
    );
}

/// A `children_source` homepage targets another folder by user intent, so it is
/// NOT scoped to the default tree (scope_default_tree stays false).
#[test]
fn synthesize_marker_children_source_not_scoped() {
    let mut doc = ParsedDocument::default();
    doc.children_source = Some("projects".to_string());
    let marker = synthesize_children_marker(&doc, "projects", "index.md", true);
    assert!(!marker.contains("scope_default_tree"), "marker: {}", marker);
}

/// The root homepage default-mode marker carries the scope flag.
#[test]
fn synthesize_marker_root_homepage_sets_scope_default_tree() {
    let doc = ParsedDocument::default();
    let marker = synthesize_children_marker(&doc, "", "index.md", true);
    assert!(marker.contains("scope_default_tree"), "marker: {}", marker);
}

/// `scope_default_tree` survives emit → parse (no fragile empty-value channel).
#[test]
fn marker_round_trips_scope_default_tree() {
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams {
        scope_default_tree: true,
        ..Default::default()
    };
    let m = moss_core::resolve::embed_renderer::folder_list::emit_marker("/", "index.md", &params);
    let body = m
        .trim_start_matches(MARKER_FOLDER_LIST)
        .trim_end_matches(MARKER_END);
    let parsed = parse_marker_body(body).unwrap();
    assert!(parsed.scope_default_tree);
}

// ---- Whole-site listing: `children: '[[/]]'` from an ordinary page ----
//
// Fixture shared by both tests: a root homepage, two content folders with
// two leaf articles each, and a root-level page `Everything.md` that is
// NEITHER the homepage nor a folder index — an ordinary page carrying
// `children: '[[/]]'` to host a whole-site listing elsewhere on the site.

fn whole_site_listing_fixture() -> (ParsedDocument, Vec<ParsedDocument>) {
    let home = make_folder_doc("index.html", "Home");
    let blog_folder = make_folder_doc("blog/index.html", "Blog");
    let blog_1 = make_doc("blog/post-one.html", "Post One", Some("2025-01-01"));
    let blog_2 = make_doc("blog/post-two.html", "Post Two", Some("2025-01-02"));
    let proj_folder = make_folder_doc("projects/index.html", "Projects");
    let proj_1 = make_doc("projects/alpha.html", "Alpha", Some("2025-02-01"));
    let proj_2 = make_doc("projects/beta.html", "Beta", Some("2025-02-02"));
    let mut everything = make_doc("everything/index.html", "Everything", None);
    everything.source_path = Some("everything.md".to_string());
    everything.children_source = Some("[[/]]".to_string());
    let docs = vec![
        home,
        blog_folder,
        blog_1,
        blog_2,
        proj_folder,
        proj_1,
        proj_2,
        everything.clone(),
    ];
    (everything, docs)
}

#[test]
fn root_wikilink_with_depth_all_lists_every_page_not_folders_or_self() {
    let (mut everything, docs) = whole_site_listing_fixture();
    everything.children_depth = Some("all".to_string());

    let marker = synthesize_children_marker(&everything, "", "everything.md", false);
    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();
    let out = resolve_markers(
        &marker,
        "everything.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );

    assert!(out.contains(">Post One<"), "got: {}", out);
    assert!(out.contains(">Post Two<"), "got: {}", out);
    assert!(out.contains(">Alpha<"), "got: {}", out);
    assert!(out.contains(">Beta<"), "got: {}", out);
    assert!(
        !out.contains(">Blog<"),
        "folder index pages must not appear in a flattened whole-site listing: {}",
        out
    );
    assert!(
        !out.contains(">Projects<"),
        "folder index pages must not appear in a flattened whole-site listing: {}",
        out
    );
    assert!(
        !out.contains(">Everything<"),
        "the hosting page must not list itself: {}",
        out
    );
}

#[test]
fn root_wikilink_without_depth_lists_direct_folders_only() {
    let (everything, docs) = whole_site_listing_fixture();
    // children_depth left unset — render_one defaults to "direct".

    let marker = synthesize_children_marker(&everything, "", "everything.md", false);
    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();
    let out = resolve_markers(
        &marker,
        "everything.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );

    assert!(out.contains(">Blog<"), "got: {}", out);
    assert!(out.contains(">Projects<"), "got: {}", out);
    assert!(
        !out.contains(">Post One<"),
        "direct depth must not reach into folders: {}",
        out
    );
    assert!(
        !out.contains(">Alpha<"),
        "direct depth must not reach into folders: {}",
        out
    );
    assert!(
        !out.contains(">Everything<"),
        "the hosting page must not list itself: {}",
        out
    );
}

/// The body-embed form (`![[/|depth:all]]`) goes through the same
/// `resolve_folder_id` + depth machinery as the frontmatter form above, but
/// `from` is anchored on whichever page the embed is written in — which is
/// where the root-slash bug actually bit: a nested `from` used to resolve
/// `/` to the EMBEDDING page's own folder instead of the site root, a bug
/// invisible as long as every test (and the real homepage itself) happened
/// to embed from a root-level page.
#[test]
fn root_body_embed_with_depth_all_lists_whole_site_from_a_nested_page() {
    let (_, docs) = whole_site_listing_fixture();
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams {
        depth: Some("all".to_string()),
        ..Default::default()
    };
    // Written inside blog/post-one.md — nested in a subfolder, not at the
    // root where the bug stayed hidden by coincidence.
    let marker = moss_core::resolve::embed_renderer::folder_list::emit_marker(
        "/",
        "blog/post-one.md",
        &params,
    );
    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();
    let out = resolve_markers(
        &marker,
        "blog/post-one.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(out.contains(">Post Two<"), "got: {}", out);
    assert!(out.contains(">Alpha<"), "got: {}", out);
    assert!(out.contains(">Beta<"), "got: {}", out);
    assert!(
        !out.contains(">Blog<"),
        "folder index pages must not appear in a flattened listing: {}",
        out
    );
    assert!(
        !out.contains(">Projects<"),
        "folder index pages must not appear in a flattened listing: {}",
        out
    );
}

#[test]
fn iframe_src_via_shared_output_url_helper_is_lowercase() {
    let docs: Vec<ParsedDocument> = vec![];
    let project = test_project_with_html(&["Photos/App/index.html"]);
    let dir_overrides = std::collections::HashMap::new();
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams::default();
    let marker = moss_core::resolve::embed_renderer::folder_list::emit_marker(
        "/Photos/App/",
        "Research.md",
        &params,
    );
    let out = resolve_markers(
        &marker,
        "Research.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(out.contains("<iframe"), "got: {}", out);
    // Research.md is a non-index root page → /research/ → one "../"; output
    // dirs slugified: Photos/App → photos/app.
    assert!(
        out.contains("src=\"../photos/app/index.html\""),
        "iframe src must be slugified lowercase via the shared helper; got: {}",
        out
    );
}

#[test]
fn grid_embed_iframe_cover_gets_dark_default_color() {
    fn make_doc(title: &str, url_path: &str, cover: Option<&str>) -> ParsedDocument {
        ParsedDocument {
            title: title.to_string(),
            label: title.to_string(),
            url_path: url_path.to_string(),
            cover: cover.map(|s| s.to_string()),
            lang: crate::i18n::Language::En,
            kind: moss_core::PageKind::Article,
            ..Default::default()
        }
    }
    let docs = vec![
        // The embed's folder-index doc is always `PageKind::Folder` in a
        // real build: a bare `index.html` url_path only happens for a
        // folder's own index page, never an article.
        ParsedDocument { kind: moss_core::PageKind::Folder, ..make_doc("Lab", "lab/index.html", None) },
        make_doc("Widget", "lab/widget-post/index.html", Some("widget.html")),
    ];
    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams {
        style: Some("grid".to_string()),
        ..Default::default()
    };
    let marker =
        moss_core::resolve::embed_renderer::folder_list::emit_marker("/lab/", "index.md", &params);
    let out = resolve_markers(
        &marker,
        "index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(
        out.contains("moss-card"),
        "precondition: grid embed must render cards. Got: {}",
        out
    );
    assert!(
        out.contains(r#"--moss-cover-color: hsla(0, 0%, 18%, 1)"#),
        "iframe cover card must get the dark default band. Got: {}",
        out
    );
}

/// CSS needs
/// to tell a body `![[folder/|…]]` embed apart from the frontmatter-
/// synthesized listing (homepage / folder index), so only the embed follows
/// block rhythm while the trailing automatic listing keeps its larger
/// separation. `data-embed` on `.moss-cards-container` is the mark —
/// `expand_markers_in_documents` (what a literal `![[…]]` in a page body
/// goes through) must stamp it; `resolve_markers` (what
/// `synthesize_children_marker` uses for the homepage/folder-index listing)
/// must never stamp it, even for the identical grid style and identical
/// children.
#[test]
fn body_embed_is_tagged_data_embed_but_the_frontmatter_listing_is_not() {
    let folder = make_folder_doc("lab/index.html", "Lab");
    let a = make_doc("lab/a.html", "A", Some("2026-01-01"));
    let b = make_doc("lab/b.html", "B", Some("2026-01-02"));
    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();

    // The literal-embed path: a host page whose body already carries the
    // marker text a real `![[lab/|style:grid]]` would have been lowered to.
    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams {
        style: Some("grid".to_string()),
        ..Default::default()
    };
    let marker =
        moss_core::resolve::embed_renderer::folder_list::emit_marker("/lab/", "home.md", &params);
    let host = ParsedDocument {
        url_path: "index.html".to_string(),
        source_path: Some("home.md".to_string()),
        html_content: format!("<p>intro</p>{}", marker),
        kind: PageKind::Article,
        ..Default::default()
    };
    let media_lookup =
        crate::build::media::dimensions::MediaDimensionLookup::new(&[], &[], &dir_overrides, None);
    let mut documents = vec![host, folder.clone(), a.clone(), b.clone()];
    expand_markers_in_documents(&mut documents, &project, &dir_overrides, true, &media_lookup, None);
    assert!(
        documents[0].html_content.contains("data-embed"),
        "a literal body embed must be tagged; got: {}",
        documents[0].html_content
    );

    // The frontmatter path: the SAME folder and children, synthesized as the
    // folder index's own automatic listing would be.
    let docs = vec![folder.clone(), a, b];
    let marker = synthesize_children_marker(&folder, "lab", "lab/index.md", false);
    let out = resolve_markers(
        &marker,
        "lab/index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(
        !out.contains("data-embed"),
        "the frontmatter-synthesized listing must NOT be tagged; got: {}",
        out
    );
}

/// A regression where a page that is NOT the home
/// embeds the vault root (`![[/|...]]`) — the same construct a page named
/// `Archive.md` might carry. The root's own home doc is a `PageKind::Folder`
/// whose SOURCE lives at the vault root but whose `url_path` a caller could
/// get wrong (home election is a separate concern from this lookup); the
/// embed must still resolve from a page other than the home, because
/// `dir_has_markdown_index` answers from `PageKind`/source location, not by
/// re-deriving "index.html" and hoping it matches.
#[test]
fn root_self_embed_resolves_from_a_page_that_is_not_the_home() {
    let home = ParsedDocument {
        url_path: "index.html".to_string(),
        source_path: Some("Garden Path.md".to_string()),
        label: "Garden Path".to_string(),
        title: "Garden Path".to_string(),
        clean_stem: "garden-path".to_string(),
        kind: PageKind::Folder,
        direct_children_sort: Some(ResolvedSort {
            axis: SortAxis::Date,
            explicit_order: None,
            series_default: false,
        }),
        ..Default::default()
    };
    let a = make_doc("writings/a.html", "A", Some("2026-01-01"));
    let b = make_doc("paintings/b.html", "B", Some("2026-01-02"));
    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();

    let params = moss_core::resolve::embed_renderer::folder_list::FolderEmbedParams {
        depth: Some("all".to_string()),
        ..Default::default()
    };
    let marker =
        moss_core::resolve::embed_renderer::folder_list::emit_marker("/", "Archive.md", &params);
    let archive = ParsedDocument {
        url_path: "archive/index.html".to_string(),
        source_path: Some("Archive.md".to_string()),
        html_content: marker,
        kind: PageKind::Article,
        ..Default::default()
    };
    let media_lookup =
        crate::build::media::dimensions::MediaDimensionLookup::new(&[], &[], &dir_overrides, None);
    let mut documents = vec![archive, home, a, b];
    expand_markers_in_documents(&mut documents, &project, &dir_overrides, true, &media_lookup, None);

    let out = &documents[0].html_content;
    assert!(
        !out.contains("moss-embed-missing"),
        "the root self-embed on a non-home page must resolve; got: {}",
        out
    );
    assert!(out.contains("A") && out.contains("B"), "got: {}", out);
}

/// `list` is the always-compact index: it must emit `data-layout="minimal"`
/// on the `.moss-cards` wrapper even when the listing is NOT year-grouped
/// (undated children → no year sections). This is what folds the retired
/// `minimal` style into `list` — `list` always gets the compact date+title
/// item CSS, never the summary-card layout. Summary keeps data-layout="list".
#[test]
fn children_style_list_always_emits_data_layout_minimal() {
    // Folder home with explicit children_style: list and UNDATED children,
    // so nothing auto-year-groups — the data-layout="minimal" comes purely
    // from `list` being the compact index, not from year-grouping.
    let mut folder = make_folder_doc("archive/index.html", "Archive");
    folder.children_style = Some(moss_core::Resolved::frontmatter("list".to_string()));

    let a = make_doc("archive/a.html", "A", None);
    let b = make_doc("archive/b.html", "B", None);
    let docs = vec![folder.clone(), a, b];

    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();

    // Synthesize the marker exactly as html.rs does for a folder-index page.
    let marker = synthesize_children_marker(&folder, "archive", "archive/index.md", false);

    let out = resolve_markers(
        &marker,
        "archive/index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );

    assert!(
        out.contains(r#"data-layout="minimal""#),
        "children_style: list must always emit data-layout=\"minimal\"; got: {}",
        out
    );
    // Undated listing has no year headings.
    assert!(
        !out.contains("moss-cards-minimal-year-group"),
        "undated list must not produce year-group sections; got: {}",
        out
    );
    // Confirm children are actually rendered (not an empty listing)
    assert!(
        out.contains(">A<") || out.contains("href="),
        "listing must include children"
    );
}

/// BUG 7 — an explicit `children_group: year` must produce year sections
/// even under a non-Date sort axis (skip_resort=true), which is what the
/// flattened home infers when section-leaf articles carry `weight:`. The
/// items are NOT pre-sorted by year, so buckets must be stable
/// (find-or-append, first-appearance order) — one section per year, no
/// re-sort — and each item must render via `child_list::render` (which
/// emits the `title`-classed span the minimal CSS needs), not render_child.
#[test]
fn year_groups_under_non_date_axis_skip_resort() {
    // Incoming order deliberately interleaves years (2024, 2025, 2024) to
    // prove a single 2024 section (not fragmented) and first-appearance
    // ordering (2024 before 2025), NOT date-descending.
    let alpha = make_doc("alpha.html", "Alpha", Some("2024-05-01"));
    let bravo = make_doc("bravo.html", "Bravo", Some("2025-03-01"));
    let charlie = make_doc("charlie.html", "Charlie", Some("2024-09-01"));
    let items = vec![&alpha, &bravo, &charlie];
    let all: Vec<&ParsedDocument> = items.clone();

    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();
    let out = generate_children(
        &items,
        &all,
        &project,
        "minimal",
        "year",
        crate::i18n::Language::En,
        true, // skip_resort — non-Date axis
        &dir_overrides,
        None,
        None,
        Some(moss_core::sort::SortAxis::Weight),
        true,
        false,
        &Default::default(),
    );

    // Year sections must appear.
    assert!(
        out.contains(r#"<section class="moss-cards-minimal-year-group minimal">"#),
        "expected minimal year-group sections; got: {}",
        out
    );
    assert!(
        out.contains("<h2>2024</h2>"),
        "missing 2024 heading; got: {}",
        out
    );
    assert!(
        out.contains("<h2>2025</h2>"),
        "missing 2025 heading; got: {}",
        out
    );

    // Exactly one section per year (find-or-append coalesces the two 2024s).
    assert_eq!(
        out.matches("<h2>2024</h2>").count(),
        1,
        "2024 fragmented; got: {}",
        out
    );

    // First-appearance order, not date-descending: 2024 before 2025.
    let pos_2024 = out.find("<h2>2024</h2>").unwrap();
    let pos_2025 = out.find("<h2>2025</h2>").unwrap();
    assert!(
        pos_2024 < pos_2025,
        "years re-sorted (date-desc) instead of stable; got: {}",
        out
    );

    // Intra-year order preserved (Alpha before Charlie, incoming order).
    let pos_alpha = out.find("Alpha").unwrap();
    let pos_charlie = out.find("Charlie").unwrap();
    assert!(
        pos_alpha < pos_charlie,
        "intra-year order not preserved; got: {}",
        out
    );

    // Items use child_list::render (title-classed span), NOT render_child.
    assert!(
        out.contains(r#"<span class="moss-prefix-link-title title">Alpha</span>"#),
        "items must use child_list::render's title span; got: {}",
        out
    );
}

/// BUG 7 regression — folders always render FLAT above the year sections
/// under group=="year" skip_resort; they are never bucketed into a year.
#[test]
fn year_groups_skip_resort_keeps_folders_flat_above() {
    let folder = make_folder_doc("sub/index.html", "SubFolder");
    let sub_child = make_doc("sub/child.html", "SubChild", Some("2023-01-01"));
    let alpha = make_doc("alpha.html", "Alpha", Some("2024-05-01"));
    let bravo = make_doc("bravo.html", "Bravo", Some("2025-03-01"));

    // Children being rendered: the folder + two articles.
    let items = vec![&folder, &alpha, &bravo];
    // all_folder_docs must contain a descendant of `sub/` so the folder is
    // detected as a folder (child_count.is_some()).
    let all = vec![&folder, &sub_child, &alpha, &bravo];

    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();
    let out = generate_children(
        &items,
        &all,
        &project,
        "minimal",
        "year",
        crate::i18n::Language::En,
        true,
        &dir_overrides,
        None,
        None,
        Some(moss_core::sort::SortAxis::Weight),
        true,
        false,
        &Default::default(),
    );

    // Sections exist for the articles.
    let first_section = out
        .find("<section class=\"moss-cards-minimal-year-group minimal\">")
        .expect(&format!("expected year sections; got: {}", out));
    // Folder renders flat (render_child) ABOVE the first section.
    let folder_pos = out
        .find("SubFolder")
        .expect(&format!("folder missing; got: {}", out));
    assert!(
        folder_pos < first_section,
        "folder must render flat above year sections; got: {}",
        out
    );
    // Folder is not inside a year <section>.
    assert!(
        !out[first_section..].contains("SubFolder"),
        "folder must not be bucketed into a year section; got: {}",
        out
    );
}

/// BUG 7 regression — group=="none" under skip_resort stays a flat list
/// with NO year-group section wrappers (the new branch is gated on
/// group=="year").
#[test]
fn no_group_skip_resort_stays_flat() {
    let alpha = make_doc("alpha.html", "Alpha", Some("2024-05-01"));
    let bravo = make_doc("bravo.html", "Bravo", Some("2025-03-01"));
    // Year-only date — the flat path must not lose the "the fix must also
    // work for a bare year" regression.
    let charlie = make_doc("charlie.html", "Charlie", Some("1793"));
    let items = vec![&alpha, &bravo, &charlie];
    let all: Vec<&ParsedDocument> = items.clone();

    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();
    let out = generate_children(
        &items,
        &all,
        &project,
        "minimal",
        "none",
        crate::i18n::Language::En,
        true,
        &dir_overrides,
        None,
        None,
        Some(moss_core::sort::SortAxis::Weight),
        true,
        false,
        &Default::default(),
    );

    assert!(
        !out.contains("moss-cards-minimal-year-group"),
        "group==none must not emit year-group sections; got: {}",
        out
    );
    assert!(
        out.contains("Alpha") && out.contains("Bravo"),
        "flat list must render items; got: {}",
        out
    );
    // A flat row (no year heading above it) must show the full-precision
    // date — never a bare month that leaves the year unstated anywhere on
    // the page.
    assert!(
        out.contains(r#"<span class="moss-prefix-link-prefix date">2024 · 05</span>"#),
        "flat row must show year and month, not month alone; got: {}",
        out
    );
    assert!(
        out.contains(r#"<span class="moss-prefix-link-prefix date">2025 · 03</span>"#),
        "flat row must show year and month, not month alone; got: {}",
        out
    );
    // Year-only date: no month to omit, renders the bare year.
    assert!(
        out.contains(r#"<span class="moss-prefix-link-prefix date">1793</span>"#),
        "year-only date must render as the bare year in a flat listing; got: {}",
        out
    );
}

/// The actual bug this fix exists for (a real site's `Writings/` folder,
/// 2026-09-14): a `children_style: summary`
/// folder with an author-chosen `weight:` order (skip_resort=true, axis
/// Weight, group != "year") interleaved folders and articles before this
/// fix, because the folder/article partition was only applied on the
/// Date-axis branch. It must still separate folders above articles with the
/// divider, honoring the caller's weight-given order WITHIN each partition
/// (folder-before-folder, article-before-article), even though the two are
/// interleaved in the input.
#[test]
fn skip_resort_summary_still_separates_folders_from_articles() {
    let sub_a = make_folder_doc("collected/index.html", "Collected");
    let sub_b = make_folder_doc("illuminated/index.html", "Illuminated");
    let article_a = make_doc("songs.html", "Songs", Some("1794-01-01"));
    let article_b = make_doc("marriage.html", "Marriage", Some("1793-01-01"));
    // Deliberately interleaved weight order: article, folder, article, folder —
    // the author's chosen sequence, unrelated to date.
    let items = vec![&article_a, &sub_a, &article_b, &sub_b];
    let sub_a_child = make_doc("collected/child.html", "CollectedChild", None);
    let sub_b_child = make_doc("illuminated/child.html", "IlluminatedChild", None);
    let all = vec![
        &article_a,
        &sub_a,
        &sub_a_child,
        &article_b,
        &sub_b,
        &sub_b_child,
    ];

    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();
    let out = generate_children(
        &items,
        &all,
        &project,
        "summary",
        "none",
        crate::i18n::Language::En,
        true, // skip_resort — author's weight order
        &dir_overrides,
        None,
        None,
        Some(moss_core::sort::SortAxis::Weight),
        true,
        false,
        &Default::default(),
    );

    // Both folders must precede both articles.
    let pos_collected = out.find("Collected").expect("Collected missing");
    let pos_illuminated = out.find("Illuminated").expect("Illuminated missing");
    let pos_songs = out.find("Songs").expect("Songs missing");
    let pos_marriage = out.find("Marriage").expect("Marriage missing");
    assert!(
        pos_collected.max(pos_illuminated) < pos_songs.min(pos_marriage),
        "folders must render above articles even under a weight-sorted skip_resort listing; got: {}",
        out
    );
    // The caller's weight order survives WITHIN each partition: Collected
    // before Illuminated (their order in `items`), Songs before Marriage.
    assert!(
        pos_collected < pos_illuminated,
        "folder order not preserved; got: {}",
        out
    );
    assert!(
        pos_songs < pos_marriage,
        "article order not preserved; got: {}",
        out
    );
    // Summary style still gets the divider between the two partitions.
    assert!(
        out.contains("moss-child-section-divider"),
        "expected a divider between folders and articles; got: {}",
        out
    );
}

/// The combination the partition fix quietly repaired on its way past:
/// `children_style: summary` + `children_group: year` under a non-Date axis
/// (skip_resort). Before the fix the summary year branch was reachable only
/// from the Date-sorted arm, so a weight-ordered summary listing that asked
/// for year sections got none — the articles fell through to the flat,
/// "already sorted, leave it alone" path and the author's `children_group`
/// was silently ignored. Buckets are find-or-append, so an interleaved
/// input yields one section per year in first-appearance order, NOT
/// date-descending.
#[test]
fn skip_resort_summary_year_still_groups_by_year() {
    // 1794, 1789, 1794 — interleaved, so a single 1794 section proves
    // find-or-append rather than a contiguous-run assumption, and
    // 1794-before-1789 proves the caller's order survived.
    let songs = make_doc("songs.html", "Songs", Some("1794-01-01"));
    let innocence = make_doc("innocence.html", "Innocence", Some("1789-01-01"));
    let thel = make_doc("thel.html", "Thel", Some("1794-06-01"));
    let items = vec![&songs, &innocence, &thel];
    let all: Vec<&ParsedDocument> = items.clone();

    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();
    let out = generate_children(
        &items,
        &all,
        &project,
        "summary",
        "year",
        crate::i18n::Language::En,
        true, // skip_resort — author's weight order
        &dir_overrides,
        None,
        None,
        Some(moss_core::sort::SortAxis::Weight),
        true,
        false,
        &Default::default(),
    );

    assert!(
        out.contains("moss-cards-minimal-year-group--summary"),
        "a weight-ordered summary listing that asked for year sections must get them; got: {}",
        out
    );
    assert_eq!(
        out.matches("<h2>1794</h2>").count(),
        1,
        "1794 fragmented — buckets must be find-or-append; got: {}",
        out
    );
    let pos_1794 = out.find("<h2>1794</h2>").expect("1794 heading missing");
    let pos_1789 = out.find("<h2>1789</h2>").expect("1789 heading missing");
    assert!(
        pos_1794 < pos_1789,
        "first-appearance order, not date-descending; got: {}",
        out
    );
}

/// The undated grid arm — the one `resolve_children_config` now auto-selects
/// for a listing of bare labels — orders folders ahead of pages and then
/// case-insensitively. The grid is flat (no `moss-child-section-divider`,
/// which is summary-only), so ordering is the only thing left carrying the
/// section/page distinction; and codepoint order used to exile a lowercase
/// label past every capitalised one.
#[test]
fn undated_grid_puts_folders_first_then_folds_case() {
    let zebra = make_folder_doc("zebra", "Zebra");
    let zebra_child = make_doc("zebra/one.html", "One", None);
    let alpha = make_doc("alpha.html", "Alpha", None);
    let mao = make_doc("mao.html", "mao", None);
    let scarly = make_doc("scarly.html", "Scarly", None);

    let items = vec![&scarly, &mao, &alpha, &zebra];
    let all: Vec<&ParsedDocument> = vec![&scarly, &mao, &alpha, &zebra, &zebra_child];

    let project = test_project();
    let out = generate_children(
        &items,
        &all,
        &project,
        "grid",
        "none",
        crate::i18n::Language::En,
        false,
        &std::collections::HashMap::new(),
        None,
        None,
        Some(moss_core::sort::SortAxis::Title),
        true,
        false,
        &Default::default(),
    );

    let order: Vec<&str> = ["Zebra", "Alpha", "mao", "Scarly"]
        .into_iter()
        .filter(|label| out.contains(label))
        .collect();
    assert_eq!(order, vec!["Zebra", "Alpha", "mao", "Scarly"], "all four must render; got: {}", out);

    let at = |label: &str| out.find(label).unwrap_or_else(|| panic!("{label} missing from: {out}"));
    assert!(at("Zebra") < at("Alpha"), "folder must precede pages; got: {}", out);
    assert!(
        at("Alpha") < at("mao") && at("mao") < at("Scarly"),
        "labels must sort case-insensitively, not by codepoint; got: {}",
        out
    );
}

fn make_doc_with_cover(url_path: &str, label: &str, date: &str, cover: Option<&str>) -> ParsedDocument {
    ParsedDocument {
        cover: cover.map(|s| s.to_string()),
        ..make_doc(url_path, label, Some(date))
    }
}

/// `children_covers: only` keeps only pages with a cover, applied after
/// flattening and before `children_limit` — so a `children_limit: 2` counts
/// off the covered set, not the full set. Fixture: five dated pages, three
/// with a cover (Alpha, Charlie, Echo); Echo and Charlie are the two newest
/// of those three, even though Bravo (no cover) is newer than both.
#[test]
fn children_covers_only_keeps_newest_covered_pages() {
    let mut home = make_folder_doc("index.html", "Home");
    home.children_depth = Some("all".to_string());
    home.children_limit = Some(2);
    home.children_covers = Some("only".to_string());
    let alpha = make_doc_with_cover("alpha.html", "Alpha", "2025-01-10", Some("alpha.jpg"));
    let bravo = make_doc_with_cover("bravo.html", "Bravo", "2025-04-10", None);
    let charlie = make_doc_with_cover("charlie.html", "Charlie", "2025-03-10", Some("charlie.jpg"));
    let delta = make_doc_with_cover("delta.html", "Delta", "2025-02-10", None);
    let echo = make_doc_with_cover("echo.html", "Echo", "2025-05-10", Some("echo.jpg"));
    let docs = vec![home.clone(), alpha, bravo, charlie, delta, echo];
    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();

    let marker = synthesize_children_marker(&home, "", "index.md", true);
    let out = resolve_markers(
        &marker,
        "index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(out.contains("Echo"), "newest covered page missing; got: {}", out);
    assert!(out.contains("Charlie"), "2nd-newest covered page missing; got: {}", out);
    assert!(!out.contains("Bravo"), "Bravo has no cover and must be filtered out; got: {}", out);
    assert!(!out.contains("Alpha"), "Alpha is covered but older than the kept two; got: {}", out);
    assert!(!out.contains("Delta"), "Delta has no cover and must be filtered out; got: {}", out);
}

/// Same fixture, no `children_covers` — today's behaviour is unchanged: the
/// two newest pages overall (Echo, Bravo), cover or not.
#[test]
fn children_covers_absent_keeps_default_behavior() {
    let mut home = make_folder_doc("index.html", "Home");
    home.children_depth = Some("all".to_string());
    home.children_limit = Some(2);
    let alpha = make_doc_with_cover("alpha.html", "Alpha", "2025-01-10", Some("alpha.jpg"));
    let bravo = make_doc_with_cover("bravo.html", "Bravo", "2025-04-10", None);
    let charlie = make_doc_with_cover("charlie.html", "Charlie", "2025-03-10", Some("charlie.jpg"));
    let delta = make_doc_with_cover("delta.html", "Delta", "2025-02-10", None);
    let echo = make_doc_with_cover("echo.html", "Echo", "2025-05-10", Some("echo.jpg"));
    let docs = vec![home.clone(), alpha, bravo, charlie, delta, echo];
    let project = test_project();
    let dir_overrides = std::collections::HashMap::new();

    let marker = synthesize_children_marker(&home, "", "index.md", true);
    let out = resolve_markers(
        &marker,
        "index.md",
        &docs,
        &project,
        &dir_overrides,
        crate::i18n::Language::En,
        None,
        None,
        true,
    );
    assert!(out.contains("Echo"), "newest page missing; got: {}", out);
    assert!(out.contains("Bravo"), "2nd-newest page missing regardless of cover; got: {}", out);
    assert!(!out.contains("Charlie"), "only the two newest overall are kept; got: {}", out);
}

/// `resolve_children_config` judges a listing by its bulk, not by its exceptions.
///
/// The auto-style arm that matters here is the one a term root lands in: a page
/// of bare labels (`/authors/`, `/tags/`) picks "grid" and reads as an index.
/// Under the old `any()` quantifier a single claimed author who wrote one
/// paragraph flipped the whole roster to "summary" — 56 archive rows, no action
/// by the other 55 authors and no signal that it happened.
#[cfg(test)]
mod bulk_style_tests {
    use super::*;

    fn bare(url: &str) -> ParsedDocument {
        make_doc(url, "Label", None)
    }

    fn rich(url: &str) -> ParsedDocument {
        let mut d = make_doc(url, "Label", None);
        d.description = Some("A claimed term with a bio.".to_string());
        d
    }

    fn auto_style(docs: &[&ParsedDocument]) -> String {
        resolve_children_config(docs, None, None, false).0.value
    }

    fn one_rich_among(bare_count: usize) -> String {
        let claimed = rich("a.html");
        let others: Vec<ParsedDocument> =
            (0..bare_count).map(|i| bare(&format!("b{i}.html"))).collect();
        let mut docs: Vec<&ParsedDocument> = vec![&claimed];
        docs.extend(others.iter());
        auto_style(&docs)
    }

    #[test]
    fn one_claimed_term_in_a_roster_leaves_the_index_a_grid() {
        // The riverbend case: 1 claimed author, the rest bare labels. Both
        // clauses must fail — it is neither mostly rich nor a handful of rows.
        assert_eq!(one_rich_among(8), "grid", "one claim must not re-style the roster");
    }

    #[test]
    fn a_handful_of_bare_siblings_still_reads_as_an_archive() {
        // A young blog: one described post beside its section folders. The
        // ratio says minority, but three label rows are archive furniture,
        // not padding, so the tolerance clause keeps "summary".
        assert_eq!(one_rich_among(3), "summary");
    }

    #[test]
    fn the_tolerance_stops_at_a_screenful() {
        // The boundary the constant names, asserted from both sides so a
        // change to the tolerance cannot pass unnoticed.
        assert_eq!(one_rich_among(3), "summary");
        assert_eq!(one_rich_among(4), "grid");
    }

    #[test]
    fn a_mostly_rich_listing_is_an_archive_however_many_bare_rows_ride_along() {
        let r: Vec<ParsedDocument> = (0..12).map(|i| rich(&format!("r{i}.html"))).collect();
        let b: Vec<ParsedDocument> = (0..8).map(|i| bare(&format!("b{i}.html"))).collect();
        let docs: Vec<&ParsedDocument> = r.iter().chain(b.iter()).collect();

        assert_eq!(auto_style(&docs), "summary", "12 of 20 rich is an archive");
    }

    #[test]
    fn a_lone_rich_child_is_an_archive_not_an_outlier() {
        let only = rich("a.html");

        assert_eq!(auto_style(&[&only]), "summary");
    }

    #[test]
    fn a_listing_with_nothing_rich_is_an_index_however_short() {
        // The `rich > 0` guard: the tolerance clause must not turn an
        // all-bare listing of two into an archive of two empty rows.
        let a = bare("a.html");
        let b = bare("b.html");

        assert_eq!(auto_style(&[&a, &b]), "grid");
    }

    #[test]
    fn dates_still_choose_list_over_grid_when_the_bulk_is_bare() {
        // The bulk test replaces only the richness quantifier; the dateless
        // tiebreak in the 2x2 is unchanged.
        let claimed = rich("a.html");
        let mut dated = bare("b.html");
        dated.date = Some("2026-01-01".to_string());
        let more: Vec<ParsedDocument> = (0..5).map(|i| bare(&format!("c{i}.html"))).collect();
        let mut docs: Vec<&ParsedDocument> = vec![&claimed, &dated];
        docs.extend(more.iter());

        assert_eq!(auto_style(&docs), "list");
    }
}

/// Markdown → the moss-core pre-pass → the marker → this renderer: the
/// whole chain a body `![[folder/|…]]` embed takes.
fn render_body_embed(markdown: &str, docs: &[ParsedDocument]) -> String {
    let graph = moss_core::content_graph::ContentGraphBuilder::new().build();
    let resolved = moss_core::resolve::resolve_content("index.md", markdown, &graph, &|_| None);
    resolve_markers(
        &resolved.content_markdown,
        "index.md",
        docs,
        &test_project(),
        &std::collections::HashMap::new(),
        crate::i18n::Language::En,
        None,
        None,
        true,
    )
}

fn render_place_map_embed(markdown: &str, docs: &[ParsedDocument]) -> String {
    let graph = moss_core::content_graph::ContentGraphBuilder::new().build();
    let resolved = moss_core::resolve::resolve_content("index.md", markdown, &graph, &|_| None);
    let table: toml::value::Table = toml::from_str(
        "[\"Kyoto\"]\nlat = 35.0116\nlng = 135.7681\nprecision = \"city\"\n",
    ).unwrap();
    let maps = crate::build::place_map::PlaceMapRenderContext::new(
        crate::build::place_map::PlaceMapContext::embedded().unwrap(),
        crate::vault::places::parse_gazetteer(&table),
        "places".into(),
        crate::build::place_map::LocatorPlacement::None,
    );
    resolve_markers_with_place_maps(
        &resolved.content_markdown, "index.md", docs, &test_project(),
        &std::collections::HashMap::new(), crate::i18n::Language::En,
        None, None, true, Some(&maps),
    )
}

fn journal_docs() -> Vec<ParsedDocument> {
    vec![
        make_folder_doc("journal/index.html", "Journal"),
        make_doc("journal/a.html", "A", Some("2025-01-01")),
        make_doc("journal/b.html", "B", Some("2025-03-01")),
    ]
}

#[test]
fn a_captioned_folder_embed_puts_its_width_on_the_figure() {
    let out = render_body_embed("![[/journal/|style:grid|wide|A caption]]\n", &journal_docs());
    assert!(
        out.starts_with(r#"<figure class="moss-embed-figure" data-width="wide"><div class="moss-cards-container">"#),
        "the figure is outermost and the container carries no width of its own: {out}"
    );
    assert!(out.contains(r#"data-layout="grid""#), "style must survive the extra segments: {out}");
    assert!(out.trim_end().ends_with("<figcaption>A caption</figcaption></figure>"), "got: {out}");
}

#[test]
fn an_uncaptioned_folder_embed_wears_its_placement_on_the_container() {
    let out = render_body_embed("![[/journal/|align-right 40%]]\n", &journal_docs());
    assert!(!out.contains("moss-embed-figure"), "no wrapper without a caption: {out}");
    assert!(
        out.contains(r#"<div class="moss-cards-container moss-align-right" style="width:40%">"#),
        "got: {out}"
    );
}

#[test]
fn a_caption_with_marker_breaking_characters_reaches_the_page_intact() {
    let out = render_body_embed("![[/journal/|wide|Before --> after, a=b]]\n", &journal_docs());
    assert!(
        out.contains("<figcaption>Before --&gt; after, a=b</figcaption>"),
        "got: {out}"
    );
}

/// A term page reached only through DERIVED membership (no real folder
/// backs `places/kyoto` — it is a pseudo-folder, not a directory) used to
/// render "Folder not found" for a body embed, because in the real build
/// pipeline `derive_terms` (which is what fills `also_in`) ran AFTER
/// markers were resolved, so `also_in` was still empty at the point this
/// pseudo-folder branch checked it. Reproduces that shape with a real
/// `derive_terms` call (not hand-authored `also_in`), then feeds the
/// resulting docs through the exact chain a body embed takes.
#[test]
fn pseudo_folder_place_embed_lists_its_derived_members() {
    let kind = crate::build::terms::TermKind {
        key: "places".to_string(),
        fields: vec!["location".to_string()],
        title: "Places".to_string(),
        is_place: true,
        parents: Default::default(),
    };
    let mut docs = vec![
        make_doc("travel/kyoto-temple.html", "Kyoto Temple", Some("2025-01-01")),
        make_doc("travel/kyoto-market.html", "Kyoto Market", Some("2025-03-01")),
    ];
    docs[0].location = vec!["Kyoto".to_string()];
    docs[1].location = vec!["Kyoto".to_string()];
    crate::build::terms::derive_terms(&mut docs, vec![kind]);

    let out = render_body_embed("![[/places/kyoto/]]\n", &docs);
    assert!(!out.contains("moss-embed-missing"), "got: {out}");
    assert!(out.contains("Kyoto Temple"), "got: {out}");
    assert!(out.contains("Kyoto Market"), "got: {out}");
    // Same default a pseudo-folder with no target doc gets everywhere else
    // in this file (date-descending) — matching the order the generated
    // term page itself would render its members in, since both paths run
    // through this same `render_one`.
    let pos_market = out.find("Kyoto Market").expect("Kyoto Market missing");
    let pos_temple = out.find("Kyoto Temple").expect("Kyoto Temple missing");
    assert!(pos_market < pos_temple, "expected date-desc order: {out}");
}

#[test]
fn place_map_embed_emits_svg_with_placement_and_caption() {
    let kind = crate::build::terms::TermKind {
        key: "places".to_string(),
        fields: vec!["location".to_string()],
        title: "Places".to_string(),
        is_place: true,
        parents: Default::default(),
    };
    let mut docs = vec![make_doc("travel/kyoto.html", "Kyoto", Some("2025-01-01"))];
    docs[0].location = vec!["Kyoto".to_string()];
    crate::build::terms::derive_terms(&mut docs, vec![kind]);

    let out = render_place_map_embed(
        "![[/places/kyoto/|style:map|align-right 40%|Kyoto map]]\n",
        &docs,
    );
    assert!(out.starts_with(r#"<div class="moss-place-map-frame moss-align-right" style="width:40%"><figure class="moss-place-map""#), "got: {out}");
    assert!(out.contains("data-map-location=\"Kyoto\""), "got: {out}");
    assert!(out.trim_end().ends_with("</figure><div class=\"moss-place-map-caption\">Kyoto map</div></div>"), "got: {out}");
    assert!(!out.contains("moss-cards-container"), "map style must replace the listing: {out}");
}

/// Same bug, the roll-up-ancestor shape: `places/japan` has no doc that
/// declares `location: Japan` directly — it exists only because a city
/// under it rolls up through the gazetteer's `parent` chain
/// (`build::terms::places`), landing `places/japan` in that doc's
/// `also_in` alongside `places/kyoto`. An ancestor reached only this way
/// still needs a listing of every doc that names it, transitively.
#[test]
fn pseudo_folder_rollup_ancestor_embed_lists_its_descendants() {
    let kind = crate::build::terms::TermKind {
        key: "places".to_string(),
        fields: vec!["location".to_string()],
        title: "Places".to_string(),
        is_place: true,
        parents: [("places/kyoto".to_string(), "Japan".to_string())].into_iter().collect(),
    };
    let mut docs = vec![make_doc("travel/kyoto-temple.html", "Kyoto Temple", Some("2025-01-01"))];
    docs[0].location = vec!["Kyoto".to_string()];
    crate::build::terms::derive_terms(&mut docs, vec![kind]);
    assert!(
        docs[0].also_in.as_ref().unwrap().contains(&"places/japan".to_string()),
        "sanity: derive_terms must roll the city up into the country's also_in"
    );

    let out = render_body_embed("![[/places/japan/]]\n", &docs);
    assert!(!out.contains("moss-embed-missing"), "got: {out}");
    assert!(out.contains("Kyoto Temple"), "got: {out}");
}
