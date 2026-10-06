//! The two decisions in [`super`] that need no plugin, no build and no vault.
//!
//! What the driver as a whole does is proved a layer up, by
//! the app's own deploy-plugin route test, which runs both binaries
//! over a vault carrying a real deploy plugin. These are the pieces that would
//! otherwise only be exercised on their happy path there.

use super::*;
use crate::plugins::types::{DeploymentInfo, HookResult};

fn completed(success: bool, deployment: Option<DeploymentInfo>) -> DeployResult {
    DeployResult::Completed {
        result: HookResult {
            success,
            message: Some("the plugin said this".to_string()),
            toast: None,
            context: None,
            deployment,
            setup: None,
        },
    }
}

fn deployment() -> DeploymentInfo {
    DeploymentInfo {
        method: "onionpress".to_string(),
        url: "http://example.onion".to_string(),
        deployed_at: "2026-01-01T00:00:00Z".to_string(),
        metadata: Default::default(),
        dns_target: None,
        addresses: vec![],
    }
}

/// The address on stdout, and the plugin's own sentence beside it. Until
/// track P slice P4 this arm printed `files_uploaded` and a hardcoded
/// `files_removed: 0` and dropped the message; the number moss had was its
/// own enumeration of the generation, not what the plugin shipped.
#[test]
fn a_published_address_becomes_the_line_stdout_prints() {
    let out = report_for(&completed(true, Some(deployment())))
        .expect("a plugin that published is a success");
    assert!(matches!(
        out,
        DeployReport::Plugin { ref url, ref message }
            if url == "http://example.onion"
                && message.as_deref() == Some("the plugin said this")
    ));

    // A plugin that publishes and says nothing still publishes; `report`
    // prints the address and nothing beside it rather than inventing an
    // outcome word moss was never told.
    let mut silent = completed(true, Some(deployment()));
    if let DeployResult::Completed { result } = &mut silent {
        result.message = None;
    }
    assert!(matches!(
        report_for(&silent).expect("still a publish"),
        DeployReport::Plugin { message: None, .. }
    ));
}

/// A plugin reports its own failure INSIDE an `Ok` — `success: false` with the
/// message it wants shown. Left unmapped, `moss deploy` would exit 0 on a
/// publish that did not happen.
#[test]
fn a_plugin_reported_failure_is_an_error_not_a_zero_exit() {
    let err = report_for(&completed(false, None))
        .expect_err("a plugin-reported failure is not a publish");
    assert_eq!(err, "the plugin said this");
}

/// Success with nowhere to point at. The app can show a toast for this; a
/// terminal whose whole stdout contract is the site address cannot.
#[test]
fn success_with_no_deployment_has_no_address_to_print() {
    let err = report_for(&completed(true, None)).expect_err("nothing to print");
    assert!(err.contains("named no published address"), "got: {err}");
}

/// The mismatch no cheaper layer can see: the deploy context enumerates a
/// directory and the plugin reads bytes through `.moss/build.nosync/current`. If the
/// build's generation is not the active one, publishing would ship one tree
/// and describe another.
#[test]
fn a_generation_that_was_not_promoted_refuses_rather_than_shipping_the_old_one() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-tmp/plugin-push");
    std::fs::create_dir_all(&base).expect("create temp base");
    let tmp = tempfile::Builder::new()
        .prefix("promote")
        .tempdir_in(&base)
        .expect("create vault dir");
    let mp = MossPaths::new(tmp.path());

    let sealed = {
        let mut pending =
            crate::build::manifest::PendingManifest::new(crate::types::content::SiteHashes::default());
        pending.register(
            &crate::build::served_path::ServedPath::from_source("index.html").unwrap(),
            b"<html></html>",
            crate::build::manifest::HashBucket::Files,
        );
        pending.seal()
    };

    // Both generations on disk; `current` points at the OTHER one.
    let stale = mp.generation_dir("0000000000000000");
    std::fs::create_dir_all(&stale).expect("stale generation");
    std::fs::create_dir_all(mp.generation_dir(sealed.generation_id())).expect("sealed generation");
    mp.set_current_ptr("0000000000000000").expect("promote the stale one");

    let err = site_dir_for_plugin(&mp, &sealed).expect_err("must refuse");
    assert!(err.contains(sealed.generation_id()), "the refusal names what was sealed: {err}");

    // And the same call once `current` catches up.
    mp.set_current_ptr(sealed.generation_id()).expect("promote the sealed one");
    assert_eq!(
        site_dir_for_plugin(&mp, &sealed).expect("must allow"),
        mp.current_ptr(),
        "the plugin is handed the path it reads bytes through, not the stage"
    );
}

