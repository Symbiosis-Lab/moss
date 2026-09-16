//! Deployment state vocabulary — the typed shape of `.moss/state.toml`'s
//! `[deployment]` table.
//!
//! Moved from the app crate (2026-08-27, M6a B3): `DomainDeploymentConfig`
//! from `domain/types.rs`, `DnsTarget`/`DnsRecord` from `plugins/types.rs`
//! (they are deployment-state vocabulary that the plugins module happened to
//! host — plugins *produce* DNS targets, the deployment record *stores* them).
//! The build tree threads a `DomainDeploymentConfig` value through slot
//! resolution and feature sync, so the type must be reachable from
//! `moss-build` when the pipeline crosses. Reading and writing `state.toml`
//! stays app-side (ADR-059's shape: vocabulary crosses, file access does not).

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// A single DNS record provided by deploy plugins
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[derive(specta::Type)]
pub struct DnsRecord {
    /// Record type: "A", "AAAA", "CNAME", "TXT", etc.
    pub record_type: String,
    /// Record name: "@" for apex, "www", etc.
    pub name: String,
    /// Record value: IP address or hostname
    pub value: String,
    /// Optional TTL in seconds
    #[serde(default)]
    pub ttl: Option<u32>,
}

/// DNS configuration provided by deploy plugins
///
/// Plugins are responsible for generating the appropriate DNS records
/// for their platform. moss just passes these through to DNS configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[derive(specta::Type)]
pub struct DnsTarget {
    /// List of DNS records to configure
    pub records: Vec<DnsRecord>,
}

/// What one address IS, which decides how moss offers it.
///
/// Closed for the kinds moss knows, open at the wire: a plugin from a newer
/// registry than this moss can name a kind we have never heard of, and that
/// must degrade to a plain labelled row rather than failing the whole deploy
/// result to deserialize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[derive(specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum AddressKind {
    /// A content identifier — immutable, names this exact publish.
    Cid,
    /// A mutable name that points at the latest publish.
    Ipns,
    /// An HTTP door onto content that is not natively HTTP.
    Gateway,
    /// A hostname the reader can type.
    Domain,
    /// A kind this moss does not know. Rendered as label + value, and
    /// persisted as `"other"` — the original word is not kept, because moss
    /// has nothing it could do with it.
    #[serde(other)]
    Other,
}

/// One way to reach the published site.
///
/// A publish produces several: the CID that names these exact bytes, the IPNS
/// name that will name the next ones too, the gateway URL that makes either
/// reachable from a browser. They are FACTS ABOUT THE SITE, not news about the
/// publish — which is why they live in the deployment record and render
/// standing in the deploy tab, instead of evaporating with the toast that
/// first announced them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[derive(specta::Type)]
pub struct DeployAddress {
    pub kind: AddressKind,
    /// Human label, the plugin's own words ("IPFS CID", "Gateway").
    pub label: String,
    /// Openable in a browser. Present for `gateway`/`domain`, usually absent
    /// for a bare `cid`.
    #[serde(default)]
    pub url: Option<String>,
    /// The literal string to copy — beside `url`, or on its own.
    #[serde(default)]
    pub value: Option<String>,
    /// One line of context the plugin wants shown beside it.
    #[serde(default)]
    pub note: Option<String>,
}

/// One timestamped snapshot of what was OBSERVED about this site's domain —
/// the server's custom-domain rows, and what the DNS-verification flow last
/// concluded. Lives at `[deployment.observed]` in state.toml, written as a
/// unit and deletable at any time with no loss: it drives UI confidence only,
/// never build output (the build's domain comes from the authored
/// `[site].domain` in config.toml).
///
/// This replaces four fields (`dns_configured`, `dns_configured_at`,
/// `dns_target`, and the per-target copy of `dns_target`) that were written
/// at different moments by different code paths and could disagree —
/// `dns_configured: true` beside a `dns_target` from a different domain was
/// representable. One record, one `checked_at`, cleared whole when the
/// domain changes.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct DomainObservation {
    /// When any part of this snapshot was last refreshed (ISO 8601).
    pub checked_at: String,
    /// The custom-domain rows seta holds for this site, as last fetched.
    /// Stored as the DURABLE trace of what the server said: the reconcile's
    /// log line and hook-summary note are transient, so this is what a later
    /// UI-confidence surface (or a human reading state.toml) has to go on
    /// when local and server disagree. Empty means "never fetched" as well
    /// as "none" — absence of the whole record already reads as unknown.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub server_domains: Vec<String>,
    /// The settled-connect verdict: the last check found every expected
    /// record serving AND the site answering over HTTPS. Written by the
    /// DNS-verification flow and by `probe_domain` when a reading settles —
    /// one field with the strict meaning, so the domain pill can seed a
    /// resting state from it on the next launch instead of replaying setup.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dns_configured: bool,
    /// seta's last word on this domain's CDN hostname (`active`,
    /// `pending_dcv`, …), as `get_domain_cdn` reported it. Stored raw; the
    /// frontend state machine owns turning it into a pill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cdn_status: Option<String>,
    /// The records the domain should carry, as the deploy target reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dns_target: Option<DnsTarget>,
}

