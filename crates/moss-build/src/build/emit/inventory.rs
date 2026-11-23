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
use moss_core::page::site_page::SitePage;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A value a page takes from the `cascade:` of a folder's home page and does
/// not set itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Inherited {
    pub name: String,
    pub value: serde_json::Value,
    /// The folder address whose home page supplies it, e.g. `/posts/`.
    pub from: String,
}

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
    /// The page's note id, which names its copies' records.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
    /// Where its copies go (see `ParsedDocument::channels`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<String>,
    /// What the page takes from a folder's `cascade:` and does not set itself.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inherited: Vec<Inherited>,
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

/// Through `MossPaths`, not a bare join: a cloud sync client's rename-aside
/// mid-build must not send this file to the decoy while the held root handle
/// already knows where the real one went.
pub fn inventory_path(moss_dir: &Path) -> PathBuf {
    crate::moss_paths::MossPaths::from_moss_dir(moss_dir.to_path_buf()).inventory()
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
    if doc.is_draft() {
        reasons.push(if doc.empty_source { "empty" } else { "draft" }.to_string());
    }
    if doc.listed == Some(false) {
        reasons.push("unlisted".to_string());
    }
    if doc.search_engines == Some(false) {
        reasons.push("search-engines".to_string());
    }
    if doc.hidden_home.is_some() {
        reasons.push("hidden-folder".to_string());
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
                draft: doc.withheld_by_author(),
                listed: doc.listed != Some(false),
                nav: doc.nav,
                slot_only: doc.slot_only,
                hidden: hidden_reasons(doc, has_content_folders),
                resolved: resolved_values(doc, position(doc), has_content_folders, &lang, site_typesetting),
                translations: doc.translations.iter().map(|t| t.url_path.clone()).collect(),
                position: position(doc),
                uid: doc.uid.clone(),
                channels: doc.channels().unwrap_or_default(),
                inherited: doc.inherited.clone(),
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

/// The [`PageFacts`] of the source at `rel_path` inside the site. A file no
/// build has reached yet, or a missing or unreadable inventory, gives none:
/// they are hints, and the next build supplies them.
pub fn page_facts_for_source(project_root: &Path, rel_path: &str) -> PageFacts {
    row_for_source(project_root, rel_path).map(|r| PageFacts { position: r.position, auto: r.resolved }).unwrap_or_default()
}

/// The uid the last build gave the source at `rel_path`, which keys its comments.
pub fn page_uid_for_source(project_root: &Path, rel_path: &str) -> Option<String> {
    row_for_source(project_root, rel_path).and_then(|r| r.uid)
}

/// The last build's row for the source at `rel_path`, matched as the content
/// graph matches paths.
fn row_for_source(project_root: &Path, rel_path: &str) -> Option<InventoryEntry> {
    let key = moss_core::content_graph::normalize_path(rel_path);
    let bytes = std::fs::read(inventory_path(&project_root.join(".moss"))).ok()?;
    let rows: Vec<InventoryEntry> = serde_json::from_slice(&bytes).ok()?;
    rows.into_iter().find(|r| r.source_path.as_deref().is_some_and(|p| moss_core::content_graph::normalize_path(p) == key))
}

/// What an editor shows beside a page: its address, whether it is live and
/// current, and where its copies went.
#[derive(Debug, PartialEq, Serialize)]
pub struct PageStatus {
    /// Its address on the site as of the last build, e.g. `/posts/heron/`.
    pub path: String,
    pub draft: bool,
    /// Whether the last publish to the current host carried it. Left out when
    /// the publish record or the site's config cannot be read now.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published: Option<bool>,
    /// Whether its source differs from what that publish carried; `false`
    /// when it was not published. Left out when either cannot be read now, or
    /// when that publish kept no hash of it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed: Option<bool>,
    pub channels: Vec<String>,
    /// Each posted copy's address, by channel.
    pub copies: BTreeMap<String, String>,
    /// What it takes from a folder's `cascade:` and does not set itself. Left
    /// out when there is none.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inherited: Vec<Inherited>,
}

/// The [`PageStatus`] of the source at `rel_path`, and a line for each fact
/// left unknown. `None` when no build made a page of it. `config` is the
/// site's publish settings, or the reason they could not be read.
///
/// Never waits on a download: a record or source still in the cloud is unknown.
pub fn page_status_for_source(
    project_root: &Path,
    rel_path: &str,
    config: Result<&crate::config::deployment::DomainDeploymentConfig, &str>,
) -> Option<(PageStatus, Vec<String>)> {
    use crate::build::cloud_readiness::{probe_input, Found};
    use crate::build::manifest::published_record::load_for_no_wait;
    use crate::syndication::record::{CopyRecord, Records, State};
    /// What the last publish to the current host knew of the page.
    enum Live {
        Unknown(String),
        NotCarried,
        CarriedUnhashed,
        Carried(String),
    }
    let row = row_for_source(project_root, rel_path).filter(|r| !r.slot_only)?;
    let source = row.source_path.clone()?;
    let paths = crate::moss_paths::MossPaths::new(project_root);
    let mut unknown = Vec::new();
    let record = config.map_err(str::to_owned).and_then(|config| load_for_no_wait(&paths, config.publish_target().as_deref()));
    let live = match record {
        Err(e) => Live::Unknown(e),
        Ok(Some(record)) if record.source_to_output.contains_key(&source) => match record.sources.get(&source) {
            Some(hash) => Live::Carried(hash.clone()),
            None => Live::CarriedUnhashed,
        },
        Ok(_) => Live::NotCarried,
    };
    let (published, changed) = match live {
        Live::Unknown(e) => {
            unknown.push(format!("publish state unknown: {e}"));
            (None, None)
        }
        Live::NotCarried => (Some(false), Some(false)),
        Live::CarriedUnhashed => {
            unknown.push(format!("changes unknown: the last publish kept no hash of {source}"));
            (Some(true), None)
        }
        Live::Carried(live) => (Some(true), match probe_input(&project_root.join(&source)) {
            Ok(Found::Bytes(bytes)) => Some(crate::build::render::blocking::source_metadata(None, &bytes).hash != live),
            Ok(Found::InCloud) => {
                unknown.push(format!("changes unknown: {source} is still in the cloud"));
                None
            }
            Err(e) => {
                unknown.push(format!("changes unknown: {source}: {e}"));
                None
            }
        }),
    };
    let records = Records::in_vault(&paths);
    let mut copies = BTreeMap::new();
    for (uid, channel) in row.uid.iter().flat_map(|uid| row.channels.iter().map(move |c| (uid, c))) {
        match records.get_no_wait(uid, channel) {
            Ok(Some(CopyRecord { state: State::Posted, remote: Some(remote), .. })) => {
                copies.insert(channel.clone(), remote.url);
            }
            Ok(_) => {}
            Err(e) => unknown.push(format!("{channel} copy unknown: {e}")),
        }
    }
    let status = PageStatus {
        path: format!("/{}", crate::build::scan::article_map::to_pretty_url(&row.url_path)),
        draft: row.draft,
        published,
        changed,
        channels: row.channels,
        copies,
        inherited: row.inherited,
    };
    Some((status, unknown))
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
    fn a_row_names_its_note_id_and_channels_and_a_page_kept_from_search_posts_no_copies() {
        let page = |url: &str, search_engines: Option<bool>| ParsedDocument {
            url_path: url.into(), uid: Some(url.into()), syndicate: Some(vec!["bluesky".into()]), search_engines, ..Default::default()
        };
        let rows = build_inventory(&[page("a/index.html", None), page("b/index.html", Some(false))], true, "en", None);
        assert_eq!((rows[0].uid.as_deref(), rows[0].channels.as_slice()), (Some("a/index.html"), &["bluesky".to_string()][..]));
        assert!(rows[1].channels.is_empty(), "a page reached only by its link posts no copies");
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
        let facts = page_facts_for_source(tmp.path(), "works/The Tyger.md");
        assert_eq!(facts.auto["url"], "the-tyger");
        assert_eq!(facts.position, Some(PagePosition::Page));
        let unbuilt = page_facts_for_source(tmp.path(), "new.md");
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
        let position = |rel: &str| page_facts_for_source(tmp.path(), rel).position;
        assert_eq!(position("index.md"), Some(PagePosition::Home));
        assert_eq!(position("en/index.md"), Some(PagePosition::Home));
        assert_eq!(position("works/index.md"), Some(PagePosition::Folder));
        assert_eq!(position("en/works/index.md"), Some(PagePosition::Folder));
        assert_eq!(position("footer.md"), None);
    }

    #[test]
    fn no_build_yet_means_no_values() {
        let tmp = tempfile::tempdir().unwrap();
        let facts = page_facts_for_source(tmp.path(), "a.md");
        assert!(facts.auto.is_empty() && facts.position.is_none());
    }

    /// A moss-hosted site whose last build made `posts/heron.md` a page that
    /// goes to Bluesky and Mastodon, the home and a footer slot, and whose last
    /// publish carried the heron page as `published_bytes`.
    fn published_site(published_bytes: &[u8]) -> tempfile::TempDir {
        use crate::build::manifest::{change_set::PublishedSnapshot, published_record};
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".moss/deploy/copies")).unwrap();
        std::fs::create_dir_all(root.join("posts")).unwrap();
        std::fs::write(root.join("posts/heron.md"), published_bytes).unwrap();
        let row = |source: &str, url: &str, slot: bool| serde_json::json!({"source_path": source, "url_path": url, "title": "",
            "date": null, "kind": "article", "draft": false, "listed": true, "nav": null, "slot_only": slot, "hidden": []});
        let mut heron = row("posts/heron.md", "posts/heron/index.html", false);
        heron["uid"] = "heron1".into();
        heron["channels"] = serde_json::json!(["bluesky", "mastodon"]);
        let rows = serde_json::json!([heron, row("posts/egret.md", "posts/egret/index.html", false), row("index.md", "index.html", false), row("footer.md", "", true)]);
        let inventory = inventory_path(&root.join(".moss"));
        std::fs::create_dir_all(inventory.parent().unwrap()).unwrap();
        std::fs::write(inventory, rows.to_string()).unwrap();
        let hash = crate::build::render::blocking::source_metadata(None, published_bytes).hash;
        let snapshot = PublishedSnapshot { target: "moss:her-blog".into(), sources: [("posts/heron.md".to_string(), hash)].into(),
            source_to_output: [("posts/heron.md", "posts/heron/index.html"), ("posts/egret.md", "posts/egret/index.html")]
                .map(|(s, o)| (s.to_string(), o.to_string())).into(),
            ..Default::default() };
        published_record::save(&crate::moss_paths::MossPaths::new(root), &snapshot).unwrap();
        let copy = serde_json::json!({"uid": "heron1", "channel": "bluesky", "state": "posted", "source_path": "posts/heron.md",
            "url_path": "posts/heron", "remote": {"id": "1", "url": "https://bsky.app/profile/her/post/1"}, "updated_at": "2026-11-20T00:00:00Z"});
        std::fs::write(root.join(".moss/deploy/copies/heron1.bluesky.json"), copy.to_string()).unwrap();
        dir
    }

    use crate::config::deployment::DomainDeploymentConfig;

    fn moss_site(id: &str) -> DomainDeploymentConfig { DomainDeploymentConfig { site_id: Some(id.into()), ..Default::default() } }

    fn plugin(method: &str) -> DomainDeploymentConfig { DomainDeploymentConfig { deploy_method: Some(method.into()), ..Default::default() } }

    fn status(dir: &tempfile::TempDir, rel: &str, config: Result<DomainDeploymentConfig, &str>) -> Option<(PageStatus, Vec<String>)> {
        page_status_for_source(dir.path(), rel, config.as_ref().map_err(|e| *e))
    }

    #[test]
    fn a_page_says_its_address_its_publish_state_and_where_its_copies_went() {
        let dir = published_site(b"# Heron\n");
        let (page, unknown) = status(&dir, "posts/heron.md", Ok(moss_site("her-blog"))).expect("a page the build made");
        assert_eq!(page, PageStatus {
            path: "/posts/heron/".into(), draft: false, published: Some(true), changed: Some(false),
            channels: vec!["bluesky".into(), "mastodon".into()],
            copies: [("bluesky".to_string(), "https://bsky.app/profile/her/post/1".to_string())].into(),
            inherited: vec![],
        });
        assert!(unknown.is_empty(), "{unknown:?}");

        std::fs::write(dir.path().join("posts/heron.md"), "# Heron, edited\n").unwrap();
        assert_eq!(status(&dir, "posts/heron.md", Ok(moss_site("her-blog"))).unwrap().0.changed, Some(true));

        // Against another host, nothing of it is live there.
        let (page, _) = status(&dir, "posts/heron.md", Ok(plugin("onionpress"))).unwrap();
        assert_eq!((page.published, page.changed), (Some(false), Some(false)));
    }

    #[test]
    fn a_page_the_publish_carried_without_a_hash_is_live_with_changes_unknown() {
        let dir = published_site(b"# Heron\n");
        let (page, unknown) = status(&dir, "posts/egret.md", Ok(moss_site("her-blog"))).unwrap();
        assert_eq!((page.published, page.changed), (Some(true), None));
        assert_eq!(unknown, ["changes unknown: the last publish kept no hash of posts/egret.md"]);
    }

    #[test]
    fn the_home_is_at_the_site_root_and_a_slot_or_an_unbuilt_file_is_no_page() {
        let dir = published_site(b"# Heron\n");
        assert_eq!(status(&dir, "index.md", Ok(Default::default())).unwrap().0.path, "/");
        assert_eq!(status(&dir, "footer.md", Ok(Default::default())), None);
        assert_eq!(status(&dir, "posts/new.md", Ok(Default::default())), None);
    }

    #[test]
    fn a_record_still_in_the_cloud_leaves_the_publish_state_unknown_at_once() {
        let dir = published_site(b"# Heron\n");
        let record = crate::build::manifest::published_record::path_for(&crate::moss_paths::MossPaths::new(dir.path()), "moss:her-blog");
        let _in_cloud = crate::build::icloud::pretend::evicted(&record);
        let started = std::time::Instant::now();
        let (page, unknown) = status(&dir, "posts/heron.md", Ok(moss_site("her-blog"))).unwrap();
        assert_eq!((page.published, page.changed), (None, None));
        assert!(unknown.iter().any(|line| line.contains("still in the cloud")), "{unknown:?}");
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "{:?}", started.elapsed());
    }

    #[test]
    fn a_source_or_a_copy_still_in_the_cloud_is_unknown_not_unchanged() {
        let dir = published_site(b"# Heron\n");
        let _source = crate::build::icloud::pretend::evicted(&dir.path().join("posts/heron.md"));
        let _copy = crate::build::icloud::pretend::evicted(&dir.path().join(".moss/deploy/copies/heron1.bluesky.json"));
        let (page, unknown) = status(&dir, "posts/heron.md", Ok(moss_site("her-blog"))).unwrap();
        assert_eq!((page.published, page.changed), (Some(true), None));
        assert!(page.copies.is_empty());
        assert_eq!(unknown.len(), 2, "{unknown:?}");
    }

    #[test]
    fn a_config_that_cannot_be_read_leaves_the_publish_state_unknown() {
        let dir = published_site(b"# Heron\n");
        let (page, unknown) = status(&dir, "posts/heron.md", Err("state.toml is still in the cloud")).unwrap();
        assert_eq!((page.published, page.changed), (None, None));
        assert_eq!(unknown, ["publish state unknown: state.toml is still in the cloud"]);
    }
}
