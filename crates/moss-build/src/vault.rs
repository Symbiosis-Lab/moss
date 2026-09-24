//! The vault family's crossed members: the path alias and the plugin-facing
//! filesystem sandbox. The rest of the family stays app-side.

/// Path alias so the moved tree's 51 `crate::vault::paths::…` spellings stay
/// valid: vault-root identity has lived in this crate since the crate split.
pub mod paths {
    pub use crate::vault_root::*;
}

// `PluginPath` sandboxing (crossed at open-CLI slice 2 — the engine's
// file arms resolve plugin-relative paths through it).
pub mod fs;
// Bringing external content INTO the folder: the scrape pipeline and the
// site-builder dialect importers (03-module-tree routes `src/scrape/` here).
// Crossed together because they are mutually recursive — `scrape::converter`
// calls `import::engine`, and `import::engine` names `scrape::converter`'s
// types.
pub mod import;
// `.moss/config.toml`'s writer primitive and `save_environment` (amendment,
// 2026-09-07): the door both binaries' `moss env` writes through.
pub mod config;
// `.moss/data/events.jsonl` and its cursor (2026-09-09, track C4e): the
// author's analytics log, and the sync loop that is its only writer.
pub mod analytics;
// `.moss/state.toml`'s `[deployment]` writers (2026-09-07, track C4b): what a
// publish records, written the same way from either binary.
pub mod deployment_state;
pub mod synced_siblings;
// `.moss/places.toml`'s reader: the gazetteer a `location:` frontmatter
// value looks a place name up in (task A1, places build slice).
pub mod places;
