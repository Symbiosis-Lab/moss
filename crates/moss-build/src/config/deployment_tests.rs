use super::*;

fn record(url: &str) -> DeploymentRecord {
    DeploymentRecord {
        site_url: Some(url.to_string()),
        last_deployment_url: Some(url.to_string()),
        ..Default::default()
    }
}

#[test]
fn slot_is_method_plus_id_for_moss_and_bare_method_for_plugins() {
    assert_eq!(slot_for("moss", Some("her-blog")).as_deref(), Some("moss:her-blog"));
    assert_eq!(slot_for("moss", None), None, "half a target is no target");
    assert_eq!(slot_for("onionpress", None).as_deref(), Some("onionpress"));
    assert_eq!(
        slot_for("ipfs", Some("her-blog")).as_deref(),
        Some("ipfs"),
        "a plugin slot never consults site_id — or site_url"
    );
}

/// The round trip, on the shapes `slot_for` can actually emit — including the
/// one a colon in a user-authored `[hooks] deploy` produces, which a plain
/// split-on-colon would file under a target that does not exist.
#[test]
fn every_slot_reads_back_as_the_method_that_wrote_it() {
    for (method, site_id) in [
        ("moss", Some("her-blog")),
        ("moss", Some("her-blog:2")),
        ("onionpress", None),
        ("ipfs", Some("her-blog")),
        ("my:deployer", None),
    ] {
        let slot = slot_for(method, site_id).expect("these shapes all produce a slot");
        assert_eq!(target_for_slot(&slot), method, "slot {slot:?} lost its method");
    }
}

#[test]
fn deploy_method_derives_from_hook_then_site_id() {
    assert_eq!(derive_deploy_method(Some("onionpress"), None).as_deref(), Some("onionpress"));
    assert_eq!(
        derive_deploy_method(Some("onionpress"), Some("her-blog")).as_deref(),
        Some("onionpress"),
        "an explicit hook wins over a registered moss site"
    );
    assert_eq!(derive_deploy_method(None, Some("her-blog")).as_deref(), Some("moss"));
    assert_eq!(derive_deploy_method(None, None), None);
    assert_eq!(derive_deploy_method(Some("  "), Some("x")).as_deref(), Some("moss"));
}

#[test]
fn legacy_flat_deployment_lifts_into_the_slot_its_method_names() {
    let mut state: DeploymentState = toml::from_str(
        r#"
        site_id = "her-blog"
        deploy_method = "onionpress"
        site_url = "http://abc.onion/"
        last_deployment_url = "http://abc.onion/"
        "#,
    )
    .unwrap();
    state.migrate_legacy();
    assert_eq!(state.targets.len(), 1);
    assert_eq!(state.targets["onionpress"].site_url.as_deref(), Some("http://abc.onion/"));
    // The next save writes no legacy keys back.
    let out = toml::to_string(&state).unwrap();
    assert!(!out.contains("deploy_method"), "deploy_method must not be re-persisted: {out}");
}

#[test]
fn legacy_lift_without_method_falls_back_to_moss() {
    let mut state: DeploymentState = toml::from_str(
        r#"
        site_id = "her-blog"
        last_deployment_url = "https://her-blog.mosspub.com"
        "#,
    )
    .unwrap();
    state.migrate_legacy();
    assert_eq!(
        state.targets["moss:her-blog"].last_deployment_url.as_deref(),
        Some("https://her-blog.mosspub.com")
    );
}

#[test]
fn legacy_fields_never_clobber_an_existing_targets_map() {
    let mut state: DeploymentState = toml::from_str(
        r#"
        site_id = "her-blog"
        deploy_method = "moss"
        last_deployment_url = "https://stale.example"

        [targets."moss:her-blog"]
        last_deployment_url = "https://her-blog.mosspub.com"
        "#,
    )
    .unwrap();
    state.migrate_legacy();
    assert_eq!(
        state.targets["moss:her-blog"].last_deployment_url.as_deref(),
        Some("https://her-blog.mosspub.com")
    );
}

#[test]
fn publishing_to_one_target_leaves_the_others_record_untouched() {
    let mut state = DeploymentState::default();
    state.site_id = Some("her-blog".to_string());
    state.targets.insert("moss:her-blog".to_string(), record("https://her-blog.mosspub.com"));
    let moss_before = state.targets["moss:her-blog"].clone();

    // Switch to onionpress and publish: read the flat view, update it the way
    // record_publish does, absorb it back.
    let mut flat = state.flat_view(Some("onionpress".to_string()), None);
    assert_eq!(flat.site_url, None, "an unpublished target reads as empty");
    flat.site_url = Some("http://abc.onion/".to_string());
    flat.last_deployment_url = Some("http://abc.onion/".to_string());
    state.absorb(&flat);

    assert_eq!(state.targets["moss:her-blog"], moss_before);
    // Switch back: the moss record answers as if nothing happened.
    let back = state.flat_view(Some("moss".to_string()), None);
    assert_eq!(back.last_deployment_url.as_deref(), Some("https://her-blog.mosspub.com"));
    assert_eq!(back.publish_target().as_deref(), Some("moss:her-blog"));
}

