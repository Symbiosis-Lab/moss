//! The open-CLI features that ride the build itself: agent-guidance sync
//! (`agents/`), called mid-conductor by `build::run_pipeline`, and the
//! terminal verbs both binaries answer.
pub mod agents;
// `describe` prints the contract: tokens, slots, plugin hooks, the command
// table. Every input was already here or in moss-core; only the binary's
// version is not, so `run` takes it from the caller.
pub mod describe;
// The command table + `<cmd> --help`, read by both binaries.
pub mod commands;
// `doctor` and `list` read a vault and print — no window, no app state, no
// network. Both already called nothing but moss-build (`vault_root`,
// `build::emit::inventory`, `build::scan::classify`) through the app crate's
// re-export shims, so crossing them resolves the same items by a shorter path.
pub mod doctor;
pub mod list;
// `env` reads and writes the environment through `vault::config`, the one
// config writer both binaries share.
pub mod env;
// `rename` is the editor's rename-with-refs from the terminal; its whole body
// is `editor::ref_scan`, which crossed with it.
pub mod rename;
// `guide` reads nothing but the binary — no project, no network, no window —
// and its whole body was already `agents::skill_package`, so it belongs beside
// what it prints rather than one crate up behind a shim.
pub mod guide;
// The nested-site decision table + CLI refusal shapes, shared by both hosts
// (open-CLI slice 3). The GUI surfaces stay app-side.
pub mod site_guard;
// `domain list` / `domain link` talk to seta with the vault's own identity.
// The client crossed into this crate, and every other door this file opens
// (VaultRoot, site_config, Identity::load_for_signing) was already here.
pub mod domain;
// Argument parsing for `moss import`, over the engine in
// `vault::import::scrape::run`.
pub mod import;
// `moss deploy --prebuilt` over the shared driver in `deploy::prebuilt`; the
// app's own deploy mode calls the same one with a Tauri sink instead of a
// printing one (track C4c).
pub mod deploy;
// The `HostPorts` a terminal build runs with, moved from `moss-cli` at C4f so
// that `deploy`'s build route — which is in this crate — can reach it.
pub mod host;
