//! What this build may reuse from the last one.
//!
//! Two independent permissions, and the code that decides them. They live
//! together, away from `build.rs`, because they are one concept answered twice:
//! "is it safe to skip work because nothing that feeds it moved?"
//!
//! Both are resolved HERE, at the entry point, from what the build knows
//! changed ([`ChangeProof`]) and the environment — the render phase reads
//! neither. It receives the answers as plain bools on `SiteConfig`.

use super::PipelineConfig;
use crate::build::media::promise::{Pass, PassOutcome};
use crate::build::BuildTrigger;
use std::path::{Path, PathBuf};

/// The two independent "may this build reuse the last one's work?" permissions.
///
/// They were one boolean until a real 217-file site measured a `footer.md` edit
/// at 94ms in the markdown phase and a `.moss/theme/style.css` edit at 23-67s:
/// the render skip's markdown-only rule was also switching off the parse cache,
/// which a stylesheet cannot invalidate. They are separate fields, not one
/// flag, because the two answers come from different safety arguments — see
/// [`ChangeProof::allows_incremental_skip`] and
/// [`ChangeProof::allows_parse_cache_reuse`] below, which are the only
/// places either is decided.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IncrementalGates {
    /// May the page-render loop carry forward pages whose
    /// fingerprint did not move? `false` without evidence of what changed
    /// (a `Full` build renders every page) and whenever `MOSS_NO_INCREMENTAL` is set.
    pub render_skip: bool,
    /// May Loop A replay the previous build's `ParsedDocument`s?
    /// Same entry-point and kill-switch rules; the difference is which changed
    /// paths it tolerates (markdown plus a parse-irrelevant allowlist).
    pub parse_cache: bool,
}

impl IncrementalGates {
    /// Both permissions for one build, from one [`ChangeProof`]. `builder` is
    /// the fingerprint `run_pipeline` stamps on the manifest it seals.
    pub fn for_build(trigger: &BuildTrigger, folder: &Path, builder: &str, seal: &LastSeal) -> Self {
        let proof = {
            let _trace = crate::build::phase::PhaseTrace::start("change_proof");
            ChangeProof::for_build(trigger, folder, builder, seal)
        };
        crate::build::phase::record_value("proved_from_seal", usize::from(matches!(proof, ChangeProof::Manifest | ChangeProof::ImagesSettled)));
        if let (ChangeProof::Manifest, Some(generation)) = (&proof, seal.generation_id()) {
            crate::build::parse_cache::note_proven_seal(folder, generation);
        }
        Self { render_skip: proof.allows_incremental_skip(), parse_cache: proof.allows_parse_cache_reuse() }
    }
}

/// The last sealed manifest, and this build's `.moss/` measurement against
/// its record. Taken at the entry point, before anything in the build
/// reads `.moss/`: the proof compares the measurement and the manifest
/// records it, so a file edited during the build moves the next record.
///
/// Some inputs are fetched or written and then rendered in the same build:
/// link cards a build fetches before rendering and the comments an exiting
/// build refreshes before its slots. Each lands after this measurement, so the
/// next build renders in full once. A blocking process hook's writes are
/// measured again after it ([`LastSeal::remeasure`]).
/// A move the build makes itself and renders from (an older link card taken
/// into the site folder, the redirect history) is recorded at its seal
/// instead; see [`crate::build::inputs::restamp`].
#[derive(Debug, Default)]
pub struct LastSeal {
    manifest: Option<crate::types::content::SiteHashes>,
    moss: crate::build::inputs::Measured,
    completes: bool,
}

impl LastSeal {
    /// `options` are the build's own, from `inputs::options`;
    /// `completes` is whether this build bakes image placeholders and indexes
    /// search before it seals ([`BuildInputs::complete`](crate::build::inputs::BuildInputs::complete)).
    pub fn read(folder: &Path, options: std::collections::BTreeMap<String, String>, completes: bool) -> Self {
        let _trace = crate::build::phase::PhaseTrace::start("seal_read");
        let manifest = crate::ops::watch::sweep::walk::read_baseline_from_disk(folder);
        let mut seal = Self { manifest, moss: Default::default(), completes };
        seal.remeasure(folder, options);
        seal
    }

