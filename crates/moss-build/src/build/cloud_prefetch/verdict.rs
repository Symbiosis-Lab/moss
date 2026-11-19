//! Whether the provider is sending a folder's files, judged per folder.
//!
//! The pool serves every open folder, but the verdict is about one: a read held
//! for folder A says nothing about folder B, which may sit in another provider,
//! and B's deliveries say nothing about A. So the evidence is recorded against
//! the folder a job belongs to when the job is given up or returns.
//!
//! A job belongs to the longest watched folder that is a prefix of its path
//! (`Path::starts_with`). Byte reads and folder listings count; a publication is moss writing into the
//! folder, so a held write is not the provider failing to send.
use super::*;

/// The evidence for one watched folder.
#[derive(Debug, Default)]
pub(super) struct Verdict {
    /// Watchers holding this folder.
    holders: usize,
    /// A job was given up and none of this folder's jobs has returned since.
    gave_up: bool,
    /// Byte reads returned since the folder's last delivered one, each with
    /// its file still in the cloud.
    undelivered: usize,
}

impl Verdict {
    fn stuck(&self) -> bool { self.gave_up || self.undelivered > 0 }
}

/// The folder `path` belongs to, if it is watched.
pub(super) fn of<'a>(q: &'a mut Queue, path: &Path) -> Option<&'a mut Verdict> {
    q.verdicts
        .iter_mut()
        .filter(|(folder, _)| path.starts_with(folder))
        .max_by_key(|(folder, _)| folder.as_os_str().len())
        .map(|(_, verdict)| verdict)
}

/// A byte read or folder listing of `path` was given up.
pub(super) fn gave_up(q: &mut Queue, path: &Path) {
    if let Some(v) = of(q, path) { v.gave_up = true; }
}

/// A job of `path` returned; `delivered` is a byte read's outcome, `None` for
/// a listing or a failed read.
pub(super) fn returned(q: &mut Queue, path: &Path, delivered: Option<bool>) {
    let Some(v) = of(q, path) else { return };
    v.gave_up = false;
    match delivered {
        Some(true) => v.undelivered = 0,
        Some(false) => v.undelivered += 1,
        None => {}
    }
}

/// Holds a folder watched while it lives. Dropping the last one forgets it.
#[must_use = "the folder is watched only while the guard lives"]
pub struct Watch {
    inner: Arc<Inner>,
    folder: PathBuf,
}

impl Drop for Watch {
    fn drop(&mut self) {
        let Ok(mut q) = self.inner.queue.lock() else { return };
        if let Some(at) = q.verdicts.iter().position(|(folder, _)| *folder == self.folder) {
            q.verdicts[at].1.holders -= 1;
            if q.verdicts[at].1.holders == 0 { q.verdicts.swap_remove(at); }
        }
    }
}

impl Prefetcher {
    /// Record the verdict for `folder` while the returned guard lives.
    pub fn watch(&self, folder: &Path) -> Watch {
        if let Ok(mut q) = self.inner.queue.lock() {
            match q.verdicts.iter_mut().find(|(watched, _)| watched == folder) {
                Some((_, v)) => v.holders += 1,
                None => q.verdicts.push((folder.to_path_buf(), Verdict { holders: 1, ..Verdict::default() })),
            }
        }
        Watch { inner: Arc::clone(&self.inner), folder: folder.to_path_buf() }
    }

    /// The provider is not sending `folder`'s files: one of its jobs was given
    /// up and none has returned since, or every byte read returned since its
    /// last delivered one left the file in the cloud. False for a folder not
    /// watched.
    pub fn stuck_for(&self, folder: &Path) -> bool {
        let Ok(mut q) = self.inner.queue.lock() else { return false };
        abandon::reap(&mut q, &self.inner, Instant::now());
        q.verdicts.iter().find(|(watched, _)| watched == folder).is_some_and(|(_, v)| v.stuck())
    }

    #[cfg(test)]
    pub(crate) fn watched(&self, folder: &Path) -> bool {
        self.inner.queue.lock().unwrap().verdicts.iter().any(|(watched, _)| watched == folder)
    }
}
