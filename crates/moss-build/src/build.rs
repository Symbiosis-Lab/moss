//! moss build orchestration - Static site generation from folders
//!
//! This module orchestrates the build process including:
//! - Tauri commands for GUI and CLI build
//! - Preview server integration
//! - File watching for live development
//!
//! ## Module Structure
//! - `pipeline` - Core build orchestration with zero-flicker staging pattern
//! - `video` - Video transcoding and conversion pipeline
//! - `media::pipeline` - Asset copying, cleanup, and synchronization
//! - `watch` - File watching for live development mode
//! - `render` - Pure static site generation (no Tauri dependencies)
//!
//! # Build directory model: stage and site
//!
//! moss writes every build to two directories:
//!
//! - `stage_dir/` (`.moss/build/staging/`) — writer surface + at-rest reader.
//!   All emits write here. HTML carries preview-only annotations
//!   (`data-source-line`, `data-source-range`, `data-source-fm`, `data-source-none`,
//!   `data-moss-preview`) used by editor↔preview scroll-sync and click-to-source.
//!   The preview server reads from stage when no rebuild is in progress.
//!
//! - `generations/<gen-id>/` — immutable frozen deploy output. Written once by
//!   `materialize_and_promote` after sealing. The `current` symlink points to
//!   the active generation; deploy reads from it. During a rebuild, the preview
//!   server temporarily switches to `current` (the previous frozen gen) to avoid
//!   flicker. After the rebuild's blocking phase completes, preview switches
//!   back to stage.
//!
//! The bridge between them is the **ship** operation:
//!
//! - **Phase ship** (`ship_phase` in `build/ship.rs`): once at the end of the
//!   blocking phase, mirroring everything emitted so far from stage to site.
//!   Produces the "frozen good build" that the next rebuild's preview-fallback
//!   switch reads. Replaces the former `copy_dir_all + sync_dir` pair.
//!
//! Ship is parameterized by extension-keyed transform (`ship::transform_for`):
//! HTML gets `StripPreviewAttrs`; everything else gets `CopyAsIs`. Adding a
//! future transform (minification, sourcemap removal, etc.) is a one-line
//! addition to `transform_for`.
//!
//! ## Reader/writer state machine
//!
//! ```text
//! state                  | preview reads | stage state | site state
//! -----------------------+---------------+-------------+----------
//! idle (no rebuild)      | stage         | last build  | last build (frozen)
//! rebuild blocking phase | site          | overwriting | last build (frozen)
//! after blocking ship    | stage         | new build   | new build (frozen)
//! deferred phase         | stage         | new build + | new build +
//!                        |               | bg outputs  | bg outputs (live)
//! ```
//!
//! `SiteDirectoryState::switch_to` calls in `build/pipeline.rs` implement
//! these transitions. Do not remove without first reading
//! `docs/archive/2026-05-04-stage-vs-site-audit.md`.
//!
//! ## Lifecycle types
//!
//! - `manifest::PendingManifest` / `SealedManifest` — typestate seal pattern.
//!   `seal()` enforces the `blocking_keys ⊆ (files ∪ image_outputs)` invariant
//!   at one point (closes #552).
//! - `manifest::HashBucket` — disambiguates registration buckets.
//! - `context::BuildContext` — emit handle. `for_render` (blocking phase,
//!   direct manifest) and `for_deferred_stage_only` (background phase,
//!   coordinator channel). Both write to stage only; `ship_phase` mirrors at
//!   the blocking-phase boundary.
//! - `coordinator::ManifestCoordinator` — single-writer drain of background
//!   emits. Channel close = materialization barrier → seal.
//! - `background::BackgroundHandle` — JoinSet wrapper. `await_completion`
//!   joins all workers, awaits coordinator, returns `SealedManifest`.
//! - `ship::ship_phase` — produces the site/ derivative.
//!
//! See `docs/archive/2026-05-03-generated-artifact-plan.md` for the refactor's
//! issue context (#524 single emit API, #552 seal invariant) and the audit
//! tables of the 18 emit sites Phase 3 will convert.

pub mod pipeline;
pub mod cloud_ledger;
pub(crate) mod cloud_prefetch;
pub mod cloud_provider;
pub mod cloud_readiness;
pub mod icloud;
pub mod markdown;
pub mod media;
pub mod render;
// Back-compat shim: video has moved to build::media::video. Remove after Phase 3 generator/ cleanup (Task 11).
pub(crate) use media::video;
// Back-compat shim: image has moved to build::media::image. Remove after Phase 3 generator/ cleanup (Task 11).
pub(crate) use media::image;
pub mod watch;
pub mod features;
pub mod components;
pub mod background;
pub mod context;
pub mod coordinator;
pub mod manifest;
pub mod progress;
pub mod ports;
pub(crate) mod process_hooks;
pub use process_hooks::spawn_process_hooks;
pub mod ship;
pub mod terms;
pub mod types;
// New top-level buckets (Task 11 — Phase 3 generator/ redistribution)
pub mod assets;
pub mod cache;
// What a `moss build` prints, and the problem count `--strict` reads. Split out
// of `crate::diagnostics` because the rest of that module is tauri-plugin-log.
pub mod cli_output;
pub mod degrade;
pub mod embed_handlers;
pub mod emit;
pub mod facade;
pub mod incremental_gates;
pub mod folder_embed;
pub mod folder_index;
pub mod feeds;
pub mod footer;
pub mod highlight;
pub mod io_utils;
pub mod media_collection;
pub mod notebook;
pub mod outcome;
pub mod parse_cache;
pub mod served_path;
pub mod page;
pub mod phase;
pub mod prior_output;
pub mod scan;

pub mod site_config;
pub mod site_meta;
pub mod site_url;
pub mod slots;
pub mod enhance;
pub mod store_gc;
pub mod svg_util;
pub mod theme_lint;

// Re-export for public API (used by integration tests via lib.rs)
pub use scan::scan::scan_folder;
pub use scan::scan::scan_folder_with_dedup;
// Re-export for deploy sync (push_site reads the hashes manifest)
pub use pipeline::load_previous_hashes;

use crate::build::feeds::search_lane;
use crate::build::outcome::{retry_once_after_discard, BuildStopped};
use crate::plugins::{ProcessContext, ProjectInfo};
use crate::types::{content::ProjectStructure, runtime::SiteDirectoryState};
use crate::vault::paths::VaultRoot;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Pipeline configuration types (ADR-010, Phase 2)
// ---------------------------------------------------------------------------

/// How progress events are delivered.
///
/// Design note (ADR-010): Progress is a property of the pipeline, not a
/// separate concept. The pipeline emits events; the sink decides where they go.
///
/// This was an enum with a `Channel(tauri::ipc::Channel, tauri::AppHandle)`
/// variant until 2026-08-17. Naming tauri in the type meant every file that
/// held a sink named tauri too, which `moss-build` may not do (ADR-050). The
/// three variants became the three implementations in [`reporter`]; the
/// tauri-backed one is `crate::events::TauriReporter`, app-side.
///
/// `Arc` rather than `Box`: the sink is cloned into `spawn_blocking` closures.
pub type ProgressSink = std::sync::Arc<dyn ports::reporter::BuildReporter>;

/// Discard every event — tests and headless builds.
pub fn null_sink() -> ProgressSink {
    std::sync::Arc::new(ports::reporter::NullReporter)
}

/// Print human-readable progress to stderr — CLI mode.
pub fn stdout_sink() -> ProgressSink {
    std::sync::Arc::new(ports::reporter::StdoutReporter)
}

/// How plugins participate in the build.
///
/// Design note (ADR-010): Inspired by Rollup's hook ordering -- the pipeline
/// is mode-agnostic, branching via config not code path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginMode {
    /// Wait for process hooks before build (moss build)
    Blocking,
    /// Fire process hooks, don't wait (preview mode)
    NonBlocking,
    /// Skip process hooks, collect slot content only (watch-mode rebuilds).
    /// Process hooks fetch external data (e.g., Matters sync) and should only
    /// run on the initial build, not on every file-edit rebuild.
    SlotsOnly,
    /// Skip all plugin hooks (--no-plugins)
    Skip,
}

/// Classification of what triggered this build (moss#922 Stage 2).
///
/// Read by `incremental_gates` (moss#968), which resolves both incremental
/// gates from it at the entry point. The watch path classifies a debounced
/// batch once, where the raw `notify` event kinds are still available, instead
/// of re-deriving that classification later from a `Vec<PathBuf>` with no kind
/// information. `Full` is the only correct value once path information is
/// unavailable or ambiguous — never guess a narrower variant from partial
/// data (Bazel/Make's over-approximate-never-under-approximate discipline).
#[derive(Debug, Clone, Default, PartialEq)]
pub enum BuildTrigger {
    /// No usable path information, or the batch could affect the site's
    /// structure (create/remove/rename) — must be treated as a full rebuild.
    #[default]
    Full,
    /// Every changed path in the batch is a create/remove/rename — narrower
    /// than `Full` in *kind* even though it still implies a full rebuild
    /// today; carries paths for the day something downstream can act on it.
    Structural(Vec<PathBuf>),
    /// Every changed path in the batch is a plain content edit to an
    /// existing file — no create/remove/rename anywhere in the batch.
    ContentOnly(Vec<PathBuf>),
}

impl BuildTrigger {
    /// Coalesce two triggers whose batches are being merged into one build.
    ///
    /// Safe rather than a guess because the gates that read this
    /// (`incremental_gates`) are predicates over the PATH SET, not over "was
    /// this one debounced batch": the union of two content-only batches is
    /// exactly as safe as one batch that had carried both paths, since a
    /// non-markdown path on either side fails the union's check as it would
    /// have failed its own. Everything else over-approximates, as
    /// `classify_trigger` does — `Structural` wins over `ContentOnly`, and
    /// `Full`, which carries no paths and so no evidence, absorbs both.
    ///
    /// Before this, `watch::worker::merge_requests` returned `Full`
    /// unconditionally, so every save landing while a build was in flight lost
    /// its classification — self-reinforcing, because the resulting full build
    /// (8-18s on a large vault against ~1.5s) all but guarantees the next
    /// keystroke also arrives mid-build.
    pub fn merge(self, other: BuildTrigger) -> BuildTrigger {
        use BuildTrigger::{ContentOnly, Full, Structural};
        fn union(mut a: Vec<PathBuf>, b: Vec<PathBuf>) -> Vec<PathBuf> {
            for p in b {
                if !a.contains(&p) {
                    a.push(p);
                }
            }
            a
        }
        match (self, other) {
            (Full, _) | (_, Full) => Full,
            (Structural(a), Structural(b))
            | (Structural(a), ContentOnly(b))
            | (ContentOnly(a), Structural(b)) => Structural(union(a, b)),
            (ContentOnly(a), ContentOnly(b)) => ContentOnly(union(a, b)),
        }
    }
}

