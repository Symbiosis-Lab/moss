//! Narrowing `FullCause::SurfaceChanged` from "render everything" to "render
//! what this specific move can reach" — see `verdict.rs`'s module doc for the
//! six non-graph channels a page's surface is load-bearing for.
//!
//! Three of the six are handled elsewhere and need nothing from this module:
//! **series siblings** and **folder-embed listings** both already have a
//! digest — `render::incremental::listing`'s group model — so once
//! `verdict::compute` stops returning early on a non-empty `surface_changed`,
//! its existing backlink/listing-group loop starts covering them for free
//! (series siblings needed one gate widened first — see
//! `listing::is_series_member`). **Translation counterparts** is not
//! narrowed at all; it is exactly the unclassified fallback
//! [`field_is_classified`] falls back to.
//!
//! The other three — site nav, homepage title, and the breadcrumb ancestor
//! trail (plus the homepage's own site-wide breadcrumb toggle) — have no
//! existing digest, because nothing upstream needed one: they are read by a
//! per-language, whole-corpus scan (nav, homepage title, breadcrumb
//! enable/disable) or a URL-prefix descendant scan (breadcrumb ancestor
//! titles), neither of which is an edge the dependency graph or the listing
//! group model can express. This module computes exactly those.

use std::collections::BTreeMap;

use crate::build::components::nav::{compute_breadcrumb_segments, is_nav_bar_item_doc};
use crate::build::facade::debug_hash;
use crate::build::types::ParsedDocument;
use crate::i18n::Language;

/// The language whose chrome `doc` shows — the same rule
/// `NavigationBuilder::effective_lang` applies per render
/// (`components/nav.rs`), re-derived here from the document alone so the
/// verdict can group documents by it without building a navigation for each
/// one. A page only leaves the site language's chrome when it actually lives
/// under a recognized language-tree folder; content-detected language alone
/// must not move it (an English-titled page with no `en/` tree would
/// otherwise get an empty nav and a home link nothing serves).
pub fn effective_lang(doc: &ParsedDocument, site_lang: Language) -> Language {
    match moss_core::home::lang_tree_prefix(&doc.url_path) {
        Some(_) => doc.lang,
        None => site_lang,
    }
}

/// One nav-eligible document's membership/display fields, in the shape
/// [`nav_globals`] hashes. `url_path` is included only so two documents that
/// otherwise tie don't collide in the `Debug` rendering — it is identity, not
/// a rendered field.
#[allow(dead_code)]
#[derive(Debug)]
struct NavEntry<'a> {
    url_path: &'a str,
    weight: Option<i32>,
    label: &'a str,
}

/// Per-language digest of "which documents does this language's nav bar
/// show, and in what order/label" — every field `is_nav_bar_item_doc`
/// branches on, plus the two display fields (`weight`, `label`) the bar
/// renders. Recomputed from scratch every build and compared against the
/// PREVIOUS build's map the same way `lang_globals`/`listing_globals`
/// already are (`facade.rs`): there is no per-page diff to take here, only
/// "does this language's answer differ from last time".
///
/// Grouped by `doc.lang` directly rather than by `effective_lang` — the
/// surface blanks `lang` out entirely (`facade::surface_debug`), so a nav
/// item's language reassignment moves neither its own facade nor its own
/// surface. Reading `lang` fresh here, unblanked, is what makes a `lang` edit
/// on a nav item detectable at all: the document leaves one language's list
/// and joins another's, so both keys move.
pub fn nav_globals(documents: &[ParsedDocument], has_content_folders: bool) -> BTreeMap<String, String> {
    let mut by_lang: BTreeMap<String, Vec<NavEntry<'_>>> = BTreeMap::new();
    for doc in documents {
        if !is_nav_bar_item_doc(doc, has_content_folders) {
            continue;
        }
        by_lang.entry(doc.lang.code().to_string()).or_default().push(NavEntry {
            url_path: &doc.url_path,
            weight: doc.weight,
            label: &doc.label,
        });
    }
    by_lang
        .into_iter()
        .map(|(lang, mut entries)| {
            entries.sort_by(|a, b| a.url_path.cmp(b.url_path));
            (lang, debug_hash(&entries))
        })
        .collect()
}

/// The document that serves as `lang`'s home page: `index.html` for the
/// site's own language, `{lang.code()}/index.html` for any other —
/// mirrors `render::html::find_homepage_doc` exactly (duplicated rather than
/// shared because that function also falls back to the site homepage for a
/// missing translation, which the two callers here must NOT do: a per-language
/// digest that fell back would silently alias two languages onto one key).
fn home_doc_for_lang<'a>(
    documents: &'a [ParsedDocument],
    lang: Language,
    site_lang: Language,
) -> Option<&'a ParsedDocument> {
    if lang == site_lang {
        documents.iter().find(|d| d.url_path == "index.html")
    } else {
        let path = format!("{}/index.html", lang.code());
        documents.iter().find(|d| d.url_path == path)
    }
}

/// Every language a document could plausibly declare — used to seed the two
/// per-lang home digests below so a language's key still moves when its home
/// page is DELETED (present last build, absent this one), not only when it is
/// edited. `Language` has exactly three interface variants (`i18n.rs`), so
/// this is a fixed, cheap enumeration rather than a corpus scan.
fn every_language() -> [Language; 3] {
    [Language::En, Language::ZhHans, Language::ZhHant]
}

/// Per-language digest of that language's home-page title —
/// [`render::html::localized_site_title`]'s exact input for a non-site
/// language, and `LayoutConfig`'s `site_name` for the site's own. Empty
/// string (a stable, hashable value) when the language has no home page at
/// all, so a home page's arrival or removal also moves the key.
pub fn home_title_globals(documents: &[ParsedDocument], site_lang: Language) -> BTreeMap<String, String> {
    every_language()
        .into_iter()
        .map(|lang| {
            let title = home_doc_for_lang(documents, lang, site_lang).map(|d| d.title.as_str()).unwrap_or("");
            (lang.code().to_string(), debug_hash(&title))
        })
        .collect()
}

