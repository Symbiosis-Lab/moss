//! Which pages a folder home page's listing shows, and in what order.
//!
//! The renderer, the folder calendar and a plugin-drawn listing all ask this
//! one module, so they cannot disagree about a listing's pages.

use moss_core::page::listing;
use moss_core::sort::SortAxis;
use moss_core::PageKind;

use super::{is_rich_child, select_children_by_slug, upcoming};
use crate::build::types::ParsedDocument;
use crate::types::content::ProjectStructure;

/// Whether a page is the home page of a folder (or the site root): a page that
/// has a source file of its own, as opposed to a folder index moss writes.
pub(crate) fn is_folder_home(doc: &ParsedDocument) -> bool {
    doc.source_path.is_some() && (doc.kind == PageKind::Folder || doc.url_path == "index.html")
}

/// The folder a page's address sits in, `/`-separated; empty for the root.
pub(crate) fn own_dir(doc: &ParsedDocument) -> Option<String> {
    let out = crate::build::served_path::ServedPath::from_source(&doc.url_path).ok()?;
    if out.as_str() == "index.html" {
        return Some(String::new());
    }
    out.as_str().strip_suffix("/index.html").map(str::to_string)
}

/// The pages the selector picks for a listing, less the host itself and, for
/// `covers:only`, the pages without a cover and, for `when`, the pages on the
/// other side of now.
#[allow(clippy::too_many_arguments)]
pub(super) fn select<'a>(
    folder_slug: &str,
    is_flatten: bool,
    scope_default_tree: bool,
    exclude_nav: bool,
    covers_only: bool,
    when: Option<&str>,
    now: chrono::NaiveDateTime,
    host: Option<&ParsedDocument>,
    all_docs: &'a [ParsedDocument],
    project: &ProjectStructure,
) -> Vec<&'a ParsedDocument> {
    let edition = host.map_or("", |h| h.edition.as_str());
    let picked = select_children_by_slug(folder_slug, is_flatten, scope_default_tree, exclude_nav, edition, all_docs, project);
    // `covers:only` is applied before sort and limit, so `limit` counts off
    // the covered set. A folder entry is covered when its own index page has a cover.
    listing::keep(picked, host, covers_only, when, &upcoming::minute(now), |d| d.listing_cover().is_some())
}

/// A listing's pages in listing order, cut at the limit.
pub(super) type Ordered<'a> = listing::Ordered<'a, ParsedDocument>;

/// `math` only decides whether a page's excerpt makes it "rich".
pub(super) fn order<'a>(
    folder_docs: &[&'a ParsedDocument],
    target_doc: Option<&ParsedDocument>,
    ask: &listing::OrderAsk<'_>,
    math: bool,
) -> Ordered<'a> {
    listing::order(folder_docs, target_doc, ask, |d| is_rich_child(d, math))
}

/// Every page the host's listing could reach: the selector's pick for each
/// listing group the host reads in its own folder, with no `covers`, sort or
/// limit applied. `None` when the host lists a folder that does not exist.
/// The folder calendar reads this, since it keeps every event the listing
/// could show whatever the listing's limit.
pub(crate) fn listed_unfiltered<'a>(
    host: &ParsedDocument,
    documents: &'a [ParsedDocument],
    project: &ProjectStructure,
) -> Option<Vec<&'a ParsedDocument>> {
    use crate::build::render::incremental::listing::{groups_read_by, Depth};
    let dir = own_dir(host)?;
    let mut seen = std::collections::BTreeSet::new();
    let mut members = Vec::new();
    for key in groups_read_by(host, documents)?.into_iter().filter(|k| k.folder_slug == dir) {
        for member in select_children_by_slug(
            &key.folder_slug,
            key.depth == Depth::All,
            key.scope_default_tree,
            key.exclude_nav,
            &key.edition,
            documents,
            project,
        ) {
            if seen.insert(member.url_path.as_str()) {
                members.push(member);
            }
        }
    }
    Some(members)
}

/// Exactly the pages the host's own listing shows, in the order it shows them:
/// the host excluded, `covers:only`, the folder's sort, `children_limit`.
pub(crate) fn listing_members<'a>(
    host: &ParsedDocument,
    documents: &'a [ParsedDocument],
    project: &ProjectStructure,
    now: chrono::NaiveDateTime,
) -> Vec<&'a ParsedDocument> {
    host_listing(host, documents, project, now).map_or_else(Vec::new, |o| o.limited)
}

/// The host's own listing, ordered and cut, with the style and group it draws;
/// `None` when the host ends in no listing.
pub(crate) fn host_listing<'a>(
    host: &ParsedDocument,
    documents: &'a [ParsedDocument],
    project: &ProjectStructure,
    now: chrono::NaiveDateTime,
) -> Option<Ordered<'a>> {
    use crate::build::render::incremental::listing::Depth;
    let key = host_key(host, documents)?;
    let picked = select(
        &key.folder_slug,
        key.depth == Depth::All,
        key.scope_default_tree,
        key.exclude_nav,
        host.children_covers.as_deref() == Some("only"),
        host.children_when.as_deref(),
        now,
        Some(host),
        documents,
        project,
    );
    ordered_like_listing(host, documents, &picked, host.children_limit.map(|n| n as usize), now)
}

/// `pages` put in the order the host's listing would put them (its sort and
/// group), without the listing's limit.
pub(crate) fn in_listing_order<'a>(
    host: &ParsedDocument,
    documents: &'a [ParsedDocument],
    pages: &[&'a ParsedDocument],
    now: chrono::NaiveDateTime,
) -> Vec<&'a ParsedDocument> {
    ordered_like_listing(host, documents, pages, None, now).map_or_else(|| pages.to_vec(), |o| o.limited)
}

fn host_key(host: &ParsedDocument, documents: &[ParsedDocument]) -> Option<crate::build::render::incremental::listing::GroupKey> {
    crate::build::render::incremental::listing::own_listing_key(host, documents)
}

fn ordered_like_listing<'a>(
    host: &ParsedDocument,
    documents: &[ParsedDocument],
    pages: &[&'a ParsedDocument],
    limit: Option<usize>,
    now: chrono::NaiveDateTime,
) -> Option<Ordered<'a>> {
    use crate::build::render::incremental::listing::Depth;
    let key = host_key(host, documents)?;
    let target_url = format!("{}/index.html", key.folder_slug);
    let target_doc = documents.iter().find(|d| {
        if key.folder_slug.is_empty() {
            d.kind == PageKind::Folder && crate::build::folder_index::is_root_source(d)
        } else {
            d.url_path == target_url
        }
    });
    // `math` only reads a page's excerpt to decide whether it is "rich", which
    // moves the auto style and never the order or the cut.
    let (style, group) = (host.children_style.as_ref(), host.children_group.as_ref());
    let now = upcoming::minute(now);
    let ask = listing::OrderAsk { flatten: key.depth == Depth::All, sort: None, style, group, limit, when: host.children_when.as_deref(), now: &now };
    Some(order(pages, target_doc, &ask, true))
}
