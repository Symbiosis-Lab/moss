//! Bringing an older `.moss/config.toml` up to `CURRENT_VERSION` — in memory.
//!
//! See `docs/reference/config-migrations.md` for the convention. Typed structs
//! elsewhere describe the current shape only; this module owns all transforms
//! from older versions to [`CURRENT_VERSION`].
//!
//! # Why the transform is here and the saving is not
//!
//! [`migrate_to_current`] rewrites a `toml::Table`. It opens nothing, writes
//! nothing, and cannot lose a byte of the user's file — so it travels with the
//! reader into the open crate, where a `moss-cli` build can compute correct
//! values out of an old config
//! ([ADR-059](../../../../../docs/decisions/ADR-059-config-reader-and-migration-runner-after-the-crate-split.md)).
//!
//! Everything that reaches disk stays in the app crate, in
//! `infra::config_migrations`: reading the file, writing the `.bak-v{n}`
//! backup, saving the migrated text as a **surgical edit** through
//! `infra::toml_rewrite` (a re-serialization would delete the comments and key
//! order of a file the user hand-wrote — that cost a real site four lines of
//! explanation on 2026-08-07), and the one step that rewrites a second,
//! non-TOML file, `.moss/.gitignore`.

use std::fmt;

pub const CURRENT_VERSION: u32 = 6;

#[derive(Debug)]
pub enum MigrationError {
    InvalidShape(String),
    VersionAhead(u32),
}

impl fmt::Display for MigrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MigrationError::InvalidShape(msg) => write!(f, "invalid config shape: {}", msg),
            MigrationError::VersionAhead(v) => write!(f, "config schema_version {} is newer than supported {}", v, CURRENT_VERSION),
        }
    }
}

impl std::error::Error for MigrationError {}

type Migration = fn(&mut toml::Table) -> Result<(), MigrationError>;

const MIGRATIONS: &[Migration] = &[
    v0_to_v1,
    v1_to_v2,
    v2_to_v3,
    v3_to_v4,
    v4_to_v5,
    v5_to_v6,
];

// Compile-time guard: the migrations array must have exactly CURRENT_VERSION entries
// (one per version bump). If you bump CURRENT_VERSION, you must add a migration; if
// you add a migration, you must bump CURRENT_VERSION. This fails the build before
// any test runs, which is why there is no test saying the same thing.
const _: () = {
    assert!(MIGRATIONS.len() == CURRENT_VERSION as usize);
};

/// Run all pending migrations on `raw`. Returns `Ok(true)` if any migration
/// ran (caller should write a backup + save), `Ok(false)` if already current.
pub fn migrate_to_current(raw: &mut toml::Table) -> Result<bool, MigrationError> {
    let current = declared_version(raw);
    if current > CURRENT_VERSION {
        return Err(MigrationError::VersionAhead(current));
    }
    if current == CURRENT_VERSION {
        return Ok(false);
    }
    for step in &MIGRATIONS[current as usize..CURRENT_VERSION as usize] {
        step(raw)?;
    }
    write_version(raw, CURRENT_VERSION);
    Ok(true)
}


/// The version a config declares. An unversioned config is a v0 config —
/// which is what `unwrap_or(0)` says, and why absence is not an error here.
pub fn declared_version(raw: &toml::Table) -> u32 {
    raw.get("schema_version")
        .and_then(|v| v.as_integer())
        .and_then(|i| u32::try_from(i).ok())
        .unwrap_or(0)
}

/// `Some(v)` when `raw` declares a schema a newer moss wrote — the one
/// predicate every version-ahead guard shares, so a schema bump only has to
/// touch [`CURRENT_VERSION`] once. `None` covers "absent, at, or behind
/// current", which is every ordinary config `write_managed_toml` is asked to
/// save. A document with no top-level `schema_version` key (e.g.
/// `.moss/state.toml`) reads as v0 here, same as [`declared_version`], so
/// this is safe to run against any managed TOML document, not just
/// `config.toml`.
pub fn version_ahead(raw: &toml::Table) -> Option<u32> {
    let v = declared_version(raw);
    (v > CURRENT_VERSION).then_some(v)
}

fn write_version(raw: &mut toml::Table, v: u32) {
    raw.insert("schema_version".to_string(), toml::Value::Integer(v as i64));
}

