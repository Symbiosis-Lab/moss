//! The publish half that needs no window — which, since track C4e, is the
//! publish.
//!
//! All three bodies live here: the prebuilt-directory path ([`prebuilt`], C4c),
//! the moss-hosted one ([`push`], C4e) and the plugin-owned one
//! ([`plugin_push`], P2b), over the machinery they share — the
//! single-flight latch, the upload window and its per-file routing, the
//! mass-removal safety gate, the record a landed publish writes ([`landed`]),
//! and the progress vocabulary they report through. What the app adds is a
//! *sink* ([`progress::DeploySink`]) and four answers ([`crate::build::ports::deploy`]);
//! `src-tauri/src/deploy/app_seam.rs` is both, in one file, deliberately.
//!
//! What stays in `src-tauri/src/deploy.rs` is everything *before* a publish
//! body, and it is app-shaped for one reason: a long-lived process with a file
//! watcher has in-flight build work to drain and a sealed manifest sitting in
//! session state. A terminal publish has neither. So the Tauri commands, that
//! resolution, and the build-liveness listener stayed.

pub mod freeze;
pub mod mass_remove;
pub mod landed;
// The bytes and one record behind every landed publish (ADR-083), read from
// the same sealed-manifest pass `landed::record_landed` already makes. No CLI
// verb yet — `moss history` and restore are later slices.
pub mod history;
// The Added/Moved/Removed page rows a publish receipt shows, merged from a
// completion's change set, its detected renames, and the article maps.
// Pure — no filesystem, no network.
pub mod change_record;
// What a publish that builds its own site does AROUND the build: catch the
// seal, and answer the app-shaped ports with nothing. Shared by `push` and
// `plugin_push`, which is why it is not inside either.
pub mod one_shot;
// The deploy hook's context and the github-pages subpath guard. `pub(crate)`:
// its only user is `plugin_push`, in this crate. It was `pub` for the app's
// copy of the plugin publish, which track P slice P3 deleted.
pub(crate) mod plugin_context;
pub mod plugin_push;
pub mod prebuilt;
// The gate that reads the deploy target's declared `setup` block against this
// user's settings and credential store: what is still missing, and the refusal
// a publish gets when something is (ADR-072). It sits beside the other publish
// refusal, `refuse_publish` below, rather than under `plugins/contributions`:
// it reads the manifest but depends on discovery, the registry and the
// credential store, and the two refusals are asked by the same call sites.
pub mod publish_setup;
pub mod push;
// Cross-process exclusion for the machine-scoped OnionPress stack: the install
// lock the app takes, and the publish lease a terminal publish writes. Here
// rather than app-side because `moss deploy` is the second process the
// exclusion exists for, and it cannot see `src-tauri`.
pub mod stack_activity;
pub mod progress;
pub mod route;
pub mod upload;

use serde::{Deserialize, Serialize};
use specta::Type;

/// Where a moss-hosted site's apex and `www` A records point.
///
/// `domain-connect/mosspub.com.website.json` — the template a DNS provider
/// applies for the user — repeats this address, and `provider_tests.rs` fails
/// if they disagree: a stale template writes a dead address into a real zone.
pub const MOSSPUB_VPS_IP: &str = "45.76.13.224";

/// Result of a push operation.
///
/// The frontend's contract for `push_site`, and only that. A terminal
/// publish's outcome type is [`DeployReport`], which wraps this — plugin
/// outcomes do not belong here, because a plugin reports a sentence and no
/// counts and this type's counts are seta's real ones (track P slice P4).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PushResult {
    /// Push succeeded, site is live.
    Success {
        url: String,
        files_uploaded: u32,
        files_removed: u32,
    },
    /// No site_id configured — user needs to run setup first.
    NeedsSetup,
}

impl PushResult {
    /// Whether bytes reached the target — the condition every publish path
    /// stamps the folder on. The one owner of that question: the app's
    /// moss-hosted call site asks it through here, and
    /// [`DeployReport::landed`] delegates to it.
    pub fn landed(&self) -> bool {
        match self {
            PushResult::Success { .. } => true,
            PushResult::NeedsSetup => false,
        }
    }
}

