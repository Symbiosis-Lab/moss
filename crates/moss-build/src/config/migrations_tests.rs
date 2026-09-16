use super::*;

#[test]
fn unversioned_config_is_treated_as_v0() {
    let mut raw: toml::Table = toml::from_str(
        r#"
            [hooks]
            syndicate = ["email"]
        "#,
    )
    .unwrap();
    let migrated = migrate_to_current(&mut raw).unwrap();
    assert!(migrated, "should report a migration happened");
    assert_eq!(
        raw.get("schema_version").and_then(|v| v.as_integer()),
        Some(CURRENT_VERSION as i64)
    );
}

#[test]
fn current_version_config_is_unchanged() {
    let original_str = format!(
        r#"
            schema_version = {}

            [channels.email]
        "#,
        CURRENT_VERSION
    );
    let mut raw: toml::Table = toml::from_str(&original_str).unwrap();
    let before = raw.clone();
    let migrated = migrate_to_current(&mut raw).unwrap();
    assert!(!migrated, "should not migrate at current version");
    assert_eq!(raw, before);
}

#[test]
fn version_ahead_is_rejected() {
    let mut raw: toml::Table =
        toml::from_str(&format!("schema_version = {}", CURRENT_VERSION + 10)).unwrap();
    let err = migrate_to_current(&mut raw).unwrap_err();
    assert!(
        matches!(err, MigrationError::VersionAhead(_)),
        "got {:?}",
        err
    );
}

#[test]
fn version_ahead_predicate_flags_only_a_newer_declared_version() {
    let ahead: toml::Table =
        toml::from_str(&format!("schema_version = {}", CURRENT_VERSION + 1)).unwrap();
    assert_eq!(version_ahead(&ahead), Some(CURRENT_VERSION + 1));

    let current: toml::Table =
        toml::from_str(&format!("schema_version = {}", CURRENT_VERSION)).unwrap();
    assert_eq!(version_ahead(&current), None);

    let behind: toml::Table = toml::from_str("schema_version = 0").unwrap();
    assert_eq!(version_ahead(&behind), None);

    // No key at all — e.g. `.moss/state.toml`, which shares the managed-TOML
    // write primitive but carries no top-level `schema_version` of its own.
    // `write_managed_toml`'s guard relies on this reading as "not ahead".
    let untagged: toml::Table = toml::from_str("[deployment]\nsite_id = \"x\"").unwrap();
    assert_eq!(version_ahead(&untagged), None);
}

#[test]
fn migrate_to_current_is_idempotent() {
    let mut raw: toml::Table = toml::from_str("[hooks]\nsyndicate = [\"email\"]").unwrap();
    assert!(migrate_to_current(&mut raw).unwrap());
    let snapshot = raw.clone();
    assert!(
        !migrate_to_current(&mut raw).unwrap(),
        "second call must be no-op"
    );
    assert_eq!(raw, snapshot);
}

// --- v0_to_v1 behavior ---

#[test]
fn v0_to_v1_moves_syndicate_ids_to_channels_tables() {
    let mut raw: toml::Table = toml::from_str(
        r#"
            [hooks]
            syndicate = ["email", "matters"]
            process = ["foo"]
        "#,
    )
    .unwrap();

    migrate_to_current(&mut raw).unwrap();

    let channels = raw
        .get("channels")
        .and_then(|v| v.as_table())
        .expect("channels table");
    assert!(channels.get("email").and_then(|v| v.as_table()).is_some());
    assert!(channels.get("matters").and_then(|v| v.as_table()).is_some());

    let hooks = raw
        .get("hooks")
        .and_then(|v| v.as_table())
        .expect("hooks still exists");
    assert!(hooks.get("syndicate").is_none());
    assert!(
        hooks.get("process").is_some(),
        "non-syndicate hooks preserved"
    );
}

#[test]
fn v0_to_v1_drops_empty_hooks_table() {
    let mut raw: toml::Table = toml::from_str(
        r#"
            [hooks]
            syndicate = ["email"]
        "#,
    )
    .unwrap();

    migrate_to_current(&mut raw).unwrap();

    assert!(raw.get("hooks").is_none(), "hooks table removed when empty");
}

#[test]
fn v0_to_v1_preserves_existing_channels_table() {
    let mut raw: toml::Table = toml::from_str(
        r#"
            [hooks]
            syndicate = ["email"]

            [channels.matters]
            already_set = true
        "#,
    )
    .unwrap();

    migrate_to_current(&mut raw).unwrap();

    let channels = raw.get("channels").and_then(|v| v.as_table()).unwrap();
    assert!(channels.get("email").is_some());
    let matters = channels.get("matters").and_then(|v| v.as_table()).unwrap();
    assert_eq!(
        matters.get("already_set").and_then(|v| v.as_bool()),
        Some(true)
    );
}

