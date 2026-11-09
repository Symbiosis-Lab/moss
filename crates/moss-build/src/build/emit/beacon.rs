//! The visit counter's one script file, `_moss/js/beacon.js`.
//!
//! Rendered pages load it with a tag from the features pass, and hand-written
//! pages include the same tag. Only a moss-hosted site with a site id gets it.

use std::path::Path;

use crate::build::context::BuildContext;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::served_path::ServedPath;
use crate::config::deployment::{DomainDeploymentConfig, MOSS_TARGET_ID};

/// The script, `assets/js/beacon.js`. The build replaces `__ENDPOINT__` with the
/// JSON-encoded endpoint URL; the script names it once.
const BEACON_SOURCE: &str = include_str!("../../assets/js/beacon.js");

/// Whether this site gets the visit counter: moss-hosted, with a site id. The
/// tag and the file both gate on this, so a tag never names a file that was not written.
pub fn is_enabled(deploy: &DomainDeploymentConfig) -> bool {
    deploy.deploy_method.as_deref() == Some(MOSS_TARGET_ID) && deploy.site_id.is_some()
}

/// Write `_moss/js/beacon.js` under `output_dir` when this site is moss-hosted.
pub fn emit(
    deploy: &DomainDeploymentConfig,
    project_path: &str,
    output_dir: &Path,
    pending: &mut PendingManifest,
) -> Result<(), String> {
    if !is_enabled(deploy) {
        return Ok(());
    }
    let site_id = deploy.site_id.as_deref().unwrap_or_default();
    let host_url = crate::build::site_config::resolve_environment(project_path).seta_url();
    let endpoint = format!("{host_url}/api/sites/{site_id}/beacon");
    let endpoint_js = serde_json::to_string(&endpoint).map_err(|e| e.to_string())?;
    let body = BEACON_SOURCE.replacen("__ENDPOINT__", &endpoint_js, 1);

    let sp = ServedPath::for_runtime_js("beacon").expect("'beacon' is a valid JS name");
    BuildContext::for_render(output_dir, pending)
        .emit(&sp, body.as_bytes(), HashBucket::Files)
        .map_err(|e| format!("Failed to emit {}: {e}", sp.as_str()))
}

#[cfg(test)]
#[path = "beacon_tests.rs"]
mod tests;
