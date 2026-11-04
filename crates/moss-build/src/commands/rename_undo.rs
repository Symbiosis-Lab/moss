//! What a rename or move hands back, and the store that keeps its applied
//! edits for one Undo. The edits never leave the engine: a caller gets a
//! summary and an opaque `undo_id`, and `undo_rename` trades the id back.
//! A client that held the plan itself could send back an edited one.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::editor::rename_plan::{PlannedMove, RenameApplyResult};

/// What a rename or move did, project-wide.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct RenameRefsSummary {
    /// Every top-level entry the caller asked to move, at its final (possibly
    /// collision-suffixed) destination. A folder's carried home file is not a
    /// separate entry.
    pub moved: Vec<PlannedMove>,
    pub edited_link_count: u32,
    /// Distinct edited pages, project-relative, in first-touched order.
    pub edited_pages: Vec<String>,
    /// Pages skipped because they changed on disk after planning.
    pub skipped_pages: Vec<String>,
    /// Empty when there is nothing to undo.
    pub undo_id: String,
}

impl RenameRefsSummary {
    /// The answer for a move that rewrote nothing: no entry moved, or a loose
    /// document's folder, where there are no links.
    pub fn empty() -> Self {
        RenameRefsSummary {
            moved: Vec::new(),
            edited_link_count: 0,
            edited_pages: Vec::new(),
            skipped_pages: Vec::new(),
            undo_id: String::new(),
        }
    }
}

/// How many applied renames are kept for Undo; the oldest is dropped first.
pub const RENAME_UNDO_CAPACITY: usize = 20;

/// Applied renames awaiting a possible Undo, keyed by an opaque id.
#[derive(Default)]
pub struct RenameUndoStore {
    entries: Mutex<VecDeque<(String, PathBuf, RenameApplyResult)>>,
}

impl RenameUndoStore {
    /// Summarize `applied` and keep it for one Undo.
    pub fn summarize_and_store(&self, project_root: &Path, applied: RenameApplyResult) -> RenameRefsSummary {
        let mut edited_pages: Vec<String> = Vec::new();
        for e in &applied.edits {
            if !edited_pages.contains(&e.file) {
                edited_pages.push(e.file.clone());
            }
        }
        let skipped_pages = applied.skipped.iter().map(|s| s.file.clone()).collect();
        let moved = applied.moves.clone();
        let edited_link_count = applied.edits.len() as u32;
        let undo_id = self.push(project_root.to_path_buf(), applied);
        RenameRefsSummary { moved, edited_link_count, edited_pages, skipped_pages, undo_id }
    }

    fn push(&self, project_root: PathBuf, applied: RenameApplyResult) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let mut entries = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        entries.push_back((id.clone(), project_root, applied));
        while entries.len() > RENAME_UNDO_CAPACITY {
            entries.pop_front();
        }
        id
    }

    /// Take the entry for `undo_id`, once: undoing the same plan twice would
    /// replay a reversed edit. `None` for an unknown, spent or aged-out id.
    pub fn take(&self, undo_id: &str) -> Option<(PathBuf, RenameApplyResult)> {
        let mut entries = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        let idx = entries.iter().position(|(id, _, _)| id == undo_id)?;
        let (_, root, applied) = entries.remove(idx)?;
        Some((root, applied))
    }

    /// Forget every entry, for a host that switches folders.
    pub fn clear(&self) {
        self.entries.lock().unwrap_or_else(|p| p.into_inner()).clear();
    }
}
