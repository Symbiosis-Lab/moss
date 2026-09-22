//! The build's config readers — `.moss/config.toml`, `.moss/state.toml`, and
//! the one app-level flag the pipeline consults.
//!
//! Moved from `domain/config.rs` (2026-08-27, M6a B3 / ADR-059): the reader
//! family crosses into the build tree so it travels into `crates/moss-build`
//! with the pipeline; the modal-driven writers stay app-side in
//! `domain/config.rs`, which re-exports these readers so existing paths keep
//! working. `ManagedToml` and the one write primitive joined this crate on
//! 2026-09-07 (`vault::config`, the ADR-059 amendment) so `moss env` writes in
//! both binaries. The eviction-aware read lives here because the
//! cloud-readiness machinery is already in-cluster.

use crate::config::deployment::DomainDeploymentConfig;
use crate::config::environment::{parse_env_name, HostingEnvironment};
use crate::config::services::ServicesConfig;
use crate::config::ConfigFile;
use std::path::Path;
use toml::Value;

/// Reads a moss-managed TOML file, returning `None` only when it is genuinely
/// absent.
///
/// This replaces the `path.exists()` pre-check these functions used to open
/// with. `exists()` is a stat, and a stat succeeds for a cloud-evicted file
/// whose bytes are not here — while on macOS 12-13 it does the opposite, since
/// an evicted file's real path returns ENOENT and the data hides in a
/// `.name.icloud` sibling. Both readings are wrong, in opposite directions.
/// Absence has to be proven from the error, which is what
/// `is_definitely_absent` does.
pub fn read_managed_toml(path: &Path) -> Result<Option<String>, String> {
    match crate::build::cloud_readiness::read_to_string_with_materialize_wait(
        path,
        crate::build::cloud_readiness::INTERACTIVE_DEADLINE,
    ) {
        Ok(content) => Ok(Some(content)),
        Err(e) if crate::build::icloud::is_definitely_absent(path, &e) => Ok(None),
        Err(e) => Err(format!("Failed to read {}: {}", path.display(), e)),
    }
}

/// Read and parse `.moss/config.toml`, or an empty config when the file is
/// genuinely absent.
///
/// **This is where the two halves of the reader meet.** The read is
/// eviction-aware — [`read_managed_toml`] proves absence from the error rather
/// than from a stat, so an iCloud-evicted config never reads as "no settings".
/// The parsing and every lookup are [`crate::config::ConfigFile`]'s, in
/// the open crate, so a `moss-cli` that reads the same file with
/// `std::fs::read_to_string` interprets it identically (ADR-059).
///
/// Parsed once per call, so a caller that wants several keys should hold the
/// result rather than ask again — asking again is a second read of a file that
/// may have changed in between.
pub fn read_project_config(project_path: &str) -> Result<ConfigFile, String> {
    let config_path = Path::new(project_path).join(".moss").join("config.toml");
    match read_managed_toml(&config_path)? {
        Some(text) => ConfigFile::parse(&text),
        None => Ok(ConfigFile::empty()),
    }
}

/// `Some(v)` when `.moss/config.toml` declares a schema newer than this
/// build supports; `Ok(None)` covers absent, at, or behind current.
///
/// One read: [`read_project_config`]'s `ConfigFile::parse` swallows
/// `VersionAhead` and falls back to the raw, unmigrated table, but the
/// original `schema_version` survives into that table untouched (the
/// migration short-circuits before mutating anything), so
/// [`ConfigFile::schema_version_ahead`] recovers the real answer from the
/// SAME parse rather than this function re-reading and re-parsing the file.
/// A caller that already holds a `ConfigFile` should call the method
/// directly instead of this free function.
pub fn config_schema_version_ahead(project_path: &str) -> Result<Option<u32>, String> {
    Ok(read_project_config(project_path)?.schema_version_ahead())
}

