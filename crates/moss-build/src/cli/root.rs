//! Read-only lookup of the moss site that owns a path, and the address it is live at.

use std::path::Path;

/// `moss root <path> --json` prints `{"root", "address"}`: the canonical owning
/// site (null when none) and its live address without the scheme (null before a
/// first publish; left out when it cannot be read). For a page the last build
/// made, `page` adds what an editor shows beside it
/// ([`crate::build::emit::inventory::PageStatus`]).
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

    let (out, unknown) = answer(Path::new(path));
    for line in unknown {
        eprintln!("moss root: {line}");
    }
    println!("{out}");
    0
}

/// What `moss root` prints for `path`, and a line for each fact it could not read.
pub(crate) fn answer(path: &Path) -> (serde_json::Value, Vec<String>) {
    let mut unknown = Vec::new();
    let root = crate::vault_root::VaultRoot::find_containing(path);
    let mut out = serde_json::json!({"root": root.as_ref().map(|root| root.as_str())});
    let Some(root) = root else {
        out["address"] = serde_json::Value::Null;
        return (out, unknown);
    };
    // Never wait on a download: a file still in the cloud is an error here.
    use crate::build::site_config;
    let read = site_config::domain_config_read_with(root.as_str(), site_config::read_managed_toml_no_wait);
    let config = read.as_ref().map_err(String::as_str);
    // An address that cannot be read is left out, which clients read as unknown;
    // null would claim the site was never published.
    match config.map_err(str::to_owned).and_then(|config| live_address(config, root.as_str())) {
        Err(e) => unknown.push(format!("address unknown: {e}")),
        Ok(address) => out["address"] = serde_json::json!(address),
    }
    // Resolved as the root was, so a path through `..` still lies under it.
    let file = crate::vault_root::resolve_input(path);
    let rel = file.strip_prefix(root.path()).ok().and_then(Path::to_str);
    let status = rel.and_then(|rel| crate::build::emit::inventory::page_status_for_source(root.path(), rel, config));
    if let Some((page, lines)) = status {
        unknown.extend(lines);
        out["page"] = serde_json::json!(page);
    }
    (out, unknown)
}

/// The address a published site is live at, as the publish success line names it;
/// for a host other than moss's own, the URL its last publish reported.
fn live_address(config: &crate::config::deployment::DomainDeploymentConfig, root: &str) -> Result<Option<String>, String> {
    use crate::build::site_config;
    // The active target's record: a site id is chosen before the first publish
    // succeeds, and survives a switch to another host.
    let Some(published) = config.last_deployment_url.clone() else {
        return Ok(None);
    };
    let url = match (config.deploy_method.as_deref(), config.site_id.as_deref()) {
        (Some(crate::config::deployment::MOSS_TARGET_ID), Some(site_id)) => {
            crate::deploy::view_site_url_for(config, site_id, site_config::resolve_environment(root))
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
        let root = dir.path().to_str().unwrap();
        let read = crate::build::site_config::domain_config_read_with(root, crate::build::site_config::read_managed_toml_no_wait);
        assert!(read.and_then(|config| live_address(&config, root)).is_err());
        // An editor gives moss root a few seconds; waiting on the download would outlast them.
        assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
    }

    #[test]
    fn settings_still_in_the_cloud_leave_a_page_published_state_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let moss = dir.path().join(".moss");
        std::fs::create_dir_all(&moss).unwrap();
        std::fs::write(moss.join("state.toml"), "schema_version = 1\n\n[deployment]\nsite_id = \"her-blog\"\n").unwrap();
        std::fs::write(dir.path().join("heron.md"), "# Heron\n").unwrap();
        let row = serde_json::json!([{"source_path": "heron.md", "url_path": "heron/index.html", "title": "", "date": null,
            "kind": "article", "draft": false, "listed": true, "nav": null, "slot_only": false, "hidden": []}]);
        let inventory = crate::build::emit::inventory::inventory_path(&moss);
        std::fs::create_dir_all(inventory.parent().unwrap()).unwrap();
        std::fs::write(inventory, row.to_string()).unwrap();
        let _in_cloud = icloud::pretend::evicted(&moss.join("state.toml"));
        let (out, unknown) = super::answer(&dir.path().join("heron.md"));
        assert_eq!(out["page"], serde_json::json!({"path": "/heron/", "draft": false, "channels": [], "copies": {}}));
        assert!(unknown.iter().any(|line| line.starts_with("publish state unknown")), "{unknown:?}");
    }

    #[test]
    fn a_page_named_through_a_parent_step_is_still_the_page() {
        let dir = tempfile::tempdir().unwrap();
        let site = dir.path().join("her-blog");
        std::fs::create_dir_all(site.join(".moss")).unwrap();
        std::fs::write(site.join("heron.md"), "# Heron\n").unwrap();
        let row = serde_json::json!([{"source_path": "heron.md", "url_path": "heron/index.html", "title": "", "date": null,
            "kind": "article", "draft": false, "listed": true, "nav": null, "slot_only": false, "hidden": []}]);
        let inventory = crate::build::emit::inventory::inventory_path(&site.join(".moss"));
        std::fs::create_dir_all(inventory.parent().unwrap()).unwrap();
        std::fs::write(inventory, row.to_string()).unwrap();
        let (out, unknown) = super::answer(&site.join("../her-blog/heron.md"));
        assert_eq!(out["page"]["path"], "/heron/", "{out} {unknown:?}");
    }
}
