//! Populate [`ParsedDocument::direct_children_sort`] for every folder index.
//!
//! Runs once during the scan pass, after frontmatter is loaded and the
//! cascade has propagated inherited `sort:` values to folder indices.
//! For each folder index (root or nested), the direct-children scope is
//! enumerated and [`moss_core::sort::resolve_folder_sort`] is invoked.
//! The result is stored on the folder's `direct_children_sort` field so that
//! downstream renderers (listing, series-nav, editor inferred-axis hint)
//! can read a single source of truth.
//!
//! Subfolders are excluded from inference signals by `resolve_folder_sort`
//! itself (see `crates/moss-core/src/sort.rs`) — we still pass them in
//! the children scope so `explicit_order` lists referencing subfolder
//! stems remain valid, but they don't affect axis inference.

use crate::build::types::ParsedDocument;

/// Populate `direct_children_sort` on every folder-index `ParsedDocument` in `docs`.
///
/// A folder index is any document whose `url_path` is `"index.html"` (root)
/// or ends with `"/index.html"` (nested). Direct children are documents whose
/// path lies one level below the folder's prefix (subfolder `index.html`s
/// count as one direct child each — their own subtree is excluded).
///
/// Must run after `apply_cascade` so that cascaded `sort:` values are visible
/// to the inference (see `resolve_folder_sort` reading `folder.declared_sort()`).
pub(crate) fn populate_direct_children_sorts(docs: &mut [ParsedDocument]) {
    let sorts: Vec<(usize, moss_core::sort::ResolvedSort, Vec<String>)> = moss_core::sort::direct_children_sorts(docs)
        .into_iter()
        .map(|(fi, resolved, undated)| {
            (fi, resolved, undated.iter().map(|d| d.source_path.clone().unwrap_or_else(|| d.url_path.clone())).collect())
        })
        .collect();
    for (fi, resolved, undated) in sorts {
        let prefix = docs[fi].url_path.trim_end_matches("index.html");
        // The site's home page is not a dated stream, and `children: false` lists nothing.
        if !undated.is_empty() && !prefix.is_empty() && docs[fi].children != Some(false) {
            crate::build::cli_output::log_warn_problem!(
                "folder `{}` lists by title, not date, because only some of its pages have a `date:`; add one to {} or set `sort: title` on the folder's home page to keep this order",
                prefix.trim_end_matches('/'),
                undated.join(", ")
            );
        }
        docs[fi].direct_children_sort = Some(resolved);
    }
}

/// A folder whose pages moss before 0.15.30 linked to the previous and next
/// page from its order alone, and that sets no `series:` to keep them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnlinkedFolder {
    /// The folder as the author named it.
    pub folder: String,
    /// The home page's file, which `moss migrate` gives `series: true`;
    /// `None` when moss draws the folder's home itself.
    pub home: Option<String>,
}

/// The folders that lost their previous and next links in 0.15.30, read after
/// the cascade and [`populate_direct_children_sorts`], so a `sort:` or
/// `series:` set from above counts. The site's home and each language tree's
/// home never drew them.
pub(crate) fn unlinked_folders(docs: &[ParsedDocument]) -> Vec<UnlinkedFolder> {
    docs.iter()
        .filter_map(|d| {
            let prefix = d.url_path.strip_suffix("index.html")?;
            let folder = prefix.trim_end_matches('/');
            let linked = d.direct_children_sort.as_ref().is_some_and(linked_by_order);
            if folder.is_empty() || folder == d.edition || d.series.is_some() || !linked || moss_core::page::series::chain(docs, d).len() < 2 {
                return None;
            }
            // Named as the author's folder, not its address, where there is a file to name it by.
            let named = d.source_path.as_deref().and_then(|home| home.rsplit_once('/')).map_or(folder, |(dir, _)| dir);
            Some(UnlinkedFolder { folder: named.to_string(), home: d.source_path.clone() })
        })
        .collect()
}

/// The note for the first build after moss 0.15.30 replaced one older than it,
/// read from the folder's last build record: an ordered folder without
/// `series:` is a valid new site, so the note is not for every build.
pub(crate) fn unlinked_note(unlinked: &[UnlinkedFolder], previous_moss: Option<&str>) -> Option<String> {
    if unlinked.is_empty() || crate::system::floor_met("0.15.30", previous_moss?) != Some(false) {
        return None;
    }
    let folders = unlinked.iter().map(|f| format!("`{}`", f.folder)).collect::<Vec<_>>().join(", ");
    Some(format!("Pages in {folders} no longer link to the previous and next page: `moss migrate --apply` keeps the links"))
}

