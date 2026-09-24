//! What a plugin contributes — the manifest's answer to "what does the user
//! gain by installing this?"
//!
//! Split out of `types.rs` because it is a vocabulary that grows: every new
//! kind of contribution lands here, while `PluginManifest` itself stays a flat
//! record of fields.
//!
//! Since 2026-08-30 this is also where the plugin setup contract lives:
//! one `setup` block per
//! contribution, with `needs` (host-evaluated preconditions), `settings` (a
//! list of [`Field`]s moss draws), and `check` (the `check_setup` hook). The
//! six declaration surfaces that grew ad hoc — `capabilities`,
//! `setup.credentials`, the parallel `config_*` maps, `requires_stack` — parse
//! as aliases that fold into this one shape, in [`PluginManifest::parse`].

pub mod settings;
pub mod stack;

pub use settings::{Field, FieldOption, FieldType, Need};
use settings::{bound_author_text_in, CredentialNeed, title_case, validate_declared_fields};

use super::types::{Capability, PluginManifest};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Schema contributions from a plugin.
///
/// Modeled after VS Code's `contributes` manifest key — plugins declare
/// field definitions in their manifest; moss merges them into the active
/// schema at runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginContributes {
    /// Frontmatter field definitions contributed by this plugin.
    #[serde(default)]
    pub frontmatter: Option<ContributedFrontmatter>,

    /// Typed-embed renderers contributed by this plugin.
    ///
    /// Each entry declares one or more file extensions and a script that
    /// renders them. At pipeline init, moss wraps each in a
    /// `PluginEmbedRenderer` adapter, registers it with the
    /// `RendererRegistry`, and registers a corresponding `MarkerHandler`
    /// that invokes the script via plugin IPC when the Deferred marker
    /// is encountered.
    ///
    #[serde(default)]
    pub embed_renderers: Vec<EmbedRendererContribution>,

    /// A place the user's writing can be sent to, and read back from.
    /// Replaces the `syndicate` / `login` / `import` capabilities:
    /// those three said which hook the plugin exported, this says what the
    /// user gains — "moss can post to Matters for you".
    #[serde(default)]
    pub channel: Option<ChannelContribution>,

    /// A place the user's site can be published to. Replaces the
    /// `deploy` capability.
    #[serde(default)]
    pub deploy_target: Option<DeployTargetContribution>,

    /// The plugin processes files before generation. Replaces the `process`
    /// capability — the null case that forced this kind to exist: with
    /// `capabilities` derive-only, a pure process plugin needs a contribution
    /// kind, and `"processor": {}` is it.
    #[serde(default)]
    pub processor: Option<ProcessorContribution>,

    /// Job descriptors a plugin supplies (Step 3 Phase 5, §8 + R13).
    ///
    /// Parallels `frontmatter`: the plugin declares the MEANING of each Job it
    /// produces (a past-tense `verb` + an amount `noun`); moss normalizes the
    /// verb via `Verb::normalized` (R13) and owns every pixel of the surface —
    /// the descriptor carries no tone/color/glyph. Keyed by a plugin-local job
    /// id (e.g. `"syndicate"`), matched to the hook that emits it.
    #[serde(default)]
    pub jobs: Option<ContributedJobs>,

    /// The long-lived local companion process this plugin declares and moss
    /// installs/runs on its behalf. A plugin declares at most one.
    /// Never synthesized from `requires_stack` — that legacy alias states
    /// only the *need*, not an artifact pin; see [`PluginManifest::stack_id`].
    #[serde(default)]
    pub stack: Option<stack::StackContribution>,
}

/// Job descriptors contributed by a plugin (§8).
///
/// `#[serde(transparent)]` over the inner map: the manifest wire shape is the
/// BARE nested map `{ "jobs": { "<id>": {verb, noun} } }`, NOT a `descriptors`
/// wrapper key (`{ "jobs": { "descriptors": { ... } } }`). The named Rust field
/// `descriptors` is an accessor convenience that parallels
/// `ContributedFrontmatter.fields`; serde sees through it. This is the settled
/// answer to the design §12 open question on the `contributes.jobs` shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContributedJobs {
    /// Descriptors keyed by plugin-local job id (e.g. `"syndicate"`).
    pub descriptors: HashMap<String, JobDescriptor>,
}

/// One plugin job descriptor: the MEANING a plugin supplies (§8 + R13).
///
/// moss owns presentation entirely — this carries no tone/color/glyph. The
/// plugin proposes only the word and the noun; moss capitalizes/clamps the
/// verb (`Verb::normalized`) and renders "Syndicated · 3 posts".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobDescriptor {
    /// The past-tense verb the plugin proposes (e.g. `"Syndicated"`).
    /// Normalized by `Verb::normalized` before display — moss owns
    /// capitalization/length/glyphs (R13).
    pub verb: String,
    /// The amount noun (e.g. `"posts"`) for the receipt "Syndicated · 3 posts".
    pub noun: String,
}

/// One entry in a plugin's `contributes.embed_renderers` list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbedRendererContribution {
    /// File extensions this renderer claims (lowercase, without leading dot).
    /// E.g., `["dot", "gv"]` for a Graphviz renderer.
    pub extensions: Vec<String>,

    /// Plugin script path (relative to the plugin directory) whose default
    /// export is invoked for each matched embed. The function receives
    /// `{ target_path, query, section, alias, site_root }` and must return
    /// a string of HTML to splice into the page.
    pub script: String,

    /// Optional human-readable name for diagnostics and the settings UI.
    #[serde(default)]
    pub name: Option<String>,
}

/// Frontmatter schema contributed by a plugin.
///
/// Uses the same `FieldDefinition` type as the builtin schema (moss-core),
/// so plugin-contributed fields get identical UI widgets and validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributedFrontmatter {
    /// Field definitions keyed by field name (same format as builtin schema).
    pub fields: HashMap<String, moss_core::schema::FieldDefinition>,
}

/// A channel the plugin adds — somewhere the user's writing goes, and
/// possibly comes back from.
///
/// Syndication is what a channel *is*, so it needs no flag. `imports` and
/// `login` each name an export the plugin also provides, and
/// [`ContributionReadiness`] says what the user must arrange first — a channel
/// that needs a token declares it the same way a deploy target does.
///
/// A consequence worth stating outright: **an import-only channel is not
/// expressible.** Any `channel` folds to `Capability::Syndicate`, so moss will
/// dispatch `syndicate(ctx)` to a plugin that declared `{"imports": true}`
/// meaning "pull my old posts in", and the missing export fails at publish
/// time. That follows from the ADR's own definition rather than from an
/// oversight — if a real plugin wants import without syndication, the answer is
/// a separate contribution, not a third flag on this one.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChannelContribution {
    /// Name shown to the user; defaults to the plugin's display name.
    #[serde(default)]
    pub display_name: Option<String>,

    /// Writing can also be pulled from this channel into the folder.
    #[serde(default)]
    pub imports: bool,

    /// The user connects an account: the plugin exports `login`, which moss
    /// invokes via `connect_account`. This is what folds to
    /// [`Capability::Login`], and it is deliberately not inferred from
    /// [`ContributionReadiness::setup`] — a channel whose setup is one API
    /// token has nothing to connect, and would otherwise get a connection
    /// row in settings and an auto-opening login prompt for an account that
    /// does not exist. A future version plans to read the export itself at install
    /// time; until then the manifest says it.
    ///
    /// Reads `requires_login` too, the name it carried before 2026-08-30, so
    /// an already-installed plugin keeps its connection.
    #[serde(default, alias = "requires_login")]
    pub login: bool,

    /// What the user must have arranged before this channel works.
    #[serde(flatten)]
    pub readiness: ContributionReadiness,
}