/// The flat per-project view of deployment state that every caller reads and
/// writes — NOT the persisted shape. `get_domain_config` composes it from
/// [`DeploymentState`] (project-scoped fields + the ACTIVE target's
/// [`DeploymentRecord`]), and `save_domain_config` splits it back out, so a
/// publish to one target can never overwrite what moss knows about another.
///
/// `deploy_method` is DERIVED on read, never stored: `[hooks] deploy` in
/// config.toml is the user's selection, and a registered `site_id` with no
/// hook means moss hosting. See [`derive_deploy_method`]. `domain` and the
/// `dns_*` fields follow the same pattern since 2026-08-31: `domain` is
/// authored in config.toml (`[site].domain`), the `dns_*` trio derives from
/// [`DomainObservation`], and all four are dropped at write time — an
/// authored fact must not live in a file moss rewrites wholesale, and a
/// stored copy of either drifts from its source.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[derive(specta::Type)]
pub struct DomainDeploymentConfig {
    /// Custom domain for this project — the authored intent from
    /// `[site].domain` in config.toml, filled at read time.
    pub domain: Option<String>,
    /// Whether DNS has been configured for this domain — derived from
    /// [`DomainObservation::dns_configured`] at read time.
    #[serde(default)]
    pub dns_configured: bool,
    /// When the observation was last refreshed (ISO 8601) — derived from
    /// [`DomainObservation::checked_at`] at read time.
    pub dns_configured_at: Option<String>,
    /// seta's last-seen CDN status for the domain — derived from
    /// [`DomainObservation::cdn_status`] at read time, dropped at write.
    /// With `dns_configured`, what lets the pill seed a resting state.
    #[serde(default)]
    pub cdn_status: Option<String>,
    /// The deploy method in effect NOW — derived by [`derive_deploy_method`]
    /// at read time, dropped at write time. Kept on the flat view so the
    /// ~50 existing read sites (Rust and frontend, over IPC) keep one field
    /// to consult; state.toml no longer carries a copy that could drift from
    /// a hand-edited `[hooks] deploy`.
    pub deploy_method: Option<String>,
    /// URL of the last deployment
    pub last_deployment_url: Option<String>,
    /// Canonical public URL of a plugin deployment (e.g. an OnionPress
    /// `http://<addr>.onion`). Set by `record_publish` when a plugin
    /// deploy supplies its own canonical url; cleared when moss hosting deploys
    /// (which resolves its URL from `site_id`). `resolve_site_url` consults this
    /// AFTER `domain` (a bought custom domain still wins → the onion becomes an
    /// Onion-Location alternate) and BEFORE `site_id`, so an onion site bakes its
    /// real address into canonical/OG/RSS instead of localhost. Additive +
    /// serde-default so pre-existing `state.toml` files still deserialize.
    #[serde(default)]
    pub site_url: Option<String>,
    /// The OnionPress name claimed for this project (e.g. "her-blog", without
    /// the `.onion` suffix), if the user chose to claim one. `None` when the
    /// user skipped naming (moss#optional-name) — the site still publishes at
    /// its raw onion address (`site_url`), just without a friendly name.
    /// Set by `set_onion_name` at claim time; nothing clears it on skip, since
    /// skip never sets it in the first place. Additive + serde-default so
    /// pre-existing `state.toml` files still deserialize.
    #[serde(default)]
    pub onion_name: Option<String>,
    /// Timestamp of the last deployment (ISO 8601)
    pub last_deployment_at: Option<String>,
    /// DNS target information for custom domain configuration — derived from
    /// [`DomainObservation::dns_target`] at read time.
    pub dns_target: Option<DnsTarget>,
    /// Plugin-provided deployment metadata (e.g., repo_url, platform-specific info)
    #[serde(default)]
    pub metadata: Option<HashMap<String, String>>,
    /// mosspub.com site identifier (e.g., "her-blog" → her-blog.mosspub.com)
    #[serde(default)]
    pub site_id: Option<String>,
    /// Optional path to a pre-built static site directory (relative to the
    /// project folder, or absolute). When set, `moss deploy` skips its own
    /// build pipeline and uploads this directory as-is. Use for projects
    /// built by an external SSG (Quire, Hugo, Jekyll, Astro, etc.).
    #[serde(default)]
    pub prebuilt_output: Option<String>,
    /// Content-hash generation id of the last successfully deployed generation.
    /// Set by `vault::deployment_state::record_publish` on every successful
    /// deploy — all three publish paths reach it. Used by
    /// the deploy short-circuit to skip a redeploy when the server is already
    /// live on this generation.
    #[serde(default)]
    pub last_deployed_generation_id: Option<String>,
    /// The last OnionPress generation moss VERIFIED live over Tor (the
    /// plugin's `moss-<unix_seconds>` id from `metadata.generation` — NOT the
    /// seal id above, which is `None` on sealless publishes). Written by
    /// `stack_serving` at the moment of the live verdict, never at commit.
    /// A deployment whose `metadata.generation` differs from this is still
    /// pending verification and resumes on folder open; the pending state is
    /// derived from that comparison, never stored.
    #[serde(default)]
    pub last_verified_generation: Option<String>,
    /// Every way to reach the site, from the active target's last publish.
    /// See [`DeployAddress`].
    #[serde(default)]
    pub addresses: Vec<DeployAddress>,
}

