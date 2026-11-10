//! Where a direct publish writes: three operations, each on one path relative
//! to the destination's root.

use std::path::PathBuf;

/// What a put writes at a path. Owned, so an implementation can hand it to a
/// blocking task.
#[derive(Debug, Clone)]
pub enum Body {
    /// The bytes of a local file, read as the put streams it.
    File(PathBuf),
    /// Bytes already in memory (the published-files record).
    Bytes(Vec<u8>),
    /// A symbolic link with this target, as the build wrote it.
    Link(String),
}

/// A place moss publishes into by writing files itself.
///
/// Every path is relative and `/`-separated, and `plan::is_safe_path` has
/// refused any with a root or a `..`. That is not containment: a link inside
/// the destination can still lead out of it, and the record naming a path may
/// not have been written by moss. So each implementation refuses a path whose
/// real parent folder is outside its root, before it reads, writes or deletes.
///
/// A failed operation is returned, not retried: only the implementation knows
/// whether its error heals (a dropped connection it can reconnect), and a
/// publish that stops at any point is finished by the next one.
#[async_trait::async_trait]
pub trait Destination: Send + Sync {
    /// The bytes at `path`, or `None` when nothing is there.
    async fn get(&self, path: &str) -> Result<Option<Vec<u8>>, String>;
    /// Replace whatever is at `path` in one step, creating its folders: a
    /// reader sees the old bytes or the new ones, never a partial file and
    /// never a missing one. Written as a temporary name renamed over the
    /// target; an implementation that cannot rename over an existing file
    /// (plain SFTP without the POSIX rename extension) refuses the
    /// destination rather than deleting first.
    async fn put_atomic(&self, path: &str, body: Body) -> Result<(), String>;
    /// Remove `path`, and any folder that removal leaves empty, up to the
    /// root. Already gone is not an error.
    async fn delete(&self, path: &str) -> Result<(), String>;
}