/// A place the site can be published to — GitHub Pages, IPFS, an onion
/// service.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeployTargetContribution {
    /// Name shown to the user; defaults to the plugin's display name.
    #[serde(default)]
    pub display_name: Option<String>,

    /// What the user must have arranged before this target can publish.
    #[serde(flatten)]
    pub readiness: ContributionReadiness,
}

/// The plugin pre-processes files before generation.
///
/// Usually the empty object — a processor has no per-target account or
/// address — but it flattens the same [`ContributionReadiness`] in, so a
/// processor that does need settings or a probe declares them the same way
/// every other kind does.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProcessorContribution {
    /// Name shown to the user; defaults to the plugin's display name.
    #[serde(default)]
    pub display_name: Option<String>,

    /// What the user must have arranged before this processor runs.
    #[serde(flatten)]
    pub readiness: ContributionReadiness,
}

/// What a contribution needs before moss dispatches to it — the part of a
/// contribution that is the same whatever kind it is.
///
/// Every contribution kind flattens this in, so `setup` sits at the top level
/// of the contribution's own object and is scoped by **where it sits** rather
/// than by a `scope:` field naming what it applies to. That is Raycast's
/// arrangement; VS Code's six-valued `scope` enum exists to retrofit settings
/// onto a multi-root editor, and moss has one vault open at a time.
///
/// Hoisted out of `DeployTargetContribution` on 2026-08-30: a channel could
/// say nothing at all about what it needed, so whether a plugin could name a
/// credential depended on what kind of plugin it was rather than on what it
/// held.
/// A new contribution kind gains the whole vocabulary by flattening this in.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContributionReadiness {
    /// Declared setup: preconditions, the fields moss draws, and whether the
    /// plugin implements the `check_setup` probe.
    #[serde(default)]
    pub setup: Option<SetupContribution>,
}

/// What a contribution needs before it can run, declared rather than
/// discovered.
///
/// Three members, ordered by what they cost to evaluate: `needs` the host
/// checks for free, `settings` moss renders from the manifest without spawning
/// the plugin, and `check` — one hook for the answers only the plugin can
/// give. moss never spawns an engine to learn something the manifest already
/// said.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SetupContribution {
    /// Preconditions the host evaluates with no plugin code running. A failed
    /// need short-circuits: `check_setup` is never spawned behind one, and
    /// moss materializes the failure as a host-authored blocker whose remedy
    /// moss owns.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub needs: Vec<Need>,

    /// The fields moss draws — in the settings page always, and in the
    /// publish gate when one that must have a value has none. Order is the
    /// display order, and it is load-bearing: a `when` may only reference a
    /// field declared earlier in this list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<Field>,

    /// This plugin implements the `check_setup` hook, so moss should ask it
    /// about the things a manifest cannot state — is the daemon running, is
    /// the token still valid.
    #[serde(default)]
    pub check: bool,

    /// The pre-contract spelling: `setup.credentials` entries become `secret`
    /// fields in `settings`. Parsed, folded in [`PluginManifest::parse`], and
    /// never serialized back out — `settings` is the shape moss writes.
    #[serde(default, skip_serializing)]
    pub(crate) credentials: Vec<CredentialNeed>,
}

impl SetupContribution {
    /// The `secret` fields that apply given the plugin's resolved settings —
    /// the map [`PluginManifest::config_defaults`] seeds and
    /// `discovery::get_plugin_config_with_defaults` returns.
    pub fn applicable_secrets<'a>(
        &'a self,
        config: &HashMap<String, serde_json::Value>,
    ) -> Vec<&'a Field> {
        self.settings
            .iter()
            .filter(|f| f.field_type == FieldType::Secret && f.when_matches(config))
            .collect()
    }
}

/// The parts of a contribution that every kind has, borrowed from whichever
/// kind granted the capability being asked about.
///
/// It exists so the capability-to-contribution match is written once. Both
/// questions moss asks of a contribution it reached through a capability —
/// what is it called, what must be arranged first — read from here, so a new
/// contribution kind is one arm rather than one arm per question.
pub struct ContributionRef<'a> {
    /// Name shown to the user, when the contribution overrode the plugin's.
    pub display_name: Option<&'a str>,
    /// What the user must have arranged before moss dispatches to it.
    pub readiness: &'a ContributionReadiness,
}

impl PluginContributes {
    /// The contribution that granted a capability.
    ///
    /// This is the generalization of what used to be a hard-coded reach into
    /// `deploy_target`: the gate asks about a (plugin, capability) pair, and
    /// the manifest answers from wherever the contribution sits.
    pub fn contribution_for(&self, capability: &Capability) -> Option<ContributionRef<'_>> {
        match capability {
            Capability::Deploy => self.deploy_target.as_ref().map(|t| ContributionRef {
                display_name: t.display_name.as_deref(),
                readiness: &t.readiness,
            }),
            Capability::Syndicate | Capability::Login | Capability::Import => {
                self.channel.as_ref().map(|c| ContributionRef {
                    display_name: c.display_name.as_deref(),
                    readiness: &c.readiness,
                })
            }
            Capability::Process => self.processor.as_ref().map(|p| ContributionRef {
                display_name: p.display_name.as_deref(),
                readiness: &p.readiness,
            }),
        }
    }

    /// The setup a given capability is gated on.
    pub fn setup_for(&self, capability: &Capability) -> Option<&SetupContribution> {
        self.contribution_for(capability)?.readiness.setup.as_ref()
    }

    /// Every setup block, in the order the settings page draws them:
    /// channel first (a channel is the account-holding kind), then deploy
    /// target, then processor. The same order
    /// [`PluginManifest::fold_contributions_into_capabilities`] derives, so
    /// `declared_settings`' walk over capabilities visits contributions
    /// identically — change one and change the other.
    fn setups_mut(&mut self) -> Vec<&mut Option<SetupContribution>> {
        let mut out = Vec::new();
        if let Some(c) = self.channel.as_mut() {
            out.push(&mut c.readiness.setup);
        }
        if let Some(t) = self.deploy_target.as_mut() {
            out.push(&mut t.readiness.setup);
        }
        if let Some(p) = self.processor.as_mut() {
            out.push(&mut p.readiness.setup);
        }
        out
    }
}

impl PluginManifest {
    /// The setup a given capability is gated on — [`PluginContributes::setup_for`]
    /// through the optional `contributes` block.
    pub fn setup_for(&self, capability: &Capability) -> Option<&SetupContribution> {
        self.contributes.as_ref()?.setup_for(capability)
    }