impl DomainDeploymentConfig {
    /// The publish target this project ships to now, or `None` when nothing
    /// names one yet. Delegates to [`slot_for`] — the one identity function —
    /// so the read side, both record-writing publish paths, and the
    /// `DeploymentState::targets` key all agree byte for byte. A miss there
    /// means `published_record::load_for` (in the build tree's manifest
    /// module) degrades the change set to unclassified.
    pub fn publish_target(&self) -> Option<String> {
        let method =
            derive_deploy_method(self.deploy_method.as_deref(), self.site_id.as_deref())?;
        slot_for(&method, self.site_id.as_deref())
    }

    /// The bare `.onion` host persisted in `site_url` (`http://<addr>.onion`
    /// → `<addr>.onion`), or `None` when `site_url` is absent or not an onion
    /// URL — a seta-hosted `site_url` must never masquerade as an onion
    /// address. This crate owns the `site_url` format (see the field doc), so
    /// the parse lives here rather than at each consumer.
    pub fn onion_host(&self) -> Option<String> {
        let raw = self.site_url.as_deref()?.trim();
        let host = raw.trim_start_matches("http://").trim_start_matches("https://").split('/').next()?;
        (host.ends_with(".onion") && host.len() > ".onion".len()).then(|| host.to_string())
    }
}

/// The stable id for moss's own hosting, as it appears in a deploy method,
/// a target id, and `[hooks] deploy`. It is written into config.toml and read
/// back, so renaming it would strand every site already pointing at it.
pub const MOSS_TARGET_ID: &str = "moss";

/// The slot naming one publish target: `moss:<site_id>` for moss hosting,
/// the bare method for a plugin target. This is the ONE identity function —
/// the state map key, the publish-record filename, and `publish_target()`
/// all derive from it, and nothing may compose a target from `site_url`
/// (a content-addressed target like IPFS changes its URL on every deploy,
/// so URL-as-identity orphans the record it just wrote).
///
/// The surviving `match` arm encodes a real distinction: moss hosting
/// multiplexes sites per folder (`moss deploy --site-id=<name>` can register
/// a second site over the folder's lifetime), so the id half is load-bearing
/// there and half a target is no target. A plugin target does not multiplex;
/// if one ever needs to, it supplies its own durable id — nothing today does.
pub fn slot_for(method: &str, site_id: Option<&str>) -> Option<String> {
    match method {
        "moss" => site_id.map(|id| format!("moss:{id}")),
        _ => Some(method.to_string()),
    }
}