/// The host seam — [`ports::host::HostPorts`], [`ports::host::HostStore`]
/// and the closure aliases that travel with them — re-exported at the
/// historic `crate::build::` spelling its consumers already use.
///
/// The declarations moved into `build/ports/` on 2026-08-27 so ratchet row (o)
/// can see them; see that module for why the old placement made NORTH-STAR's
/// abort threshold unfalsifiable.
pub use ports::host::{HostPorts, HostStore, ServerDiff, ServerDiffFuture, ServerFuture};

/// Single configuration struct for all pipeline behavior.
///
/// Design note (ADR-010): Inspired by Vite's mode-agnostic config.
/// Each entry point builds a PipelineConfig and calls run_pipeline().
/// There is no branching logic inside the build -- all behavioral
/// differences are expressed in the config.
pub struct PipelineConfig {
    /// The vault root this invocation builds — resolved ONCE, at the entry point.
    /// Carries the load-bearing folder name; nothing downstream re-derives it.
    pub root: VaultRoot,
    /// How to deliver progress events
    pub progress: ProgressSink,
    /// Plugin participation mode
    pub plugins: PluginMode,
    /// Whether the host starts file watching after this build. Informational:
    /// the watch is host-started; nothing inside `run_pipeline` reads it.
    pub watch: bool,
    /// Start the embedded preview server after this build completes.
    /// Orchestration ONLY — never read this as "is this a preview build":
    /// build output is mode-independent by design
    /// (docs/archive/2026-06-10-email-footer-mode-independent-design.md).
    /// The folder-open build passes true; watch rebuilds pass false because
    /// the server is already running — any content gated on this flag
    /// flip-flops on disk between those two triggers (the 2026-06-10 email
    /// footer bug).
    pub start_server: bool,
    /// What the host provides to this build — see [`HostPorts`]. This
    /// replaced `app: Option<tauri::AppHandle>` (2026-08-27, M6a plan C1).
    pub host: HostPorts,
    /// What triggered this build (watch mode) — see `BuildTrigger`.
    /// Read by `allows_incremental_skip` (moss#922 Stage 5b).
    pub trigger: BuildTrigger,
    /// True when this build is invoked from a process that will exit shortly
    /// after `run_pipeline` returns (CLI subcommands, snapshot tests). Such
    /// callers cannot rely on `spawn_native_process_sync`'s background task
    /// to populate the link-meta cache — it gets killed before completion.
    /// When set, `run_pipeline` synchronously prewarms link-meta before
    /// render. Defaults to `false`; only `start_cli_build` and `build_sync`
    /// flip it.
    ///
    /// Note: this is NOT derivable from `progress` or the host ports. The
    /// moss binary is a Tauri app even in CLI mode, so a shell can be
    /// attached. And `progress: Null` is used by GUI-initiated rebuilds
    /// (deploy, plugin install, watch) where the runtime IS alive — those
    /// must NOT prewarm synchronously.
    pub exits_after_build: bool,
    /// CLI `--site-url` override. When `Some`, the value is passed through to
    /// `generate_blocking_content` via `SiteConfig` and ultimately consumed by
    /// `resolve_site_url` (Task 2.5). `None` everywhere except the CLI entry
    /// point that received the flag.
    pub site_url_override: Option<String>,
    /// A preview server already serving this folder, as its dispatcher knew it.
    /// Only the watcher sets it; `None` does not mean nothing is serving.
    pub server_port: Option<u16>,
    /// Promotion epoch minted at ADMISSION by the per-folder rebuild worker
    /// (`build_shell::watch::worker`), `None` for every other caller.
    ///
    /// The epoch is what orders this build against other builds of the same
    /// folder at promotion time (`ship::try_promote`). Minting it when the
    /// worker dequeues the request — rather than when the build finishes —
    /// means a build that wedges and un-wedges after its successor can never
    /// mint a newer epoch and promote stale output over it (phase 1a of
    /// docs/archive/2026-08-18-watcher-reliability-architecture.md; the
    /// zombie-promotes-stale defect). `None` preserves the old post-build
    /// mint exactly, which is safe wherever builds of a folder are strictly
    /// serialized (CLI, deploy, plugin install).
    pub admission_epoch: Option<u64>,
    /// What is serving this folder RIGHT NOW — supplied by the app half
    /// (`build_shell::live_port_resolver`), `None` for CLI and headless builds.
    /// Both fields' reasoning lives on
    /// [`ports::LivePortResolver`](crate::build::ports::LivePortResolver).
    pub live_port: Option<crate::build::ports::LivePortResolver>,
}

impl PipelineConfig {
    /// The source folder path (was the `source: PathBuf` field).
    pub fn source(&self) -> &Path { self.root.path() }

}

/// The reporter to use for Tier-1 (loading-screen) progress, if anything is
/// listening for it.
///
/// `None` is not "no reporter" — it is "Tier-1 has nowhere to go", which is the
/// case for stdout and null. That distinction is load-bearing: before this was
/// a port it read `Option<&Channel>`, so in CLI mode `send_progress` was a
/// no-op and the CLI printed its own coarse progress instead. Returning the
/// reporter unconditionally here would push every fine-grained pipeline step to
/// stderr. See `reporter::BuildReporter::wants_tier1`.
pub(crate) fn tier1_reporter(sink: &ProgressSink) -> Option<ProgressSink> {
    sink.wants_tier1().then(|| sink.clone())
}

/// Lightweight homepage detection for use outside the full scan pipeline.
///
/// Lists `.md` files in the root directory and uses moss-core's home file
/// detection to find the homepage (index.md, readme.md, _index.md, etc.).
/// Does NOT do a full recursive scan — just a single directory listing.
///
/// Skips root agent-config files, which the scanner also skips but this raw
/// `read_dir` would not: `detect_home_file_in_folder` falls back to "first
/// document alphabetically", so `AGENTS.md` wins any folder without an
/// `index.md` and elects a homepage the scan refuses to publish.
pub fn quick_detect_homepage(root: &VaultRoot) -> Option<String> {
    let filenames: Vec<String> = std::fs::read_dir(root.path())
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
        .filter(|e| !scan::classify::is_agent_config_name(&e.file_name().to_string_lossy()))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    let filename_refs: Vec<&str> = filenames.iter().map(|s| s.as_str()).collect();
    moss_core::home::detect_home_file_in_folder(&filename_refs, root.name())
        .map(|s| s.to_string())
}

/// Resolve the site name from the homepage file's frontmatter title or folder
/// name, at context-construction time (before the full build pipeline runs).
///
/// The structural index-stem-vs-folder-name decision is owned by
/// `moss_core::home::site_name` — the SAME owner the render pipeline's
/// `<title>`/`og:title` route through (#775) — so this no longer duplicates
/// the pipeline's title logic. The only work done here is the legitimate I/O:
/// reading the homepage file to recover its frontmatter `title:`.
///
/// Body-`# H1` is intentionally NOT consulted: moss never sources a page title
/// from body content (Obsidian-match, 2026-05-30; see
/// `docs/reference/title-rendering.md`). The previous `extract_h1_heading`
/// fallback here contradicted that rule and was the duplicate this consolidates
/// away.
///
/// Note: The lightweight frontmatter parser handles unquoted, double-quoted,
/// and single-quoted title values. It does not handle multi-line YAML values
/// or escaped quotes — acceptable for site title use cases.
pub fn resolve_site_name(root: &VaultRoot, homepage_file: Option<&str>) -> Option<String> {
    // Legitimate I/O: read the homepage's frontmatter `title:` (if any).
    let frontmatter_title = homepage_file.and_then(|homepage| {
        let full_path = root.path().join(homepage);
        std::fs::read_to_string(&full_path)
            .ok()
            .and_then(|content| extract_frontmatter_title(&content))
            .filter(|t| !t.trim().is_empty())
    });

    // Delegate the structural decision (index-stem/self-named → folder name,
    // else frontmatter title, else folder name) to the shared owner.
    // `name_opt()` keeps the `None`-for-`/` shape this function always had.
    root.name_opt().map(|folder| {
        moss_core::home::site_name(homepage_file, frontmatter_title.as_deref(), folder)
    })
}

/// Extract the `title` field from a document's frontmatter (either dialect).
///
/// Performs a lightweight line-by-line parse -- no full YAML parser needed.
fn extract_frontmatter_title(content: &str) -> Option<String> {
    // Where the block ends is `frontmatter_span`'s call, for both dialects; a
    // simplified-frontmatter page used to come back titleless (moss#937).
    let span = moss_core::frontmatter::frontmatter_span(content)?;
    // Char-aligned: `fields` is a line-boundary range from the splitter.
    #[allow(clippy::string_slice)]
    let fields = &content[span.fields];

    for line in fields.lines() {
        // Only match top-level keys (not indented nested YAML)
        if let Some(rest) = line.strip_prefix("title:") {
            let value = rest.trim();
            // Strip surrounding quotes if present
            let value = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                .unwrap_or(value);
            return Some(value.to_string());
        }
    }

    None
}