/// The shared refusal every outward door (publish, syndicate, email send)
/// goes through before acting on a site's config: `get_site_*` /
/// `get_services_config` read through `ConfigFile::parse`, which silently
/// defaults every setting it doesn't recognize the shape of on
/// `VersionAhead` — fine for preview (which says so via the advisory in
/// `build/progress.rs`), not fine for a door that reaches a real audience
/// (subscribers, syndication platforms, the live site). One function so
/// there is one place these doors agree on, rather than three copies of the
/// same check drifting.
///
/// A config READ failure (evicted, half-synced, permissions) is deliberately
/// not a refusal here — only a *confirmed* `VersionAhead` is. These doors are
/// not the config's authority; something upstream (the build this door
/// follows, or the config's own readers) already owns surfacing a genuine
/// read error, and refusing a publish or a send on a transient I/O hiccup
/// this check merely stumbled over would be a new, unrelated failure mode.
pub fn ensure_config_current(project_path: &str) -> Result<(), String> {
    if let Ok(Some(found)) = config_schema_version_ahead(project_path) {
        return Err(format!(
            "This site's .moss/config.toml is schema_version {found}, newer than this app \
             supports (up to {}). Acting now would use every unrecognized setting at its \
             default. Update moss first.",
            crate::config::migrations::CURRENT_VERSION,
        ));
    }
    Ok(())
}

/// Read `[services.<kind>]` sections from .moss/config.toml. Permissive —
/// unknown providers are preserved as-is in the `provider` field; wiring
/// errors are not checked here. Call `validate()` on the returned config
/// from the site-load boundary (once per folder open) to surface load errors.
///
/// Consumers that read services config on hot paths (e.g. per-page render)
/// should use this permissive reader so a misconfigured section doesn't
/// convert into a render-time panic.
pub fn get_services_config(project_path: &str) -> Result<ServicesConfig, String> {
    let config = read_project_config(project_path)?;
    match config.section(&["services"]) {
        Some(v) => v
            .clone()
            .try_into()
            .map_err(|e| format!("Failed to parse [services]: {}", e)),
        None => Ok(ServicesConfig::default()),
    }
}

/// Read `[site].rss_footer`. None = not set (caller uses its own default).
pub fn get_site_rss_footer(project_path: &str) -> Result<Option<bool>, String> {
    get_site_bool_field(project_path, "rss_footer")
}

/// Read `[site].search` — full-text search index + nav entrance.
/// None = not set (caller uses its own default, which is off).
pub fn get_site_search(project_path: &str) -> Result<Option<bool>, String> {
    get_site_bool_field(project_path, "search")
}

/// Read `[site].floating_nav` — the floating nav island (ADR-049).
/// None = not set (caller uses its own default, which is ON).
pub fn get_site_floating_nav(project_path: &str) -> Result<Option<bool>, String> {
    get_site_bool_field(project_path, "floating_nav")
}

/// Get the flat deployment view for a project.
///
/// Composed from two files, matching who owns what: `.moss/state.toml
/// [deployment]` holds moss's own records (project identity + one
/// [`crate::config::deployment::DeploymentRecord`] per target ever
/// published), while the selector — which target is in effect NOW — derives
/// from the user's `[hooks] deploy` in config.toml plus `site_id`
/// ([`crate::config::deployment::derive_deploy_method`]). The returned
/// view carries the ACTIVE target's record; the other targets' records stay
/// on disk untouched, which is what lets a user switch hosts freely.
pub fn get_domain_config(project_path: &str) -> Result<DomainDeploymentConfig, String> {
    let state = read_deployment_state(project_path)?;
    // ONE parse of config.toml for both authored inputs: the target selector
    // (`[hooks] deploy`) and the domain intent (`[site].domain`). The raw
    // key, deliberately NOT `plugins::discovery::get_hook_config`, which
    // injects the build-time `DEFAULT_DEPLOYER` fallback: an unconfigured
    // folder must read as "no selection" so the first publish opens the
    // choice, not a silent default.
    let config = read_project_config(project_path)?;
    let hooks_deploy = config
        .section(&["hooks", "deploy"])
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let domain = config.site_str("domain").map(str::to_string);
    let method = crate::config::deployment::derive_deploy_method(
        hooks_deploy.as_deref(),
        state.site_id.as_deref(),
    );
    Ok(state.flat_view(method, domain))
}