/// Digest of the ONE global breadcrumb enable/disable toggle —
/// `compute_breadcrumb_segments` (`components/nav.rs`) reads `all_docs`'s
/// literal `index.html` (the root homepage) for this on every page it
/// renders, for every language: there is no per-language toggle to key by,
/// unlike [`nav_globals`] and [`home_title_globals`]. A single-entry map
/// rather than a bool so the caller (`verdict::compute`) can diff it with the
/// same `moved_keys` shape `FacadeCache`'s other globals use; a non-empty
/// diff widens to every page site-wide, not to one language's bucket.
pub fn home_breadcrumb_globals(documents: &[ParsedDocument]) -> BTreeMap<String, String> {
    let flag = documents.iter().find(|d| d.url_path == "index.html").and_then(|d| d.breadcrumb);
    BTreeMap::from([("site".to_string(), debug_hash(&flag))])
}

/// Every OTHER document whose breadcrumb trail names `folder`'s label as an
/// ancestor segment — the descendants `compute_breadcrumb_segments`
/// (`components/nav.rs`) walks back up to when it renders THEIR trail, which
/// is the only reason a folder-index's `label` edit reaches a page other than
/// itself. Filtered to documents that actually show a breadcrumb trail today
/// (an empty `site_title` is passed through: it only names the trail's first
/// segment, never whether one is shown at all, so it cannot change the
/// answer here).
pub fn breadcrumb_ancestor_descendants(
    folder: &ParsedDocument,
    documents: &[ParsedDocument],
    has_content_folders: bool,
) -> Vec<String> {
    let prefix = match folder.url_path.strip_suffix("index.html") {
        Some(p) => p.to_string(),
        None => return Vec::new(),
    };
    documents
        .iter()
        .filter(|d| d.url_path != folder.url_path && d.url_path.starts_with(&prefix))
        // `force: d.is_place_namespace_root` — an explorer root's breadcrumb
        // is forced on independent of the site's own setting (`components/
        // nav.rs`'s own doc), so a descendant that happens to be one must be
        // treated as showing a trail today even when the site-wide answer
        // alone would have said no.
        .filter(|d| compute_breadcrumb_segments(d, documents, "", has_content_folders, d.is_place_namespace_root).is_some())
        .filter_map(|d| d.source_path.clone())
        .collect()
}

/// Frontmatter keys that already have their own dedicated, separately
/// -surfaced struct field (`weight`, `label`, …), so their copy inside
/// `raw_frontmatter` carries no information [`field_is_classified`] does not
/// already see under the field's own name.
///
/// `facade::surface_debug` strips exactly these keys out of a page's
/// `raw_frontmatter` before hashing it (see
/// [`strip_typed_frontmatter_keys`]) — without that, editing only `weight:`
/// would ALSO move the opaque, un-decomposable `raw_frontmatter` field
/// (`raw_frontmatter` is a whole-map catch-all kept specifically so a
/// plugin-only key like `syndicated:` still reaches the surface; see that
/// field's own doc comment), and an opaque move can never be classified —
/// every edit to a nav-eligible page's weight would still force
/// `FullCause::SurfaceChanged` despite `nav_globals` correctly narrowing the
/// named `weight` field right next to it.
const TYPED_FRONTMATTER_KEYS: &[&str] =
    &["nav", "draft", "weight", "label", "title", "breadcrumb", "date", "series"];

/// Remove [`TYPED_FRONTMATTER_KEYS`] from a page's raw-frontmatter map before
/// it is hashed into the surface. Called from `facade::surface_debug` only —
/// `facade::compute_page_facade` must keep every key, because a page's own
/// re-render still has to fire on any frontmatter edit whatsoever.
pub fn strip_typed_frontmatter_keys(raw: &mut std::collections::BTreeMap<String, serde_json::Value>) {
    raw.retain(|k, _| !TYPED_FRONTMATTER_KEYS.contains(&k.as_str()));
}

/// Is `field`'s move on `doc` fully accounted for by a channel this module
/// (or the pre-existing listing-group/backlink machinery) models? `false`
/// means the safety fallback applies: the caller must render everything,
/// exactly as it did before this narrowing existed.
///
/// This is deliberately NOT a flat allow-list of field names. `label` and
/// `weight` also drive the auto-generated footer link list
/// (`components/nav.rs::generate_footer`, keyed on `footer: true` rather than
/// nav eligibility) — a whole-corpus, every-page channel nothing here
/// models. A `footer: true` document is excluded from the classified set for
/// exactly those two fields so that gap stays on the safe (Full) side rather
/// than silently shipping a stale footer link on every OTHER page.
/// `series` similarly turns unsafe on a folder document, because a series
/// parent's OWN `series:`/chrome-default is read by every child's rendered
/// prev/next block through no digest at all — only a listed CHILD's `series`
/// (its own opt-out) is covered, via the group-digest loop's blanket
/// inclusion of every non-blanked field (`listing::project_child`).
pub fn field_is_classified(doc: &ParsedDocument, field: &str) -> bool {
    match field {
        "nav" | "draft" | "slot_only" | "is_root_level" | "clean_stem" => true,
        "label" | "weight" => doc.footer != Some(true),
        "title" | "breadcrumb" | "date" => true,
        "series" => doc.kind != moss_core::PageKind::Folder,
        _ => false,
    }
}

#[cfg(test)]
#[path = "dependents_tests.rs"]
mod tests;