    /// Measure `.moss/` again, against the same record, after something wrote
    /// to it; whether it still holds. A build proves from one measurement,
    /// taken after the last such write.
    pub(super) fn remeasure(&mut self, folder: &Path, options: std::collections::BTreeMap<String, String>) -> bool {
        self.moss = crate::build::inputs::measure(folder, self.manifest.as_ref().and_then(|m| m.build_inputs.as_ref()), self.generation_id().as_deref(), options);
        self.moss.holds
    }

    /// What this build records of its `.moss/` reads; see
    /// `PendingManifest::set_build_inputs`.
    pub(crate) fn record(&self) -> Option<crate::build::inputs::BuildInputs> {
        self.moss.record.clone()
    }

    /// Whether `.moss/config.toml` differs from the seal's only in
    /// [`FINGERPRINTED_KEYS`](crate::build::inputs::FINGERPRINTED_KEYS).
    pub(crate) fn config_rest_holds(&self) -> bool {
        let sealed = self.manifest.as_ref().and_then(|m| m.build_inputs.as_ref()).and_then(|r| r.config_rest.as_ref());
        let now = self.moss.record.as_ref().and_then(|r| r.config_rest.as_ref());
        sealed.is_some() && sealed == now
    }

    /// See [`BuildInputs::media_settled`](crate::build::inputs::BuildInputs::media_settled).
    fn media_settled(&self) -> bool {
        self.manifest.as_ref().and_then(|m| m.build_inputs.as_ref()).is_some_and(|r| r.media_settled)
    }

    /// Whether this build completes what the seal left to later passes, so
    /// nothing it sealed may be carried or kept.
    fn completes_what_the_seal_left(&self) -> bool {
        self.completes && !self.manifest.as_ref().and_then(|m| m.build_inputs.as_ref()).is_some_and(|r| r.complete)
    }

    /// Whether the generation this manifest describes is the one `current`
    /// names, and is on disk. The manifest is written before the generation
    /// is promoted, so its presence alone proves neither.
    fn sealed_generation_is_current(&self, folder: &Path) -> bool {
        let Some(id) = self.generation_id() else { return false };
        let paths = crate::moss_paths::MossPaths::new(folder);
        paths.current_generation_id().is_ok_and(|current| current == id) && paths.sealed_generation_dir(&id).is_some()
    }

    /// Whether a build that ends at its entry can hand this seal over: it left
    /// its evidence, and every file it names is in the generation on disk.
    fn hands_over(&self, folder: &Path) -> bool {
        let Some(manifest) = self.manifest.as_ref().filter(|m| m.build_inputs.as_ref().is_some_and(|r| r.evidence.is_some())) else { return false };
        let Some(dir) = self.generation_id().and_then(|id| crate::moss_paths::MossPaths::new(folder).sealed_generation_dir(&id)) else { return false };
        manifest.files.keys().all(|key| crate::build::io_utils::output_present(&dir.join(key)))
    }

    fn generation_id(&self) -> Option<String> {
        self.manifest.as_ref().map(|manifest| crate::build::assets::paths::compute_manifest_generation_id(&manifest.files))
    }

    /// The manifest as read, before key migration; `build_inner` migrates its
    /// own copy, so the proof and the carry judge one read.
    pub(crate) fn manifest(&self) -> Option<&crate::types::content::SiteHashes> {
        self.manifest.as_ref()
    }
}

/// What this build knows changed since the last sealed build.
///
/// The trigger is the caller's fact; this is what the gates make of it.
#[derive(Debug, Clone, PartialEq)]
pub enum ChangeProof {
    /// The watcher's batch, an edit or a create, remove or rename alike.
    /// `config_held`: the batch also edited `.moss/config.toml`, and only in
    /// keys each page's fingerprint covers ([`LastSeal::config_rest_holds`]);
    /// those paths are not in `paths`.
    Events { paths: Vec<PathBuf>, config_held: bool },
    /// The last sealed manifest proves that nothing but markdown pages changed
    /// since; see [`SinceSeal::walk`].
    Manifest,
    /// [`ChangeProof::Manifest`] for a build that renders the pictures the last
    /// render could not finish. Their blur and colour are baked into parsed
    /// HTML, so no parsed page is replayed; the render verdict re-renders the
    /// pages showing a picture whose placeholder moved.
    ImagesSettled,
    /// Nothing is known, so every page renders.
    None,
}

