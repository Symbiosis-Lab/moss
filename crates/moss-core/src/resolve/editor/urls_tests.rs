use super::*;

fn article(source_path: &str) -> Article {
    Article { source_path: source_path.into(), ..Default::default() }
}

#[test]
fn exact_and_normalized_from_articles_and_pages() {
    let mut m = UrlMap::default();
    m.articles.insert("research/".into(), article("Research.md"));
    m.pages.insert("news/".into(), "News/News.md".into());
    let idx = ArticleMapIndex::from_map(&m);
    assert!(idx.lookup_exact("research/"));
    assert!(idx.lookup_exact("news/"));
    assert!(!idx.lookup_exact("Research/"));
    assert_eq!(idx.lookup_normalized("/Research/"), Some("/research/".into()));
    assert_eq!(idx.resolve_reference_to_url("Research", "main.md"), Some("/research/".into()));
}

#[test]
fn ambiguous_normalized_returns_none() {
    let mut m = UrlMap::default();
    m.articles.insert("research/".into(), article("Research.md"));
    m.articles.insert("RESEARCH/".into(), article("RESEARCH.md"));
    let idx = ArticleMapIndex::from_map(&m);
    assert_eq!(idx.lookup_normalized("/research/"), None); // ambiguous → silent
}

/// Keys may be stored with a trailing slash, the home page under the empty key;
/// a fragment or query is dropped; the lookup is case-sensitive.
#[test]
fn source_path_for_url_reads_both_key_forms_and_drops_fragment_and_query() {
    let mut m = UrlMap::default();
    m.articles.insert("research/".into(), article("Research.md"));
    m.pages.insert("news/".into(), "News/News.md".into());
    m.pages.insert(String::new(), "main.md".into());
    for (url, source) in [("/research/", Some("Research.md")), ("research/", Some("Research.md")),
        ("/research/#heading", Some("Research.md")), ("/research/?q=foo", Some("Research.md")),
        ("/news/", Some("News/News.md")), ("/", Some("main.md")), ("/nope/", None), ("/Research/", None)] {
        assert_eq!(m.source_path_for_url(url).as_deref(), source, "{url}");
    }
}

/// An article and a page under the same key: the article's source wins.
#[test]
fn source_path_for_url_prefers_an_article_to_a_page() {
    let mut m = UrlMap::default();
    m.articles.insert("about/".into(), article("About.md"));
    m.pages.insert("about".into(), "about/index.md".into());
    assert_eq!(m.source_path_for_url("/about/").as_deref(), Some("About.md"));
}

// ── FolderFacts ────────────────────────────────────────────────────────

