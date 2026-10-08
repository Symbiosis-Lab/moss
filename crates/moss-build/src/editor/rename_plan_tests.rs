//! End-to-end tests for the resolver-driven plan/apply/undo engine, against
//! a real project on disk (tempdir). Pure-function coverage for the shared
//! formatter pieces (`match_and_retarget`, `render_bare_value`, …) lives in
//! `ref_rewrite_tests.rs`; the existing shortcode/frontmatter harbor fixture
//! and the CLI door stay in `tests/rename_boundary_test.rs`.

use super::*;
use crate::editor::ref_scan::build_indexes;
use std::fs;
use std::path::Path;

fn w(root: &Path, rel: &str, content: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, content).unwrap();
}

fn r(root: &Path, rel: &str) -> String {
    fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

fn do_move(root: &Path, old_rel: &str, new_rel: &str) -> RenameApplyResult {
    let old = root.join(old_rel).to_string_lossy().to_string();
    let new = root.join(new_rel).to_string_lossy().to_string();
    let plan = plan_moves(root, &[(old, new)]).expect("plan");
    apply_planned_moves(root, &plan).expect("apply")
}

#[cfg(unix)]
#[test]
fn plan_apply_accepts_paths_spelled_through_a_symlinked_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(tmp.path()).unwrap();
    w(&root, "target.md", "# Target\n");

    let aliases = tempfile::tempdir().unwrap();
    let alias_root = aliases.path().join("vault");
    std::os::unix::fs::symlink(&root, &alias_root).unwrap();
    let old = alias_root.join("target.md").to_string_lossy().into_owned();
    let new = alias_root.join("moved.md").to_string_lossy().into_owned();

    let plan = plan_moves(&root, &[(old, new)]).expect("plan through symlink alias");
    apply_planned_moves(&root, &plan).expect("apply through symlink alias");
    assert!(root.join("moved.md").exists());
    assert!(!root.join("target.md").exists());
}

#[cfg(unix)]
#[test]
fn plan_rejects_traversal_after_an_alias_before_touching_the_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(tmp.path()).unwrap();
    w(&root, "target.md", "# Target\n");

    let aliases = tempfile::tempdir().unwrap();
    let alias_root = aliases.path().join("vault");
    std::os::unix::fs::symlink(&root, &alias_root).unwrap();
    let cases = [
        (alias_root.join("target.md"), alias_root.join("dir/../moved.md")),
        (alias_root.join("dir/../target.md"), alias_root.join("moved.md")),
    ];
    for (old, new) in cases {
        let old = old.to_string_lossy().into_owned();
        let new = new.to_string_lossy().into_owned();
        let err = plan_moves(&root, &[(old, new)]).unwrap_err();
        assert_eq!(err, "Path traversal not allowed");
    }
    assert!(root.join("target.md").exists());
    assert!(!root.join("moved.md").exists());
}

#[test]
fn plan_rejects_a_destination_outside_the_root_before_touching_the_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(tmp.path()).unwrap();
    w(&root, "target.md", "# Target\n");
    let outside = tempfile::tempdir().unwrap();
    let old = root.join("target.md").to_string_lossy().into_owned();
    let new = outside.path().join("moved.md").to_string_lossy().into_owned();

    let err = plan_moves(&root, &[(old, new)]).unwrap_err();
    assert!(err.contains("within the project directory"), "{err}");
    assert!(root.join("target.md").exists());
    assert!(!outside.path().join("moved.md").exists());
}

#[test]
fn plan_rejects_a_missing_destination_parent_before_touching_the_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(tmp.path()).unwrap();
    w(&root, "target.md", "# Target\n");
    let old = root.join("target.md").to_string_lossy().into_owned();
    let new = root.join("missing/moved.md").to_string_lossy().into_owned();

    let err = plan_moves(&root, &[(old, new)]).unwrap_err();
    assert!(err.contains("Failed to resolve"), "{err}");
    assert!(root.join("target.md").exists());
    assert!(!root.join("missing").exists());
}

#[test]
fn plan_rejects_a_destination_without_a_final_component_before_touching_the_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(tmp.path()).unwrap();
    w(&root, "target.md", "# Target\n");
    let old = root.join("target.md").to_string_lossy().into_owned();

    let err = plan_moves(&root, &[(old, String::new())]).unwrap_err();
    assert!(err.starts_with("Invalid destination path:"), "{err}");
    assert!(root.join("target.md").exists());
}

// ── The reported case ────────────────────────────────────────────────────

#[test]
fn folder_note_shorthand_markdown_link_rewritten_after_folder_rename() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "index.md", "See [x](旧夹.md) here.\n");
    w(root, "旧夹/旧夹.md", "# 旧夹\n");

    do_move(root, "旧夹", "新夹");

    assert_eq!(r(root, "index.md"), "See [x](新夹.md) here.\n");
    assert!(root.join("新夹/新夹.md").exists());
    assert!(!root.join("旧夹").exists());
}

#[test]
fn folder_note_shorthand_wikilink_and_embed_rewritten() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "index.md", "[[旧夹]] and ![[旧夹]]\n");
    w(root, "旧夹/旧夹.md", "# 旧夹\n");

    do_move(root, "旧夹", "新夹");

    assert_eq!(r(root, "index.md"), "[[新夹]] and ![[新夹]]\n");
}

#[test]
fn folder_note_shorthand_frontmatter_cover_rewritten() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "page.md", "---\ncover: 旧夹.md\n---\n\nBody\n");
    w(root, "旧夹/旧夹.md", "# 旧夹\n");

    do_move(root, "旧夹", "新夹");

    assert_eq!(r(root, "page.md"), "---\ncover: 新夹.md\n---\n\nBody\n");
}

// A folder with NO home file of its own (no `旧夹.md`, no `index.md`) still
// gets a real page in the build — the renderer auto-generates its index —
// so a bare wikilink to it must survive the folder's rename. Before the
// planner registered auto-index dirs on its own graphs, this reference
// resolved to nothing pre-move (invisible to the resolver, case 4 of the
// invariant) and rename left it dangling.
#[test]
fn bare_wikilink_to_an_auto_index_folder_is_rewritten_after_rename() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "旧夹/note.md", "第一段。\n");
    w(root, "index.md", "[[旧夹]] here.\n");

    do_move(root, "旧夹", "新夹");

    assert_eq!(r(root, "index.md"), "[[新夹]] here.\n");
    assert!(root.join("新夹/note.md").exists());
}

// An angle-bracket destination `(<path>)` is CommonMark's way to let a link
// destination contain a space; pulldown-cmark strips the brackets before the
// build resolves it. The rewrite must keep them — the retargeted path still
// has a space, so bare (unbracketed) syntax would break the link.
//
// The decoy at `other/my note.md` matters: without it, "my note" is a
// globally unique stem and the reference would already resolve correctly by
// bare-stem fallback post-move (same free pass as
// `bare_reference_resolving_by_basename_is_untouched_on_folder_rename`),
// leaving the text untouched and never exercising the bracket-preserving
// rewrite this test is for.
#[test]
fn angle_bracket_destination_is_rewritten_and_keeps_its_brackets() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "posts/my note.md", "第一段。\n");
    w(root, "other/my note.md", "decoy\n");
    w(root, "index.md", "[x](<posts/my note.md>)\n");

    do_move(root, "posts", "文章");

    assert_eq!(r(root, "index.md"), "[x](<文章/my note.md>)\n");
}

// A `?query` suffix (`[x](note.md?v=1)`) is opaque to resolution — the build
// splits it off before the graph lookup — but must survive a rewrite the
// same way a `#anchor` suffix already does.
#[test]
fn query_suffix_survives_rename() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "note.md", "第一段。\n");
    w(root, "index.md", "[x](note.md?v=1)\n");

    do_move(root, "note.md", "笔记.md");

    assert_eq!(r(root, "index.md"), "[x](笔记.md?v=1)\n");
}

// A reference-style link: pulldown-cmark resolves `[x][id]` against the
// `[id]: note.md` definition and renders it as a normal link, so only the
// definition's destination is a reference to rewrite; the `[x][id]` use
// itself must survive byte-identical.
#[test]
fn reference_style_definition_is_rewritten_use_stays_untouched() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "note.md", "第一段。\n");
    w(root, "index.md", "See [x][id] here.\n\n[id]: note.md\n");

    do_move(root, "note.md", "笔记.md");

    assert_eq!(r(root, "index.md"), "See [x][id] here.\n\n[id]: 笔记.md\n");
}

// pulldown-cmark recognizes a link reference definition inside a list item
// or a blockquote the same way it does at the top level, so the scanner must
// too — a bare top-level-only definition scanner missed exactly these.
#[test]
fn definition_inside_a_list_item_is_rewritten() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "note.md", "第一段。\n");
    w(root, "index.md", "- [id]: note.md\n");

    do_move(root, "note.md", "笔记.md");

    assert_eq!(r(root, "index.md"), "- [id]: 笔记.md\n");
}

#[test]
fn definition_inside_a_blockquote_is_rewritten() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "note.md", "第一段。\n");
    w(root, "index.md", "> [id]: note.md\n");

    do_move(root, "note.md", "笔记.md");

    assert_eq!(r(root, "index.md"), "> [id]: 笔记.md\n");
}

// Obsidian (wikilinks off) writes a percent-encoded destination for a path
// with a space: `my%20note.md` names the real file "my note.md". The
// rewrite must emit the same encoding style — the renamed file still has a
// space, so a bare (literal-space) destination would be invalid CommonMark.
#[test]
fn percent_encoded_destination_is_rewritten_still_percent_encoded() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "my note.md", "第一段。\n");
    w(root, "index.md", "[x](my%20note.md)\n");

    do_move(root, "my note.md", "her note.md");

    assert_eq!(r(root, "index.md"), "[x](her%20note.md)\n");
}

// ── A moved file's own outgoing links ───────────────────────────────────