fn collect_native_slots_for_documents(
    folder_path: &str,
    cached_domain_config: crate::config::deployment::DomainDeploymentConfig,
    project_structure: &ProjectStructure,
    pages: Vec<crate::build::types::ParsedDocument>,
    start_server: bool,
    site_lang: &str,
    site_comments: Option<bool>, // see generate_native_slots's own param doc
) -> crate::build::enhance::ResolvedSlots {
    // The frontmatter-analytics migration runs at build entry
    // (`HostStore::run_vault_migrations`, ADR-059), so this read already sees
    // the migrated value.
    let services_config = crate::build::site_config::get_services_config(folder_path)
        .unwrap_or_default();

    let is_moss_hosted = cached_domain_config.deploy_method.as_deref() == Some("moss");
    let channels_config = crate::plugins::discovery::get_channels_config(folder_path)
        .unwrap_or_default();
    let email_channel_installed = channels_config.is_installed("email");
    let matters_domain = crate::build::features::comment::resolve_matters_domain(
        &crate::build::site_config::resolve_environment(folder_path),
        None,
    )
    .unwrap_or_else(|| {
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK.to_string()
    });
    let comments_enabled = services_config.is_enabled(
        crate::config::services::ServiceKind::Comments,
        is_moss_hosted,
    );
    let has_inline_subscribe = pages.iter().any(|p| p.features.inline_subscribe);
    let has_inline_apply = pages.iter().any(|p| p.features.inline_apply);
    let has_footer_file = pages.iter().any(|p| p.slot_only);

    // Site fingerprint: a PII-free, one-line summary of the build-shaping state
    // that decides footer chrome + slot resolution. Emitted here — BEFORE the
    // no-services early-return below — so it fires on every STANDARD build,
    // including the services-free case (email not installed, no footer.md) that
    // surfaced the footer stray-">" report. Rides
    // "Send logs"; names/counts only, no user content. This is the context that
    // was missing when diagnosing that report.
    log::info!(
        target: "moss::build",
        "site fingerprint: pages={}, email_installed={}, comments={}, moss_hosted={}, footer_md={}, inline_subscribe={}, inline_apply={}, analytics={}",
        pages.len(),
        email_channel_installed,
        comments_enabled,
        is_moss_hosted,
        has_footer_file,
        has_inline_subscribe,
        has_inline_apply,
        services_config.analytics.is_some(),
    );

    // Record the site's audiences ({scope, lang}) so every reader — the email
    // settings command, the syndicate hook — gets EXACTLY what the build
    // derived, with no re-derivation drift. Same judge
    // (`derive_language_sections`) as the subscribe form's supported_scopes, so
    // form and picker agree.
    //
    // ABOVE the no-services early-return: this is the site's language, not an
    // email detail, and a vault with no services installed has one too. It was
    // written below the guard until 2026-08-31, which was invisible while the
    // root language also lived in config.toml. Best-effort: a write failure
    // must not fail the build (readers fall back to a fresh derive).
    {
        // The ROOT audience (scope "") leads the list, carrying the language the
        // build just resolved. It used to be prepended by `email_site_langs`
        // from `[site] lang` in config.toml — a value the build wrote there and
        // read back. The build knows the language; writing it into the artifact
        // it already emits every build is what let the config write go away
        // (docs/archive/2026-08-31-site-lang-derived-state.md).
        let sections =
            crate::build::features::email::site_audience_list(Some(site_lang), &pages);
        let meta_path = Path::new(folder_path)
            .join(".moss")
            .join("build")
            .join("site-languages.json");
        if let Ok(json) = serde_json::to_string(&sections) {
            let _ = crate::build::io_utils::write_output(&meta_path, json.as_bytes());
        }
    }

    // Subscribe-family membership (`email` channel / inline `:::subscribe` / inline
    // `:::apply`) is single-sourced through `should_inject_subscribe_assets` so this
    // early-return guard and the downstream injector cannot drift — they did once,
    // when `inline_apply` was added to the injector but not to this guard, silently
    // dropping the apply form's CSS/JS on apply-only sites.
    if !(services_config.analytics.is_some()
        || services_config.comments.is_some()
        || services_config.email.is_some()
        || is_moss_hosted
        || comments_enabled
        || crate::build::features::should_inject_subscribe_assets(&pages, email_channel_installed)
        || has_footer_file)
    {
        return crate::build::enhance::ResolvedSlots::empty();
    }

    let article_map_path = Path::new(folder_path)
        .join(".moss")
        .join("build")
        .join("article-map.json");
    let native_article_map = load_article_map_for_features(&article_map_path);
    let domain = cached_domain_config.domain.clone();
    let media_lookup = crate::build::media::dimensions::MediaDimensionLookup::new(
        &project_structure.image_files,
        &project_structure.video_files,
        // Feature/email path: no dir_overrides in scope here. Base slugification
        // (empty map) matches feature-page cover/slug URLs; a per-folder slug
        // override does not reach this snapshot. See BUG 6 notes on the lookup.
        &std::collections::HashMap::new(),
        // Feature/email path: no `BuildServices` here, so no variants.
        None,
    );

    crate::build::features::generate_native_slots(
        &services_config,
        folder_path,
        email_channel_installed,
        &matters_domain,
        &native_article_map,
        &pages,
        &site_lang,
        domain.as_deref(),
        Some(cached_domain_config),
        start_server,
        Some(&media_lookup),
        site_comments,
    )
}

// ---------------------------------------------------------------------------
// Unified pipeline entry point (ADR-010, Phase 2)
// ---------------------------------------------------------------------------

/// Unified pipeline function -- the single source of truth for build.
///
/// All entry points (`build`, `build_with_options`, `build_sync`,
/// `build_folder`) build a [`PipelineConfig`] and delegate here.
/// Behavioral differences (progress delivery, plugin participation, server
/// startup) are expressed in the config, not as branching logic.
///
/// # Returns
/// * `Ok(String)` -- success message with build summary
/// * `Err(String)` -- error message describing what went wrong

/// Read the deploy plugin's `video_max_size_mb` config value.
///
/// Which plugin that is comes from [`site_config::current_deploy_plugin`], the
/// one resolver every "where does this site publish" question asks. It used to
/// ask `get_deploy_plugin_readonly` instead, which falls back to the sole
/// installed deploy plugin — so a moss-hosted vault with a deploy plugin
/// installed encoded every video under that plugin's cap while publishing
/// nowhere near it.
///
/// Returns `Some(mb)` if a deploy plugin is in effect and has a
/// `video_max_size_mb` setting, otherwise `None` (the caller uses the generic
/// default). Read once per build by `run_pipeline` and carried on
/// `BuildServices`; the video worker used to call this itself, which put plugin
/// discovery and the manifest schema inside the compiler (ADR-050 §1 forbids
/// both by name).
pub fn deploy_video_max_size_mb(folder_path: &str) -> Option<u32> {
    use crate::plugins::discovery::{get_plugin_config_with_defaults, load_installed_plugin};
    let plugin_name = crate::build::site_config::current_deploy_plugin(folder_path)?;
    // Overlay persisted values on the plugin's manifest-declared defaults, so an
    // unset `video_max_size_mb` still honors the plugin author's cap instead of
    // falling through to the generic VideoCompressionConfig default.
    //
    // Read the installed manifest directly rather than via `discover_plugins`:
    // discovery's first act is `ensure_bundled_plugins_installed`, which would
    // turn this read into a write over installed plugin code.
    let defaults = load_installed_plugin(folder_path, &plugin_name)
        .map(|p| p.manifest.config)
        .unwrap_or_default();
    let config = get_plugin_config_with_defaults(folder_path, &plugin_name, &defaults);
    config.get("video_max_size_mb")?.as_u64().map(|v| v as u32)
}

/// The video cap this build should use, or `None` to fall back to the generic
/// default.
///
/// A named function rather than an inline `if` so the `--no-plugins` guard is
/// testable. It is the whole of the guard: under `PluginMode::Skip` the deploy
/// plugin is never consulted, because consulting it means asking discovery,
/// and discovery writes.
pub fn video_cap_for_build(skip_plugins: bool, folder_path: &str) -> Option<u32> {
    if skip_plugins {
        return None;
    }
    deploy_video_max_size_mb(folder_path)
}

/// Read the value a build's cache keys are derived from.
///
/// Split out of `run_pipeline` so it can be tested directly. It is the only
/// thing standing between an injected `CacheKeyInputs` and a stub: every
/// in-tree pipeline test supplies a literal fingerprint, so a regression that
/// filled this with an empty string would leave the whole workspace green
/// while silently disabling "the moss binary changed, re-render everything".
///
/// Took a `folder_path` for the second key, a digest over installed enhance
/// plugins, until ADR-055 retired that capability.
pub fn sample_cache_keys() -> crate::build::ports::CacheKeyInputs {
    crate::build::ports::CacheKeyInputs {
        builder: crate::plugins::fingerprint::builder_fingerprint(),
    }
}

/// Should this build's in-memory content hashes become the watcher's refresh
/// baseline? Only when the build actually became what the preview shows.
///
/// The stash must describe what the screen shows. The pipeline switches the
/// preview to this build's staging output only when `publishable` is true
/// (`pipeline::run`, the two `if publishable { state.switch_to(..) }` arms),
/// and a cancelled build early-returns before either switch (with
/// `publishable: false, cancelled: true`). Stashing a non-switched build's
/// hashes poisons the baseline: the next publishable build's
/// `changed_output_files` diff (via `peek_content_hashes`) then compares
/// against output the user never saw, drops pages edited before the withheld
/// build, and the preview silently never refreshes.
pub(crate) fn should_stash_hashes(publishable: bool, cancelled: bool) -> bool {
    publishable && !cancelled
}

