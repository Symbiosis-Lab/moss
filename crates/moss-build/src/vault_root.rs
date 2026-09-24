//! WHICH folder is the vault root, and what is it CALLED.
//!
//! One owner for project-root identity. Before this module the answer was derived at 69
//! sites by 8 independent resolvers and 6 disagreeing basename algorithms; `moss build .`
//! and `moss build /abs/path/site` built *different sites* from the same folder because
//! `Path::new(".").file_name()` is `None` and the empty root name demoted the self-named
//! home (`site/site.md`) off `/`.
//!
//! Three jobs, three entry points — deliberately NOT one function:
//!   * [`resolve_input`] — "the user typed a string; make it a real absolute path". The only
//!     place in the tree allowed to read `std::env::current_dir()` for this purpose.
//!   * [`VaultRoot::resolve`] — "this path IS the root; what is its identity?"
//!   * [`VaultRoot::containing`] — "which ancestor OWNS this path?" (the `.moss/` walk)
//!
//! Not this module's job: "is this path INSIDE the vault?" (`vault::fs::validate_entry_path`),
//! canonical registry KEYS (plugin manager / preview server / folder session), and OS-input
//! decoding (`system::path_extractor`, which CALLS this module and rides M5 to `platform/`).
//!
//! ## The absolute-path contract
//!
//! A plain absolute path passes through byte-identical. Symlinks are NOT resolved for such
//! paths: `/var/folders/...` must keep naming itself, or every recents entry, window title
//! and folder-session key silently changes for symlinked vaults. `.` and trailing separators
//! are handled lexically (`Path::components()` already drops them). Only a `..` component
//! forces a filesystem consult, because `..` cannot be collapsed lexically without lying
//! about symlinks.

use std::path::{Component, Path, PathBuf};

/// The one user-facing message for "there is no vault root to act on".
///
/// The GUI's `AppState` (app-side) and the HTTP carrier's `InvokeCtx`
/// (crate-side after the server relocation) answer the same absence with the
/// same words; before this const the parity was comment-only across five
/// hardcoded copies.
pub const NO_PROJECT_OPEN: &str = "No project open. Please open a project first.";

/// Errors from turning a user- or OS-supplied path into a vault target.
#[derive(Debug, PartialEq)]
pub enum VaultPathError {
    /// No file paths provided in the list
    EmptyPathList,
    /// Invalid file:// URL format
    InvalidUrl(String),
    /// Path does not exist on filesystem
    PathDoesNotExist(String),
    /// File type is not supported (only .md and .markdown allowed)
    UnsupportedFileType(String),
}

impl std::fmt::Display for VaultPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VaultPathError::EmptyPathList => write!(f, "No item selected"),
            VaultPathError::InvalidUrl(url) => write!(f, "Invalid file URL: {}", url),
            VaultPathError::PathDoesNotExist(path) => write!(f, "Path does not exist: {}", path),
            VaultPathError::UnsupportedFileType(ext) => write!(f, "Unsupported file type: {}", ext),
        }
    }
}

impl std::error::Error for VaultPathError {}

/// Absolutize `raw` against the process CWD and resolve `.`/`..`/trailing separators.
///
/// THE path-normalization primitive. [`VaultRoot::resolve`], [`VaultTarget::resolve`] and
/// the CLI's file-argument sites (`moss rename <old> <new>`, `moss doctor <folder>`) all
/// route through it, so a dot-path means the same thing to every subcommand.
pub fn resolve_input(raw: impl AsRef<Path>) -> PathBuf {
    resolve_input_in(raw.as_ref(), &std::env::current_dir().unwrap_or_default())
}

/// Testable core of [`resolve_input`] with the working directory passed in.
pub fn resolve_input_in(raw: &Path, cwd: &Path) -> PathBuf {
    // `""` is the ABSENCE of a path, not a relative path to the CWD. `cwd.join("")` is the
    // CWD, so resolving it would turn "caller passed no folder" into "build whatever
    // directory moss is running in" — silently, with no error. The emptiness guards that
    // already exist downstream (`run_pipeline`, the Tauri commands) stay load-bearing.
    if raw.as_os_str().is_empty() {
        return PathBuf::new();
    }
    let absolute = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        cwd.join(raw)
    };
    // `Path::components()` already drops trailing separators and interior `.`
    // (`/a/b/.` → `/`,`a`,`b`), so `moss build .` is answered lexically — no filesystem, no
    // symlink rewrite. Only `..` survives `components()`, and only `..` is genuinely
    // unanswerable lexically: `/a/symlink/..` is the parent of the symlink's TARGET, not of
    // `/a/symlink`. So the filesystem is consulted for exactly that one case.
    let has_parent_dir = absolute
        .components()
        .any(|c| matches!(c, Component::ParentDir));
    if has_parent_dir {
        // Lexical collapse is the fallback for a folder that does not exist yet
        // (`moss build ./new-site`), where canonicalize cannot answer at all.
        std::fs::canonicalize(&absolute).unwrap_or_else(|_| lexical_collapse(&absolute))
    } else {
        // The byte-identity guarantee for absolute paths.
        absolute.components().collect()
    }
}

