//! Who may subscribe (`open` or `invite`), read by the hosting service.

use std::path::Path;
use crate::build::{context::BuildContext, manifest::{HashBucket, PendingManifest}, served_path::ServedPath};
use crate::config::services::MembersConfig;

pub const MEMBERS_PATH: &str = "_moss/members.json";

pub fn emit(config: &MembersConfig, output: &Path, pending: &mut PendingManifest) -> Result<(), String> {
    let bytes = serde_json::to_vec(config).map_err(|e| format!("Cannot serialize membership policy: {e}"))?;
    BuildContext::for_render(output, pending)
        .emit_held(&ServedPath::for_members_json(), bytes, HashBucket::Files)
        .map_err(|e| format!("Failed to emit {MEMBERS_PATH}: {e}"))
}
