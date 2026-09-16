//! The tauri-free stack executor (ADR-080 item 1, S4 of
//! `docs/archive/2026-09-11-stack-executor-s4-plan.md`).
//!
//! `obtain` / `stage` / `run` are the whole surface an app-side caller needs
//! to acquire, unpack and drive a plugin-declared stack
//! (`plugins::contributions::stack::StackContribution`). This module owns
//! none of the app's own concerns — progress UI, `AppHandle`, the receiver
//! HTTP probe, the typed `Stop` reason — those stay in `src-tauri` (S4 §1,
//! S5). If any signature here needed one of them, this crate would be the
//! wrong home for it, and `check-crate-dag.mjs` rule 2 would say so at the
//! first compile.
//!
//! This slice lands `stage` (step 3), `obtain` (step 4) and `run` (step 5),
//! each re-exported here from the submodule that implements it so a caller
//! reaches all three as `stack_exec::{obtain, stage, run}`.

pub mod artifact;
pub mod exec;
pub mod extract;
pub mod layout;

pub use artifact::obtain;
pub use exec::{run, RunOpts, Verb};

use std::fmt;
use std::path::{Path, PathBuf};

use crate::plugins::contributions::stack::{StackContribution, StackSource};

/// A stack's home directory and its declaration together — every layout,
/// extraction and staging decision reads both, so the pair travels as one
/// value instead of two positional arguments that could be passed in either
/// order.
pub struct StackHome<'a> {
    pub root: &'a Path,
    pub stack: &'a StackContribution,
}

/// What `stage` promotes: a downloaded file waiting to be extracted, or the
/// no-op case a `Path` source produces — no artifact is ever fetched for
/// one, so there is nothing here to stage.
#[derive(Debug)]
pub enum Artifact {
    File(PathBuf),
    AlreadyPresent,
}

