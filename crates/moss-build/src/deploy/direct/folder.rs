//! A folder on this computer as a [`Destination`]: the way the publish core
//! is exercised without a server.

use std::path::{Path, PathBuf};

use super::destination::{Body, Destination};

/// A folder on this computer.
#[derive(Clone)]
pub struct FolderDestination {
    root: PathBuf,
}

impl FolderDestination {
    /// The folder must exist: publishing into a path that is not there is far
    /// more likely a missing mount than a request to create one.
    pub fn new(root: impl Into<PathBuf>) -> Result<FolderDestination, String> {
        let root = root.into();
        let root = std::fs::canonicalize(&root)
            .ok()
            .filter(|r| r.is_dir())
            .ok_or_else(|| format!("No folder at {}", root.display()))?;
        Ok(FolderDestination { root })
    }

    /// `path` joined to the root, refused if a link inside the destination
    /// would carry it outside: the record naming it may not have been
    /// written by moss.
    fn resolve(&self, path: &str) -> Result<PathBuf, String> {
        let full = self.root.join(path);
        let mut parent = full.parent().unwrap_or(&self.root).to_path_buf();
        // The nearest ancestor that exists decides where the path lands.
        while !parent.exists() {
            parent = match parent.parent() {
                Some(p) => p.to_path_buf(),
                None => break,
            };
        }
        let real = std::fs::canonicalize(&parent).map_err(|e| format!("{path}: {e}"))?;
        if !real.starts_with(&self.root) {
            return Err(format!("{path} leads outside {}", self.root.display()));
        }
        Ok(full)
    }

    fn put_blocking(&self, path: &str, body: Body) -> Result<(), String> {
        let full = self.resolve(path)?;
        let dir = full.parent().unwrap_or(&self.root);
        std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        let name = full.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
        let temp = dir.join(format!(".{name}.moss-tmp-{}", std::process::id()));
        // allow:raw_write a temporary file just minted in the destination, never moss's build output
        let written = match body {
            Body::File(source) => std::fs::copy(source, &temp).map(|_| ()).and_then(|()| readable(&temp)),
            Body::Bytes(bytes) => std::fs::write(&temp, bytes), // allow:raw_write the same temporary file
            Body::Link(target) => link(&target, &temp),
        };
        let renamed = written.and_then(|()| std::fs::rename(&temp, &full));
        renamed.map_err(|e| {
            let _ = std::fs::remove_file(&temp);
            format!("write {path}: {e}")
        })
    }

    fn delete_blocking(&self, path: &str) -> Result<(), String> {
        let full = self.resolve(path)?;
        match std::fs::remove_file(&full) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(format!("delete {path}: {e}")),
        }
        // Folders the deletion emptied go too, up to the root. `remove_dir`
        // refuses a folder with anything left in it.
        let mut dir = full.parent();
        while let Some(d) = dir.filter(|d| *d != self.root && d.starts_with(&self.root)) {
            if std::fs::remove_dir(d).is_err() {
                break;
            }
            dir = d.parent();
        }
        Ok(())
    }
}

impl FolderDestination {
    /// Run one file operation off the async runtime: a copy can be a large
    /// video.
    async fn blocking<T: Send + 'static>(
        &self,
        op: impl FnOnce(&FolderDestination) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || op(&this))
            .await
            .map_err(|e| format!("file operation stopped: {e}"))?
    }
}

#[async_trait::async_trait]
impl Destination for FolderDestination {
    async fn get(&self, path: &str) -> Result<Option<Vec<u8>>, String> {
        let path = path.to_string();
        self.blocking(move |d| {
            let full = d.resolve(&path)?;
            // allow:raw_read the destination's own file, not a vault input
            match std::fs::read(&full) {
                Ok(bytes) => Ok(Some(bytes)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(format!("read {path}: {e}")),
            }
        })
        .await
    }

    async fn put_atomic(&self, path: &str, body: Body) -> Result<(), String> {
        let path = path.to_string();
        self.blocking(move |d| d.put_blocking(&path, body)).await
    }

    async fn delete(&self, path: &str) -> Result<(), String> {
        let path = path.to_string();
        self.blocking(move |d| d.delete_blocking(&path)).await
    }
}

/// A copy keeps its source's mode, and a build's file may be read-only;
/// a web server reading the copy needs it readable by others.
#[cfg(unix)]
fn readable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644))
}

#[cfg(not(unix))]
fn readable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn link(target: &str, at: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, at)
}

#[cfg(not(unix))]
fn link(_target: &str, _at: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "symbolic links are not published from this system"))
}