    /// Every field this plugin declares, across all of its contributions.
    ///
    /// The publish gate asks about one capability because a publish uses one
    /// contribution. The settings page is the other question — "what does moss
    /// draw for this plugin?" — and its answer is the union, in declaration
    /// order, deduplicated by key: a plugin that both publishes and
    /// syndicates keeps both tokens in the same store and corrects them in
    /// one place.
    ///
    /// Walking `capabilities` keeps this on
    /// [`PluginContributes::contribution_for`]'s single match: a new kind is
    /// reachable here the moment it grants a capability. The channel grants
    /// three capabilities off one block, which the dedup collapses.
    pub fn declared_settings(&self) -> Vec<&Field> {
        let mut found: Vec<&Field> = Vec::new();
        for capability in &self.capabilities {
            let Some(setup) = self.setup_for(capability) else { continue };
            for field in &setup.settings {
                if !found.iter().any(|seen| seen.key == field.key) {
                    found.push(field);
                }
            }
        }
        found
    }

    /// The `secret` fields that apply given the plugin's resolved settings —
    /// what the settings page draws as credentials and what the publish gate
    /// asks the store about.
    pub fn declared_secrets(
        &self,
        config: &HashMap<String, serde_json::Value>,
    ) -> Vec<&Field> {
        self.declared_settings()
            .into_iter()
            .filter(|f| f.field_type == FieldType::Secret && f.when_matches(config))
            .collect()
    }

    /// The credential keys moss itself asks the user for: every declared
    /// `secret` field that is not `hidden`. A hidden secret is one no person
    /// ever types — the plugin's own flow earns and deposits it — so it is
    /// the one kind the plugin write arm may reach.
    ///
    /// This is the exclusion set for that arm. A plugin deposits what a login
    /// flow handed it; it may not reach a slot the user filled in by hand,
    /// because overwriting one would let a plugin substitute a token for the
    /// one the user believes is there — and the user has no way to see that
    /// it happened, since moss never shows a stored value back.
    ///
    /// Deliberately NOT filtered by `when`. The gate has to hold for every
    /// setting the user might have, not the one they have now; a set that
    /// narrowed as a setting changed would be openable by flipping the setting
    /// that hides the `when`.
    pub fn user_supplied_secret_keys(&self) -> Vec<String> {
        self.secret_keys(false)
    }

    /// The complement: `secret` settings declared `hidden` — credentials the
    /// plugin itself deposits (a token its login flow earned), which is the
    /// set the write arm ADMITS. Unfiltered by `when` for the same reason.
    pub fn plugin_owned_secret_keys(&self) -> Vec<String> {
        self.secret_keys(true)
    }

    fn secret_keys(&self, hidden: bool) -> Vec<String> {
        self.declared_settings()
            .into_iter()
            .filter(|f| f.field_type == FieldType::Secret && f.hidden == hidden)
            .map(|f| f.key.clone())
            .collect()
    }

    /// The default value of every declared setting, under the manifest's
    /// legacy `config` map. Seeds `get_plugin_config_with_defaults`, so a
    /// field the user never touched resolves to its declared default in hook
    /// contexts, `when` evaluation, and the settings form alike.
    pub fn config_defaults(&self) -> HashMap<String, serde_json::Value> {
        let mut defaults = self.config.clone();
        for field in self.declared_settings() {
            if let Some(value) = &field.default {
                defaults.entry(field.key.clone()).or_insert_with(|| value.clone());
            }
        }
        defaults
    }

    /// Does any contribution need moss's Docker stack? The registry index and
    /// the install flow read this; `requires_stack: true` parses as an alias
    /// that lands here.
    pub fn needs_stack(&self) -> bool {
        self.capabilities.iter().any(|capability| {
            self.setup_for(capability)
                .is_some_and(|setup| setup.needs.contains(&Need::Stack))
        })
    }

    /// The id of the stack this plugin declares, if it declares one. `Need`
    /// carries no payload for the id (amended 2026-09-10):
    /// this accessor is what a caller reads instead.
    pub fn stack_id(&self) -> Option<&str> {
        Some(self.contributes.as_ref()?.stack.as_ref()?.id.as_str())
    }

    /// What the contribution granting a capability calls itself, when it
    /// overrode the plugin's own name — so the row and the modal that opens
    /// from it cannot disagree.
    pub fn display_name_for(&self, capability: &Capability) -> Option<&str> {
        self.contributes.as_ref()?.contribution_for(capability)?.display_name
    }

    /// Parse a manifest and normalize every legacy surface into the one shape.
    ///
    /// **Every production read of a manifest goes through here** — enforced by
    /// `manifest_parse_invariant_test`, because a raw `serde_json::from_str`
    /// would see a plugin that declares only `contributes.deploy_target` as
    /// having no capabilities at all, and silently drop it from the deploy
    /// menu.
    ///
    /// In order, and each step idempotent so a serialized manifest re-parses
    /// to itself:
    /// 1. Contributions are synthesized from declared `capabilities`
    ///    (`process` → `processor`, `deploy` → `deploy_target`,
    ///    `syndicate`/`import`/`login` → `channel`), so the legacy
    ///    spelling keeps working.
    /// 2. Natively-declared `settings` pass [`validate_declared_fields`] —
    ///    the strict rules bind the surface that was born with them.
    /// 3. The legacy surfaces fold in: `setup.credentials` become `secret`
    ///    fields, the `config_*` maps and `config` defaults become fields on
    ///    the plugin's account-holding contribution, `requires_stack` becomes
    ///    `needs: ["stack"]`.
    /// 4. `capabilities` is re-derived from the contributions — parsed and
    ///    never trusted, so the fold is the only source.
    pub fn parse(json: &str) -> serde_json::Result<Self> {
        use serde::de::Error as _;
        let mut manifest: Self = serde_json::from_str(json)?;
        manifest.synthesize_contributions_from_capabilities();
        for capability in [Capability::Syndicate, Capability::Deploy, Capability::Process] {
            if let Some(setup) = manifest.setup_for(&capability) {
                validate_declared_fields(&setup.settings)
                    .map_err(serde_json::Error::custom)?;
            }
        }
        if let Some(stack) = manifest.contributes.as_ref().and_then(|c| c.stack.as_ref()) {
            stack.validate().map_err(serde_json::Error::custom)?;
        }
        manifest.fold_legacy_surfaces();
        manifest.fold_contributions_into_capabilities();
        Ok(manifest)
    }

    /// The legacy spelling: a bare `capabilities` list with no
    /// `contributes` block. Each declared capability synthesizes the
    /// contribution that now grants it, so the fold below can stay the only
    /// source of capabilities without unloading an installed plugin.
    fn synthesize_contributions_from_capabilities(&mut self) {
        if self.capabilities.is_empty() {
            return;
        }
        let contributes = self.contributes.get_or_insert_with(|| PluginContributes {
            frontmatter: None,
            embed_renderers: Vec::new(),
            channel: None,
            deploy_target: None,
            processor: None,
            jobs: None,
            stack: None,
        });
        for capability in &self.capabilities {
            match capability {
                Capability::Process => {
                    contributes.processor.get_or_insert_with(Default::default);
                }
                Capability::Deploy => {
                    contributes.deploy_target.get_or_insert_with(Default::default);
                }
                Capability::Syndicate => {
                    contributes.channel.get_or_insert_with(Default::default);
                }
                Capability::Import => {
                    contributes.channel.get_or_insert_with(Default::default).imports = true;
                }
                Capability::Login => {
                    contributes.channel.get_or_insert_with(Default::default).login = true;
                }
            }
        }
    }