/// The deploy plugin Publish will dispatch to, or `None` for moss's own
/// hosting. Every "where is this site about to go" question — the Host row, the
/// setup gate, the dispatcher, the GitHub-Pages subpath guard, the video cap —
/// asks this one, so none of them can answer differently from the others.
///
/// It is the derived deploy method with moss's own name filtered out, and that
/// is the whole of it. The method is non-moss only when the user pinned
/// `[hooks] deploy`, and a pinned hook is exactly what
/// `plugins::discovery::resolve_deploy_plugin` returns before it ever looks at
/// the plugin list — so consulting discovery here could only ever hand back the
/// value already in hand, at the price of a directory walk and a manifest parse
/// on a path the serving watch hits every tick. No `deploy_method` at all means
/// moss too: a vault that has never published opens the moss wizard even with a
/// deploy plugin installed, matching `hasDeployConfig == false →
/// FirstPublishModal`.
///
/// Lives beside [`get_domain_config`] because it is a pure function of it —
/// app-side and build-side callers then share one answer instead of the four
/// divergent resolvers this replaced.
pub fn current_deploy_plugin(project_path: &str) -> Option<String> {
    get_domain_config(project_path)
        .unwrap_or_default()
        .deploy_method
        .filter(|m| m != crate::config::deployment::MOSS_TARGET_ID)
}

/// Parse `.moss/state.toml [deployment]` into the persisted shape, lifting a
/// legacy flat block into `targets[<slot>]` in memory (persisted on the next
/// save by `save_domain_config`, which does the same lift on its own read).
pub fn read_deployment_state(
    project_path: &str,
) -> Result<crate::config::deployment::DeploymentState, String> {
    let state_path = Path::new(project_path).join(".moss").join("state.toml");
    let Some(content) = read_managed_toml(&state_path)? else {
        return Ok(Default::default());
    };
    let state: Value = toml::from_str(&content)
        .map_err(|e| format!("Failed to parse state.toml: {}", e))?;
    crate::config::deployment::DeploymentState::from_toml(state.get("deployment"))
}

/// Read `site_id` from `.moss/state.toml [deployment]`.
///
/// The "have I deployed this folder yet?" question, which the domain verb, the
/// email commands and the newsletter sender each ask before they can name a
/// site. It sits beside the reader it wraps rather than in any one caller.
pub fn site_id_for_folder(folder: &Path) -> Result<String, String> {
    let config = get_domain_config(&folder.to_string_lossy())
        .map_err(|e| format!("failed to read .moss/state.toml: {}", e))?;
    config
        .site_id
        .ok_or_else(|| "no site_id — deploy this folder first (moss deploy <folder>)".to_string())
}

/// Resolve the hosting environment for a project.
///
/// Precedence (highest first):
/// 1. `MOSS_ENV` env var — a *named* environment (also selects the matching Stripe key).
/// 2. `MOSS_SETA_URL` raw-URL override — localhost/127.0.0.1 URLs resolve to `Local`
///    (badge shows LOCAL, test Stripe/Matters keys apply); any other URL resolves to
///    `Production` (the URL itself is used directly by `get_seta_url()` / the env
///    constructors regardless of this label).
/// 3. `.moss/config.toml` top-level `environment` field.
/// 4. Default: Production.
pub fn resolve_environment(project_path: &str) -> HostingEnvironment {
    if let Ok(name) = std::env::var("MOSS_ENV") {
        if let Some(env) = parse_env_name(name.trim()) {
            return env;
        }
        log::warn!("MOSS_ENV='{}' is not one of staging|local|production; ignoring", name);
    }
    if let Ok(url) = std::env::var("MOSS_SETA_URL") {
        // A raw URL carries no environment name. A localhost URL is the local-dev
        // backend, so label it Local; a staging URL is Staging; any other URL is
        // Production (the client uses the raw URL directly via get_seta_url()/
        // for_environment()). String-contains matches the localhost style above.
        let is_localhost = url.contains("localhost") || url.contains("127.0.0.1");
        return if is_localhost {
            HostingEnvironment::Local
        } else if url.contains("staging.mosspub.com") {
            HostingEnvironment::Staging
        } else {
            HostingEnvironment::Production
        };
    }
    if let Ok(Some(name)) = get_environment_field(project_path) {
        if let Some(env) = parse_env_name(name.trim()) {
            return env;
        }
    }
    HostingEnvironment::Production
}

/// Read a string field from `.moss/config.toml` under the `[site]` section.
fn get_site_field(project_path: &str, field: &str) -> Result<Option<String>, String> {
    Ok(read_project_config(project_path)?
        .site_str(field)
        .map(|s| s.to_string()))
}

/// Get the persisted site language from .moss/config.toml [site] section
pub fn get_site_lang(project_path: &str) -> Result<Option<String>, String> {
    get_site_field(project_path, "lang")
}

