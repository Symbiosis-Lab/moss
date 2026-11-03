//! The publish dates a tree walk reads from each page's frontmatter, kept
//! between walks.

use super::DateSource;

/// Per-project publish-date cache passed to `list_tree_inner_cached`.
///
/// Keyed by `(file_path, mtime_secs)` so a stale entry never gets served.
/// [`PublishDateMemo`] owns the entries per folder; this is a borrowed view.
///
/// The cached tuple is `(publish_date, date_source, home_marker)` — the third
/// element records whether the file's frontmatter set `home: true`, extracted
/// from the SAME 4 KB frontmatter read that resolves the date (zero extra
/// I/O), and feeds the per-folder home election in `list_tree_inner_cached`.
pub type PublishDateCacheView<'a> = std::sync::Mutex<
    &'a mut std::collections::HashMap<
        (std::path::PathBuf, u64),
        (Option<String>, DateSource, bool),
    >,
>;

/// The publish dates the open folder's tree walks have already read, kept
/// between walks so a re-list reads only the files whose mtime moved. One
/// folder at a time: a walk of another folder starts empty and replaces it.
#[derive(Default)]
pub struct PublishDateMemo {
    slot: std::sync::Mutex<Option<(std::path::PathBuf, Entries)>>,
}

type Entries = std::collections::HashMap<(std::path::PathBuf, u64), (Option<String>, DateSource, bool)>;

impl PublishDateMemo {
    /// Run `walk` over `root`'s entries. They are taken out for the walk and put
    /// back after it, so two walks never hold the lock across the disk reads.
    pub fn with_folder<T>(&self, root: &std::path::Path, walk: impl FnOnce(&PublishDateCacheView<'_>) -> T) -> T {
        let lock = || self.slot.lock().unwrap_or_else(|p| p.into_inner());
        let mut entries = match lock().take() {
            Some((held, entries)) if held == root => entries,
            _ => Entries::default(),
        };
        let out = walk(&std::sync::Mutex::new(&mut entries));
        *lock() = Some((root.to_path_buf(), entries));
        out
    }

    /// Forget the folder, for a host that switches folders.
    pub fn clear(&self) {
        *self.slot.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }
}
