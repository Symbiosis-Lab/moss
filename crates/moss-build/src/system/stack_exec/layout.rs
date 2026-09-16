//! Every path the executor derives, from the declaration and the stack's own
//! home directory — never a hardcoded bundle name. The old
//! `system::stack_install` (`OnionPress.app`, `Contents/MacOS/onionpress`)
//! is the shape this replaces: a second stack used to cost four more
//! constants, and now costs a manifest block.

use std::path::{Path, PathBuf};

use crate::plugins::contributions::stack::{self, StackContribution, StackSource};

use super::StackExecError;

/// `~/.moss/stacks`, `MOSS_STACKS_ROOT` overriding. The ONE definition — it
/// used to be defined twice (`stack_install.rs` and `deploy::stack_activity`,
/// which would have made an executor's copy the third). Tests need a root
/// that is not the developer's own `~/.moss`.
pub fn stacks_root() -> Result<PathBuf, String> {
    if let Some(over) = std::env::var_os("MOSS_STACKS_ROOT") {
        return Ok(PathBuf::from(over));
    }
    dirs::home_dir()
        .map(|h| h.join(".moss").join("stacks"))
        .ok_or_else(|| "Cannot determine home directory".to_string())
}

/// The source this host's platform can install from, if the declaration has
/// one. Reads the host's own platform key once here so the selection itself
/// (`contributions::stack::artifact_source`) stays a pure function of its
/// arguments, exactly as `system::stack_install::pin::artifact_source` does
/// today.
pub(crate) fn winning_source(stack: &StackContribution) -> Option<&StackSource> {
    let key = crate::build::assets::binary_resolver::get_current_platform();
    stack::artifact_source(stack, key.as_deref().ok())
}

/// A relative, declaration-supplied path resolved onto `base` — never
/// `base.join(rel)` directly. Rejects an absolute path and any `..`
/// component, naming `field` in the refusal so the error names the
/// declaration's own field rather than this function.
pub fn enclosed(base: &Path, rel: &str, field: &'static str) -> Result<PathBuf, StackExecError> {
    let rel_path = Path::new(rel);
    let escapes = rel_path.is_absolute()
        || rel_path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir));
    if escapes {
        return Err(StackExecError::UnsafeDeclaredPath {
            field,
            value: rel.to_string(),
        });
    }
    Ok(base.join(rel_path))
}

/// Where an archive format's contents land, and `binary_path`'s default when
/// a `Download` source names no `executable`. A fixed, format-agnostic name
/// inside `home` — never a per-app name, and never fallible: it names a path,
/// it does not touch disk.
pub fn bundle_root(home: &Path) -> PathBuf {
    home.join("bundle")
}

/// The scratch sibling `stage`'s rotation assembles the new bundle at.
/// Dot-prefixed so a leftover from a crashed attempt is never mistaken for
/// an install by [`is_installed`].
pub fn incoming_path(home: &Path) -> PathBuf {
    home.join(".bundle.incoming")
}

/// The scratch sibling the previously-installed bundle is displaced to
/// during the rotation. See [`incoming_path`].
pub fn outgoing_path(home: &Path) -> PathBuf {
    home.join(".bundle.outgoing")
}

/// Re-discovery: "installed" == the runnable binary exists on disk.
/// Registry state is in-memory and gone after a restart; disk is the source
/// of truth.
pub fn is_installed(home: &Path, stack: &StackContribution) -> bool {
    binary_path(home, stack).map(|p| p.exists()).unwrap_or(false)
}

/// The binary to run, from the platform's winning source. `Download`: its
/// `executable` when present, resolved UNDER `bundle_root` — that is where
/// `stage` actually extracts every non-`raw` format, so an `executable`
/// resolved against `home` directly would name a path one level too high and
/// never exist. Else `bundle_root` itself: the artifact's contents ARE the
/// binary when nothing more specific is declared. `Path`: its `binary`, used
/// as-is when absolute and joined onto `home` otherwise. Every relative
/// value goes through [`enclosed`], so an absolute path in the `Download`
/// arm or any `..` component in either is `UnsafeDeclaredPath`.
pub fn binary_path(home: &Path, stack: &StackContribution) -> Result<PathBuf, StackExecError> {
    let source = winning_source(stack).ok_or_else(|| StackExecError::NoSourceForPlatform {
        stack: stack.id.clone(),
    })?;
    match source {
        StackSource::Download { executable, .. } => match executable {
            Some(rel) => enclosed(&bundle_root(home), rel, "executable"),
            None => Ok(bundle_root(home)),
        },
        StackSource::Path { binary, .. } => {
            let p = Path::new(binary);
            if p.is_absolute() {
                Ok(p.to_path_buf())
            } else {
                enclosed(home, binary, "binary")
            }
        }
    }
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod layout_tests;
