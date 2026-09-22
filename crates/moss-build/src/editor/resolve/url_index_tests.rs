use super::*;
use crate::build::scan::article_map::ArticleInfo;
use std::collections::HashMap;

fn make_article(source_path: &str, url_path: &str) -> ArticleInfo {
    ArticleInfo {
        source_path: source_path.into(),
        title: String::new(),
        content: String::new(),
        html_content: None,
        frontmatter: HashMap::new(),
        url_path: url_path.into(),
        date: None,
        tags: vec![],
        uid: None,
    }
}

#[test]
fn exact_and_normalized_from_articles_and_pages() {
    let mut m = ArticleMap::new();
    m.articles
        .insert("research/".into(), make_article("Research.md", "research/"));
    m.pages.insert("news/".into(), "News/News.md".into());
    let idx = ArticleMapIndex::from_map(&m);
    assert!(idx.lookup_exact("research/"));
    assert!(idx.lookup_exact("news/"));
    assert!(!idx.lookup_exact("Research/"));
    assert_eq!(
        idx.lookup_normalized("/Research/"),
        Some("/research/".into())
    );
    assert_eq!(
        idx.resolve_reference_to_url("Research", "main.md"),
        Some("/research/".into())
    );
}

#[test]
fn ambiguous_normalized_returns_none() {
    let mut m = ArticleMap::new();
    m.articles
        .insert("research/".into(), make_article("Research.md", "research/"));
    m.articles
        .insert("RESEARCH/".into(), make_article("RESEARCH.md", "RESEARCH/"));
    let idx = ArticleMapIndex::from_map(&m);
    assert_eq!(idx.lookup_normalized("/research/"), None); // ambiguous → silent
}

// ── FolderFacts ────────────────────────────────────────────────────────

fn article_with(source_path: &str, fm: &[(&str, serde_json::Value)]) -> ArticleInfo {
    let mut a = make_article(source_path, "");
    a.frontmatter = fm.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
    a
}

fn scratch() -> tempfile::TempDir {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-tmp");
    std::fs::create_dir_all(&base).expect("create test-tmp base");
    tempfile::TempDir::new_in(&base).expect("tmp dir")
}

/// `awards/` holding two articles, a sub-folder page, and a grandchild article.
fn awards_map() -> ArticleMap {
    let mut m = ArticleMap::new();
    m.pages.insert("awards/".into(), "awards/index.md".into());
    m.pages
        .insert("awards/2024/".into(), "awards/2024/index.md".into());
    m.articles
        .insert("awards/a".into(), make_article("awards/a.md", "awards/a"));
    m.articles
        .insert("awards/b".into(), make_article("awards/b.md", "awards/b"));
    m.articles.insert(
        "awards/2024/x".into(),
        make_article("awards/2024/x.md", "awards/2024/x"),
    );
    m
}

#[test]
fn counts_split_direct_from_descendant() {
    let f = FolderFacts::from_map(&awards_map());
    let dir = scratch();
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
    m.articles.insert(
        "awards/draft".into(),
        article_with("awards/draft.md", &[("draft", serde_json::json!(true))]),
    );
    m.articles.insert(
        "awards/hidden".into(),
        article_with("awards/hidden.md", &[("listed", serde_json::json!(false))]),
    );
    let f = FolderFacts::from_map(&m);
    let dir = scratch();
    let info = f.lookup("awards").expect("awards");
    assert_eq!(info.direct_child_count, 3, "draft/unlisted must not count");
}

