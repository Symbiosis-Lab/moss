//! `.moss/build.nosync/inventory.json` — the build's own account of every document it
//! parsed, whether or not that document became a page.
//!
//! ## Why the build owns this file
//!
//! The signals that answer "why is this page not in the listing?" — `kind`,
//! `draft`, `listed`, `nav`, `slot_only` — exist together in exactly one place:
//! on [`ParsedDocument`], inside the build. Nothing downstream can rebuild
//! them. `article-map.json` cannot: `build_article_map` routes folder pages and
//! the root index into a bare url→path map with no title and no date, and drops
//! slot-only documents entirely.
//!
//! So the build writes the rows here, and `moss list` reads the file back and
//! renders it. That split is the design: this module is a build output, the
//! presentation half (`cli::list`) is a CLI surface, and the JSON on disk is
//! the contract between them. It used to live whole in `cli::list`, which made
//! the build tree call into the CLI to write its own output.
//!
//! It sits in `emit/` because that is what it is: one generated-artifact
//! family owned end to end — projection, serialization and disk write — with a
//! single entry point `blocking.rs` merely gates.
//!
//! [`ParsedDocument`]: crate::build::types::ParsedDocument

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One row of the inventory: a document the build parsed, whether or not it
/// became a page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryEntry {
    /// Project-root-relative source file, e.g. `works/《河灣》第一期 — 遠行.md`.
    /// `None` for synthesized pages that have no markdown file behind them.
    pub source_path: Option<String>,
    /// Where it publishes, relative to the site root, e.g.
    /// `works/河灣第一期-遠行/index.html`. Empty for a slot-only document,
    /// which is not published at a URL at all.
    pub url_path: String,
    /// The chrome label (nav, breadcrumb, cards) — `label`, not the body H1.
    pub title: String,
    /// Frontmatter `date:` as written, else the day an event page starts.
    pub date: Option<String>,
    /// `article` or `folder` (a folder index page).
    pub kind: String,
    /// `draft: true` — built for preview, never published, and hidden from
    /// every listing, feed, the sitemap and nav.
    pub draft: bool,
    /// False only for `listed: false` — out of generated listings/feeds, but
    /// still a public, indexable page. Absent frontmatter means listed.
    pub listed: bool,
    /// Frontmatter `nav:` as written: `Some(true)` forces it into the nav bar,
    /// `Some(false)` keeps it out, `None` leaves it to the adaptive default.
    pub nav: Option<bool>,
    /// Layout chrome (`footer.md`, or `slot:` frontmatter) — fills a slot in
    /// the template instead of being a page.
    pub slot_only: bool,
    /// Why this document is absent from generated article listings, in the
    /// vocabulary the author would have to change to fix it. Empty when it
    /// is listed. See [`hidden_reasons`].
    pub hidden: Vec<String>,
    /// The BCP-47 tag this page will carry in `<html lang>`: the document's
    /// own language, not the interface language (a `ja/` tree reads `ja` here
    /// while its chrome stays the site default's). It answers "did moss
    /// register my new edition?": a language folder moss does not recognize is
    /// built as ordinary content, and only this column shows the difference.
    ///
    /// `#[serde(default)]`, as on every field added here: one build writes
    /// this file and a later `moss list`, possibly a newer moss, reads it, so
    /// a new field must not make an older inventory fail to parse.
    #[serde(default)]
    pub lang: String,
    /// What the build picked for each field below, keyed by frontmatter name:
    /// the page's own value where it sets one, else the automatic one. An
    /// editor shows it beside a field the file leaves out, so the author sees
    /// what adding the field would change. `#[serde(default)]` for the same
    /// cross-version reason as `lang`.
    ///
    /// Covers fields whose value is a pure function of the page and site:
    /// `title`, `url`, `layout`, `lang`, `nav`, `typesetting`, `listed`,
    /// `children`, and on a listing page `sort` and `children_depth`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub resolved: BTreeMap<String, serde_json::Value>,
    /// Where the other-language versions of this page publish: the
    /// translations the build linked by `translationKey`, filename suffix or
    /// mirrored path. Without it an agent has to re-derive all three rules
    /// from the source files to tell which rows are one piece. Empty for a
    /// page in one language. `#[serde(default)]` for the same cross-version
    /// reason as `lang`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub translations: Vec<String>,
    /// Where the page sits, which decides the fields that apply to it. `None`
    /// for a slot, which is not a page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<moss_core::schema::PagePosition>,
}

