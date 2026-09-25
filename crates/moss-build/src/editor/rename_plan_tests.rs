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
    let resolved = resolve_by_route(ref_route(&rr.syntax), &rr.text, "merged/x.md", &ReferenceContext { assets: &GraphAssetIndex(&idx.graph), folders: &idx.folders, urls: &NoUrlIndex }, &idx.graph);
    assert_eq!(resolved.as_deref(), Some("merged/y.md"), "x's link to y must resolve to the moved y, text was {:?}", rr.text);

    let hub_out = r(root, "hub.md");
    for rr in extract_md_references(&hub_out) {
        let resolved = resolve_by_route(ref_route(&rr.syntax), &rr.text, "hub.md", &ReferenceContext { assets: &GraphAssetIndex(&idx.graph), folders: &idx.folders, urls: &NoUrlIndex }, &idx.graph);
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
    // resolving through `resolve_asset_ref`'s own separator-fallback tier
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
/// super::*`) — not the live editor's FS-backed `FsAssetIndex`. The two
/// disagree on one real edge case: `FsAssetIndex::contains` does a bare
/// `read_dir` match with no file/dir distinction, so a bare reference that
/// happens to equal its own containing FOLDER's name (`[[旧夹]]` next to a
/// folder literally named `旧夹`) resolves LITERALLY to the folder itself
/// there (an `Other`-extension Link, so `target_path` is `None`) and never
/// reaches the embed-retry-as-`.md` fallback that would find the folder
/// note. `ContentGraph`-backed `GraphAssetIndex` only indexes real FILES, so
/// the same bare name falls through to that fallback and finds the note —
/// which is also what `ContentGraph::resolve_path` (the actual page-render
/// build's own resolver, confirmed via `ast/resolve_urls.rs`) gives via its
/// filename-stem index. This helper mirrors the planner, not the editor.
fn resolves_to(root: &Path, target: &str) -> usize {
    let pre_files = walk_all_files(root);
    let idx = Indexes::build(&pre_files);
    let assets = GraphAssetIndex(&idx.graph);
    let urls = NoUrlIndex;
    let ctx = ReferenceContext { assets: &assets, folders: &idx.folders, urls: &urls };
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
// `classify_reference`/`resolve_asset_ref` is not what the build uses to
// render a plain link or a non-embed wikilink — that's
// `ContentGraph::resolve_path` (see `RefRoute`'s doc comment). These three
// fixtures are the divergences a reviewer found with scratch tests: an
// index-stem folder home (`notes/index.md`) that the build's folder-note
// fallback resolves unconditionally for a bare `[[notes]]` but
// `resolve_asset_ref` never finds; and two same-stem files, where the build
// always resolves to a deterministic tiebreak winner rather than reporting
// ambiguity.

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
    let ctx = ReferenceContext { assets: &GraphAssetIndex(&idx.graph), folders: &idx.folders, urls: &NoUrlIndex };
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