#[test]
fn v0_to_v1_handles_no_syndicate_array() {
    let mut raw: toml::Table = toml::from_str(
        r#"
            [hooks]
            process = ["foo"]
        "#,
    )
    .unwrap();

    migrate_to_current(&mut raw).unwrap();

    assert!(raw.get("channels").is_none(), "no channels section created");
    assert!(raw
        .get("hooks")
        .and_then(|v| v.as_table())
        .unwrap()
        .get("process")
        .is_some());
}

#[test]
fn v0_to_v1_rejects_non_table_channels_value() {
    // A user who hand-wrote `channels = "email"` puts a string where migration expects a table.
    // Migration must error with a clear shape message rather than silently clobbering or panicking.
    let mut raw: toml::Table = toml::from_str(
        r#"
            channels = "not-a-table"

            [hooks]
            syndicate = ["email"]
        "#,
    )
    .unwrap();

    let err = migrate_to_current(&mut raw).unwrap_err();
    assert!(
        matches!(err, MigrationError::InvalidShape(_)),
        "got {:?}",
        err
    );
}

// --- v1_to_v2: services shape rewrite ---

#[test]
fn v1_to_v2_promotes_analytics_section() {
    let mut raw: toml::Table = toml::from_str(
        r#"
            schema_version = 1
            [analytics]
            script = "https://guo.goatcounter.com/count"
        "#,
    )
    .unwrap();
    migrate_to_current(&mut raw).unwrap();
    let analytics = raw
        .get("services")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("analytics"))
        .and_then(|v| v.as_table())
        .expect("services.analytics");
    assert_eq!(
        analytics.get("script").and_then(|v| v.as_str()),
        Some("https://guo.goatcounter.com/count")
    );
    assert_eq!(
        analytics.get("provider").and_then(|v| v.as_str()),
        Some("goatcounter")
    );
    assert!(raw.get("analytics").is_none());
}

#[test]
fn v1_to_v2_promotes_email_section_preserving_api_key() {
    let mut raw: toml::Table = toml::from_str(
        r#"
            schema_version = 1
            [email]
            api_key = "btd-abc123"
        "#,
    )
    .unwrap();
    migrate_to_current(&mut raw).unwrap();
    let email = raw
        .get("services")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("email"))
        .and_then(|v| v.as_table())
        .expect("services.email");
    assert_eq!(
        email.get("api_key").and_then(|v| v.as_str()),
        Some("btd-abc123")
    );
    assert_eq!(
        email.get("provider").and_then(|v| v.as_str()),
        Some("buttondown")
    );
    assert!(raw.get("email").is_none());
}

#[test]
fn v1_to_v2_drops_redundant_features_toggles() {
    let mut raw: toml::Table = toml::from_str(
        r#"
            schema_version = 1
            [features]
            comments = true
            analytics = true
            subscribe = true
        "#,
    )
    .unwrap();
    migrate_to_current(&mut raw).unwrap();
    assert!(
        raw.get("features").is_none(),
        "[features] table removed when only redundant toggles present"
    );
}

#[test]
fn v1_to_v2_preserves_explicit_disable_on_comments() {
    let mut raw: toml::Table = toml::from_str(
        r#"
            schema_version = 1
            [comments]
            server_url = "https://api.moss.host/comments"
            [features]
            comments = false
        "#,
    )
    .unwrap();
    migrate_to_current(&mut raw).unwrap();
    let comments = raw
        .get("services")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("comments"))
        .and_then(|v| v.as_table())
        .expect("services.comments");
    assert_eq!(
        comments.get("enabled").and_then(|v| v.as_bool()),
        Some(false),
        "explicit features.comments=false preserved as services.comments.enabled=false"
    );
}

#[test]
fn v1_to_v2_moves_rss_to_site_rss_footer() {
    let mut raw: toml::Table = toml::from_str(
        r#"
            schema_version = 1
            [features]
            rss = true
        "#,
    )
    .unwrap();
    migrate_to_current(&mut raw).unwrap();
    let site = raw
        .get("site")
        .and_then(|v| v.as_table())
        .expect("site table");
    assert_eq!(site.get("rss_footer").and_then(|v| v.as_bool()), Some(true));
    assert!(raw.get("features").is_none());
}

#[test]
fn v1_to_v2_rss_footer_overrides_legacy_rss_when_both_present() {
    let mut raw: toml::Table = toml::from_str(
        r#"
            schema_version = 1
            [features]
            rss = true
            rss_footer = false
        "#,
    )
    .unwrap();
    migrate_to_current(&mut raw).unwrap();
    assert_eq!(
        raw.get("site")
            .and_then(|v| v.get("rss_footer"))
            .and_then(|v| v.as_bool()),
        Some(false),
        "explicit rss_footer wins over legacy rss"
    );
}

