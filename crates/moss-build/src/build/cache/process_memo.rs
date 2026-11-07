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
