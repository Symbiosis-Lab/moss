//! A value a long-lived process keeps per site root from one build to the
//! next, for work a watch or app session would otherwise redo on every
//! rebuild. The build that uses it takes it out and puts its own back, so two
//! builds of one root never share one; a few roots are kept, the least
//! recently built dropped first. What a value may hold, and when it is still
//! valid, is its owner's to say.

use std::collections::HashMap;
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Roots a process memo keeps. A test process builds hundreds of temporary
/// roots at once, and evicting one between two of its test's builds would turn
/// the hit under test into a miss.
pub(crate) const ROOTS: usize = if cfg!(test) { usize::MAX } else { 8 };

pub(crate) struct ProcessMemo<V> {
    roots: Mutex<Vec<(PathBuf, V)>>,
    cap: usize,
}

impl<V> ProcessMemo<V> {
    pub(crate) const fn new(cap: usize) -> Self {
        Self { roots: Mutex::new(Vec::new()), cap }
    }

    pub(crate) fn take(&self, root: &Path) -> Option<V> {
        let mut roots = self.roots.lock().unwrap_or_else(|e| e.into_inner());
        roots.iter().position(|(r, _)| r == root).map(|i| roots.remove(i).1)
    }

    pub(crate) fn keep(&self, root: &Path, value: V) {
        let mut roots = self.roots.lock().unwrap_or_else(|e| e.into_inner());
        roots.retain(|(r, _)| r != root);
        roots.insert(0, (root.to_path_buf(), value));
        roots.truncate(self.cap);
    }
}

/// A process memo's value kept in a file, so the next process (a one-shot
/// `moss build`, or the app after a restart) starts from it rather than from
/// nothing. Only for a value that proves itself on every read, since the file
/// outlives everything the process knew; the owner says when to write it.
///
/// Headed by the memo's name, its version and the binary that wrote it, since
/// another moss may read the same files differently. The app and the command
/// line are different binaries sharing one file, so each discards what the
/// other wrote: switching between them costs one full pass. A missing, foreign
/// or cut-short file reads as none, which is what a new process had before.
pub(crate) struct MemoFile {
    name: &'static str,
    version: u32,
}

impl MemoFile {
    pub(crate) const fn new(name: &'static str, version: u32) -> Self {
        Self { name, version }
    }

    fn path(&self, dir: &Path) -> PathBuf {
        dir.join(format!("{}.json", self.name))
    }

    fn header(&self) -> String {
        format!("moss-memo {} {} {}\n", self.name, self.version, crate::plugins::fingerprint::builder_fingerprint())
    }

    pub(crate) fn read<T: serde::de::DeserializeOwned>(&self, dir: &Path) -> Option<T> {
        let bytes = std::fs::read(self.path(dir)).ok()?;
        serde_json::from_slice(bytes.strip_prefix(self.header().as_bytes())?).ok()
    }

    /// Not flushed: a torn or lost file costs the next process one full pass.
    pub(crate) fn write<T: serde::Serialize>(&self, dir: &Path, value: &T) {
        let mut out = self.header().into_bytes();
        let written = serde_json::to_writer(&mut out, value)
            .map_err(std::io::Error::other)
            .and_then(|()| crate::build::io_utils::write_output(&self.path(dir), &out));
        if let Err(e) = written {
            log::debug!("memo {} not kept in {}: {e}", self.name, dir.display());
        }
    }
}

/// Answers a process keeps by a key that names their inputs whole: the cache
/// that holds the answer, a content hash or stat key, and parameters. Cleared
/// when it reaches its bound.
pub(crate) struct KeyedMemo<K, V> {
    answers: OnceLock<Mutex<HashMap<K, V>>>,
}

impl<K: Eq + Hash, V: Clone> KeyedMemo<K, V> {
    const BOUND: usize = 1 << 16;

    pub(crate) const fn new() -> Self {
        Self { answers: OnceLock::new() }
    }

    fn answers(&self) -> std::sync::MutexGuard<'_, HashMap<K, V>> {
        self.answers.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn get(&self, key: &K) -> Option<V> {
        self.answers().get(key).cloned()
    }

    pub(crate) fn put(&self, key: K, value: V) {
        let mut answers = self.answers();
        if answers.len() >= Self::BOUND {
            answers.clear();
        }
        answers.insert(key, value);
    }

    #[cfg(test)]
    pub(crate) fn retain(&self, mut keep: impl FnMut(&K) -> bool) {
        self.answers().retain(|key, _| keep(key));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_least_recently_built_root_is_dropped_first() {
        let memo = ProcessMemo::new(2);
        memo.keep(Path::new("/a"), 1);
        memo.keep(Path::new("/b"), 2);
        let a = memo.take(Path::new("/a")).unwrap();
        memo.keep(Path::new("/a"), a);
        memo.keep(Path::new("/c"), 3);
        assert_eq!(memo.take(Path::new("/b")), None, "b was built least recently");
        assert_eq!(memo.take(Path::new("/a")), Some(1));
        assert_eq!(memo.take(Path::new("/c")), Some(3));
    }
}