#[test]
fn moved_files_own_relative_link_is_rebased_and_untouched_links_stay_byte_identical() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    // `a/sub/page.md` moves into `b/`. Its own `../shared.md` link named
    // `a/shared.md` from the old location; a decoy at `b/shared.md` means the
    // unchanged text would silently point at the WRONG file from the new
    // location, so it must be rebased. Its `[[unique]]` link resolves by a
    // project-wide-unique stem, independent of the file's own directory, so
    // it must survive byte-identical (minimal edits).
    w(root, "a/shared.md", "# shared A\n");
    w(root, "b/shared.md", "# shared B (decoy)\n");
    w(root, "unique.md", "# Unique note\n");
    w(root, "a/sub/page.md", "[y](../shared.md)\n[[unique]]\n");

    do_move(root, "a/sub/page.md", "b/page.md");

    let out = r(root, "b/page.md");
    assert!(out.contains("[[unique]]"), "an unaffected link stays byte-identical, got:\n{out}");
    assert!(!out.contains("(../shared.md)"), "the rebased link must not keep pointing at the decoy, got:\n{out}");

    // Whatever form the rebase took, it must resolve to the ORIGINAL a/shared.md.
    let (assets, folders, url) = build_indexes(&fs::canonicalize(root).unwrap());
    let ctx = ReferenceContext { assets: &assets, folders: &folders, urls: &url };
    let rr = extract_md_references(&out).into_iter().find(|r| r.text.contains("shared")).expect("shared link");
    let resolved = classify_reference(&rr.text, "b/page.md", true, &ctx);
    assert_eq!(resolved.target_path.as_deref(), Some("a/shared.md"), "rebased link must resolve to a/shared.md, ref text was {:?}", rr.text);
}

// ── Minimal edits ────────────────────────────────────────────────────────

#[test]
fn bare_reference_resolving_by_basename_is_untouched_on_folder_rename() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "posts/note.md", "# Note\n");
    w(root, "index.md", "[[note]] here.\n");

    do_move(root, "posts", "articles");

    // The bare reference resolves by unique basename regardless of the
    // note's directory, so the rename must not touch it at all.
    assert_eq!(r(root, "index.md"), "[[note]] here.\n");
    assert!(root.join("articles/note.md").exists());
}

// ── Ambiguity escalation ─────────────────────────────────────────────────

#[test]
fn rename_that_introduces_a_bare_name_collision_escalates_to_an_explicit_path() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    // Renaming old/photo.jpg -> old/renamed.jpg would make the bare
    // "renamed.jpg" form collide with a pre-existing, unrelated file of the
    // same name elsewhere — the same-shape candidate must fail verification
    // and escalate to an explicit path instead of emitting a now-ambiguous
    // reference.
    w(root, "old/photo.jpg", "fake-bytes");
    w(root, "decoy/renamed.jpg", "fake-bytes-2");
    w(root, "gallery.md", ":::gallery\nphoto.jpg\n:::\n");

    do_move(root, "old/photo.jpg", "old/renamed.jpg");

    let out = r(root, "gallery.md");
    assert!(!out.contains("\nrenamed.jpg\n"), "the bare form is now ambiguous and must not be emitted, got:\n{out}");

    let (assets, folders, url) = build_indexes(&fs::canonicalize(root).unwrap());
    let ctx = ReferenceContext { assets: &assets, folders: &folders, urls: &url };
    let span = extract_structural_asset_refs(&out).into_iter().next().expect("gallery path");
    let resolved = classify_reference(&span.path, "gallery.md", true, &ctx);
    assert_eq!(resolved.target_path.as_deref(), Some("old/renamed.jpg"), "escalated form must resolve to the moved file, got path {:?}", span.path);
}

// ── Unrelated same-stem file is left alone ──────────────────────────────

#[test]
fn reference_to_a_different_same_named_file_is_not_touched() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "a/note.md", "# A note\n");
    w(root, "b/note.md", "# B note (being renamed)\n");
    w(root, "index.md", "[[a/note]]\n");

    do_move(root, "b/note.md", "b/renamed.md");

    assert_eq!(r(root, "index.md"), "[[a/note]]\n", "an unrelated same-stem file must not be touched");
}

// ── Batch moves ──────────────────────────────────────────────────────────

#[test]
fn batch_move_two_files_into_one_folder_keeps_every_link_resolving() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "groupA/x.md", "Links to [[y]].\n");
    w(root, "groupB/y.md", "# Y\n");
    w(root, "hub.md", "[a](groupA/x.md) and [b](groupB/y.md)\n");
    // `rename_entry_inner` is a plain `fs::rename` underneath: it does not
    // create the destination's parent, the same contract a single move
    // already has. The caller (the desktop's `move_one`) creates the target
    // folder first; the fixture does the same.
    fs::create_dir_all(root.join("merged")).unwrap();

    let old_x = root.join("groupA/x.md").to_string_lossy().to_string();
    let new_x = root.join("merged/x.md").to_string_lossy().to_string();
    let old_y = root.join("groupB/y.md").to_string_lossy().to_string();
    let new_y = root.join("merged/y.md").to_string_lossy().to_string();
    let plan = plan_moves(root, &[(old_x, new_x), (old_y, new_y)]).expect("plan");
    apply_planned_moves(root, &plan).expect("apply");

    assert!(root.join("merged/x.md").exists());
    assert!(root.join("merged/y.md").exists());

    // `[[y]]` and `[a](...)`/`[b](...)` are wikilink/markdown-link kinds —
    // the build resolves those via `ContentGraph::resolve_path`, not
    // `classify_reference`, so verification here goes through the same
    // production dispatch (`ref_route` + `resolve_by_route`) the planner
    // itself uses, against a fresh in-memory graph of the post-apply tree.
    let post_files = walk_all_files(root);
    let idx = Indexes::build(&post_files);

    let x_out = r(root, "merged/x.md");
    let rr = extract_md_references(&x_out).into_iter().next().expect("x's own link");
    let resolved = resolve_by_route(ref_route(&rr.syntax), &rr.text, "merged/x.md", &ReferenceContext { assets: &idx.graph, folders: &idx.folders, urls: &NoUrlIndex }, &idx.graph);
    assert_eq!(resolved.as_deref(), Some("merged/y.md"), "x's link to y must resolve to the moved y, text was {:?}", rr.text);

    let hub_out = r(root, "hub.md");
    for rr in extract_md_references(&hub_out) {
        let resolved = resolve_by_route(ref_route(&rr.syntax), &rr.text, "hub.md", &ReferenceContext { assets: &idx.graph, folders: &idx.folders, urls: &NoUrlIndex }, &idx.graph);
        let target = resolved.expect("hub's links must still resolve");
        assert!(target == "merged/x.md" || target == "merged/y.md", "unexpected target {target}");
    }
}

// ── Regression coverage migrated from the deleted rewrite_refs_by_raw_match
// end-to-end tests (folder rename + gallery bare path; overlapping tokens on
// one line) — now exercised through the real resolver instead of hand-picked
// text matching.

#[test]
fn folder_rename_updates_a_gallery_bare_path_resolution() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "關於/x.png", "bytes");
    w(root, "page.md", ":::gallery\n關於/x.png\n:::\n");

    do_move(root, "關於", "about");

    // `x.png` is project-wide unique, so the path-qualified reference keeps
    // resolving through the resolver's own partial-path search
    // even with its text untouched — a stronger form of the minimal-edit
    // invariant than the old hand-picked matcher had (its unit test on this
    // same shape pinned a rewrite that a resolver-driven engine correctly
    // recognizes as unnecessary). What must hold is resolution, not a
    // specific mutation.
    let out = r(root, "page.md");
    let (assets, folders, url) = build_indexes(root);
    let ctx = ReferenceContext { assets: &assets, folders: &folders, urls: &url };
    let span = extract_structural_asset_refs(&out).into_iter().next().expect("gallery path");
    let resolved = classify_reference(&span.path, "page.md", true, &ctx);
    assert_eq!(resolved.target_path.as_deref(), Some("about/x.png"), "got path {:?} in {out:?}", span.path);
}

#[test]
fn two_identical_tokens_on_one_gallery_line_both_rewrite() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "x.png", "bytes");
    // `![](x.png) ![](x.png)` makes the structural gallery-line parser treat
    // the whole line as one bare "path" (it contains an unescaped `(`), so a
    // token edit AND a structural edit can land on overlapping bytes.
    w(root, "page.md", ":::gallery\n![](x.png) ![](x.png)\n:::\n");

    do_move(root, "x.png", "y.png");

    assert_eq!(r(root, "page.md"), ":::gallery\n![](y.png) ![](y.png)\n:::\n");
}

// ── Project-level invariant ──────────────────────────────────────────────

