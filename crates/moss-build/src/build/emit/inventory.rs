//! `.moss/build/inventory.json` — the build's own account of every document it
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
use std::path::{Path, PathBuf};

/// One row of the inventory: a document the build parsed, whether or not it
/// became a page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryEntry {
    /// Project-root-relative source file, e.g. `works/《潮汐》第一期 — 邊界.md`.
    /// `None` for synthesized pages that have no markdown file behind them.
    pub source_path: Option<String>,
    /// Where it publishes, relative to the site root, e.g.
    /// `works/潮汐第一期-邊界/index.html`. Empty for a slot-only document,
    /// which is not published at a URL at all.
    pub url_path: String,
    /// The chrome label (nav, breadcrumb, cards) — `label`, not the body H1.
    pub title: String,
    /// Frontmatter `date:` as written, when present.
    pub date: Option<String>,
    /// `article` or `folder` (a folder index page).
    pub kind: String,
    /// `draft: true` — rendered at its URL, but hidden from every listing,
    /// feed, the sitemap and nav, and marked `noindex`.
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
    /// The BCP-47 tag this page will carry in `<html lang>`.
    ///
    /// The question this answers is "did moss register my new edition?", and
    /// before this column there was no way to ask it: a language folder moss
    /// does not recognize is silently treated as ordinary content, so the
    /// build succeeds either way and the difference shows up only in emitted
    /// HTML. Hugo answers it with a per-language page-count table in its build
    /// summary; a 2026-08-05 side-by-side trial had the agent use exactly that
    /// to confirm its change had landed.
    ///
    /// This is the document's own language, not the interface language — a
    /// `ja/` tree reads `ja` here while its chrome stays the site default's
    /// (#977).
    ///
    /// `#[serde(default)]` because this file is written by one build and read
    /// by a later `moss list`, and the two can be different moss versions —
    /// upgrading mid-project must not turn a readable inventory into a parse
    /// error. It did, for the hour between this column landing and the trial
    /// that caught it: every inventory written before it failed with
    /// `missing field 'lang'`, and the advice printed alongside ("run `moss
    /// build`") was the one thing that would not have explained it. Every
    /// field added here from now on needs the same treatment.
    #[serde(default)]
    pub lang: String,
}

/// `.moss/build/inventory.json` — written by the build, read by `moss list`.
pub fn inventory_path(moss_dir: &Path) -> PathBuf {
    moss_dir.join("build").join("inventory.json")
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
) -> Vec<InventoryEntry> {
    let mut rows: Vec<InventoryEntry> = documents
        .iter()
        .map(|doc| InventoryEntry {
            source_path: doc.source_path.clone(),
            url_path: if doc.slot_only { String::new() } else { doc.url_path.clone() },
            title: doc.label.clone(),
            date: doc.date.clone(),
            kind: match doc.kind {
                moss_core::PageKind::Folder => "folder".to_string(),
                moss_core::PageKind::Article => "article".to_string(),
            },
            draft: doc.draft == Some(true),
            listed: doc.listed != Some(false),
            nav: doc.nav,
            slot_only: doc.slot_only,
            hidden: hidden_reasons(doc, has_content_folders),
            // Same precedence the renderer uses (`html.rs`), so this column
            // reports what the page will actually say rather than a second
            // guess at it — including the fallback, which is the SITE's tag
            // and not the page's UI language (#977: a `fr` site serving
            // English chrome still says `lang="fr"`).
            lang: doc.lang_tag.clone().unwrap_or_else(|| site_lang_tag.to_string()),
        })
        .collect();
    // Sorted by URL so two builds of the same site produce the same file and
    // a diff of the inventory is a diff of the site's routing.
    rows.sort_by(|a, b| (&a.url_path, &a.source_path).cmp(&(&b.url_path, &b.source_path)));
    rows
}

/// Write `.moss/build/inventory.json` from the build's parsed documents.
///
/// Called once per build, right where the article map is saved. Cheap: it
/// clones a handful of short strings per document and serializes them.
///
/// Goes through `build::io_utils` because everything landing under
/// `.moss/build/` must (ADR-043) — a raw `fs::write` there fails against a
/// cloud-evicted destination and is caught by `output_write_invariant_test`.
pub fn write_inventory(
    documents: &[crate::build::types::ParsedDocument],
    moss_dir: &Path,
    has_content_folders: bool,
    site_lang_tag: &str,
) -> Result<(), String> {
    let rows = build_inventory(documents, has_content_folders, site_lang_tag);
    let json = serde_json::to_string_pretty(&rows)
        .map_err(|e| format!("Failed to serialize inventory: {}", e))?;
    crate::build::io_utils::write_output(&inventory_path(moss_dir), json.as_bytes())
        .map_err(|e| format!("Failed to write inventory: {}", e))
}