#[test]
fn also_in_counts_once_whether_or_not_it_lives_under_the_folder() {
    let mut m = awards_map();
    // Lives elsewhere, claims membership → a direct child AND a descendant.
    m.articles.insert(
        "posts/elsewhere".into(),
        article_with(
            "posts/elsewhere.md",
            &[("also_in", serde_json::json!(["awards"]))],
        ),
    );
    // Already under awards/ AND claims membership → counted ONCE (the build's
    // membership test is an OR, not a sum).
    m.articles.insert(
        "awards/both".into(),
        article_with(
            "awards/both.md",
            &[("also_in", serde_json::json!(["awards"]))],
        ),
    );
    let f = FolderFacts::from_map(&m);
    let dir = scratch();
    let info = f.lookup("awards").expect("awards");
    assert_eq!(info.direct_child_count, 5);
    assert_eq!(info.descendant_count, 5);
}

#[test]
fn slug_overridden_cjk_folder_is_found_by_its_on_disk_name() {
    // `獎項/獎項.md` carries `url: awards`, so the build published it at
    // `awards/`. The classifier hands back the author-typed on-disk path.
    let mut m = ArticleMap::new();
    m.pages.insert("awards/".into(), "獎項/獎項.md".into());
    m.articles
        .insert("awards/a".into(), make_article("獎項/a.md", "awards/a"));
    let f = FolderFacts::from_map(&m);
    let dir = scratch();
    let info = f
        .lookup("獎項")
        .expect("reverse source-dir lookup");
    assert_eq!(info.direct_child_count, 1);
    // …and the URL-space spelling finds the same folder.
    assert_eq!(
        f.lookup("/awards/")
            .expect("url-space")
            .direct_child_count,
        1
    );
}

#[test]
fn root_homepage_is_nobodys_child() {
    let mut m = ArticleMap::new();
    m.pages.insert("".into(), "index.md".into());
    m.pages.insert("awards/".into(), "awards/index.md".into());
    m.articles
        .insert("hello".into(), make_article("hello.md", "hello"));
    let f = FolderFacts::from_map(&m);
    let dir = scratch();
    let info = f.lookup("/").expect("root key");
    // The top-level article and the top-level folder page — not the homepage.
    assert_eq!(info.direct_child_count, 2);
    assert_eq!(info.descendant_count, 1);
}

#[test]
fn also_in_with_a_multibyte_folder_key_does_not_panic() {
    // REGRESSION: the ancestry walk must never byte-index a URL. `獎項/` is 7
    // bytes; `posts/hello` is 11 — an offset-based `&url[key.len()..]` lands
    // mid-sequence on other URLs and panics.
    let mut m = ArticleMap::new();
    m.pages.insert("獎項/".into(), "獎項/獎項.md".into());
    m.articles.insert(
        "posts/hello".into(),
        article_with("posts/hello.md", &[("also_in", serde_json::json!(["獎項"]))]),
    );
    let f = FolderFacts::from_map(&m);
    let dir = scratch();
    let info = f.lookup("獎項").expect("獎項");
    assert_eq!(info.direct_child_count, 1);
    assert_eq!(info.descendant_count, 1);
}

#[test]
fn unknown_folder_is_none_but_an_empty_known_folder_is_zero() {
    let mut m = ArticleMap::new();
    m.pages.insert("empty/".into(), "empty/index.md".into());
    let f = FolderFacts::from_map(&m);
    let dir = scratch();
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
    let mut m = ArticleMap::new();
    m.pages.insert("awards/".into(), "awards/index.md".into());
    m.page_titles.insert("awards/".into(), "獎項".into());
    m.pages.insert("news/".into(), "news/News.md".into());
    m.page_titles.insert("news/".into(), String::new());
    m.pages.insert("old/".into(), "old/index.md".into());
    let f = FolderFacts::from_map(&m);

    assert_eq!(f.lookup("awards").expect("awards").title.as_deref(), Some("獎項"));
    assert_eq!(f.lookup("news").expect("news").title, None, "empty title → None");
    assert_eq!(f.lookup("old").expect("old").title, None, "no record → None");
}

// ── Generated pages and moved terms ────────────────────────────────────