#[test]
fn project_invariant_every_reference_resolves_to_its_mapped_target_or_is_untouched() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "index.md", "See [x](旧夹.md), [[旧夹]] and [[note]].\n");
    w(root, "旧夹/旧夹.md", "# 旧夹\n");
    w(root, "posts/note.md", "# Note\n");
    w(root, "unrelated.md", "Nothing here moves: [[note]] and plain text.\n");
    let unrelated_before = r(root, "unrelated.md");

    // Snapshot how many references resolve to each target BEFORE the move —
    // the invariant this whole engine exists for is that this count, per
    // MAPPED target, survives the move exactly (a rewrite neither drops nor
    // duplicates a live reference). `resolves_to` is the small local helper
    // below, reused for the "after" count too.
    let before_folder_note = resolves_to(root, "旧夹/旧夹.md");
    let before_note = resolves_to(root, "posts/note.md");
    assert_eq!(before_folder_note, 2, "sanity: [x](旧夹.md) and [[旧夹]] both resolve to the folder note before the move");
    assert_eq!(before_note, 2, "sanity: [[note]] resolves from both index.md and unrelated.md before the move");

    let old1 = root.join("旧夹").to_string_lossy().to_string();
    let new1 = root.join("新夹").to_string_lossy().to_string();
    let plan = plan_moves(root, &[(old1, new1)]).expect("plan must find a verified rewrite for every affected reference");
    let applied = apply_planned_moves(root, &plan).expect("apply");
    assert!(applied.skipped.is_empty(), "nothing should be stale on a fresh fixture: {:?}", applied.skipped);

    // The untouched file is byte-identical — not even opened for writing.
    assert_eq!(r(root, "unrelated.md"), unrelated_before);

    // The core invariant, checked directly: the same number of references
    // now resolve to the MAPPED target as resolved to the original before —
    // not merely "whatever currently resolves points at something real",
    // which a reference gone silently unresolved would pass vacuously.
    assert_eq!(resolves_to(root, "新夹/新夹.md"), before_folder_note, "every reference that pointed at the folder note must still reach it, now at its new path");
    assert_eq!(resolves_to(root, "posts/note.md"), before_note, "an unaffected target's reference count must not change");

    // And every reference left in the project resolves to something real —
    // no rewrite ever points at a phantom path.
    let (assets, folders, url) = build_indexes(root);
    let ctx = ReferenceContext { assets: &assets, folders: &folders, urls: &url };
    for entry in walkdir::WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if !p.is_file() || p.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let rel = p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
        let source = fs::read_to_string(p).unwrap();
        for rr in extract_md_references(&source) {
            let resolved = classify_reference(&rr.text, &rel, true, &ctx);
            if let Some(target) = resolved.target_path {
                assert!(root.join(&target).exists(), "{rel}: reference {:?} resolves to non-existent {target}", rr.text);
            }
        }
    }
}

/// Count every markdown reference in `root` whose current resolution equals
/// `target` (root-relative). Builds the SAME in-memory, `ContentGraph`-backed
/// context `plan_moves` itself resolves against (via the crate-private
/// `walk_all_files` / `Indexes` this test module can see through `use
/// super::*`).
fn resolves_to(root: &Path, target: &str) -> usize {
    let pre_files = walk_all_files(root);
    let idx = Indexes::build(&pre_files);
    let urls = NoUrlIndex;
    let ctx = ReferenceContext { assets: &idx.graph, folders: &idx.folders, urls: &urls };
    let mut count = 0;
    for rel in pre_files.iter().filter(|r| r.ends_with(".md")) {
        let source = fs::read_to_string(root.join(rel)).unwrap();
        for rr in extract_md_references(&source) {
            let resolved = classify_reference(&rr.text, rel, true, &ctx);
            if resolved.target_path.as_deref() == Some(target) {
                count += 1;
            }
        }
    }
    count
}

// ── Undo ───────────────────────────────────────────────────────────────────

#[test]
fn plan_apply_undo_round_trips_to_byte_identical() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "index.md", "See [x](旧夹.md), [[旧夹]] and [[note]].\n");
    w(root, "旧夹/旧夹.md", "# 旧夹\n");
    w(root, "posts/note.md", "# Note\n");

    let snapshot_before: Vec<(String, String)> = ["index.md", "旧夹/旧夹.md", "posts/note.md"]
        .iter()
        .map(|rel| (rel.to_string(), r(root, rel)))
        .collect();

    let old1 = root.join("旧夹").to_string_lossy().to_string();
    let new1 = root.join("新夹").to_string_lossy().to_string();
    let plan = plan_moves(root, &[(old1, new1)]).expect("plan");
    let applied = apply_planned_moves(root, &plan).expect("apply");
    assert_ne!(r(root, "index.md"), snapshot_before[0].1, "sanity: the rename must have changed something");

    let undo = undo_applied(root, &applied).expect("undo");
    assert!(undo.skipped.is_empty(), "nothing should be stale immediately after apply: {:?}", undo.skipped);

    assert!(root.join("旧夹/旧夹.md").exists(), "the folder must be back under its original name");
    assert!(!root.join("新夹").exists());
    for (rel, before) in snapshot_before {
        assert_eq!(r(root, &rel), before, "{rel} must be byte-identical to before the rename");
    }
}

// ── Per-kind resolver dispatch (review finding) ─────────────────────────
//
// A plain link or a non-embed wikilink resolves through
// `ContentGraph::resolve_path` directly (see `RefRoute`'s doc comment). These
// fixtures pin what that route finds that `classify_reference` alone does not
// reach: an index-stem folder home (`notes/index.md`) for a bare `[[notes]]`,
// and two same-stem files, where the build always resolves to a deterministic
// tiebreak winner.

#[test]
fn bare_wikilink_matches_an_index_stem_folder_home_unconditionally() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "notes/index.md", "# Notes\n");
    w(root, "index.md", "[[notes]] link\n");

    do_move(root, "notes", "articles");

    let out = r(root, "index.md");
    assert!(!out.contains("[[notes]]"), "the stale bare form must not survive the rename, got:\n{out}");

    let post_files = walk_all_files(root);
    let idx = Indexes::build(&post_files);
    let rr = extract_md_references(&out).into_iter().next().expect("the wikilink");
    let ctx = ReferenceContext { assets: &idx.graph, folders: &idx.folders, urls: &NoUrlIndex };
    let resolved = resolve_by_route(ref_route(&rr.syntax), &rr.text, "index.md", &ctx, &idx.graph);
    assert_eq!(resolved.as_deref(), Some("articles/index.md"), "must resolve to the moved folder's home, text was {:?}", rr.text);
}

#[test]
fn renaming_the_tiebreak_winner_of_two_same_stem_files_rewrites_the_bare_wikilink() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "alpha/readme.md", "# Alpha readme\n");
    w(root, "beta/readme.md", "# Beta readme\n");
    w(root, "index.md", "[[readme]] link\n");

    // ContentGraph::resolve_path's tiebreak (no shared lang tree, no common
    // directory prefix with "index.md", so it falls to the final
    // alphabetical rule) deterministically picks "alpha/readme.md" here —
    // pinned so the rest of the test is not guessing which file "wins".
    let post_files = walk_all_files(root);
    let idx = Indexes::build(&post_files);
    assert_eq!(idx.graph.resolve_path("readme", "index.md"), Some("alpha/readme.md".to_string()));

    do_move(root, "alpha/readme.md", "alpha/renamed.md");

    assert_eq!(r(root, "index.md"), "[[renamed]] link\n", "the winner's own rename must follow the bare reference");
}

#[test]
fn renaming_the_tiebreak_loser_of_two_same_stem_files_leaves_the_bare_wikilink_untouched() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "alpha/readme.md", "# Alpha readme\n");
    w(root, "beta/readme.md", "# Beta readme\n");
    w(root, "index.md", "[[readme]] link\n");

    do_move(root, "beta/readme.md", "beta/renamed.md");

    assert_eq!(r(root, "index.md"), "[[readme]] link\n", "the bare reference still means the untouched winner, alpha/readme.md");
}

// ── plan_moves validation ────────────────────────────────────────────────

#[test]
fn plan_moves_rejects_a_move_nested_inside_another_move_in_the_same_batch() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "dir/sub/leaf.md", "# Leaf\n");

    let old_dir = root.join("dir").to_string_lossy().to_string();
    let new_dir = root.join("dir2").to_string_lossy().to_string();
    let old_sub = root.join("dir/sub").to_string_lossy().to_string();
    let new_sub = root.join("dir/sub2").to_string_lossy().to_string();

    let err = plan_moves(root, &[(old_dir, new_dir), (old_sub, new_sub)]).unwrap_err();
    assert!(err.contains("inside another"), "got: {err}");

    // Nothing was touched — plan_moves is pure even when it errors.
    assert!(root.join("dir/sub/leaf.md").exists());
}

// ── Ablation harness note ───────────────────────────────────────────────
// See the report: `plan_one_ref`'s `resolved_pre.target_path` gate was
// disabled by hand (a scratch edit, never committed) to confirm the tests
// above fail without it, then restored via `git checkout --`.

// ── Destination already exists ───────────────────────────────────────────

fn snapshot_tree(root: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    walk_all_files(root).into_iter().map(|p| { let b = fs::read(root.join(&p)).unwrap(); (p, b) }).collect()
}

fn rename_core(root: &Path, old_rel: &str, new_rel: &str) -> Result<RenameApplyResult, String> {
    crate::editor::ref_scan::rename_entry_with_refs_core(
        root.to_path_buf(),
        &root.join(old_rel).to_string_lossy(),
        &root.join(new_rel).to_string_lossy(),
    )
}

#[test]
fn rename_onto_an_existing_file_fails_and_changes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "a.md", "# A\n");
    w(root, "b.md", "# B original\n");
    w(root, "r.md", "[[a]] [[b]]\n");
    let before = snapshot_tree(root);

    let err = rename_core(root, "a.md", "b.md").expect_err("destination exists");

    assert!(err.contains("already exists"), "error should say why: {err}");
    assert_eq!(snapshot_tree(root), before, "nothing may move or be rewritten");
}

#[test]
fn case_only_rename_still_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "media/Photo.jpg", "x");
    w(root, "r.md", "![](media/Photo.jpg)\n");

    rename_core(root, "media/Photo.jpg", "media/photo.jpg").expect("case-only rename");

    let names: Vec<String> =
        fs::read_dir(root.join("media")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, vec!["photo.jpg".to_string()]);
}

// ── Undo with several edits in one file ──────────────────────────────────

fn undo_restores_exactly(new_name: &str) {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "media/photo.jpg", "x");
    w(root, "blog/post.md", "![a](../media/photo.jpg)\n\ntext\n\n![b](../media/photo.jpg)\n\n![c](../media/photo.jpg)\n");
    let before = snapshot_tree(root);

    let applied = do_move(root, "media/photo.jpg", &format!("media/{new_name}"));
    assert_eq!(applied.edits.len(), 3);
    let undone = undo_applied(root, &applied).expect("undo");

    assert!(undone.skipped.is_empty(), "nothing was edited since: {:?}", undone.skipped);
    assert_eq!(snapshot_tree(root), before);
}

