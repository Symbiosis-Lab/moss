//! moss-build — the UI-free build pipeline crate.
//!
//! Seeded 2026-08-11 with vault-root identity per
//! [ADR-051](../../../../docs/decisions/ADR-051-vault-root-ships-in-moss-build.md): root
//! identity is the compiler's *input identity* (`PipelineConfig.root` is a
//! [`vault_root::VaultRoot`]), so the owner lands here before the M6a pipeline move —
//! the moved pipeline files then reach it in-crate instead of needing a forbidden
//! `moss-build → moss` edge (`scripts/check-crate-dag.mjs` rule 4).
//!
//! Joined 2026-08-18 by the `.moss` **layout** owner per
//! [ADR-057](../../../../docs/decisions/ADR-057-moss-paths-ships-in-moss-build.md): the crate
//! that writes `.moss/build/generations/<id>/` owns the names for it, and 22 of
//! `MossPaths`'s consumers sit in the build tree that moves at M6a. Same argument as
//! ADR-051, second type — identity, then layout.
//!
//! Joined 2026-08-18 by [`config`] — the `.moss/config.toml` **reader** and the pure
//! migration transform, per
//! [ADR-059](../../../../docs/decisions/ADR-059-config-reader-and-migration-runner-after-the-crate-split.md).
//! The app still owns the file. Its modal-driven writers stay app-side; the one write
//! primitive they share (`vault::config`, amendment of 2026-09-07) is here so the open
//! binary's `env` write and the compiler's plugin uninstall edit the user's bytes
//! through the same door, never re-emit them.
//!
//! Until M6a the rest of the compiler charter
//! (scan → render → emit, headless, no tauri) migrates in per
//! `docs/reference/target/03-module-tree.md`.

pub mod advisory;
pub mod config;
pub mod moss_paths;
pub mod nested_roots;
pub mod vault_root;

// ── The M6a move (2026-08-28): the compiler itself ──────────────────────────
//
// The build tree and its private support cluster crossed from `src-tauri` in
// one enlarged `git mv` (docs/archive/2026-08-27-m6a-execution-plan.md § M).
// Files kept their intra-tree `crate::…` spellings, so the module names here
// mirror the app crate's: `build`, `i18n`, `types`, `tasks`, and the partial
// families below. `src-tauri` re-exports each at its old path (shim table:
// docs/reference/target/MIGRATION-STATE.md).
pub mod build;
// The editor backend (ADR-071): resolve/tree/content/source-asset cores the
// HTTP command carrier calls; the app keeps the Tauri wrappers + shims.
pub mod editor;
// The QuickJS plugin engine core (open-CLI slice 2, #1019): pure rquickjs +
// tokio, app reach injected through `engine::app_host::AppHost`.
pub mod engine;
pub mod i18n;
// Operational modes: the preview server (ops/serve, S1 of the ADR-067
// relocation, 2026-08-28); ops/watch follows at W1.
pub mod ops;
pub mod tasks;
pub mod types;

/// Partial families: only the members the build tree reaches crossed; the rest
/// of each family stays app-side and re-exports the crossed members.
pub mod cli;
pub mod deploy;
pub mod identity;
pub mod infra;
pub mod platform;
pub mod seta;
pub mod plugins;
pub mod system;

pub mod vault;

/// Crate-wide mutex serializing every test that mutates process-global env vars
/// (`MOSS_ENV`, `MOSS_SETA_URL`, `STRIPE_PUBLISHABLE_KEY`, `MOSS_MATTERS_*`).
/// Module-local mutexes don't serialize across modules in the same test binary.
/// Crossed with the build tree; `src-tauri` keeps its own copy — the lock is
/// per test binary, so a cross-crate re-export would serialize nothing.
#[cfg(test)]
pub(crate) static ENV_TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Wine skip guard (ADR-033 amendment): a test may skip under Wine ONLY via
/// this self-detection — the `HKLM\Software\Wine` registry key exists in every
/// Wine prefix and is structurally absent on real Windows, so a skip taken
/// here can never hide a real-Windows failure. Callers must eprintln! the
/// skip so it stays loud in the run log.
#[cfg(all(test, windows))]
pub(crate) fn running_under_wine() -> bool {
    std::process::Command::new("reg")
        .args(["query", r"HKLM\Software\Wine"])
        .output()
        .is_ok_and(|o| o.status.success())
}
