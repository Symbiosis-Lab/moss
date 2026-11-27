//! Which pages a folder listing shows, read over [`SitePage`] so the build
//! and every host pick the same pages.

use std::collections::HashMap;

use super::site_page::SitePage;
use crate::resolved::Resolved;
use crate::sort::{ResolvedSort, SortAxis};

/// What a listing reads about the site as a whole.
#[derive(Debug, Clone, Copy, Default)]
pub struct ListingSite {
    /// The site keeps its editions in language trees (`zh-hans/…`).
    pub has_language_trees: bool,
    /// The site has content folders, so its top-level pages are nav items.
    pub has_content_folders: bool,
}

/// What a listing of one folder asks for.
#[derive(Debug, Clone, Copy, Default)]
pub struct ListingScope<'a> {
    /// The folder, url space without slashes at either end; `""` for the root.
    pub folder: &'a str,
    /// Every page below the folder (`children_depth: all`), not only its own.
    pub flatten: bool,
    /// Only the pages of the home's own edition.
    pub default_tree_only: bool,
    /// Leave out the top-level pages the nav bar shows.
    pub exclude_nav: bool,
    /// The edition of the page the listing is on.
    pub edition: &'a str,
}

/// The pages a listing of `scope.folder` shows, in `pages`' order, before any
/// sort or limit.
pub fn select<'a, P: SitePage>(scope: &ListingScope<'_>, pages: &'a [P], site: ListingSite) -> Vec<&'a P> {
    let folder = scope.folder;
    let target_url = if folder.is_empty() { "index.html".to_string() } else { format!("{folder}/index.html") };
    // A page of the site language's own tree is in the default edition, and
    // sits under that tree's folder, so only another edition's page differs.
    let own_tree = crate::home::lang_tree_prefix(&target_url).unwrap_or("");
    // With the trailing slash, `blog/` does not match `blog-extra/index.html`.
    let prefix = if folder.is_empty() { String::new() } else { format!("{folder}/") };
    // Folders whose home says `children_listed: here`: their pages stay out of
    // a flattened listing of any folder above them. A listing of the folder
    // itself, or of one inside it, is not above it.
    let kept_here: Vec<&str> = if scope.flatten {
        pages
            .iter()
            .filter(|p| p.is_folder_index() && p.children_listed() == Some("here"))
            .filter_map(|p| p.url_path().strip_suffix("index.html"))
            .filter(|dir| !dir.is_empty() && !prefix.starts_with(dir))
            .collect()
    } else {
        Vec::new()
    };
    let picked = pages
        .iter()
        .filter(|p| {
            let url = p.url_path();
            if url == target_url {
                return false;
            }
            // A page-less entry is a row in its own folder only, never a term
            // page or an `also_in` one.
            if !p.is_listable_in(&prefix) && !(p.is_pageless_entry() && url.starts_with(&prefix) && p.reaches(&prefix)) {
                return false;
            }
            if scope.default_tree_only && site.has_language_trees && !p.edition().is_empty() && !p.edition().eq_ignore_ascii_case(own_tree) {
                return false;
            }
            if scope.exclude_nav && is_listing_nav_item(*p, site.has_content_folders) {
                return false;
            }
            let in_folder = (url.starts_with(&prefix) && !kept_here.iter().any(|dir| url.starts_with(dir)))
                || p.also_in().iter().any(|f| f == folder);
            if !in_folder {
                return false;
            }
            if scope.flatten {
                return !p.is_folder_index();
            }
            // An `also_in` page need not live under this folder at all: its
            // declaration is the membership claim, nothing to depth-check.
            url.strip_prefix(prefix.as_str())
                .is_none_or(|rest| !rest.trim_end_matches("/index.html").trim_end_matches('/').contains('/'))
        })
        .collect();
    one_per_translation(picked, scope.edition)
}

/// A listing that reaches more than one edition shows a translated piece once:
/// the page in the listing page's own edition, else the default edition's, else
/// the first picked. Pages of one edition never displace each other, so a
/// listing within an edition is returned as it came.
fn one_per_translation<'a, P: SitePage>(picked: Vec<&'a P>, edition: &str) -> Vec<&'a P> {
    if picked.iter().all(|p| p.edition() == picked[0].edition()) {
        return picked;
    }
    let rank = |p: &P| {
        if p.edition().eq_ignore_ascii_case(edition) { 0 } else if p.edition().is_empty() { 1 } else { 2 }
    };
    let at: HashMap<&str, usize> = picked.iter().enumerate().map(|(i, p)| (p.url_path(), i)).collect();
    let better_twin = |i: usize, p: &P| {
        p.translation_urls().filter_map(|t| at.get(t)).any(|&j| {
            let twin = picked[j];
            twin.edition() != p.edition() && (rank(twin), j) < (rank(p), i)
        })
    };
    let dropped: Vec<bool> = picked.iter().enumerate().map(|(i, p)| better_twin(i, p)).collect();
    picked.into_iter().zip(dropped).filter(|(_, gone)| !gone).map(|(p, _)| p).collect()
}