pub async fn run_pipeline(config: PipelineConfig) -> Result<String, String> {
    use crate::build::phase::PhaseTrace;
    let _pipeline_trace = PhaseTrace::start("run_pipeline");
    let folder_path = config.root.as_str().to_string();

    if folder_path.is_empty() {
        return Err("Empty folder path provided".to_string());
    }

    // Bring `.moss/` up to date before ANYTHING reads it. This used to sit
    // inside `pipeline::run`, which put it after the `[services]` read that
    // feeds the native-process sync — so a site still on the pre-v2 schema
    // spawned comment/review sync with default service settings on its first
    // build after upgrading, `v1_to_v2` being the step that reshapes
    // `[services]`. Hoisting it to the entry point is what ADR-059 rules
    // ("migration persistence becomes a precondition the entry point
    // satisfies"), and it closes that ordering hole on the way.
    config.host.store.run_vault_migrations(&config.root);

    // Begin a fresh link-meta URL session for this build. Render fills the
    // session via `record_urls_for_prewarm`; we flush it just before spawning
    // the native-process sync so sync sees this build's URL set.
    crate::build::page::link_meta::start_build_session(
        &Path::new(&folder_path).join(".moss"),
    );

    // Synchronous link-meta prewarm. The Tauri-app build path defers metadata
    // fetches to `spawn_native_process_sync`, but CLI builds and snapshot
    // tests exit moments after render — that background task gets killed
    // before it can populate the cache. So when `config.exits_after_build`
    // is set, we synchronously fetch (in parallel) any URLs recorded by the
    // previous build BEFORE render. Render then reads a warm cache.
    //
    // First-ever build for a folder is still cold (no `.urls.json` yet);
    // second+ build serves populated cards. The one-build lag for newly-
    // added URLs is intentional: `.urls.json` is written AFTER render in
    // the current build, so prewarm reads the *previous* build's URL set.
    // Don't try to "fix" this by scanning markdown pre-render — that would
    // duplicate the parser work that render already does.
    //
    // Wrapped in `block_in_place` so the blocking I/O doesn't tie up a
    // tokio worker: `std::thread::scope` joins synchronously, and on a
    // multi-thread runtime that worker is unavailable for other tasks for
    // the duration. Today CLI mode has nothing else on the runtime, but
    // future watch/serve paths might.
    if config.exits_after_build {
        let _trace = PhaseTrace::start("link_meta_prewarm");
        let moss_dir = Path::new(&folder_path).join(".moss");
        let (seen, fetched) = tokio::task::block_in_place(|| {
            // `block_in_place` lets the blocking std::thread::scope inside
            // `prewarm_link_meta_for_build` not hold a tokio worker hostage.
            crate::build::page::link_meta::prewarm_link_meta_for_build_with_progress(
                &moss_dir,
                |cold_count| {
                    // Stderr progress so a slow network doesn't look like a
                    // hang. Only fires when we're about to hit the network.
                    if cold_count > 0 && config.progress.is_terminal() {
                        eprintln!("Prewarming {cold_count} link previews…");
                    }
                },
            )
        });
        if seen > 0 {
            log::info!(
                target: "link-meta",
                "link-meta prewarm: {} URLs seen, {} fetched (rest from fresh cache)",
                seen,
                fetched
            );
        }
    }

    // Derive the .moss dir — always present, used as the preview-server dedup key.
    let moss_dir_path = config.source().join(".moss");
    let moss_dir_str = moss_dir_path.to_string_lossy().to_string();

    // Ensure .moss/ itself exists (the staging/generations subdirs are created
    // by the pipeline; we just need the parent for .gitignore).
    std::fs::create_dir_all(&moss_dir_path).map_err(|e| format!("Failed to create .moss directory: {}", e))?;
    crate::infra::moss_paths::exclude_dirs_from_cloud_sync(&moss_dir_path); // regenerable output: keep it out of iCloud, and out of the set moss waits for

    crate::infra::moss_paths::ensure_moss_gitignore(&moss_dir_path)?;

    // Retire what an older moss left behind: the synced ~5 MB copy of the
    // record of what is live in `.moss/data/`, which on one vault was the
    // single file a provider would not hand back (moss#1079), plus the legacy
    // output roots and a directory-shaped `current` that fails every
    // promotion. Per build, not per folder-open — neither location is watched,
    // so a file that finally syncs mid-session has no other trigger.
    crate::build::manifest::live_baseline::migrate(&crate::moss_paths::MossPaths::new(
        config.source(),
    ));
    crate::infra::moss_paths::retire_legacy_roots(&moss_dir_path);

    // Keep this project's coding-agent guidance current — see `SYNC_TRIGGERS`.
    let full_build = matches!(config.trigger, BuildTrigger::Full);
    crate::cli::agents::sync::sync_after_build(config.source(), full_build);

    // What to serve from the moment the server is ready, before this rebuild
    // produces annotated /staging: the last frozen generation (current → gen)
    // if it exists, else /staging — see initial_serve_dir().
    //
    // NOTE: this seed also runs on the start_server=false GUI rebuild paths
    // (deploy pre-build, plugin-install) against the LIVE managed Arc. There it
    // flips the live server to the frozen `current` at rebuild start — safe (and
    // safer than the old switch_to(/site)): the pipeline re-points to /staging at
    // completion (pipeline.rs) and `current` is a clean frozen generation, never
    // a half-written in-place rebuild target.
    let initial_serve_dir = crate::moss_paths::MossPaths::new(config.source()).initial_serve_dir();

    // Create SiteDirectoryState. CRITICAL (C3 / regression cce38aa0fc): in GUI mode
    // the preview SERVER reads the app-MANAGED SiteDirectoryState, so the build must
    // run against that SAME Arc — otherwise the zero-flicker dance's switch_to(stage)
    // at build completion (pipeline.rs) writes a disconnected Arc the server never
    // sees, leaving the preview stuck on the seed dir all session → click-to-source
    // dead. Share the managed Arc (GUI) or a fresh one (CLI). The CLI case is NOT "no
    // live server" — `moss build --serve` runs one here, so the fresh Arc is handed to
    // `start_preview_server` below. See docs/reference/editor-preview-sync.md 1 + 2.
    let shared_dir = config
        .host
        .site_dir
        .clone()
        .unwrap_or_else(|| std::sync::Arc::new(std::sync::RwLock::new(initial_serve_dir.clone())));
    let standalone_state = SiteDirectoryState { current_dir: shared_dir };
    // Rest on the frozen previous build initially: during a rebuild the dance
    // serves this clean, deploy-ready generation; it flips to /stage at
    // completion (annotated, for scroll sync).
    standalone_state.switch_to(initial_serve_dir.clone());

    // Start preview server FIRST — before scan, before anything. The shared Arc
    // already rests on initial_serve_dir (the frozen `current` generation when the
    // site was built before), so the server serves the last-built HTML immediately
    // while we rebuild in the background.
    let port = if config.start_server {
        // Hand the server THIS build's cell — in CLI mode that is the only way it
        // learns about `switch_to(staging)` below.
        let cell = Some(standalone_state.current_dir.clone());
        let launched = match config.host.launch_server.as_ref() {
            Some(launch) => launch(moss_dir_str.clone(), cell).await,
            None => Err("this host provides no preview server".to_string()),
        };
        match launched {
            Ok(p) => {
                log::info!("🚀 Preview server started on port {}", p);
                Some(p)
            }
            Err(e) => {
                log::warn!("⚠️ Preview server failed to start: {}", e);
                // Surface the failure to the frontend. Without this, the
                // pipeline still completes with `port: None` and the panel
                // silently warns then sits at the last BackgroundProgress
                // value forever. The typed event lets `preview-state.ts`
                // flip to an error state and unblock the user.
                config.host.events.report(&progress::PipelineEvent::PreviewServerFailed {
                    message: e.clone(),
                });
                None
            }
        }
    } else {
        config.server_port // what its dispatcher knew; `live_port` knows now
    };

    // Asked wherever used, never resolved once and carried — and asked through
    // the seam, so this file never calls into `build_shell`:
    let port_now = crate::build::ports::port_of_this_build(port, config.live_port.clone());
    // Tell the frontend the port early, so the previous build's pages are on
    // screen in milliseconds — but only while those pages are still
    // recognizably this site. Withholding this leaves the user on the
    // blueprint grid and its build progress, which beats watching a broken
    // version of their own work. See `build::prior_output`.
    if let Some(p) = port_now() {
        // Not from `cache_keys` — that is sampled later, after process hooks.
        // The builder half is a memoized constant for the life of the process
        // (`OnceLock` in `plugins::fingerprint`), so reading it here and there
        // cannot disagree; only the plugin half is timing-sensitive.
        let prior = prior_output::classify_prior_output(
            config.source(),
            &initial_serve_dir,
            &crate::plugins::fingerprint::builder_fingerprint(),
        );
        log::info!("Prior output for early preview: {:?}", prior);
        if prior.may_show_early() {
            progress::emit_event(
                &config.progress,
                &progress::PipelineEvent::Progress {
                    step: "server-ready".to_string(),
                    message: "Loading preview...".to_string(),
                    percentage: 5,
                    completed: false,
                    port: Some(p),
                    is_empty: Some(false),
                },
            );
        }
    }

    // --- SCAN ONCE at pipeline start ---
    // Build ProjectInfo + ProjectStructure once; shared by all consumers
    // (process hooks, enhance, build). Eliminates the redundant second scan
    // that build_inner() previously did.
    let _scan_trace = PhaseTrace::start("scan");
    // `mut`: content-producing process hooks (e.g. Matters import) may write
    // files into the source folder; we re-scan + rebind after the hook await
    // so the SAME build renders them (B13). See the post-hook re-scan below.
    let (mut project_info, mut project_structure) = {
        let ps = scan::scan::scan_folder_with_dedup_emit(
            &folder_path,
            config.host.metadata_dedup.as_deref(),
            config.host.scan_events.as_ref(),
            config.defers_image_placeholders(),
        )
        .map_err(|e| format!("Failed to scan folder: {}", e))?;
        let pi = ProjectInfo::from_structure(&ps, &config.root);
        (pi, ps)
    };

    drop(_scan_trace);


    // Derive plugin wait behavior from PluginMode
    let skip_plugins = matches!(config.plugins, PluginMode::Skip);
    let skip_process = matches!(config.plugins, PluginMode::SlotsOnly);

    // Honor PluginMode::NonBlocking's contract ("Fire process hooks, don't wait
    // — preview mode") on the LIVE-PREVIEW path (NonBlocking + a running server).
    // There the process hook (e.g. the whole Matters sync, minutes long) runs
    // DETACHED: the first build below paints immediately on pre-hook content and
    // the file watcher fills articles in progressively as the download lands —
    // instead of the preview sitting blank until the entire sync finishes and
    // then dumping every article at once.
    //
    // Everything else still AWAITS inline:
    //  - Blocking (moss build / publish): the SAME build must render imported
    //    content and the enhance hook must read complete process output
    //    (e.g. comment.json). The detached path preserves this via the file
    //    watcher — its final debounce batch re-runs the pipeline (enhance
    //    included) on the complete on-disk output (see the detached branch below).
    //  - A NonBlocking build with no server (rare, non-preview) stays one-shot.
    let detach_process =
        matches!(config.plugins, PluginMode::NonBlocking) && config.start_server;
    let wait_process = !detach_process;

    // Execute process hooks (unless plugins are skipped or SlotsOnly).
    // SlotsOnly skips process hooks — they fetch external data (e.g., Matters sync)
    // and should only run on the initial build, not on every file-edit rebuild.
    // Whether plugins run is a Config decision, never a routing fork (ADR-010):
    // a missing `AppHandle` is not a licence to build a plugin-less site behind
    // the user's back. Since #1019 the process path needs no app.
    let process_hooks_handle: Option<crate::build::ports::spawner::Joining> =
        if !skip_plugins && !skip_process {
            Some(spawn_process_hooks(
                &folder_path,
                config.host.plugins.clone(),
                &*config.host.spawner,
                project_info.clone(),
            ))
        } else { None };

    if let Some(handle) = process_hooks_handle {
        if wait_process {
            let _trace = PhaseTrace::start("process_hooks_await");
            // Blocking build: block until the hook finishes so the SAME build
            // re-scans + renders the imported content and the enhance hook reads
            // complete process output.
            let await_start = std::time::Instant::now();
            progress::announce_process_hook_wait(&config.progress, port_now());
            if let Err(e) = handle.await {
                log::warn!("process hook task failed: {}", e);
            }
            let waited_ms = await_start.elapsed().as_millis();
            progress::announce_process_hook_done(&config.progress, waited_ms, port_now());
        } else {
            // Live preview (NonBlocking + running server): honor the contract —
            // do NOT await. `spawn_process_hooks` already runs the hook in its
            // own detached task, and dropping a `Spawner::spawn` handle detaches
            // rather than aborts (the underlying runtime aborts only via an
            // explicit `.abort()`), so dropping it here lets the run complete. The first build below paints the
            // shell immediately; the file watcher then renders downloaded content
            // as it lands (a full re-scan per debounce batch), and its FINAL
            // batch re-runs the enhance hook on the COMPLETE process output —
            // the watcher watches the source folder recursively (plus the
            // allowlisted `.moss/*` files), which is where process hooks write.
            // This preserves the invariant that made Blocking await, without
            // blocking the preview on the entire sync.
            log::info!("[diag] preview-wait: process hooks detached — preview paints now, watcher fills as sync lands");
            drop(handle);
        }
    }

    // Sample the cache keys here, and only here.
    //
    // "Here" is load-bearing in two directions. It is *after* the process
    // hooks, because a hook writes files into `.moss/plugins/<name>/` and the
    // plugin digest hashes every file in that directory — sampling earlier
    // records a digest of the folder as it was before the hook touched it, so
    // the change lands one build late. And it is *once*, because the GUI and
    // the CLI both funnel through `run_pipeline`: two sampling sites would let
    // the two binaries derive different keys for the same folder, a divergence
    // `build_parity_test` cannot see because it compares emitted bytes rather
    // than what was reused. See `ports::CacheKeyInputs`.
    let cache_keys = sample_cache_keys();

    // --- RE-SCAN after content-producing process hooks ---
    // Process hooks (e.g. the Matters import) may have written .md / asset files
    // into the source folder. Re-scan + rebind so the SAME build renders them,
    // instead of rendering the pre-import (empty/stale) snapshot from the scan
    // at pipeline start (B13). This INTENTIONALLY reverts the "scan once"
    // optimization above for the import case only — do not "re-optimize" it
    // away. The re-scan is SILENT (no ScanEventEmitter): the live file watcher
    // drives the editor's file tree; this scan only feeds render + language
    // resolution. Gated to the import case where we AWAITED the hook (Blocking):
    // the detached preview path renders pre-hook content and lets the watcher
    // re-scan as downloaded files land, so a synchronous re-scan here would only
    // re-render the same empty snapshot.
    // The app supplies only the metadata dedup cache, which is an optimization —
    // gating the whole re-scan on it made a headless build render the pre-import
    // snapshot and left every imported page one generation behind (#1019).
    if wait_process && !skip_plugins && !skip_process {
        let _rescan_trace = PhaseTrace::start("scan_after_process_hooks");
        match scan::scan::scan_folder_with_dedup_emit(
            &folder_path,
            config.host.metadata_dedup.as_deref(),
            None,
            config.defers_image_placeholders(),
        ) {
            Ok(ps) => {
                project_info = ProjectInfo::from_structure(&ps, &config.root);
                project_structure = ps;
            }
            Err(e) => {
                log::warn!(
                    target: "build",
                    "Re-scan after process hooks failed: {}; rendering pre-import snapshot",
                    e
                );
            }
        }
    }

    // Cache domain config once — reused by native process, feature slots, and build.
    // Avoids re-reading and re-parsing .moss/state.toml on every access.
    let cached_domain_config = crate::build::site_config::get_domain_config(&folder_path)
        .unwrap_or_default();

    // --- NATIVE PROCESS PHASE ---
    //
    // Comment + review fetches are network I/O and MUST NOT sit on the build's
    // critical path. We spawn them as a background task; their output (written
    // to .moss/data/social/*.json) is read by the NEXT build's enhance phase.
    //
    // Architectural invariant: `run_pipeline` does no network fetches itself
    // **except for the link-meta prewarm above**, which is gated on
    // `config.exits_after_build` (CLI / snapshot tests) where there's no
    // long-lived runtime to host `spawn_native_process_sync`. GUI builds
    // never fetch inline — the background task handles them async.
    // If you find yourself wanting to add a NEW network fetch — don't.
    // Spawn it from `features::sync` so build still returns immediately
    // when the upstream is slow or unreachable. See moss issue #570.
    //
    // Skipped during SlotsOnly (watch rebuilds) — those run frequently and
    // would otherwise pile up sync tasks. The `features::sync` module also has
    // a per-folder singleflight so even if SlotsOnly's behavior changes, rapid
    // rebuilds don't compound.
    if !skip_process {
        let _trace = PhaseTrace::start("native_process_spawn");
        let services_config = crate::build::site_config::get_services_config(&folder_path)
            .unwrap_or_default();
        crate::build::features::sync::spawn_native_process_sync(
            folder_path.clone(),
            services_config,
            cached_domain_config.clone(),
            // The typed bus, not `config.progress`: this completion event is a
            // frontend signal, and in CLI mode the progress sink prints to
            // stderr — routing it there would add a line `moss build` has
            // never printed.
            config.host.events.clone(),
            &*config.host.spawner,
        );
    }

    // Build the site.
    // Note: Don't error on empty folder - file watcher will detect when content arrives
    // (e.g., when process plugins download files). Frontend shows placeholder based
    // on is_empty in the progress update.

    // The host constructed BuildServices at the entry point
    // (`BuildServices::from_app` / `headless_for`); this build takes its
    // own handle on the shared Arcs.
    let mut services = config.host.services.clone();
    // Read the deploy plugin's video cap HERE, once: the video worker is a
    // leaf of the compiler and may not ask the plugin layer anything
    // (ADR-050 §1). One reader means the GUI and the CLI cannot encode the same
    // video two different ways.
    // Guarded on `skip_plugins`: the read reaches `load_installed_plugin`, and
    // a `--no-plugins` build asks the plugin layer nothing at all.
    services.deploy_video_max_size_mb = video_cap_for_build(skip_plugins, &folder_path);
    // moss#867: each pipeline attempt below moves its own clone of `services`
    // into `spawn_blocking`, so both `advertise_sealed` call sites (seal task,
    // exits-after-build tail) take the registry handle from here instead.
    let assets_for_advertise = services.assets.clone();

    let port_owned = port_now.clone(); // moved into whichever run-arm executes

    // Emit build-start progress for CLI (the stdout reporter prints to stderr)
    progress::emit_event(
        &config.progress,
        &progress::PipelineEvent::Progress {
            step: "build".to_string(),
            message: "Building site...".to_string(),
            percentage: 10,
            completed: false,
            port: port_now(),
            is_empty: None,
        },
    );

    // --- BUILD: Generate marked HTML, then resolve slots before artifact finalization ---
    // The internal pipeline renders HTML with <!-- slot:NAME --> markers, writes
    // the fresh article-map, calls this resolver to collect native/plugin slots
    // from the current build's documents, and injects slots before hash
    // comparison, ship_phase, and manifest seal.
    let _build_trace = PhaseTrace::start("build");
    let pipeline_build_start = std::time::Instant::now();
    // `pipeline::run()` returns `PipelineRunOutput`; `build_documents` carries the parsed page slice.
    // The parsed page slice is threaded all the way back from
    // `generate_blocking_content` so pre-finalization native slot collection
    // (`generate_native_slots`) can consult typed feature flags
    // (`features.inline_subscribe`) and reach slot-only files (`footer.md`)
    // through the same data path the rest of the build uses. Replaces the
    // pre-PR7b filesystem-scan stand-ins `project_has_inline_subscribe`
    // and `render_footer_pages_from_disk` (moss#599).
    //
    // Slots come from moss's own features only — the comment section, the
    // subscribe form, the analytics beacon. A plugin could contribute one
    // through the `enhance` capability until ADR-055 retired it, having gone
    // three months with no implementation in any plugin, bundled or WIP.
    let make_slot_resolver = |folder_path_for_slots: String,
                              cached_domain_config_for_slots: crate::config::deployment::DomainDeploymentConfig,
                              project_structure_for_slots: ProjectStructure|
     -> pipeline::SlotResolver {
        let start_server_for_slots = config.start_server;
        Box::new(move |pages, site_lang, site_comments| {
            let _slots_trace = PhaseTrace::start("slot_resolution");
            Ok(collect_native_slots_for_documents(
                &folder_path_for_slots,
                cached_domain_config_for_slots,
                &project_structure_for_slots,
                pages.to_vec(),
                start_server_for_slots,
                site_lang,
                site_comments,
            ))
        })
    };

    let pipeline::PipelineRunOutput { is_empty: _is_empty, bg_handle: _bg_handle, build_documents: _build_documents, content_hashes, missing_media, cancelled, home_ready: _home_ready, publishable } = {
        // moss's own generator, always. A plugin could replace it wholesale
        // through the `generate` capability until ADR-055 retired it: three
        // months, no implementation, and the branch had already decayed into
        // a GUI-only path whose headless arm re-ran this same code with a
        // warning.
        // One attempt: its own clones, its own `SlotResolver`.
        let attempt = || {
            let root_owned = config.root.clone();
            // Share the Arc (C3) — NOT SiteDirectoryState::new (a fresh Arc) — so the
            // pipeline's zero-flicker switch_to(current_ptr)/switch_to(stage) drives the
            // SAME state the preview server reads. ::new severed it, so the GUI markdown
            // cold start served the empty /site until the initial-build-complete event.
            let state_owned = SiteDirectoryState { current_dir: standalone_state.current_dir.clone() };
            let reporter_owned = tier1_reporter(&config.progress);
            let ps = project_structure.clone();
            let site_url_override = config.site_url_override.clone();
            let incremental = crate::build::render::IncrementalGates {
                render_skip: config.allows_incremental_skip(),
                parse_cache: config.allows_parse_cache_reuse(),
            };
            let search_freshness = search_lane::Freshness::of(config.exits_after_build);
            let slot_resolver = make_slot_resolver(folder_path.clone(), cached_domain_config.clone(), ps.clone());
            let cache_keys_owned = cache_keys.clone();
            let (services, port_owned) = (services.clone(), port_owned.clone());
            async move {
                tokio::task::spawn_blocking(move || {
                    pipeline::run(&root_owned, Some(&state_owned), reporter_owned.as_deref(), &port_owned, Some(&services), Some(slot_resolver), &ps, site_url_override, incremental, search_freshness, &cache_keys_owned)
                }).await.map_err(|e| BuildStopped::from(format!("Build task panicked: {}", e)))?
            }
        };
        retry_once_after_discard(attempt).await?
    };

    // Stash the build's IN-MEMORY content hashes (keyed by folder) so the file
    // watcher's `do_rebuild_and_notify` decides the live-preview refresh from
    // race-free in-memory data instead of re-reading the asynchronously-
    // persisted hashes.json (which a detached seal task writes only after the
    // background media phase — the stale-hash-race that suppressed refreshes).
    // Uses AppState::stash_content_hashes (testable without AppHandle).
    //
    // INVARIANT (gate-soundness keystone): a FAILED build must never reach
    // this block. The `?` on the pipeline result above returns first, so the
    // previous SHOWN build's stash survives as the baseline for both the
    // watcher's refresh diff and the admission-time content-hash gate. Were a
    // failed build's hashes stashed, the user's edit could compare "unchanged"
    // against output that never rendered and the retry be gated out forever.
    // Pinned by `a_failed_build_leaves_the_previous_stash_as_the_gate_baseline`
    // (watch_tests.rs); do not move this block ahead of the error return.
    //
    // Guarded by `should_stash_hashes`: the stash must describe what the
    // screen shows. A withheld (`publishable == false`) or cancelled build
    // never switched the preview, so stashing its hashes would make the
    // watcher diff the next build against output the user never saw — and a
    // page edited before the withheld build would then read as "unchanged"
    // and never refresh.
    if should_stash_hashes(publishable, cancelled) {
        crate::system::build_records::records().record_content_hashes(&folder_path, content_hashes);
    }

    // And what this build could not find. Stashed unconditionally — an empty
    // list is the answer that UNBLOCKS a publish, so skipping the write on a
    // clean build would leave an earlier failure standing forever.
    crate::system::build_records::records().record_missing_media(&folder_path, missing_media);

    // This build's promotion epoch (moss#968 §5d). The rebuild worker mints
    // it at ADMISSION and passes it in (see `PipelineConfig::admission_epoch`
    // for why that survives a wedged predecessor); every other caller mints
    // here, not in the seal — for them this is the last point still ordered
    // against the *next* build of this folder, because their builds are
    // strictly serialized. A content-hash `generation_id` can never say which
    // build is newer.
    let promotion_epoch = config
        .admission_epoch
        .unwrap_or_else(crate::build::ship::next_promotion_epoch);

    // Spawn the seal+persist side task that owns the BackgroundHandle.
    //
    // This fixes B1 from the build lifecycle review: previously the handle was stored
    // in AppState and only awaited at deploy time. If the user never deployed (just
    // rebuilt), nobody awaited the handle, nobody wrote hashes.json with deferred
    // entries, and the next build's stale cleanup deleted generated media files.
    //
    // New flow: a side task increments the FolderSession's UiBound counter (so
    // wait_for_in_flight_work gates on it), awaits await_completion(), writes
    // the SealedManifest to hashes.json, stores it in AppState.current_sealed_manifest,
    // then decrements the counter.
    // Deploy hard-errors on None (no disk fallback since Stage 0); the
    // The seal+persist task must populate the slot before Publish is
    // attempted.

    if let Some(handle) = _bg_handle {
        if config.host.shell_attached || !config.exits_after_build {
            // Long-lived runtime — the desktop app, or a headless
            // `moss build --serve --watch` rebuild. Detached seal task,
            // announced through whichever ports fit the shell: the app's
            // webviews + AppState, or the SSE carrier. Until 2026-08-24 this
            // was three arms — app-with-AppState, an app-sans-AppState
            // "legacy" arm that awaited the workers and threw the seal away,
            // and NO arm at all for headless watch, whose rebuilds updated
            // staging forever without ever promoting a generation (#1097).
            //
            // Compute paths for this build's source folder. We need:
            //   - hashes.json (manifest persistence target)
            //   - staging (stale-cleanup target — moved here from
            //     copy_deferred_assets to fix #621: cleanup must run AFTER the
            //     coordinator has merged in-flight worker EmitMessages, otherwise
            //     a freshly-written `.webp` can be deleted as "stale" before its
            //     registration is observed).
            let mp = crate::moss_paths::MossPaths::new(std::path::Path::new(&folder_path));
            let hashes_path = mp.hashes();
            let stage_dir = mp.staging_dir();

            // Acquire FolderSession clone so the spawned task can decrement
            // its UiBound counter once seal+persist completes.
            let session_opt = crate::system::folder_session::registry().get(&folder_path);

            // Increment BEFORE spawning so wait_for_in_flight_work gates on this
            // task starting immediately (no window where has_ui_bound() is false).
            if let Some(ref s) = session_opt {
                s.begin_ui_bound();
            }

            // The host's ports ride into the 'static task as owned Arcs —
            // which is the whole point of resolving them at the entry point:
            // nothing here holds a tauri::State across an await boundary.
            let seal_ports = SealPorts {
                events: config.host.events.clone(),
                announcer: config.host.announcer.clone(),
                server_diff: config.host.server_diff.clone(),
            };
            let store = config.host.store.clone();
            // Clone folder_path so the spawn can construct MossPaths for materialize.
            // (MossPaths is not Clone; we pass the root string and construct inside.)
            let folder_path_for_mat = folder_path.clone();
            // moss#867: the registry the render/blocking phase registered
            // variants into, cloned so the 'static spawned task can pass
            // it to advertise_sealed's degrade step.
            let assets_for_seal = assets_for_advertise.clone();
            let seal_freshness =
                crate::build::feeds::search_lane::Freshness::of(config.exits_after_build);

            // Dropping the returned Joining DETACHES the task (the runtime
            // aborts only via an explicit `.abort()`), which is exactly what
            // a detached seal wants.
            let _detached = config.host.spawner.spawn(Box::pin(async move {
                seal_ports.events.report(&crate::build::progress::PipelineEvent::BackgroundProgress {
                    task: "sealing".to_string(),
                    current: 0,
                    total: 1,
                    message: "Preparing...".to_string(),
                    completed: false,
                    advisories: vec![],
                });
                match handle.await_completion().await {
                    Ok(sealed) => {
                        log::info!(
                            "seal+persist: generation {} sealed ({} files)",
                            sealed.generation_id(),
                            sealed.files().len()
                        );
                        let mp_for_mat = crate::moss_paths::MossPaths::new(
                            std::path::Path::new(&folder_path_for_mat),
                        );
                        advertise_sealed(
                            &seal_ports,
                            &mp_for_mat,
                            &hashes_path,
                            &stage_dir,
                            sealed,
                            assets_for_seal,
                            |g| store.is_pinned(g),
                            session_opt.as_ref(),
                            promotion_epoch,
                            publishable,
                            seal_freshness,
                        )
                        .await;
                    }
                    Err(e) => {
                        log::error!("seal+persist: background work failed: {}", e);
                        seal_ports.events.report(
                            &crate::build::progress::PipelineEvent::BackgroundProgress {
                                task: "sealing".to_string(),
                                current: 1,
                                total: 1,
                                message: "Seal failed".to_string(),
                                completed: true,
                                advisories: vec![],
                            },
                        );
                    }
                }
                // Decrement so deploy's wait_for_in_flight_work can proceed.
                // Runs AFTER cleanup so wait_for_in_flight_work gates on the
                // cleanup step too.
                if let Some(s) = session_opt {
                    s.end_ui_bound();
                }
            }));
        } else {
            // No app context AND the caller (CLI / build_sync / snapshot harness)
            // will drop the tokio runtime as soon as this returns. A detached
            // seal would be cancelled mid-write when the runtime shuts down,
            // leaving deferred assets missing from the current generation — so
            // this arm awaits the SAME tail inline. Until 2026-08-24 it was a
            // hand-copied subset of that tail, which is how it silently lacked
            // the moss#867 degrade pass (#1097): a background encode failure
            // shipped a live 404 inside <picture> from `moss build` while the
            // app degraded it. The host's announcer, as in the detached arm:
            // hardcoding `LogAnnouncer` here dropped the manifest a C4f hosted
            // deploy publishes. `tier2_reporter(None)` because ADR-066's
            // headless carrier is the one publisher here. The session
            // is the one the CLI entry point registered — BOTH of them, since
            // 2026-08-24; `run_cli_build` (the path an ordinary `moss build`
            // takes) registered none until then, and an earlier version of this
            // comment named only `start_cli_build`. So the tail takes the same
            // stage-write guard the build's own span took — uncontended here
            // (this arm exists because nothing else will run), and that is the
            // price of one tail rather than a second code path.
            match handle.await_completion().await {
                Ok(sealed) => {
                    log::info!(
                        "seal+persist (sync): generation {} sealed ({} files)",
                        sealed.generation_id(),
                        sealed.files().len()
                    );
                    let mp = crate::moss_paths::MossPaths::new(std::path::Path::new(&folder_path));
                    let session = crate::system::folder_session::registry().get(&folder_path);
                    advertise_sealed(
                        &SealPorts {
                            events: config.host.events.clone(),
                            announcer: config.host.announcer.clone(),
                            server_diff: config.host.server_diff.clone(),
                        },
                        &mp,
                        &mp.hashes(),
                        &mp.staging_dir(),
                        sealed,
                        assets_for_advertise.clone(),
                        |_| false,
                        session.as_ref(),
                        promotion_epoch,
                        publishable,
                        crate::build::feeds::search_lane::Freshness::Now,
                    )
                    .await;
                }
                Err(e) => {
                    log::error!("seal+persist (sync): background work failed: {}", e);
                }
            }
        }
    }

    log::debug!(target: "timing", "[pipeline] build returned: {:?}", pipeline_build_start.elapsed());
    drop(_build_trace);

    // Flush this build's link-meta URL set to disk so the next build's
    // background sync can prewarm the cache. Atomic write, replace-not-merge —
    // removed URLs vanish from the list (no unbounded growth).
    let flushed = crate::build::page::link_meta::flush_build_session(
        &Path::new(&folder_path).join(".moss"),
    );
    if flushed > 0 {
        log::debug!(target: "link-meta", "flushed {} URL(s) for next-build prewarm", flushed);
    }

    // Emit build-complete progress for CLI. Skipped for a `cancelled` result
    // (docs/archive/2026-07-31-cloud-download-waiting-mode.md Stage 2's
    // folder-switch-mid-wait early return): that placeholder result's
    // `is_empty: true` doesn't describe a real build, and this event's
    // `port`/GUI progress channel is shared app-wide (not folder-scoped on
    // the frontend) — emitting it here could flip the CURRENT (different)
    // folder's preview to a spurious "ready, empty site" state while its own
    // build is still in flight.
    if !cancelled {
        progress::emit_event(
            &config.progress,
            &progress::PipelineEvent::Progress {
                step: "build".to_string(),
                message: "Build complete".to_string(),
                percentage: 100,
                completed: true,
                port: port_now(), // again about NOW: this runs later still
                is_empty: Some(_is_empty),
            },
        );
    }

    let folder_name = config.root.name_opt().unwrap_or("unnamed-site");

    let current_output = crate::moss_paths::MossPaths::new(config.source()).current_ptr();
    let base_message = format!("📁 '{}': Site generated at {}", folder_name, current_output.display());

    if let Some(p) = port_now() {
        let preview_url = format!("http://localhost:{}", p);
        Ok(format!(
            "{}\n🌐 Preview server ready! Access at {}",
            base_message, preview_url
        ))
    } else {
        Ok(base_message)
    }
}



