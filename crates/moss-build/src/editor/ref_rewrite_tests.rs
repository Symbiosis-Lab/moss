//! Unit tests for the pure rewrite layer. No filesystem: every function
//! under test is `(source, old, new) -> String`. The two removal tests that
//! need a real `ReferenceContext` stay in `ref_scan_tests.rs` next to
//! `build_indexes`.

use super::*;

/// The common case: the renamed entry's stem and file name are both unique
/// in the project, so bare refs of either shape are safe to rewrite.
const UNIQUE: RefAmbiguity = RefAmbiguity { stem_unique: true, name_unique: true };
/// Neither is unique — every bare ref must be left alone.
const AMBIGUOUS: RefAmbiguity = RefAmbiguity { stem_unique: false, name_unique: false };


#[test]
fn rewrite_refs_by_raw_match_stem() {
    let source = "See [[note]] for details.";
    // Unique stem, extensionless ref → rewritten to the new stem.
    let result = rewrite_refs_by_raw_match(source, "", "posts/note.md", "posts/renamed.md", false, UNIQUE);
    assert_eq!(result, "See [[renamed]] for details.");
    // An extension-carrying wikilink now keeps its extension (it names a
    // FILE, not a markdown stem) instead of being flattened to `[[renamed]]`.
    let with_ext = "See [[note.md]] for details.";
    let result = rewrite_refs_by_raw_match(with_ext, "", "posts/note.md", "posts/renamed.md", false, UNIQUE);
    assert_eq!(result, "See [[renamed.md]] for details.");
}

#[test]
fn rewrite_refs_by_raw_match_preserves_alias() {
    let source = "See [[note|My Note]] for details.";
    let result = rewrite_refs_by_raw_match(source, "", "note.md", "renamed.md", false, UNIQUE);
    assert!(
        result.contains("[[renamed|My Note]]"),
        "alias should be preserved, got: {}",
        result
    );
}

#[test]
fn rewrite_refs_by_raw_match_folder() {
    let source = "See [[posts/note]] here.";
    let result = rewrite_refs_by_raw_match(source, "", "posts", "archive", true, UNIQUE);
    assert!(
        result.contains("[[archive/note]]"),
        "folder prefix should be updated, got: {}",
        result
    );
}

#[test]
fn rewrite_refs_skips_bare_stem_when_ambiguous() {
    // Two files share the stem "notes" (posts/notes.md and drafts/notes.md).
    // Renaming posts/notes.md must NOT rewrite a bare [[notes]] (could mean
    // drafts/notes.md). stem_is_unique=false → bare ref untouched.
    let source = "See [[notes]] for details.";
    let result =
        rewrite_refs_by_raw_match(source, "", "posts/notes.md", "posts/renamed.md", false, AMBIGUOUS);
    assert_eq!(result, source, "ambiguous bare stem must not be rewritten");
    // A path-qualified ref is still rewritten even when the stem is ambiguous.
    let source2 = "See [[posts/notes]] explicitly.";
    let result2 =
        rewrite_refs_by_raw_match(source2, "", "posts/notes.md", "posts/renamed.md", false, AMBIGUOUS);
    assert!(
        result2.contains("[[posts/renamed]]"),
        "explicit path should still rewrite, got: {}",
        result2
    );
}


// ── Structural asset paths (2026-08-03) ──────────────────────────────────

fn gallery(body: &str) -> String {
    format!(":::gallery\n{body}\n:::\n")
}

#[test]
fn rewrite_gallery_bare_path() {
    // The reported bug: a bare path on a gallery body line is not a markdown
    // token, so rename used to leave it pointing at a file that no longer
    // exists — silently.
    let src = gallery("關於/頭像-李柏萱.png");
    let out = rewrite_refs_by_raw_match(
        &src, "", "關於/頭像-李柏萱.png", "關於/avatar-lee.png", false, UNIQUE,
    );
    assert_eq!(out, gallery("關於/avatar-lee.png"));
}

#[test]
fn rewrite_does_not_repoint_different_extension() {
    // Renaming photo.jpg must NOT touch a `photo.png` reference — a
    // DIFFERENT file. Matching a bare extension-carrying ref on STEM would
    // turn a loud broken link into a silent wrong one.
    let src = gallery("photo.png\n![](photo.png)");
    let out = rewrite_refs_by_raw_match(&src, "", "photo.jpg", "new.jpg", false, UNIQUE);
    assert_eq!(out, src, "a different extension is a different file");
}

#[test]
fn rewrite_preserves_extension_for_bare_asset_ref() {
    let out = rewrite_refs_by_raw_match(
        "![](photo.jpg) and more", "", "photo.jpg", "new.jpg", false, UNIQUE,
    );
    assert_eq!(out, "![](new.jpg) and more");

    let src = gallery("photo.jpg");
    let out = rewrite_refs_by_raw_match(&src, "", "photo.jpg", "new.jpg", false, UNIQUE);
    assert_eq!(out, gallery("new.jpg"));
}

#[test]
fn rewrite_skips_ambiguous_bare_asset_name() {
    // Two `photo.jpg` in different folders → `name_unique == false`.
    let src = gallery("photo.jpg");
    let amb = RefAmbiguity { stem_unique: true, name_unique: false };
    let out = rewrite_refs_by_raw_match(&src, "", "a/photo.jpg", "a/new.jpg", false, amb);
    assert_eq!(out, src, "an ambiguous bare file name must not be rewritten");
}

