//! What a publish carries for members: who may subscribe (`open` or
//! `invite`), read by the hosting service, and the members-only version of
//! each gated address.
//!
//! The version only members may read of the address `P` is written at
//! `_gated/P` in the published root: a page at `_gated/<path>/index.html`, a
//! data file at `_gated/<path>`. A page keeps its public version at `P`; a
//! data file has no file at `P`. A host that can check a reader's membership
//! serves `_gated/P` at `P` to a member and never serves the prefix itself.
//! Every other host is sent [`public_files`] only.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use moss_core::frontmatter::members::Members;
use serde::Serialize;
use crate::build::{context::BuildContext, manifest::{HashBucket, PendingManifest}, served_path::ServedPath};
use crate::config::services::MembersConfig;

pub const MEMBERS_PATH: &str = "_moss/members.json";

pub fn emit(config: &MembersConfig, output: &Path, pending: &mut PendingManifest) -> Result<(), String> {
    let bytes = serde_json::to_vec(config).map_err(|e| format!("Cannot serialize membership policy: {e}"))?;
    BuildContext::for_render(output, pending)
        .emit_held(&ServedPath::for_members_json(), bytes, HashBucket::Files)
        .map_err(|e| format!("Failed to emit {MEMBERS_PATH}: {e}"))
}

pub use crate::build::served_path::GATED_PREFIX;

/// What a host answers at a gated address for a reader who is not a member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Fallback {
    /// The page's public version, which the publish also holds at the address.
    Projection,
    /// Nothing: the address answers 401 with an empty body.
    Unauthorized,
}

/// The access policy of one gated file, as the upload manifest carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct GatedEntry {
    pub access: Members,
    pub fallback: Fallback,
}

/// The address a gated file is served at, or `None` for a public file.
pub fn gated_address(path: &str) -> Option<&str> {
    path.strip_prefix(GATED_PREFIX)
}

/// The policy of every gated file in `files` (output path to manifest entry),
/// keyed by the file's own path. The fallback is read from the files: a gated
/// file whose address also holds a public file falls back to it.
pub fn gated_entries(files: &HashMap<String, String>) -> BTreeMap<String, GatedEntry> {
    files.keys().filter_map(|path| {
        let address = gated_address(path)?;
        let fallback = if files.contains_key(address) { Fallback::Projection } else { Fallback::Unauthorized };
        Some((path.clone(), GatedEntry { access: Members::Subscribers, fallback }))
    }).collect()
}

/// `files` without the members-only versions: what a host that cannot check
/// membership is sent. Says so when that leaves something out.
pub fn public_files(files: &HashMap<String, String>) -> HashMap<String, String> {
    let public: HashMap<_, _> = files.iter().filter(|(path, _)| gated_address(path).is_none()).map(|(p, e)| (p.clone(), e.clone())).collect();
    let withheld = files.len() - public.len();
    if withheld > 0 {
        crate::build::cli_output::log_note!(
            "{withheld} members-only {} not published: this host cannot check who is a member. Public versions are published.",
            if withheld == 1 { "file was" } else { "files were" }
        );
    }
    public
}

/// Whether `files` holds a members-only version.
pub fn has_gated(files: &HashMap<String, String>) -> bool {
    files.keys().any(|path| gated_address(path).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(paths: &[&str]) -> HashMap<String, String> {
        paths.iter().map(|p| (p.to_string(), "100644:0123456789abcdef".to_string())).collect()
    }

    #[test]
    fn the_upload_manifest_names_each_gated_file_with_its_access_and_fallback() {
        let files = files(&["index.html", "post/index.html", "_gated/post/index.html", "_gated/map/detail/incidents.csv"]);
        let gated = gated_entries(&files);
        assert_eq!(
            serde_json::to_value(&gated).unwrap(),
            serde_json::json!({
                "_gated/map/detail/incidents.csv": { "access": "subscribers", "fallback": "unauthorized" },
                "_gated/post/index.html": { "access": "subscribers", "fallback": "projection" },
            })
        );
    }

    #[test]
    fn a_host_that_cannot_check_membership_is_sent_no_gated_file() {
        let files = files(&["index.html", "post/index.html", "_gated/post/index.html", "_gated/data.json"]);
        let mut public: Vec<_> = public_files(&files).into_keys().collect();
        public.sort();
        assert_eq!(public, ["index.html", "post/index.html"]);
        assert!(gated_entries(&public_files(&files)).is_empty());
    }

    #[test]
    fn no_source_file_may_sit_under_the_reserved_name_at_any_depth() {
        for path in ["_gated/notes.csv", "_Gated/notes.csv", "notes/_gated/x.csv", "notes/_GATED/deep/x.csv"] {
            assert!(ServedPath::from_source(path).is_err(), "{path}");
        }
        assert!(ServedPath::from_source("_gated.csv").is_ok());
        assert!(ServedPath::from_source("notes/gated/x.csv").is_ok());
    }
}