#[test]
fn publish_target_derives_from_selector_never_from_site_url() {
    let config = DomainDeploymentConfig {
        deploy_method: Some("ipfs".to_string()),
        site_url: Some("https://gateway/cid-that-changes-every-deploy".to_string()),
        ..Default::default()
    };
    assert_eq!(config.publish_target().as_deref(), Some("ipfs"));
}

#[test]
fn absorbing_an_all_none_flat_view_mints_no_target_record() {
    let mut state = DeploymentState::default();
    state.absorb(&DomainDeploymentConfig {
        deploy_method: Some("ipfs".to_string()),
        domain: Some("example.com".to_string()),
        ..Default::default()
    });
    assert!(state.targets.is_empty(), "a selection with no publish is not a record");
    assert!(!state.records_publish_evidence());
}

#[test]
fn unfileable_legacy_evidence_survives_a_save_round_trip() {
    // A legacy record with no method and no site_id names no slot — the
    // record is dropped, but the evidence must outlive the save that retires
    // the legacy keys, or the nested-site guard deletes a published .moss.
    let state =
        DeploymentState::from_toml(Some(&toml::from_str("site_url = \"https://u.github.io\"").unwrap()))
            .unwrap();
    assert!(state.records_publish_evidence());

    let reread = DeploymentState::from_toml(Some(
        &toml::from_str(&toml::to_string(&state).unwrap()).unwrap(),
    ))
    .unwrap();
    assert!(reread.records_publish_evidence(), "evidence must persist, not live in memory");
}

#[test]
fn legacy_dns_fields_lift_into_one_observation_and_never_persist_flat() {
    let mut state: DeploymentState = toml::from_str(
        r#"
        site_id = "her-blog"
        domain = "um-chps.org"
        dns_configured = true
        dns_configured_at = "2026-01-01T00:00:00Z"
        "#,
    )
    .unwrap();
    state.migrate_legacy();

    let observed = state.observed.as_ref().expect("legacy dns fields lift into observed");
    assert!(observed.dns_configured);
    assert_eq!(observed.checked_at, "2026-01-01T00:00:00Z");

    // The next save writes the observation record and none of the flat keys.
    // `domain` is authored intent — it never re-persists to state.toml; the
    // open-hook migration carries it into config.toml instead.
    let out = toml::to_string(&state).unwrap();
    assert!(out.contains("[observed]"), "observation must persist: {out}");
    assert!(!out.contains("dns_configured_at"), "legacy key re-persisted: {out}");
    assert!(!out.contains("domain"), "authored intent leaked back into state.toml: {out}");
}

#[test]
fn flat_view_derives_dns_fields_from_the_observation() {
    let mut state = DeploymentState::default();
    state.observed = Some(DomainObservation {
        checked_at: "2026-01-01T00:00:00Z".to_string(),
        dns_configured: true,
        cdn_status: Some("active".to_string()),
        ..Default::default()
    });
    let flat = state.flat_view(None, Some("um-chps.org".to_string()));
    assert_eq!(flat.domain.as_deref(), Some("um-chps.org"));
    assert!(flat.dns_configured);
    assert_eq!(flat.dns_configured_at.as_deref(), Some("2026-01-01T00:00:00Z"));
    assert_eq!(flat.cdn_status.as_deref(), Some("active"));

    // And it survives the disk round trip — additive + serde-default is the
    // whole versioning story for this record.
    let out = toml::to_string(&state).unwrap();
    let back = DeploymentState::from_toml(Some(&out.parse::<toml::Value>().unwrap())).unwrap();
    assert_eq!(back.observed.unwrap().cdn_status.as_deref(), Some("active"));

    // Absorbing the flat view writes none of the derived fields back — the
    // observation has exactly one writer.
    state.observed = None;
    state.absorb(&flat);
    assert!(state.observed.is_none(), "absorb must not re-mint the observation");
}