/// Load article-map.json and convert to the ArticleInfo map needed by native features.
pub fn load_article_map_for_features(
    article_map_path: &Path,
) -> std::collections::HashMap<String, crate::build::features::ArticleInfo> {
    use crate::build::features::ArticleInfo;

    if !article_map_path.exists() {
        return std::collections::HashMap::new();
    }

    let content = match std::fs::read_to_string(article_map_path) {
        Ok(c) => c,
        Err(_) => return std::collections::HashMap::new(),
    };

    let value: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return std::collections::HashMap::new(),
    };

    let articles = match value.get("articles").and_then(|a| a.as_object()) {
        Some(a) => a,
        None => return std::collections::HashMap::new(),
    };

    let mut result = std::collections::HashMap::new();
    for (url_path, article) in articles {
        let uid = article
            .get("uid")
            .and_then(|u| u.as_str())
            .unwrap_or("")
            .to_string();

        if uid.is_empty() {
            continue;
        }

        let info = ArticleInfo {
            url_path: url_path.clone(),
            title: article
                .get("title")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string(),
            uid: uid.clone(),
            review_of: article
                .get("frontmatter")
                .and_then(|fm| fm.get("review_of"))
                .and_then(|r| r.as_str())
                .map(|s| s.to_string()),
            rating: article
                .get("frontmatter")
                .and_then(|fm| fm.get("rating"))
                .and_then(|r| r.as_u64())
                .map(|r| r as u8),
            cover: article
                .get("frontmatter")
                .and_then(|fm| fm.get("cover"))
                .and_then(|c| c.as_str())
                .map(|s| s.to_string()),
            comments: article
                .get("frontmatter")
                .and_then(|fm| fm.get("comments"))
                .and_then(|c| c.as_bool()),
            lang: article
                .get("frontmatter")
                .and_then(|fm| fm.get("lang"))
                .and_then(|l| l.as_str())
                .map(|s| s.to_string()),
            syndicated: article
                .get("frontmatter")
                .and_then(|fm| fm.get("syndicated"))
                .and_then(|s| s.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
        };

        result.insert(uid, info);
    }

    result
}

