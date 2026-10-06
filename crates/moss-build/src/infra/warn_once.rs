//! Generic "warn at most once per key, per process" gate.
//!
//! Three call sites grew the identical three-line shape independently: a
//! lazily-initialized `Mutex<HashSet<K>>` plus a `seen.insert(key)` gate — a
//! directory path (`moss_paths`'s cloud-exclude warning), an
//! `(source_path, what)` pair (`advisory::Advisory::for_source`), and an
//! image path (`build/media/decode.rs`'s extension/content-mismatch
//! warning). Each site still owns its own process-wide `HashSet` (the key
//! types differ, and so does what should share one), but the pure gate
//! itself is one function now.

use std::collections::HashSet;
use std::hash::Hash;
use std::sync::Mutex;

/// `true` the first time `key` is seen in `seen`, `false` on every later
/// call with an equal key. Takes `seen` explicitly rather than reading a
/// process-wide static directly, so a test exercises a local set —
/// parallel-safe, independent of every other test in the binary.
pub(crate) fn should_warn_once<K: Eq + Hash>(key: K, seen: &Mutex<HashSet<K>>) -> bool {
    seen.lock().unwrap_or_else(|e| e.into_inner()).insert(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fires_only_on_first_sighting_of_a_key() {
        let seen: Mutex<HashSet<&str>> = Mutex::new(HashSet::new());
        assert!(should_warn_once("a", &seen), "first sighting of a must warn");
        assert!(!should_warn_once("a", &seen), "second sighting of a must stay quiet");
        assert!(should_warn_once("b", &seen), "a different key must still warn");
        assert!(!should_warn_once("b", &seen), "and then go quiet in turn");
    }
}