/// Whether a listing leaves the page out because the nav bar shows it: its
/// `nav:`, else, on a site with content folders, every top-level page but an
/// edition's home. A site of loose pages lists every one of them.
pub fn is_listing_nav_item<P: SitePage + ?Sized>(page: &P, has_content_folders: bool) -> bool {
    page.nav().unwrap_or(has_content_folders && page.is_root_level() && !crate::home::is_language_root(page.url_path()))
}

/// What a home page lists when its frontmatter says nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HomeDefaults<'a> {
    /// The folder it lists: `""` for the root home, `de` for `de/index.html`.
    pub tree: &'a str,
    /// Depth defaults to "all" rather than "direct".
    pub depth_all: bool,
    /// Pages of other editions and top-level nav pages are left out.
    pub scoped: bool,
}

/// The root home lists its whole tree. An edition's own `<lang>/index.md`
/// does the same for its tree, unless it sets `children_source` or
/// `children_depth` itself: either one is the author choosing a listing, and
/// `children_depth: direct` is the way back to the narrow edition listing.
pub fn home_defaults<P: SitePage + ?Sized>(page: &P) -> HomeDefaults<'_> {
    let url = page.url_path();
    let tree = crate::home::lang_tree_prefix(url).unwrap_or("");
    let is_root = url == "index.html";
    let is_edition_home = !is_root && !page.edition().is_empty() && crate::home::is_language_root(url);
    let edition_defaults = is_edition_home && page.children_source().is_none() && page.children_depth().is_none();
    HomeDefaults {
        tree,
        depth_all: is_root || edition_defaults,
        scoped: page.children_source().is_none() && (is_root || edition_defaults),
    }
}

/// The listing a page ends in: the folder it shows, at what depth, and
/// whether it keeps to the home's edition and leaves out nav pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnListing {
    pub folder: String,
    pub flatten: bool,
    pub scoped: bool,
}

impl OwnListing {
    pub fn scope<'a>(&'a self, edition: &'a str) -> ListingScope<'a> {
        ListingScope { folder: &self.folder, flatten: self.flatten, default_tree_only: self.scoped, exclude_nav: self.scoped, edition }
    }
}

/// The listing a page ends in, with the filters it carries, or `None` when it
/// ends in none.
pub fn own_listing<P: SitePage>(page: &P, pages: &[P]) -> Option<OwnListing> {
    let (folder, names_scoped_home) = own_listing_target(page, pages)?;
    let home = home_defaults(page);
    let flatten = page.children_depth().map_or(home.depth_all, |depth| depth == "all");
    Some(OwnListing { folder, flatten, scoped: home.scoped || names_scoped_home })
}

/// The folder a page's own listing shows and whether it names an edition home
/// (the listing then takes that home's filters). A home (the root's or a
/// language's) lists its tree; any other page a claimed term, else its own
/// folder when it is a folder index, else the folder its `children_source`
/// names. `children_source` redirects a home too.
pub fn own_listing_target<P: SitePage>(page: &P, pages: &[P]) -> Option<(String, bool)> {
    let url = page.url_path();
    let source = page.children_source().map(|r| children_source_folder(r, pages));
    if crate::home::is_language_root(url) {
        return Some(source.unwrap_or_else(|| (home_defaults(page).tree.to_string(), false)));
    }
    let names_scoped_home = source.as_ref().is_some_and(|t| t.1);
    let folder = match page.term_listing() {
        Some(term) => term.to_string(),
        None if page.is_folder_index() && url.ends_with("/index.html") => url.trim_end_matches("/index.html").to_string(),
        None => source?.0,
    };
    Some((folder, names_scoped_home))
}

/// The folder `children_source:` names, `""` for the root, never the hosting
/// page's own; true when it names an edition home listing its own edition only.
pub fn children_source_folder<P: SitePage>(reference: &str, pages: &[P]) -> (String, bool) {
    if let Some(home) = named_edition_home(reference, pages) {
        let defaults = home_defaults(home);
        return (defaults.tree.to_string(), defaults.scoped);
    }
    let stem = crate::frontmatter_typed::frontmatter_ref_to_stem(reference);
    let path = pages
        .iter()
        .find(|p| p.is_folder_index() && p.url_path().trim_end_matches("/index.html").trim_end_matches('/').eq_ignore_ascii_case(&stem))
        .map(|p| p.url_path().trim_end_matches("index.html").trim_end_matches('/').to_string())
        .unwrap_or_else(|| stem.to_lowercase());
    (path, false)
}

