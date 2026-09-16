//! Tests for the publish setup gate — see `publish_setup.rs`.

use super::*;

/// A vault whose only deploy plugin is `ipfs`, SELECTED as the deploy
/// target, with `manifest_extra` spliced into its manifest and `config`
/// written as its `config.toml`.
///
/// The selection is not decoration. The gate asks which target Publish will
/// use, and an unselected vault publishes through moss's own hosting
/// however many plugins are installed — so without `[hooks] deploy` there
/// is nothing here to gate.
fn vault_with_ipfs(manifest_extra: &str, config: &str) -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join(".moss/plugins/ipfs");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        tmp.path().join(".moss/config.toml"),
        "[hooks]\ndeploy = \"ipfs\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("manifest.json"),
        format!(
            r#"{{"name":"ipfs","version":"1.0.0","entry":"main.js",
                 "capabilities":["deploy"]{manifest_extra}}}"#
        ),
    )
    .unwrap();
    std::fs::write(dir.join("main.js"), "// plugin").unwrap();
    if !config.is_empty() {
        std::fs::write(dir.join("config.toml"), config).unwrap();
    }
    tmp
}

fn needs(tmp: &tempfile::TempDir, stored: &[&str]) -> Option<PublishSetupNeeds> {
    deploy_setup_needs(tmp.path().to_str().unwrap(), &|_, key| stored.contains(&key))
}

const SETUP_TWO_PROVIDERS: &str = r#","display_name":"IPFS","contributes":{"deploy_target":{
    "setup":{"credentials":[
        {"key":"pinata_jwt","label":"Pinata API token","help_url":"https://example.test/keys",
         "when":{"provider":"pinata"}},
        {"key":"w3_token","label":"web3.storage token","when":{"provider":"web3storage"}}
    ]}}}"#;

/// The gate asks for the token the user's own provider setting selects —
/// asking for both would be a gate nobody could pass.
#[test]
fn the_gate_asks_only_for_what_this_users_settings_select() {
    let tmp = vault_with_ipfs(SETUP_TWO_PROVIDERS, "provider = \"pinata\"\n");
    let n = needs(&tmp, &[]).expect("the deploy target declares setup");
    assert_eq!(n.plugin, "ipfs");
    assert_eq!(n.plugin_name, "IPFS");
    assert_eq!(n.credentials.len(), 1);
    assert_eq!(n.credentials[0].key, "pinata_jwt");
    assert_eq!(n.credentials[0].label, "Pinata API token");
    assert_eq!(n.credentials[0].help_url.as_deref(), Some("https://example.test/keys"));
    assert!(!n.credentials[0].stored);
}

/// Already stored = nothing to ask. This is the common case, and it is the
/// one the whole declared-setup design exists to make free.
///
/// A stored credential is still LISTED — it is one the user can be asked to
/// correct when the service rejects it — so "nothing to ask" is `stored`
/// being true, not the list being short. The other provider's token stays
/// filtered out by its `when` clause either way: correcting a credential
/// must not ask for one this user's settings never select.
#[test]
fn a_stored_credential_leaves_nothing_to_ask_but_stays_correctable() {
    let tmp = vault_with_ipfs(SETUP_TWO_PROVIDERS, "provider = \"pinata\"\n");
    let n = needs(&tmp, &["pinata_jwt"]).expect("setup is declared");
    let keys: Vec<&str> = n.credentials.iter().map(|c| c.key.as_str()).collect();
    assert_eq!(keys, vec!["pinata_jwt"]);
    assert_eq!(n.credentials[0].label, "Pinata API token");
    assert!(n.credentials[0].stored, "nothing is missing");
}

/// A deploy plugin that declares nothing gates nothing — every plugin
/// shipping today is in this shape and must keep publishing unprompted.
#[test]
fn a_target_without_a_setup_block_is_not_gated() {
    let tmp = vault_with_ipfs("", "");
    assert!(needs(&tmp, &[]).is_none());
}

/// No deploy plugin at all (moss's own hosting) is not a gate either.
#[test]
fn a_vault_with_no_deploy_plugin_is_not_gated() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join(".moss")).unwrap();
    assert!(deploy_setup_needs(tmp.path().to_str().unwrap(), &|_, _| false).is_none());
}

/// Publishing through moss's own hosting asks a deploy plugin for nothing,
/// even when one is installed and fully declared.
///
/// Choosing moss hosting CLEARS `[hooks] deploy`, and a gate that resolved
/// the target by "the only deploy plugin installed" therefore picked the
/// plugin the publish was not going to use — so a user who had installed
/// ipfs to try it could not publish their moss-hosted site without first
/// producing a Pinata token they had no use for.
#[test]
fn a_moss_hosted_publish_is_not_gated_on_an_installed_plugin() {
    let tmp = vault_with_ipfs(SETUP_TWO_PROVIDERS, "provider = \"pinata\"\n");
    assert!(needs(&tmp, &[]).is_some(), "the fixture is a gated ipfs target");

    // The Host row switched to moss, which is written as the absence of a
    // deploy hook.
    std::fs::write(tmp.path().join(".moss/config.toml"), "").unwrap();
    assert!(
        needs(&tmp, &[]).is_none(),
        "a moss-hosted publish must not be gated on the ipfs plugin's token"
    );
}