/// What a finished `moss deploy` gives the terminal.
///
/// Wider than [`PushResult`] — the frontend's contract for `push_site` —
/// because the two kinds of publish moss can run know different things about
/// what they did. seta answers a commit with exactly which files it updated
/// and removed. A deploy plugin is handed a directory, uploads it however its
/// target works, and reports back one sentence plus whatever it measured;
/// moss counts nothing at the far end and cannot. Flattening the plugin route
/// onto `PushResult` is what made `files_removed: 0` a fabricated number, and
/// what dropped the plugin's own sentence on the floor (track P slice P4).
#[derive(Debug)]
pub enum DeployReport {
    /// moss's own hosting, or a prebuilt upload — carried whole rather than
    /// re-spelled, so seta's counts and its setup refusal have one shape in
    /// the tree and this enum adds only what is genuinely new.
    Push(PushResult),
    /// A deploy plugin published. `message` is the plugin's own sentence,
    /// printed unchanged — for OnionPress it is the outcome of the receiver's
    /// dual-probe reachability check ("live on Tor at …" versus "Published to
    /// …"), which is the only reachability answer a terminal publish has,
    /// because moss runs no probe of its own on this route.
    Plugin {
        url: String,
        message: Option<String>,
    },
}

impl DeployReport {
    /// Whether bytes reached the target — [`PushResult::landed`] owns the
    /// question for everything that arrived as a push. A plugin that reported
    /// success WITH a deployment is the plugin route's own proof of landing;
    /// every other plugin outcome is an `Err` before it gets here.
    pub fn landed(&self) -> bool {
        match self {
            DeployReport::Push(result) => result.landed(),
            DeployReport::Plugin { .. } => true,
        }
    }
}

impl From<PushResult> for DeployReport {
    fn from(result: PushResult) -> Self {
        DeployReport::Push(result)
    }
}

/// URL for the post-publish success surfaces — the "View site" toast action,
/// the CLI "Site published:" line, and the frontend's post-website walk: the
/// custom domain once DNS is verified, otherwise the site's subdomain URL.
///
/// The custom domain only wins in the Production environment — it serves the
/// production site, so a staging/local deploy's success URL must stay on the
/// environment's own subdomain or "View site" silently shows prod content.
///
/// Reads state.toml fresh at the moment of use so a domain the orchestrator
/// verified during THIS deploy is already reflected. Never use this for
/// `record_publish` — the saved `last_deployment_url` must stay
/// the subdomain URL (caption invariant, see `build_deployment_status`).
pub fn view_site_url(
    folder_path: &str,
    site_id: &str,
    env: crate::config::environment::HostingEnvironment,
) -> String {
    let custom = if env == crate::config::environment::HostingEnvironment::Production {
        let config = crate::build::site_config::get_domain_config(folder_path).unwrap_or_default();
        crate::config::deployment::live_custom_domain_url(&config)
    } else {
        None
    };
    custom.unwrap_or_else(|| format!("https://{}{}", site_id, env.site_suffix()))
}

#[cfg(test)]
mod view_site_url_tests {
    use super::view_site_url;
    use crate::config::deployment::DomainDeploymentConfig;
    use crate::config::environment::HostingEnvironment;