#[test]
fn undo_restores_a_file_with_several_edits_when_the_new_name_is_longer() {
    undo_restores_exactly("a-considerably-longer-file-name-than-before.jpg");
}

#[test]
fn undo_restores_a_file_with_several_edits_when_the_new_name_is_shorter() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "media/a-considerably-longer-file-name-than-before.jpg", "x");
    w(root, "post.md", "![a](media/a-considerably-longer-file-name-than-before.jpg) ![b](media/a-considerably-longer-file-name-than-before.jpg)\n");
    let before = snapshot_tree(root);

    let applied = do_move(root, "media/a-considerably-longer-file-name-than-before.jpg", "media/p.jpg");
    let undone = undo_applied(root, &applied).expect("undo");

    assert!(undone.skipped.is_empty(), "{:?}", undone.skipped);
    assert_eq!(snapshot_tree(root), before);
}

#[test]
fn undo_still_refuses_a_file_whose_rewritten_text_was_edited_afterwards() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "media/photo.jpg", "x");
    w(root, "post.md", "![a](media/photo.jpg)\n\n![b](media/photo.jpg)\n\n![c](media/photo.jpg)\n");

    let applied = do_move(root, "media/photo.jpg", "media/a-considerably-longer-file-name.jpg");
    let edited = r(root, "post.md").replacen("![c]", "![changed]", 1).replace("![b](media/a-considerably-longer-file-name.jpg)", "![b](media/other.jpg)");
    w(root, "post.md", &edited);
    let undone = undo_applied(root, &applied).expect("undo");

    assert_eq!(undone.skipped.len(), 1);
    assert_eq!(r(root, "post.md"), edited, "a user-edited file is left alone");
}

// ── A failure midway rolls everything back ───────────────────────────────

#[cfg(unix)]
#[test]
fn a_read_only_page_midway_rolls_the_whole_rename_back() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "media/photo.jpg", "x");
    for n in 1..=4 {
        w(root, &format!("a{n}.md"), "![](media/photo.jpg)\n");
    }
    let before = snapshot_tree(root);
    fs::set_permissions(root.join("a3.md"), fs::Permissions::from_mode(0o444)).unwrap();
    if fs::OpenOptions::new().write(true).open(root.join("a3.md")).is_ok() {
        eprintln!("skipped: this user can write a read-only file (running as root), so the failure cannot be staged");
        return;
    }

    let result = rename_core(root, "media/photo.jpg", "media/pic.jpg");

    fs::set_permissions(root.join("a3.md"), fs::Permissions::from_mode(0o644)).unwrap();
    let err = result.expect_err("a3.md cannot be written");
    assert!(err.contains("a3.md"), "the error names the page that failed: {err}");
    // The write was refused before a byte changed, so the page is untouched,
    // not possibly damaged.
    assert!(err.contains("nothing was left changed"), "{err}");
    assert!(!err.contains("damaged"), "{err}");
    assert_eq!(snapshot_tree(root), before, "photo back in place, every page byte-identical");
}

// ── Scanner edge cases that used to corrupt the rewritten line ──────────

// A footnote definition (`[^1]: …`) is not a link reference definition: its
// body is prose that may itself hold a link. The link must be rewritten as an
// ordinary link and the footnote label left alone.
#[test]
fn footnote_whose_body_is_a_link_keeps_its_link_when_the_target_is_renamed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "notes/alpha.md", "# Alpha\n");
    w(root, "index.md", "x[^1]\n\n[^1]: [t](notes/alpha.md)\n");

    do_move(root, "notes/alpha.md", "notes/beta.md");

    assert_eq!(r(root, "index.md"), "x[^1]\n\n[^1]: [t](notes/beta.md)\n");
}

// In a table cell a wikilink alias is written `[[target\|Alias]]`: the
// backslash escapes the pipe from the table, it is not part of the target.
// The rewrite must replace `target` only and keep the escape, or the bare `|`
// splits the cell.
#[test]
fn wikilink_alias_in_a_table_cell_keeps_its_escaped_pipe() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "notes/alpha.md", "# Alpha\n");
    w(root, "notes/zeta.md", "# Zeta\n");
    w(root, "index.md", "| a |\n|---|\n| [[alpha\\|Alias]] |\n");

    do_move(root, "notes/alpha.md", "notes/beta.md");

    assert_eq!(r(root, "index.md"), "| a |\n|---|\n| [[beta\\|Alias]] |\n");
}

// ── A rewritten destination must still be a link another tool can follow ─

/// What a real Markdown parser sees: every link/image destination in `src`,
/// as `(is_image, dest_url)`.
fn parsed_destinations(src: &str) -> Vec<(bool, String)> {
    use pulldown_cmark::{Event, Parser, Tag};
    Parser::new(src)
        .filter_map(|ev| match ev {
            Event::Start(Tag::Link { dest_url, .. }) => Some((false, dest_url.to_string())),
            Event::Start(Tag::Image { dest_url, .. }) => Some((true, dest_url.to_string())),
            _ => None,
        })
        .collect()
}

/// The file a destination names EXACTLY, the way a tool with no name search
/// would follow it from the page `from`: root-relative if it starts with `/`,
/// else relative to the page; `?query`/`#fragment` dropped; one pass of
/// percent-decoding. `None` when no such file exists.
fn exact_target(root: &Path, from: &str, dest: &str) -> Option<String> {
    let cut = dest.find(['?', '#']).unwrap_or(dest.len());
    let path = moss_core::resolve::fuzzy_path::percent_decode_path(&dest[..cut]);
    let joined = match path.strip_prefix('/') {
        Some(rest) => rest.to_string(),
        None => format!("{}/{}", dirname(from), path),
    };
    let norm = moss_core::content_graph::join_written("", &joined)?;
    if root.join(&norm).is_file() {
        return Some(norm);
    }
    // A published address (`/docs/`, `/blog/post/`): the page at that address.
    let addressy = path.ends_with('/') || !norm.rsplit('/').next().unwrap_or("").contains('.');
    let candidates = [format!("{norm}.md"), format!("{norm}/index.md")];
    if addressy {
        return candidates.into_iter().map(|c| c.trim_start_matches('/').to_string()).find(|c| root.join(c).is_file());
    }
    None
}

/// Rename `old` to `new`, then assert the page's text is exactly `want` and
/// that a real Markdown parser sees one link/image there whose destination
/// exactly names `new`.
fn assert_rewrite_names_exactly(root: &Path, page: &str, old: &str, new: &str, want: &str) {
    do_move(root, old, new);
    let out = r(root, page);
    let body = out.split_once("\n---\n").map_or(out.as_str(), |(_, b)| b);
    assert!(body.contains(want), "expected {want:?} in:\n{out}");
    let dests = parsed_destinations(&out);
    assert!(!dests.is_empty(), "the rewrite is no longer a link for a real parser:\n{out}");
    for (_, d) in dests {
        assert_eq!(exact_target(root, page, &d).as_deref(), Some(new), "destination {d:?} in:\n{out}");
    }
}

fn site_with_link(line: &str, target: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(tmp.path()).unwrap();
    w(&root, target, "x");
    w(&root, "index.md", &format!("{line}\n\n[u]: {target}\n\n[use][u]\n"));
    (tmp, root)
}

#[test]
fn rewritten_destination_with_a_space_stays_a_link() {
    let (_t, root) = site_with_link("[t](notes/alpha.md)", "notes/alpha.md");
    assert_rewrite_names_exactly(&root, "index.md", "notes/alpha.md", "notes/my page.md", "[t](notes/my%20page.md)");
    assert!(r(&root, "index.md").contains("[u]: notes/my%20page.md"));
}

#[test]
fn rewritten_image_with_a_space_stays_an_image() {
    let (_t, root) = site_with_link("![i](media/photo.jpg)", "media/photo.jpg");
    assert_rewrite_names_exactly(&root, "index.md", "media/photo.jpg", "media/my photo.jpg", "![i](media/my%20photo.jpg)");
}

#[test]
fn rewritten_destination_escapes_hash_question_percent_and_unbalanced_parens() {
    for (new, want) in [
        ("notes/a#b.md", "[t](notes/a%23b.md)"),
        ("notes/a?b.md", "[t](notes/a%3Fb.md)"),
        ("notes/a%20b.md", "[t](notes/a%2520b.md)"),
        ("notes/a(b.md", "[t](notes/a%28b.md)"),
        ("notes/a)b.md", "[t](notes/a%29b.md)"),
        ("notes/a(b).md", "[t](notes/a(b).md)"),
        ("notes/笔记 一.md", "[t](notes/笔记%20一.md)"),
    ] {
        let (_t, root) = site_with_link("[t](notes/alpha.md)", "notes/alpha.md");
        assert_rewrite_names_exactly(&root, "index.md", "notes/alpha.md", new, want);
    }
}

#[test]
fn rewritten_angle_bracket_destination_keeps_its_style() {
    let (_t, root) = site_with_link("[t](<notes/alpha.md>)", "notes/alpha.md");
    assert_rewrite_names_exactly(&root, "index.md", "notes/alpha.md", "notes/my (page.md", "[t](<notes/my %28page.md>)");

    let (_t, root) = site_with_link("[t](<notes/alpha.md>)", "notes/alpha.md");
    assert_rewrite_names_exactly(&root, "index.md", "notes/alpha.md", "notes/a#b.md", "[t](<notes/a%23b.md>)");
}