/// A synthesized index page (index-less folder, generated term page, the
/// namespace root) is a deployed URL with no source: it resolves exactly and
/// by case, but never by stem — there is nothing to open.
#[test]
fn generated_pages_resolve_without_a_source() {
    let mut m = ArticleMap::new();
    m.generated = vec!["writings/".into(), "authors/".into(), "authors/馬欣宜/".into()];
    let idx = ArticleMapIndex::from_map(&m);
    assert!(idx.lookup_exact("/writings/"));
    assert!(idx.lookup_exact("authors/馬欣宜"));
    assert_eq!(idx.lookup_normalized("/Writings/"), Some("/writings/".into()));
    assert_eq!(idx.resolve_reference_to_url("writings", "main.md"), None);
    assert_eq!(idx.lookup_moved("/writings/"), None);
}

/// A claimed term's generated URL is gone; the index points it at the claim
/// page. An unclaimed term is not moved, and its generated page still resolves.
#[test]
fn claimed_term_url_is_moved_to_the_claiming_page() {
    use crate::build::terms::TermSite;
    let mut m = ArticleMap::new();
    m.articles.insert("about/ma/".into(), make_article("關於/作者/ma.md", "about/ma/"));
    m.generated = vec!["authors/".into(), "authors/scarly/".into()];
    m.terms.insert("authors/馬欣宜".into(), TermSite { display: "馬欣宜".into(), claimed_by: Some("about/ma/".into()) });
    m.terms.insert("authors/scarly".into(), TermSite { display: "Scarly".into(), claimed_by: None });
    // The home page claiming a term: its pretty URL key is empty, the site root.
    m.terms.insert("tags/home".into(), TermSite { display: "Home".into(), claimed_by: Some(String::new()) });
    let idx = ArticleMapIndex::from_map(&m);
    assert_eq!(idx.lookup_moved("/tags/home/"), Some("/".into()));
    assert!(!idx.lookup_exact("/authors/馬欣宜/"), "the generated URL no longer exists");
    assert_eq!(idx.lookup_moved("/authors/馬欣宜/"), Some("/about/ma/".into()));
    assert_eq!(idx.lookup_moved("/authors/scarly/"), None);
    assert!(idx.lookup_exact("/authors/scarly/"));
}

/// A declared kind's namespace is not special here, and this file needs no
/// production change to serve one: it reads `generated` and `terms` as
/// opaque strings, with no `authors`/`tags` branch anywhere. Pinned as a
/// test so the next person adding a namespace can see that, rather than
/// inferring it from the absence of a branch.
#[test]
fn people_url_classifies_the_way_authors_url_does() {
    use crate::build::terms::TermSite;
    let mut m = ArticleMap::new();
    m.articles.insert("ada-lin/".into(), make_article("ada-lin.md", "ada-lin/"));
    m.generated = vec![
        "authors/".into(),
        "authors/sam-okafor/".into(),
        "people/".into(),
        "people/sam-okafor/".into(),
    ];
    m.terms.insert("authors/ada-lin".into(), TermSite { display: "Ada Lin".into(), claimed_by: Some("ada-lin/".into()) });
    m.terms.insert("people/ada-lin".into(), TermSite { display: "Ada Lin".into(), claimed_by: Some("ada-lin/".into()) });
    m.terms.insert("authors/sam-okafor".into(), TermSite { display: "Sam Okafor".into(), claimed_by: None });
    m.terms.insert("people/sam-okafor".into(), TermSite { display: "Sam Okafor".into(), claimed_by: None });
    let idx = ArticleMapIndex::from_map(&m);

    assert_eq!(idx.lookup_moved("/people/ada-lin/"), idx.lookup_moved("/authors/ada-lin/"));
    assert_eq!(idx.lookup_moved("/people/ada-lin/"), Some("/ada-lin/".into()));
    assert_eq!(
        idx.lookup_exact("/people/sam-okafor/"),
        idx.lookup_exact("/authors/sam-okafor/")
    );
    assert!(idx.lookup_exact("/people/sam-okafor/"));
    assert!(idx.lookup_exact("/people/"));
}
