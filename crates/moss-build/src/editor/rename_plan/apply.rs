//! Applying a [`RenamePlan`] to the project, and taking it back.
//!
//! [`apply_planned_moves`] is the only place in the rename engine that writes:
//! it moves the entries and rewrites the pages, and when either fails it
//! undoes what already happened with [`undo_applied`]. Planning is
//! `rename_plan`'s; this half only replays a finished plan.

use std::collections::HashSet;
use std::path::Path;

use super::{
    expand_with_home_carry, map_path, walk_all_files, AppliedEdit, PlannedEdit, RenameApplyResult, RenamePlan,
    ResolvedMove, SkippedFile, UndoResult,
};
use crate::editor::ref_rewrite::{apply_edits, Edit};

/// Apply a [`RenamePlan`]: OS-rename every entry (via `rename_entry_inner`,
/// which carries a folder's self-named home file the same way a plain
/// rename does), then write exactly the planned edits. Before writing each
/// file, every one of its edits must still match its recorded position and
/// text — a mismatch (the file changed since planning) skips the WHOLE file
/// rather than clobbering it or guessing a new position.
///
/// All-or-nothing as far as the filesystem allows: if a move or a write
/// fails, what was already applied is undone with [`undo_applied`] and the
/// error says what failed (and, if the undo itself fell short, what is left).
pub fn apply_planned_moves(project_root: &Path, plan: &RenamePlan) -> Result<RenameApplyResult, String> {
    // Not the temp-file-and-rename helper: that replaces the file, and a page
    // in a synced folder must keep its identity, mode and extended attributes.
    // allow:raw_write the vault's own .md source, rewritten in place after a rename -- not build output
    apply_planned_moves_with(project_root, plan, &|path, text| std::fs::write(path, text))
}

/// [`apply_planned_moves`] with the page writer passed in.
fn apply_planned_moves_with(
    project_root: &Path,
    plan: &RenamePlan,
    write: &dyn Fn(&Path, &str) -> std::io::Result<()>,
) -> Result<RenameApplyResult, String> {
    let canonical_root =
        std::fs::canonicalize(project_root).map_err(|e| format!("Cannot canonicalize root: {}", e))?;
    let mut applied = RenameApplyResult { moves: Vec::new(), edits: Vec::new(), skipped: Vec::new() };
    match apply_steps(&canonical_root, plan, &mut applied, write) {
        Ok(()) => Ok(applied),
        Err(failure) => Err(roll_back(&canonical_root, &applied, &failure)),
    }
}

/// Why an apply or undo stopped: what failed, and the pages that could not be
/// put back to their earlier text afterwards.
struct WriteFailure {
    cause: String,
    damaged: Vec<String>,
}

impl WriteFailure {
    fn message(&self) -> String {
        if self.damaged.is_empty() {
            return self.cause.clone();
        }
        format!("{}; could not write the original text back to [{}], which may be damaged", self.cause, self.damaged.join(", "))
    }
}

/// Write `text` to the page `rel`. A failed write may have left the page
/// truncated, so on failure `previous`, the text read just before, is written
/// back unless the page still holds it (a write refused outright, such as to a
/// read-only file, changed nothing); the failure lists the page as damaged
/// only if it holds something else and cannot be written back.
fn write_page(
    write: &dyn Fn(&Path, &str) -> std::io::Result<()>,
    abs: &Path,
    rel: &str,
    text: &str,
    previous: &str,
) -> Result<(), WriteFailure> {
    write(abs, text).map_err(|e| {
        let intact = || std::fs::read(abs).is_ok_and(|bytes| bytes == previous.as_bytes());
        WriteFailure {
            cause: format!("Failed to write '{}': {}", abs.display(), e),
            damaged: if intact() || write(abs, previous).is_ok() { Vec::new() } else { vec![rel.to_string()] },
        }
    })
}