impl ChangeProof {
    pub fn for_build(trigger: &BuildTrigger, folder: &Path, builder: &str, seal: &LastSeal) -> Self {
        if kill_switch() {
            return Self::None;
        }
        match trigger {
            BuildTrigger::Full | BuildTrigger::VideosSettled => Self::None,
            BuildTrigger::Structural(paths) | BuildTrigger::ContentOnly(paths) => {
                let config = crate::moss_paths::MossPaths::new(folder).config();
                let (held, paths): (Vec<_>, Vec<_>) = paths.iter().cloned().partition(|p| *p == config && seal.config_rest_holds());
                Self::Events { paths, config_held: !held.is_empty() }
            }
            // A seal whose media settled after its render leaves pages a later
            // build renders differently; only the re-render those media owe may
            // prove from it.
            BuildTrigger::Unobserved if seal.media_settled() && SinceSeal::walk(folder, builder, seal).is_some() => Self::Manifest,
            BuildTrigger::ImagesSettled { .. } if SinceSeal::walk(folder, builder, seal).is_some() => Self::ImagesSettled,
            BuildTrigger::Unobserved | BuildTrigger::ImagesSettled { .. } => Self::None,
        }
    }

    /// Whether the render phase may skip re-rendering pages whose fingerprint
    /// is unchanged.
    ///
    /// Four conditions, all necessary, in the "over-approximate, never
    /// under-approximate" discipline the whole design rests on:
    ///
    /// 1. `MOSS_NO_INCREMENTAL` unset ([`ChangeProof::for_build`] answers
    ///    `None` under it). The kill switch ships with the first skip, not
    ///    after it — it is the bisect tool for the first stale-page report.
    ///    Any value counts.
    /// 2. The batch is the watcher's, or the proof is [`ChangeProof::Manifest`]
    ///    (an `Unobserved` build: `moss build`, a deploy). Entry points that
    ///    pass `Full` render everything. A `Structural` batch (create, remove,
    ///    rename) is admitted: the parse re-asks every page's links, and the
    ///    render verdict judges a page entering or leaving by the digests of
    ///    what it reaches (listings, nav, folder cards, card answers), with a
    ///    folder, home or generated page still rendering everything
    ///    (`FullCause::FolderPageMoved`).
    /// 3. Every changed path is markdown. A `.moss/config.toml` or `.css`
    ///    edit also arrives as a plain modify, but it moves build-globals
    ///    (`css_version`, `site_url`, layout config) that are baked into
    ///    EVERY page's HTML while changing no page's fingerprint — the one
    ///    way this design could silently serve a stale whole site. The one
    ///    exception is a `config.toml` edit `config_held` vouches for: it moved
    ///    only keys each page's own fingerprint covers.
    /// 4. At least one path — an empty batch carries no information.
    ///
    /// Condition 3 stays MARKDOWN-ONLY here even though the parse cache below
    /// now admits `.css`/`.js`/fonts. Not because a CSS edit is unsafe for the
    /// render skip — `FacadeCache::asset_versions`
    /// (`render/incremental/facade.rs:145-176`) already folds `css_version`,
    /// `user_css_version` and `user_js_version` and yields
    /// `FullCause::AssetVersionsMoved`, so a stylesheet edit forces a full
    /// render through a path that does not consult this flag at all. Relaxing
    /// condition 3 would therefore change nothing for CSS while widening the
    /// blast radius of every OTHER build-global this one boolean happens to
    /// stand in for. Two gates, two rules: this one is coarse and cheap to
    /// reason about, the parse cache's is narrow because that is where the
    /// 23-67s lives.
    ///
    /// [`ChangeProof::Manifest`] meets conditions 2 to 4 by construction: the
    /// proof itself checked that no file was created, removed or renamed, that
    /// only markdown changed and that every build-global input held.
    pub fn allows_incremental_skip(&self) -> bool {
        match self {
            Self::Events { paths, config_held } => (*config_held || !paths.is_empty()) && paths.iter().all(|p| is_markdown_ext(p)),
            Self::Manifest | Self::ImagesSettled => true,
            Self::None => false,
        }
    }

