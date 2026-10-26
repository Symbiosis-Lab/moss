//! Which pending read starts next: what the person is looking at, then what
//! just arrived, then everything else.
//!
//! The preview server notes the page it last served to the shell
//! ([`note_viewed`]); the scheduler asks again each time a thread is free, so a
//! navigation reorders whatever has not started and never touches a read that
//! has.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

/// Where a pending read stands, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// A page the preview is showing embeds the file.
    Viewed,
    /// No earlier result exists for these bytes: a file that arrived or
    /// changed, as opposed to one re-read because the plugin changed.
    Arrived,
    Rest,
}

static VIEWED: LazyLock<Mutex<HashMap<PathBuf, String>>> = LazyLock::new(Mutex::default);

fn key(folder: &Path) -> PathBuf {
    std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf())
}

/// Record the URL path of the page the preview just served for `folder`.
pub fn note_viewed(folder: &Path, url_path: &str) {
    if let Ok(mut viewed) = VIEWED.lock() {
        viewed.insert(key(folder), url_path.to_string());
    }
}

pub fn viewed(folder: &Path) -> Option<String> {
    VIEWED.lock().ok()?.get(&key(folder)).cloned()
}

/// The tiers of one batch: `embedders[i]` are the site-relative pages
/// embedding job `i`, `arrived[i]` whether it is in [`Tier::Arrived`].
pub struct Schedule {
    folder: PathBuf,
    embedders: Vec<Vec<String>>,
    arrived: Vec<bool>,
    /// The viewed URL and the page it resolved to, kept until the URL changes.
    resolved: Mutex<Option<(String, Option<String>)>>,
}

impl Schedule {
    pub fn new(folder: &Path, embedders: Vec<Vec<String>>, arrived: Vec<bool>) -> Self {
        Self { folder: folder.to_path_buf(), embedders, arrived, resolved: Mutex::new(None) }
    }

    fn viewed_page(&self) -> Option<String> {
        let url = viewed(&self.folder)?;
        let mut resolved = self.resolved.lock().ok()?;
        if resolved.as_ref().is_none_or(|(seen, _)| *seen != url) {
            let page = crate::editor::page_source::resolve_page_source(&url, &self.folder)
                .ok()
                .and_then(|p| p.source_path)
                .and_then(|abs| Path::new(&abs).strip_prefix(&self.folder).ok().map(|r| r.to_string_lossy().replace('\\', "/")));
            *resolved = Some((url, page));
        }
        resolved.as_ref().and_then(|(_, page)| page.clone())
    }

    pub fn tier(&self, job: usize) -> Tier {
        let page = self.viewed_page();
        if page.is_some_and(|p| self.embedders[job].contains(&p)) {
            Tier::Viewed
        } else if self.arrived[job] {
            Tier::Arrived
        } else {
            Tier::Rest
        }
    }
}

#[cfg(test)]
#[path = "priority_tests.rs"]
mod priority_tests;
