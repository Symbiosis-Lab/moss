//! The visit counter's one script file, `_moss/js/beacon.js`.
//!
//! Rendered pages load it with a tag from the features pass, and hand-written
//! pages include the same tag. Only a moss-hosted site with a site id gets it.

use std::path::Path;

use crate::build::context::BuildContext;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::served_path::ServedPath;
use crate::config::deployment::{DomainDeploymentConfig, MOSS_TARGET_ID};

/// The script body. `ENDPOINT` is replaced by the JSON-encoded endpoint URL.
/// The hosting server decides which events count, so this carries no
/// hostname checks of its own.
const BEACON_TEMPLATE: &str = r#"(function(){var b=ENDPOINT;var s=function(d){fetch(b,{method:'POST',body:JSON.stringify(d),keepalive:true}).catch(function(){})};var v=function(){s({url:location.href,referrer:document.referrer||null,screen_width:innerWidth})};v();addEventListener('pageshow',function(e){if(e.persisted)v()});document.addEventListener('click',function(e){var t=e.target.closest&&e.target.closest('[data-track]');if(t)s({url:location.href,event:t.getAttribute('data-track')})})})();"#;

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
    let body = BEACON_TEMPLATE.replacen("ENDPOINT", &endpoint_js, 1);

    let sp = ServedPath::for_runtime_js("beacon").expect("'beacon' is a valid JS name");
    BuildContext::for_render(output_dir, pending)
        .emit(&sp, body.as_bytes(), HashBucket::Files)
        .map_err(|e| format!("Failed to emit {}: {e}", sp.as_str()))
}

#[cfg(test)]
#[path = "beacon_tests.rs"]
mod tests;
