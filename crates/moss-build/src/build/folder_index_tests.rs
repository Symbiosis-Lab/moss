use super::*;

fn folder_doc(source_path: &str, url_path: &str) -> ParsedDocument {
    ParsedDocument {
        source_path: Some(source_path.to_string()),
        url_path: url_path.to_string(),
        kind: PageKind::Folder,
        ..Default::default()
    }
}

fn article_doc(source_path: &str, url_path: &str) -> ParsedDocument {
    ParsedDocument {
        source_path: Some(source_path.to_string()),
        url_path: url_path.to_string(),
        kind: PageKind::Article,
        ..Default::default()
    }
}

/// The regression this predicate exists to close (moss#1101): a root
/// folder-note's `url_path` can end up NOT being `index.html` (its home
/// election can miss for reasons entirely upstream of this predicate), while
/// its `PageKind::Folder` + source location still correctly say it IS the
/// vault root's index. `dir_has_markdown_index` must answer from the second
/// pair, never the first, or the two can silently disagree.
#[test]
fn dir_has_markdown_index_root_ignores_url_path_and_reads_kind() {
    let docs = vec![folder_doc("William Blake.md", "william-blake/index.html")];
    let idx = BuildFolderIndex { docs: &docs, html_files: &[] };
    assert!(
        idx.dir_has_markdown_index(""),
        "a root PageKind::Folder doc is the root index regardless of what its url_path happens to be"
    );
}

#[test]
fn dir_has_markdown_index_root_rejects_a_root_article() {
    let docs = vec![article_doc("Archive.md", "archive/index.html")];
    let idx = BuildFolderIndex { docs: &docs, html_files: &[] };
    assert!(
        !idx.dir_has_markdown_index(""),
        "an ordinary article at the root is not the root's index just because it lives there"
    );
}

#[test]
fn dir_has_markdown_index_nested_folder_matches_its_own_source_dir_only() {
    let docs = vec![
        folder_doc("notes/daily/daily.md", "notes/daily/index.html"),
        folder_doc("William Blake.md", "index.html"),
    ];
    let idx = BuildFolderIndex { docs: &docs, html_files: &[] };
    assert!(idx.dir_has_markdown_index("notes/daily"));
    assert!(idx.dir_has_markdown_index(""));
    assert!(!idx.dir_has_markdown_index("notes"));
}

/// `root_rel` is raw marker text (an author writes `![[Notes/Daily/|...]]`
/// with whatever case they used), so it is slug-normalized before it meets
/// `url_path`, same as before this fix.
#[test]
fn dir_has_markdown_index_slug_normalizes_both_sides() {
    let docs = vec![folder_doc("Notes/Daily/Daily.md", "notes/daily/index.html")];
    let idx = BuildFolderIndex { docs: &docs, html_files: &[] };
    assert!(idx.dir_has_markdown_index("notes/daily"));
    assert!(idx.dir_has_markdown_index("Notes/Daily"));
}

/// A `url:` override renames the served directory independently of its
/// source path (e.g. `獎項/` served at `awards/`). The root-only fix must
/// not touch this: nested folders still answer from `url_path`, which is
/// the only place the override is recorded.
#[test]
fn dir_has_markdown_index_nested_url_override_still_matches_by_url_path() {
    let docs = vec![folder_doc("獎項/獎項.md", "awards/index.html")];
    let idx = BuildFolderIndex { docs: &docs, html_files: &[] };
    assert!(idx.dir_has_markdown_index("awards"));
    assert!(!idx.dir_has_markdown_index("獎項"));
}
