//! External asset and binary resolution for the moss build pipeline.
//!
//! Handles resolution of external assets (JupyterLite, etc.) and binaries
//! (FFmpeg, Git, Hugo) through a unified download-on-demand pipeline.

pub mod asset_resolver;
pub mod binary_resolver;
pub mod download;
pub mod git;
pub mod paths;