fn lexical_collapse(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The identity of a vault root: the resolved folder path, plus THE folder name.
///
/// `name` is derived once, at construction, and stored. It is the load-bearing token that
/// decides whether `site/site.md` is the home at `/` (`moss_core::home::is_home_file`),
/// seeds `moss_core::home::site_name` (`<title>`, `og:title`, llms.txt), drives
/// `detect_home_file_in_folder`, and tells plugins what to name a folder home. Callers read
/// [`VaultRoot::name`]; nobody re-derives it from [`VaultRoot::path`].
///
/// `name` is empty only for the filesystem root `/` — today's value, deliberately preserved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultRoot {
    /// Resolved absolute path. Stored as `String` because every consumer in the tree types
    /// a folder path as `String`; the lossy conversion happens here, once, instead of at
    /// ~40 `.to_string_lossy().to_string()` call sites.
    path: String,
    name: String,
}

impl VaultRoot {
    /// Adopt a path that IS the root: CLI/deep-link argument, folder-picker result, stored
    /// session path, pipeline `folder_path`. Idempotent, and free of filesystem I/O for a
    /// plain absolute path — safe to call anywhere a `&str` folder path is handed over.
    pub fn resolve(raw: impl AsRef<Path>) -> Self {
        Self::from_resolved(resolve_input(raw))
    }

    /// Testable core of [`VaultRoot::resolve`].
    pub fn resolve_in(raw: &Path, cwd: &Path) -> Self {
        Self::from_resolved(resolve_input_in(raw, cwd))
    }

    fn from_resolved(path: PathBuf) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self {
            path: path.to_string_lossy().into_owned(),
            name,
        }
    }

    /// Find the root that OWNS `path` by walking up to the nearest ancestor with a `.moss/`
    /// directory. Falls back to the starting directory (new-project behavior).
    ///
    /// The input is resolved FIRST. Without that, `ancestors()` over `/x/blog/..` walks
    /// `/x/blog/..` → `/x/blog` → … and silently re-enters the folder the user asked to
    /// leave, because `Path::new("/a/b/..").parent()` is `/a/b`.
    pub fn containing(path: &Path) -> Self {
        Self::containing_in(path, &std::env::current_dir().unwrap_or_default())
    }

    /// Testable core of [`VaultRoot::containing`].
    pub fn containing_in(path: &Path, cwd: &Path) -> Self {
        let start_dir = Self::start_dir_in(path, cwd);
        Self::find_containing_in(path, cwd).unwrap_or_else(|| Self::from_resolved(start_dir))
    }

    /// The directory the `.moss/` walk starts from: `path` itself for a directory,
    /// its parent for a file.
    fn start_dir_in(path: &Path, cwd: &Path) -> PathBuf {
        let resolved = resolve_input_in(path, cwd);
        if resolved.is_file() {
            resolved.parent().unwrap_or(&resolved).to_path_buf()
        } else {
            resolved
        }
    }

    /// Like [`VaultRoot::containing`], but returns `None` when NO ancestor owns a
    /// `.moss/` — i.e. it distinguishes "found a vault" from "fell back to the
    /// starting directory".
    ///
    /// That distinction is the whole point. `containing` is total: it answers
    /// with the start directory when the walk finds nothing, which is right for
    /// its original caller ("the user pointed at a folder they want to make into
    /// a site") and silently wrong for a file. Because the fallback was
    /// indistinguishable from a hit, opening a loose `~/Downloads/notes.md` built
    /// and served all of `~/Downloads`. A caller that must decline needs to SEE
    /// that there was nothing to find (document mode).
    ///
    /// `containing` is implemented in terms of this, so the two can never drift.
    pub fn find_containing(path: &Path) -> Option<Self> {
        Self::find_containing_in(path, &std::env::current_dir().unwrap_or_default())
    }

    /// Testable core of [`VaultRoot::find_containing`].
    pub fn find_containing_in(path: &Path, cwd: &Path) -> Option<Self> {
        let start_dir = Self::start_dir_in(path, cwd);

        // Never walk above the user's home directory: `~/.moss/` is moss's app-level cache
        // (bin/, cache/), not a project marker. Without this guard a folder with no `.moss/`
        // resolves to `~` and moss scans the entire home directory. Canonical comparison
        // too — on Windows an ancestor can surface as an 8.3 short path
        // (`C:\Users\RUNNER~1`) while `dirs::home_dir()` is the long form.
        let home_dir = dirs::home_dir();
        let home_canon = home_dir.as_ref().and_then(|h| h.canonicalize().ok());

        for ancestor in start_dir.ancestors() {
            if let Some(ref home) = home_dir {
                let at_home = ancestor == home.as_path()
                    || matches!(
                        (ancestor.canonicalize().ok(), home_canon.as_ref()),
                        (Some(ac), Some(hc)) if &ac == hc
                    );
                if at_home {
                    break;
                }
            }
            if ancestor.join(".moss").is_dir() {
                return Some(Self::from_resolved(ancestor.to_path_buf()));
            }
        }
        None
    }

    /// The resolved absolute path.
    pub fn path(&self) -> &Path {
        Path::new(&self.path)
    }

    /// The resolved absolute path as `&str` — what the `folder_path: &str` call sites want.
    pub fn as_str(&self) -> &str {
        &self.path
    }

    /// Consume into the owned path string (for `RunMode`/`PipelineConfig` construction).
    pub fn into_path_string(self) -> String {
        self.path
    }

    /// THE root folder name. Empty only for `/`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The root folder name as `Option`, `None` for `/`.
    ///
    /// Exists because `ProjectInfo.folder_name` and `build::resolve_site_name` are
    /// `Option`-shaped on their plugin/Tauri-facing surfaces and must keep returning `None`
    /// for the pathological root — the same value `file_name()` produced there before.
    pub fn name_opt(&self) -> Option<&str> {
        Some(self.name.as_str()).filter(|n| !n.is_empty())
    }
}