/// Read `[editor].attachment_folder` — where dropped/pasted images land.
/// None = not set (caller uses the default: next to the page). Semantics and
/// validation live in [`moss_core::attachment`].
pub fn get_editor_attachment_folder(project_path: &str) -> Result<Option<String>, String> {
    Ok(read_project_config(project_path)?
        .section(&["editor", "attachment_folder"])
        .and_then(|v| v.as_str())
        .map(|s| s.to_string()))
}

/// Read the raw `[editor].attachment_folder` value ("" when unset). Invalid
/// values (absolute / `..`) surface a config diagnostic in the log and fall
/// back to the default, so a hand-edited config degrades to current behavior
/// instead of scattering images somewhere surprising.
///
/// Two callers on opposite sides of the app read this: the editor, to place an
/// image it is about to write, and the scan, to decide which directories are
/// storage rather than sections of the site. Both get the same string, so a
/// user who changes the setting moves both answers at once.
pub fn load_attachment_folder(project_path: &str) -> String {
    let raw = get_editor_attachment_folder(project_path)
        .ok()
        .flatten()
        .unwrap_or_default();
    match moss_core::attachment::validate_attachment_folder(&raw) {
        Ok(()) => raw,
        Err(reason) => {
            log::warn!(
                "config.toml [editor].attachment_folder = {raw:?} is invalid ({reason}); \
                 falling back to the default (next to the page)"
            );
            String::new()
        }
    }
}

/// Read the top-level `environment` key from `.moss/config.toml`.
/// Returns `None` if the file or key is absent. Top-level (not under `[site]`)
/// so it never collides with `state.toml`'s `[deployment]` section.
pub fn get_environment_field(project_path: &str) -> Result<Option<String>, String> {
    Ok(read_project_config(project_path)?
        .environment()
        .map(|s| s.to_string()))
}

/// Read a boolean field from `.moss/config.toml` under the `[site]` section.
fn get_site_bool_field(project_path: &str, field: &str) -> Result<Option<bool>, String> {
    Ok(read_project_config(project_path)?.site_bool(field))
}

/// Get the site-wide LaTeX-math preference from `.moss/config.toml`
/// `[site]` section (ADR-030). Returns `None` when the key is absent —
/// which is every site today, since nothing writes it. The caller treats
/// absence as `true`: math is on by default, and authors whose prose
/// makes `$` pair up opt out with `[site].math = false`.
pub fn get_site_math(project_path: &str) -> Result<Option<bool>, String> {
    get_site_bool_field(project_path, "math")
}

/// `[site].hard_line_breaks` — Obsidian-parity soft-break rendering.
pub fn get_site_hard_line_breaks(project_path: &str) -> Result<Option<bool>, String> {
    get_site_bool_field(project_path, "hard_line_breaks")
}

/// Read `[build].passthrough` from `.moss/config.toml`.
///
/// Returns an ordered list of path strings. Entries without a prefix are
/// explicit passthrough roots; entries prefixed with `!` opt a directory
/// OUT of auto-detection. Returns an empty vec when the key or file is absent.
pub fn get_build_passthrough(project_path: &str) -> Result<Vec<String>, String> {
    Ok(read_project_config(project_path)?.build_passthrough())
}

/// Read `[build].keep_generations` from `.moss/config.toml`.
///
/// How many generation directories under `.moss/build.nosync/generations/` survive
/// retention. `None` when the key is absent or not an integer; the caller
/// applies `store_gc::KEEP_GENERATIONS_DEFAULT` and the floor.
///
/// Lives here, beside [`get_build_passthrough`], because `config.toml` has one
/// reader by charter (NORTH-STAR ownership map: `vault/config.rs` is THE
/// config.toml reader, of which this module is the current home). `store_gc`
/// itself stays free of config access so it can move into `crates/moss-build`
/// unchanged.
///
/// A malformed value must never fail a build, so every error path folds to
/// `None` rather than propagating — an unreadable knob means "use the default",
/// not "stop".
pub fn get_build_keep_generations(project_path: &str) -> Option<usize> {
    read_project_config(project_path).ok()?.build_keep_generations()
}