/// The build's own home election: the root of the site or of a language tree
/// is a home, another folder index is a folder, the rest are pages.
fn position(doc: &crate::build::types::ParsedDocument) -> Option<moss_core::schema::PagePosition> {
    use moss_core::schema::PagePosition;
    if doc.slot_only {
        None
    } else if crate::build::render::lang_roots::is_language_root(&doc.url_path) {
        Some(PagePosition::Home)
    } else if doc.kind == moss_core::PageKind::Folder {
        Some(PagePosition::Folder)
    } else {
        Some(PagePosition::Page)
    }
}

/// The values for [`InventoryEntry::resolved`]. A slot is not a page, so it
/// has none.
fn resolved_values(
    doc: &crate::build::types::ParsedDocument,
    position: Option<moss_core::schema::PagePosition>,
    has_content_folders: bool,
    lang: &str,
    site_typesetting: Option<&str>,
) -> BTreeMap<String, serde_json::Value> {
    use crate::build::page::shell::{ShellRegistry, ShellType};
    use serde_json::json;
    use moss_core::schema::PagePosition;
    let mut values = BTreeMap::new();
    let Some(position) = position else { return values };
    let is_home = position == PagePosition::Home;
    let lists = position != PagePosition::Page;
    let layout = match ShellRegistry::select_shell_type(Some(doc), is_home, has_content_folders) {
        ShellType::Page => "page",
        ShellType::Article => "article",
    };
    let typesetting = crate::build::render::config::effective_typesetting(doc.typesetting.as_deref(), site_typesetting);
    values.insert("title".into(), json!(doc.label));
    if !is_home {
        if let Some(slug) = doc.url_path.strip_suffix("/index.html").and_then(|dir| dir.rsplit('/').next()) {
            values.insert("url".into(), json!(slug));
        }
    }
    values.insert("layout".into(), json!(layout));
    values.insert("lang".into(), json!(lang));
    values.insert("nav".into(), json!(crate::build::components::nav::is_nav_bar_item_doc(doc, has_content_folders)));
    values.insert("typesetting".into(), json!(typesetting.unwrap_or("horizontal")));
    values.insert("listed".into(), json!(doc.listed != Some(false)));
    values.insert("children".into(), json!(lists));
    if lists {
        values.insert("sort".into(), json!(doc.resolve_for_direct_children().axis));
        values.insert("children_depth".into(), json!(if is_home { "all" } else { "direct" }));
    }
    values
}

/// `.moss/build.nosync/inventory.json` — written by the build, read by `moss list`.
///
/// Through `MossPaths::build_dir()`, not a bare join: a cloud sync client's
/// rename-aside mid-build must not send this file to the decoy while the
/// held root handle already knows where the real one went.
pub fn inventory_path(moss_dir: &Path) -> PathBuf {
    crate::moss_paths::MossPaths::from_moss_dir(moss_dir.to_path_buf()).build_dir().join("inventory.json")
}

/// Why `doc` will not appear in a generated article listing.
///
/// One string per mechanism, because they are genuinely independent: a file
/// can be both a draft and a nav item, and clearing one of them changes
/// nothing. The names are the frontmatter the author would edit, so the
/// report is directly actionable.
fn hidden_reasons(
    doc: &crate::build::types::ParsedDocument,
    has_content_folders: bool,
) -> Vec<String> {
    let mut reasons = Vec::new();
    if doc.slot_only {
        // Not a page at all — it renders into a layout slot. Reported first
        // because it subsumes the rest: the other flags are moot for chrome.
        reasons.push("slot".to_string());
    }
    if doc.draft == Some(true) {
        reasons.push("draft".to_string());
    }
    if doc.listed == Some(false) {
        reasons.push("unlisted".to_string());
    }
    if !doc.slot_only
        && crate::build::components::nav::is_listing_nav_item(doc, has_content_folders)
    {
        // The one hiding mechanism with no frontmatter behind it: in a site
        // that has content folders, every root-level non-index file is a nav
        // item, and nav items are excluded from the root article listing.
        reasons.push("nav-item".to_string());
    }
    reasons
}