/// The edition home a `children:` reference names by its file stem
/// (`[[Field Notes]]` for the root home, `[[Notes]]` for `en/Notes.md`), if any.
/// Such a listing means what that home's own listing means: its tree, with its
/// edition scoping.
pub fn named_edition_home<'a, P: SitePage>(reference: &str, pages: &'a [P]) -> Option<&'a P> {
    let stem = crate::frontmatter_typed::frontmatter_ref_to_stem(reference);
    pages.iter().find(|p| {
        crate::home::is_language_root(p.url_path())
            && p.source_path()
                .and_then(|s| std::path::Path::new(s).file_stem()?.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case(&stem))
    })
}

/// What a listing's order reads beyond its pages.
#[derive(Debug, Clone, Copy, Default)]
pub struct OrderAsk<'a> {
    /// The listing reaches every page below its folder.
    pub flatten: bool,
    /// A per-listing `sort:`, which wins over the folder's.
    pub sort: Option<SortAxis>,
    pub style: Option<&'a Resolved<String>>,
    pub group: Option<&'a Resolved<String>>,
    pub limit: Option<usize>,
    /// `children_when:`: `past` or `upcoming`.
    pub when: Option<&'a str>,
    /// The minute the site is built at, `YYYY-MM-DDTHH:MM`.
    pub now: &'a str,
}

/// A listing's pages in listing order, cut at the limit.
#[derive(Debug, Clone)]
pub struct Ordered<'a, P> {
    pub resolved: ResolvedSort,
    pub style: Resolved<String>,
    /// The group the listing draws, after the sort axis has had its say.
    pub group: String,
    /// Upcoming events placed soonest first, which the renderer must not
    /// date-sort back to newest first.
    pub pre_ordered: bool,
    pub limited: Vec<&'a P>,
    pub truncated: bool,
}

/// Of the pages the selector picked, those a host's listing keeps: never the
/// host itself, only covered ones under `covers: only`, and only those its
/// `when` keeps. `covered` says whether a page has a listing cover.
pub fn keep<'a, P: SitePage>(picked: Vec<&'a P>, host: Option<&P>, covers_only: bool, when: Option<&str>, now: &str, covered: impl Fn(&P) -> bool) -> Vec<&'a P> {
    // A page that lists a folder it sits inside (a whole-site listing at the
    // root) is among the selector's pages; a folder's own index never is.
    picked
        .into_iter()
        .filter(|p| host.is_none_or(|h| p.url_path() != h.url_path()))
        .filter(|p| !covers_only || covered(p))
        .filter(|p| keeps(when, *p, now))
        .collect()
}

/// An event that has not ended at `now`; ordinary articles never are, whatever their `date`.
pub fn is_upcoming<P: SitePage + ?Sized>(page: &P, now: &str) -> bool {
    page.event_span().is_some_and(|e| e.is_upcoming(now))
}

/// Whether `page` passes a `when:` filter: `past` keeps whatever is not
/// upcoming (articles included), `upcoming` keeps only events that have not
/// ended. An event still waiting for its date is in neither. No filter, or an
/// unknown one, keeps everything.
pub fn keeps<P: SitePage + ?Sized>(when: Option<&str>, page: &P, now: &str) -> bool {
    match when {
        Some("upcoming") => is_upcoming(page, now),
        Some("past") => !is_upcoming(page, now) && !page.awaits_event_date(),
        _ => true,
    }
}

/// `pages` sorted, grouped and cut the way the listing shows them. `target`
/// is the listed folder's home, `None` for a folder without one; `is_rich`
/// says whether a page has a cover or an excerpt, which moves the detected style.
pub fn order<'a, P: SitePage>(pages: &[&'a P], target: Option<&P>, ask: &OrderAsk<'_>, is_rich: impl Fn(&P) -> bool) -> Ordered<'a, P> {
    // A folder without a home has no sort intent: newest first.
    let mut resolved = match target {
        Some(t) if ask.flatten => crate::sort::resolve_folder_sort(t, pages),
        Some(t) => t.resolve_for_direct_children(),
        None => ResolvedSort { axis: SortAxis::Date, explicit_order: None },
    };
    if let Some(axis) = ask.sort {
        resolved = ResolvedSort { axis, explicit_order: None };
    }
    let (style, group) = listing_config(pages, ask.style, ask.group, is_rich);
    let group = effective_group(&group, &resolved);
    let sorted = crate::sort::sort_by_resolved(pages, &resolved);
    // Upcoming events lead whatever the folder's sort is, so `limit` cuts from
    // that order.
    let soonest_first = group != "upcoming"
        && ask.when == Some("upcoming")
        && ask.sort.is_none()
        && target.is_none_or(|t| t.declared_sort().is_none());
    let sorted = if group == "upcoming" || soonest_first {
        upcoming_first(sorted, ask.now, !resolved.is_date_order() && !soonest_first)
    } else {
        sorted
    };
    let (limited, truncated) = match ask.limit {
        Some(n) if n > 0 && n < sorted.len() => (sorted[..n].to_vec(), true),
        _ => (sorted, false),
    };
    Ordered { resolved, style, group, pre_ordered: soonest_first, limited, truncated }
}