    /// Fold the legacy declaration surfaces into `settings` and `needs`.
    ///
    /// The `config_*` maps are plugin-wide, so they land on ONE contribution —
    /// the first of channel, deploy target, processor, which is the settings
    /// page's own draw order. Synthesized fields go in sorted by key (the maps
    /// carry no order), ahead of any `setup.credentials` fold so a legacy
    /// credential's `when` can still see the provider field it references.
    /// A key the contribution already declares natively is skipped — native
    /// wins, and the skip is what makes a re-parse of a serialized manifest a
    /// no-op.
    fn fold_legacy_surfaces(&mut self) {
        let legacy_fields = self.legacy_config_fields();
        let fold_stack = self.requires_stack;
        let mut setups = match &mut self.contributes {
            Some(contributes) => contributes.setups_mut(),
            None => Vec::new(),
        };

        // Both plugin-wide surfaces need a contribution to land on; a plugin
        // that has none has nothing moss will ever dispatch to, so the folds
        // would vanish silently — say so instead, once for both.
        if setups.is_empty() {
            if !legacy_fields.is_empty() || fold_stack {
                log::warn!(
                    target: "plugin",
                    "plugin `{}` declares config fields or a stack requirement but no \
                     channel, deploy target or processor to hold them — declare a contribution",
                    self.name
                );
            }
            return;
        }

        // Every contribution folds its own `credentials` — they were declared
        // inside that block and blocking is scoped to the contribution. The
        // label fill runs in the same pass: every drawn field has a name, so
        // no renderer needs its own fallback.
        for setup in setups.iter_mut().filter_map(|s| s.as_mut()) {
            let credentials = std::mem::take(&mut setup.credentials);
            for need in credentials {
                if setup.settings.iter().any(|f| f.key == need.key) {
                    continue;
                }
                setup.settings.push(Field::legacy_secret(
                    need.key, need.label, need.help_url, need.when,
                ));
            }
            for field in &mut setup.settings {
                if field.label.is_empty() {
                    field.label = title_case(&field.key);
                }
            }
            // After the fill, so a title-cased key is bounded on the same
            // terms as a declared label, and after the legacy fold, so the
            // fields it pushed are covered by the one pass rather than by a
            // second rule that would have to be kept in step with this one.
            bound_author_text_in(&mut setup.settings);
        }

        // `requires_stack` was plugin-wide: whatever contribution runs, the
        // stack has to be up first, so the need lands on every one.
        if fold_stack {
            for slot in setups.iter_mut() {
                let setup = slot.get_or_insert_with(Default::default);
                if !setup.needs.contains(&Need::Stack) {
                    setup.needs.push(Need::Stack);
                }
            }
        }

        // The config maps land on the first contribution — the settings
        // page's own draw order decides which one holds the plugin's fields.
        if !legacy_fields.is_empty() {
            let setup = setups[0].get_or_insert_with(Default::default);
            let mut merged: Vec<Field> = legacy_fields
                .into_iter()
                .filter(|f| !setup.settings.iter().any(|have| have.key == f.key))
                .collect();
            merged.append(&mut setup.settings);
            setup.settings = merged;
        }
    }

    /// Fields synthesized from `config_schema` + the parallel `config_*` maps
    /// + `config` defaults, sorted by key for a stable order. Legacy types map
    /// onto the closed set: `enum` becomes a `string` with `options` (labels
    /// from the `"<field>.<value>"` entries of `config_labels`); a type the
    /// contract has no shape for (`array`) is skipped with a warning — its
    /// persisted value still reaches hooks, it just has no drawn field.
    fn legacy_config_fields(&self) -> Vec<Field> {
        let Some(schema) = &self.config_schema else { return Vec::new() };
        let mut keys: Vec<&String> = schema.keys().collect();
        keys.sort();
        let mut fields = Vec::new();
        for key in keys {
            let declared = schema[key].as_str();
            let (field_type, options) = match declared {
                "string" => (FieldType::String, Vec::new()),
                "number" => (FieldType::Number, Vec::new()),
                "boolean" => (FieldType::Boolean, Vec::new()),
                "secret" => (FieldType::Secret, Vec::new()),
                "enum" => {
                    let values = self
                        .config_options
                        .as_ref()
                        .and_then(|o| o.get(key))
                        .cloned()
                        .unwrap_or_default();
                    let options = values
                        .into_iter()
                        .map(|value| FieldOption {
                            label: self
                                .config_labels
                                .as_ref()
                                .and_then(|l| l.get(&format!("{key}.{value}")))
                                .cloned()
                                .unwrap_or_else(|| value.clone()),
                            description: None,
                            value,
                        })
                        .collect();
                    (FieldType::String, options)
                }
                other => {
                    log::warn!(
                        target: "plugin",
                        "plugin `{}` config field `{key}` has type `{other}`, which the \
                         setup contract has no shape for — the field will not be drawn",
                        self.name
                    );
                    continue;
                }
            };
            let lookup = |map: &Option<HashMap<String, String>>| {
                map.as_ref().and_then(|m| m.get(key)).cloned()
            };
            fields.push(Field {
                key: key.clone(),
                field_type,
                label: lookup(&self.config_labels).unwrap_or_else(|| title_case(key)),
                description: lookup(&self.config_descriptions),
                // The legacy surface has no way to declare a group.
                group: None,
                // A secret must not carry a default; a legacy manifest that
                // paired the two keeps loading, minus the default.
                default: (field_type != FieldType::Secret)
                    .then(|| self.config.get(key).cloned())
                    .flatten(),
                options,
                required: false,
                hidden: false,
                placeholder: lookup(&self.config_placeholders),
                help_url: None,
                pattern: None,
                pattern_message: None,
                when: HashMap::new(),
            });
        }
        fields
    }