#[derive(Debug)]
pub enum StackExecError {
    /// The declaration has no source for this host's platform.
    NoSourceForPlatform { stack: String },
    /// A `download` source with no `sha256` — refused before any bytes move.
    MissingHash { stack: String, url: String },
    /// A `download` source whose `sha256` is a known placeholder value.
    PlaceholderHash { stack: String, url: String },
    /// The downloaded (or cached) bytes do not match the declared hash.
    HashMismatch { url: String, expected: String, actual: String },
    /// `archive_format` names something no extractor arm handles.
    UnsupportedFormat { format: String },
    /// An archive entry tried to escape the extraction target.
    UnsafeEntry { archive: String, entry: String },
    /// A declaration-supplied path (`executable`, `binary`, …) tried to
    /// escape the stack home.
    UnsafeDeclaredPath { field: &'static str, value: String },
    /// The stack is not installed where a caller expected it to be.
    NotInstalled { expected: PathBuf },
    Obtain(String),
    Extract(String),
    Run(String),
}

impl fmt::Display for StackExecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StackExecError::NoSourceForPlatform { stack } => {
                write!(f, "stack '{stack}' declares no source for this platform")
            }
            StackExecError::MissingHash { stack, url } => {
                write!(f, "stack '{stack}' source {url} declares no sha256")
            }
            StackExecError::PlaceholderHash { stack, url } => {
                write!(f, "stack '{stack}' source {url} carries a placeholder sha256")
            }
            StackExecError::HashMismatch { url, expected, actual } => write!(
                f,
                "{url} sha256 mismatch: expected {expected}, got {actual}"
            ),
            StackExecError::UnsupportedFormat { format } => {
                write!(f, "unsupported archive format '{format}'")
            }
            StackExecError::UnsafeEntry { archive, entry } => {
                write!(f, "{archive} entry '{entry}' escapes the extraction target")
            }
            StackExecError::UnsafeDeclaredPath { field, value } => {
                write!(f, "{field} '{value}' escapes the stack home")
            }
            StackExecError::NotInstalled { expected } => {
                write!(f, "expected an installed stack at {}", expected.display())
            }
            StackExecError::Obtain(e) => write!(f, "{e}"),
            StackExecError::Extract(e) => write!(f, "{e}"),
            StackExecError::Run(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for StackExecError {}

/// Best-effort removal of whatever is at `path` — a directory (the ordinary
/// archive-format case) or a plain file (the `raw`-format case, where the
/// scratch path is the binary itself).
fn remove_any(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// Extract/stage `artifact` into `home.root` through the hardened extractor
/// for the source's declared `archive_format`, promoting by atomic rotation.
///
/// `before_extract` runs once, immediately before the archive is unpacked —
/// the last point at which the OLD bundle (if any) is still exactly what it
/// was before this call, and the only hook a caller can use to stop whatever
/// it is currently running before replacing it out from under it. Unlike
/// `before_replace`, it can refuse the whole call: a caller whose "stop the
/// old process" step fails must not go on to extract and rotate as if it had
/// succeeded, so the closure returns a `Result` and `stage` propagates an
/// `Err` from it before touching anything on disk. Pass `&mut || Ok(())`
/// when there is nothing to stop.
///
/// `before_replace` runs once, after the new bundle is fully staged at the
/// scratch path and immediately before the first rename of the rotation.
/// From that call until the caller writes a new receipt, whatever is at the
/// bundle path is of uncertain vintage, so a caller that keeps a receipt
/// forgets it HERE — an interrupted rotation then reads as stale (safe,
/// converges by re-install) rather than as current. Pass `&mut || {}` when
/// there is nothing to forget; it is not an `Option`, so no caller can omit
/// it by accident. (Moved verbatim from `stack_install.rs:888-889`.)
pub fn stage(
    home: &StackHome<'_>,
    artifact: &Artifact,
    before_extract: &mut dyn FnMut() -> Result<(), StackExecError>,
    before_replace: &mut dyn FnMut(),
) -> Result<(), StackExecError> {
    let artifact_path = match artifact {
        // A `Path` source has no artifact — `obtain` never fetches one for
        // it, and there is nothing here to unpack or rotate into place.
        Artifact::AlreadyPresent => return Ok(()),
        Artifact::File(path) => path,
    };

    let source = layout::winning_source(home.stack).ok_or_else(|| StackExecError::NoSourceForPlatform {
        stack: home.stack.id.clone(),
    })?;
    let format = match source {
        StackSource::Download { archive_format, .. } => archive_format.as_str(),
        // Mirrors the guard above: a `Path` source never produces a `File`
        // artifact, so this arm should be unreachable in practice.
        StackSource::Path { .. } => return Ok(()),
    };

    // `raw` has no archive structure to unpack into a directory: the
    // artifact bytes ARE the binary, so the rotation's destination is
    // wherever the declaration says the binary lives. Every other format
    // extracts into the generic bundle location, and `binary_path` resolves
    // the runnable file inside it afterward.
    let dest = if format == "raw" {
        layout::binary_path(home.root, home.stack)?
    } else {
        layout::bundle_root(home.root)
    };

    let incoming = layout::incoming_path(home.root);
    let outgoing = layout::outgoing_path(home.root);

    // A scratch path left by a crashed attempt is never adopted — its
    // contents are a copy that stopped somewhere unknown.
    if incoming.exists() {
        remove_any(&incoming)
            .map_err(|e| StackExecError::Extract(format!("remove leftover staging path: {e}")))?;
    }

    before_extract()?;

    extract::extract(format, artifact_path, &incoming)?;

    before_replace();

    // Promote by ROTATION, never by delete-then-rename: deleting first
    // destroys the installed bundle before its replacement is proven
    // placeable, and both renames below are within one directory, so each
    // is atomic and the old bundle stays recoverable right up to the moment
    // the new one is in place.
    if dest.exists() {
        std::fs::rename(&dest, &outgoing)
            .map_err(|e| StackExecError::Extract(format!("move the installed bundle aside: {e}")))?;
    }
    let promote = std::fs::rename(&incoming, &dest).map_err(|e| {
        if outgoing.exists() {
            let _ = std::fs::rename(&outgoing, &dest);
        }
        StackExecError::Extract(format!("move staged bundle into place: {e}"))
    });

    // Neither scratch path may outlive the attempt.
    if incoming.exists() {
        let _ = remove_any(&incoming);
    }
    if outgoing.exists() && dest.exists() {
        let _ = remove_any(&outgoing);
    }

    promote?;
    make_binary_executable(home);
    Ok(())
}

/// Every extractor arm can land a non-runnable file: `tar_gz_safe`'s
/// `File::create` and the hardened zip writer both default to the
/// platform's ordinary file mode rather than carrying the archive entry's
/// own unix mode, and a `raw` download's mode is whatever the HTTP fetch
/// gave it — usually not executable. Set the bit once here, on the actual
/// binary `stage` just promoted, rather than inside every arm that writes
/// one. Best-effort: a `binary_path` that cannot be resolved or is not a
/// plain file (a `dmg`'s `.app` bundle, whose own `cp -R` already preserved
/// the mounted image's modes) is left alone.
#[cfg(unix)]
fn make_binary_executable(home: &StackHome<'_>) {
    use std::os::unix::fs::PermissionsExt;
    let Ok(bin) = layout::binary_path(home.root, home.stack) else {
        return;
    };
    if !bin.is_file() {
        return;
    }
    let _ = std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755));
}

#[cfg(not(unix))]
fn make_binary_executable(_home: &StackHome<'_>) {}
