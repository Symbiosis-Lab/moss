//! Evidence about inputs consumed by one build attempt.
//!
//! The scan can find a cloud placeholder that no renderer reaches. A later
//! arrival does not change what that attempt rendered: only a new read can
//! replace its pending entry. The mutable collector belongs to the attempt;
//! `snapshot` freezes its answer before the manifest seals.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputState {
    Read { hash: String },
    Pending { retained: bool },
    ReadError { detail: String, retained: bool },
    ConfirmedAbsent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputRole {
    PageMetadata,
    PageContent,
    SharedConfig,
    Theme,
    Layout,
    GeneratedData,
    Media,
}

impl InputRole {
    pub fn required_for_publish(self) -> bool { !matches!(self, Self::Media) }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputEntry {
    pub role: InputRole,
    pub state: InputState,
}

#[derive(Debug, Clone, Default)]
pub struct InputEvidence {
    root: PathBuf,
    entries: Arc<Mutex<BTreeMap<String, InputEntry>>>,
}

impl InputEvidence {
    pub fn new(root: &Path) -> Self {
        Self { root: root.to_path_buf(), entries: Arc::default() }
    }

    pub fn root_path(&self) -> &Path { &self.root }

    fn key(&self, path: &Path) -> Option<String> {
        path.strip_prefix(&self.root).ok()
            .map(|rel| moss_core::slug::normalize_separators(&rel.to_string_lossy()))
    }

    fn record(&self, path: &Path, state: InputState) {
        if let Some(key) = self.key(path) {
            let mut entries = self.entries.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if matches!(entries.get(&key), Some(InputEntry { state: InputState::Pending { retained: true } | InputState::ReadError { retained: true, .. }, .. })) {
                return;
            }
            let role = entries.get(&key).map_or(InputRole::Media, |entry| entry.role);
            if matches!(entries.get(&key), Some(InputEntry { state: InputState::Read { .. }, .. }))
                && !matches!(state, InputState::Read { .. }) {
                entries.insert(key, InputEntry { role, state: InputState::ReadError {
                    detail: "input changed during build".into(), retained: false,
                } });
                return;
            }
            entries.insert(key, InputEntry { role, state });
        }
    }

    pub fn require(&self, path: &Path, role: InputRole) {
        if let Some(key) = self.key(path) {
            let mut entries = self.entries.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            entries.entry(key).and_modify(|entry| entry.role = role)
                .or_insert(InputEntry { role, state: InputState::Pending { retained: false } });
        }
    }

    pub fn pending(&self, path: &Path) {
        if let Some(key) = self.key(path) {
            let mut entries = self.entries.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if !matches!(entries.get(&key), Some(InputEntry { state: InputState::Read { .. } | InputState::Pending { retained: true } | InputState::ReadError { retained: true, .. }, .. })) {
                let role = entries.get(&key).map_or(InputRole::Media, |entry| entry.role);
                entries.insert(key, InputEntry { role, state: InputState::Pending { retained: false } });
            }
        }
    }

    pub fn read(&self, path: &Path, hash: String) {
        if let Some(key) = self.key(path) {
            let mut entries = self.entries.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            // A later successful re-read cannot make an earlier fallback
            // disappear from output that already used it.
            let role = entries.get(&key).map_or(InputRole::Media, |entry| entry.role);
            match entries.get(&key).map(|entry| &entry.state) {
                Some(InputState::ReadError { .. } | InputState::Pending { retained: true }) => return,
                Some(InputState::Read { hash: previous }) if previous != &hash => {
                    entries.insert(key, InputEntry { role, state: InputState::ReadError {
                        detail: "input changed during build".into(), retained: false,
                    } });
                }
                _ => { entries.insert(key, InputEntry { role, state: InputState::Read { hash } }); }
            }
        }
    }

    pub fn read_bytes(&self, path: &Path, bytes: &[u8]) {
        use sha2::{Digest, Sha256};
        self.read(path, format!("{:x}", Sha256::digest(bytes)));
    }