    /// Derive `capabilities` from `contributes` — the only source. A declared
    /// list is parsed and never trusted: after the synthesis step above it has
    /// already said everything it can say as contributions.
    pub fn fold_contributions_into_capabilities(&mut self) {
        let Some(contributes) = &self.contributes else {
            self.capabilities.clear();
            return;
        };
        let mut derived = Vec::new();
        if let Some(channel) = &contributes.channel {
            derived.push(Capability::Syndicate);
            if channel.login {
                derived.push(Capability::Login);
            }
            if channel.imports {
                derived.push(Capability::Import);
            }
        }
        if contributes.deploy_target.is_some() {
            derived.push(Capability::Deploy);
        }
        if contributes.processor.is_some() {
            derived.push(Capability::Process);
        }
        self.capabilities = derived;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use moss_core::untrusted_text::{MAX_NAME, MAX_SENTENCE};

    fn manifest_with_setup(setup: &str) -> PluginManifest {
        PluginManifest::parse(&format!(
            r#"{{"name":"ipfs","version":"1.0.0","entry":"main.js",
                "contributes":{{"deploy_target":{{"display_name":"IPFS","setup":{setup}}}}}}}"#
        ))
        .expect("manifest parses")
    }

    fn setup_of(m: &PluginManifest) -> &SetupContribution {
        m.setup_for(&Capability::Deploy).expect("the deploy target declares setup")
    }

    fn config(pairs: &[(&str, serde_json::Value)]) -> HashMap<String, serde_json::Value> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }

    /// The parse error for a settings list, as a string to assert on.
    fn settings_error(settings: &str) -> String {
        PluginManifest::parse(&format!(
            r#"{{"name":"p","version":"1.0.0","entry":"main.js",
                "contributes":{{"deploy_target":{{"setup":{{"settings":{settings}}}}}}}}}"#
        ))
        .expect_err("the manifest must refuse to parse")
        .to_string()
    }

    /// Every string the author wrote for the form crosses bounded and stripped.
    ///
    /// One test over all six carriers because they are one rule: the label,
    /// the sentence under the input, the ghost text, the validation message,
    /// and both halves of a select option are all drawn inside sentences moss
    /// wrote. A `U+202E` in any of them reverses moss's words, not the
    /// author's, and an unbounded one lays out a form with no column to stop
    /// it. Asserted through `parse`, not by calling the pass, so it is the
    /// SEAM that is proven and not the helper.
    #[test]
    fn every_author_written_string_on_a_field_is_bounded_and_stripped() {
        let long = "x".repeat(MAX_NAME + 40);
        let sentence = "y".repeat(MAX_SENTENCE + 40);
        let m = manifest_with_setup(&format!(
            r#"{{"settings":[{{"key":"tok","type":"string",
                 "label":"lab\u202Eel{long}","description":"{sentence}\u202E",
                 "placeholder":"ghost\u202E{long}","pattern":"^a$",
                 "pattern_message":"nope\u202E{sentence}",
                 "options":[{{"value":"a","label":"opt\u202E{long}",
                              "description":"od\u202E{sentence}"}}]}}]}}"#
        ));
        let field = &setup_of(&m).settings[0];

        for text in [
            &field.label,
            field.description.as_ref().unwrap(),
            field.placeholder.as_ref().unwrap(),
            field.pattern_message.as_ref().unwrap(),
            &field.options[0].label,
            field.options[0].description.as_ref().unwrap(),
        ] {
            assert!(!text.contains('\u{202E}'), "a direction override survived: {text:?}");
        }
        assert_eq!(field.label.chars().count(), MAX_NAME + 1, "the label, at a name's length");
        assert_eq!(field.placeholder.as_ref().unwrap().chars().count(), MAX_NAME + 1);
        assert_eq!(field.options[0].label.chars().count(), MAX_NAME + 1);
        assert_eq!(
            field.description.as_ref().unwrap().chars().count(),
            MAX_SENTENCE + 1,
            "the sentence, at a sentence's"
        );
        assert_eq!(field.pattern_message.as_ref().unwrap().chars().count(), MAX_SENTENCE + 1);
        assert_eq!(field.options[0].description.as_ref().unwrap().chars().count(), MAX_SENTENCE + 1);
    }

    /// A key moss title-cases is bounded on the same terms as a declared one.
    #[test]
    fn a_filled_label_is_bounded_too() {
        let key = "k".repeat(MAX_NAME + 40);
        let m = manifest_with_setup(&format!(
            r#"{{"settings":[{{"key":"{key}","type":"string"}}]}}"#
        ));
        assert_eq!(setup_of(&m).settings[0].label.chars().count(), MAX_NAME + 1);
    }

    /// The block is optional, and a target that declares none is ready — the
    /// existing deploy plugins say nothing and must keep publishing.
    #[test]
    fn a_deploy_target_without_a_setup_block_still_parses() {
        let m = PluginManifest::parse(
            r#"{"name":"gh","version":"1.0.0","entry":"main.js",
                "contributes":{"deploy_target":{"display_name":"GitHub Pages"}}}"#,
        )
        .unwrap();
        assert!(m.setup_for(&Capability::Deploy).is_none());
        assert!(m.capabilities.contains(&Capability::Deploy));
    }

    // ------------------------------------------------------------------
    // The settings surface and its parse-time closure
    // ------------------------------------------------------------------

    /// The contract's own worked example: a select, a conditional secret, a
    /// plain field — parsed, ordered, and resolved by `when`.
    #[test]
    fn the_settings_list_parses_and_when_selects_by_the_users_settings() {
        let m = manifest_with_setup(
            r#"{"settings":[
                {"key":"provider","type":"string","label":"Pinning service","default":"pinata",
                 "options":[{"value":"pinata","label":"Pinata"},
                            {"value":"local","label":"Local Kubo node"}]},
                {"key":"pinata_jwt","type":"secret","label":"Pinata API token",
                 "help_url":"https://example.test/keys","when":{"provider":"pinata"}},
                {"key":"node_rpc","type":"string","label":"Node RPC endpoint",
                 "when":{"provider":"local"}}
            ],"check":true}"#,
        );
        let setup = setup_of(&m);
        assert!(setup.check);
        assert_eq!(setup.settings.len(), 3);
        assert_eq!(setup.settings[0].options.len(), 2);

        let picked = setup.applicable_secrets(&config(&[("provider", "pinata".into())]));
        assert_eq!(picked.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(), ["pinata_jwt"]);
        assert_eq!(picked[0].help_url.as_deref(), Some("https://example.test/keys"));
        assert!(setup.applicable_secrets(&config(&[("provider", "local".into())])).is_empty());

        // The default fills the resolved config, so an untouched manifest
        // still selects the pinata token rather than asking for nothing.
        let defaults = m.config_defaults();
        assert_eq!(defaults.get("provider"), Some(&serde_json::json!("pinata")));
        assert_eq!(setup.applicable_secrets(&defaults).len(), 1);
    }

    #[test]
    fn a_secret_with_a_default_is_refused() {
        let err = settings_error(
            r#"[{"key":"tok","type":"secret","label":"T","default":"paste-here"}]"#,
        );
        assert!(err.contains("a `secret` must not declare a `default`"), "{err}");
    }

    #[test]
    fn a_secret_with_required_is_refused() {
        let err = settings_error(r#"[{"key":"tok","type":"secret","label":"T","required":true}]"#);
        assert!(err.contains("does not take `required`"), "{err}");
    }

    #[test]
    fn required_with_a_default_is_refused() {
        let err = settings_error(
            r#"[{"key":"host","type":"string","label":"H","required":true,"default":"a"}]"#,
        );
        assert!(err.contains("both `required` and `default`"), "{err}");
    }

    #[test]
    fn a_key_declared_twice_in_settings_is_refused() {
        let err = settings_error(
            r#"[{"key":"host","type":"string","label":"H"},
                {"key":"host","type":"string","label":"Host again"}]"#,
        );
        assert!(err.contains("declared twice"), "{err}");
    }

    #[test]
    fn a_pattern_without_a_message_is_refused() {
        let err = settings_error(
            r#"[{"key":"name","type":"string","label":"N","pattern":"^[a-z]+$"}]"#,
        );
        assert!(err.contains("`pattern` without a `pattern_message`"), "{err}");
    }

    /// The three `when` closure rules: only backward references, only onto a
    /// closed select, only values the select offers.
    #[test]
    fn a_when_referencing_a_later_field_is_refused() {
        let err = settings_error(
            r#"[{"key":"tok","type":"secret","label":"T","when":{"provider":"pinata"}},
                {"key":"provider","type":"string","label":"P",
                 "options":[{"value":"pinata","label":"Pinata"}]}]"#,
        );
        assert!(err.contains("not declared earlier"), "{err}");
    }

    #[test]
    fn a_when_referencing_a_field_without_options_is_refused() {
        let err = settings_error(
            r#"[{"key":"provider","type":"string","label":"P"},
                {"key":"tok","type":"secret","label":"T","when":{"provider":"pinata"}}]"#,
        );
        assert!(err.contains("has no `options`"), "{err}");
    }

    #[test]
    fn a_when_value_outside_the_options_is_refused() {
        let err = settings_error(
            r#"[{"key":"provider","type":"string","label":"P",
                 "options":[{"value":"pinata","label":"Pinata"}]},
                {"key":"tok","type":"secret","label":"T","when":{"provider":"filebase"}}]"#,
        );
        assert!(err.contains("not one of `provider`'s options"), "{err}");
    }

    /// A `when` value that is not a string — the list form included — is
    /// refused at the type, which is what designs out the silent-false of a
    /// scalar comparison against a list (Ansible's `required_if` bug).
    #[test]
    fn a_non_string_when_value_is_refused() {
        let err = settings_error(
            r#"[{"key":"provider","type":"string","label":"P",
                 "options":[{"value":"pinata","label":"Pinata"}]},
                {"key":"tok","type":"secret","label":"T",
                 "when":{"provider":["pinata","local"]}}]"#,
        );
        assert!(err.contains("must be a single string"), "{err}");
    }

    #[test]
    fn options_on_a_non_string_field_are_refused() {
        let err = settings_error(
            r#"[{"key":"port","type":"number","label":"P",
                 "options":[{"value":"22","label":"SSH"}]}]"#,
        );
        assert!(err.contains("`options` close the value set of a `string` field"), "{err}");
    }

    /// A clause is an AND, not an OR: two conditions mean both.
    #[test]
    fn a_multi_pair_when_clause_is_an_and() {
        let m = manifest_with_setup(
            r#"{"settings":[
                {"key":"provider","type":"string","label":"P",
                 "options":[{"value":"pinata","label":"Pinata"},{"value":"local","label":"Local"}]},
                {"key":"tier","type":"string","label":"T",
                 "options":[{"value":"free","label":"Free"},{"value":"paid","label":"Paid"}]},
                {"key":"tok","type":"secret","label":"Token",
                 "when":{"provider":"pinata","tier":"paid"}}
            ]}"#,
        );
        let setup = setup_of(&m);
        assert!(setup
            .applicable_secrets(&config(&[("provider", "pinata".into()), ("tier", "free".into())]))
            .is_empty());
        assert_eq!(
            setup
                .applicable_secrets(&config(&[
                    ("provider", "pinata".into()),
                    ("tier", "paid".into())
                ]))
                .len(),
            1
        );
        // A pair moss cannot evaluate does not exclude: absent is fail-closed,
        // so an unset `tier` leaves the secret applicable rather than skipped.
        assert_eq!(setup.applicable_secrets(&config(&[("provider", "pinata".into())])).len(), 1);
    }

    // ------------------------------------------------------------------
    // needs
    // ------------------------------------------------------------------

    #[test]
    fn needs_parse_from_the_closed_vocabulary() {
        let m = manifest_with_setup(r#"{"needs":["stack","binary:ipfs"],"check":true}"#);
        assert_eq!(setup_of(&m).needs, vec![Need::Stack, Need::Binary("ipfs".into())]);
        assert!(m.needs_stack());
    }

    #[test]
    fn an_unknown_need_is_refused() {
        let err = PluginManifest::parse(
            r#"{"name":"p","version":"1.0.0","entry":"main.js",
                "contributes":{"deploy_target":{"setup":{"needs":["gpu"]}}}}"#,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("unknown need `gpu`"), "{err}");
        assert!(err.contains("`stack` and `binary:<name>`"), "{err}");
    }

    /// `requires_stack: true` is the pre-contract spelling of
    /// `needs: ["stack"]`; onionpress ships it today and must keep loading.
    #[test]
    fn requires_stack_folds_into_needs() {
        let m = PluginManifest::parse(
            r#"{"name":"onionpress","version":"0.4.0","entry":"main.js",
                "capabilities":["deploy"],"requires_stack":true}"#,
        )
        .unwrap();
        assert!(m.needs_stack());
        assert_eq!(setup_of(&m).needs, vec![Need::Stack]);
        // Idempotent: serialize → re-parse does not stack a second need.
        let again = PluginManifest::parse(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(setup_of(&again).needs, vec![Need::Stack]);
    }

    /// Every drawn field has a name: a field the manifest gave no `label`
    /// gets its title-cased key at parse, so no renderer needs a fallback.
    #[test]
    fn a_field_without_a_label_gets_its_title_cased_key() {
        let m = manifest_with_setup(r#"{"settings":[{"key":"pin_name","type":"string"}]}"#);
        assert_eq!(setup_of(&m).settings[0].label, "Pin Name");
    }

    /// `check` is what says the plugin implements `check_setup`; default false
    /// so moss never dispatches a hook that isn't there.
    #[test]
    fn check_is_off_unless_the_plugin_says_otherwise() {
        assert!(!setup_of(&manifest_with_setup(r#"{"settings":[]}"#)).check);
        assert!(setup_of(&manifest_with_setup(r#"{"check":true}"#)).check);
    }

    // ------------------------------------------------------------------
    // Capabilities: derived, never trusted
    // ------------------------------------------------------------------

    /// `capabilities` in a manifest that also declares `contributes` is parsed
    /// and never trusted — the contributions are the only source. A declared
    /// capability no contribution grants is synthesized INTO a contribution
    /// first (the legacy spelling), so nothing an installed plugin could
    /// say is lost; what cannot happen is the two lists disagreeing.
    #[test]
    fn capabilities_are_derived_from_contributions() {
        // matters' real shape: a declared `process` beside a channel.
        let m = PluginManifest::parse(
            r#"{"name":"matters","version":"1.0.0","entry":"main.js",
                "capabilities":["process"],
                "contributes":{"channel":{"imports":true,"login":true}}}"#,
        )
        .unwrap();
        for c in [Capability::Syndicate, Capability::Login, Capability::Import, Capability::Process]
        {
            assert!(m.capabilities.contains(&c), "{c:?}");
        }

        // The old spelling alone still grants everything it named.
        let legacy = PluginManifest::parse(
            r#"{"name":"t","version":"1.0.0","entry":"main.js","capabilities":["process"]}"#,
        )
        .unwrap();
        assert_eq!(legacy.capabilities, vec![Capability::Process]);
        assert!(legacy.contributes.as_ref().unwrap().processor.is_some());
    }

    /// The null case the `processor` kind exists for: a pure process plugin
    /// with `capabilities` derive-only needs one empty object to exist.
    #[test]
    fn a_processor_contribution_grants_process() {
        let m = PluginManifest::parse(
            r#"{"name":"terrarium","version":"0.1.0","entry":"main.js",
                "contributes":{"processor":{}}}"#,
        )
        .unwrap();
        assert_eq!(m.capabilities, vec![Capability::Process]);
        assert!(m.setup_for(&Capability::Process).is_none());
    }

    // ------------------------------------------------------------------
    // The legacy folds
    // ------------------------------------------------------------------

    /// `setup.credentials` is the pre-contract spelling: each entry becomes a
    /// `secret` field, reachable from the same accessors the new surface uses.
    #[test]
    fn legacy_credentials_fold_into_secret_settings() {
        let m = manifest_with_setup(
            r#"{"credentials":[
                {"key":"pinata_jwt","label":"Pinata token","help_url":"https://x.test",
                 "when":{"provider":"pinata"}}],"check":true}"#,
        );
        let setup = setup_of(&m);
        assert_eq!(setup.settings.len(), 1);
        let field = &setup.settings[0];
        assert_eq!(field.field_type, FieldType::Secret);
        assert_eq!(field.label, "Pinata token");
        assert_eq!(field.help_url.as_deref(), Some("https://x.test"));
        // The fold is not serialized back out as `credentials`.
        let json = serde_json::to_string(&m).unwrap();
        assert!(!json.contains("credentials"), "{json}");
        // And a `when` a strict manifest could not declare stays fail-closed
        // at runtime: nothing supplies `provider`, so the secret still asks.
        assert_eq!(setup.applicable_secrets(&config(&[])).len(), 1);
        assert!(setup.applicable_secrets(&config(&[("provider", "local".into())])).is_empty());
    }

    /// The seven parallel maps become fields on the plugin's account-holding
    /// contribution: `config_schema` types, `config` defaults, labels,
    /// descriptions, placeholders, and `enum`+`config_options` as a select.
    #[test]
    fn the_config_maps_fold_into_settings() {
        let m = PluginManifest::parse(
            r#"{"name":"ipfs","version":"1.0.0","entry":"main.js",
                "capabilities":["deploy"],
                "config":{"use_ipns":true,"provider":"pinata"},
                "config_schema":{"use_ipns":"boolean","provider":"enum","gateway":"string",
                                 "max_mb":"number"},
                "config_options":{"provider":["pinata","local"]},
                "config_labels":{"use_ipns":"Stable IPNS name","provider.local":"Local Kubo"},
                "config_descriptions":{"use_ipns":"Publish a stable name."},
                "config_placeholders":{"gateway":"dweb.link"}}"#,
        )
        .unwrap();
        let setup = setup_of(&m);
        let keys: Vec<&str> = setup.settings.iter().map(|f| f.key.as_str()).collect();
        // Sorted by key — the maps carry no order to preserve.
        assert_eq!(keys, ["gateway", "max_mb", "provider", "use_ipns"]);

        let provider = &setup.settings[2];
        assert_eq!(provider.field_type, FieldType::String);
        assert_eq!(provider.options.len(), 2);
        assert_eq!(provider.options[1].label, "Local Kubo");
        assert_eq!(provider.default, Some("pinata".into()));

        let ipns = &setup.settings[3];
        assert_eq!(ipns.field_type, FieldType::Boolean);
        assert_eq!(ipns.label, "Stable IPNS name");
        assert_eq!(ipns.description.as_deref(), Some("Publish a stable name."));
        assert_eq!(ipns.default, Some(true.into()));

        // A field the maps never labelled gets its title-cased key.
        assert_eq!(setup.settings[1].label, "Max Mb");
        assert_eq!(setup.settings[0].placeholder.as_deref(), Some("dweb.link"));
        assert_eq!(setup.settings[1].field_type, FieldType::Number);

        // `config_defaults` carries the fold, so hook contexts and `when`
        // evaluation resolve the same values the form shows.
        assert_eq!(m.config_defaults().get("use_ipns"), Some(&serde_json::json!(true)));
    }

    /// A `config_schema` secret folds like every other field — same store,
    /// same custody, one declaration surviving the merge.
    #[test]
    fn a_legacy_config_schema_secret_folds_to_a_secret_field() {
        let m = PluginManifest::parse(
            r#"{"name":"p","version":"1.0.0","entry":"main.js",
                "capabilities":["syndicate"],
                "config_schema":{"api_key":"secret"}}"#,
        )
        .unwrap();
        let secrets = m.declared_secrets(&config(&[]));
        assert_eq!(secrets.len(), 1);
        assert_eq!(secrets[0].key, "api_key");
        assert_eq!(m.user_supplied_secret_keys(), ["api_key"]);
    }

    /// `group` is native-settings-only: a manifest can name one, and the
    /// legacy `config_schema` fold has no way to produce one.
    #[test]
    fn a_native_field_names_its_group_and_a_legacy_field_gets_none() {
        let m = manifest_with_setup(
            r#"{"settings":[{"key":"gateway","type":"string","label":"Gateway","group":"Publishing"}]}"#,
        );
        assert_eq!(setup_of(&m).settings[0].group.as_deref(), Some("Publishing"));

        let legacy = PluginManifest::parse(
            r#"{"name":"p","version":"1.0.0","entry":"main.js",
                "capabilities":["deploy"],
                "config_schema":{"gateway":"string"}}"#,
        )
        .unwrap();
        assert_eq!(setup_of(&legacy).settings[0].group, None);
    }

    /// A natively-declared key wins over the legacy maps, and the winner makes
    /// a serialize → parse round trip a no-op rather than a duplicate.
    #[test]
    fn native_settings_win_over_the_fold_and_the_fold_is_idempotent() {
        let m = PluginManifest::parse(
            r#"{"name":"p","version":"1.0.0","entry":"main.js",
                "config":{"gateway":"old-default"},
                "config_schema":{"gateway":"string"},
                "contributes":{"deploy_target":{"setup":{"settings":[
                    {"key":"gateway","type":"string","label":"Custom gateway host"}]}}}}"#,
        )
        .unwrap();
        let setup = setup_of(&m);
        assert_eq!(setup.settings.len(), 1);
        assert_eq!(setup.settings[0].label, "Custom gateway host");
        assert!(setup.settings[0].default.is_none(), "the native field said no default");

        let reparsed = PluginManifest::parse(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(setup_of(&reparsed).settings.len(), 1);
    }

    // ------------------------------------------------------------------
    // Accessors across contributions
    // ------------------------------------------------------------------

    /// The hoist: the same block a deploy target declares, declared by a
    /// channel, reached by the same accessor.
    #[test]
    fn a_channel_declares_the_same_setup_block_a_deploy_target_does() {
        let m = PluginManifest::parse(
            r#"{"name":"matters","version":"1.0.0","entry":"main.js",
                "contributes":{"channel":{"display_name":"Matters","imports":true,
                    "login":true,
                    "setup":{"check":true,
                             "settings":[{"key":"tok","type":"secret","label":"Matters token"}]}}}}"#,
        )
        .unwrap();
        let setup = m.setup_for(&Capability::Syndicate).expect("the channel declares setup");
        assert!(setup.check);
        assert_eq!(setup.applicable_secrets(&config(&[])).len(), 1);
        // Same block, whichever of the channel's capabilities asks for it.
        assert!(m.setup_for(&Capability::Login).is_some());
        assert!(m.setup_for(&Capability::Import).is_some());
        // A capability no contribution grants is gated on nothing.
        assert!(m.setup_for(&Capability::Deploy).is_none());
        // And the name resolves off the same match, from the same contribution.
        assert_eq!(m.display_name_for(&Capability::Import), Some("Matters"));
        assert_eq!(m.display_name_for(&Capability::Deploy), None);
    }

    /// The settings page's question: everything moss draws for this plugin,
    /// however many contributions asked for it — deduplicated, because a
    /// channel grants three capabilities off one block.
    #[test]
    fn declared_settings_is_the_union_across_contributions() {
        let m = PluginManifest::parse(
            r#"{"name":"both","version":"1.0.0","entry":"main.js",
                "contributes":{
                  "deploy_target":{"setup":{"settings":[
                      {"key":"provider","type":"string","label":"Provider",
                       "options":[{"value":"pinata","label":"Pinata"},
                                  {"value":"web3storage","label":"web3.storage"}]},
                      {"key":"pinata_jwt","type":"secret","label":"Pinata API token",
                       "when":{"provider":"pinata"}},
                      {"key":"w3_token","type":"secret","label":"web3.storage token",
                       "when":{"provider":"web3storage"}}]}},
                  "channel":{"imports":true,"login":true,"setup":{"settings":[
                      {"key":"matters_token","type":"secret","label":"Matters token"}]}}}}"#,
        )
        .unwrap();

        // Channel first (the settings page's own order), then deploy target.
        let keys: Vec<&str> = m.declared_settings().iter().map(|f| f.key.as_str()).collect();
        assert_eq!(keys, vec!["matters_token", "provider", "pinata_jwt", "w3_token"]);

        // `declared_secrets` filters by `when` against the user's settings.
        let with_provider = config(&[("provider", serde_json::json!("pinata"))]);
        let keys: Vec<&str> =
            m.declared_secrets(&with_provider).iter().map(|f| f.key.as_str()).collect();
        assert_eq!(keys, vec!["matters_token", "pinata_jwt"]);

        // A plugin that declares nothing has nothing for settings to draw.
        let bare = PluginManifest::parse(
            r#"{"name":"b","version":"1.0.0","entry":"main.js","capabilities":["process"]}"#,
        )
        .unwrap();
        assert!(bare.declared_settings().is_empty());
    }

    /// The plugin write arm's exclusion set: a hidden secret is the plugin's
    /// to deposit, a drawn one is the user's — and `when` never narrows the
    /// set, or flipping the setting that hides the condition would open it.
    #[test]
    fn user_supplied_secret_keys_are_the_visible_secrets_unfiltered_by_when() {
        let m = manifest_with_setup(
            r#"{"settings":[
                {"key":"provider","type":"string","label":"P",
                 "options":[{"value":"pinata","label":"Pinata"},{"value":"local","label":"Local"}]},
                {"key":"pinata_jwt","type":"secret","label":"Pinata API token",
                 "when":{"provider":"pinata"}},
                {"key":"session","type":"secret","hidden":true,"label":"Session",
                 "description":"Earned by signing in; never typed."}
            ]}"#,
        );
        assert_eq!(m.user_supplied_secret_keys(), ["pinata_jwt"]);
        // The custody complement: the hidden secret is the plugin's own to
        // deposit, and the two sets partition the declared secrets.
        assert_eq!(m.plugin_owned_secret_keys(), ["session"]);
        // The hidden secret is still a declared secret the settings page and
        // gate can see when applicable.
        assert_eq!(m.declared_secrets(&config(&[("provider", "local".into())])).len(), 1);
    }

    /// A credential is not an account. `Capability::Login` says the plugin
    /// exports `login` for `connect_account` to invoke, so it comes from the
    /// channel saying so — not from the presence of a setup block.
    #[test]
    fn a_setup_block_alone_is_not_a_login() {
        let connects = PluginManifest::parse(
            r#"{"name":"matters","version":"1.0.0","entry":"main.js",
                "contributes":{"channel":{"login":true}}}"#,
        )
        .unwrap();
        assert!(connects.capabilities.contains(&Capability::Login));
        assert!(connects.capabilities.contains(&Capability::Syndicate));

        let token_only = PluginManifest::parse(
            r#"{"name":"webhook","version":"1.0.0","entry":"main.js",
                "contributes":{"channel":{"setup":{"settings":[
                    {"key":"tok","type":"secret","label":"API token"}]}}}}"#,
        )
        .unwrap();
        assert!(!token_only.capabilities.contains(&Capability::Login));
        assert!(token_only.capabilities.contains(&Capability::Syndicate));
        assert!(token_only.setup_for(&Capability::Syndicate).is_some());
    }

    /// An installed plugin manifest predating 2026-08-30 spells the same fact
    /// `requires_login`; losing it would silently drop the connection row and
    /// the account with it.
    #[test]
    fn the_old_requires_login_spelling_still_connects() {
        let m = PluginManifest::parse(
            r#"{"name":"matters","version":"1.0.0","entry":"main.js",
                "contributes":{"channel":{"requires_login":true,"imports":true}}}"#,
        )
        .unwrap();
        assert!(m.capabilities.contains(&Capability::Login));
    }

    /// A default on a legacy credential is refused at the door, and only this
    /// key is — an unknown key stays forward-compatible.
    #[test]
    fn a_legacy_credential_may_not_declare_a_default() {
        let err = PluginManifest::parse(
            r#"{"name":"ipfs","version":"1.0.0","entry":"main.js",
                "contributes":{"deploy_target":{"setup":{"credentials":[
                    {"key":"k","label":"K","default":"paste-yours-here"}]}}}}"#,
        )
        .expect_err("a defaulted credential must not parse");
        assert!(err.to_string().contains("must not declare a `default`"), "{err}");

        assert!(PluginManifest::parse(
            r#"{"name":"ipfs","version":"1.0.0","entry":"main.js",
                "contributes":{"deploy_target":{"setup":{"credentials":[
                    {"key":"k","label":"K","note":"from a newer moss"}]}}}}"#,
        )
        .is_ok());
    }

    /// A serialized manifest is one moss can read back — the folds must not
    /// write a shape the parser then refuses.
    #[test]
    fn a_folded_manifest_round_trips() {
        let m = manifest_with_setup(
            r#"{"credentials":[{"key":"k","label":"K"}],"needs":["binary:ipfs"],"check":true}"#,
        );
        let again = PluginManifest::parse(&serde_json::to_string(&m).unwrap()).unwrap();
        let setup = setup_of(&again);
        assert_eq!(setup.settings.len(), 1);
        assert_eq!(setup.needs, vec![Need::Binary("ipfs".into())]);
        assert!(setup.check);
    }
}