    /// Whether Loop A may replay the previous build's `ParsedDocument`s
    /// (see `build/parse_cache.rs`).
    ///
    /// Conditions 1 and 4 above hold verbatim — same kill switch, same
    /// non-empty batch. The other two differ, and both differences are the
    /// point of having two gates.
    ///
    /// Condition 2 is WIDER: `Structural` is admitted, not just `ContentOnly`.
    /// The trigger is not what makes this safe — [`inputs_fingerprint`]
    /// (`build/parse_cache.rs`) is. It hashes the sorted `markdown_files`
    /// list, so a create, delete or rename of any markdown file moves the
    /// fingerprint and bypasses the cache for the WHOLE build no matter which
    /// trigger carried it. Excluding `Structural` here therefore protected
    /// nothing that was not already protected, while charging a full reparse
    /// — 6.5 s of an 11.9 s rebuild on a 226-page vault — to every batch that
    /// was structural for a reason no markdown parse can see.
    ///
    /// Condition 3 is NARROWER: instead of "every changed path is markdown",
    /// it is "every changed path is markdown OR **parse-irrelevant**".
    ///
    /// A path is parse-irrelevant when its bytes provably cannot change any
    /// markdown file's parse. That is a claim about Loop A's inputs, not about
    /// the page's final HTML: stylesheets, scripts and fonts are referenced by
    /// the SHELL (`css_version` and friends, applied at render time), never
    /// read while a document is parsed. Images are NOT in the list and must
    /// never be — `event_level_image_lookup` bakes dimensions, LQIP and the
    /// `<picture><source>` wrap into parsed HTML, so an image edit replaying a
    /// cached parse would leave the `<picture>` markup stale.
    /// `.moss/config.toml` is likewise excluded: it moves site scalars that
    /// `process_markdown_file` reads. A `config_held` edit is admitted, since
    /// no fingerprinted key is a parse input.
    ///
    /// Unrecognised extensions — and extensionless paths — disable the cache.
    /// The allowlist is the whole safety argument, so it over-approximates by
    /// construction: a new file type is ineligible until someone proves it
    /// parse-irrelevant and adds it here.
    pub fn allows_parse_cache_reuse(&self) -> bool {
        match self {
            Self::Events { paths, config_held } => (*config_held || !paths.is_empty())
                && paths.iter().all(|p| is_markdown_ext(p) || is_parse_irrelevant_ext(p)),
            Self::Manifest => true,
            Self::ImagesSettled | Self::None => false,
        }
    }
}

/// What a build with no events finds changed since the last seal: nothing, or
/// only the text of markdown pages. `None` from [`SinceSeal::walk`] is
/// anything else, or not known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SinceSeal {
    Nothing,
    OnlyPages,
}