/// Do the moves and rewrites, recording each into `applied` only once it has
/// happened, so a failure leaves `applied` describing exactly what to undo.
/// A page whose write fails is written back from the text read just before,
/// since the failed write may have left it truncated; the error carries the
/// pages that could not be put back.
fn apply_steps(
    canonical_root: &Path,
    plan: &RenamePlan,
    applied: &mut RenameApplyResult,
    write: &dyn Fn(&Path, &str) -> std::io::Result<()>,
) -> Result<(), WriteFailure> {
    for mv in &plan.moves {
        let old_abs = canonical_root.join(&mv.old_path);
        let new_abs = canonical_root.join(&mv.new_path);
        crate::vault::fs::rename_entry_inner(
            canonical_root,
            &old_abs.to_string_lossy(),
            &new_abs.to_string_lossy(),
        )
        .map_err(|cause| WriteFailure { cause, damaged: Vec::new() })?;
        applied.moves.push(mv.clone());
    }

    let mut file_order: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for e in &plan.edits {
        if seen.insert(e.file.clone()) {
            file_order.push(e.file.clone());
        }
    }

    for file in file_order {
        let file_edits: Vec<&PlannedEdit> = plan.edits.iter().filter(|e| e.file == file).collect();
        let abs = canonical_root.join(&file);
        let source = match std::fs::read_to_string(&abs) {
            Ok(s) => s,
            Err(_) => {
                applied.skipped.push(SkippedFile { file, reason: "file not found after the move".to_string() });
                continue;
            }
        };
        let stale = file_edits.iter().any(|e| source.get(e.byte_from..e.byte_to) != Some(e.old_text.as_str()));
        if stale {
            applied.skipped.push(SkippedFile { file, reason: "reference text changed since planning".to_string() });
            continue;
        }
        let to_apply: Vec<Edit> =
            file_edits.iter().map(|e| Edit { from: e.byte_from, to: e.byte_to, text: e.new_text.clone() }).collect();
        let rewritten = apply_edits(&source, to_apply);
        write_page(write, &abs, &file, &rewritten, &source)?;
        // Each edit's position AFTER all the file's edits: every earlier edit
        // in the file shifts it by its length change.
        let mut by_position = file_edits;
        by_position.sort_by_key(|e| e.byte_from);
        let mut shift: isize = 0;
        for e in by_position {
            let from = (e.byte_from as isize + shift) as usize;
            applied.edits.push(AppliedEdit {
                file: file.clone(),
                byte_from: from,
                byte_to: from + e.new_text.len(),
                old_text: e.old_text.clone(),
                new_text: e.new_text.clone(),
            });
            shift += e.new_text.len() as isize - (e.byte_to - e.byte_from) as isize;
        }
    }

    Ok(())
}

/// Undo what a failed apply already did and build the error to return.
/// Reuses [`undo_applied`]; when that falls short too, the message lists the
/// entries still at their new path and the pages still rewritten.
fn roll_back(canonical_root: &Path, applied: &RenameApplyResult, failure: &WriteFailure) -> String {
    let undone = undo_applied(canonical_root, applied);
    let cause = failure.message();
    let shortfall = match &undone {
        Ok(u) if u.skipped.is_empty() && failure.damaged.is_empty() => {
            return format!("{cause} (rolled back: nothing was left changed)")
        }
        Ok(u) if u.skipped.is_empty() => return format!("{cause} (everything else was rolled back)"),
        Ok(u) => format!("pages skipped: {}", u.skipped.iter().map(|s| format!("{} ({})", s.file, s.reason)).collect::<Vec<_>>().join(", ")),
        Err(e) => e.clone(),
    };
    let moved: Vec<&str> = applied
        .moves
        .iter()
        .filter(|m| canonical_root.join(&m.new_path).exists())
        .map(|m| m.new_path.as_str())
        .collect();
    let mut rewritten: Vec<&str> = Vec::new();
    for e in &applied.edits {
        let still = std::fs::read_to_string(canonical_root.join(&e.file))
            .is_ok_and(|src| src.get(e.byte_from..e.byte_to) == Some(e.new_text.as_str()));
        if still && !rewritten.contains(&e.file.as_str()) {
            rewritten.push(&e.file);
        }
    }
    format!(
        "{cause}; rollback failed ({shortfall}). Still moved to the new name: [{}]. Still rewritten: [{}]",
        moved.join(", "),
        rewritten.join(", ")
    )
}