/// Project the build's documents into the inventory rows.
///
/// Pure: takes the parsed documents, returns the rows. The I/O is
/// [`write_inventory`].
pub fn build_inventory(
    documents: &[crate::build::types::ParsedDocument],
    has_content_folders: bool,
    site_lang_tag: &str,
    site_typesetting: Option<&str>,
) -> Vec<InventoryEntry> {
    let mut rows: Vec<InventoryEntry> = documents
        .iter()
        .map(|doc| {
            // Same precedence the renderer uses (`html.rs`), so this column
            // reports what the page will actually say rather than a second
            // guess at it — including the fallback, which is the SITE's tag
            // and not the page's UI language (a `fr` site serving
            // English chrome still says `lang="fr"`).
            let lang = doc.lang_tag.clone().unwrap_or_else(|| site_lang_tag.to_string());
            InventoryEntry {
                source_path: doc.source_path.clone(),
                url_path: if doc.slot_only { String::new() } else { doc.url_path.clone() },
                title: doc.label.clone(),
                date: doc.date.clone().or_else(|| doc.event.as_ref().map(|e| e.start.day_key())),
                kind: match doc.kind {
                    moss_core::PageKind::Folder => "folder".to_string(),
                    moss_core::PageKind::Article => "article".to_string(),
                },
                draft: doc.draft == Some(true),
                listed: doc.listed != Some(false),
                nav: doc.nav,
                slot_only: doc.slot_only,
                hidden: hidden_reasons(doc, has_content_folders),
                resolved: resolved_values(doc, position(doc), has_content_folders, &lang, site_typesetting),
                translations: doc.translations.iter().map(|t| t.url_path.clone()).collect(),
                position: position(doc),
                lang,
            }
        })
        .collect();
    // Sorted by URL so two builds of the same site produce the same file and
    // a diff of the inventory is a diff of the site's routing.
    rows.sort_by(|a, b| (&a.url_path, &a.source_path).cmp(&(&b.url_path, &b.source_path)));
    rows
}

/// Write `.moss/build.nosync/inventory.json` from the build's parsed documents.
///
/// Called once per build, right where the article map is saved. Cheap: it
/// clones a handful of short strings per document and serializes them.
///
/// Goes through `build::io_utils` because everything landing under
/// `.moss/build.nosync/` must — a raw `fs::write` there fails against a
/// cloud-evicted destination and is caught by `output_write_invariant_test`.
pub fn write_inventory(
    documents: &[crate::build::types::ParsedDocument],
    moss_dir: &Path,
    has_content_folders: bool,
    site_lang_tag: &str,
    site_typesetting: Option<&str>,
) -> Result<(), String> {
    let rows = build_inventory(documents, has_content_folders, site_lang_tag, site_typesetting);
    let json = serde_json::to_string_pretty(&rows)
        .map_err(|e| format!("Failed to serialize inventory: {}", e))?;
    crate::build::io_utils::write_output(&inventory_path(moss_dir), json.as_bytes())
        .map_err(|e| format!("Failed to write inventory: {}", e))
}

/// What the last build knows about one source file, for an editor's add list:
/// where the page sits, which decides the fields that apply, and the value the
/// build gave each field the file leaves out.
#[derive(Debug, Default, Serialize, specta::Type)]
pub struct PageFacts {
    /// `None` for a slot, which is not a page.
    pub position: Option<moss_core::schema::PagePosition>,
    pub auto: BTreeMap<String, serde_json::Value>,
}