fn article_with(source_path: &str, fm: &[(&str, serde_json::Value)]) -> Article {
    Article { source_path: source_path.into(), frontmatter: fm.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

/// `awards/` holding two articles, a sub-folder page, and a grandchild article.
fn awards_map() -> UrlMap {
    let mut m = UrlMap::default();
    m.pages.insert("awards/".into(), "awards/index.md".into());
    m.pages.insert("awards/2024/".into(), "awards/2024/index.md".into());
    m.articles.insert("awards/a".into(), article("awards/a.md"));
    m.articles.insert("awards/b".into(), article("awards/b.md"));
    m.articles.insert("awards/2024/x".into(), article("awards/2024/x.md"));
    m
}

#[test]
fn counts_split_direct_from_descendant() {
    let f = FolderFacts::from_map(&awards_map());
    let info = f.lookup("awards").expect("awards is in the map");
    // direct: a, b, and the 2024/ folder page. The grandchild is not direct.
    assert_eq!(info.direct_child_count, 3);
    // descendant: a, b, 2024/x — ARTICLES only; the folder page is excluded
    // (page_kind::is_listable_at_depth_all is Article-only).
    assert_eq!(info.descendant_count, 3);
}

#[test]
fn draft_and_unlisted_articles_are_excluded() {
    let mut m = awards_map();
    m.articles.insert("awards/draft".into(), article_with("awards/draft.md", &[("draft", serde_json::json!(true))]));
    m.articles.insert("awards/hidden".into(), article_with("awards/hidden.md", &[("listed", serde_json::json!(false))]));
    let f = FolderFacts::from_map(&m);
    let info = f.lookup("awards").expect("awards");
    assert_eq!(info.direct_child_count, 3, "draft/unlisted must not count");
}

#[test]
fn also_in_counts_once_whether_or_not_it_lives_under_the_folder() {
    let mut m = awards_map();
    // Lives elsewhere, claims membership → a direct child AND a descendant.
    m.articles.insert("posts/elsewhere".into(), article_with("posts/elsewhere.md", &[("also_in", serde_json::json!(["awards"]))]));
    // Already under awards/ AND claims membership → counted ONCE (the build's
    // membership test is an OR, not a sum).
    m.articles.insert("awards/both".into(), article_with("awards/both.md", &[("also_in", serde_json::json!(["awards"]))]));
    let f = FolderFacts::from_map(&m);
    let info = f.lookup("awards").expect("awards");
    assert_eq!(info.direct_child_count, 5);
    assert_eq!(info.descendant_count, 5);
}

#[test]
fn slug_overridden_cjk_folder_is_found_by_its_on_disk_name() {
    // `評選/評選.md` carries `url: awards`, so the build published it at
    // `awards/`. The classifier hands back the author-typed on-disk path.
    let mut m = UrlMap::default();
    m.pages.insert("awards/".into(), "評選/評選.md".into());
    m.articles.insert("awards/a".into(), article("評選/a.md"));
    let f = FolderFacts::from_map(&m);
    let info = f.lookup("評選").expect("reverse source-dir lookup");
    assert_eq!(info.direct_child_count, 1);
    // …and the URL-space spelling finds the same folder.
    assert_eq!(f.lookup("/awards/").expect("url-space").direct_child_count, 1);
}

#[test]
fn root_homepage_is_nobodys_child() {
    let mut m = UrlMap::default();
    m.pages.insert("".into(), "index.md".into());
    m.pages.insert("awards/".into(), "awards/index.md".into());
    m.articles.insert("hello".into(), article("hello.md"));
    let f = FolderFacts::from_map(&m);
    let info = f.lookup("/").expect("root key");
    // The top-level article and the top-level folder page — not the homepage.
    assert_eq!(info.direct_child_count, 2);
    assert_eq!(info.descendant_count, 1);
}

#[test]
fn also_in_with_a_multibyte_folder_key_does_not_panic() {
    // The ancestry walk must never byte-index a URL. `評選/` is 7 bytes;
    // `posts/hello` is 11 — an offset-based `&url[key.len()..]` lands
    // mid-sequence on other URLs and panics.
    let mut m = UrlMap::default();
    m.pages.insert("評選/".into(), "評選/評選.md".into());
    m.articles.insert("posts/hello".into(), article_with("posts/hello.md", &[("also_in", serde_json::json!(["評選"]))]));
    let f = FolderFacts::from_map(&m);
    let info = f.lookup("評選").expect("評選");
    assert_eq!(info.direct_child_count, 1);
    assert_eq!(info.descendant_count, 1);
}

#[test]
fn unknown_folder_is_none_but_an_empty_known_folder_is_zero() {
    let mut m = UrlMap::default();
    m.pages.insert("empty/".into(), "empty/index.md".into());
    let f = FolderFacts::from_map(&m);
    // Unknown → None, so the card shows NO count (distinct from "zero").
    assert!(f.lookup("nope").is_none());
    let info = f.lookup("empty").expect("known folder");
    assert_eq!(info.direct_child_count, 0);
    assert_eq!(info.descendant_count, 0);
}

#[test]
fn title_comes_from_the_map_and_is_none_when_the_scan_recorded_none() {
    // The scan records every folder page's title beside its source, so no
    // editor surface opens the index file to name a folder. A map written by
    // an older build carries no titles: the card then shows no title, never a
    // stale or guessed one.
    let mut m = UrlMap::default();
    m.pages.insert("awards/".into(), "awards/index.md".into());
    m.page_titles.insert("awards/".into(), "評選".into());
    m.pages.insert("news/".into(), "news/News.md".into());
    m.page_titles.insert("news/".into(), String::new());
    m.pages.insert("old/".into(), "old/index.md".into());
    let f = FolderFacts::from_map(&m);

    assert_eq!(f.lookup("awards").expect("awards").title.as_deref(), Some("評選"));
    assert_eq!(f.lookup("news").expect("news").title, None, "empty title → None");
    assert_eq!(f.lookup("old").expect("old").title, None, "no record → None");
}

// ── Generated pages and moved terms ────────────────────────────────────

/// A synthesized index page (index-less folder, generated term page, the
/// namespace root) is a deployed URL with no source: it resolves exactly and
/// by case, but never by stem — there is nothing to open.
#[test]
fn generated_pages_resolve_without_a_source() {
    let mut m = UrlMap::default();
    m.generated = vec!["writings/".into(), "authors/".into(), "authors/林小滿/".into()];
    let idx = ArticleMapIndex::from_map(&m);
    assert!(idx.lookup_exact("/writings/"));
    assert!(idx.lookup_exact("authors/林小滿"));
    assert_eq!(idx.lookup_normalized("/Writings/"), Some("/writings/".into()));
    assert_eq!(idx.resolve_reference_to_url("writings", "main.md"), None);
    assert_eq!(idx.lookup_moved("/writings/"), None);
}

/// A claimed term's generated URL is gone; the index points it at the claim
/// page. An unclaimed term is not moved, and its generated page still resolves.
#[test]
fn claimed_term_url_is_moved_to_the_claiming_page() {
    let mut m = UrlMap::default();
    m.articles.insert("about/ma/".into(), article("關於/作者/ma.md"));
    m.generated = vec!["authors/".into(), "authors/scarly/".into()];
    m.moved.insert("authors/林小滿".into(), "about/ma/".into());
    // The home page claiming a term: its pretty URL key is empty, the site root.
    m.moved.insert("tags/home".into(), String::new());
    let idx = ArticleMapIndex::from_map(&m);
    assert_eq!(idx.lookup_moved("/tags/home/"), Some("/".into()));
    assert!(!idx.lookup_exact("/authors/林小滿/"), "the generated URL no longer exists");
    assert_eq!(idx.lookup_moved("/authors/林小滿/"), Some("/about/ma/".into()));
    assert_eq!(idx.lookup_moved("/authors/scarly/"), None);
    assert!(idx.lookup_exact("/authors/scarly/"));
}

// ── Folder-index wikilinks (UrlMap::folder_indexes) ────────────────────

/// A bare top-level folder with children but no index of its own resolves
/// through `folder_indexes`, case-insensitively — unlike `generated`, which
/// never joins the stem set (see `generated_pages_resolve_without_a_source`).
#[test]
fn folder_index_resolves_bare_and_case_insensitively() {
    let mut m = UrlMap::default();
    m.folder_indexes.insert("writings".into(), "writings".into());
    let idx = ArticleMapIndex::from_map(&m);
    assert_eq!(idx.resolve_reference_to_url("writings", "main.md"), Some("/writings/".into()));
    assert_eq!(idx.resolve_reference_to_url("Writings", "main.md"), Some("/writings/".into()));
}

/// A page whose filename stem collides with a folder name wins — `by_stem`
/// is checked first and `folder_indexes` is a fallback, so the two maps
/// never merge into one ambiguous bucket.
#[test]
fn a_same_named_page_wins_over_its_folder() {
    let mut m = UrlMap::default();
    m.folder_indexes.insert("writings".into(), "writings".into());
    m.articles.insert("other/writings".into(), article("Other/Writings.md"));
    let idx = ArticleMapIndex::from_map(&m);
    assert_eq!(
        idx.resolve_reference_to_url("Writings", "main.md"),
        Some("/other/writings/".into()),
        "the page must win, not the folder"
    );
}

/// A bare leaf must not match a nested folder, on the editor as on the build
/// (`by_stem`'s lookup ignores path, so a leaf match would reach past a
/// folder's nesting). The full path still resolves, case-insensitively.
#[test]
fn a_bare_leaf_does_not_match_a_nested_folder_but_the_full_path_does() {
    let mut m = UrlMap::default();
    m.folder_indexes.insert("obsidian/notes".into(), "obsidian/notes".into());
    let idx = ArticleMapIndex::from_map(&m);
    assert_eq!(idx.resolve_reference_to_url("notes", "main.md"), None);
    assert_eq!(idx.resolve_reference_to_url("obsidian/notes", "main.md"), Some("/obsidian/notes/".into()));
    assert_eq!(idx.resolve_reference_to_url("Obsidian/NOTES", "main.md"), Some("/obsidian/notes/".into()));
}

/// The value is the URL a `url:` override produced, taken verbatim — not
/// re-derived from the directory's own on-disk name.
#[test]
fn folder_index_value_carries_the_override_not_the_directory_name() {
    let mut m = UrlMap::default();
    m.folder_indexes.insert("評選".into(), "awards".into());
    let idx = ArticleMapIndex::from_map(&m);
    assert_eq!(idx.resolve_reference_to_url("評選", "main.md"), Some("/awards/".into()));
}

/// A declared kind's namespace is not special here: the index reads
/// `generated` and `moved` as opaque strings, with no `authors`/`tags` branch
/// anywhere. Pinned so the next person adding a namespace can see that,
/// rather than inferring it from the absence of a branch.
#[test]
fn people_url_classifies_the_way_authors_url_does() {
    let mut m = UrlMap::default();
    m.articles.insert("ada-lin/".into(), article("ada-lin.md"));
    m.generated = vec!["authors/".into(), "authors/sam-okafor/".into(), "people/".into(), "people/sam-okafor/".into()];
    m.moved.insert("authors/ada-lin".into(), "ada-lin/".into());
    m.moved.insert("people/ada-lin".into(), "ada-lin/".into());
    let idx = ArticleMapIndex::from_map(&m);

    assert_eq!(idx.lookup_moved("/people/ada-lin/"), idx.lookup_moved("/authors/ada-lin/"));
    assert_eq!(idx.lookup_moved("/people/ada-lin/"), Some("/ada-lin/".into()));
    assert_eq!(idx.lookup_exact("/people/sam-okafor/"), idx.lookup_exact("/authors/sam-okafor/"));
    assert!(idx.lookup_exact("/people/sam-okafor/"));
    assert!(idx.lookup_exact("/people/"));
}