// A wikilink target cannot carry `|`, `#`, `?` or `]`; the rename is refused and
// nothing on disk changes, rather than writing a link that points elsewhere.
#[test]
fn wikilink_target_with_unrepresentable_characters_refuses_the_rename() {
    for new in ["notes/a|b.md", "notes/a#b.md", "notes/a]b.md", "notes/a?b.md"] {
        let tmp = tempfile::tempdir().unwrap();
        let root = &fs::canonicalize(tmp.path()).unwrap();
        w(root, "notes/alpha.md", "x");
        w(root, "index.md", "[[notes/alpha.md]]\n");
        let old = root.join("notes/alpha.md").to_string_lossy().to_string();
        let newp = root.join(new).to_string_lossy().to_string();
        let err = plan_moves(root, &[(old, newp)]).expect_err(new);
        assert!(err.contains("cannot be written in a wikilink"), "{new}: {err}");
        assert_eq!(r(root, "index.md"), "[[notes/alpha.md]]\n");
        assert!(root.join("notes/alpha.md").exists());
    }
}

// A reference that names its file exactly and cannot be respelled for the new
// name refuses the whole rename; the message says which page holds it.
#[test]
fn an_exact_reference_that_cannot_be_respelled_refuses_the_rename_and_names_its_page() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "notes/alpha.md", "x");
    w(root, "blog/post.md", "[[notes/alpha.md]]\n");
    w(root, "index.md", "[ok](notes/alpha.md)\n");
    let before = snapshot_tree(root);

    let err = rename_core(root, "notes/alpha.md", "notes/a|b.md").expect_err("cannot be respelled");

    assert!(err.contains("in blog/post.md") && err.contains("\"notes/alpha.md\""), "{err}");
    assert_eq!(snapshot_tree(root), before, "nothing moves and no page is rewritten");
}

// ── An exact path stays an exact path, in the same style ────────────────

fn site(files: &[(&str, &str)]) -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(tmp.path()).unwrap();
    for (rel, content) in files {
        w(&root, rel, content);
    }
    (tmp, root)
}

/// After the move, every real-parser destination in `page` exactly names a
/// file (no name search), and `page` reads `want`.
fn assert_page(root: &Path, page: &str, want: &str, target: &str) {
    let out = r(root, page);
    assert_eq!(out, want);
    for (_, d) in parsed_destinations(&out) {
        assert_eq!(exact_target(root, page, &d).as_deref(), Some(target), "destination {d:?} in:\n{out}");
    }
}

#[test]
fn renaming_a_target_keeps_a_page_relative_path_page_relative() {
    let (_t, root) = site(&[
        ("notes/alpha.md", "x"),
        ("media/photo.jpg", "x"),
        ("blog/post.md", "[t](../notes/alpha.md) ![i](../media/photo.jpg)\n"),
    ]);
    do_move(&root, "notes/alpha.md", "notes/beta.md");
    assert_eq!(r(&root, "blog/post.md"), "[t](../notes/beta.md) ![i](../media/photo.jpg)\n");
    do_move(&root, "media/photo.jpg", "media/pic.jpg");
    let out = r(&root, "blog/post.md");
    assert_eq!(out, "[t](../notes/beta.md) ![i](../media/pic.jpg)\n");
    let dests = parsed_destinations(&out);
    assert_eq!(exact_target(&root, "blog/post.md", &dests[0].1).as_deref(), Some("notes/beta.md"));
    assert_eq!(exact_target(&root, "blog/post.md", &dests[1].1).as_deref(), Some("media/pic.jpg"));
}

#[test]
fn renaming_an_asset_keeps_a_page_relative_frontmatter_and_gallery_path_page_relative() {
    let (_t, root) = site(&[
        ("media/photo.jpg", "x"),
        ("blog/post.md", "---\ncover: ../media/photo.jpg\n---\n\n:::gallery\n../media/photo.jpg\n:::\n"),
    ]);
    do_move(&root, "media/photo.jpg", "media/pic.jpg");
    assert_eq!(
        r(&root, "blog/post.md"),
        "---\ncover: ../media/pic.jpg\n---\n\n:::gallery\n../media/pic.jpg\n:::\n"
    );
}

#[test]
fn moving_a_target_rewrites_a_path_that_would_otherwise_only_resolve_by_name_search() {
    let (_t, root) = site(&[
        ("notes/alpha.md", "x"),
        ("blog/post.md", "[t](../notes/alpha.md) [r](/notes/alpha.md)\n"),
    ]);
    fs::create_dir_all(root.join("archive/deep")).unwrap();
    do_move(&root, "notes/alpha.md", "archive/deep/alpha.md");
    assert_eq!(
        r(&root, "blog/post.md"),
        "[t](../archive/deep/alpha.md) [r](/archive/deep/alpha.md)\n"
    );
}

#[test]
fn moving_the_referencing_page_rebases_exact_relative_paths() {
    let (_t, root) = site(&[
        ("blog/img/a.jpg", "x"),
        ("other.md", "x"),
        ("blog/post.md", "![](./img/a.jpg) [t](../other.md)\n"),
    ]);
    fs::create_dir_all(root.join("other/x")).unwrap();
    do_move(&root, "blog/post.md", "other/x/post.md");
    let out = r(&root, "other/x/post.md");
    assert_eq!(out, "![](../../blog/img/a.jpg) [t](../../other.md)\n");
    let dests = parsed_destinations(&out);
    assert_eq!(exact_target(&root, "other/x/post.md", &dests[0].1).as_deref(), Some("blog/img/a.jpg"));
    assert_eq!(exact_target(&root, "other/x/post.md", &dests[1].1).as_deref(), Some("other.md"));
}

#[test]
fn a_dot_slash_path_keeps_its_dot_slash_when_no_up_step_is_needed() {
    let (_t, root) = site(&[("blog/img/a.jpg", "x"), ("blog/post.md", "![](./img/a.jpg)\n")]);
    do_move(&root, "blog/img", "blog/pics");
    assert_page(&root, "blog/post.md", "![](./pics/a.jpg)\n", "blog/pics/a.jpg");
}

#[test]
fn a_same_folder_path_follows_a_target_that_moves_away() {
    let (_t, root) = site(&[("notes/alpha.md", "x"), ("notes/page.md", "[t](alpha.md)\n")]);
    fs::create_dir_all(root.join("archive")).unwrap();
    do_move(&root, "notes/alpha.md", "archive/alpha.md");
    assert_page(&root, "notes/page.md", "[t](../archive/alpha.md)\n", "archive/alpha.md");
}

#[test]
fn a_published_address_stays_a_published_address() {
    let (_t, root) = site(&[
        ("docs/index.md", "# Docs\n"),
        ("docs/intro.md", "x"),
        ("index.md", "[a](/docs/) [b](/docs/intro/) [c](/docs)\n"),
    ]);
    do_move(&root, "docs", "guide");
    assert_eq!(r(&root, "index.md"), "[a](/guide/) [b](/guide/intro/) [c](/guide)\n");
}

#[test]
fn a_path_shaped_wikilink_follows_a_move_but_a_bare_one_is_left_alone() {
    let (_t, root) = site(&[
        ("notes/alpha.md", "x"),
        ("index.md", "[[notes/alpha]] [[notes/alpha.md|A]] [[alpha]]\n"),
    ]);
    fs::create_dir_all(root.join("archive/deep")).unwrap();
    do_move(&root, "notes/alpha.md", "archive/deep/alpha.md");
    assert_eq!(
        r(&root, "index.md"),
        "[[archive/deep/alpha]] [[archive/deep/alpha.md|A]] [[alpha]]\n"
    );
}

#[test]
fn a_reference_that_only_resolved_by_name_search_keeps_the_existing_rule() {
    // `notes/alpha.md` is not a path relative to `blog/`, so this link works
    // only through name search; it is rewritten only if search would pick
    // another file, and a move that leaves it unique leaves it alone.
    let (_t, root) = site(&[("notes/alpha.md", "x"), ("blog/post.md", "[t](alpha.md) [u](notes/alpha.md)\n")]);
    fs::create_dir_all(root.join("archive")).unwrap();
    do_move(&root, "notes/alpha.md", "archive/alpha.md");
    assert_eq!(r(&root, "blog/post.md"), "[t](alpha.md) [u](notes/alpha.md)\n");
}

#[test]
fn folder_rename_onto_its_own_carried_home_name_is_refused_before_anything_moves() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "a/a.md", "# home\n");
    w(root, "a/b.md", "# other page\n");
    let before = snapshot_tree(root);

    let err = rename_core(root, "a", "b").expect_err("the carry would overwrite a/b.md");

    assert!(err.contains("already exists"), "{err}");
    assert_eq!(snapshot_tree(root), before, "folder must not be left renamed with its home uncarried");
    assert!(root.join("a").is_dir() && !root.join("b").exists());
}

#[test]
fn undo_of_a_folder_rename_refuses_to_overwrite_a_page_added_since() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "a/a.md", "# home\n");
    let applied = rename_core(root, "a", "c").expect("rename");
    w(root, "c/a.md", "# written after the rename\n");
    let before = snapshot_tree(root);

    let err = undo_applied(root, &applied).expect_err("undo carry would overwrite c/a.md");

    assert!(err.contains("already exists"), "{err}");
    assert_eq!(snapshot_tree(root), before);
    assert!(root.join("c").is_dir() && !root.join("a").exists());
}

#[cfg(unix)]
#[test]
fn rename_onto_a_symlink_or_hard_link_of_the_source_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "a.md", "# A\n");
    std::os::unix::fs::symlink(root.join("a.md"), root.join("sym.md")).unwrap();
    // allow:hard_link a hard link to the source is the case under test
    fs::hard_link(root.join("a.md"), root.join("hard.md")).unwrap();

    for dest in ["sym.md", "hard.md"] {
        let err = rename_core(root, "a.md", dest).expect_err(dest);
        assert!(err.contains("already exists"), "{dest}: {err}");
    }
    assert!(root.join("a.md").exists());
    assert!(fs::symlink_metadata(root.join("sym.md")).unwrap().file_type().is_symlink());
}

