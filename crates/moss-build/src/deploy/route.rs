//! Which of the four publish routes a folder takes — one answer, one owner.
//!
//! `moss deploy` can mean four different things depending on what is in the
//! folder: upload a directory some other tool built, build the site and publish
//! it to moss's own hosting, hand the whole job to a deploy plugin, or write
//! the files into a destination moss reaches itself.
//!
//! The route is a value, so no caller works it out for itself and two callers
//! cannot disagree about where a folder with both a `site_id` and a pinned
//! `[hooks] deploy` publishes. [`DeployRoute::resolve`] answers it from the
//! folder alone and both binaries read the same answer, through
//! `moss_build::cli::deploy`, to pick a driver.
//!
//! No route needs a window: the app's `startup::headless::intercept` diverges
//! on `RunMode::Deploy` unconditionally and never reaches `tauri::Builder` for
//! a publish.
//!
//! What is deliberately NOT a variant: "this folder has no site yet".
//! Registration is not a route, it is the first step of one — the drivers
//! answer it through [`crate::deploy::resolve_publish_inputs`], and hoisting it
//! here would put the same decision back in two places.

use std::path::{Path, PathBuf};

/// Where a publish of this folder goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeployRoute {
    /// A directory built by another tool (Quire, Hugo, Jekyll, Astro), named by
    /// `--prebuilt=<dir>` or by `[deployment].prebuilt_output`. Uploaded as-is;
    /// moss's own build never runs.
    Prebuilt {
        /// Already resolved, because both callers need the path and neither
        /// should re-derive it.
        dir: PathBuf,
    },
    /// moss builds the site and publishes what it sealed, to moss hosting.
    ///
    /// Carries nothing: the `site_id` is read by
    /// [`crate::deploy::resolve_publish_inputs`] on the way in, and a copy here
    /// would be a second answer to a question that already has an owner.
    Hosted,
    /// A deploy plugin owns this folder's publish
    /// ([`crate::deploy::plugin_push`]).
    ///
    /// It needed the app until P2b, and what needed it was the driver rather
    /// than the runtime: `ManagerCache` has had a headless constructor for a
    /// while now. A plugin that reaches for something only a window has — the
    /// github plugin's WebKit cookie jar — is refused by `need_app` in
    /// `engine/host_fns.rs`, which names the command it refused, rather than
    /// by this route pre-emptively refusing every plugin.
    Plugin {
        /// The `[hooks] deploy` selection, which is what the dispatcher routes on.
        id: String,
    },
    /// A target moss writes to itself, file by file ([`crate::deploy::direct`]),
    /// named by [`crate::config::deployment::is_direct_target`].
    Direct {
        id: String,
    },
}

impl DeployRoute {
    /// Read the folder's config and say where a publish would go.
    ///
    /// Prebuilt wins outright, and before the plugin check, because a prebuilt
    /// folder that fell through to a plugin would silently take a route that
    /// cannot publish it. A built-in direct target comes next, before the
    /// plugin check, so its name never reaches the plugin runtime. Otherwise
    /// the selector is [`crate::build::site_config::current_deploy_plugin`],
    /// the same one the Publish button asks, which is what makes the two
    /// agree. That function still answers a direct target's name as a plugin
    /// id, because the app reads its `None` as moss hosting; the app asks the
    /// built-in list itself once it can publish to one.
    pub fn resolve(folder: &Path, prebuilt_arg: Option<&str>) -> Self {
        let folder_str = folder.to_string_lossy().to_string();
        let config = crate::build::site_config::get_domain_config(&folder_str).unwrap_or_default();

        if let Some(dir) = crate::deploy::prebuilt::resolve_prebuilt_dir(folder, &config, prebuilt_arg)
        {
            return DeployRoute::Prebuilt { dir };
        }

        if let Some(id) = config.deploy_method.as_deref().filter(|m| crate::config::deployment::is_direct_target(m)) {
            return DeployRoute::Direct { id: id.to_string() };
        }

        match crate::build::site_config::current_deploy_plugin(&folder_str) {
            Some(id) => DeployRoute::Plugin { id },
            None => DeployRoute::Hosted,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DeployRoute;

    fn vault(body: &str, state: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let moss = dir.path().join(".moss");
        std::fs::create_dir_all(&moss).expect("mkdir .moss");
        std::fs::write(moss.join("config.toml"), body).expect("config.toml");
        std::fs::write(moss.join("state.toml"), state).expect("state.toml");
        dir
    }

    /// The drift this model exists to end. A folder with BOTH a registered
    /// `site_id` and a pinned `[hooks] deploy` published to seta from
    /// `moss deploy` and to the plugin from the Publish button, because the two
    /// callers routed on different fields. One answer now, and it is the
    /// button's: a pinned hook is a deliberate selection, a `site_id` is a
    /// residue of having published once.
    #[test]
    fn a_pinned_deploy_hook_wins_over_a_registered_site_id() {
        let dir = vault("[hooks]\ndeploy = \"onionpress\"\n", "[deployment]\nsite_id = \"blog\"\n");
        assert_eq!(
            DeployRoute::resolve(dir.path(), None),
            DeployRoute::Plugin { id: "onionpress".to_string() }
        );
    }

    #[test]
    fn a_registered_site_with_no_hook_is_hosted() {
        let dir = vault("", "[deployment]\nsite_id = \"blog\"\n");
        assert_eq!(DeployRoute::resolve(dir.path(), None), DeployRoute::Hosted);
    }

    /// A folder that has never published is Hosted, not Plugin: the first
    /// publish opens moss's own registration rather than a silent default.
    #[test]
    fn an_unconfigured_folder_is_hosted() {
        let dir = vault("", "");
        assert_eq!(DeployRoute::resolve(dir.path(), None), DeployRoute::Hosted);
    }

    #[test]
    fn your_own_server_is_a_direct_route_not_a_plugin() {
        let dir = vault("[hooks]\ndeploy = \"server\"\n", "[deployment]\nsite_id = \"blog\"\n");
        assert_eq!(
            DeployRoute::resolve(dir.path(), None),
            DeployRoute::Direct { id: "server".to_string() }
        );
    }

    /// Prebuilt is decided before the plugin check, so `--prebuilt` on a
    /// plugin-configured folder still uploads the directory rather than
    /// diverting into a runtime that would not know what to do with it.
    #[test]
    fn prebuilt_wins_over_every_other_selector() {
        let dir = vault("[hooks]\ndeploy = \"onionpress\"\n", "[deployment]\nsite_id = \"blog\"\n");
        let out = dir.path().join("_site");
        std::fs::create_dir_all(&out).expect("mkdir _site");
        assert_eq!(
            DeployRoute::resolve(dir.path(), Some("_site")),
            DeployRoute::Prebuilt { dir: out }
        );
    }
}