    fn folder_with(domain: Option<&str>, dns_configured: bool) -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().to_str().unwrap();
        let config = DomainDeploymentConfig {
            site_id: Some("her-blog".to_string()),
            ..Default::default()
        };
        crate::vault::deployment_state::save_domain_config(path, &config).expect("write state.toml");
        // Domain is authored intent (config.toml); dns verification is an
        // observation (state.toml). Written through their real writers so
        // these tests exercise the same read path the app does.
        if let Some(domain) = domain {
            crate::vault::config::save_site_str(path, "domain", domain).expect("write config.toml");
        }
        if dns_configured {
            crate::vault::deployment_state::update_domain_observation(path, |mut o| {
                o.dns_configured = true;
                o.checked_at = "2026-01-01T00:00:00Z".to_string();
                Some(o)
            })
            .expect("write observation");
        }
        dir
    }

    #[test]
    fn prefers_custom_domain_once_dns_is_configured() {
        let dir = folder_with(Some("example.com"), true);
        assert_eq!(
            view_site_url(
                dir.path().to_str().unwrap(),
                "her-blog",
                HostingEnvironment::Production
            ),
            "https://example.com"
        );
    }

    #[test]
    fn staging_deploys_ignore_the_custom_domain() {
        // The custom domain serves the PRODUCTION site. A staging deploy's
        // "View site" must land on what was just deployed — the staging
        // subdomain — not silently show prod content.
        let dir = folder_with(Some("example.com"), true);
        assert_eq!(
            view_site_url(
                dir.path().to_str().unwrap(),
                "her-blog",
                HostingEnvironment::Staging
            ),
            "https://her-blog.staging.mosspub.com"
        );
    }

    #[test]
    fn falls_back_to_subdomain_while_dns_unverified() {
        let dir = folder_with(Some("example.com"), false);
        assert_eq!(
            view_site_url(
                dir.path().to_str().unwrap(),
                "her-blog",
                HostingEnvironment::Production
            ),
            "https://her-blog.mosspub.com"
        );
    }

    #[test]
    fn falls_back_to_subdomain_without_custom_domain() {
        let dir = folder_with(None, true);
        assert_eq!(
            view_site_url(
                dir.path().to_str().unwrap(),
                "her-blog",
                HostingEnvironment::Production
            ),
            "https://her-blog.mosspub.com"
        );
    }

    #[test]
    fn falls_back_when_no_state_file_exists() {
        let dir = tempfile::TempDir::new().unwrap();
        assert_eq!(
            view_site_url(
                dir.path().to_str().unwrap(),
                "her-blog",
                HostingEnvironment::Staging
            ),
            "https://her-blog.staging.mosspub.com"
        );
    }
}

/// Bring publish's own inputs down from the cloud before the publish commits
/// the user to anything.
///
/// The identity key (`.moss/identity/secret-key`) lives inside the synced
/// vault, so a plain read of an evicted one returns `EDEADLK` under the
/// dataless fail-fast policy. `resolve_sealed_manifest_for_deploy` loads the
/// identity in its LAST step, after draining the whole media queue, so on the
/// reported site (moss#986) the user waited minutes at "Preparing to publish…"
/// to be told `Resource deadlock avoided (os error 11)`.
///
/// Here, the failure arrives in seconds — and usually stops being a failure at
/// all, because asking is what makes a file arrive.
///
/// A file that is genuinely absent passes: publish has always been allowed to
/// mint an identity on first use, and `materialize_input` answers only the
/// cloud question. Only "present, but the bytes are elsewhere" stops here.
///
/// Crossed here at C4f, from `src-tauri/src/deploy.rs`, because it had become
/// the app's private answer to a question every publish asks. `moss deploy
/// --prebuilt` had never asked it — that path loads the same key with a plain
/// read and would have reported the raw `EDEADLK` moss#986 was closed for. It
/// now calls this, as does the hosted route the terminal gained in the same
/// slice.
pub async fn preflight_publish_inputs(folder_path: &std::path::Path) -> Result<(), String> {
    use crate::build::cloud_readiness::{materialize_input, INTERACTIVE_DEADLINE};

    let identity_dir = folder_path.join(".moss").join("identity");
    let inputs = [identity_dir.join("secret-key"), identity_dir.join("public.json")];
    let stalled: Option<std::path::PathBuf> = tokio::task::spawn_blocking(move || {
        // `materialize_input` blocks for up to the deadline, so it runs off the
        // async runtime — a publish must not park a runtime worker for 15s.
        inputs
            .into_iter()
            .find(|p| materialize_input(p, INTERACTIVE_DEADLINE).is_err())
    })
    .await
    .map_err(|e| format!("Pre-publish check failed: {e}"))?;

    let Some(path) = stalled else { return Ok(()) };
    let provider = crate::build::cloud_provider::detect_from_path(folder_path)
        .and_then(|p| p.display_name())
        .unwrap_or("the cloud");
    log::warn!(
        "publish: {} is still in the cloud after the pre-flight wait — refusing to start",
        path.display()
    );
    Err(format!(
        "Your site's identity is still downloading from {provider}. \
         moss has asked for it — try publishing again in a moment."
    ))
}