/// v0 to v1: replace `[hooks].syndicate = [...]` with `[channels.<id>]` tables.
/// See `docs/archive/2026-04-23-channels-architecture-design.md`.
fn v0_to_v1(raw: &mut toml::Table) -> Result<(), MigrationError> {
    let syndicate_ids: Vec<String> = raw
        .get("hooks")
        .and_then(|h| h.as_table())
        .and_then(|t| t.get("syndicate"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();

    // Verify existing [channels] (if any) is a table before we touch it.
    if let Some(existing) = raw.get("channels") {
        if !existing.is_table() {
            return Err(MigrationError::InvalidShape(
                "'channels' exists but is not a table; expected [channels.<id>] tables".into(),
            ));
        }
    }

    if !syndicate_ids.is_empty() {
        // The shape check above already returned on a non-table `channels`,
        // and an absent key is a table this line just made.
        let channels_table = crate::vault::config::subtable(raw, "channels")
            .map_err(MigrationError::InvalidShape)?;
        for id in syndicate_ids {
            channels_table.entry(id).or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        }
    }

    // Strip syndicate from [hooks]. If [hooks] becomes empty, remove it.
    if let Some(hooks) = raw.get_mut("hooks").and_then(|v| v.as_table_mut()) {
        hooks.remove("syndicate");
    }
    let drop_hooks = raw
        .get("hooks")
        .and_then(|v| v.as_table())
        .map(|t| t.is_empty())
        .unwrap_or(false);
    if drop_hooks {
        raw.remove("hooks");
    }

    Ok(())
}

/// v1 to v2: replace `[features]` + top-level `[comments]` / `[analytics]` /
/// `[email]` sections with the uniform `[services.<kind>]` shape. Also moves
/// `features.rss` / `features.rss_footer` to `[site].rss_footer`.
///
/// See docs/archive/2026-04-24-services-schema-design.md for the design and
/// the full migration rules.
fn v1_to_v2(root: &mut toml::Table) -> Result<(), MigrationError> {
    // Capture legacy data up front (all immutable snapshots so we can
    // mutate `root` below without borrow conflicts).
    let legacy_comments_url: Option<String> = root
        .get("comments")
        .and_then(|v| v.get("server_url"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let legacy_analytics_script: Option<String> = root
        .get("analytics")
        .and_then(|v| v.get("script"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let legacy_email_api_key: Option<String> = root
        .get("email")
        .and_then(|v| v.get("api_key"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let features_comments_explicit_off: bool = root
        .get("features")
        .and_then(|v| v.get("comments"))
        .and_then(|v| v.as_bool())
        == Some(false);

    // rss_footer wins over legacy rss.
    let legacy_rss_footer: Option<bool> = root
        .get("features")
        .and_then(|v| v.get("rss_footer"))
        .and_then(|v| v.as_bool())
        .or_else(|| {
            root.get("features")
                .and_then(|v| v.get("rss"))
                .and_then(|v| v.as_bool())
        });


    // 1. Scrub the pre-v2 half-built flat fields from any existing [services].
    //    A v1 [services] table only ever held endpoint / comments_endpoint /
    //    subscribe_endpoint. We drop them — if it leaves [services] empty,
    //    remove it entirely so we don't confuse the per-kind insertion below.
    if let Some(services) = root.get_mut("services").and_then(|v| v.as_table_mut()) {
        services.remove("endpoint");
        services.remove("comments_endpoint");
        services.remove("subscribe_endpoint");
    }
    let drop_services = root
        .get("services")
        .and_then(|v| v.as_table())
        .map(|t| t.is_empty())
        .unwrap_or(false);
    if drop_services {
        root.remove("services");
    }

    // Helper: write a field under [services.<kind>] only if the user hasn't
    // already authored it at v1 (per reviewer: user-authored [services.comments]
    // must not be clobbered by migration defaults).
    fn set_if_absent(table: &mut toml::value::Table, key: &str, value: toml::Value) {
        table.entry(key.to_string()).or_insert(value);
    }

    // 2. Promote [comments] -> [services.comments].
    if let Some(url) = legacy_comments_url {
        let services = root
            .entry("services".to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()))
            .as_table_mut()
            .ok_or_else(|| MigrationError::InvalidShape("[services] is not a table".into()))?;
        let comments = services
            .entry("comments".to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()))
            .as_table_mut()
            .ok_or_else(|| {
                MigrationError::InvalidShape("[services.comments] is not a table".into())
            })?;
        set_if_absent(comments, "provider", toml::Value::String("artalk".to_string()));
        set_if_absent(comments, "server_url", toml::Value::String(url));
        if features_comments_explicit_off {
            comments.insert("enabled".to_string(), toml::Value::Boolean(false));
        }
        root.remove("comments");
    }

    // 3. Promote [analytics] -> [services.analytics].
    if let Some(script) = legacy_analytics_script {
        let services = root
            .entry("services".to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()))
            .as_table_mut()
            .ok_or_else(|| MigrationError::InvalidShape("[services] is not a table".into()))?;
        let analytics = services
            .entry("analytics".to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()))
            .as_table_mut()
            .ok_or_else(|| {
                MigrationError::InvalidShape("[services.analytics] is not a table".into())
            })?;
        set_if_absent(analytics, "provider", toml::Value::String("goatcounter".to_string()));
        set_if_absent(analytics, "script", toml::Value::String(script));
        root.remove("analytics");
    }

    // 4. Promote [email] -> [services.email].
    if let Some(api_key) = legacy_email_api_key {
        let services = root
            .entry("services".to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()))
            .as_table_mut()
            .ok_or_else(|| MigrationError::InvalidShape("[services] is not a table".into()))?;
        let email = services
            .entry("email".to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()))
            .as_table_mut()
            .ok_or_else(|| {
                MigrationError::InvalidShape("[services.email] is not a table".into())
            })?;
        set_if_absent(email, "provider", toml::Value::String("buttondown".to_string()));
        set_if_absent(email, "api_key", toml::Value::String(api_key));
        root.remove("email");
    }

    // 5. Move rss/rss_footer to [site].rss_footer.
    if let Some(rss_footer) = legacy_rss_footer {
        let site = root
            .entry("site".to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()))
            .as_table_mut()
            .ok_or_else(|| MigrationError::InvalidShape("[site] is not a table".into()))?;
        site.insert("rss_footer".to_string(), toml::Value::Boolean(rss_footer));
    }

    // 6. Drop [features] entirely — any remaining fields were redundant
    //    (comments/analytics/subscribe) or moved (rss/rss_footer).
    root.remove("features");

    Ok(())
}

/// v2 to v3: no-op stamp.
///
/// This step originally wrote `[site].implicit_figure = false` to opt
/// pre-existing sites out of the Pandoc-style implicit-figure rule shipped
/// in figure-captions Phase A. We've decided not to opt anyone out — every
/// site (new and pre-existing) gets the new behavior via the absence-as-true
/// default in the build pipeline's `SiteConfig` construction.
///
/// The version bump itself stays so already-migrated configs (which may have
/// `implicit_figure = false` written from before this decision) continue to
/// validate without `VersionAhead` errors. `or_insert` semantics in the old
/// step were first-writer-wins, so user-authored values were never clobbered;
/// the same applies retroactively when this step runs as a no-op.
///
/// See `docs/archive/2026-05-05-figure-captions-design.md`.
fn v2_to_v3(_raw: &mut toml::Table) -> Result<(), MigrationError> {
    Ok(())
}

/// True when `url`'s authority is a moss-operated host. Path components
/// embedding a moss domain (a self-hosted proxy like
/// `https://self.example.org/via/api.mosspub.com/relay`) must NOT match —
/// a migration silently dropping a custom value is data loss.
fn is_moss_operated_comment_host(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let authority = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .unwrap_or(lower.as_str())
        .split('/')
        .next()
        .unwrap_or("");
    let host = authority.split(':').next().unwrap_or("");
    host == "api.moss.host"
        || host == "api.mosspub.com"
        || host.ends_with(".moss.host")
        || host.ends_with(".mosspub.com")
}

/// v3 to v4: comments go environment-derived and Artalk-only.
///
/// - Drop `[services.comments] provider` — Waline support is removed; the
///   provider is always Artalk.
/// - Drop `server_url` when it points at a moss-operated host. The build now
///   resolves the comment server from `HostingEnvironment::comments_server_url()`,
///   so a stale stored value (api.moss.host died with the HK VPS, stranding
///   every site that froze it into config) can never strand a site again.
///   Genuinely self-hosted URLs are preserved as overrides.
///
/// See docs/archive/2026-06-10-comments-local-first-design.md §3.
fn v3_to_v4(raw: &mut toml::Table) -> Result<(), MigrationError> {
    let Some(comments) = raw
        .get_mut("services")
        .and_then(|s| s.get_mut("comments"))
        .and_then(|c| c.as_table_mut())
    else {
        return Ok(());
    };
    comments.remove("provider");
    let moss_operated = comments
        .get("server_url")
        .and_then(|v| v.as_str())
        .map(is_moss_operated_comment_host)
        .unwrap_or(false);
    if moss_operated {
        comments.remove("server_url");
    }
    Ok(())
}

/// v4 to v5: nothing changes in `config.toml`.
///
/// The actual v4→v5 work is on `.moss/.gitignore`, which is not TOML, so it
/// cannot be expressed as one of these `toml::Value` transforms. It is run
/// from [`ensure_config_migrated`] instead; this stamp is what records that it
/// has run. Keeping the array one-entry-per-version is what makes the
/// compile-time `MIGRATIONS.len() == CURRENT_VERSION` guard mean something.
/// `v2_to_v3` is the same shape for a different reason.
fn v4_to_v5(_raw: &mut toml::Table) -> Result<(), MigrationError> {
    Ok(())
}

/// v5 to v6: nothing changes in `config.toml`.
///
/// Same shape as [`v4_to_v5`], for the same reason: the work is on
/// `.moss/.gitignore` — pruning the `state.toml` and `deploy/` lines moss no
/// longer emits, so a vault's publish records can reach git and a fresh clone
/// knows what is live (moss#993). The stamp records that it has run.
fn v5_to_v6(_raw: &mut toml::Table) -> Result<(), MigrationError> {
    Ok(())
}

#[cfg(test)]
#[path = "migrations_tests.rs"]
mod tests;
