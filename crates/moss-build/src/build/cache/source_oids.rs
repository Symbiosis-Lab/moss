//! One content hash per source file per build, shared by the asset walk and the
//! media lanes. They run at the same time and each keeps its own hash index, so
//! without this a new video or source image is read in full by each of them.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use super::HashIndex;
use crate::build::stat::FileStat;

/// A finished hash and the stat taken before its read; `None` when it failed.
type Slot = OnceLock<Option<(FileStat, String)>>;

#[derive(Debug, Default)]
pub(crate) struct SourceOids {
    slots: Mutex<HashMap<String, Arc<Slot>>>,
}

/// An oid this caller read the file for, or one it took from another caller,
/// which its own index has not recorded.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Oid {
    Hashed(String),
    Taken(String),
}

impl SourceOids {
    /// [`resolve_at`](Self::resolve_at) at the stat `file` shows now, returned
    /// with the oid so the caller records the two together.
    pub(crate) fn resolve(
        &self,
        file: &std::path::Path,
        rel: &str,
        hash: impl FnOnce() -> Result<String, String>,
    ) -> Result<(FileStat, Oid), String> {
        let meta = std::fs::metadata(file).map_err(|e| format!("Failed to stat {}: {}", file.display(), e))?;
        let stat = FileStat::of(&meta);
        Ok((stat, self.resolve_at(rel, &stat, hash)?))
    }

    /// `rel`'s content hash, given the stat the caller took before calling. `hash`
    /// runs unless another caller has hashed the file at this same stat, or is
    /// hashing it now, in which case this waits and takes its oid. A failure is
    /// never handed on: a waiter that finds one, or a hash taken at another stat,
    /// runs its own `hash`. A first hasher that panics leaves the slot empty, and
    /// the next caller hashes.
    pub(crate) fn resolve_at(
        &self,
        rel: &str,
        stat: &FileStat,
        hash: impl FnOnce() -> Result<String, String>,
    ) -> Result<Oid, String> {
        let slot = Arc::clone(self.slots.lock().unwrap_or_else(|e| e.into_inner()).entry(rel.to_string()).or_default());
        let mut hash = Some(hash);
        let mut own = None;
        let first = slot.get_or_init(|| {
            let result = hash.take().expect("the slot runs one initializer")();
            let shared = result.as_ref().ok().map(|oid| (*stat, oid.clone()));
            own = Some(result);
            shared
        });
        if let Some(result) = own {
            return result.map(Oid::Hashed);
        }
        match first {
            Some((at, oid)) if at == stat => Ok(Oid::Taken(oid.clone())),
            _ => hash.take().expect("not run by the slot")().map(Oid::Hashed),
        }
    }
}

impl Oid {
    /// The oid, recorded in `index` if it was taken, in the shape that index's own
    /// lookup rule reads: whole seconds for a video. Recording a taken oid keeps
    /// the index's next save from writing back an older entry for the file.
    pub(crate) fn recorded_in(self, index: &mut HashIndex, rel: &str, stat: &FileStat, video: bool) -> String {
        match self {
            Oid::Hashed(oid) => oid,
            Oid::Taken(oid) => {
                let stat = if video { FileStat::whole_second(stat.size, stat.mtime) } else { *stat };
                index.adopt(rel.to_string(), stat, oid.clone());
                oid
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn stat(size: u64) -> FileStat {
        FileStat { size, mtime: 7, ..FileStat::default() }
    }

    /// Runs `first` on a thread and returns once it is inside its hash, holding
    /// the slot, so the caller's own `resolve` has to wait on it.
    fn first_hasher(
        oids: &Arc<SourceOids>,
        first: impl FnOnce() -> Result<String, String> + Send + 'static,
    ) -> std::thread::JoinHandle<()> {
        let (inside, entered) = mpsc::channel();
        let oids = Arc::clone(oids);
        let handle = std::thread::spawn(move || {
            let _ = oids.resolve_at("clip.mp4", &stat(1), move || {
                inside.send(()).unwrap();
                std::thread::sleep(Duration::from_millis(100));
                first()
            });
        });
        entered.recv().unwrap();
        handle
    }

    #[test]
    fn a_second_caller_waits_for_the_first_hash_and_takes_it() {
        let oids = Arc::new(SourceOids::default());
        let first = first_hasher(&oids, || Ok("abc".into()));
        let got = oids.resolve_at("clip.mp4", &stat(1), || panic!("read a second time"));
        first.join().unwrap();
        assert_eq!(got, Ok(Oid::Taken("abc".into())));
    }

    #[test]
    fn a_hash_taken_at_another_stat_is_not_taken() {
        let oids = SourceOids::default();
        oids.resolve_at("clip.mp4", &stat(1), || Ok("old".into())).unwrap();
        assert_eq!(oids.resolve_at("clip.mp4", &stat(2), || Ok("new".into())), Ok(Oid::Hashed("new".into())));
    }

    #[test]
    fn a_first_hasher_that_fails_or_is_cancelled_leaves_the_waiter_to_hash() {
        let oids = Arc::new(SourceOids::default());
        let first = first_hasher(&oids, || Err("cancelled".into()));
        let got = oids.resolve_at("clip.mp4", &stat(1), || Ok("abc".into()));
        first.join().unwrap();
        assert_eq!(got, Ok(Oid::Hashed("abc".into())));
    }

    #[test]
    fn a_first_hasher_that_panics_leaves_the_waiter_to_hash() {
        let oids = Arc::new(SourceOids::default());
        let first = first_hasher(&oids, || panic!("the hasher died"));
        let got = oids.resolve_at("clip.mp4", &stat(1), || Ok("abc".into()));
        assert!(first.join().is_err());
        assert_eq!(got, Ok(Oid::Hashed("abc".into())));
    }

    #[test]
    fn a_taken_oid_is_recorded_in_the_shape_its_index_reads() {
        let mut index = HashIndex::new();
        let at = FileStat { mtime_nanos: Some(5), ..stat(3) };
        assert_eq!(Oid::Taken("abc".into()).recorded_in(&mut index, "clip.mp4", &at, true), "abc");
        assert_eq!(index.lookup_whole_second("clip.mp4", 3, 7), Some("abc"));
        assert_eq!(index.lookup("clip.mp4", &at), None, "a video entry is never a full-stat record");
        Oid::Taken("def".into()).recorded_in(&mut index, "cover.webp", &at, false);
        assert_eq!(index.lookup("cover.webp", &at), Some("def"));
    }
}