/// The method a slot names — `slot_for` read backwards, and it lives here so
/// the two cannot drift. Only the moss arm appends an id, so only a slot that
/// starts `moss:` is split; a plugin name is passed through whole, because
/// `[hooks] deploy` is user-authored and `deploy = "my:deployer"` would
/// otherwise be filed under a target called `my`.
pub fn target_for_slot(slot: &str) -> &str {
    match slot.split_once(':') {
        Some(("moss", _)) => "moss",
        _ => slot,
    }
}

/// The deploy method in effect — derived, never stored. `[hooks] deploy` in
/// config.toml is the user's decision and wins outright (pass the RAW key
/// here, not `get_hook_config`'s output, which injects the build-time
/// `DEFAULT_DEPLOYER` fallback); with no hook, a registered moss site
/// (`site_id` present) means moss hosting; with neither, no target is
/// configured and the next publish opens the first-publish flow.
///
/// state.toml used to carry a `deploy_method` copy of this decision, kept in
/// sync by `set_deploy_target`. A cache of user intent drifts the moment
/// someone hand-edits config.toml — a supported way to drive moss — so it
/// was deleted rather than reconciled: with nothing stored, there is nothing
/// left to drift.
pub fn derive_deploy_method(
    hooks_deploy: Option<&str>,
    site_id: Option<&str>,
) -> Option<String> {
    match hooks_deploy.map(str::trim) {
        Some(m) if !m.is_empty() => Some(m.to_string()),
        _ => site_id.map(|_| MOSS_TARGET_ID.to_string()),
    }
}

/// What moss knows about ONE publish target — everything a successful deploy
/// to that target writes. Lives at `[deployment.targets.<slot>]` in
/// state.toml, keyed by [`slot_for`], so publishing to target B structurally
/// cannot overwrite target A's record. Field meanings are documented on
/// [`DomainDeploymentConfig`], whose per-target half this is.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct DeploymentRecord {
    #[serde(default)]
    pub site_url: Option<String>,
    #[serde(default)]
    pub last_deployment_url: Option<String>,
    #[serde(default)]
    pub last_deployment_at: Option<String>,
    /// Legacy (read-only): DNS targets describe the DOMAIN, which is
    /// project-scoped — storing one per publish target was the modeling
    /// error this field's retirement fixes. Lifted into
    /// `[deployment.observed]` by [`DeploymentState::migrate_legacy`];
    /// `skip_serializing` retires it from disk on the next save.
    #[serde(default, skip_serializing)]
    pub dns_target: Option<DnsTarget>,
    #[serde(default)]
    pub metadata: Option<HashMap<String, String>>,
    #[serde(default)]
    pub last_deployed_generation_id: Option<String>,
    #[serde(default)]
    pub last_verified_generation: Option<String>,
    /// Every way to reach the site this target published, as the plugin
    /// reported them. Replaced wholesale by each publish: an address list is
    /// a snapshot of one deploy, never an accumulation across deploys.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub addresses: Vec<DeployAddress>,
}

impl DeploymentRecord {
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// The persisted shape of `.moss/state.toml [deployment]`: project-scoped
/// fields once, plus one [`DeploymentRecord`] per target ever published.
/// Only `get_domain_config` / `save_domain_config` (app side) touch this —
/// every other caller works on the flat [`DomainDeploymentConfig`] view.
///
/// The legacy flat fields at the bottom are the pre-targets schema
/// (one shared record that every target overwrote). They are read so
/// [`Self::migrate_legacy`] can lift them into `targets[<slot>]`, and
/// `skip_serializing` means the next save drops them from disk.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct DeploymentState {
    /// Legacy (read-only): the domain is authored intent and lives in
    /// config.toml `[site].domain` since 2026-08-31 — state.toml is a file
    /// moss rewrites wholesale, and an authored fact must not live there
    /// (the CPHS vault baked a mosspub canonical for months because this
    /// copy was absent locally while the server had the real domain). Read
    /// so the project-open migration can adopt it into config.toml;
    /// `skip_serializing` retires it from disk on the next save.
    #[serde(default, skip_serializing)]
    pub domain: Option<String>,
    /// Legacy (read-only): lifted into [`Self::observed`] by
    /// [`Self::migrate_legacy`], retired from disk on the next save.
    #[serde(default, skip_serializing)]
    dns_configured: bool,
    #[serde(default, skip_serializing)]
    dns_configured_at: Option<String>,
    /// What was last observed about this site's domain — see
    /// [`DomainObservation`]. Replaced as a unit, deletable with no loss.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed: Option<DomainObservation>,
    #[serde(default)]
    pub site_id: Option<String>,
    #[serde(default)]
    pub onion_name: Option<String>,
    #[serde(default)]
    pub prebuilt_output: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub targets: BTreeMap<String, DeploymentRecord>,

