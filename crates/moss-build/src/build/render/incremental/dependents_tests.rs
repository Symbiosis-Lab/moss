//! Unit tests for the pure per-channel functions `verdict::compute` calls.
//! The end-to-end narrowing behavior (which pages actually end up in the
//! render set) is pinned in `verdict_tests.rs`, against the real `compute`
//! entry point — these tests are about each channel's own digest/predicate
//! in isolation, so a channel's bug shows up here without a full verdict run.

use super::*;
use crate::i18n::Language;
use moss_core::PageKind;

fn doc(url: &str, kind: PageKind) -> ParsedDocument {
    let stem = url.trim_end_matches("/index.html").rsplit('/').next().unwrap_or(url);
    ParsedDocument {
        url_path: url.to_string(),
        source_path: Some(if url == "index.html" { "index.md".to_string() } else { url.replace("index.html", "index.md") }),
        title: stem.to_string(),
        label: stem.to_string(),
        clean_stem: stem.to_string(),
        kind,
        ..Default::default()
    }
}

#[test]
fn effective_lang_is_the_site_language_outside_any_language_tree() {
    let mut d = doc("writings/alpha/index.html", PageKind::Article);
    d.lang = Language::ZhHans; // content-detected, no language-tree folder
    assert_eq!(effective_lang(&d, Language::En), Language::En);
}

#[test]
fn effective_lang_follows_the_documents_own_lang_inside_its_tree() {
    let mut d = doc("zh-hans/alpha/index.html", PageKind::Article);
    d.lang = Language::ZhHans;
    assert_eq!(effective_lang(&d, Language::En), Language::ZhHans);
}

#[test]
fn nav_globals_buckets_by_the_documents_own_lang_and_hashes_order_and_weight() {
    let mut en_item = doc("about/index.html", PageKind::Article);
    en_item.nav = Some(true);
    en_item.weight = Some(1);
    let mut zh_item = doc("zh-hans/about/index.html", PageKind::Article);
    zh_item.nav = Some(true);
    zh_item.lang = Language::ZhHans;
    let not_eligible = doc("writings/alpha/index.html", PageKind::Article); // nested, no override

    let docs = vec![en_item.clone(), zh_item.clone(), not_eligible];
    let globals = nav_globals(&docs, true);
    assert_eq!(globals.len(), 2, "one bucket per language that actually has a nav item: {globals:?}");
    assert!(globals.contains_key("en"));
    assert!(globals.contains_key("zh-hans"));

    // A weight-only edit moves the "en" digest and nothing else.
    let mut reweighed = docs_from(&[en_item.clone(), zh_item.clone()]);
    reweighed[0].weight = Some(2);
    let after = nav_globals(&reweighed, true);
    assert_ne!(globals["en"], after["en"]);
    assert_eq!(globals["zh-hans"], after["zh-hans"]);
}

fn docs_from(items: &[ParsedDocument]) -> Vec<ParsedDocument> {
    items.to_vec()
}

/// `lang` is excluded from the per-page surface (`facade::surface_debug`),
/// so nothing about a reassigned document's OWN fingerprint can tell a
/// caller its language moved. Reading
/// `doc.lang` fresh here — never through the surface — is what lets the two
/// affected buckets differ from a plain recomputation.
#[test]
fn nav_globals_moves_both_the_left_and_the_joined_language_bucket() {
    let mut item = doc("about/index.html", PageKind::Article);
    item.nav = Some(true);
    let before = nav_globals(&[item.clone()], true);
    item.lang = Language::ZhHans;
    let after = nav_globals(&[item], true);

    assert!(before.contains_key("en") && !before.contains_key("zh-hans"));
    assert!(after.contains_key("zh-hans") && !after.contains_key("en"));
}

#[test]
fn home_title_globals_is_per_language_and_empty_string_when_absent() {
    let mut root = doc("index.html", PageKind::Folder);
    root.title = "Home".to_string();
    let mut zh_home = doc("zh-hans/index.html", PageKind::Folder);
    zh_home.title = "首页".to_string();
    zh_home.lang = Language::ZhHans;
    let docs = vec![root, zh_home];

    let globals = home_title_globals(&docs, Language::En);
    // zh-hant has no home page at all — still present, with a stable digest
    // over the empty string, so a LATER arrival of a zh-hant home is a real
    // move rather than a key that silently never existed.
    assert_eq!(globals.len(), 3);
    assert_ne!(globals["en"], globals["zh-hans"]);
    assert_eq!(globals["zh-hant"], debug_hash(&""));
}