    /// A renderer-owned source normalization replaces bytes it just read.
    /// Only that exact predecessor hash may advance; an independent write or
    /// an earlier retained fallback remains unresolved for this attempt.
    pub fn owned_rewrite(&self, path: &Path, original: &[u8], final_bytes: &[u8]) {
        use sha2::{Digest, Sha256};
        let Some(key) = self.key(path) else { return; };
        let before = format!("{:x}", Sha256::digest(original));
        let after = format!("{:x}", Sha256::digest(final_bytes));
        let mut entries = self.entries.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(entry) = entries.get_mut(&key) else { return; };
        match &mut entry.state {
            InputState::Read { hash } if hash.as_str() == before => *hash = after,
            InputState::Read { .. } => entry.state = InputState::ReadError {
                detail: "input changed during build".into(), retained: false,
            },
            _ => {}
        }
    }

    pub fn known_unchanged(&self, path: &Path, hash: String) {
        if let Some(key) = self.key(path) {
            self.entries.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
                .entry(key).or_insert(InputEntry { role: InputRole::PageContent, state: InputState::Read { hash } });
        }
    }

    pub fn read_error(&self, path: &Path, detail: String) {
        self.record(path, InputState::ReadError { detail, retained: false });
    }

    pub fn absent(&self, path: &Path) {
        self.record(path, InputState::ConfirmedAbsent);
    }

    pub fn retained(&self, path: &Path) {
        if let Some(key) = self.key(path) {
            let mut entries = self.entries.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            match entries.get_mut(&key).map(|entry| &mut entry.state) {
                Some(InputState::Pending { retained }) | Some(InputState::ReadError { retained, .. }) => *retained = true,
                _ => {}
            }
        }
    }