/// Reverse an [`apply_planned_moves`] result: rename every entry back (in
/// reverse order), then restore each edit's original text — with the same
/// staleness check, so a page a viewer edited after the rename is skipped
/// rather than clobbered. Restores the ORIGINAL bytes even when the forward
/// rewrite escalated a bare reference to an explicit path.
pub fn undo_applied(project_root: &Path, applied: &RenameApplyResult) -> Result<UndoResult, String> {
    // allow:raw_write undoing a rename's own reference rewrite -- not build output
    undo_applied_with(project_root, applied, &|path, text| std::fs::write(path, text))
}

/// [`undo_applied`] with the page writer passed in.
fn undo_applied_with(
    project_root: &Path,
    applied: &RenameApplyResult,
    write: &dyn Fn(&Path, &str) -> std::io::Result<()>,
) -> Result<UndoResult, String> {
    let canonical_root =
        std::fs::canonicalize(project_root).map_err(|e| format!("Cannot canonicalize root: {}", e))?;

    // The reverse move-set, built from the CURRENT (post-apply, pre-undo)
    // file list so the folder-note carry prediction sees the state the
    // forward apply actually left behind.
    let current_files = walk_all_files(&canonical_root);
    let reversed_top: Vec<ResolvedMove> = applied
        .moves
        .iter()
        .map(|m| ResolvedMove { old: m.new_path.clone(), new: m.old_path.clone(), is_dir: m.is_dir })
        .collect();
    let expanded_reverse = expand_with_home_carry(&reversed_top, &current_files);

    for mv in applied.moves.iter().rev() {
        let cur_abs = canonical_root.join(&mv.new_path);
        let restored_abs = canonical_root.join(&mv.old_path);
        crate::vault::fs::rename_entry_inner(
            &canonical_root,
            &cur_abs.to_string_lossy(),
            &restored_abs.to_string_lossy(),
        )?;
    }

    let mut file_order: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for e in &applied.edits {
        if seen.insert(e.file.clone()) {
            file_order.push(e.file.clone());
        }
    }

    let mut restored_files = Vec::new();
    let mut skipped = Vec::new();
    for post_apply_file in file_order {
        let pre_apply_file = map_path(&post_apply_file, &expanded_reverse);
        let file_edits: Vec<&AppliedEdit> = applied.edits.iter().filter(|e| e.file == post_apply_file).collect();
        let abs = canonical_root.join(&pre_apply_file);
        let source = match std::fs::read_to_string(&abs) {
            Ok(s) => s,
            Err(_) => {
                skipped.push(SkippedFile { file: pre_apply_file, reason: "file not found while undoing".to_string() });
                continue;
            }
        };
        let stale = file_edits.iter().any(|e| source.get(e.byte_from..e.byte_to) != Some(e.new_text.as_str()));
        if stale {
            skipped.push(SkippedFile { file: pre_apply_file, reason: "reference text changed since the rename".to_string() });
            continue;
        }
        let to_apply: Vec<Edit> =
            file_edits.iter().map(|e| Edit { from: e.byte_from, to: e.byte_to, text: e.old_text.clone() }).collect();
        let restored = apply_edits(&source, to_apply);
        write_page(write, &abs, &pre_apply_file, &restored, &source).map_err(|f| match restored_files.is_empty() {
            true => f.message(),
            false => format!("{}; pages already restored: [{}]", f.message(), restored_files.join(", ")),
        })?;
        restored_files.push(pre_apply_file);
    }

    Ok(UndoResult { restored_files, skipped })
}

#[cfg(test)]
#[path = "apply_tests.rs"]
mod tests;
