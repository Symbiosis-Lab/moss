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

/// The start order of one batch: `embedders[i]` are the site-relative pages
/// embedding job `i`, `arrived[i]` whether it is in [`Tier::Arrived`].
pub struct Schedule<'a> {
    folder: &'a Path,
    embedders: Vec<&'a [String]>,
    arrived: Vec<bool>,
    /// The viewed URL and the page it resolved to, kept until the URL changes.
    resolved: Mutex<Option<(String, Option<String>)>>,
}

impl<'a> Schedule<'a> {
    pub fn new(folder: &'a Path, embedders: Vec<&'a [String]>, arrived: Vec<bool>) -> Self {
        Self { folder, embedders, arrived, resolved: Mutex::new(None) }
    }

    fn viewed_page(&self) -> Option<String> {
        let url = viewed(self.folder)?;
        let mut resolved = self.resolved.lock().ok()?;
        if resolved.as_ref().is_none_or(|(seen, _)| *seen != url) {
            let page = crate::editor::page_source::resolve_page_source(&url, self.folder)
                .ok()
                .and_then(|p| p.source_path)
                .and_then(|abs| Path::new(&abs).strip_prefix(self.folder).ok().map(|r| r.to_string_lossy().replace('\\', "/")));
            *resolved = Some((url, page));
        }
        resolved.as_ref().and_then(|(_, page)| page.clone())
    }

    pub fn tier(&self, job: usize) -> Tier {
        self.tier_given(job, self.viewed_page().as_deref())
    }

    fn tier_given(&self, job: usize, page: Option<&str>) -> Tier {
        if page.is_some_and(|p| self.embedders[job].iter().any(|e| e == p)) {
            Tier::Viewed
        } else if self.arrived[job] {
            Tier::Arrived
        } else {
            Tier::Rest
        }
    }

    /// The position in `waiting` (job indices in job order) to start next.
    pub fn pick(&self, waiting: &[usize]) -> usize {
        let page = self.viewed_page();
        waiting
            .iter()
            .enumerate()
            .min_by_key(|(_, &job)| self.tier_given(job, page.as_deref()))
            .map_or(0, |(at, _)| at)
    }
}

#[cfg(test)]
#[path = "priority_tests.rs"]
mod priority_tests;
