//! Finding, and when asked installing, the pinned binary a derivation runs.
//!
//! The pin is read as a stack (`DerivationContribution::as_stack`), so the
//! stack executor's `obtain` and `stage` do the download, hash check and
//! unpacking, and `binary_path` says where the runnable file lands. A `path`
//! source is a binary the user already has: nothing is fetched.

use std::path::PathBuf;

use super::DeriveFailure;
use crate::system::derive::registry::DerivationEntry;
use crate::system::large_download::ProgressFn;
use crate::system::stack_exec::{self, layout, StackHome};

/// Hash of the whole declaration, locations included: names the install
/// directory, so a changed URL or archive installs afresh.
fn declaration_hash(entry: &DerivationEntry) -> String {
    use sha2::{Digest, Sha256};
    let pinned = serde_json::json!({
        "run": entry.contribution.run,
        "fragments": entry.contribution.fragments,
    });
    format!("{:x}", Sha256::digest(pinned.to_string().as_bytes()))
}

#[cfg(test)]
thread_local! {
    /// How many times [`content_hash`] read a file rather than remembering it.
    pub(super) static COMPUTED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Content hash of a file, recomputed only when its size or modification
/// time changes. A build asks for each demanded file's hash several times
/// (the lookup, the status, the run) and a local binary's once per file, so
/// this is one read per changed file, not one per question.
pub(super) fn content_hash(path: &std::path::Path) -> Result<String, String> {
    use std::sync::{Mutex, OnceLock};
    type Key = (PathBuf, u64, Option<std::time::SystemTime>);
    static HASHES: OnceLock<Mutex<std::collections::HashMap<Key, String>>> = OnceLock::new();
    let meta = std::fs::metadata(path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    let key = (path.to_path_buf(), meta.len(), meta.modified().ok());
    let hashes = HASHES.get_or_init(Default::default);
    if let Some(hash) = hashes.lock().ok().and_then(|h| h.get(&key).cloned()) {
        return Ok(hash);
    }
    #[cfg(test)]
    COMPUTED.with(|count| count.set(count.get() + 1));
    let hash = crate::build::cache::ObjectStore::hash_file(path)?;
    if let Ok(mut hashes) = hashes.lock() {
        hashes.insert(key, hash.clone());
    }
    Ok(hash)
}

/// The content hash of the binary a `path` source names, `None` for any
/// other source (a pinned download carries its hash in the declaration).
fn local_binary_hash(entry: &DerivationEntry) -> Option<String> {
    let stack = entry.contribution.as_stack(&stack_id(entry), &entry.plugin_version);
    let local = matches!(layout::winning_source(&stack), Some(crate::plugins::contributions::stack::StackSource::Path { .. }));
    resolve_binary(entry).ok().filter(|_| local).and_then(|path| content_hash(&path).ok())
}

/// What a cached result depends on besides the input file: the declared
/// fragments and the executable's identity, never where it came from. An
/// installed executable (downloaded or `path`) is identified by its content,
/// so moving the same binary between the two keeps every result. Before a
/// download is installed its declared archive sha256 stands in, which keys
/// differently from the installed executable.
pub fn pin_fingerprint(entry: &DerivationEntry) -> String {
    fingerprint(entry, resolve_binary(entry).ok().as_deref())
}

fn fingerprint(entry: &DerivationEntry, installed: Option<&std::path::Path>) -> String {
    use crate::plugins::contributions::stack::StackSource;
    use sha2::{Digest, Sha256};
    let stack = entry.contribution.as_stack(&stack_id(entry), &entry.plugin_version);
    let identity = match installed.and_then(|path| content_hash(path).ok()) {
        Some(hash) => format!("exe:{hash}"),
        None => match layout::winning_source(&stack) {
            Some(StackSource::Download { sha256, .. }) => format!("archive:{}", sha256.clone().unwrap_or_default()),
            _ => "absent".to_string(),
        },
    };
    let pinned = serde_json::json!({ "fragments": entry.contribution.fragments, "binary": identity });
    format!("{:x}", Sha256::digest(pinned.to_string().as_bytes()))
}

/// The content hashes of the `path`-sourced binaries a plugin's derivations
/// run, in derivation order. A user's consent to the plugin names its code;
/// for these the manifest names only where the binary is, so the binary's
/// bytes have to be part of what was allowed.
pub fn local_binary_hashes(plugin_dir: &std::path::Path, manifest: &crate::plugins::types::PluginManifest) -> Vec<String> {
    let Some(derivations) = manifest.contributes.as_ref().and_then(|c| c.derivations.as_ref()) else {
        return Vec::new();
    };
    derivations
        .iter()
        .filter_map(|(derivation_id, contribution)| {
            local_binary_hash(&DerivationEntry {
                plugin_id: manifest.name.clone(),
                derivation_id: derivation_id.clone(),
                plugin_version: manifest.version.clone(),
                plugin_dir: plugin_dir.to_path_buf(),
                contribution: contribution.clone(),
            })
        })
        .collect()
}

/// Where this derivation's binary is installed: keyed by plugin version and
/// the declaration's pin, so a new version or a re-pin never runs the old
/// binary.
fn home(entry: &DerivationEntry) -> Result<PathBuf, DeriveFailure> {
    let root = layout::stacks_root().map_err(DeriveFailure::new)?;
    Ok(root
        .join("derive")
        .join(&entry.plugin_id)
        .join(&entry.derivation_id)
        .join(format!("{}-{}", entry.plugin_version, &declaration_hash(entry)[..12])))
}

fn stack_id(entry: &DerivationEntry) -> String {
    format!("{}/{}", entry.plugin_id, entry.derivation_id)
}

/// The installed binary, or the sentence saying why there is none.
pub fn resolve_binary(entry: &DerivationEntry) -> Result<PathBuf, DeriveFailure> {
    binary_in(entry, &home(entry)?)
}

fn binary_in(entry: &DerivationEntry, root: &std::path::Path) -> Result<PathBuf, DeriveFailure> {
    let stack = entry.contribution.as_stack(&stack_id(entry), &entry.plugin_version);
    let path = layout::binary_path(root, &stack)
        .map_err(|e| DeriveFailure::new(format!("{}: {e}", stack_id(entry))))?;
    if path.is_file() {
        Ok(path)
    } else {
        Err(DeriveFailure::new(format!(
            "{} is not installed (expected {})",
            entry.contribution.run.binary,
            path.display()
        )))
    }
}

/// [`resolve_binary`], downloading and unpacking the pinned artifact first when
/// it is not there yet. Network and disk work: call it before a build batch,
/// never from the per-file path.
pub fn ensure_binary(
    entry: &DerivationEntry,
    on_bytes: Option<&ProgressFn<'_>>,
) -> Result<PathBuf, DeriveFailure> {
    if let Ok(path) = resolve_binary(entry) {
        return Ok(path);
    }
    if let Some(platforms) = crate::plugins::contributions::platform::unserved(&entry.contribution) {
        return Err(DeriveFailure::new(crate::plugins::contributions::platform::sentence(&entry.plugin_id, &platforms)));
    }
    install_into(entry, &home(entry)?, on_bytes)
}

/// The download-and-unpack half of [`ensure_binary`], into an explicit `root`.
fn install_into(
    entry: &DerivationEntry,
    root: &std::path::Path,
    on_bytes: Option<&ProgressFn<'_>>,
) -> Result<PathBuf, DeriveFailure> {
    let stack = entry.contribution.as_stack(&stack_id(entry), &entry.plugin_version);
    let fail = |e: stack_exec::StackExecError| DeriveFailure::new(format!("{}: {e}", stack_id(entry)));
    let cache_dir = root.join(".downloads");
    let _installing = stack_exec::lock_install(root).map_err(fail)?;
    // A peer holding the lock may have installed it while this one waited.
    if let Ok(path) = binary_in(entry, root) {
        return Ok(path);
    }
    let stack_home = StackHome { root, stack: &stack };
    let artifact = stack_exec::obtain(&stack_home, &cache_dir, on_bytes).map_err(fail)?;
    stack_exec::stage(&stack_home, &artifact, &mut || Ok(()), &mut || {}).map_err(fail)?;
    binary_in(entry, root)
}

#[cfg(test)]
#[path = "binary_tests.rs"]
mod binary_tests;