/// Refuse the publish if this site points at media that does not exist.
///
/// A published site cannot contain a broken image. moss will not paper over one
/// either — the blueprint grid the author sees locally is a preview affordance
/// and ships nowhere — so the only honest options are to fix the reference or
/// not to publish. This is the second one.
///
/// There is deliberately NO override. An override would make the refusal
/// advisory, which is the same as not having one: the damage lands on a
/// stranger's screen, and the person who chose it can never see the result,
/// because their own preview looks the same either way.
///
/// THE GATE, NOT THE MESSAGE. The app calls `list_missing_media` before it ever
/// calls publish, so a user should never read this string — they get a list of
/// files they can click. This exists so the guarantee holds for every other
/// caller too: the CLI, a plugin deploy, a second call site added later.
///
/// The verdict comes from the last build's own resolver
/// (`BuildRecords`), never from a fresh scan. A second scan
/// would be a second opinion about the same question, free to disagree with the
/// site that was actually built.
///
/// Absent means "no build in this session", not "clean" — and since 2026-08-29
/// a headless build records its verdict here too, so the CLI is refused on the
/// same evidence the app is.
///
/// Crossed here at C4f. It had sat in `src-tauri/src/missing_media.rs` with a
/// doc promising the guarantee held "for every other caller too: the CLI, a
/// plugin deploy, a second call site added later" — which the CLI could not
/// honour, because it could not name the function. The evidence it reads
/// (`build_records`) crossed in 2026-08-29; the verdict followed here.
pub fn refuse_publish(folder_path: &str) -> Result<(), String> {
    match crate::system::build_records::records().missing_media(folder_path) {
        Some(missing) if !missing.is_empty() => Err(refusal_text(missing.len())),
        _ => Ok(()),
    }
}

/// The refusal a user reads. Says what did not happen, not which subsystem said
/// no — see `docs/reference/operating/writing-release-notes.md`.
pub(crate) fn refusal_text(count: usize) -> String {
    let subject = if count == 1 {
        "1 file is missing".to_string()
    } else {
        format!("{count} files are missing")
    };
    format!(
        "Nothing published — {subject}. A published site can't show a broken image. \
         Fix these, then publish again."
    )
}

/// What every publish needs before it can start: the site it publishes to, and
/// the key it signs with.
///
/// `Ok(None)` is `NeedsSetup`, and since C4g it means one specific thing: the
/// folder has no environment set, so moss will not mint a site for it. A folder
/// that HAS one and no `site_id` yet is registered here rather than refused —
/// the first publish is a step of the route, not a route of its own, and
/// putting it here is what lets both drivers and both binaries reach it.
/// `requested_site_id` is `--site-id=<name>`; without it the name is derived
/// from the folder.
///
/// One owner since C4f, when the second driver arrived. Both
/// [`prebuilt::run_prebuilt_deploy`] and [`push::run_hosted_deploy`] opened
/// with the same four steps in the same order, and the twin had already
/// drifted before it was noticed: only the app asked
/// [`preflight_publish_inputs`], so `moss deploy --prebuilt` on an evicted
/// iCloud folder reported the raw `Resource deadlock avoided` that moss#986
/// was closed for. A gate added to a publish now goes in one place, which is
/// the property that failure cost.
/// Whether moss may mint a site for a folder that has never published.
///
/// Registering is irreversible external state at seta — a site ID cannot be
/// un-minted — so it happens only when the environment was named DELIBERATELY.
/// `moss env <staging|production|local>` writes the top-level `environment` key
/// in `.moss/config.toml`; a folder without it reads as Production by default,
/// and defaulting into a real production site nobody asked for is not a default
/// anyone can undo.
///
/// `Err` is not `false`. An evicted `.moss/config.toml` fails to parse, and
/// answering that with "you never ran `moss env`" sends the author to fix a
/// thing that is not wrong — the moss#986 shape, one file over. The read's own
/// error carries what happened.
///
/// Pure and separate from [`resolve_publish_inputs`] so a test can assert it
/// without entering a function that, if the gate regressed, would go on to call
/// a real server. A test whose failure mode is the irreversible action is the
/// wrong shape however green it is.
fn first_publish_is_permitted(folder_str: &str) -> Result<bool, String> {
    Ok(crate::build::site_config::get_environment_field(folder_str)?.is_some())
}