#[test]
fn v1_to_v2_drops_half_built_services_endpoint_fields() {
    // Pre-v2, [services] had a flat shape (endpoint, comments_endpoint, ...).
    // Those fields are dropped; if nothing else lived under [services], the
    // whole table is removed so later per-kind insertions start clean.
    let mut raw: toml::Table = toml::from_str(
        r#"
            schema_version = 1
            [services]
            endpoint = "https://api.example.com"
            comments_endpoint = "https://comments.example.com"
        "#,
    )
    .unwrap();
    migrate_to_current(&mut raw).unwrap();
    // No legacy [comments]/[analytics]/[email] to promote here, so [services]
    // should be gone entirely after the scrub.
    assert!(
        raw.get("services").is_none(),
        "[services] with only flat endpoint fields should be dropped entirely, got: {:?}",
        raw.get("services")
    );
}

#[test]
fn v1_to_v2_preserves_hand_written_services_comments() {
    // A user at v1 could have hand-written [services.comments]. The v1_to_v2 migration
    // must NOT clobber their provider/server_url (first-writer-wins). However, v3_to_v4
    // then applies: provider is always dropped (Artalk is the only provider), and a
    // custom (non-moss-operated) server_url is preserved as a self-hosted override.
    let mut raw: toml::Table = toml::from_str(
        r#"
            schema_version = 1
            [services.comments]
            provider = "waline"
            server_url = "https://user-chosen.example"
            [comments]
            server_url = "https://legacy.example"
        "#,
    )
    .unwrap();
    migrate_to_current(&mut raw).unwrap();
    let comments = raw
        .get("services")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("comments"))
        .and_then(|v| v.as_table())
        .expect("services.comments");
    assert!(
        comments.get("provider").is_none(),
        "provider dropped by v3_to_v4 (always Artalk)"
    );
    assert_eq!(
        comments.get("server_url").and_then(|v| v.as_str()),
        Some("https://user-chosen.example"),
        "custom (non-moss-operated) server_url preserved as self-hosted override"
    );
    assert!(raw.get("comments").is_none(), "legacy [comments] dropped");
}

#[test]
fn v1_to_v2_endpoint_scrub_coexists_with_comments_promotion() {
    // A v1 config can carry the half-built [services].endpoint AND a
    // legacy [comments] section. The scrub drops endpoint; the promotion
    // then creates [services.comments] cleanly.
    let mut raw: toml::Table = toml::from_str(
        r#"
            schema_version = 1
            [services]
            endpoint = "https://drop-me.example"
            [comments]
            server_url = "https://keep-me.example"
        "#,
    )
    .unwrap();
    migrate_to_current(&mut raw).unwrap();
    let services = raw
        .get("services")
        .and_then(|v| v.as_table())
        .expect("services");
    assert!(services.get("endpoint").is_none(), "flat endpoint scrubbed");
    let comments = services
        .get("comments")
        .and_then(|v| v.as_table())
        .expect("services.comments");
    assert_eq!(
        comments.get("server_url").and_then(|v| v.as_str()),
        Some("https://keep-me.example")
    );
}

#[test]
fn v1_to_v2_liu_guo_worked_example() {
    // End-to-end: the 刘果 legacy config goes from pre-v1 through v2 in one call.
    let mut raw: toml::Table = toml::from_str(
        r#"
            [analytics]
            script = "https://guo.goatcounter.com/count"

            [comments]
            server_url = "https://api.moss.host/comments"

            [email]
            api_key = "71a1403a-7c4f-4818-aa5a-1d75366684c2"

            [features]
            comments = true
            rss = true
            subscribe = true

            [hooks]
            syndicate = ["email", "matters"]
        "#,
    )
    .unwrap();

    migrate_to_current(&mut raw).unwrap();

    // schema_version stamped at CURRENT_VERSION.
    assert_eq!(
        raw.get("schema_version").and_then(|v| v.as_integer()),
        Some(CURRENT_VERSION as i64)
    );

    // Services present and wired.
    let services = raw
        .get("services")
        .and_then(|v| v.as_table())
        .expect("services");
    assert!(services.get("analytics").is_some());
    assert!(services.get("comments").is_some());
    assert!(services.get("email").is_some());

    // [channels.email] and [channels.matters] created by v0_to_v1.
    let channels = raw
        .get("channels")
        .and_then(|v| v.as_table())
        .expect("channels");
    assert!(channels.get("email").is_some());
    assert!(channels.get("matters").is_some());

    // [features], [analytics], [comments], [email] top-level all removed.
    assert!(raw.get("features").is_none());
    assert!(raw.get("analytics").is_none());
    assert!(raw.get("comments").is_none());
    assert!(raw.get("email").is_none());

    // [site].rss_footer reflects legacy features.rss.
    let rss_footer = raw
        .get("site")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("rss_footer"))
        .and_then(|v| v.as_bool());
    assert_eq!(rss_footer, Some(true));
}

