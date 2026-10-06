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
        match_and_retarget("note", "posts/note.md", "posts/renamed.md", false),
        Some("renamed".to_string())
    );
    // Extension-carrying ref → it names a FILE, so the full name swaps
    // (not flattened to the bare stem).
    assert_eq!(
        match_and_retarget("note.md", "posts/note.md", "posts/renamed.md", false),
        Some("renamed.md".to_string())
    );
}

#[test]
fn match_and_retarget_folder_prefix() {
    assert_eq!(
        match_and_retarget("posts/note", "posts", "archive", true),
        Some("archive/note".to_string())
    );
}

#[test]
fn match_and_retarget_does_not_match_a_different_extension() {
    // A `.png` reference is never mistaken for a `.jpg` target — matching on
    // stem here would repoint it at an unrelated file.
    assert_eq!(match_and_retarget("photo.png", "photo.jpg", "new.jpg", false), None);
}

#[test]
fn match_and_retarget_leaves_a_page_relative_path_to_the_exact_style_code() {
    // `pics/x.png` written in `articles/post.md` names `articles/pics/x.png`
    // only relative to the page; this matcher sees root-relative shapes and
    // bare names, and `exact_style` / `exact_spelling` own the page-relative
    // spelling.
    assert_eq!(match_and_retarget("pics/x.png", "articles/pics/x.png", "articles/pics/y.png", false), None);
}

#[test]
fn exact_style_recognises_each_way_of_naming_a_file_exactly() {
    let st = |base: &str, from: &str, t: &str, wiki: bool| exact_style(base, from, t, false, wiki);
    assert_eq!(st("../notes/a.md", "blog", "notes/a.md", false).map(|s| (s.anchor, s.form)), Some((Anchor::Page, PathForm::Full)));
    assert_eq!(st("/notes/a.md", "blog", "notes/a.md", false).map(|s| (s.anchor, s.form)), Some((Anchor::Root, PathForm::Full)));
    assert_eq!(st("/docs/", "", "docs/index.md", false).map(|s| (s.anchor, s.form)), Some((Anchor::Root, PathForm::AddrHome)));
    assert_eq!(st("/blog/post/", "", "blog/post.md", false).map(|s| (s.anchor, s.form)), Some((Anchor::Root, PathForm::AddrPage)));
    assert_eq!(st("notes/a", "", "notes/a.md", true).map(|s| (s.anchor, s.form)), Some((Anchor::RootBare, PathForm::NoExt)));
    // Name-shaped or suffix-only: not exact.
    assert_eq!(st("a", "", "a.md", true), None, "a bare wikilink is a name");
    assert_eq!(st("notes/a.md", "blog", "notes/a.md", false), None, "root-relative text from another folder resolves only by search");
    assert_eq!(st("../../x.md", "blog", "x.md", false), None, "escapes the root");
}

#[test]
fn exact_spelling_climbs_and_keeps_the_authored_style() {
    let style = |base: &str, from: &str, t: &str| exact_style(base, from, t, false, false).unwrap();
    assert_eq!(exact_spelling(&style("../notes/a.md", "blog", "notes/a.md"), "archive/deep/a.md", "blog").as_deref(), Some("../archive/deep/a.md"));
    assert_eq!(exact_spelling(&style("./img/a.jpg", "blog", "blog/img/a.jpg"), "blog/pics/a.jpg", "blog").as_deref(), Some("./pics/a.jpg"));
    assert_eq!(exact_spelling(&style("./img/a.jpg", "blog", "blog/img/a.jpg"), "gallery/a.jpg", "blog").as_deref(), Some("../gallery/a.jpg"));
    assert_eq!(exact_spelling(&style("/docs/", "", "docs/index.md"), "guide/index.md", "").as_deref(), Some("/guide/"));
    // A file that is no longer a folder's home page is served at its own
    // address; the address never turns into a `.md` path.
    assert_eq!(exact_spelling(&style("/docs/", "", "docs/index.md"), "guide/intro.md", "").as_deref(), Some("/guide/intro/"));
    assert_eq!(style("/docs/", "", "docs/index.md").full_path(), None);
    assert!(style("notes/a", "", "notes/a.md").full_path().is_some());
}

#[test]
fn match_and_retarget_strips_but_does_not_reattach_anchor() {
    // The caller (`rename_plan::plan_one_ref`) reattaches `#anchor` after
    // calling this — pin that the match still succeeds despite it.
    assert_eq!(
        match_and_retarget("note#heading", "posts/note.md", "posts/renamed.md", false),
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

// ── render_destination ───────────────────────────────────────────────────

fn md(kind: MdKind, angle: bool, base: &str) -> Option<String> {
    render_destination(DestForm::Markdown { kind, angle }, base, "", false)
}

#[test]
fn render_destination_escapes_only_what_breaks_or_changes_a_destination() {
    assert_eq!(md(MdKind::Link, false, "a b.md").as_deref(), Some("a%20b.md"));
    assert_eq!(md(MdKind::Link, true, "a b.md").as_deref(), Some("a b.md"));
    assert_eq!(md(MdKind::Link, false, "笔记 (一).md").as_deref(), Some("笔记%20(一).md"));
    assert_eq!(md(MdKind::Link, false, "a&amp;b.md").as_deref(), Some("a%26amp;b.md"));
    assert_eq!(md(MdKind::Link, false, "a\\*b.md").as_deref(), Some("a%5C*b.md"));
    assert_eq!(md(MdKind::Image, false, "a>b.png").as_deref(), Some("a%3Eb.png"));
    assert_eq!(md(MdKind::Definition, false, "a b.md").as_deref(), Some("a%20b.md"));
    // Fragment and query are the author's, not escaped.
    assert_eq!(
        render_destination(DestForm::Markdown { kind: MdKind::Link, angle: false }, "a b.md", "#sec?x", false).as_deref(),
        Some("a%20b.md#sec?x")
    );
}

#[test]
fn render_destination_refuses_a_wikilink_name_it_cannot_carry() {
    for base in ["a|b.md", "a#b.md", "a]b.md", "a?b.md"] {
        assert_eq!(render_destination(DestForm::Wiki, base, "", false), None, "{base}");
    }
    assert_eq!(render_destination(DestForm::Wiki, "a b.md", "#h", false).as_deref(), Some("a b.md#h"));
}