impl SinceSeal {
    /// Whether a build with no events can prove from the last sealed manifest
    /// that nothing, or only the text of markdown pages, changed since.
    /// `ChangeProof::for_build` and `AtEntry::check` both ask it.
    ///
    /// Each rule below answers no:
    ///
    /// - The manifest is missing or unreadable, or a different moss binary
    ///   sealed it (its templates are an input no file shows).
    /// - The seal left the search index or image placeholders to later
    ///   passes and this build completes them
    ///   ([`BuildInputs::complete`](crate::build::inputs::BuildInputs::complete)).
    /// - Its [`BuildInputs`](crate::build::inputs::BuildInputs) are missing,
    ///   or a `.moss/` file, a link card or an environment variable they
    ///   recorded moved. They name what the last build read outside the page
    ///   tree, so no list of such inputs is kept here.
    /// - The page tree moved in a way the watcher would have called
    ///   structural, or a non-markdown file changed. The measurement is the
    ///   watch sweep's own, judged by the same three-tier compare against the
    ///   manifest's capture clock, so a file the sweep would call unchanged is
    ///   unchanged here too.
    /// - Anything is still in the cloud, could not be judged, or took longer
    ///   than the sweep's own pass budget.
    fn walk(folder: &Path, builder: &str, seal: &LastSeal) -> Option<Self> {
        use crate::build::watch::drift;
        use crate::ops::watch::sweep::{walk, WALK_DEADLINE};
        let baseline = seal.manifest.as_ref()?;
        if baseline.builder_fingerprint.as_deref() != Some(builder) || !seal.moss.holds || seal.completes_what_the_seal_left() {
            return None;
        }
        let deadline = std::time::Instant::now() + WALK_DEADLINE;
        let walked = walk::walk_with_error(folder, Some(deadline), None).ok()?;
        if walked.deadline_blown || !walked.dataless.is_empty() {
            return None;
        }
        let report = drift::detect_drift(baseline, &walked.files, &[], folder, drift::probe_missing, Some(deadline), true);
        let only_pages = report.drifted.iter()
            .all(|(rel, drift)| drift.kind == drift::DriftKind::Edited && crate::build::manifest::is_page_source_key(rel));
        if report.deadline_blown || report.offline > 0 || !only_pages {
            return None;
        }
        Some(if report.drifted.is_empty() { Self::Nothing } else { Self::OnlyPages })
    }
}

/// What a one-shot build with no events does at its entry, before it scans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AtEntry {
    /// Nothing the last seal read has moved, and the generation it sealed is
    /// what `current` names, whole, with the evidence the seal left: that
    /// generation is this build's output.
    Unchanged,
    /// Build. `comments_refreshed` when the comment refresh already ran here,
    /// so the slot pass does not ask the server a second time.
    Build { comments_refreshed: bool },
}

impl AtEntry {
    /// Decide under the folder lock. `fetch` runs the passes that read from
    /// outside the folder (comments, link cards) once the folder itself is
    /// shown unchanged; what they write lands in rows `BuildInputs` measures,
    /// so measuring again after them answers whether they brought anything,
    /// and a build that goes on proves from what they wrote, whatever `fetch`
    /// answers. `fetch` answers `None` when the build must render: it cannot
    /// say what the folder is missing, or it brought something.
    pub(crate) fn check(folder: &Path, builder: &str, seal: &mut LastSeal, options: std::collections::BTreeMap<String, String>, fetch: impl FnOnce() -> Option<bool>) -> Self {
        let _trace = crate::build::phase::PhaseTrace::start("entry_check");
        if kill_switch() {
            return Self::Build { comments_refreshed: false };
        }
        let Ok(_lock) = crate::infra::folder_lock::acquire(folder) else { return Self::Build { comments_refreshed: false } };
        let Some(recorded) = seal.manifest.as_ref().and_then(|m| m.build_inputs.as_ref()) else { return Self::Build { comments_refreshed: false } };
        if !recorded.day.as_ref().is_some_and(|d| d.is_today()) || !seal.media_settled() || !seal.sealed_generation_is_current(folder) || !seal.hands_over(folder) || SinceSeal::walk(folder, builder, seal) != Some(SinceSeal::Nothing) {
            return Self::Build { comments_refreshed: false };
        }
        let fetched = fetch();
        match (fetched, seal.remeasure(folder, options)) {
            (Some(_), true) => {
                if let Some(generation) = seal.generation_id() {
                    crate::build::parse_cache::note_proven_seal(folder, generation);
                }
                Self::Unchanged
            }
            (comments_refreshed, _) => Self::Build { comments_refreshed: comments_refreshed.unwrap_or(false) },
        }
    }
}