    pub fn snapshot(&self) -> BTreeMap<String, InputEntry> {
        self.entries.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    pub fn unresolved_structural(&self) -> Vec<String> {
        self.snapshot().into_iter()
            .filter(|(_, entry)| entry.role.required_for_publish()
                && matches!(entry.state, InputState::Pending { .. } | InputState::ReadError { .. }))
            .map(|(path, _)| path)
            .collect()
    }

    pub fn unresolved_for_preview(
        &self,
        needed_pages: &std::collections::BTreeSet<String>,
        include_listing_metadata: bool,
        include_places: bool,
    ) -> Vec<String> {
        unresolved_preview_entries(&self.snapshot(), needed_pages, include_listing_metadata, include_places, None)
    }

    pub fn pending_count(&self) -> usize {
        self.snapshot().values().filter(|entry| matches!(entry.state, InputState::Pending { .. })).count()
    }

}

pub fn unresolved_preview_entries(
    entries: &BTreeMap<String, InputEntry>,
    needed_pages: &std::collections::BTreeSet<String>,
    include_listing_metadata: bool,
    include_places: bool,
    retained_source: Option<&str>,
) -> Vec<String> {
        entries.iter()
            .filter(|(path, entry)| {
                let needed = match entry.role {
                    InputRole::SharedConfig | InputRole::Theme | InputRole::Layout => true,
                    InputRole::GeneratedData => include_places,
                    InputRole::PageMetadata | InputRole::PageContent =>
                        include_listing_metadata || needed_pages.contains(*path),
                    InputRole::Media => false,
                };
                let retained_page = retained_source == Some(path.as_str())
                    && matches!(entry.role, InputRole::PageContent | InputRole::PageMetadata)
                    && matches!(entry.state, InputState::Pending { retained: true }
                        | InputState::ReadError { retained: true, .. });
                needed && !retained_page
                    && matches!(entry.state, InputState::Pending { .. } | InputState::ReadError { .. })
            })
            .map(|(path, _)| path.clone())
            .collect()
}

/// Inputs whose absence can change page identity, layout, or site structure.
pub fn is_structural_source(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else { return false; };
    let ext = ext.to_ascii_lowercase();
    crate::build::scan::classify::is_page_source(&ext)
        || matches!(ext.as_str(), "html" | "htm" | "docx" | "doc" | "pages" | "ipynb" | "toml" | "css")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn late_arrival_does_not_rewrite_an_attempt() {
        let root = Path::new("/site");
        let evidence = InputEvidence::new(root);
        evidence.require(&root.join("index.md"), InputRole::PageContent);
        evidence.pending(&root.join("index.md"));
        assert_eq!(evidence.unresolved_structural(), ["index.md"]);
        evidence.retained(&root.join("index.md"));
        assert_eq!(evidence.unresolved_structural(), ["index.md"]);
        let sealed = evidence.snapshot();
        evidence.read(&root.join("index.md"), "fresh".into());
        assert!(matches!(sealed["index.md"].state, InputState::Pending { retained: true }));
        assert_eq!(evidence.unresolved_structural(), ["index.md"]);
    }

    #[test]
    fn read_after_scan_replaces_pending() {
        let root = Path::new("/site");
        let evidence = InputEvidence::new(root);
        evidence.pending(&root.join("page.md"));
        evidence.read(&root.join("page.md"), "bytes-hash".into());
        evidence.pending(&root.join("page.md"));
        assert!(matches!(evidence.snapshot()["page.md"].state, InputState::Read { .. }));
    }

    #[test]
    fn fallback_stays_unresolved_after_another_consumer_reads_late_bytes() {
        let root = Path::new("/site");
        let evidence = InputEvidence::new(root);
        let page = root.join("page.md");
        evidence.require(&page, InputRole::PageContent);
        evidence.pending(&page);
        evidence.retained(&page);
        evidence.read(&page, "late bytes".into());
        assert_eq!(evidence.unresolved_structural(), ["page.md"]);
        assert!(matches!(evidence.snapshot()["page.md"].state, InputState::Pending { retained: true }));
    }

    #[test]
    fn skipped_notebook_cannot_be_cleared_by_a_late_reader() {
        let root = Path::new("/site");
        let notebook = root.join("analysis.ipynb");
        for reason in ["notebook processing timed out", "notebook processing failed"] {
            let attempt = InputEvidence::new(root);
            attempt.require(&notebook, InputRole::PageContent);
            let worker = attempt.clone();
            let source = notebook.clone();
            let (go, ready) = std::sync::mpsc::channel::<()>();
            let thread = std::thread::spawn(move || {
                ready.recv().unwrap();
                worker.read_bytes(&source, b"late success");
            });
            attempt.read_error(&notebook, reason.into());
            go.send(()).unwrap();
            thread.join().unwrap();
            assert_eq!(attempt.unresolved_structural(), ["analysis.ipynb"]);

            let retry = InputEvidence::new(root);
            retry.require(&notebook, InputRole::PageContent);
            retry.read_bytes(&notebook, b"late success");
            assert!(retry.unresolved_structural().is_empty());
        }
    }

    #[test]
    fn two_different_reads_in_one_attempt_cannot_certify_either_version() {
        let root = Path::new("/site");
        let evidence = InputEvidence::new(root);
        let config = root.join(".moss/config.toml");
        evidence.require(&config, InputRole::SharedConfig);
        evidence.read(&config, "first".into());
        evidence.read(&config, "second".into());
        assert_eq!(evidence.unresolved_structural(), [".moss/config.toml"]);
    }

    #[test]
    fn renderer_owned_rewrite_advances_only_the_exact_consumed_version() {
        let root = Path::new("/site");
        let page = root.join("index.md");
        let evidence = InputEvidence::new(root);
        evidence.require(&page, InputRole::PageContent);
        evidence.read_bytes(&page, b"# Home");
        evidence.owned_rewrite(&page, b"# Home", b"---\nuid: fresh\n---\n# Home");
        evidence.read_bytes(&page, b"---\nuid: fresh\n---\n# Home");
        assert!(evidence.unresolved_structural().is_empty());

        evidence.owned_rewrite(&page, b"different predecessor", b"third");
        assert_eq!(evidence.unresolved_structural(), ["index.md"]);
    }

    #[test]
    fn roles_select_generated_data_and_scripts_without_extension_guessing() {
        let root = Path::new("/site");
        let evidence = InputEvidence::new(root);
        let script = root.join(".moss/theme/script.js");
        let places = root.join(".moss/places.toml");
        let media = root.join("cover.jpg");
        evidence.require(&script, InputRole::Theme);
        evidence.pending(&script);
        evidence.require(&places, InputRole::GeneratedData);
        evidence.pending(&places);
        evidence.pending(&media);
        let needed = std::collections::BTreeSet::new();
        assert_eq!(evidence.unresolved_for_preview(&needed, false, false), [".moss/theme/script.js"]);
        assert_eq!(evidence.unresolved_for_preview(&needed, false, true), [".moss/places.toml", ".moss/theme/script.js"]);
        assert_eq!(evidence.unresolved_structural(), [".moss/places.toml", ".moss/theme/script.js"]);
    }
}