    // ---- legacy flat schema (read-only; never written back) ----
    #[serde(default, skip_serializing)]
    deploy_method: Option<String>,
    #[serde(default, skip_serializing)]
    site_url: Option<String>,
    #[serde(default, skip_serializing)]
    last_deployment_url: Option<String>,
    #[serde(default, skip_serializing)]
    last_deployment_at: Option<String>,
    #[serde(default, skip_serializing)]
    dns_target: Option<DnsTarget>,
    #[serde(default, skip_serializing)]
    metadata: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing)]
    last_deployed_generation_id: Option<String>,
    #[serde(default, skip_serializing)]
    last_verified_generation: Option<String>,
    /// Set by [`Self::migrate_legacy`] when a legacy record or stored
    /// selection existed but named no slot to file it under. The record is
    /// gone from the struct, but it WAS publish evidence — see
    /// [`Self::records_publish_evidence`]. Persisted (unlike the legacy
    /// fields it summarizes), so the evidence survives the save that retires
    /// them from disk.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    legacy_trace: bool,
}

impl DeploymentState {
    /// Parse a `[deployment]` table (or its absence) into the persisted
    /// shape, migration included. The ONLY way to obtain a `DeploymentState`
    /// from TOML — both app-side readers go through here, so a caller cannot
    /// parse without migrating.
    pub fn from_toml(deployment: Option<&toml::Value>) -> Result<Self, String> {
        let mut state: Self = deployment
            .cloned()
            .map(|v| v.try_into())
            .transpose()
            .map_err(|e| format!("Failed to parse deployment config: {}", e))?
            .unwrap_or_default();
        state.migrate_legacy();
        Ok(state)
    }

    /// Lift a legacy flat `[deployment]` into `targets[<slot>]`. The slot is
    /// derived from the legacy `deploy_method` (the last stored selection),
    /// falling back to moss when only a `site_id` exists. A `targets` map
    /// already on disk wins — the legacy fields are then stale leftovers a
    /// pre-migration moss wrote, and lifting them would clobber a real record.
    /// Legacy fields are cleared either way; `skip_serializing` retires them
    /// from disk on the next save.
    fn migrate_legacy(&mut self) {
        // The flat legacy `dns_target` feeds the observation, not a target
        // record — DNS targets describe the domain, which is project-scoped.
        let legacy_flat_dns_target = self.dns_target.take();
        self.lift_targets(legacy_flat_dns_target.is_some());
        self.lift_observation(legacy_flat_dns_target);
    }

    /// The pre-targets flat-record lift (see [`Self::migrate_legacy`]'s doc).
    /// `had_dns_target` keeps the publish-evidence reading of the old schema:
    /// a flat block whose only content was a `dns_target` still counted.
    fn lift_targets(&mut self, had_dns_target: bool) {
        let legacy = DeploymentRecord {
            site_url: self.site_url.take(),
            last_deployment_url: self.last_deployment_url.take(),
            last_deployment_at: self.last_deployment_at.take(),
            dns_target: None,
            metadata: self.metadata.take(),
            last_deployed_generation_id: self.last_deployed_generation_id.take(),
            last_verified_generation: self.last_verified_generation.take(),
            // Never persisted by a pre-targets moss — the field is newer than
            // the schema this migration reads.
            addresses: Vec::new(),
        };
        let method = self.deploy_method.take();
        if !self.targets.is_empty() {
            return;
        }
        if legacy.is_empty() {
            // A stored legacy selection with nothing recorded still marks the
            // folder as deployment-touched — evidence, with nothing to file.
            // So does a flat block whose only content was a `dns_target`,
            // which now files under the observation rather than a record.
            // `|=`: the flag only ever latches on — a re-parse of state that
            // already carries `legacy_trace = true` must not clear it.
            self.legacy_trace |= method.is_some() || had_dns_target;
            return;
        }
        let method = match method {
            Some(m) => m,
            None if self.site_id.is_some() => "moss".to_string(),
            // Per-target fields with no method and no site_id name half a
            // target; there is no slot to file them under.
            None => {
                self.legacy_trace = true;
                return;
            }
        };
        match slot_for(&method, self.site_id.as_deref()) {
            Some(slot) => {
                self.targets.insert(slot, legacy);
            }
            None => self.legacy_trace = true,
        }
    }