/// Whether this build ends at its entry, and what it reports if it does; when
/// it must build, what the check learned on the way. A serving build never
/// waits on the network here: its outside reads are the background sync's, as
/// for a build that renders.
pub(super) async fn stop_at_entry(config: &PipelineConfig, pass: Pass, seal: &mut LastSeal, options: std::collections::BTreeMap<String, String>, port: Option<u16>) -> Result<PassOutcome, AtEntry> {
    if !config.may_stop_at_entry(pass) {
        return Err(AtEntry::Build { comments_refreshed: false });
    }
    let paths = crate::moss_paths::MossPaths::new(config.source());
    let at_entry = AtEntry::check(config.source(), &crate::plugins::fingerprint::builder_fingerprint(), seal, options, || match config.start_server {
        true => Some(false),
        false => fetch_before_stop(pass.plugins, config.root.as_str(), &paths),
    });
    if at_entry != AtEntry::Unchanged {
        return Err(at_entry);
    }
    hand_over(config, pass, seal, port).await.ok_or(AtEntry::Build { comments_refreshed: false })
}

/// What a build that ended at its entry reports: the generation it kept.
fn unchanged_at_entry(config: &PipelineConfig, port: Option<u16>) -> PassOutcome {
    crate::build::phase::record_value("unchanged_at_entry", 1);
    PassOutcome { message: format!("Unchanged since the last build.\n{}", super::generated_message(config, port)), owed: false, is_empty: false, port }
}

/// What a build that stopped at its entry still owes its caller: the kept
/// seal and every record the publish gate reads, the publish change set, and
/// for a serving build, staging shown as the seal left it, the background sync
/// and `Build complete`. `None` when staging cannot be shown as it is; the
/// caller then builds.
async fn hand_over(config: &PipelineConfig, pass: Pass, seal: &LastSeal, port: Option<u16>) -> Option<PassOutcome> {
    use crate::build::manifest::backfill;
    let sealed = crate::build::manifest::SealedManifest::kept(seal.manifest()?.clone())?;
    let manifest = sealed.site_hashes_view();
    let paths = crate::moss_paths::MossPaths::new(config.source());
    let generation = paths.current_generation_id().ok()?;
    if config.start_server {
        crate::build::pipeline::reveal_sealed_staging(&paths, manifest, &generation, config.host.services.cancellation.protected_outputs())?;
        config.host.services.reporter.stage_ready(&paths.staging_dir());
    }
    let folder = config.root.as_str();
    let records = crate::system::build_records::records();
    records.record_content_hashes(folder, manifest.clone());
    records.install_kept_seal(folder, manifest.build_inputs.as_ref()?.evidence.as_ref()?);
    let removed = backfill::removed_for_seal(&paths, manifest);
    records.record_removed_addresses(folder, removed.clone());
    let verdict = backfill::for_seal(&paths, Some(manifest)).await;
    let ask = match (&verdict, &config.host.server_diff) {
        (Some(backfill::SealVerdict::AskServer), Some(port)) => Some((port, manifest.files().into_owned(), generation)),
        _ => None,
    };
    config.host.announcer.adopt_sealed(sealed.clone()).await;
    config.host.announcer.publish_change_set(config.source(), backfill::settle(verdict, ask, removed).await).await;
    if config.start_server {
        if !matches!(pass.plugins, crate::build::PluginMode::SlotsOnly) {
            let domain = crate::build::site_config::get_domain_config(folder).unwrap_or_default();
            crate::build::features::sync::start_background_sync(folder.to_string(), domain, config.host.events.clone(), &*config.host.spawner);
        }
        super::emit_build_complete(&config.progress, port, false, None);
    }
    Some(unchanged_at_entry(config, port))
}