#[test]
fn domain_reconcile_adopts_only_the_single_unambiguous_case() {
    let one = vec!["um-chps.org".to_string()];
    let two = vec!["a.org".to_string(), "b.org".to_string()];

    // The one silent case: nothing authored, exactly one server domain.
    assert_eq!(
        reconcile_domain(None, &one),
        DomainReconciliation::Adopt("um-chps.org".to_string())
    );
    assert_eq!(reconcile_domain(Some("  "), &one), DomainReconciliation::Adopt("um-chps.org".to_string()));

    // Ambiguity or nothing to adopt: settled, no write.
    assert_eq!(reconcile_domain(None, &two), DomainReconciliation::Settled);
    assert_eq!(reconcile_domain(None, &[]), DomainReconciliation::Settled);

    // Agreement, or an authored domain the server hasn't heard of yet.
    assert_eq!(reconcile_domain(Some("um-chps.org"), &one), DomainReconciliation::Settled);
    assert_eq!(reconcile_domain(Some("um-chps.org"), &[]), DomainReconciliation::Settled);

    // Disagreement surfaces; it is never auto-resolved.
    assert_eq!(
        reconcile_domain(Some("other.org"), &one),
        DomainReconciliation::Conflict {
            authored: "other.org".to_string(),
            server: one.clone(),
        }
    );
}

#[test]
fn an_address_kind_this_moss_does_not_know_degrades_instead_of_failing() {
    // A plugin from a newer registry names a kind we have never heard of.
    // The whole deploy result must still deserialize — the label and value
    // are what the reader needs; the kind only picks an affordance.
    let json = r#"{"kind":"swarm","label":"Swarm hash","value":"abc123"}"#;
    let address: DeployAddress = serde_json::from_str(json).unwrap();
    assert_eq!(address.kind, AddressKind::Other);
    assert_eq!(address.label, "Swarm hash");
    assert_eq!(address.value, Some("abc123".to_string()));
}

#[test]
fn addresses_ride_the_flat_view_in_both_directions() {
    let mut state = DeploymentState::default();
    let mut flat = state.flat_view(Some("ipfs".to_string()), None);
    flat.addresses = vec![DeployAddress {
        kind: AddressKind::Ipns,
        label: "IPNS".to_string(),
        url: Some("https://ipfs.io/ipns/k51".to_string()),
        value: Some("k51".to_string()),
        note: None,
    }];
    state.absorb(&flat);

    let back = state.flat_view(Some("ipfs".to_string()), None);
    assert_eq!(back.addresses.len(), 1);
    assert_eq!(back.addresses[0].kind, AddressKind::Ipns);

    // Filed under the target that published them: another target's view is
    // not a place the reader's IPNS name should appear.
    let other = state.flat_view(Some("github-pages".to_string()), None);
    assert!(other.addresses.is_empty());
}

#[test]
fn onion_host_accepts_only_onion_site_urls() {
    let cfg = |url: Option<&str>| DomainDeploymentConfig {
        site_url: url.map(str::to_string),
        ..Default::default()
    };
    assert_eq!(cfg(Some("http://abcdef.onion")).onion_host().as_deref(), Some("abcdef.onion"));
    assert_eq!(cfg(Some("http://abcdef.onion/path")).onion_host().as_deref(), Some("abcdef.onion"));
    // A seta-hosted site_url must never masquerade as an onion address.
    assert_eq!(cfg(Some("https://chps.mosspub.com")).onion_host(), None);
    assert_eq!(cfg(Some(".onion")).onion_host(), None);
    assert_eq!(cfg(None).onion_host(), None);
}

// ============================================================================
// live_custom_domain_url — the single domain-preference rule
// ============================================================================

#[test]
fn live_custom_domain_url_returns_domain_once_dns_configured() {
    let config = DomainDeploymentConfig {
        domain: Some("example.com".to_string()),
        dns_configured: true,
        ..Default::default()
    };
    assert_eq!(
        live_custom_domain_url(&config),
        Some("https://example.com".to_string())
    );
}

#[test]
fn live_custom_domain_url_none_while_dns_unverified() {
    // A configured-but-unverified domain doesn't serve the site yet —
    // pointing "View site" at it would land on NXDOMAIN or a parked page.
    let config = DomainDeploymentConfig {
        domain: Some("example.com".to_string()),
        dns_configured: false,
        ..Default::default()
    };
    assert_eq!(live_custom_domain_url(&config), None);
}

#[test]
fn live_custom_domain_url_none_without_domain() {
    let config = DomainDeploymentConfig {
        dns_configured: true,
        ..Default::default()
    };
    assert_eq!(live_custom_domain_url(&config), None);
}

#[test]
fn live_custom_domain_url_none_for_blank_domain() {
    let config = DomainDeploymentConfig {
        domain: Some("   ".to_string()),
        dns_configured: true,
        ..Default::default()
    };
    assert_eq!(live_custom_domain_url(&config), None);
}