pub async fn resolve_publish_inputs(
    folder: &std::path::Path,
    requested_site_id: Option<&str>,
    sink: &std::sync::Arc<dyn progress::DeploySink>,
) -> Result<Option<(String, crate::identity::Identity)>, String> {
    let folder_str = folder.to_string_lossy().to_string();
    // NOT `unwrap_or_default()`: this read is what decides whether to register,
    // and an unparsable `state.toml` defaults to `site_id: None` — "this folder
    // never published". A registered folder whose state.toml is evicted would
    // pass the gate and mint a SECOND site. A missing file is `Ok(Default)`, so
    // a genuine first publish is unaffected.
    let config = crate::build::site_config::get_domain_config(&folder_str)?;

    // Every pure refusal first, so nothing below is paid for by a publish that
    // was never going to happen. A name the server would reject is knowable at
    // t=0, and saying so after a 15-second materialize wait and a freshly
    // minted key is the ordering this function exists to avoid — which is why
    // the DERIVED name is checked here too, not only the one a flag supplied.
    let requested = match &config.site_id {
        Some(_) => None,
        None => {
            if !first_publish_is_permitted(&folder_str)? {
                return Ok(None);
            }
            let chosen = requested_site_id.map(str::to_string).unwrap_or_else(|| {
                derive_site_id_from_root(&crate::vault::paths::VaultRoot::resolve(folder))
            });
            validate_site_id(&chosen)?;
            Some(chosen)
        }
    };

    // Before anything expensive, and before the key is read: asking the cloud
    // for a file is usually what makes it arrive.
    //
    // Ahead of the registration below, not after it, and the difference is a
    // reachable bug rather than tidiness. A refused publish leaves an identity
    // behind (see the note under it), so a folder can hold a key and no
    // `site_id`; reading that key on the retry with no materialize wait is the
    // raw `Resource deadlock avoided` moss#986 was closed for.
    preflight_publish_inputs(folder).await?;

    // Mint one if this folder has never published. A publish that cannot sign
    // cannot start, so this fails before a byte is read off disk — and, on the
    // hosted route, before the build. That ordering means a `moss deploy` that
    // is later refused (a broken image, say) has still written `.moss/identity/`
    // into a first-time vault. Deliberate: failing after a multi-minute build
    // to say something knowable at t=0 is the worse trade, and it is what the
    // prebuilt route always did.
    let mut service = crate::identity::service::IdentityService::new(folder);
    service.ensure_signing_key().map_err(|e| e.to_string())?;
    let identity = service.get_or_create().map_err(|e| e.to_string())?.clone();

    let site_id = match (config.site_id, requested) {
        (Some(id), _) => id,
        (None, Some(chosen)) => {
            // The only step in here that can take seconds AND is not the
            // caller's own stage. Without it a first publish shows nothing
            // between the command and the build — the prebuilt route used to
            // say this from its own registrar, and losing the line with the
            // registrar would have been a silent regression.
            sink.stage(
                progress::DeployStage::Preparing,
                0,
                0,
                &crate::infra::app_advisory::t("registering_site"),
            );
            register_moss_host_site(folder, &chosen, &identity).await?
        }
        // Unreachable by construction: `requested` is `Some` on exactly the
        // branch where `site_id` was `None` and the gate let it through.
        (None, None) => return Ok(None),
    };

    Ok(Some((site_id, identity)))
}

#[cfg(test)]
mod registration_gate_tests {
    use super::first_publish_is_permitted;