#[test]
fn home_title_globals_changes_only_the_edited_languages_key() {
    let mut root = doc("index.html", PageKind::Folder);
    root.title = "Home".to_string();
    let before = home_title_globals(&[root.clone()], Language::En);
    root.title = "New Home".to_string();
    let after = home_title_globals(&[root], Language::En);
    assert_ne!(before["en"], after["en"]);
    assert_eq!(before["zh-hans"], after["zh-hans"]);
}

#[test]
fn home_breadcrumb_globals_is_one_global_key_not_per_language() {
    let mut root = doc("index.html", PageKind::Folder);
    root.breadcrumb = Some(true);
    let globals = home_breadcrumb_globals(&[root]);
    assert_eq!(globals.keys().collect::<Vec<_>>(), vec!["site"]);
}

#[test]
fn home_breadcrumb_globals_ignores_a_non_root_homes_own_field() {
    // Only the LITERAL root `index.html` is read — `compute_breadcrumb_segments`
    // never consults a per-language home's own `breadcrumb:` field (see the
    // function's doc comment), so this must not move the digest.
    let mut root = doc("index.html", PageKind::Folder);
    root.breadcrumb = Some(false);
    let mut zh_home = doc("zh-hans/index.html", PageKind::Folder);
    zh_home.lang = Language::ZhHans;
    zh_home.breadcrumb = Some(true);
    let before = home_breadcrumb_globals(&[root.clone(), zh_home.clone()]);
    zh_home.breadcrumb = Some(false);
    let after = home_breadcrumb_globals(&[root, zh_home]);
    assert_eq!(before, after);
}

#[test]
fn breadcrumb_ancestor_descendants_is_scoped_to_the_folders_own_prefix() {
    let mut root = doc("index.html", PageKind::Folder);
    root.breadcrumb = Some(true);
    let folder = doc("writings/index.html", PageKind::Folder);
    let child = doc("writings/alpha/index.html", PageKind::Article);
    let other_folder = doc("other/index.html", PageKind::Folder);
    let other_child = doc("other/leaf/index.html", PageKind::Article);
    let docs = vec![root, folder.clone(), child, other_folder, other_child];

    let extra = breadcrumb_ancestor_descendants(&folder, &docs, true);
    assert_eq!(extra, vec!["writings/alpha/index.md".to_string()]);
}

#[test]
fn breadcrumb_ancestor_descendants_is_empty_when_breadcrumbs_are_off() {
    let root = doc("index.html", PageKind::Folder); // breadcrumb: None, nav empty => auto-enable would be true...
    let mut folder = doc("writings/index.html", PageKind::Folder);
    folder.nav = Some(true); // ...so give the corpus a nav item to auto-disable breadcrumbs
    let child = doc("writings/alpha/index.html", PageKind::Article);
    let docs = vec![root, folder.clone(), child];
    assert!(breadcrumb_ancestor_descendants(&folder, &docs, true).is_empty());
}

#[test]
fn field_is_classified_guards_label_and_weight_on_a_footer_linked_page() {
    let mut footer_page = doc("about/index.html", PageKind::Article);
    footer_page.footer = Some(true);
    let ordinary = doc("about/index.html", PageKind::Article);

    for field in ["label", "weight"] {
        assert!(!field_is_classified(&footer_page, field), "{field} must fall back to Full on a footer-linked page");
        assert!(field_is_classified(&ordinary, field), "{field} is safe off the footer link list");
    }
}

#[test]
fn field_is_classified_guards_series_only_on_a_folder_document() {
    let folder = doc("series/index.html", PageKind::Folder);
    let article = doc("series/part-1/index.html", PageKind::Article);
    assert!(!field_is_classified(&folder, "series"), "a series parent's own chrome toggle reaches every child through no digest");
    assert!(field_is_classified(&article, "series"), "a member's own opt-out is covered by the listing group's blanket inclusion");
}

#[test]
fn field_is_classified_defaults_unknown_fields_to_unsafe() {
    let d = doc("about/index.html", PageKind::Article);
    for field in ["translations", "children_source", "sidebar", "term_listing", "is_home_override", "also_in"] {
        assert!(!field_is_classified(&d, field), "{field} must stay in the Full fallback");
    }
}
