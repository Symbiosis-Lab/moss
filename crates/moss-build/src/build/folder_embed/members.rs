//! Which pages a folder home page's listing shows, and in what order.
//!
//! The renderer, the folder calendar and a plugin-drawn listing all ask this
//! one module, so they cannot disagree about a listing's pages.

use moss_core::sort::{ResolvedSort, SortAxis};
use moss_core::PageKind;

use super::{effective_group_for_axis, resolve_children_config, select_children_by_slug, upcoming};
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
/// `covers:only`, the pages without a cover.
#[allow(clippy::too_many_arguments)]
pub(super) fn select<'a>(
    folder_slug: &str,
    is_flatten: bool,
    scope_default_tree: bool,
    exclude_nav: bool,
    covers_only: bool,
    host: Option<&ParsedDocument>,
    all_docs: &'a [ParsedDocument],
    project: &ProjectStructure,
) -> Vec<&'a ParsedDocument> {
    let picked = select_children_by_slug(folder_slug, is_flatten, scope_default_tree, exclude_nav, all_docs, project);
    // A page that hosts a listing of a folder it also sits inside (a
    // `children:`/`children_more` whole-site listing at the root, where every
    // page's prefix is "") must not list itself. The folder's own index
    // already excludes itself because its url IS the selector's target; a page
    // that merely POINTS at a folder has no such coincidence.
    let picked = picked.into_iter().filter(|d| host.is_none_or(|h| d.url_path != h.url_path));
    // `covers:only` is applied before sort and limit, so `limit` counts off
    // the covered set. A folder entry is covered when its own index page has a cover.
    picked.filter(|d| !covers_only || d.listing_cover().is_some()).collect()
}

/// A listing's pages in listing order, cut at the limit.
pub(super) struct Ordered<'a> {
    pub resolved: ResolvedSort,
    pub style: moss_core::Resolved<String>,
    /// The group the listing draws, after the sort axis has had its say.
    pub group: String,
    pub skip_resort: bool,
    pub limited: Vec<&'a ParsedDocument>,
    pub truncated: bool,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn order<'a>(
    folder_docs: &[&'a ParsedDocument],
    target_doc: Option<&ParsedDocument>,
    is_flatten: bool,
    sort_override: Option<SortAxis>,
    style_override: Option<&moss_core::Resolved<String>>,
    group_override: Option<&moss_core::Resolved<String>>,
    limit: Option<usize>,
    math: bool,
    now: chrono::NaiveDateTime,
) -> Ordered<'a> {
    // Flatten uses resolve_for_flatten; direct uses resolve_for_direct_children.
    // A per-listing sort override wins over both. A pseudo-folder has no target
    // doc to carry sort intent, so it gets the default a folder with no sort
    // cache gets (date-descending).
    let mut resolved = match target_doc {
        Some(t) if is_flatten => t.resolve_for_flatten(folder_docs),
        Some(t) => t.resolve_for_direct_children(),
        None => ResolvedSort { axis: SortAxis::Date, explicit_order: None, series_default: false },
    };
    if let Some(axis) = sort_override {
        // Drop explicit_order (sort:axis is a user-typed scalar, not a list)
        // and recompute series_default to match.
        resolved = ResolvedSort { axis, explicit_order: None, series_default: matches!(axis, SortAxis::Weight) };
    }
    let (style, group) = resolve_children_config(folder_docs, style_override, group_override, math);
    // An explicit `order:`/`sort:` list is a manual sequence the author chose;
    // it must survive even when the inferred axis is Date (which would otherwise
    // re-sort the children date-descending and discard the order).
    let skip_resort = !resolved.axis.shows_date() || resolved.explicit_order.is_some();
    let group = effective_group_for_axis(&group, resolved.axis);
    let sorted = moss_core::sort::sort_by_resolved(folder_docs, &resolved);
    // Upcoming events lead whatever the folder's sort is, so `limit` cuts
    // from that order (the same grouping `generate_children` applies later).
    let sorted = if group == "upcoming" {
        let upcoming_urls = upcoming::urls(&group, folder_docs, now);
        upcoming::order_docs(sorted, &upcoming_urls, skip_resort)
    } else {
        sorted
    };
    let (limited, truncated) = match limit {
        Some(n) if n > 0 && n < sorted.len() => (sorted.iter().take(n).copied().collect(), true),
        _ => (sorted, false),
    };
    Ordered { resolved, style, group, skip_resort, limited, truncated }
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
) -> Vec<&'a ParsedDocument> {
    use crate::build::render::incremental::listing::Depth;
    let Some(key) = host_key(host, documents) else { return Vec::new() };
    let picked = select(
        &key.folder_slug,
        key.depth == Depth::All,
        key.scope_default_tree,
        key.exclude_nav,
        host.children_covers.as_deref() == Some("only"),
        Some(host),
        documents,
        project,
    );
    ordered_like_listing(host, documents, &picked, host.children_limit.map(|n| n as usize))
}

/// `pages` put in the order the host's listing would put them (its sort and
/// group), without the listing's limit.
pub(crate) fn in_listing_order<'a>(
    host: &ParsedDocument,
    documents: &'a [ParsedDocument],
    pages: &[&'a ParsedDocument],
) -> Vec<&'a ParsedDocument> {
    ordered_like_listing(host, documents, pages, None)
}

fn host_key(host: &ParsedDocument, documents: &[ParsedDocument]) -> Option<crate::build::render::incremental::listing::GroupKey> {
    crate::build::render::incremental::listing::groups_read_by(host, documents)?.into_iter().next()
}

fn ordered_like_listing<'a>(
    host: &ParsedDocument,
    documents: &[ParsedDocument],
    pages: &[&'a ParsedDocument],
    limit: Option<usize>,
) -> Vec<&'a ParsedDocument> {
    use crate::build::render::incremental::listing::Depth;
    let Some(key) = host_key(host, documents) else { return pages.to_vec() };
    let target_url = format!("{}/index.html", key.folder_slug);
    let target_doc = documents.iter().find(|d| {
        if key.folder_slug.is_empty() {
            d.kind == PageKind::Folder && crate::build::folder_index::is_root_source(d)
        } else {
            d.url_path == target_url
        }
    });
    let style = host.children_style.as_ref().map(|r| moss_core::Resolved::frontmatter(r.value.clone()));
    let group = host.children_group.as_ref().map(|r| moss_core::Resolved::frontmatter(r.value.clone()));
    // `math` only reads a page's excerpt to decide whether it is "rich", which
    // moves the auto style and never the order or the cut.
    order(pages, target_doc, key.depth == Depth::All, None, style.as_ref(), group.as_ref(), limit, true, upcoming::now()).limited
}