/// Read `[build].prune_orphaned_images` from `.moss/config.toml` — whether
/// ship-time orphan pruning runs.
///
/// `None` when unset; the caller supplies the default (on). This is an OFF
/// SWITCH, not a feature flag: pruning decides "referenced" by scanning
/// emitted text, so a reference moss cannot see in the output — the live case
/// is a plugin building a src by concatenation, where the literal path never
/// appears in the JS — reads as unreferenced and the image is deleted from a
/// published site, silently. Until the reference set comes from the emitter
/// rather than a regex (moss#976 B2 follow-up), a user hitting that has no
/// other recourse and support has nothing to suggest.
pub fn get_build_prune_orphaned_images(project_path: &str) -> Option<bool> {
    read_project_config(project_path)
        .ok()?
        .build_prune_orphaned_images()
}

/// Read `[history].enabled` from `.moss/config.toml`. `None` when the key or
/// file is absent or unreadable; `deploy::history` applies the default (on).
pub fn get_history_enabled(project_path: &str) -> Option<bool> {
    read_project_config(project_path).ok()?.history_enabled()
}

#[cfg(test)]
mod attachment_folder_tests {
    use super::{load_attachment_folder, site_id_for_folder};

    // Placement and validation are pinned by the shared golden vectors in
    // `moss_core::attachment`. What is only testable here is the config I/O
    // around them: an unreadable or rejected value must degrade to the
    // default rather than propagate.

    fn project_with_config(body: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let moss = dir.path().join(".moss");
        std::fs::create_dir_all(&moss).unwrap();
        std::fs::write(moss.join("config.toml"), body).unwrap();
        dir
    }

    #[test]
    fn invalid_config_value_falls_back_to_default_at_load() {
        let dir = project_with_config("[editor]\nattachment_folder = \"../escape\"\n");
        assert_eq!(load_attachment_folder(&dir.path().to_string_lossy()), "");
    }

    #[test]
    fn configured_value_round_trips_through_load() {
        let dir = project_with_config("[editor]\nattachment_folder = \"./assets\"\n");
        assert_eq!(load_attachment_folder(&dir.path().to_string_lossy()), "./assets");
    }

    #[test]
    fn missing_config_means_default() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_attachment_folder(&dir.path().to_string_lossy()), "");
    }

    /// A folder nobody has deployed has no `site_id`, and the error says what
    /// to run — the domain verb and the newsletter sender both stop here.
    #[test]
    fn site_id_for_folder_errors_without_state_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let err = site_id_for_folder(tmp.path()).unwrap_err();
        assert!(
            err.contains("no site_id") || err.contains("state.toml"),
            "got: {}",
            err
        );
    }
}

#[cfg(test)]
mod version_ahead_guard_tests {
    use super::*;

    fn project_with_schema_version(v: u32) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let moss = dir.path().join(".moss");
        std::fs::create_dir_all(&moss).unwrap();
        std::fs::write(moss.join("config.toml"), format!("schema_version = {v}\n")).unwrap();
        dir
    }

    #[test]
    fn config_schema_version_ahead_reads_through_the_one_parse_read_project_config_already_does() {
        let dir = project_with_schema_version(crate::config::migrations::CURRENT_VERSION + 3);
        let path = dir.path().to_str().unwrap();
        assert_eq!(
            config_schema_version_ahead(path).unwrap(),
            Some(crate::config::migrations::CURRENT_VERSION + 3)
        );

        let dir = project_with_schema_version(crate::config::migrations::CURRENT_VERSION);
        assert_eq!(config_schema_version_ahead(dir.path().to_str().unwrap()).unwrap(), None);

        let dir = tempfile::tempdir().unwrap();
        assert_eq!(config_schema_version_ahead(dir.path().to_str().unwrap()).unwrap(), None);
    }

    /// The one door guard every outward door (publish, syndicate, email
    /// send) shares. Ablated by reverting `ensure_config_current` to
    /// `Ok(())` unconditionally: goes red on `is_err()`.
    #[test]
    fn ensure_config_current_refuses_only_a_confirmed_version_ahead() {
        let ahead = project_with_schema_version(crate::config::migrations::CURRENT_VERSION + 1);
        let err = ensure_config_current(ahead.path().to_str().unwrap()).unwrap_err();
        assert!(err.contains("schema_version") && err.contains("newer"), "got: {err}");

        let current = project_with_schema_version(crate::config::migrations::CURRENT_VERSION);
        assert!(ensure_config_current(current.path().to_str().unwrap()).is_ok());

        // A read failure (no config at all here — genuinely absent) must not
        // refuse: absence is not version-ahead.
        let absent = tempfile::tempdir().unwrap();
        assert!(ensure_config_current(absent.path().to_str().unwrap()).is_ok());
    }
}
