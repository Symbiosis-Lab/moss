//! Data types and structures for moss publishing system
//!
//! Split 2026-08-10 (M5a) into sibling modules along the M6a boundary:
//! [`content`] holds pure build data a future headless moss-build crate
//! needs; [`events`] holds frontend-facing payloads; [`runtime`],
//! [`assets`] and [`services`] hold app machinery.
//!
//! This file is the module entry only. The flat `crate::types::X`
//! re-export surface (and the `crate::build::types` / `crate::deploy::types`
//! back-compat re-exports) was burned down 2026-08-12 (M5a landing 3):
//! every consumer imports each item from its owning module directly.

pub mod assets;
pub mod content;
pub mod event_payloads;
pub mod events;
pub mod runtime;
pub mod services;
pub mod toast;

// The previous `pending_renames` queue and its tests were removed in PR-1.
// Rename pair stitching now happens at the watcher
// layer via `notify-debouncer-full`'s inode/file-id pairing; see
// the desktop app's build watch module.
