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
    let folder_idxs: Vec<usize> = docs
        .iter()
        .enumerate()
        .filter(|(_, d)| d.url_path == "index.html" || d.url_path.ends_with("/index.html"))
        .map(|(i, _)| i)
        .collect();

    for fi in folder_idxs {
        // Prefix of this folder. Root index has empty prefix (everything is
        // a descendant); nested folders use their parent path with a trailing
        // slash, e.g. "blog/" for "blog/index.html".
        let prefix = if docs[fi].url_path == "index.html" {
            String::new()
        } else {
            docs[fi].url_path.trim_end_matches("index.html").to_string()
        };

        let resolved = {
            let children: Vec<&ParsedDocument> = docs
                .iter()
                .enumerate()
                .filter(|(i, d)| {
                    if *i == fi {
                        return false;
                    }
                    // Strip the folder prefix and any trailing "/index.html"
                    // (so "blog/post/index.html" -> "post") or trailing "/"
                    // (defensive). If the remaining path contains a "/", the
                    // doc is a grandchild, not a direct child — exclude it.
                    let Some(rem) = d.url_path.strip_prefix(prefix.as_str()) else {
                        return false;
                    };
                    let clean = rem
                        .trim_end_matches("/index.html")
                        .trim_end_matches('/');
                    !clean.is_empty() && !clean.contains('/')
                })
                .map(|(_, d)| d)
                .collect();
            moss_core::sort::resolve_folder_sort(&docs[fi], &children)
        };
        docs[fi].direct_children_sort = Some(resolved);
    }
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
        assert!(!r.series_default);
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
    fn weight_signal_in_children_infers_weight_axis_and_series_default() {
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
        assert!(r.series_default, "weight axis should default series chrome on");
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
}