#[test]
fn a_later_move_failing_in_a_batch_rolls_back_the_earlier_moves_and_their_rewrites() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "media/one.jpg", "1");
    w(root, "media/two.jpg", "2");
    w(root, "page.md", "![](media/one.jpg) ![](media/two.jpg)\n");
    let abs = |rel: &str| root.join(rel).to_string_lossy().into_owned();
    let plan = plan_moves(
        root,
        &[(abs("media/one.jpg"), abs("media/uno.jpg")), (abs("media/two.jpg"), abs("media/dos.jpg"))],
    )
    .expect("plan");
    // Taken after planning, so the second move is the one that fails.
    w(root, "media/dos.jpg", "someone else's file");
    let before = snapshot_tree(root);

    let err = apply_planned_moves(root, &plan).expect_err("second move refused");

    assert!(err.contains("already exists") && err.contains("rolled back"), "{err}");
    assert_eq!(snapshot_tree(root), before, "first move undone, no page rewritten");
}

#[test]
fn rename_onto_an_existing_directory_is_refused_empty_or_not() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "a.md", "# A\n");
    w(root, "r.md", "[[a]]\n");
    fs::create_dir(root.join("empty")).unwrap();
    w(root, "full/inside.md", "# inside\n");
    fs::create_dir(root.join("folder")).unwrap();
    let before = snapshot_tree(root);

    for dest in ["empty", "full"] {
        let err = rename_core(root, "a.md", dest).expect_err(dest);
        assert!(err.contains("already exists"), "{dest}: {err}");
        let err = rename_core(root, "folder", dest).expect_err(dest);
        assert!(err.contains("already exists"), "{dest}: {err}");
    }
    assert_eq!(snapshot_tree(root), before);
    assert!(root.join("empty").is_dir() && root.join("folder").is_dir());
}

#[test]
fn undo_restores_an_edit_whose_new_text_has_the_same_length_as_the_old() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "media/photo.jpg", "x");
    w(root, "post.md", "![a](media/photo.jpg) and ![b](media/photo.jpg)\n");
    let before = snapshot_tree(root);

    let applied = do_move(root, "media/photo.jpg", "media/image.jpg");
    assert_eq!(r(root, "post.md"), "![a](media/image.jpg) and ![b](media/image.jpg)\n");
    let undone = undo_applied(root, &applied).expect("undo");

    assert!(undone.skipped.is_empty(), "{:?}", undone.skipped);
    assert_eq!(snapshot_tree(root), before);
}

#[test]
fn a_multi_word_title_survives_a_rename_byte_for_byte() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "media/p.jpg", "x");
    w(root, "notes/n.md", "# n\n");
    w(
        root,
        "page.md",
        "![i](media/p.jpg \"a title with spaces\")\n\n[n](notes/n.md 'two words')\n\n[n](<notes/n.md> (a  b))\n",
    );

    do_move(root, "media/p.jpg", "media/q.jpg");
    do_move(root, "notes/n.md", "notes/m.md");

    assert_eq!(
        r(root, "page.md"),
        "![i](media/q.jpg \"a title with spaces\")\n\n[n](notes/m.md 'two words')\n\n[n](<notes/m.md> (a  b))\n"
    );
}

// A folder address stops being one when its home page is renamed to a name
// that is not a home page. A destination written as an address stays an
// address: the page is now served at its own address, and a `.md` path after
// a leading `/` is kept verbatim by the build and would not be served.
#[test]
fn a_folder_address_becomes_the_address_of_the_renamed_page() {
    for page in ["page.md", "sub/p.md", "docs/p.md"] {
        let tmp = tempfile::tempdir().unwrap();
        let root = &fs::canonicalize(tmp.path()).unwrap();
        w(root, "docs/index.md", "# Docs\n");
        w(root, page, "[a](/docs/) [b](/docs)\n");

        do_move(root, "docs/index.md", "docs/intro.md");

        assert_eq!(r(root, page), "[a](/docs/intro/) [b](/docs/intro/)\n", "from {page}");
    }
}

// The reverse: a page that becomes its folder's home is served at the folder.
#[test]
fn a_page_address_becomes_the_folder_address_when_the_page_becomes_its_home() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "docs/intro.md", "# Docs\n");
    w(root, "p.md", "[a](/docs/intro/)\n");

    do_move(root, "docs/intro.md", "docs/index.md");

    assert_eq!(r(root, "p.md"), "[a](/docs/)\n");
}

// A path-shaped wikilink written page-relative stays a path even when the
// shortest relative spelling has no `/` left: a bare `[[intro.md]]` is a name
// the resolver searches for, which no other tool follows.
#[test]
fn a_page_relative_wikilink_never_collapses_to_a_bare_name() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "docs/a.md", "# a\n");
    w(root, "docs/p.md", "[[../docs/a.md]]\n");

    do_move(root, "docs/a.md", "docs/b.md");

    assert_eq!(r(root, "docs/p.md"), "[[./b.md]]\n");
}

// A `|` in a destination splits a table cell, so it is always percent-encoded;
// the Markdown parser must still see one link in the cell.
#[test]
fn a_pipe_in_a_new_name_is_encoded_so_a_table_cell_does_not_split() {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};
    for (link, want) in [("[t](notes/a.md)", "[t](notes/a%7Cb.md)"), ("[t](<notes/a.md>)", "[t](<notes/a%7Cb.md>)")] {
        let tmp = tempfile::tempdir().unwrap();
        let root = &fs::canonicalize(tmp.path()).unwrap();
        w(root, "notes/a.md", "# a\n");
        w(root, "page.md", &format!("| one | two |\n|---|---|\n| {link} | x |\n"));

        do_move(root, "notes/a.md", "notes/a|b.md");

        let page = r(root, "page.md");
        assert_eq!(page, format!("| one | two |\n|---|---|\n| {want} | x |\n"));
        let (mut cells, mut links) = (0, 0);
        for ev in Parser::new_ext(&page, moss_core::ast::parser::parser_options(false)) {
            match ev {
                Event::End(TagEnd::TableCell) => cells += 1,
                Event::Start(Tag::Link { .. }) => links += 1,
                _ => {}
            }
        }
        assert_eq!((cells, links), (4, 1), "{page}");
    }
}

// The Markdown parser keeps the backslash of `[[alpha\|Alias]]` in the
// destination, in a paragraph and in a table cell alike (`alpha\`); the build
// resolves that to the same file as `alpha`. The scanner reports `alpha` in
// both, so a rewrite replaces the name and leaves the `\|Alias` untouched.
#[test]
fn escaped_pipe_wikilink_is_read_alike_in_a_paragraph_and_a_table_cell() {
    use pulldown_cmark::{Event, LinkType, Parser, Tag};
    let paths = vec!["notes/alpha.md".to_string(), "index.md".to_string()];
    let idx = Indexes::build(&paths);
    for src in ["[[alpha\\|Alias]]\n", "| a |\n|---|\n| [[alpha\\|Alias]] |\n"] {
        let parsed: Vec<String> = Parser::new_ext(src, moss_core::ast::parser::parser_options(false))
            .filter_map(|ev| match ev {
                Event::Start(Tag::Link { dest_url, link_type: LinkType::WikiLink { .. }, .. }) => Some(dest_url.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(parsed, vec!["alpha\\".to_string()], "the parser keeps the backslash: {src:?}");
        let scanned = extract_md_references(src);
        assert_eq!(scanned.len(), 1, "{src:?}");
        assert_eq!(scanned[0].text, "alpha", "{src:?}");

        let urls = NoUrlIndex;
        let ctx = ReferenceContext { assets: &idx.graph, folders: &idx.folders, urls: &urls };
        let build_sees = resolve_by_route(RefRoute::PageGraph, &parsed[0], "index.md", &ctx, &idx.graph);
        let scanner_sees = resolve_by_route(RefRoute::PageGraph, &scanned[0].text, "index.md", &ctx, &idx.graph);
        assert_eq!(build_sees.as_deref(), Some("notes/alpha.md"), "{src:?}");
        assert_eq!(build_sees, scanner_sees, "{src:?}");
    }
}

#[test]
fn wikilink_alias_with_an_escaped_pipe_outside_a_table_keeps_its_backslash() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "notes/alpha.md", "# Alpha\n");
    w(root, "index.md", "See [[alpha\\|Alias]].\n");

    do_move(root, "notes/alpha.md", "notes/beta.md");

    assert_eq!(r(root, "index.md"), "See [[beta\\|Alias]].\n");
}

#[test]
fn moving_a_folder_keeps_links_inside_it_and_respells_the_ones_that_leave() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "x.md", "# x\n");
    w(root, "deep/keep.md", "# keep\n");
    w(root, "notes/b.md", "# b\n");
    let inside = "[b](b.md) [b](./b.md) [[b]] ![i](pic.jpg)\n";
    w(root, "notes/pic.jpg", "x");
    w(root, "notes/a.md", &format!("{inside}[out](../x.md)\n"));

    do_move(root, "notes", "deep/notes");

    assert_eq!(r(root, "deep/notes/a.md"), format!("{inside}[out](../../x.md)\n"));
}

#[test]
fn a_batch_moving_both_the_page_and_its_target_ends_with_the_reference_correct() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "p.md", "[t](t.md) ![i](pic.jpg)\n");
    w(root, "t.md", "# t\n");
    w(root, "pic.jpg", "x");
    fs::create_dir(root.join("sub")).unwrap();
    fs::create_dir(root.join("other")).unwrap();
    let abs = |rel: &str| root.join(rel).to_string_lossy().into_owned();
    let plan = plan_moves(
        root,
        &[(abs("p.md"), abs("sub/p.md")), (abs("t.md"), abs("other/t.md")), (abs("pic.jpg"), abs("sub/pic.jpg"))],
    )
    .expect("plan");

    apply_planned_moves(root, &plan).expect("apply");

    assert_eq!(r(root, "sub/p.md"), "[t](../other/t.md) ![i](pic.jpg)\n");
}

