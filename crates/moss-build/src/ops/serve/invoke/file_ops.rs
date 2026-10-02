//! File-operation arms for the mutation carrier — the tree's rename, delete
//! and reference-cleanup commands, plus the batch form of delete. Split out
//! of `invoke.rs` so this slice's growth doesn't push that file over its size
//! baseline; `invoke.rs`'s `mod file_ops;` wires this in, and its `carrier!`
//! lists reference these arms by path. Same rules as every arm in the parent
//! file: call the SAME core the real command calls, and route every
//! path-shaped argument through `super::confine`.

use super::*;

/// `delete_entry(path)` — now ONE call into [`arm_delete_entries`], the batch
/// form's one-element case. There is exactly one deletion arm, matching
/// `vault::fs::delete_entries_inner` being the one deletion core underneath.
pub(super) async fn arm_delete_entry(ctx: &Session, args: Value) -> ArmResult {
    #[derive(serde::Deserialize)]
    struct A {
        path: String,
    }
    let a: A = parse_args(args)?;
    arm_delete_entries(ctx, json!({ "paths": [a.path] })).await
}

/// `delete_entries(paths)` — the tree's multi-select delete. Calls the SAME
/// `delete_entries_inner` core the desktop's batch delete command calls; each
/// path is still individually guarded by that core's own `validate_entry_path`
/// (containment + symlink-escape recheck), the same defence the single-path
/// arm always had.
pub(super) async fn arm_delete_entries(ctx: &Session, args: Value) -> ArmResult {
    #[derive(serde::Deserialize)]
    struct A {
        paths: Vec<String>,
    }
    let a: A = parse_args(args)?;
    let root = project_root(ctx);
    let joined: Vec<String> = a
        .paths
        .iter()
        .map(|p| confine(ctx, p).map(|pb| pb.to_string_lossy().into_owned()))
        .collect::<Result<Vec<String>, ArmError>>()?;
    crate::vault::fs::delete_entries_inner(&root, &joined).map_err(ArmError::Command)?;
    to_value(())
}

/// `scan_references_for_delete(path)` — the delete confirmation's reference
/// count, read-only in effect but tiered with the other file-operation
/// commands rather than the public read-only carrier (it exposes project
/// structure, like the authed-read tier's own reads). Calls the SAME
/// `scan_project_references_to` core the desktop's delete-confirmation
/// command calls.
pub(super) async fn arm_scan_references_for_delete(ctx: &Session, args: Value) -> ArmResult {
    #[derive(serde::Deserialize)]
    struct A {
        path: String,
    }
    let a: A = parse_args(args)?;
    let root = project_root(ctx);
    let target = confine(ctx, &a.path)?;
    let hits = crate::editor::ref_scan::scan_project_references_to(&target, &root)
        .map_err(ArmError::Command)?;
    to_value(hits)
}

/// `clean_references_and_delete(paths)` — strip every reference to `paths`
/// from the project's markdown, then trash the paths themselves through the
/// SAME `delete_entries_inner` core `delete_entries` uses — never a second
/// delete implementation.
pub(super) async fn arm_clean_references_and_delete(ctx: &Session, args: Value) -> ArmResult {
    #[derive(serde::Deserialize)]
    struct A {
        paths: Vec<String>,
    }
    let a: A = parse_args(args)?;
    let root = project_root(ctx);
    let joined: Vec<String> = a
        .paths
        .iter()
        .map(|p| confine(ctx, p).map(|pb| pb.to_string_lossy().into_owned()))
        .collect::<Result<Vec<String>, ArmError>>()?;
    crate::editor::ref_scan::clean_references_to_paths(&root, &joined).map_err(ArmError::Command)?;
    crate::vault::fs::delete_entries_inner(&root, &joined).map_err(ArmError::Command)?;
    to_value(())
}

