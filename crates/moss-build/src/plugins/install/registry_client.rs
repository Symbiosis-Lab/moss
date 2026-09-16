//! The moss-side registry client: what the app knows about plugins it did not
//! ship with.
//!
//! Four layers, split by what each is blind to rather than by noun — the
//! rules most likely to be wrong are the ones a network or a disk dependency
//! would make untestable, so they own a file that has neither:
//!
//! | layer | owns | I/O |
//! |---|---|---|
//! | [`index`] | the wire schema, serial monotonicity, the fail-closed revocation rules | none |
//! | [`cache`] | app-data persistence and the monotonic serial floors | disk |
//! | [`fetch`] | the pinned origin, and the refresh that composes the three | network |
//! | [`artifact`] | turning one index entry into an installed plugin directory | both |
//!
//! [`enforce`] is the verdict those layers feed: whether a given plugin may
//! install or load at all. [`receipt`] is what an install leaves behind in the
//! plugin directory so a later update can tell the plugin's own files from the
//! last version's code; [`approval`] is what the user's consent leaves behind
//! in app data, so a plugin that arrived with a shared folder does not run
//! until they say so.
//!
//! Named `registry_client` rather than `registry` because
//! `plugins/registry.rs` — the catalog — migrates into this directory later
//! (`docs/reference/target/03-module-tree.md`), and two files called
//! "registry" meaning different things is how the next reader loses.
//!
//! Design: `docs/archive/2026-07-23-plugin-registry-design.md` (client, M2).

pub mod approval;
pub mod artifact;
pub mod cache;
pub mod enforce;
pub mod fetch;
pub mod icon;
pub mod index;
pub mod receipt;