// A path-shaped wikilink whose target moves to the site root has no `/` left
// in its root-relative spelling. `[[./a]]` is the page-relative spelling and
// reads as a different style, so the wikilink falls back to the bare-name
// style the target now fits.
#[test]
fn a_path_shaped_wikilink_follows_its_target_to_the_site_root() {
    let (_t, root) = site(&[("notes/a.md", "x"), ("p.md", "[[notes/a]] [[notes/a.md|A]]\n")]);

    do_move(&root, "notes/a.md", "a.md");

    assert_eq!(r(&root, "p.md"), "[[a]] [[a.md|A]]\n");
}

#[test]
fn a_path_shaped_wikilink_moved_to_the_root_still_finds_it_beside_another_file_of_that_name() {
    let (_t, root) = site(&[
        ("notes/a.md", "x"),
        ("other/a.md", "y"),
        ("p.md", "[[notes/a]]\n"),
    ]);

    do_move(&root, "notes/a.md", "a.md");

    let out = r(&root, "p.md");
    assert_eq!(page_graph_targets(&root, "p.md"), vec![Some("a.md".to_string())], "{out}");
}

/// What the build's page resolver makes of every reference in `page`.
fn page_graph_targets(root: &Path, page: &str) -> Vec<Option<String>> {
    let files = walk_all_files(root);
    let idx = Indexes::build(&files);
    let urls = NoUrlIndex;
    let ctx = ReferenceContext { assets: &idx.graph, folders: &idx.folders, urls: &urls };
    let source = r(root, page);
    extract_md_references(&source)
        .iter()
        .map(|rr| resolve_by_route(RefRoute::PageGraph, &rr.text, page, &ctx, &idx.graph))
        .collect()
}

// A rooted destination that is not the file's exact root path (`/alpha.md` for
// `notes/alpha.md`) resolves only by name search: a rename that breaks the
// search respells it to a path that names the file, a move that keeps the
// name leaves it as written.
#[test]
fn a_rooted_reference_that_only_resolves_by_name_is_respelled_by_a_rename_and_kept_by_a_move() {
    let files = [("notes/alpha.md", "x"), ("blog/p.md", "[l](/alpha.md)\n")];
    let (_t, root) = site(&files);
    do_move(&root, "notes/alpha.md", "notes/beta.md");
    assert_page(&root, "blog/p.md", "[l](../notes/beta.md)\n", "notes/beta.md");

    let (_t, root) = site(&files);
    fs::create_dir_all(root.join("archive")).unwrap();
    do_move(&root, "notes/alpha.md", "archive/alpha.md");
    assert_eq!(r(&root, "blog/p.md"), "[l](/alpha.md)\n");
}

// A folder embed written rooted, with a trailing slash, follows its folder.
#[test]
fn a_rooted_folder_embed_with_a_trailing_slash_follows_a_folder_rename() {
    let (_t, root) = site(&[
        ("posts/index.md", "x"),
        ("posts/a.md", "x"),
        ("blog/p.md", "![[/posts/]] ![[/posts/|style:grid]]\n"),
    ]);
    do_move(&root, "posts", "articles");
    assert_eq!(r(&root, "blog/p.md"), "![[/articles/]] ![[/articles/|style:grid]]\n");
}

// The scanner reads `p.jpg "t" "u"` as the destination `p.jpg`; Markdown does
// not treat that text as a link at all. Recorded so a change to either side is
// noticed.
#[test]
fn the_scanner_accepts_a_destination_followed_by_two_titles_that_markdown_does_not_link() {
    let src = "![i](p.jpg \"t\" \"u\")\n";
    assert!(parsed_destinations(src).is_empty(), "markdown no longer treats this as an image");
    let found: Vec<String> = extract_md_references(src).into_iter().map(|r| r.text).collect();
    assert_eq!(found, vec!["p.jpg".to_string()]);
}

