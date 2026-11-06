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
///
/// Each event carries a random id, new per event and kept only in memory, so
/// the server counts a retry once; a browser that cannot make one sends the
/// event once, never retried. A 429, a 503 or a network failure is
/// retried at most twice, after the server's Retry-After (capped at 30 s) or
/// 2 s then 4 s; at most ten retries wait at once, and the rest are dropped.
/// The body stays a CORS-simple request: no headers, so no preflight.
const BEACON_TEMPLATE: &str = r#"(function(){var b=ENDPOINT,w=0;var u=function(){return location.origin+location.pathname+location.search};var r=function(){try{var a=crypto.getRandomValues(new Uint8Array(8)),h='';for(var i=0;i<8;i++)h+=(a[i]|256).toString(16).slice(1);return h}catch(_){return null}};var s=function(d,n){var again=function(sec){if(n<2&&w<10){w++;setTimeout(function(){w--;s(d,n+1)},Math.min(sec||2<<n,30)*1e3)}};fetch(b,{method:'POST',body:JSON.stringify(d),keepalive:true}).then(function(x){if(x.status==429||x.status==503)again(+x.headers.get('Retry-After'))},function(){again(0)})};var e=function(d){d.id=r();s(d,d.id?0:2)};var v=function(){e({url:u(),referrer:document.referrer||null,screen_width:innerWidth})};v();addEventListener('pageshow',function(x){if(x.persisted)v()});document.addEventListener('click',function(x){var t=x.target.closest&&x.target.closest('[data-track]');if(t)e({url:u(),event:t.getAttribute('data-track')})})})();"#;

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