/// A sealless publish is refused, and this is the case the e2e above cannot
/// see: when promotion is withheld, `current` and the file list BOTH come from
/// the previous generation, so every end-to-end assertion still passes while
/// the wrong tree ships. Driven through the whole inner body rather than
/// through `site_dir_for_plugin`, because the bug was that the body returned
/// early before reaching the check.
///
/// A folder that closes mid-build (`PipelineRunOutput::publishable == false`)
/// is how a real build lands here: promotion is withheld
/// (`ship::Promotion::Withheld`), the seal tail skips `adopt_sealed`, and the
/// slot the driver captured stays empty. Originally, a dehydrated
/// cloud folder whose sources could not be read reached the same withhold;
/// that path was removed on 2026-09-17, leaving
/// cancellation as the only cause.
#[tokio::test]
async fn a_publish_with_no_promoted_generation_refuses() {
    struct NoSink;
    impl progress::DeploySink for NoSink {
        fn deploy_progress(&self, _: progress::DeployProgress) {}
    }

    let managers = crate::plugins::manager::ManagerCache::headless(
        crate::build::ports::reporter::discard_owned(),
    );
    let ports = crate::deploy::one_shot::HeadlessDeployPorts;
    let sink: std::sync::Arc<dyn progress::DeploySink> = std::sync::Arc::new(NoSink);
    let err = run_plugin_deploy_inner(&PluginDeployContext {
        folder: std::path::Path::new("/nonexistent-vault"),
        sealed: None,
        managers: &managers,
        sink: &sink,
        ports: &ports,
    })
    .await;
    let Err(err) = err else {
        panic!("a publish that promoted nothing must not reach the plugin");
    };
    assert!(err.contains("not sealed yet"), "got: {err}");
}

/// A version-ahead config refuses before the seal check even runs — this is
/// the plugin route's half of the three-door guard (`push_site_inner_impl`
/// for hosted, this for plugin, `push_prebuilt_inner` for prebuilt), all
/// three calling `site_config::ensure_config_current`. `sealed: None` here
/// would normally refuse with "not sealed yet" (see the test above); getting
/// the schema_version message instead proves this check runs first. Ablated
/// by deleting the `ensure_config_current(&folder_str)?;` call at the top of
/// `run_plugin_deploy_inner`: goes red as the "not sealed yet" message from
/// `require_sealed` instead.
#[tokio::test]
async fn a_version_ahead_config_refuses_before_the_seal_check() {
    struct NoSink;
    impl progress::DeploySink for NoSink {
        fn deploy_progress(&self, _: progress::DeployProgress) {}
    }

    let tmp = tempfile::tempdir().unwrap();
    let moss_dir = tmp.path().join(".moss");
    std::fs::create_dir_all(&moss_dir).unwrap();
    let future = crate::config::migrations::CURRENT_VERSION + 1;
    std::fs::write(moss_dir.join("config.toml"), format!("schema_version = {future}\n")).unwrap();

    let managers = crate::plugins::manager::ManagerCache::headless(
        crate::build::ports::reporter::discard_owned(),
    );
    let ports = crate::deploy::one_shot::HeadlessDeployPorts;
    let sink: std::sync::Arc<dyn progress::DeploySink> = std::sync::Arc::new(NoSink);
    let err = run_plugin_deploy_inner(&PluginDeployContext {
        folder: tmp.path(),
        sealed: None,
        managers: &managers,
        sink: &sink,
        ports: &ports,
    })
    .await
    .expect_err("a version-ahead config must refuse");
    assert!(err.contains("schema_version") && err.contains("newer"), "got: {err}");
}