/// How a [`VaultTarget`]'s root was determined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootKind {
    /// An ancestor owns a `.moss/` — the target belongs to a real vault.
    Vault,
    /// No ancestor owns a `.moss/`; `root` is the starting directory that
    /// [`VaultRoot::containing`] falls back to.
    ///
    /// For a DIRECTORY target this is the normal new-project case: the user
    /// pointed at a folder they want to turn into a site, and opening it builds.
    /// For a FILE target it means a loose document — nobody has asked for a site
    /// here, so building the containing folder would be an unrequested scan of
    /// whatever the file happens to sit in (`~/Downloads`, `~/Desktop`). That is
    /// the document-mode trigger.
    Unowned,
}

/// A vault root plus an optional in-vault file to open after the build.
#[derive(Debug, Clone, PartialEq)]
pub struct VaultTarget {
    pub root: VaultRoot,
    /// `/`-separated path from the root to the selected file or subfolder.
    /// `None` when the root itself was selected.
    pub target_file: Option<String>,
    /// Whether `root` is a real vault or the walk's fallback.
    pub root_kind: RootKind,
}

impl VaultTarget {
    /// True when this target is a loose document: a FILE with no `.moss/`
    /// ancestor. The document-mode predicate.
    ///
    /// Deliberately requires BOTH conditions. A directory with no `.moss/` is a
    /// new project and must keep building; a file inside a real vault belongs to
    /// a site and must keep its preview.
    pub fn is_loose_document(&self) -> bool {
        self.root_kind == RootKind::Unowned && self.target_file.is_some()
    }

    /// Resolve a filesystem path (file or directory) to a root + optional target.
    ///
    /// Directories: the root is the nearest ancestor owning `.moss/`. Files: the extension
    /// is validated (`.md`/`.markdown`) first.
    pub fn resolve(path: &Path) -> Result<Self, VaultPathError> {
        let resolved = resolve_input(path);
        if !resolved.exists() {
            return Err(VaultPathError::PathDoesNotExist(
                resolved.to_string_lossy().into_owned(),
            ));
        }
        if resolved.is_file() {
            match resolved.extension().map(|e| e.to_string_lossy().to_lowercase()) {
                Some(ext) if ext == "md" || ext == "markdown" => {}
                Some(ext) => return Err(VaultPathError::UnsupportedFileType(format!(".{}", ext))),
                None => return Err(VaultPathError::UnsupportedFileType("(none)".to_string())),
            }
        }
        let (root, root_kind) = match VaultRoot::find_containing(&resolved) {
            Some(vault) => (vault, RootKind::Vault),
            // Same fallback `VaultRoot::containing` applies — but recorded as
            // such, so a caller can tell a real vault from a guess.
            None => (VaultRoot::containing(&resolved), RootKind::Unowned),
        };
        let target_file = resolved
            .strip_prefix(root.path())
            .ok()
            .filter(|rel| !rel.as_os_str().is_empty())
            // Windows `strip_prefix` yields backslash-separated relative paths; normalize
            // to `/` so target_file uses the build convention.
            .map(|rel| moss_core::slug::normalize_separators(&rel.to_string_lossy()));
        Ok(Self {
            root,
            target_file,
            root_kind,
        })
    }
}

#[cfg(test)]
#[path = "vault_root_tests.rs"]
mod tests;