/// `rename_entry_with_refs(old_path, new_path)` — the tree's rename, with
/// every project-wide reference rewritten to follow it. Calls the SAME
/// `rename_entry_with_refs_core` the desktop's rename command and `moss
/// rename` both call, and returns its `RenameApplyResult` so the caller can
/// later send it back to `undo_rename`.
pub(super) async fn arm_rename_entry_with_refs(ctx: &Session, args: Value) -> ArmResult {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct A {
        old_path: String,
        new_path: String,
    }
    let a: A = parse_args(args)?;
    let root = project_root(ctx);
    let old = confine(ctx, &a.old_path)?;
    let new = confine(ctx, &a.new_path)?;
    let r = crate::editor::ref_scan::rename_entry_with_refs_core(
        root,
        &old.to_string_lossy(),
        &new.to_string_lossy(),
    )
    .map_err(ArmError::Command)?;
    to_value(r)
}

/// `undo_rename(applied)` — reverse a `rename_entry_with_refs` result. Calls
/// the SAME `undo_applied` core the desktop's undo-toast command calls.
///
/// `undo_applied` itself joins every path inside `applied` onto the project
/// root with no containment check of its own — it trusts the plan it was
/// handed, which on the desktop is always a value this same process just
/// produced. Over HTTP the request body is attacker-controlled, so the
/// carrier confines every path first, through the SAME `confine` helper
/// every other path-taking arm uses, before `applied` ever reaches the core.
pub(super) async fn arm_undo_rename(ctx: &Session, args: Value) -> ArmResult {
    #[derive(serde::Deserialize)]
    struct A {
        applied: crate::editor::rename_plan::RenameApplyResult,
    }
    let a: A = parse_args(args)?;
    for mv in &a.applied.moves {
        confine(ctx, &mv.old_path)?;
        confine(ctx, &mv.new_path)?;
    }
    for e in &a.applied.edits {
        confine(ctx, &e.file)?;
    }
    let root = project_root(ctx);
    let r = crate::editor::rename_plan::undo_applied(&root, &a.applied).map_err(ArmError::Command)?;
    to_value(r)
}

/// `resolve_attachment_dir(pageRelativePath)` — the project-relative,
/// forward-slash directory (`""` = project root) where an image dropped on
/// that page belongs. Same request and response as the desktop command: the
/// SAME `load_attachment_folder` + `attachment_dir_for_page` pair answers it.
/// Unlike the desktop command it does not create the folder, because this sits
/// on the read tier; the upload route creates its target when the first file
/// lands. The page path is only ever a key into path arithmetic, but it is
/// confined anyway so the arm refuses what every other path-taking arm refuses.
pub(super) async fn arm_resolve_attachment_dir(ctx: &Session, args: Value) -> ArmResult {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct A {
        page_relative_path: String,
    }
    let a: A = parse_args(args)?;
    confine(ctx, &a.page_relative_path)?;
    let root = project_root(ctx);
    let raw = crate::build::site_config::load_attachment_folder(&root.to_string_lossy());
    to_value(moss_core::attachment::attachment_dir_for_page(
        &raw,
        &a.page_relative_path.replace('\\', "/"),
    ))
}

/// `resolve_page_source(urlPath)` — the source page that produces a served URL
/// (the root for `""` or `"/"`). Same request and response as the desktop
/// command; the SAME `editor::page_source::resolve_page_source` core answers
/// both. A non-root URL is joined onto the vault by the core, so it goes
/// through `confine` first and a `..` URL is refused like any escaping path.
pub(super) async fn arm_resolve_page_source(ctx: &Session, args: Value) -> ArmResult {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct A {
        url_path: String,
    }
    let a: A = parse_args(args)?;
    let url = a.url_path.trim_start_matches('/');
    if !url.is_empty() {
        confine(ctx, url)?;
    }
    let page = crate::editor::page_source::resolve_page_source(url, &project_root(ctx))
        .map_err(ArmError::Command)?;
    to_value(page)
}