    /// The gate on the only irreversible thing a publish does.
    #[test]
    fn only_a_top_level_environment_key_permits_a_first_publish() {
        let dir = tempfile::tempdir().expect("tempdir");
        let moss = dir.path().join(".moss");
        std::fs::create_dir_all(&moss).expect("mkdir .moss");
        let folder = dir.path().to_string_lossy().to_string();
        let permitted = |body: &str| {
            std::fs::write(moss.join("config.toml"), body).expect("write");
            first_publish_is_permitted(&folder).expect("a parsable config.toml")
        };

        assert!(
            !first_publish_is_permitted(&folder).expect("a missing file is not an error"),
            "a folder with no config.toml at all must not be registrable"
        );
        assert!(
            !permitted("[site]\nenvironment = \"staging\"\n"),
            "the key is TOP-LEVEL; `moss env` does not write it under `[site]`, and reading \
             one there would let a folder that never ran it mint a production site"
        );
        assert!(permitted("environment = \"staging\"\n"), "`moss env` writes exactly this");
    }

    /// A name the server would refuse is knowable with no server. The folder
    /// name is the default site ID, so a folder called `www` is a real case —
    /// and before the check moved up front, it was answered only after a
    /// materialize wait and a freshly minted signing key.
    #[tokio::test]
    async fn a_reserved_folder_name_is_refused_before_anything_expensive() {
        let base = tempfile::tempdir().expect("tempdir");
        let vault = base.path().join("www");
        std::fs::create_dir_all(vault.join(".moss")).expect("mkdir");
        std::fs::write(vault.join(".moss").join("config.toml"), "environment = \"staging\"\n")
            .expect("write");

        let sink = super::progress::silent();
        let err = super::resolve_publish_inputs(&vault, None, &sink)
            .await
            .expect_err("a reserved name must refuse");
        assert!(err.contains("reserved"), "{err}");
        assert!(
            !vault.join(".moss").join("identity").exists(),
            "the refusal must land before a signing key is minted"
        );
    }
}

/// Reserved site IDs that conflict with existing subdomains/services.
/// Must match the server's RESERVED_SITE_IDS in moss-seta/src/db/client.ts.
const RESERVED_SITE_IDS: &[&str] = &[
    "api", "staging", "www", "moss", "mail", "ftp", "admin", "static",
];

/// Validate a site_id: lowercase alphanumeric + hyphens, 3-63 chars, no
/// leading/trailing hyphens, not a reserved word.
pub fn validate_site_id(site_id: &str) -> Result<(), String> {
    if site_id.len() < 3 || site_id.len() > 63 {
        return Err("Site ID must be 3-63 characters".to_string());
    }
    if !site_id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err("Site ID must contain only lowercase letters, digits, and hyphens".to_string());
    }
    if site_id.starts_with('-') || site_id.ends_with('-') {
        return Err("Site ID must not start or end with a hyphen".to_string());
    }
    if RESERVED_SITE_IDS.contains(&site_id) {
        return Err(format!("\"{}\" is a reserved name", site_id));
    }
    Ok(())
}

/// Derive a valid moss-host site ID from a resolved vault root.
///
/// Lowercases the root NAME, replaces every character that is not `[a-z0-9]` with
/// `-`, collapses consecutive `-`, and trims leading/trailing `-`. If the result is
/// shorter than 3 characters the string is prefixed with `"site-"` to guarantee it
/// passes [`validate_site_id`].
///
/// A site ID is irreversible external state at seta, so the name must be the ONE
/// resolved name. The previous `components().filter_map(Normal).last()` walk was a
/// seventh basename algorithm and answered `/Sites/blog/..` with "blog" — the folder
/// the user asked to leave. For a plain absolute path `components().last()` and
/// `file_name()` agree, so every site ID minted to date is preserved.
pub fn derive_site_id_from_root(root: &crate::vault::paths::VaultRoot) -> String {
    let base = root.name();

    // Lowercase and replace non-alnum chars with hyphens.
    let lower: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                c
            } else if c.is_ascii_uppercase() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();

    // Collapse consecutive hyphens into one.
    let mut collapsed = String::new();
    let mut prev_hyphen = false;
    for c in lower.chars() {
        if c == '-' {
            if !prev_hyphen {
                collapsed.push(c);
            }
            prev_hyphen = true;
        } else {
            collapsed.push(c);
            prev_hyphen = false;
        }
    }

    // Trim leading/trailing hyphens.
    let trimmed = collapsed.trim_matches('-').to_string();

    // Ensure minimum length (validate_site_id requires >= 3 chars).
    let result = if trimmed.len() < 3 {
        format!("site-{}", if trimmed.is_empty() { "pub".to_string() } else { trimmed })
    } else {
        trimmed
    };

    // Ensure maximum length (validate_site_id requires <= 63 chars).
    if result.len() > 63 {
        result.get(..63).unwrap_or(&result).trim_matches('-').to_string()
    } else {
        result
    }
}