/// Whether moss before 0.15.30 drew previous and next links in a folder with
/// this order and no `series:`: a weight order or a typed `sort:` list.
pub(crate) fn linked_by_order(resolved: &moss_core::sort::ResolvedSort) -> bool {
    resolved.axis == moss_core::sort::SortAxis::Weight || resolved.explicit_order.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use moss_core::sort::{SortAxis, SortField};

    #[test]
    fn scan_populates_direct_children_sort_on_folders() {
        let mut docs = vec![
            ParsedDocument {
                url_path: "blog/index.html".into(),
                clean_stem: "index".into(),
                ..Default::default()
            },
            ParsedDocument {
                url_path: "blog/post-1.html".into(),
                clean_stem: "post-1".into(),
                date: Some("2025-01-01".into()),
                ..Default::default()
            },
            ParsedDocument {
                url_path: "blog/post-2.html".into(),
                clean_stem: "post-2".into(),
                date: Some("2025-02-01".into()),
                ..Default::default()
            },
        ];

        populate_direct_children_sorts(&mut docs);

        let folder = docs.iter().find(|d| d.url_path == "blog/index.html").unwrap();
        let r = folder.direct_children_sort.as_ref().expect("direct_children_sort should be set");
        assert_eq!(r.axis, SortAxis::Date);
    }

    #[test]
    fn scan_populates_direct_children_sort_for_root_index() {
        let mut docs = vec![
            ParsedDocument {
                url_path: "index.html".into(),
                clean_stem: "index".into(),
                ..Default::default()
            },
            ParsedDocument {
                url_path: "welcome.html".into(),
                clean_stem: "welcome".into(),
                ..Default::default()
            },
        ];
        populate_direct_children_sorts(&mut docs);
        let root = docs.iter().find(|d| d.url_path == "index.html").unwrap();
        assert!(
            root.direct_children_sort.is_some(),
            "root index should also get direct_children_sort populated"
        );
    }

    #[test]
    fn declared_axis_overrides_inference() {
        let mut docs = vec![
            ParsedDocument {
                url_path: "guide/index.html".into(),
                clean_stem: "index".into(),
                sort: Some(SortField::Axis(SortAxis::Title)),
                ..Default::default()
            },
            ParsedDocument {
                url_path: "guide/page-1.html".into(),
                clean_stem: "page-1".into(),
                date: Some("2025-01-01".into()),
                ..Default::default()
            },
            ParsedDocument {
                url_path: "guide/page-2.html".into(),
                clean_stem: "page-2".into(),
                date: Some("2025-02-01".into()),
                ..Default::default()
            },
        ];
        populate_direct_children_sorts(&mut docs);
        let folder = docs.iter().find(|d| d.url_path == "guide/index.html").unwrap();
        let r = folder.direct_children_sort.as_ref().unwrap();
        // Declared `sort: title` wins over the date signal from children.
        assert_eq!(r.axis, SortAxis::Title);
    }

    #[test]
    fn weight_signal_in_children_infers_weight_axis() {
        let mut docs = vec![
            ParsedDocument {
                url_path: "course/index.html".into(),
                clean_stem: "index".into(),
                ..Default::default()
            },
            ParsedDocument {
                url_path: "course/lesson-1.html".into(),
                clean_stem: "lesson-1".into(),
                weight: Some(1),
                ..Default::default()
            },
            ParsedDocument {
                url_path: "course/lesson-2.html".into(),
                clean_stem: "lesson-2".into(),
                weight: Some(2),
                ..Default::default()
            },
        ];
        populate_direct_children_sorts(&mut docs);
        let folder = docs
            .iter()
            .find(|d| d.url_path == "course/index.html")
            .unwrap();
        let r = folder.direct_children_sort.as_ref().unwrap();
        assert_eq!(r.axis, SortAxis::Weight);
    }

    #[test]
    fn grandchildren_are_excluded_from_direct_children_scope() {
        // blog/ has one direct child (subfolder index) — its grandchild
        // blog/post/page.html must NOT be counted as a direct child of blog/.
        let mut docs = vec![
            ParsedDocument {
                url_path: "blog/index.html".into(),
                clean_stem: "index".into(),
                ..Default::default()
            },
            ParsedDocument {
                url_path: "blog/post/index.html".into(),
                clean_stem: "post".into(),
                ..Default::default()
            },
            // Grandchild — should not contribute to blog/'s inference.
            ParsedDocument {
                url_path: "blog/post/page.html".into(),
                clean_stem: "page".into(),
                date: Some("2025-01-01".into()),
                ..Default::default()
            },
        ];
        populate_direct_children_sorts(&mut docs);
        let blog = docs.iter().find(|d| d.url_path == "blog/index.html").unwrap();
        let r = blog.direct_children_sort.as_ref().unwrap();
        // Only direct child is a subfolder index (which resolve_folder_sort
        // filters out of inference signals). No dated articles -> Title.
        assert_eq!(r.axis, SortAxis::Title);
    }

    #[test]
    fn nested_folder_also_gets_direct_children_sort() {
        let mut docs = vec![
            ParsedDocument {
                url_path: "blog/index.html".into(),
                clean_stem: "index".into(),
                ..Default::default()
            },
            ParsedDocument {
                url_path: "blog/post/index.html".into(),
                clean_stem: "post".into(),
                ..Default::default()
            },
            ParsedDocument {
                url_path: "blog/post/page.html".into(),
                clean_stem: "page".into(),
                date: Some("2025-01-01".into()),
                ..Default::default()
            },
        ];
        populate_direct_children_sorts(&mut docs);
        let nested = docs
            .iter()
            .find(|d| d.url_path == "blog/post/index.html")
            .unwrap();
        assert!(
            nested.direct_children_sort.is_some(),
            "nested folder index should also be populated"
        );
    }

    fn ordered_folder(prefix: &str, edition: &str, series: Option<moss_core::frontmatter_typed::SeriesField>) -> Vec<ParsedDocument> {
        let page = |stem: &str, weight| ParsedDocument {
            url_path: format!("{prefix}{stem}.html"),
            clean_stem: stem.into(),
            weight: Some(weight),
            edition: edition.into(),
            ..Default::default()
        };
        let home = ParsedDocument {
            url_path: format!("{prefix}index.html"),
            clean_stem: "index".into(),
            kind: moss_core::PageKind::Folder,
            source_path: Some(format!("{prefix}index.md")),
            edition: edition.into(),
            series,
            ..Default::default()
        };
        vec![home, page("a", 1), page("b", 2)]
    }

    fn unlinked(docs: Vec<ParsedDocument>) -> Vec<String> {
        let mut docs = docs;
        populate_direct_children_sorts(&mut docs);
        unlinked_folders(&docs).into_iter().map(|f| f.folder).collect()
    }

    #[test]
    fn a_weighted_folder_without_series_is_unlinked_and_a_site_or_language_home_or_series_folder_is_not() {
        assert_eq!(unlinked(ordered_folder("Journal/", "", None)), ["Journal"]);
        assert!(unlinked(ordered_folder("", "", None)).is_empty());
        assert!(unlinked(ordered_folder("zh-hans/", "zh-hans", None)).is_empty());
        let off = Some(moss_core::frontmatter_typed::SeriesField::Flag(false));
        assert!(unlinked(ordered_folder("Journal/", "", off)).is_empty());
    }

    #[test]
    fn a_weighted_folder_of_article_subfolders_is_unlinked_and_of_plain_subfolders_is_not() {
        let folders = |layout: &str| {
            let sub = |stem: &str, weight| ParsedDocument {
                url_path: format!("Awards/{stem}/index.html"),
                clean_stem: "index".into(),
                kind: moss_core::PageKind::Folder,
                layout: Some(layout.into()),
                weight: Some(weight),
                ..Default::default()
            };
            let mut docs = ordered_folder("Awards/", "", None);
            docs.truncate(1);
            docs[0].sort = Some(SortField::Axis(SortAxis::Weight));
            docs.extend([sub("writing", 1), sub("film", 2)]);
            docs
        };
        assert_eq!(unlinked(folders("article")), ["Awards"]);
        assert!(unlinked(folders("page")).is_empty());
    }

    #[test]
    fn the_note_is_for_the_first_build_after_an_older_moss_only() {
        let folders = [UnlinkedFolder { folder: "Journal".into(), home: None }];
        let note = unlinked_note(&folders, Some("0.15.29")).expect("a site last built by 0.15.29 is told");
        assert_eq!(note, "Pages in `Journal` no longer link to the previous and next page: `moss migrate --apply` keeps the links");
        assert_eq!(unlinked_note(&folders, Some("0.15.30")), None);
        assert_eq!(unlinked_note(&folders, None), None);
        assert_eq!(unlinked_note(&[], Some("0.15.29")), None);
    }
}