// extract_review_articles_from_map and extract_comment_articles_from_map have
// been removed — the native process phase now parses article-map.json once via
// load_article_map_for_features() and derives both review and comment lists by
// filtering the resulting HashMap.



/// Shared post-seal tail: persist → materialize → GC → stale-clean → advertise.
///
/// Called from two sites, both of them the normal build path: the detached
/// seal task's `Ok(sealed)` arm, and the synchronous tail an
/// `exits_after_build` run takes instead of detaching.
///
/// `stage_dir` is the directory to stale-clean after materialize —
/// `.moss/build/staging`.
///
/// `is_pinned` is called by GC to skip generations currently in-flight.
/// Pass `|g| s.is_generation_pinned(g)` when AppState is available, or
/// `|_| false` as a safe fallback.
/// Image/video output-variant paths that are NEW-or-CHANGED in `sealed`
/// versus `previous`. Restricted to the image/video output buckets — the ones
/// the preview iframe can srcset-swap; notebook outputs are excluded (no swap
/// path exists yet). Every variant is mirrored into `files` with a content hash
/// (see `manifest.rs` register: ImageOutputs/VideoOutputs land in `files` too),
/// so a byte-change (source edited, same filename → re-encode) is detected via
/// the hash, not merely add/remove. Empty result → nothing the preview needs to
/// swap (pure text edit, or an identical re-encode). Pure (no AppHandle/FS) so
/// the post-seal diff in `advertise_sealed` is unit-testable in isolation.
pub fn diff_settled_assets(
    previous: &crate::types::content::SiteHashes,
    sealed: &crate::types::content::SiteHashes,
) -> Vec<crate::build::progress::SettledAsset> {
    use crate::build::progress::SettledAsset;
    // A variant is "settled" when it now carries a content hash in `files` that
    // the previous manifest didn't (new or re-encoded). asset_type is taken
    // from the bucket the path lives in — authoritative, not an extension guess:
    // image_outputs → "image"; video_outputs → "video", refined to "thumbnail"
    // for the poster frame (`.thumb.jpg`). These strings match the canonical
    // asset_type the real-time AssetReady path emits and iframe-bridge swaps on.
    let is_settled = |path: &str| {
        let new_entry = sealed.files.get(path);
        new_entry.is_some() && new_entry != previous.files.get(path)
    };
    let mut changed: Vec<SettledAsset> = Vec::new();
    for path in &sealed.image_outputs {
        if is_settled(path) {
            changed.push(SettledAsset { path: path.clone(), asset_type: "image".to_string() });
        }
    }
    for path in &sealed.video_outputs {
        if is_settled(path) {
            let asset_type = if path.ends_with(".thumb.jpg") { "thumbnail" } else { "video" };
            changed.push(SettledAsset { path: path.clone(), asset_type: asset_type.to_string() });
        }
    }
    changed.sort_by(|a, b| a.path.cmp(&b.path));
    changed.dedup_by(|a, b| a.path == b.path);
    changed
}