/// Upcoming events first (soonest first), then the rest (newest first). With
/// `keep_order` (already ordered by a non-date axis) each part keeps its order.
fn upcoming_first<'a, P: SitePage>(pages: Vec<&'a P>, now: &str, keep_order: bool) -> Vec<&'a P> {
    let (soon, rest): (Vec<_>, Vec<_>) = pages.into_iter().partition(|p| is_upcoming(*p, now));
    if keep_order {
        return soon.into_iter().chain(rest).collect();
    }
    let by = |axis| ResolvedSort { axis, explicit_order: None };
    let mut sorted = crate::sort::sort_by_resolved(&soon, &by(SortAxis::DateAsc));
    sorted.extend(crate::sort::sort_by_resolved(&rest, &by(SortAxis::Date)));
    sorted
}

/// A listing's style and group: the author's (frontmatter or cascade) when
/// set, else detected from its pages.
pub fn listing_config<P: SitePage>(
    pages: &[&P],
    style: Option<&Resolved<String>>,
    group: Option<&Resolved<String>>,
    is_rich: impl Fn(&P) -> bool,
) -> (Resolved<String>, Resolved<String>) {
    let dated = || pages.iter().any(|p| p.date().is_some());
    let style = style.cloned().unwrap_or_else(|| {
        // Bulk, not exceptions: one claimed term must not turn 55 bare labels
        // into empty archive rows. Mostly-rich is an archive at any size, and
        // so is a rich page beside at most 3 bare ones; what summary cannot
        // survive is a screenful of them.
        let rich = pages.iter().filter(|p| is_rich(p)).count();
        let has_rich = rich > 0 && (rich * 2 > pages.len() || pages.len() - rich <= 3);
        // Nothing at all is an index of bare labels, which a grid lays out.
        Resolved::auto(match (has_rich, dated()) {
            (true, _) => "summary",
            (false, false) => "grid",
            (false, true) => "list",
        }.to_string())
    });
    let group = group.cloned().unwrap_or_else(|| {
        // Any dated page under a style that does not stand alone gets year headings.
        let by_year = !matches!(style.value.as_str(), "summary" | "lead" | "stream") && dated();
        Resolved::auto(if by_year { "year" } else { "none" }.to_string())
    });
    (style, group)
}

/// The group a listing draws: an explicit one always, a detected one only on
/// a date order (a title, weight or typed order is a sequence, which year
/// sections would regroup).
pub fn effective_group(group: &Resolved<String>, sort: &ResolvedSort) -> String {
    if !sort.is_date_order() && !group.is_explicit() {
        "none".to_string()
    } else {
        group.value.clone()
    }
}

/// The listing `host` ends in, picked, kept, ordered and cut as the page
/// shows it; `None` when it ends in none. `now` is `YYYY-MM-DDTHH:MM`;
/// `is_rich` and `covered` say whether a page has a cover or an excerpt, and
/// whether it has a listing cover.
pub fn host_listing<'a, P: SitePage>(
    host: &P,
    pages: &'a [P],
    site: ListingSite,
    now: &str,
    is_rich: impl Fn(&P) -> bool,
    covered: impl Fn(&P) -> bool,
) -> Option<Ordered<'a, P>> {
    let own = own_listing(host, pages)?;
    let picked = select(&own.scope(host.edition()), pages, site);
    let kept = keep(picked, Some(host), host.children_covers() == Some("only"), host.children_when(), now, covered);
    let target_url = if own.folder.is_empty() { "index.html".to_string() } else { format!("{}/index.html", own.folder) };
    let target = pages.iter().find(|p| p.is_folder_index() && p.url_path() == target_url);
    let ask = OrderAsk {
        flatten: own.flatten,
        sort: None,
        style: host.children_style(),
        group: host.children_group(),
        limit: host.children_limit().map(|n| n as usize),
        when: host.children_when(),
        now,
    };
    Some(order(&kept, target, &ask, is_rich))
}

#[cfg(test)]
#[path = "listing_tests.rs"]
mod tests;