#[cfg(test)]
mod validate_site_id_tests {
    use super::validate_site_id;

    #[test]
    fn accepts_lowercase_alnum_and_inner_hyphens() {
        for id in ["her-blog", "my-site-123", "abc"] {
            assert!(validate_site_id(id).is_ok(), "{id} should be valid");
        }
    }

    #[test]
    fn refuses_what_the_server_would_refuse() {
        // Length, character set, hyphen placement, and the reserved list.
        // The client-side check exists to say no BEFORE a round trip, not
        // instead of one — moss-seta refuses the same names in `db/client.ts`,
        // and the constant this reads carries that pointer.
        for id in [
            "ab", "", "Her-Blog", "my site", "my_site", "my.site", "-my-site", "my-site-", "www",
        ] {
            assert!(validate_site_id(id).is_err(), "{id:?} should be refused");
        }
    }
}

#[cfg(test)]
mod derive_site_id_tests {
    use super::{derive_site_id_from_root, validate_site_id};
    use crate::vault::paths::VaultRoot;

    fn id_for(path: &str) -> String {
        let id = derive_site_id_from_root(&VaultRoot::resolve(path));
        validate_site_id(&id).unwrap_or_else(|e| panic!("derived {id:?} is not a valid site id: {e}"));
        id
    }