/// The subset of [`HostPorts`](crate::build::HostPorts) a seal tail needs,
/// cloned ONCE where the tail is launched instead of field-by-field at every
/// call site — the next port a tail needs joins here instead of growing
/// `advertise_sealed`'s signature again.
pub(crate) struct SealPorts {
    pub events: crate::build::ProgressSink,
    pub announcer: std::sync::Arc<dyn crate::build::ports::announcer::SealAnnouncer>,
    pub server_diff: Option<crate::build::ports::host::ServerDiff>,
}

/// `publishable` is `pipeline::should_publish`'s verdict for the build that
/// produced `sealed`. `false` makes the promotion below a no-op
/// (`ship::Promotion::Withheld`) and, through `tail_owns_shared_state`, keeps
/// this tail off `hashes.json` and the staging sweep as well.
async fn advertise_sealed(
    ports: &SealPorts,
    mp: &crate::moss_paths::MossPaths,
    hashes_path: &std::path::Path,
    stage_dir: &std::path::Path,
    mut sealed: crate::build::manifest::SealedManifest,
    assets: Option<std::sync::Arc<crate::types::assets::AssetRegistry>>,
    is_pinned: impl Fn(&str) -> bool,
    session: Option<&std::sync::Arc<crate::system::folder_session::FolderSession>>,
    promotion_epoch: u64,
    publishable: bool,
    freshness: crate::build::feeds::search_lane::Freshness,
) {
    let reporter = ports.events.as_ref();
    let announcer = ports.announcer.as_ref();
    // 0. Read the PREVIOUS on-disk manifest BEFORE step 2 overwrites it, so the
    //    post-seal asset diff (step 7) compares the freshly-sealed view against
    //    the prior build. Missing/corrupt → default (empty), so a first build
    //    treats every materialized variant as new. write_to_disk below clobbers
    //    this file, hence the read must happen here.
    let previous_hashes: crate::types::content::SiteHashes = std::fs::read_to_string(hashes_path)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default();

    // Seal-persist-race-404 fix: acquire the SAME per-folder stage-write
    // guard that `build_inner` (pipeline.rs) holds across its own
    // stage-writing span. This task runs detached — well after the
    // watcher's `is_rebuilding` lock has already released — so without this
    // guard, the moss#867 degrade pass and steps 2 and 4 below (which all
    // read/write/delete directly against the live, possibly-being-served
    // `stage_dir`) can interleave with a NEXT rebuild's writes into that
    // same directory, corrupting the generation that gets promoted to
    // `current`. See `FolderSession::stage_write_lock` doc comment for the
    // full rationale. Held from here through step 4 (the moss#867 rewrite
    // plus the fast directory walk/copy/cleanup), not across anything
    // before this point — in particular NOT across the `await_completion`
    // media-encode wait that already happened before `advertise_sealed` was
    // called.
    let _stage_write_guard = match session {
        Some(s) => Some(s.lock_stage_write().await),
        None => None,
    };

    // Remove every variant that must not ship, then repair the HTML that
    // referenced it. Runs before `materialize_and_promote` and `write_to_disk`
    // below, which would otherwise ship bytes it just deleted from stage_dir;
    // it writes into `stage_dir`, under `_stage_write_guard` above. The
    // registry is read here because it is the only thing in this tail that
    // needs one — see `degrade::repair_staged_html` for the four sources and
    // why their order is what it is.
    crate::build::degrade::repair_staged_html(
        mp,
        stage_dir,
        &mut sealed,
        assets.as_ref().map(|r| r.failed_keys()).unwrap_or_default(),
    );

    // Last moment the manifest and the stage agree on what shipped — the one
    // place a whole-site link check can run (moss#1187), and advisory only.
    crate::build::manifest::link_audit::audit_and_report(stage_dir, &sealed);

    // 1. Copy stage_dir → generations/<gen-id>/ and swap `current`. mat_ok gates
    //    advertisement to deploy: a failed materialize must NOT publish a
    //    manifest whose generation_dir is partial/absent.
    //    `Superseded` (moss#968 §5d) is the third outcome: a NEWER build already
    //    promoted, so this tail's swap was refused. Not an error — but not
    //    `mat_ok` either, since advertising this older manifest to deploy rolls
    //    the published site back exactly as the symlink swap would have.
    use crate::build::ship::Promotion;
    let promotion = crate::build::ship::materialize_and_promote(
        &sealed,
        mp,
        stage_dir,
        None,
        promotion_epoch,
        publishable,
    );
    match &promotion {
        Ok(Promotion::Promoted) => {}
        Ok(Promotion::Superseded) => log::info!(
            "advertise_sealed: generation {} was superseded before promotion",
            sealed.generation_id()
        ),
        Ok(Promotion::Withheld) => log::info!(
            "advertise_sealed: generation {} was withheld — built without sources that are \
             still downloading",
            sealed.generation_id()
        ),
        Err(_) => log::error!("advertise_sealed: materialize failed — current_ptr stays put"),
    }
    let mat_ok = matches!(promotion, Ok(Promotion::Promoted));
    let owns_shared = crate::build::ship::tail_owns_shared_state(&promotion);

    // Say it out loud, and only for a real swap: this instant — not
    // `BuildComplete`, which fired back when `await_completion` returned — is
    // when the user's change became what the preview loads. `Superseded`,
    // `Withheld` and `Err` all leave `current` where it was, so none of them
    // may claim it. Announced here rather than after step 2 so nothing between
    // the swap and the announcement can fail and swallow it.
    if mat_ok {
        announcer.promoted(sealed.generation_id());
    }

    // 2. Persist so the next build's load_previous_hashes reads a complete
    //    manifest — AFTER the promotion, and never from a superseded tail:
    //    `hashes.json` and `current` are read as a pair (see
    //    `ship::tail_owns_shared_state`).
    if owns_shared {
        if let Err(e) = sealed.write_to_disk(hashes_path) {
            log::warn!("advertise_sealed: failed to write hashes.json: {}", e);
        }
    }

    // 3. Retain generations + sweep the cache. Non-fatal, and deliberately
    //    inside the `_stage_write_guard` span below: `cache::gc` requires a
    //    quiescent cache, and this guard is what serializes us against the
    //    next rebuild's stage-writing span. Blocking work, so it goes to the
    //    blocking pool rather than stalling this executor thread — we still
    //    await it, because releasing the guard early is the thing the
    //    quiescence requirement forbids.
    if mat_ok {
        // `project_root()`, not `root()`: `MossPaths::new` appends `.moss`, so
        // handing it the `.moss` dir builds paths under `.moss/.moss/` and the
        // collector sweeps an empty tree.
        let project_root = mp.project_root().to_path_buf();
        let gen_id = sealed.generation_id().to_string();
        // Resolve the pin set HERE, on this thread: `is_pinned` borrows
        // `AppState` and cannot cross into the blocking pool. A generation
        // pinned now cannot become unpinned in a way that matters — unpinning
        // only ever makes a directory *more* collectable, and missing that
        // costs one extra retained generation, not a deleted live upload.
        // Also fold in the search lane's in-flight set (ADR-045): a
        // generation the lane is still indexing must survive GC exactly like
        // an in-flight deploy does — this is the same skip
        // `ship::gc_old_generations` used to apply directly.
        let pinned: std::collections::HashSet<String> =
            crate::build::store_gc::list_generations(&mp.generations_dir())
                .into_iter()
                .filter(|g| is_pinned(g) || crate::build::feeds::search_lane::is_indexing(mp, g))
                .collect();
        let _ = tokio::task::spawn_blocking(move || {
            let mp = crate::moss_paths::MossPaths::new(&project_root);
            collect_build_store(&mp, &gen_id, &pinned);
        })
        .await;
        // 3b. Hand the FROZEN generation to the search lane (ADR-045) — staging
        //     is wrong, the next build rewrites it. Nothing here awaits.
        //     `Freshness::Now` (a process that exits when the build returns)
        //     already indexed synchronously inside the build, and a request
        //     issued here would die with the runtime mid-index — so the
        //     promise and its keeper now gate on the SAME value (ADR-010's
        //     first corollary; they used to sit on different predicates).
        use crate::build::feeds::search_lane as lane;
        if matches!(freshness, lane::Freshness::Lane) && lane::enabled_for(mp) {
            let want = lane::PageSet::of(&sealed.site_hashes_view().files);
            lane::request(mp, sealed.generation_id(), want);
        }
    }

    // 4. Stale-file + stale-dir cleanup. Unconditional on `mat_ok` (staging is
    //    still this build's), but not from a superseded tail — a newer build
    //    already swept the same shared tree, and this view would delete its files.
    let view = sealed.site_hashes_view();
    if owns_shared {
        crate::build::media::pipeline::remove_stale_files(stage_dir, view, "staging");
        let expected_dirs = crate::build::media::pipeline::compute_expected_dirs(view);
        crate::build::media::pipeline::remove_stale_dirs(stage_dir, &expected_dirs);
    }

    // Seal-persist-race-404 fix: release the stage-write guard now — steps
    // 2-4 (the only ones touching `stage_dir`) are done. Everything below
    // (advertising the manifest, progress events) doesn't touch the
    // filesystem, so there's nothing left to protect.
    drop(_stage_write_guard);

    // Compute the post-seal asset diff HERE (step 7 emits it) while `view` still
    // borrows `sealed` — step 5 below MOVES `sealed` into the deploy slot. The
    // result is an owned Vec, so the borrow ends here.
    let settled_changed = diff_settled_assets(&previous_hashes, view);

    // 4b. Classify what publishing this generation would change on the live
    //     site, against the record of the last publish that landed — or, with
    //     no record, against the last deployed generation still on disk
    //     (`manifest::backfill`). `tail_speaks` decides whether this tail may
    //     describe a publish at all; `None` means it has nothing to say, and
    //     step 5b then leaves the stash alone (see its doc comment).
    //
    //     The LOCAL arms run before step 5, borrowing `sealed` rather than
    //     cloning it: the app's deploy waits on `has_ui_bound()`, which this
    //     whole function holds, so moving the walk later shortens nobody's wait
    //     and the clone would cost a manifest copy per seal. A headless deploy
    //     needs no such wait: `headless_for` leaves `spawner: None`. Only
    //     the AskServer arm pays a clone, and only when a port exists — the
    //     price of resolving the network ask after step 5. Here because this
    //     is the one point where a complete manifest exists — on every path,
    //     including the one-shot, which awaits this same function inline
    //     since #1097.
    use crate::build::manifest::backfill::{self, SealVerdict};
    let verdict =
        backfill::for_seal(mp, backfill::tail_speaks(&sealed, mat_ok, owns_shared)).await;
    // The server ask (the AskServer arm) is deliberately NOT resolved here:
    // it is a network call, and step 5 below is what deploy waits on. Clone
    // its two inputs out of `sealed` before step 5 moves it, resolve after.
    let server_ask = match (&verdict, &ports.server_diff) {
        (Some(SealVerdict::AskServer), Some(_)) => {
            Some((sealed.files().clone(), sealed.generation_id().to_string()))
        }
        _ => None,
    };

    // 5. Advertise the sealed manifest to deploy ONLY when its generation
    //    materialized on disk (mat_ok).
    if mat_ok {
        announcer.adopt_sealed(sealed).await;
    }

    // 5b. Stash the change set as a read model for webviews that boot between
    //     builds (pull via `deploy::get_publish_change_set`), then push it to
    //     the ones already listening. A tail with nothing to say (4b) does
    //     neither, and does not CLEAR: the manifest step 5 left standing is
    //     still publishable and the standing stash describes it — they are a
    //     pair (`AppState::current_change_set`), keyed by folder on both sides.
    //     Resolving AskServer here — after step 5 — is what keeps a slow
    //     server off the publish path: it can delay this stash refresh (by
    //     the port's bounded timeout), never the manifest deploy reads.
    let change_set = match verdict {
        None => None,
        Some(SealVerdict::Ready(set)) => Some(set),
        Some(SealVerdict::AskServer) => Some(match (&ports.server_diff, server_ask) {
            (Some(port), Some((files, gen_id))) => {
                backfill::from_server(port, files, gen_id).await
            }
            _ => Default::default(),
        }),
    };
    announcer
        .publish_change_set(mp.project_root(), change_set)
        .await;

    // 6. Emit the "Sealed" progress tick (mat_ok-gated).
    reporter.report(&crate::build::progress::PipelineEvent::BackgroundProgress {
        task: "sealing".to_string(),
        current: 1,
        total: 1,
        message: if mat_ok { "Sealed".to_string() } else { "Sealed (materialize failed)".to_string() },
        completed: true,
        advisories: vec![],
    });

    // 7. Post-seal asset sweep (2026-07-02): the one authoritative "background
    //    variants have materialized" signal, decoupled from the watch refresh-
    //    diff (which runs on a pre-background snapshot that can never contain the
    //    freshly-encoded .webp). Diff the sealed view's image/video variants
    //    against the previous manifest and, if any are new-or-changed, emit
    //    AssetsSettled so the frontend swaps each LQIP placeholder to the real
    //    variant via the iframe's page-agnostic srcset cache-bust. Only when
    //    mat_ok (the variants are actually on disk under current/). Empty set →
    //    nothing emitted (pure text edit / identical re-encode).
    if mat_ok && !settled_changed.is_empty() {
        reporter.report(&crate::build::progress::PipelineEvent::AssetsSettled {
            changed: settled_changed,
        });
    }
}