/// An installed-but-unchosen plugin in a vault that has never published:
/// Publish opens moss's first-publish wizard, so there is nothing to gate.
#[test]
fn an_unchosen_target_is_not_gated() {
    let tmp = vault_with_ipfs(SETUP_TWO_PROVIDERS, "provider = \"pinata\"\n");
    std::fs::remove_file(tmp.path().join(".moss/config.toml")).unwrap();
    assert!(needs(&tmp, &[]).is_none());
}

/// The backend refuses the publish the frontend modal would have gated —
/// the CLI and any later call site get the same answer, in words that name
/// the target, the missing credential, and something the reader can do.
///
/// The last of those is pinned because it is the one that can quietly stop
/// being true: this refusal is read from a terminal since P2b, where an
/// instruction to click a button nobody can see is a dead end.
#[test]
fn a_missing_declared_credential_refuses_the_publish_itself() {
    let tmp = vault_with_ipfs(SETUP_TWO_PROVIDERS, "provider = \"pinata\"\n");
    let err = refusal(needs(&tmp, &[]).as_ref()).expect_err("nothing is stored");
    assert!(err.contains("IPFS"), "{err}");
    assert!(err.contains("Pinata API token"), "{err}");
    assert!(
        err.contains("Open this folder in the moss app"),
        "the remedy has to name where to go, for a reader with no window: {err}"
    );

    assert!(refusal(needs(&tmp, &["pinata_jwt"]).as_ref()).is_ok());
    assert!(refusal(None).is_ok());
}

/// The gate is not deploy-shaped: a channel declaring the same block is
/// answered by the same function, asked with the channel's capability.
///
/// Nothing gates a channel yet — publish is the only caller — so this is
/// the layer that can see the generalization at all: `deploy_setup_needs`
/// resolves the target, and everything below it takes the capability.
#[test]
fn a_channels_setup_is_resolved_by_the_same_gate() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join(".moss/plugins/matters");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("manifest.json"),
        r#"{"name":"matters","version":"1.0.0","entry":"main.js",
            "contributes":{"channel":{"display_name":"Matters","setup":{"check":true,
                "credentials":[{"key":"matters_token","label":"Matters token"}]}}}}"#,
    )
    .unwrap();
    std::fs::write(dir.join("main.js"), "// plugin").unwrap();

    let path = tmp.path().to_str().unwrap();
    let n = setup_needs(path, "matters", &Capability::Syndicate, &|_, _| false)
        .expect("the channel declares setup");
    assert_eq!(n.plugin, "matters");
    assert_eq!(n.plugin_name, "Matters");
    assert_eq!(n.credentials.len(), 1);
    assert_eq!(n.credentials[0].key, "matters_token");
    assert!(n.wants_check);

    // The channel's block is not the deploy target's: asking for a
    // capability this plugin does not contribute gates nothing.
    assert!(setup_needs(path, "matters", &Capability::Deploy, &|_, _| false).is_none());
}

/// The probe step follows when the plugin declared `check_setup` — or
/// when it declared `needs`, which the probe command evaluates host-side:
/// a needs-only manifest with `wants_check: false` would ship
/// preconditions nothing ever checks.
#[test]
fn wants_check_reports_what_the_manifest_declared() {
    let tmp = vault_with_ipfs(
        r#","contributes":{"deploy_target":{"setup":{"check":true}}}"#,
        "",
    );
    let n = needs(&tmp, &[]).expect("setup is declared");
    assert!(n.wants_check);
    assert!(n.credentials.is_empty());

    let tmp = vault_with_ipfs(
        r#","contributes":{"deploy_target":{"setup":{"needs":["binary:ipfs"]}}}"#,
        "",
    );
    assert!(needs(&tmp, &[]).expect("setup is declared").wants_check);

    let tmp = vault_with_ipfs(r#","contributes":{"deploy_target":{"setup":{}}}"#, "");
    assert!(!needs(&tmp, &[]).expect("setup is declared").wants_check);
}

/// A failed need becomes a HOST-authored blocker: `moss:stack` carries the
/// start button only moss can wire; a missing binary has no button — the
/// remedy is an install moss cannot perform. A satisfied need contributes
/// nothing.
#[test]
fn failed_needs_become_host_blockers_and_satisfied_ones_vanish() {
    let needs = [Need::Stack, Need::Binary("ipfs".into())];

    let out = blockers(&needs, &|| false, &|_| false);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].id, "moss:stack");
    assert_eq!(out[0].form.as_ref().unwrap().submit, "Start the stack");
    assert!(out[0].form.as_ref().unwrap().fields.is_empty());
    assert_eq!(out[1].id, "moss:binary:ipfs");
    assert!(out[1].message.contains("ipfs"), "{}", out[1].message);
    assert!(out[1].form.is_none());

    assert!(blockers(&needs, &|| true, &|_| true).is_empty());
}