    /// A site ID is IRREVERSIBLE external state at seta — once minted it names
    /// the user's site forever. The old walk (`components().filter_map(Normal)
    /// .last()`) answers `/Sites/blog/..` with "blog": the folder the user
    /// asked to LEAVE. Only the filesystem knows the real answer, which is why
    /// the resolver canonicalizes for `..` and this function starts from the
    /// resolved name.
    #[test]
    fn a_parent_dir_argument_names_the_parent() {
        let base = std::env::temp_dir().join(format!("moss_siteid_{}", uuid::Uuid::new_v4()));
        let site = base.join("Sites").join("blog");
        std::fs::create_dir_all(&site).unwrap();

        let root = VaultRoot::resolve_in(std::path::Path::new(".."), &site);
        let id = derive_site_id_from_root(&root);
        assert_eq!(id, "sites", "`..` must name the parent, not `blog` and not `site-pub`");
        assert!(validate_site_id(&id).is_ok());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn folder_names_are_lowercased_hyphenated_and_padded() {
        assert_eq!(id_for("/Users/alice/My Blog"), "my-blog");
        assert_eq!(id_for("/projects/Stage2 Verify!"), "stage2-verify");
        assert_eq!(id_for("/tmp/--Foo--"), "foo");
        assert_eq!(id_for("/tmp/ab"), "site-ab", "under 3 chars is padded, not rejected");
        assert_eq!(id_for("/Users/bob/my-site-123"), "my-site-123", "already valid passes through");
    }
}

/// Register a new site with moss hosting and write the returned id into
/// `.moss/state.toml`. Returns the confirmed id, which the server may differ
/// from the requested one.
///
/// Registration IS the selection: with no `[hooks] deploy` in config.toml, a
/// present `site_id` derives `deploy_method = "moss"`.
///
/// Crossed here at C4g. `setup_moss_host` looked app-bound and was not — it
/// took an `tauri::AppHandle` that its body never once names, and every read it
/// does is either a value the caller already holds or a function that had
/// already crossed: `active_environment()` is `resolve_environment(folder)`
/// (C4d), the client is ADR-078's, and the config doors are `vault::config`'s.
/// The same shape C4d found on the publish body, one function later. That is
/// why a first publish no longer needs a window, and why the deploy route model
/// has two variants rather than three.
pub async fn register_moss_host_site(
    folder: &std::path::Path,
    site_id: &str,
    identity: &crate::identity::Identity,
) -> Result<String, String> {
    validate_site_id(site_id)?;
    let folder_str = folder.to_string_lossy().to_string();

    // The signing identity is the caller's, because both callers already hold
    // one: a publish resolves it through the cloud-aware door in
    // `resolve_publish_inputs`, and the app command through `AppState`. Reading
    // it a second time here would be the second read that made the first one's
    // materialize wait pointless.
    let env = crate::build::site_config::resolve_environment(&folder_str);
    let client = crate::seta::client::MossSetaClient::for_environment(identity, &env);
    let result = match client.register_site(site_id).await {
        Ok(r) => r,
        // Invite gate: surface the friendly "apply for access" message + link.
        Err(crate::seta::client::SetaError::NotAllowlisted { apply_url }) => {
            return Err(not_allowlisted_message(&apply_url));
        }
        Err(e) => return Err(register_failure_message(site_id, e)),
    };

    // NOT `unwrap_or_default()`: this read is followed by a write, so a
    // `state.toml` that fails to parse — an evicted iCloud file, most often —
    // would be replaced by a default carrying only the new `site_id`, silently
    // dropping whatever else the folder had recorded.
    let mut config = crate::build::site_config::get_domain_config(&folder_str)?;
    config.site_id = Some(result.site_id.clone());
    crate::vault::deployment_state::save_domain_config(&folder_str, &config)?;

    Ok(result.site_id)
}

/// Turn seta's registration refusals into something a person can act on.
///
/// Both sentences were the prebuilt route's alone until C4g, written inline
/// beside a `register_site` call that route no longer makes. They are here
/// because registration has one owner now, so both routes and both binaries
/// get them — which the app half never had, and the CLI `--site-id` half needs
/// most, since only the app pre-checks a name with `check_site_id_available`.
fn register_failure_message(site_id: &str, e: crate::seta::client::SetaError) -> String {
    let msg = e.to_string();
    // Seta returns 403 "pubkey not verified for this email" BEFORE it checks
    // ACLs, so this is the typical first-publish-of-a-new-folder failure.
    if msg.contains("pubkey not verified") || msg.contains("verification") {
        return format!(
            "Identity not yet verified for an account. \
             Run `moss` on this folder once to verify the new identity \
             via the magic-link email flow, then retry.\n\n\
             Underlying error: {}",
            msg
        );
    }
    if msg.contains("already taken") || msg.contains("must be fresh") {
        return format!(
            "Site ID '{}' is unavailable or this pubkey can't claim it. \
             Try a different site_id or use a fresh project folder.\n\n\
             Underlying error: {}",
            site_id, msg
        );
    }
    format!("Could not register the site: {}", msg)
}

/// Build the localized "you're not on the invite list" message for the seta
/// invite gate's `not_allowlisted` 403: the friendly closed-test copy (localized
/// via `app_advisory`) plus the apply link, so an un-admitted user gets an
/// actionable message instead of an opaque error. One caller since C4g —
/// [`register_moss_host_site`], which every route reaches.
pub fn not_allowlisted_message(apply_url: &str) -> String {
    let base = crate::infra::app_advisory::t("not_allowlisted");
    if apply_url.is_empty() {
        base
    } else {
        format!("{}\n{}", base, apply_url)
    }
}

#[cfg(test)]
mod not_allowlisted_message_tests {
    use super::not_allowlisted_message;

    #[test]
    fn includes_apply_url_when_present() {
        let msg = not_allowlisted_message("https://mosspub.com/apply");
        assert!(
            msg.contains("https://mosspub.com/apply"),
            "message must include the apply link: {msg}"
        );
        assert!(!msg.is_empty());
    }

    #[test]
    fn omits_link_when_apply_url_empty() {
        let msg = not_allowlisted_message("");
        assert!(!msg.contains("http"), "no link when apply_url is empty: {msg}");
        assert!(!msg.is_empty(), "still carries the base invite message");
    }
}


#[cfg(test)]
#[path = "deploy/missing_media_gate_tests.rs"]
mod missing_media_gate_tests;
