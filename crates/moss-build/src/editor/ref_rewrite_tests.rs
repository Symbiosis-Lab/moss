//! Unit tests for the pure rewrite layer. No filesystem: every function
//! under test is a pure text/shape transform. The removal tests that need a
//! real `ReferenceContext` stay in `ref_scan_tests.rs` next to
//! `build_indexes`; the resolver-driven, end-to-end rename tests (extract,
//! resolve, shape, verify — against a real project on disk) live in
//! `rename_plan_tests.rs`.
//!
//! `match_and_retarget` / `retarget_root_relative` used to also decide
//! WHETHER a reference should be rewritten (a hand-picked, ambiguity-gated
//! pattern match against the renamed entry's own old/new path). That
//! decision now belongs to the resolver (`rename_plan::plan_one_ref` calls
//! `classify_reference` first, and only reaches these functions for a
//! reference it already knows names the target) — so `RefAmbiguity` and the
//! tests that pinned its gating are gone; what these functions still do, and
//! what stays tested here, is purely FORM: given a text's own shape and a
//! (old, new) target pair it is already known to name, produce the
//! same-shaped replacement.

use super::*;

// ── match_and_retarget: shape production ────────────────────────────────────

#[test]
fn match_and_retarget_bare_stem_and_name() {
    // Extensionless ref → markdown stem swap.
    assert_eq!(
        match_and_retarget("note", "", "posts/note.md", "posts/renamed.md", false),
        Some("renamed".to_string())
    );
    // Extension-carrying ref → it names a FILE, so the full name swaps
    // (not flattened to the bare stem).
    assert_eq!(
        match_and_retarget("note.md", "", "posts/note.md", "posts/renamed.md", false),
        Some("renamed.md".to_string())
    );
}

#[test]
fn match_and_retarget_folder_prefix() {
    assert_eq!(
        match_and_retarget("posts/note", "", "posts", "archive", true),
        Some("archive/note".to_string())
    );
}

#[test]
fn match_and_retarget_does_not_match_a_different_extension() {
    // A `.png` reference is never mistaken for a `.jpg` target — matching on
    // stem here would repoint it at an unrelated file.
    assert_eq!(match_and_retarget("photo.png", "", "photo.jpg", "new.jpg", false), None);
}

#[test]
fn match_and_retarget_root_anchored_form_preserved() {
    assert_eq!(
        match_and_retarget("/photo.jpg", "", "photo.jpg", "new.jpg", false),
        Some("/new.jpg".to_string())
    );
    // A trailing slash (folder embed) survives the round trip.
    assert_eq!(
        match_and_retarget("/posts/", "", "posts", "archive", true),
        Some("/archive/".to_string())
    );
}

#[test]
fn match_and_retarget_document_relative_fallback() {
    // `articles/post.md` refers to `pics/x.png`, which is
    // `articles/pics/x.png` from the project root. Root-relative matching
    // fails; the document-relative attempt catches it and re-emits in the
    // same shape.
    assert_eq!(
        match_and_retarget("pics/x.png", "articles", "articles/pics/x.png", "articles/pics/y.png", false),
        Some("pics/y.png".to_string())
    );
}

#[test]
fn match_and_retarget_strips_but_does_not_reattach_anchor() {
    // The caller (`rename_plan::plan_one_ref`) reattaches `#anchor` after
    // calling this — pin that the match still succeeds despite it.
    assert_eq!(
        match_and_retarget("note#heading", "", "posts/note.md", "posts/renamed.md", false),
        Some("renamed".to_string())
    );
}

// ── render_bare_value / required_quote: structural formatting ──────────────
// Unchanged by this fix; exercised directly now instead of through the
// deleted whole-file orchestrator.

use moss_core::resolve::md_extract::PathContainer;

#[test]
fn render_bare_value_preserves_pipe_attrs() {
    assert_eq!(render_bare_value(&PathContainer::GalleryBody, None, "new.jpg", "cover top"), "new.jpg|cover top");
}

#[test]
fn render_bare_value_hero_attr_non_ascii_unquoted() {
    // The rename half of the ASCII-only `is_bareword` bug: a non-ASCII value
    // must come back UNQUOTED, or `required_quote`/`is_bareword` have drifted
    // apart again.
    let container = PathContainer::ShortcodeAttr { key: "image".into() };
    assert_eq!(render_bare_value(&container, None, "肖像.png", ""), "肖像.png");
}

#[test]
fn render_bare_value_hero_attr_requotes_when_needed() {
    // A value with a space is not a legal bareword — `parse_attrs` has no
    // single-quote form, so `"` is the only option.
    let container = PathContainer::ShortcodeAttr { key: "image".into() };
    let out = render_bare_value(&container, None, "my photo.png", "");
    assert_eq!(out, "\"my photo.png\"");
    assert!(moss_core::ast::attrs::parse_attrs(&format!("{{image={out}}}")).is_ok());
}

#[test]
fn render_bare_value_frontmatter_preserves_quote_style() {
    let container = PathContainer::FrontmatterField { key: "cover".into() };
    assert_eq!(render_bare_value(&container, Some('\''), "new.png", ""), "'new.png'");
}

#[test]
fn render_bare_value_frontmatter_quotes_when_value_needs_it() {
    let container = PathContainer::FrontmatterField { key: "cover".into() };
    assert_eq!(render_bare_value(&container, None, "#odd name.png", ""), "'#odd name.png'");
}

// ── apply_edits: unchanged ───────────────────────────────────────────────

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