    /// Lift the scattered legacy DNS fields into ONE [`DomainObservation`].
    ///
    /// Sources, in trust order: the flat legacy `dns_target` (pre-targets
    /// schema), then any per-target record's copy (the 2026-06 → 2026-08
    /// schema; project-scoped data misfiled per-target, so the first one
    /// found is the only one there ever was in practice). All copies are
    /// cleared either way — after this, the observation is the single home
    /// and `skip_serializing` retires the old fields from disk on save.
    fn lift_observation(&mut self, legacy_flat_dns_target: Option<DnsTarget>) {
        let record_dns_target = self
            .targets
            .values_mut()
            .find_map(|r| r.dns_target.take());
        for record in self.targets.values_mut() {
            record.dns_target = None;
        }
        let dns_configured = std::mem::take(&mut self.dns_configured);
        let checked_at = self.dns_configured_at.take();
        if self.observed.is_some() {
            return; // already migrated; the legacy leftovers are stale copies
        }
        let dns_target = legacy_flat_dns_target.or(record_dns_target);
        if dns_configured || checked_at.is_some() || dns_target.is_some() {
            self.observed = Some(DomainObservation {
                checked_at: checked_at.unwrap_or_default(),
                server_domains: Vec::new(),
                dns_configured,
                cdn_status: None,
                dns_target,
            });
        }
    }

    /// Any trace that a publish happened: a filed target record, or a legacy
    /// flat record that could not be filed under a slot. The nested-site
    /// guard reads this so an unfileable record still fails toward rename,
    /// never delete — the flat view alone would show nothing.
    pub fn records_publish_evidence(&self) -> bool {
        !self.targets.is_empty() || self.legacy_trace
    }

    /// Compose the flat view every caller reads: project-scoped fields plus
    /// the active target's record. `deploy_method` is the derived selector
    /// (see [`derive_deploy_method`]); `domain` is the authored intent from
    /// config.toml `[site].domain`, passed in by the reader that parsed that
    /// file; the `dns_*` trio derives from [`Self::observed`]. A target the
    /// selector names that has never published reads as an empty record,
    /// exactly like a fresh folder.
    pub fn flat_view(
        &self,
        deploy_method: Option<String>,
        domain: Option<String>,
    ) -> DomainDeploymentConfig {
        let record = deploy_method
            .as_deref()
            .and_then(|m| slot_for(m, self.site_id.as_deref()))
            .and_then(|slot| self.targets.get(&slot))
            .cloned()
            .unwrap_or_default();
        let observed = self.observed.as_ref();
        DomainDeploymentConfig {
            domain,
            dns_configured: observed.map_or(false, |o| o.dns_configured),
            dns_configured_at: observed
                .filter(|o| !o.checked_at.is_empty())
                .map(|o| o.checked_at.clone()),
            cdn_status: observed.and_then(|o| o.cdn_status.clone()),
            deploy_method,
            site_id: self.site_id.clone(),
            onion_name: self.onion_name.clone(),
            prebuilt_output: self.prebuilt_output.clone(),
            site_url: record.site_url,
            last_deployment_url: record.last_deployment_url,
            last_deployment_at: record.last_deployment_at,
            dns_target: observed.and_then(|o| o.dns_target.clone()),
            metadata: record.metadata,
            last_deployed_generation_id: record.last_deployed_generation_id,
            last_verified_generation: record.last_verified_generation,
            addresses: record.addresses,
        }
    }

