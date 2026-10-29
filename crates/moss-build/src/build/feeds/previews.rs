//! Per-page hover-preview excerpts, kept between builds of one dev-server process.
//!
//! `extract_preview` parses a page's whole text to keep one paragraph, so on a
//! site of long pages it dominated every rebuild. The excerpt is a pure function
//! of the page's `content` and the `math` flag, so it is keyed on a hash of
//! exactly those: unlike the source-file hash the parse cache keys on, that
//! hash cannot miss an input the final `content` was assembled from.
//!
//! Like the parse cache this lives for the process and holds one root; a folder
//! switch replaces it, and a new moss version starts with it empty.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

struct Entry {
    hash: u64,
    math: bool,
    excerpt: String,
}

static STORE: Mutex<Option<(PathBuf, HashMap<String, Entry>)>> = Mutex::new(None);

static LAST_COMPUTED: AtomicUsize = AtomicUsize::new(0);

/// How many excerpts the last finished build computed rather than replayed.
pub fn last_computed() -> usize {
    LAST_COMPUTED.load(Ordering::Relaxed)
}

/// One build's pass over the cache: created before the page loop, consumed after.
pub struct PreviewCache {
    root: PathBuf,
    previous: HashMap<String, Entry>,
    next: HashMap<String, Entry>,
    computed: usize,
}

/// Tests share one process; only a test holding the parse-cache guard may read
/// or publish the store, as for the parse cache itself.
fn shared() -> bool {
    #[cfg(test)]
    return crate::build::parse_cache::cache_test_is_active();
    #[cfg(not(test))]
    true
}

impl PreviewCache {
    pub fn begin(root: &Path) -> Self {
        let previous = if shared() {
            STORE
                .lock()
                .ok()
                .and_then(|mut s| s.take().filter(|(stored, _)| stored == root))
                .map(|(_, entries)| entries)
                .unwrap_or_default()
        } else {
            HashMap::new()
        };
        Self { root: root.to_path_buf(), previous, next: HashMap::new(), computed: 0 }
    }

    /// The excerpt for the page at `key`, replayed when `content` is unchanged.
    pub fn excerpt(&mut self, key: &str, content: &str, math: bool, compute: impl FnOnce(&str) -> String) -> String {
        let hash = xxhash_rust::xxh3::xxh3_64(content.as_bytes());
        let entry = match self.previous.remove(key) {
            Some(entry) if entry.hash == hash && entry.math == math => entry,
            _ => {
                self.computed += 1;
                Entry { hash, math, excerpt: compute(content) }
            }
        };
        let excerpt = entry.excerpt.clone();
        self.next.insert(key.to_string(), entry);
        excerpt
    }

    /// Keep this build's entries for the next one. Pages not asked for this
    /// build (removed, or no longer public) are dropped.
    pub fn finish(self) {
        if !shared() {
            return;
        }
        LAST_COMPUTED.store(self.computed, Ordering::Relaxed);
        if let Ok(mut store) = STORE.lock() {
            *store = Some((self.root, self.next));
        }
    }
}