// The address a page is served at after a rename comes from the build's own
// page map, read from the files where they are now. A page that cannot be read
// is left out of that map, as the build leaves it out, and does not stop a
// link to another page from following its rename.
#[cfg(unix)]
#[test]
fn an_unreadable_page_elsewhere_does_not_stop_a_published_address_from_following() {
    use std::os::unix::fs::PermissionsExt;
    let (_t, root) = site(&[
        ("notes/beta.md", "# Beta\n"),
        ("blog/post.md", "[b](/notes/beta/)\n"),
        ("drafts/locked.md", "# Locked\n"),
    ]);
    fs::set_permissions(root.join("drafts/locked.md"), fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read(root.join("drafts/locked.md")).is_ok() {
        eprintln!("skipped: this user can read an unreadable file (running as root), so the failure cannot be staged");
        return;
    }

    let old = root.join("notes/beta.md").to_string_lossy().into_owned();
    let new = root.join("notes/Gamma Two.md").to_string_lossy().into_owned();
    let planned = plan_moves(&root, &[(old, new)]);

    fs::set_permissions(root.join("drafts/locked.md"), fs::Permissions::from_mode(0o644)).unwrap();
    let plan = planned.expect("plan");
    apply_planned_moves(&root, &plan).expect("apply");
    assert_eq!(r(&root, "blog/post.md"), "[b](/notes/gamma-two/)\n");
}

// A folder home and its translation share the folder's address in the page
// map; the site serves the translation under its language's prefix, and a
// link written to that address follows the translation.
#[test]
fn a_link_to_a_translated_home_follows_that_translation() {
    let (_t, root) = site(&[
        (".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n"),
        ("docs/index.md", "# Docs\n"),
        ("docs/index.zh-hans.md", "# Docs in Chinese\n"),
        ("index.md", "[d](/docs/) [z](/zh-hans/docs/)\n"),
    ]);
    do_move(&root, "docs", "guide");
    assert_eq!(r(&root, "index.md"), "[d](/guide/) [z](/zh-hans/guide/)\n");
}

// A link written as a published address cannot follow a page to a name the
// site does not serve as a page; the refusal says so.
#[test]
fn moving_a_linked_page_where_it_is_not_served_refuses_and_says_why() {
    let (_t, root) = site(&[("notes/beta.md", "# Beta\n"), ("blog/post.md", "[b](/notes/beta/)\n")]);
    let abs = |rel: &str| root.join(rel).to_string_lossy().into_owned();

    let err = plan_moves(&root, &[(abs("notes/beta.md"), abs("footer.md"))]).expect_err("footer.md is not a page");

    assert!(err.contains("blog/post.md") && err.contains("would not be served as a page"), "{err}");
}

// `/` is the site's front door whichever file is served there, so a link to it
// stays `/` when the home page is renamed.
#[test]
fn a_link_to_the_site_root_stays_the_site_root() {
    let (_t, root) = site(&[("index.md", "# Home\n"), ("docs/p.md", "[Home](/) [top](/#start)\n")]);
    do_move(&root, "index.md", "about.md");
    assert_eq!(r(&root, "docs/p.md"), "[Home](/) [top](/#start)\n");
}

// The build's folder walk does not follow a symbolic link, so neither does
// the rename: the page is rewritten once, through its real path.
#[cfg(unix)]
#[test]
fn a_page_reached_through_a_symbolic_link_is_rewritten_once_through_its_real_path() {
    let (_t, root) = site(&[("notes/beta.md", "# Beta\n"), ("real.md", "[b](notes/beta.md)\n")]);
    std::os::unix::fs::symlink(root.join("real.md"), root.join("alias.md")).unwrap();

    let applied = do_move(&root, "notes/beta.md", "notes/gamma.md");

    assert!(applied.skipped.is_empty(), "{:?}", applied.skipped);
    assert_eq!(applied.edits.iter().map(|e| e.file.as_str()).collect::<Vec<_>>(), vec!["real.md"]);
    assert_eq!(r(&root, "real.md"), "[b](notes/gamma.md)\n");
    assert!(fs::symlink_metadata(root.join("alias.md")).unwrap().file_type().is_symlink());
}

// A page inside a nested site belongs to that site, not this one: moving it
// out is not refused on its account, and a link to an address this site never
// served is left as written.
#[test]
fn moving_a_page_out_of_a_nested_site_is_not_refused() {
    let (_t, root) = site(&[
        ("inner/.moss/config.toml", "schema_version = 6\n"),
        ("inner/beta.md", "# Beta\n"),
        ("blog/post.md", "[b](/inner/beta/)\n"),
    ]);
    fs::create_dir_all(root.join("notes")).unwrap();

    do_move(&root, "inner/beta.md", "notes/beta.md");

    assert_eq!(r(&root, "blog/post.md"), "[b](/inner/beta/)\n");
}

// Working out where pages are served reads the site; it never asks the cloud
// provider to download a page that is not on disk.
#[test]
fn planning_never_asks_for_an_offline_page_to_be_downloaded() {
    use crate::build::icloud::pretend;
    let (_t, root) = site(&[
        ("notes/beta.md", "# Beta\n"),
        ("blog/post.md", "[b](/notes/beta/)\n"),
        ("drafts/away.md", "# Away\n"),
    ]);
    let _away = pretend::evicted(&root.join("drafts/away.md"));

    do_move(&root, "notes/beta.md", "notes/gamma.md");

    assert_eq!(r(&root, "blog/post.md"), "[b](/notes/gamma/)\n");
    assert_eq!(pretend::requests_for(&root.join("drafts/away.md")), 0);
}

// When the page a published address names is offline, its new address cannot
// be worked out; the refusal says why and nothing is downloaded.
#[test]
fn an_offline_target_of_a_published_address_refuses_and_says_it_is_offline() {
    use crate::build::icloud::pretend;
    let (_t, root) = site(&[("notes/beta.md", "# Beta\n"), ("blog/post.md", "[b](/notes/beta/)\n")]);
    let _away = pretend::evicted(&root.join("notes/beta.md"));
    let abs = |rel: &str| root.join(rel).to_string_lossy().into_owned();

    let err = plan_moves(&root, &[(abs("notes/beta.md"), abs("notes/gamma.md"))]).expect_err("offline");

    assert!(err.contains("blog/post.md") && err.contains("not downloaded"), "{err}");
    assert_eq!(pretend::requests_for(&root.join("notes/beta.md")), 0);
}

// The files whose own links a rename rewrites are the site's pages, the agent
// instruction files in the site folder, and every file being moved, wherever
// it comes from; the build leaves the last two out of the site, but their
// links still have to keep working.
#[test]
fn a_page_moved_out_of_a_hidden_folder_has_its_own_links_rewritten() {
    let (_t, root) = site(&[("img/a.png", "x"), (".drafts/x.md", "![](../img/a.png)\n")]);
    fs::create_dir_all(root.join("posts/2026")).unwrap();
    do_move(&root, ".drafts/x.md", "posts/2026/x.md");
    assert_eq!(r(&root, "posts/2026/x.md"), "![](../../img/a.png)\n");
}

#[test]
fn a_root_agent_instruction_file_has_its_links_rewritten() {
    let (_t, root) = site(&[("notes/style.md", "# Style\n"), ("CLAUDE.md", "[s](notes/style.md)\n")]);
    do_move(&root, "notes/style.md", "notes/house-style.md");
    assert_eq!(r(&root, "CLAUDE.md"), "[s](notes/house-style.md)\n");
}

#[test]
fn a_page_moved_out_of_a_nested_site_has_its_own_links_rewritten() {
    let (_t, root) = site(&[
        ("img/a.png", "x"),
        ("inner/.moss/config.toml", "schema_version = 6\n"),
        ("inner/x.md", "![](../img/a.png)\n"),
    ]);
    fs::create_dir_all(root.join("posts/2026")).unwrap();
    do_move(&root, "inner/x.md", "posts/2026/x.md");
    assert_eq!(r(&root, "posts/2026/x.md"), "![](../../img/a.png)\n");
}

#[test]
fn a_page_with_an_upper_case_extension_is_rewritten() {
    let (_t, root) = site(&[("notes/a.md", "# A\n"), ("Post.MD", "[a](notes/a.md)\n")]);
    do_move(&root, "notes/a.md", "notes/b.md");
    assert_eq!(r(&root, "Post.MD"), "[a](notes/b.md)\n");
}

#[test]
fn moving_a_folder_leaves_pages_the_site_leaves_out_inside_it_untouched() {
    let (_t, root) = site(&[
        ("img/a.png", "x"),
        ("tools/guide.md", "![](../img/a.png)\n"),
        ("tools/node_modules/pkg/readme.md", "![](../../../img/a.png)\n"),
        ("tools/.cache/note.md", "![](../../img/a.png)\n"),
        ("tools/shop/.moss/config.toml", "schema_version = 6\n"),
        ("tools/shop/page.md", "![](../../img/a.png)\n"),
    ]);
    fs::create_dir_all(root.join("deep")).unwrap();
    do_move(&root, "tools", "deep/tools");
    assert_eq!(r(&root, "deep/tools/guide.md"), "![](../../img/a.png)\n");
    assert_eq!(r(&root, "deep/tools/node_modules/pkg/readme.md"), "![](../../../img/a.png)\n");
    assert_eq!(r(&root, "deep/tools/.cache/note.md"), "![](../../img/a.png)\n");
    assert_eq!(r(&root, "deep/tools/shop/page.md"), "![](../../img/a.png)\n");
}

#[test]
fn moving_a_nested_site_or_hidden_folder_rewrites_its_own_pages_but_not_what_it_leaves_out() {
    let (_t, root) = site(&[
        ("img/a.png", "x"),
        ("shop/.moss/config.toml", "schema_version = 6\n"),
        ("shop/page.md", "![](../img/a.png)\n"),
        ("shop/node_modules/x.md", "![](../../img/a.png)\n"),
        (".drafts/x.md", "![](../img/a.png)\n"),
        (".drafts/node_modules/y.md", "![](../../img/a.png)\n"),
    ]);
    fs::create_dir_all(root.join("deep")).unwrap();
    do_move(&root, "shop", "deep/shop");
    do_move(&root, ".drafts", "deep/.drafts");
    assert_eq!(r(&root, "deep/shop/page.md"), "![](../../img/a.png)\n");
    assert_eq!(r(&root, "deep/shop/node_modules/x.md"), "![](../../img/a.png)\n");
    assert_eq!(r(&root, "deep/.drafts/x.md"), "![](../../img/a.png)\n");
    assert_eq!(r(&root, "deep/.drafts/node_modules/y.md"), "![](../../img/a.png)\n");
}

// Moving a root page into its own folder as that folder's home keeps its
// address (`/events/`), so root-relative links to it, with or without the
// trailing slash, are already right and stay byte-identical.
#[test]
fn a_page_becoming_its_folders_home_leaves_root_relative_links_to_its_address_alone() {
    let (_t, root) = site(&[
        ("events.md", "# Events\n"),
        ("events/past.md", "x"),
        ("index.md", "[a](/events) [b](/events/) [c](/events#top) [d](/events/past/)\n"),
    ]);
    do_move(&root, "events.md", "events/events.md");
    assert_eq!(r(&root, "index.md"), "[a](/events) [b](/events/) [c](/events#top) [d](/events/past/)\n");
}

// A page moved to another folder is served at a new address, so a
// root-relative link to its old address follows it, keeping its slash habit.
#[test]
fn moving_a_page_to_another_folder_rewrites_root_relative_links_to_its_address() {
    let (_t, root) = site(&[
        ("events.md", "# Events\n"),
        ("archive/old.md", "x"),
        ("index.md", "[a](/events) [b](/events/) [c](/events/#top)\n"),
    ]);
    do_move(&root, "events.md", "archive/events.md");
    assert_eq!(r(&root, "index.md"), "[a](/archive/events/) [b](/archive/events/) [c](/archive/events/#top)\n");
}

// The reverse of the folder-home case: the folder home becomes an ordinary
// page next to its folder, still served at the old folder address.
#[test]
fn a_folder_home_leaving_its_folder_keeps_root_relative_links_when_the_address_is_unchanged() {
    let (_t, root) = site(&[
        ("events/events.md", "# Events\n"),
        ("events/past.md", "x"),
        ("index.md", "[a](/events) [b](/events/)\n[rel](./events/events.md)\n"),
    ]);
    do_move(&root, "events/events.md", "events.md");
    assert_eq!(r(&root, "index.md").lines().next().unwrap(), "[a](/events) [b](/events/)");
}

// Page-relative links name the file, not its address, so they follow the file
// into the folder even though the address did not change.
#[test]
fn a_page_becoming_its_folders_home_still_rewrites_page_relative_links() {
    let (_t, root) = site(&[
        ("events.md", "# Events\n"),
        ("events/past.md", "x"),
        ("index.md", "[a](./events.md) [b](events.md) [c](/events)\n"),
    ]);
    do_move(&root, "events.md", "events/events.md");
    assert_eq!(r(&root, "index.md"), "[a](./events/events.md) [b](events/events.md) [c](/events)\n");
}

// A query string rides along on a rewritten address and is ignored when the
// address is unchanged.
#[test]
fn a_query_string_on_a_root_relative_address_is_kept() {
    let (_t, root) = site(&[
        ("events.md", "# Events\n"),
        ("archive/old.md", "x"),
        ("index.md", "[a](/events?page=2) [b](/events/?page=2#top)\n"),
    ]);
    do_move(&root, "events.md", "archive/events.md");
    assert_eq!(r(&root, "index.md"), "[a](/archive/events/?page=2) [b](/archive/events/?page=2#top)\n");

    let (_t, root) = site(&[
        ("events.md", "# Events\n"),
        ("events/past.md", "x"),
        ("index.md", "[a](/events?page=2) [b](/events/?page=2#top)\n"),
    ]);
    do_move(&root, "events.md", "events/events.md");
    assert_eq!(r(&root, "index.md"), "[a](/events?page=2) [b](/events/?page=2#top)\n");
}

// A root-relative link to a file (not a page address) stays on the path
// route: it follows the file, and the address lookup never claims it.
#[test]
fn a_root_relative_file_link_stays_on_the_path_route() {
    let (_t, root) = site(&[
        ("files/a.pdf", "x"),
        ("docs/b.pdf", "x"),
        ("index.md", "[a](/files/a.pdf)\n"),
    ]);
    do_move(&root, "files/a.pdf", "docs/a.pdf");
    assert_eq!(r(&root, "index.md"), "[a](/docs/a.pdf)\n");
}

// A `url:` override is relative to the page's folder, so moving the file
// changes the address it is served at (`/calendar/` becomes
// `/archive/calendar/`); the link follows the address the build computes, not
// the file name.
#[test]
fn a_url_override_moves_with_the_folder_and_the_link_follows_it() {
    let (_t, root) = site(&[
        ("events.md", "---\nurl: /calendar/\n---\n# Events\n"),
        ("archive/old.md", "x"),
        ("index.md", "[a](/calendar/) [b](/calendar)\n"),
    ]);
    do_move(&root, "events.md", "archive/events.md");
    assert_eq!(r(&root, "index.md"), "[a](/archive/calendar/) [b](/archive/calendar/)\n");
}

// An address served only by a `[redirects]` entry is not a page: the redirect
// serves it whatever happens to the page it points at, so rename leaves it.
#[test]
fn a_link_to_a_redirect_source_is_left_alone() {
    let (_t, root) = site(&[
        (".moss/config.toml", "schema_version = 6\n[redirects]\n\"/old-events/\" = \"/events/\"\n"),
        ("events.md", "# Events\n"),
        ("archive/old.md", "x"),
        ("index.md", "[a](/old-events/) [b](/old-events)\n"),
    ]);
    do_move(&root, "events.md", "archive/events.md");
    assert_eq!(r(&root, "index.md"), "[a](/old-events/) [b](/old-events)\n");
}