/// The [`PageFacts`] of one source file. A file no build has reached yet, or
/// a missing or unreadable inventory, gives none: they are hints, and the next
/// build supplies them.
pub fn page_facts_for_source(project_root: &Path, file_path: &str) -> Result<PageFacts, String> {
    let key = moss_core::content_graph::normalize_path(&crate::editor::content::project_relative(project_root, file_path)?);
    let Ok(bytes) = std::fs::read(inventory_path(&project_root.join(".moss"))) else { return Ok(PageFacts::default()) };
    let rows: Vec<InventoryEntry> = serde_json::from_slice(&bytes).unwrap_or_default();
    Ok(rows
        .into_iter()
        .find(|r| r.source_path.as_deref().is_some_and(|p| moss_core::content_graph::normalize_path(p) == key))
        .map(|r| PageFacts { position: r.position, auto: r.resolved })
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::types::{EventFields, ParsedDocument};
    use moss_core::frontmatter_typed::FrontMatter;
    use moss_core::schema::PagePosition;

    fn event_doc(start: &str, date: Option<&str>) -> ParsedDocument {
        let fm = FrontMatter { start: Some(start.to_string()), ..Default::default() };
        ParsedDocument {
            url_path: "work/example-show/index.html".to_string(),
            date: date.map(str::to_string),
            event: EventFields::from_frontmatter(&fm),
            ..Default::default()
        }
    }

    #[test]
    fn an_event_page_without_a_date_reports_its_start_day() {
        let rows = build_inventory(&[event_doc("2026-11-03 19:30", None)], false, "en", None);
        assert_eq!(rows[0].date.as_deref(), Some("2026-11-03"));
    }

    #[test]
    fn a_page_date_wins_over_the_event_start() {
        let rows = build_inventory(&[event_doc("2026-11-03", Some("2026-10-01"))], false, "en", None);
        assert_eq!(rows[0].date.as_deref(), Some("2026-10-01"));
    }

    fn resolved_for(url_path: &str, kind: moss_core::PageKind, site_typesetting: Option<&str>) -> BTreeMap<String, serde_json::Value> {
        let doc = ParsedDocument {
            url_path: url_path.to_string(),
            kind,
            label: "Label".to_string(),
            is_root_level: url_path.matches('/').count() == 1,
            ..Default::default()
        };
        build_inventory(&[doc], true, "en", site_typesetting).remove(0).resolved
    }

    #[test]
    fn an_ordinary_page_resolves_to_an_article_under_its_slug() {
        let r = resolved_for("works/the-tyger/index.html", moss_core::PageKind::Article, None);
        assert_eq!(r["layout"], "article");
        assert_eq!(r["url"], "the-tyger");
        assert_eq!(r["title"], "Label");
        assert_eq!(r["lang"], "en");
        assert_eq!(r["nav"], false);
        assert_eq!(r["children"], false);
        assert_eq!(r["typesetting"], "horizontal");
        assert!(!r.contains_key("sort") && !r.contains_key("children_depth"));
    }

    #[test]
    fn a_top_level_page_of_an_organized_site_is_in_the_nav_and_still_an_article() {
        let r = resolved_for("about/index.html", moss_core::PageKind::Article, None);
        assert_eq!(r["nav"], true);
        assert_eq!(r["layout"], "article");
    }

    #[test]
    fn a_folder_index_lists_its_direct_children_by_date() {
        let r = resolved_for("works/index.html", moss_core::PageKind::Folder, None);
        assert_eq!(r["layout"], "page");
        assert_eq!(r["children"], true);
        assert_eq!(r["sort"], "date");
        assert_eq!(r["children_depth"], "direct");
    }

    #[test]
    fn the_home_lists_everything_and_has_no_slug() {
        let r = resolved_for("index.html", moss_core::PageKind::Folder, None);
        assert_eq!(r["layout"], "page");
        assert_eq!(r["children_depth"], "all");
        assert!(!r.contains_key("url"));
    }

    #[test]
    fn typesetting_follows_the_site_setting() {
        assert_eq!(resolved_for("a/b/index.html", moss_core::PageKind::Article, Some("vertical"))["typesetting"], "vertical");
    }

    #[test]
    fn a_slot_resolves_nothing() {
        let doc = ParsedDocument { slot_only: true, url_path: "footer/index.html".into(), ..Default::default() };
        assert!(build_inventory(&[doc], true, "en", None)[0].resolved.is_empty());
    }

    #[test]
    fn an_editor_reads_the_row_of_the_file_it_has_open() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = ParsedDocument {
            source_path: Some("works/The Tyger.md".into()),
            url_path: "works/the-tyger/index.html".into(),
            ..Default::default()
        };
        write_inventory(&[doc], &tmp.path().join(".moss"), true, "en", None).unwrap();
        let open = tmp.path().join("works/The Tyger.md");
        let facts = page_facts_for_source(tmp.path(), &open.to_string_lossy()).unwrap();
        assert_eq!(facts.auto["url"], "the-tyger");
        assert_eq!(facts.position, Some(PagePosition::Page));
        let unbuilt = page_facts_for_source(tmp.path(), &tmp.path().join("new.md").to_string_lossy()).unwrap();
        assert!(unbuilt.auto.is_empty() && unbuilt.position.is_none());
    }

    /// A language tree's root index is a home like the site's own, and a slot
    /// has no position.
    #[test]
    fn an_editor_reads_where_the_build_placed_the_page() {
        let tmp = tempfile::tempdir().unwrap();
        let folder = |source: &str, url: &str| ParsedDocument {
            source_path: Some(source.into()),
            url_path: url.into(),
            kind: moss_core::PageKind::Folder,
            ..Default::default()
        };
        let footer = ParsedDocument { source_path: Some("footer.md".into()), slot_only: true, ..Default::default() };
        let docs = [
            folder("index.md", "index.html"),
            folder("en/index.md", "en/index.html"),
            folder("works/index.md", "works/index.html"),
            folder("en/works/index.md", "en/works/index.html"),
            footer,
        ];
        write_inventory(&docs, &tmp.path().join(".moss"), true, "zh-hans", None).unwrap();
        let position = |rel: &str| page_facts_for_source(tmp.path(), &tmp.path().join(rel).to_string_lossy()).unwrap().position;
        assert_eq!(position("index.md"), Some(PagePosition::Home));
        assert_eq!(position("en/index.md"), Some(PagePosition::Home));
        assert_eq!(position("works/index.md"), Some(PagePosition::Folder));
        assert_eq!(position("en/works/index.md"), Some(PagePosition::Folder));
        assert_eq!(position("footer.md"), None);
    }

    #[test]
    fn no_build_yet_means_no_values() {
        let tmp = tempfile::tempdir().unwrap();
        let facts = page_facts_for_source(tmp.path(), &tmp.path().join("a.md").to_string_lossy()).unwrap();
        assert!(facts.auto.is_empty() && facts.position.is_none());
    }
}
