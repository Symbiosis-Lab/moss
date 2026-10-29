//! Which pending read starts next: what the person is looking at, then what
//! just arrived, then everything else.
//!
//! The preview server notes the page it last served to the shell
//! ([`note_viewed`]) and the watcher the files that arrived or changed
//! ([`note_arrived`]); the scheduler asks again each time a thread is free, so a
//! navigation reorders whatever has not started and never touches a read that
//! has.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

/// Where a pending read stands, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// A page the preview is showing embeds the file, or lists a page that does.
    Viewed,
    /// A file that arrived or changed in this session, or has no earlier
    /// result for its bytes, as opposed to one re-read because the plugin
    /// changed.
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

static ARRIVED: LazyLock<Mutex<HashMap<PathBuf, HashSet<String>>>> = LazyLock::new(Mutex::default);

fn relative(folder: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(folder).or_else(|_| path.strip_prefix(key(folder))).ok()?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

/// Record files the watcher saw created, renamed in or modified under `folder`
/// (a page counts for the files it embeds) until [`settle`] says they are read.
pub fn note_arrived(folder: &Path, paths: &[PathBuf]) {
    if let Ok(mut arrived) = ARRIVED.lock() {
        arrived.entry(key(folder)).or_default().extend(paths.iter().filter_map(|p| relative(folder, p)));
    }
}

/// Whether the source or one of the pages embedding it arrived this session.
pub fn arrived_in_session(folder: &Path, source: &str, embedders: &[String]) -> bool {
    ARRIVED.lock().is_ok_and(|a| {
        a.get(&key(folder)).is_some_and(|set| set.contains(source) || embedders.iter().any(|p| set.contains(p)))
    })
}

/// Forget arrivals whose read finished.
pub fn settle(folder: &Path, sources: &[&str]) {
    if let Ok(mut arrived) = ARRIVED.lock() {
        if let Some(set) = arrived.get_mut(&key(folder)) {
            for s in sources {
                set.remove(*s);
            }
        }
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
    /// The viewed URL and what it resolved to, kept until the URL changes.
    resolved: Mutex<Option<(String, Viewed)>>,
}

/// The page on screen: its source, and the folders whose direct children it
/// may list.
#[derive(Clone, Default)]
struct Viewed {
    page: Option<String>,
    listed_dirs: Vec<String>,
}

fn parent_dir(path: &str) -> String {
    path.rsplit_once('/').map_or("", |(dir, _)| dir).to_string()
}

impl Schedule {
    pub fn new(folder: &Path, embedders: Vec<Vec<String>>, arrived: Vec<bool>) -> Self {
        Self { folder: folder.to_path_buf(), embedders, arrived, resolved: Mutex::new(None) }
    }

    fn viewed_page(&self) -> Option<Viewed> {
        let url = viewed(&self.folder)?;
        let mut resolved = self.resolved.lock().ok()?;
        if resolved.as_ref().is_none_or(|(seen, _)| *seen != url) {
            let decoded = percent_encoding::percent_decode_str(url.trim_matches('/')).decode_utf8_lossy().into_owned();
            let mut listed_dirs = vec![decoded];
            let source = crate::editor::page_source::resolve_page_source(&url, &self.folder).ok();
            let page = source.as_ref().and_then(|p| p.source_path.as_deref()).and_then(|abs| {
                Path::new(abs).strip_prefix(&self.folder).ok().map(|r| r.to_string_lossy().replace('\\', "/"))
            });
            // A folder page is `dir/index.md`, `dir/dir.md` or `dir.md`.
            if let Some(page) = page.as_ref().filter(|_| source.is_some_and(|p| p.is_dir)) {
                listed_dirs.push(parent_dir(page));
                listed_dirs.push(page.rsplit_once('.').map_or(page.as_str(), |(stem, _)| stem).to_string());
            }
            *resolved = Some((url, Viewed { page, listed_dirs }));
        }
        resolved.as_ref().map(|(_, v)| v.clone())
    }

    pub fn tier(&self, job: usize) -> Tier {
        let viewed = self.viewed_page();
        let on_screen = |page: &String| {
            viewed.as_ref().is_some_and(|v| v.page.as_ref() == Some(page) || v.listed_dirs.contains(&parent_dir(page)))
        };
        if self.embedders[job].iter().any(on_screen) {
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
