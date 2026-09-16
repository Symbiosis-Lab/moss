//! The open binary's side of the pipeline's host seam: [`HostPorts::headless`]
//! with the store a terminal answers for.
//!
//! Lived in `moss-cli/src/host.rs` until C4f, where `moss deploy` gained a
//! route that builds. That route is `cli::deploy`, inside this crate, so the
//! store had to be reachable from here — and the alternative was a second
//! `HostStore` for the same binary, differing from this one by nothing.

use std::sync::Arc;

use crate::build::{HostPorts, HostStore};

/// Resolve every host capability the pipeline needs, once, at the entry point.
/// The shared headless answers live on the struct; what is the open binary's
/// own is [`CliStore`].
pub fn cli_host_ports(folder_path: &str) -> HostPorts {
    HostPorts::headless(folder_path, Arc::new(CliStore))
}

/// The open binary's [`HostStore`]: no pins, and — deliberately — no writes to
/// the user's files.
struct CliStore;

impl HostStore for CliStore {
    fn is_pinned(&self, _generation: &str) -> bool {
        false
    }

    /// Not persisted, same reason. Config VALUES are still correct: the one
    /// parse funnel (`ConfigFile::parse`) migrates in memory, so a legacy
    /// config builds at its migrated meaning with the file untouched.
    fn run_vault_migrations(&self, root: &crate::vault::paths::VaultRoot) {
        log::debug!(target: "config", "config for {} migrates in-memory on read; on-disk migration runs when the moss app opens it", root.as_str());
    }
}