/// Retain generations and sweep the content-addressed cache after a successful
/// materialize. Called from `advertise_sealed`, the one seal tail on every
/// path since #1097 — the CLI's hand-copied predecessor had no retention call
/// at all before moss#976, so `moss build` grew `generations/` without bound.
///
/// Both collectors are non-fatal: a GC failure never invalidates a build that
/// already succeeded.
///
/// **Callers must hold the per-folder `stage_write_lock`.** Every path does:
/// both CLI entry points register a `FolderSession` (since 2026-08-24), so the
/// "no concurrent build is possible on the CLI" exemption this doc used to
/// claim is gone — and it was never true of `--serve --watch` anyway.
/// `cache::gc` requires a quiescent cache.
fn collect_build_store(
    mp: &crate::moss_paths::MossPaths,
    current_gen_id: &str,
    pinned: &std::collections::HashSet<String>,
) {
    use crate::build::store_gc;

    let project_path = mp.project_root().to_string_lossy().to_string();
    // The app resolves the two app-side inputs — the config knob and the
    // last-deployed pointer — and hands `store_gc` plain data. That is what
    // keeps the collector tauri-free and mechanically movable into
    // `crates/moss-build` at M6a.
    let last_deployed = crate::build::site_config::get_domain_config(&project_path)
        .ok()
        .and_then(|c| c.last_deployed_generation_id);
    let roots = store_gc::gc_roots(current_gen_id, pinned, last_deployed.as_deref());
    let keep = store_gc::effective_keep_generations(
        crate::build::site_config::get_build_keep_generations(&project_path),
        &mp.build_dir(),
    );
    if let Err(e) = store_gc::gc_old_generations(&mp.generations_dir(), &roots, keep) {
        log::warn!("generation GC failed (non-fatal): {}", e);
    }
    store_gc::maybe_gc_cache(&mp.build_dir());
}

