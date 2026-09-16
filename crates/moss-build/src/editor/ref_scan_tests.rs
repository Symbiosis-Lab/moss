use super::*;
use std::fs;
use std::path::Path;

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

#[test]
fn scan_finds_wikilink_ref_to_target() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(root, "target.md", "# Target");
    write(root, "other.md", "# Other\n\nSee [[target]] for details.");
    write(root, "unrelated.md", "# Unrelated\n\nNo refs here.");

    let target = root.join("target.md");
    let hits = scan_project_references_to(&target, root).expect("scan");
    let hit_texts: Vec<_> = hits.iter().map(|h| h.ref_text.as_str()).collect();
    assert!(
        hit_texts.contains(&"target"),
        "should find [[target]] wikilink"
    );
    // unrelated.md should NOT appear
    let unrelated: Vec<_> = hits
        .iter()
        .filter(|h| h.referencing_file.contains("unrelated"))
        .collect();
    assert!(unrelated.is_empty(), "no false positive from unrelated.md");
}

#[test]
fn scan_finds_markdown_link_ref() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(root, "doc.md", "# Doc");
    write(root, "index.md", "See [the doc](doc.md) here.");

    let target = root.join("doc.md");
    let hits = scan_project_references_to(&target, root).expect("scan");
    assert!(!hits.is_empty(), "should find [the doc](doc.md)");
}

#[test]
fn scan_no_false_positives() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(root, "alpha.md", "# Alpha");
    write(root, "beta.md", "# Beta");
    write(root, "index.md", "See [[beta]] here.");

    let target = root.join("alpha.md");
    let hits = scan_project_references_to(&target, root).expect("scan");
    assert!(
        hits.is_empty(),
        "should not match [[beta]] when scanning for alpha.md"
    );
}

#[test]
fn scan_finds_embed_ref_to_target() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(root, "photo.png", "fake-png-bytes");
    write(root, "page.md", "# Page\n\n![[photo.png]] embedded.");

    let target = root.join("photo.png");
    let hits = scan_project_references_to(&target, root).expect("scan");
    assert!(!hits.is_empty(), "should find ![[photo.png]] embed");
}

#[test]
fn scan_dedups_overlapping_folder_and_file_hits() {
    // The same reference is matched once by the folder target and once by
    // the file-inside-folder target. After dedup, scan_project_references_to
    // for each target returns one hit each; the command-level dedup folds
    // identical (file, ref_text, line) hits across targets. Here we assert
    // the per-target scan does not itself emit duplicates for one ref.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(root, "posts/note.md", "# Note");
    write(root, "index.md", "See [[posts/note]] once.");

    let folder = root.join("posts");
    let hits = scan_project_references_to(&folder, root).expect("scan");
    assert_eq!(
        hits.len(),
        1,
        "one ref should yield exactly one hit, got: {:?}",
        hits
    );
}
#[test]
fn rewrite_for_removal_empties_token() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(root, "note.md", "# Note");

    let canonical_root = fs::canonicalize(root).unwrap();
    let (fs_assets, fs_folders, article_idx) = build_indexes(&canonical_root);
    let ctx = ReferenceContext {
        assets: &fs_assets,
        folders: &fs_folders,
        urls: &article_idx,
    };
    let source = "Before ![[note.md]] after.";
    let result = rewrite_for_removal(source, "index.md", "note.md", &ctx, false);
    assert_eq!(result, "Before  after.");
}


#[test]
fn removal_deletes_whole_gallery_line_for_both_syntaxes() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(root, "x.png", "bytes");
    let canonical_root = fs::canonicalize(root).unwrap();
    let (fs_assets, fs_folders, article_idx) = build_indexes(&canonical_root);
    let ctx = ReferenceContext {
        assets: &fs_assets,
        folders: &fs_folders,
        urls: &article_idx,
    };

    let src = ":::gallery\nx.png\n![](x.png)\nkeep.png\n:::\n";
    let out = rewrite_for_removal(src, "index.md", "x.png", &ctx, false);
    assert_eq!(
        out, ":::gallery\nkeep.png\n:::\n",
        "both lines vanish whole — no blank residue, no orphan attrs"
    );
}

#[test]
fn scan_finds_bare_gallery_path_reference() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(root, "關於/x.png", "bytes");
    write(root, "page.md", ":::gallery\n關於/x.png\n:::\n");

    let hits = scan_project_references_to(&root.join("關於/x.png"), root).expect("scan");
    assert_eq!(hits.len(), 1, "got: {hits:?}");
    assert_eq!(hits[0].ref_text, "關於/x.png");
    assert_eq!(hits[0].line, 2);
}

#[test]
fn scan_finds_frontmatter_cover_reference() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(root, "cover.png", "bytes");
    write(root, "page.md", "---\ncover: cover.png\n---\n\nBody\n");

    let hits = scan_project_references_to(&root.join("cover.png"), root).expect("scan");
    assert_eq!(hits.len(), 1, "got: {hits:?}");
    assert_eq!(hits[0].ref_text, "cover.png");
}
