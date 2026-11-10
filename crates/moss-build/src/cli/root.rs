//! Read-only lookup of the moss site that owns a path, and the address it is live at.

use std::path::Path;

/// `moss root <path> --json` prints `{"root", "address"}`: the canonical owning
/// site (null when none) and its live address without the scheme (null before a
/// first publish; left out when it cannot be read).
pub fn run(args: &[String]) -> i32 {
    let mut path = None;
    let mut json = false;
    for arg in args {
        if arg == "--json" && !json {
            json = true;
        } else if arg.starts_with('-') || path.is_some() {
            eprintln!("Usage: moss root <path> --json");
            return 2;
        } else {
            path = Some(arg);
        }
    }
    let Some(path) = path.filter(|_| json) else {
        eprintln!("Usage: moss root <path> --json");
        return 2;
    };

    let root = crate::vault_root::VaultRoot::find_containing(Path::new(path));
    let mut out = serde_json::json!({"root": root.as_ref().map(|root| root.as_str())});
    // An address that cannot be read is left out, which clients read as unknown;
    // null would claim the site was never published.
    match root.as_ref().map(|root| live_address(root.as_str())) {
        Some(Err(e)) => eprintln!("moss root: address unknown: {e}"),
        Some(Ok(address)) => out["address"] = serde_json::json!(address),
        None => out["address"] = serde_json::Value::Null,
    }
    println!("{out}");
    0
}

/// The address a published site is live at, as the publish success line names it;
/// for a host other than moss's own, the URL its last publish reported.
fn live_address(root: &str) -> Result<Option<String>, String> {
    use crate::build::site_config;
    // Never wait on a download: a file still in the cloud is an error here.
    let config = site_config::domain_config_read_with(root, site_config::read_managed_toml_no_wait)?;
    // The active target's record: a site id is chosen before the first publish
    // succeeds, and survives a switch to another host.
    let Some(published) = config.last_deployment_url.clone() else {
        return Ok(None);
    };
    let url = match (config.deploy_method.as_deref(), config.site_id.as_deref()) {
        (Some(crate::config::deployment::MOSS_TARGET_ID), Some(site_id)) => {
            crate::deploy::view_site_url_for(&config, site_id, site_config::resolve_environment(root))
        }
        _ => published,
    };
    let url = crate::build::site_url::SiteUrl::parse(&url).map_err(|e| e.to_string())?;
    Ok(Some(url.host().to_owned()))
}

#[cfg(test)]
mod tests {
    use super::live_address;
    use crate::build::icloud;
    use std::time::{Duration, Instant};

    #[test]
    fn a_state_file_still_in_the_cloud_makes_the_address_unknown_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let moss = dir.path().join(".moss");
        std::fs::create_dir_all(&moss).unwrap();
        std::fs::write(
            moss.join("state.toml"),
            "schema_version = 1\n\n[deployment]\nsite_id = \"her-blog\"\n\n\
             [deployment.targets.\"moss:her-blog\"]\nlast_deployment_url = \"https://her-blog.mosspub.com\"\n",
        )
        .unwrap();
        let _in_cloud = icloud::pretend::evicted(&moss.join("state.toml"));
        let started = Instant::now();
        assert!(live_address(dir.path().to_str().unwrap()).is_err());
        // An editor gives moss root a few seconds; waiting on the download would outlast them.
        assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
    }
}