// --- v2_to_v3: figure-caption migration is a no-op stamp ---
//
// The original v2_to_v3 wrote `[site].implicit_figure = false` to opt
// pre-existing sites out of the Pandoc-style implicit-figure rule. We
// dropped that opt-out — every site gets the new behavior via
// absence-as-true at the build pipeline. The version bump itself stays
// so already-migrated configs (some of which still have
// `implicit_figure = false` written from before this decision) keep
// validating without `VersionAhead` errors.

#[test]
fn v2_to_v3_does_not_write_implicit_figure() {
    let mut raw: toml::Table = toml::from_str(
        r#"
            schema_version = 2
            [site]
            lang = "en"
        "#,
    )
    .unwrap();
    migrate_to_current(&mut raw).unwrap();
    let site = raw
        .get("site")
        .and_then(|v| v.as_table())
        .expect("site table");
    assert!(
        site.get("implicit_figure").is_none(),
        "v2_to_v3 must not write implicit_figure (default-on via absence)"
    );
    // Existing keys untouched.
    assert_eq!(site.get("lang").and_then(|v| v.as_str()), Some("en"));
}

#[test]
fn v2_to_v3_does_not_create_site_table_when_missing() {
    let mut raw: toml::Table = toml::from_str(
        r#"
            schema_version = 2
        "#,
    )
    .unwrap();
    migrate_to_current(&mut raw).unwrap();
    // The no-op migration must not synthesize an empty [site] table.
    assert!(
        raw.get("site").is_none(),
        "v2_to_v3 must not create [site] table when no other migration needs it"
    );
}

#[test]
fn v3_to_v4_drops_provider_and_moss_operated_server_url() {
    let mut raw: toml::Table = toml::from_str(
        r#"
schema_version = 3
[services.comments]
provider = "artalk"
server_url = "https://api.moss.host/comments"
"#,
    )
    .unwrap();
    assert!(migrate_to_current(&mut raw).unwrap());
    let comments = raw.get("services").unwrap().get("comments").unwrap();
    assert!(comments.get("provider").is_none());
    assert!(comments.get("server_url").is_none());
    assert_eq!(
        raw.get("schema_version").unwrap().as_integer(),
        Some(CURRENT_VERSION as i64)
    );
}

#[test]
fn v3_to_v4_preserves_custom_server_url() {
    let mut raw: toml::Table = toml::from_str(
        r#"
schema_version = 3
[services.comments]
provider = "artalk"
server_url = "https://comments.example.org"
"#,
    )
    .unwrap();
    assert!(migrate_to_current(&mut raw).unwrap());
    let comments = raw.get("services").unwrap().get("comments").unwrap();
    assert!(comments.get("provider").is_none());
    assert_eq!(
        comments.get("server_url").unwrap().as_str(),
        Some("https://comments.example.org")
    );
}

#[test]
fn v3_to_v4_no_comments_section_is_noop() {
    let mut raw: toml::Table =
        toml::from_str("schema_version = 3\n[site]\nlang = \"en\"\n").unwrap();
    assert!(migrate_to_current(&mut raw).unwrap());
    assert_eq!(
        raw.get("schema_version").unwrap().as_integer(),
        Some(CURRENT_VERSION as i64)
    );
    assert!(raw.get("services").is_none());
}

#[test]
fn v3_to_v4_preserves_url_with_mosspub_in_path() {
    let mut raw: toml::Table = toml::from_str(
            "schema_version = 3\n[services.comments]\nserver_url = \"https://self.example.org/via/api.mosspub.com/relay\"\n",
        )
        .unwrap();
    assert!(migrate_to_current(&mut raw).unwrap());
    let comments = raw.get("services").unwrap().get("comments").unwrap();
    assert_eq!(
        comments.get("server_url").unwrap().as_str(),
        Some("https://self.example.org/via/api.mosspub.com/relay")
    );
}

#[test]
fn v3_to_v4_drops_uppercase_moss_operated_server_url() {
    // is_moss_operated_comment_host must be case-insensitive — an uppercase
    // URL (e.g. copy-pasted from a browser address bar) must still be stripped.
    let mut raw: toml::Table = toml::from_str(
            "schema_version = 3\n[services.comments]\nserver_url = \"HTTPS://API.MOSSPUB.COM/comments\"\n",
        )
        .unwrap();
    assert!(migrate_to_current(&mut raw).unwrap());
    let comments = raw.get("services").unwrap().get("comments").unwrap();
    assert!(
        comments.get("server_url").is_none(),
        "uppercase moss-operated URL must be stripped"
    );
}