/// The passes a stopped build still owes the site, since they read from
/// outside the folder: another try at each link card the sealed generation
/// showed waiting, and the comment refresh. Returns whether the comments were
/// refreshed, or `None` when the build must render: which cards are waiting is
/// not known, or one arrived.
fn fetch_before_stop(plugins: crate::build::PluginMode, folder_path: &str, paths: &crate::moss_paths::MossPaths) -> Option<bool> {
    let cards = crate::phase!("link_meta_build_fetch", { crate::build::page::link_meta::settle(paths) })?;
    if cards.fetched > 0 {
        return None;
    }
    let refresh = !matches!(plugins, crate::build::PluginMode::SlotsOnly);
    if refresh {
        let _trace = crate::build::phase::PhaseTrace::start("comment_refresh");
        let domain = crate::build::site_config::get_domain_config(folder_path).unwrap_or_default();
        crate::build::features::sync::refresh_comments_for_exiting_build(folder_path.to_string(), domain);
    }
    Some(refresh)
}

/// The bisect tool for a stale-page report: any value turns both gates off.
fn kill_switch() -> bool {
    std::env::var("MOSS_NO_INCREMENTAL").is_ok()
}

/// `foo.md` / `foo.markdown` — the only sources Loop A parses.
fn is_markdown_ext(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(is_markdown_extension)
}

impl PipelineConfig {
    /// May image dominant-colour/LQIP extraction be deferred off the blocking
    /// scan, to be enriched later by the background media phase?
    ///
    /// The third permission of the same family as the two above, and it was
    /// decided the same wrong way they once were: from a flag that means
    /// something adjacent. `defer_placeholders` read `start_server`, which is
    /// orchestration ("this build launches the server") and true only for the
    /// folder-open build — so every WATCH rebuild took the deploy path and
    /// full-decoded every image with no content-hash cache entry. Measured on
    /// a 876-image vault: 794 decodes, 14s of a 16.0s rebuild, in front of the
    /// user's first save; and colour/LQIP flipping None → Some moves the
    /// SURFACE of every page showing those images, forcing a whole-site
    /// re-render on top.
    ///
    /// Ask the question by name. `server_port` is the watcher's own "a server
    /// is already serving this folder"; it is conservative, so a false
    /// negative only costs work and can never ship a bare placeholder. Deploy,
    /// plugin-install and CLI builds leave it `None` and keep baking.
    pub fn defers_image_placeholders(&self) -> bool {
        self.start_server || self.server_port.is_some()
    }

    /// May this build end at its entry, unchanged ([`AtEntry`])? A build with no
    /// events, on its first pass, with no plugin hook that brings content in
    /// before the scan, that either opens a preview (it then shows staging as
    /// the seal left it) or is one-shot with no server and no shell.
    pub(crate) fn may_stop_at_entry(&self, pass: crate::build::media::promise::Pass) -> bool {
        use crate::build::PluginMode;
        matches!(self.trigger, BuildTrigger::Unobserved)
            && (self.start_server || (self.exits_after_build && !self.defers_image_placeholders() && !self.host.shell_attached))
            && !pass.repeat
            && (matches!(pass.plugins, PluginMode::Skip | PluginMode::SlotsOnly) || !self.host.plugins.get_or_create(self.root.as_str()).is_ok_and(|m| m.has_process_hooks()))
    }
}

/// [`is_markdown_ext`] over a bare extension, so callers holding a
/// project-relative `&str` key (the manifest's `sources`) share the one answer.
pub(crate) fn is_markdown_extension(ext: &str) -> bool {
    crate::build::scan::classify::is_page_source(&ext.to_ascii_lowercase())
}

/// Extensions whose bytes are never read while a markdown file is parsed.
///
/// Presentation-layer assets only: stylesheets, scripts, web fonts. Everything
/// they affect is applied at render time from a version stamp, which the render
/// side gates independently (`FacadeCache::asset_versions`). Deliberately NOT
/// here: any image or media extension (dimensions and LQIP are baked into
/// parsed HTML), `.toml` (site scalars), `.json`, `.yaml`, and anything else.
const PARSE_IRRELEVANT_EXTENSIONS: &[&str] =
    &["css", "js", "mjs", "woff", "woff2", "ttf", "otf", "eot"];

fn is_parse_irrelevant_ext(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| PARSE_IRRELEVANT_EXTENSIONS.iter().any(|k| e.eq_ignore_ascii_case(k)))
}

#[cfg(test)]
#[path = "incremental_gates_tests.rs"]
mod tests;