#[test]
fn rewrite_gallery_bare_path_on_folder_rename() {
    let src = gallery("關於/x.png");
    let out = rewrite_refs_by_raw_match(&src, "", "關於", "about", true, UNIQUE);
    assert_eq!(out, gallery("about/x.png"));
}

#[test]
fn rewrite_gallery_bare_path_preserves_pipe_attrs() {
    let src = gallery("photo.jpg|cover top");
    let out = rewrite_refs_by_raw_match(&src, "", "photo.jpg", "new.jpg", false, UNIQUE);
    assert_eq!(out, gallery("new.jpg|cover top"));
}

#[test]
fn rewrite_hero_image_attr() {
    let src = ":::hero {.big image=cover.jpg}\nOverlay\n:::\n";
    let out = rewrite_refs_by_raw_match(src, "", "cover.jpg", "new.jpg", false, UNIQUE);
    assert_eq!(out, ":::hero {.big image=new.jpg}\nOverlay\n:::\n");
}

// The rename half of the ASCII-only `is_bareword` bug: an unquoted non-ASCII
// value made `parse_attrs_spanned` bail, so `hero_asset_spans` produced no
// span and rename silently skipped the reference — leaving the author a hero
// pointing at a file that no longer exists, with no report. The renamed-to
// value is non-ASCII too, so it must come back UNQUOTED: re-quoting here would
// mean `required_quote` and `is_bareword` had drifted apart again.
#[test]
fn rewrite_hero_image_attr_non_ascii_unquoted() {
    let src = ":::hero {.big image=頭像.png}\nOverlay\n:::\n";
    let out = rewrite_refs_by_raw_match(src, "", "頭像.png", "肖像.png", false, UNIQUE);
    assert_eq!(out, ":::hero {.big image=肖像.png}\nOverlay\n:::\n");
}

#[test]
fn rewrite_hero_image_attr_requotes_when_needed() {
    // A new name with a space is not a legal bareword, so the value must be
    // quoted — and `parse_attrs` has no single-quote form, so `"` only.
    let src = ":::hero {image=cover.jpg}\n:::\n";
    let out = rewrite_refs_by_raw_match(src, "", "cover.jpg", "my photo.png", false, UNIQUE);
    assert_eq!(out, ":::hero {image=\"my photo.png\"}\n:::\n");
    assert!(moss_core::ast::attrs::parse_attrs("{image=\"my photo.png\"}").is_ok());
}

#[test]
fn rewrite_frontmatter_cover_preserves_quoting_and_comments() {
    let src = "---\ntitle: Hi\ncover: 'old.png'  # keep\nauthor: me\n---\n\nBody\n";
    let out = rewrite_refs_by_raw_match(src, "", "old.png", "new.png", false, UNIQUE);
    assert_eq!(
        out,
        "---\ntitle: Hi\ncover: 'new.png'  # keep\nauthor: me\n---\n\nBody\n"
    );
}

#[test]
fn rewrite_frontmatter_cover_quotes_when_value_needs_it() {
    let src = "---\ncover: old.png\n---\n";
    let out = rewrite_refs_by_raw_match(src, "", "old.png", "#odd name.png", false, UNIQUE);
    assert_eq!(out, "---\ncover: '#odd name.png'\n---\n");
}

#[test]
fn rewrite_document_relative_gallery_path() {
    // `articles/post.md` refers to `pics/x.png`, which is
    // `articles/pics/x.png` from the project root. Root-relative matching
    // returns None; the doc-relative attempt catches it and re-emits in the
    // same shape.
    let src = gallery("pics/x.png");
    let out = rewrite_refs_by_raw_match(
        &src, "articles", "articles/pics/x.png", "articles/pics/y.png", false, UNIQUE,
    );
    assert_eq!(out, gallery("pics/y.png"));
}

#[test]
fn two_identical_tokens_on_one_gallery_line() {
    // `![](x.png) ![](x.png)` makes `parse_markdown_image` return None (the
    // path would contain `(`), so the whole line becomes one structural
    // "path". Two token edits plus one structural edit over the same bytes
    // used to be able to hand `replace_range` overlapping ranges.
    let src = gallery("![](x.png) ![](x.png)");
    let out = rewrite_refs_by_raw_match(&src, "", "x.png", "y.png", false, UNIQUE);
    assert_eq!(out, gallery("![](y.png) ![](y.png)"));
}

#[test]
fn apply_edits_drops_overlapping_and_non_boundary_edits() {
    let src = "héllo world";
    // Earlier wins: the second edit intersects the first and is dropped.
    let out = apply_edits(
        src,
        vec![
            Edit { from: 0, to: 6, text: "HI".into() },
            Edit { from: 3, to: 8, text: "XX".into() },
        ],
    );
    assert_eq!(out, "HI world");
    // Non-char-boundary and inverted bounds are dropped, not panicked on.
    let out = apply_edits(
        src,
        vec![
            Edit { from: 2, to: 3, text: "!".into() }, // mid-'é'
            Edit { from: 8, to: 4, text: "!".into() }, // from > to
            Edit { from: 0, to: 99, text: "!".into() }, // out of bounds
        ],
    );
    assert_eq!(out, src);
}