    /// Split a modified flat view back out: project-scoped fields overwrite
    /// in place, per-target fields land in the active target's record and
    /// nowhere else. The slot re-derives from the flat view itself so a write
    /// that just registered a `site_id` (first publish) files under the new
    /// `moss:<id>` slot. With no active target the per-target fields are
    /// dropped — nothing reachable writes them in that state.
    ///
    /// The derived fields — `deploy_method`, `domain`, and the `dns_*` trio —
    /// are dropped here: config.toml owns the first two, the observation owns
    /// the third (written via its own setter, never through this view).
    pub fn absorb(&mut self, flat: &DomainDeploymentConfig) {
        self.site_id = flat.site_id.clone();
        self.onion_name = flat.onion_name.clone();
        self.prebuilt_output = flat.prebuilt_output.clone();
        let record = DeploymentRecord {
            site_url: flat.site_url.clone(),
            last_deployment_url: flat.last_deployment_url.clone(),
            last_deployment_at: flat.last_deployment_at.clone(),
            dns_target: None,
            metadata: flat.metadata.clone(),
            last_deployed_generation_id: flat.last_deployed_generation_id.clone(),
            last_verified_generation: flat.last_verified_generation.clone(),
            addresses: flat.addresses.clone(),
        };
        match flat.publish_target() {
            // An all-None record can only come from an empty or absent slot
            // (the flat view IS the active record), so filing it would mint a
            // `targets` entry for a target that never published — which
            // `records_publish_evidence` would then read as a publish.
            Some(slot) if record.is_empty() => {
                self.targets.remove(&slot);
            }
            Some(slot) => {
                self.targets.insert(slot, record);
            }
            None if !record.is_empty() => {
                log::warn!(
                    "deployment state: per-target fields written with no active target; dropped"
                );
            }
            None => {}
        }
    }
}

/// What to do about the authored domain given what the server reports.
///
/// Reconciliation is a VISIBLE event with exactly one silent case: no local
/// intent and the server naming exactly one domain — then there is nothing
/// to conflict with and the server's answer is adopted (the CPHS case: a
/// domain configured on the server, absent locally, every page baking the
/// mosspub canonical). Everything else either needs no action or needs a
/// human.
#[derive(Debug, Clone, PartialEq)]
pub enum DomainReconciliation {
    /// Nothing to change: agreement, nothing authored and nothing (or too
    /// much) to adopt, or an authored domain the server simply doesn't know
    /// yet (normal mid-setup).
    Settled,
    /// No authored domain and the server names exactly one — adopt it into
    /// config.toml, and log that it happened.
    Adopt(String),
    /// The two disagree. Surfaced, never auto-resolved: both sides are
    /// claims about where a real site should live.
    Conflict { authored: String, server: Vec<String> },
}

/// Pure decision half of the project-open domain reconcile; the caller owns
/// the fetch and the writes.
pub fn reconcile_domain(authored: Option<&str>, server: &[String]) -> DomainReconciliation {
    match authored.map(str::trim).filter(|a| !a.is_empty()) {
        None => match server {
            [one] => DomainReconciliation::Adopt(one.clone()),
            _ => DomainReconciliation::Settled,
        },
        Some(a) => {
            if server.is_empty() || server.iter().any(|d| d == a) {
                DomainReconciliation::Settled
            } else {
                DomainReconciliation::Conflict {
                    authored: a.to_string(),
                    server: server.to_vec(),
                }
            }
        }
    }
}

/// The custom-domain URL to surface for a LIVE site, when one is actually
/// serving: requires a non-empty authored domain (`[site].domain` in
/// config.toml) AND an observation with `dns_configured`.
/// Callers fall back to the mosspub subdomain URL when this returns `None`.
///
/// This is the one domain-preference rule shared by the publish surfaces
/// ("View site" toast, CLI success line, syndication). Do NOT feed the result
/// into [`crate::vault::deployment_state::record_publish`] — `last_deployment_url` must stay the
/// original subdomain URL (see `build_deployment_status`'s pass-through
/// invariant: the publish-panel caption derives "Also redirects from …"
/// from it).
pub fn live_custom_domain_url(config: &DomainDeploymentConfig) -> Option<String> {
    match (config.domain.as_deref(), config.dns_configured) {
        (Some(domain), true) if !domain.trim().is_empty() => {
            Some(format!("https://{}", domain.trim()))
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "deployment_tests.rs"]
mod tests;

/// Static site deployment configuration.
///
/// Specifies where and how the generated site should be published,
/// supporting multiple hosting providers and custom domains.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, specta::Type)]
pub struct DeploymentConfig {
    /// Hosting provider identifier ("mosspub.com", "github", "netlify", etc.)
    pub provider: String,
    /// Custom domain name for the published site
    pub custom_domain: Option<String>,
    /// Whether to automatically republish when content changes
    pub auto_publish: bool,
}

