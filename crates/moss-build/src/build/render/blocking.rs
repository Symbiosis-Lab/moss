//! Blocking content generation phase: produces all HTML/CSS/JS needed for immediate preview.
//!
//! Part of the two-phase build pipeline. The blocking phase
//! writes every file that must exist before the preview iframe can render.
//! Asset-heavy work (images, static dirs, video conversion) is deferred to
//! `BackgroundContext` and handled by the background phase in `pipeline.rs`.

use crate::build::cloud_readiness;
use crate::build::phase::PhaseTrace;
use crate::build::cli_output::{cli_warn, log_warn_problem};
use crate::moss_paths::MossPaths;
use crate::{build::types::ParsedDocument, types::{content::{ProjectStructure, SiteHashes, SiteResult}, services::BackgroundContext}};
use moss_core::PageKind;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

// Import progress helper
use crate::build::pipeline::send_progress;

// Generator submodule imports
use crate::build::scan::article_map::build_article_map;
use crate::build::scan::classify::folder_index_keys;
use crate::build::components::nav::{NavigationBuilder, compute_breadcrumb_segments};
use crate::build::page::layout::LayoutConfig;
use crate::build::markdown::{process_markdown_file, resolve_duplicate_slugs_with_lang};
use crate::build::folder_embed::{generate_children, resolve_children_config};
use crate::build::assets::paths::{compute_binary_hash, compute_content_hash, PathResolver};
use crate::build::context::BuildContext;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::media::qr;
use crate::build::outcome::BuildStopped;
use crate::build::served_path::ServedPath;
use crate::build::page::shell::{
    ShellProcessor, ShellType, ShellVars,
};
use crate::build::scan::page_map::{
    build_page_map_and_external_urls_cached, compute_home_file_winners, compute_home_overrides,
    folder_of, resolve_folder_languages, FolderLangCache, FrontmatterScanCache,
};
use crate::build::scan::page_map::resolve_path_with_overrides;
use crate::build::scan::cascade::apply_cascade;
use crate::build::scan::sort_inference::populate_direct_children_sorts;

// Sibling module imports (within build/render/)
pub use super::config::SiteConfig;
pub use crate::build::incremental_gates::IncrementalGates;
use super::config::{resolve_logo_url, resolve_data_attr, resolve_comments_attr};
use super::image_util::PREVIEW_MAX_CHARS;
use super::preflight::{check_misplaced_theme_files, check_mixed_multilingual_structure};
use super::html::{generate_html_collect_og, tab_title};

/// # Note
/// The caller is responsible for creating the output directory and managing
/// atomic swaps. This function assumes output_dir is a clean, empty directory.
/// Tuple returned by [`generate_blocking_content`].
///
/// `documents` is the parsed page slice the caller (`pipeline::run`) hands
/// back to `build.rs` for post-build native-slot collection. An earlier
/// change made it include both renderable pages AND slot-only files
/// (e.g. `footer.md`) so `collect_footer_slots_by_language` and
/// `should_inject_subscribe_assets` can read the typed page set
/// directly — replacing the previous filesystem-scan hacks
/// (`render_footer_pages_from_disk`, `project_has_inline_subscribe`).
///
/// The fourth element is the shadow-verification snapshot: `Some` only under
/// `MOSS_INCREMENTAL_VERIFY=1`, and only ever consumed
/// AFTER the enhance hook has run — see `render::incremental::carry_verify`.
pub type BlockingContentOutput = (
    SiteResult,
    BackgroundContext,
    Vec<ParsedDocument>,
    Option<crate::build::render::incremental::CarryVerification>,
);

/// Map each auto-generated folder-index URL key (the lowercased slug path,
/// e.g. `writings`) back to that folder's ORIGINAL on-disk leaf name
/// (`Writings`).
///
/// The URL slug is intentionally lowercased for the address, but the
/// folder-index page TITLE/H1 must preserve the author's directory casing: a
/// home-less `Writings/` folder should read "Writings", not "writings". The
/// synthetic folder-index title is otherwise derived from the slug leaf, which
/// [`generate_slug`](crate::build::markdown::generate_slug) has already
/// lowercased — so without this lookup the on-disk case is lost.
///
/// Keys come from [`folder_index_keys`], so they line up exactly with the keys
/// the two folder-index synthesis blocks below seed.
fn folder_display_leaves(
    dirs: &[String],
    dir_overrides: &std::collections::HashMap<String, String>,
) -> std::collections::HashMap<String, String> {
    folder_index_keys(dirs, dir_overrides)
        .map(|(dir, key)| (key, dir.rsplit('/').next().unwrap_or(dir).to_string()))
        .collect()
}

/// The language a synthetic folder index speaks: its path (`en/` → `En`),
/// else what the folder declared or inferred — the SAME `folder_languages`
/// the authored pages read. Reading only the path put a synthesized index in
/// English chrome, saying "3 posts" over the Chinese articles it lists.
/// `folder_languages` is keyed by on-disk directory while `folder` is the
/// URL-space key, so the lookup only lands where slug and directory name
/// agree; a folder whose two names differ falls through to `site_lang`.
fn synthetic_folder_lang(
    folder: &str,
    folder_languages: &std::collections::HashMap<String, crate::i18n::Language>,
    site_lang: crate::i18n::Language,
) -> crate::i18n::Language {
    crate::i18n::path::ancestor_lang_from_path(&format!("{}/index.md", folder))
        .or_else(|| folder_languages.get(folder).copied())
        .unwrap_or(site_lang)
}

/// The synthetic folder document for an indexless folder — the one
/// constructor both folder-index synthesis blocks read, so a folder has
/// identical title, label and language as a page-tree entry and as an
/// emitted page.
///
/// Title: a term pseudo-folder takes the term's display name (the
/// first-seen original-case form — never `filename_text`, which would
/// mangle a hyphenated slug like "david-yang"); a term namespace root takes
/// its i18n heading; any other folder its ORIGINAL on-disk name (proper
/// case, "Writings") over the lowercased URL-slug leaf ("writings"), routed
/// through `filename_text` (no title-casing) so a folder named
/// `tech-tips` renders "tech tips" with or without an `index.md`. Falls
/// back to the slug leaf for URL-override/also_in folders with no matching
/// on-disk dir.
///
/// A term namespace root is kept out of every listing by the frontmatter
/// field an author would use for the same effect (`listed: false` —
/// `is_listable`, which every list-emitting site consults, and which also
/// drops it from the sitemap: the breadcrumb on every term page is the
/// crawler's path to it). `nav: false` is declared intent only: no folder
/// index is root-level, so none is ever a nav item today. No consumer needs
/// to know it is a root.
fn synthetic_folder_doc(
    folder: &str,
    term_index: &crate::build::terms::TermIndex,
    folder_display: &std::collections::HashMap<String, String>,
    folder_languages: &std::collections::HashMap<String, crate::i18n::Language>,
    site_lang: crate::i18n::Language,
) -> ParsedDocument {
    let folder_leaf = folder.rsplit('/').next().unwrap_or(folder);
    let lang = synthetic_folder_lang(folder, folder_languages, site_lang);
    // A term namespace root (`authors`, `tags`) is the namespace in use,
    // whatever serves the key: the real directory a claim file lands in
    // (`作者/` with `url: authors`) IS the root, so its synthesized index
    // keeps the localized title and stays unlisted as the pseudo-root did.
    // Before 2026-09-05 a real directory outranked the pseudo-root, which
    // retitled Authors to `authors` and relisted it at the first claim file.
    let term_root = term_index.roots_in_use().any(|ns| ns == folder);
    let title = if let Some(term_display) = term_index.display(folder) {
        term_display.to_string()
    } else if let Some(kind_title) = term_index.kind_for(folder).map(|k| k.title.clone()) {
        kind_title
    } else if let Some(endonym) = (!folder.contains('/')).then(|| moss_core::home::endonym(folder)).flatten() {
        // A language tree's synthesized home (`zh-hans/`, no `index.zh-hans.md`)
        // is titled by the language's own name, not its code's filename text.
        endonym.to_string()
    } else {
        let display_leaf = folder_display.get(folder).map(String::as_str).unwrap_or(folder_leaf);
        moss_core::heading::filename_text(display_leaf)
    };
    let hidden_root = term_root.then_some(false);
    ParsedDocument {
        label: title.clone(),
        title,
        url_path: format!("{}/index.html", folder),
        kind: PageKind::Folder,
        slug: folder_leaf.to_string(),
        permalink: format!("/{}/", folder), // allow:served-path-url-construct (folder index page permalink, derived from folder path)
        lang,
        clean_stem: folder_leaf.to_string(),
        nav: hidden_root,
        listed: hidden_root,
        ..Default::default()
    }
}

/// Hash a page's (or, since `is_reload_tracked_source_key` widened, one of
/// the synchronous vault-config sources') raw bytes for the durable manifest.
///
/// The parse cache's own index cannot answer "did the author change this page
/// since the last publish?" — it is a watch-loop memo, consulted only under
/// `BuildTrigger::ContentOnly`, and every cross-process entry point (`moss
/// build`, deploy) cold-misses it by construction. `SiteHashes::sources` can
/// answer it, and it is persisted. SHA-256 keeps page entries in the same hash
/// domain as the asset entries already in that map.
///
/// `bytes` is passed separately from `path` because the two differ on the uid
/// write-back path: moss rewrites the file's frontmatter after parsing, and the
/// bytes now on disk are the ones the next build will hash. `pub(crate)`
/// rather than file-local: `pipeline.rs`'s places.toml registration is not
/// inside this file (the gazetteer is read before `generate_blocking_content`
/// is even called) and reuses this rather than a second hasher.
pub(crate) fn source_metadata(path: &Path, bytes: &[u8]) -> crate::build::types::SourceMetadata {
    use sha2::{Digest, Sha256};
    let md = std::fs::metadata(path).ok();
    let mtime = md
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
    let (ctime, inode) = md
        .as_ref()
        .map(crate::build::types::stat_identity)
        .unwrap_or((None, None));
    crate::build::types::SourceMetadata {
        hash: format!("{:x}", Sha256::digest(bytes)),
        size: md.as_ref().map(|m| m.len()).unwrap_or(bytes.len() as u64),
        mtime: mtime.map(|d| d.as_secs()).unwrap_or(0),
        // None ⇔ no readable mtime; the watcher gate then always hashes
        // instead of trusting a size-only match (fail open).
        mtime_nanos: mtime.map(|d| d.subsec_nanos()),
        ctime,
        inode,
    }
}

/// Register a page's source hash beside its `source_to_output` mapping.
///
/// **Call this everywhere `register_source_mapping` is called.** The two maps
/// must cover the same page set: a mapping without a hash makes the page
/// unclassifiable at publish time, and on a watch-loop rebuild — where most of
/// a large site is skipped — that is most of the site.
///
/// A page absent from `hashes` was not read this build, which is only ever
/// because its bytes provably did not change (parse-cache hit, Stage-5b carry)
/// or could not be read (iCloud deferral). All three want the previous hash.
fn register_page_source(
    pending: &mut PendingManifest,
    hashes: &HashMap<String, crate::build::types::SourceMetadata>,
    src: &str,
) {
    match hashes.get(src) {
        Some(meta) => pending.register_page_source_hash(src.to_string(), meta.clone()),
        // `None` here means the previous manifest had no hash either (first
        // build, or one predating page hashing) — the classifier's hash-carry
        // valve reads that as "unknown", never as added or deleted.
        None => {
            pending.carry_forward_page_source(src);
        }
    }
}

pub fn generate_blocking_content(
    root: &crate::vault::paths::VaultRoot,
    project_structure: &ProjectStructure,
    output_dir: &Path,
    services: Option<&crate::types::services::BuildServices>,
    progress_sender: Option<&dyn crate::build::ports::reporter::BuildReporter>,
    emit_source_lines: bool,
    site_config: SiteConfig,
    pending: &mut PendingManifest,
    // `BuildStopped`, not `String`: this runs before the cloud gate is emitted,
    // so an `Err` here is what used to silence it. See `build::outcome`.
) -> Result<BlockingContentOutput, BuildStopped> {
    let total_start = std::time::Instant::now();

    // `pending` is pre-seeded with previous_hashes by the caller (PendingManifest::new).
    // Pattern A artifact registrations route through:
    //   BuildContext::for_render(output_dir, pending).emit(&ServedPath::from_source(&rel_path), bytes, HashBucket::Files)
    // Pattern D (OG cards, Sites 4/14b) register from the receipt the render returns
    //   (`CardOutput::register`): the file is already written, so nothing is re-written.
    //
    // Pattern E (SVG video placeholder) was removed; in the PREVIEW the
    // blueprint placeholder (injected client-side by the desktop app's preview
    // bridge, on top of what the preview server serves) stands in for a
    // thumbnail that hasn't landed yet. A
    // published site gets no placeholder — it can't contain a broken reference,
    // because moss refuses to deploy one. The previously-required
    // `blocking_insert!` macro is gone too.

    // `source_path` shorthand keeps the ~80 interior uses unchanged; the root NAME now
    // comes from `root.name()` and is never re-derived from this string.
    let source_path = root.as_str();
    let source_path_buf = root.path();
    let moss_dir = source_path_buf.join(".moss");
    let paths = MossPaths::from_moss_dir(moss_dir.clone());

    // Load previous hashes for:
    // 1. Video conversion caching (sources) — needed by BackgroundContext
    // 2. Carry-forward is already in `pending.inner` (seeded by caller via PendingManifest::new)
    let hashes_path = paths.hashes();
    let previous_hashes: SiteHashes = if let Ok(content) = fs::read_to_string(&hashes_path) {
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        SiteHashes::default()
    };
    // Note: the carry-forward of files/video_outputs/image_outputs/notebook_outputs is now
    // implicit in `pending` (caller seeded it with PendingManifest::new(previous_hashes)).
    // We still need `previous_hashes` for BackgroundContext.previous_hashes.

    // Create .moss directory if it doesn't exist
    if !moss_dir.exists() {
        // allow:raw_write `.moss` itself; the regenerable tree starts below it
        fs::create_dir_all(&moss_dir)
            .map_err(|e| format!("Failed to create .moss directory: {}", e))?;
    }

    // Copy user's favicon (svg, png, or ico) or write default moss logo.
    // When the favicon is SVG, also rasterize PNG sizes 16/32/180.
    let favicon = resolve_favicon(Path::new(source_path), output_dir)?;
    // Site 1 (Pattern A): register favicon in manifest.
    // resolve_favicon already wrote the file; re-read bytes so ctx.emit can
    // compute the SHA-256 and register in pending. The overwrite is harmless
    // (identical bytes). Fall back to apply_message on read error.
    {
        let rel = favicon.relative_path.clone();
        let abs = output_dir.join(&rel);
        match fs::read(&abs) {
            Ok(bytes) => {
                let sp = ServedPath::from_source(&rel)
                    .map_err(|e| format!("Failed to construct favicon path: {}", e))?;
                BuildContext::for_render(output_dir, pending)
                    .emit(&sp, &bytes, HashBucket::Files)
                    .map_err(|e| format!("Failed to emit favicon: {}", e))?;
            }
            Err(e) => {
                log::warn!("could not read favicon for manifest registration {}: {}", rel, e);
                pending.apply_message(rel, &favicon.content_hash, HashBucket::Files, None);
            }
        }
    }
    if favicon.has_raster_pngs {
        // Site 2 (Pattern A): register each rasterized PNG in the manifest.
        // The actual bytes were written synchronously by `generate_favicons`;
        // re-hash via ctx.emit using SHA-256. Fall back to apply_message on
        // read error so stale cleanup still preserves the path.
        for size in [16u32, 32, 180] {
            let rel = format!("assets/favicon-{}.png", size);
            let abs = output_dir.join(&rel);
            match fs::read(&abs) {
                Ok(bytes) => {
                    let sp = ServedPath::from_source(&rel)
                        .map_err(|e| format!("Failed to construct favicon PNG path: {}", e))?;
                    BuildContext::for_render(output_dir, pending)
                        .emit(&sp, &bytes, HashBucket::Files)
                        .map_err(|e| format!("Failed to emit favicon PNG: {}", e))?;
                }
                Err(e) => {
                    log::warn!("could not hash {}: {}", rel, e);
                    pending.apply_message(rel, "favicon-png", HashBucket::Files, None);
                }
            }
        }
    }
    let favicon_filename = favicon.filename;
    let favicon_has_raster_pngs = favicon.has_raster_pngs;

    // Build ContentGraph once for Obsidian-style fuzzy path resolution.
    let content_graph = crate::build::scan::scan::build_content_graph(project_structure);
    log::debug!(target: "timing", "[render] content_graph: {:?}", total_start.elapsed());

    // Pre-compute home-file winners. The root folder name is load-bearing — it decides
    // whether a self-named note (`site/site.md`) is the home at `/`, and it seeds the site
    // name — and it now arrives already resolved on `root`. (Before 2026-07-24 this line
    // re-derived it by re-reading the filesystem, while `compute_home_overrides`, eight
    // lines down, read `""` from the same string.)
    let root_folder_name = root.name();
    // A doc with the `home: true` marker wins its folder's home slot regardless
    // of filename — lets `en/Mountain Home.md` (or any non-INDEX_STEM, non-self-named
    // file) be the EN homepage at `/en/` instead of getting a slug URL while
    // moss synthesizes an empty `en/index.html` titled `"En"`.
    let home_overrides = compute_home_overrides(&project_structure.markdown_files, root);
    let home_file_winners = compute_home_file_winners(
        &project_structure.markdown_files,
        root_folder_name,
        &home_overrides,
    );

    // Build PageMap: pre-scan all markdown files to compute source_path → url_path mapping.
    // This includes cascading url overrides from folder index files, so child pages
    // get their final url_path before the main processing loop.
    //
    // Merged with the `external_url_map` pre-scan below (linkblog pattern)
    // into ONE read+parse pass per file, cached across builds by
    // `FrontmatterScanCache` — both fields live on the same parsed
    // frontmatter struct, and on an unchanged file neither needs re-reading
    // at all. Before this, both scans ran full-corpus on EVERY build
    // regardless of what changed: measured on a 226-page vault, ~150ms +
    // ~180ms on a rebuild that touched exactly one file.
    let frontmatter_scan_cache_path = paths.cache_frontmatter_scan();
    let mut frontmatter_scan_cache = FrontmatterScanCache::load(&frontmatter_scan_cache_path);
    let (page_map, dir_overrides, external_url_map) = build_page_map_and_external_urls_cached(
        &project_structure.markdown_files,
        source_path_buf,
        root_folder_name,
        &home_file_winners,
        &home_overrides,
        &mut frontmatter_scan_cache,
    );
    if let Err(e) = frontmatter_scan_cache.save(&frontmatter_scan_cache_path) {
        log::warn!("failed to persist frontmatter-scan cache: {}", e);
    }
    log::debug!(target: "timing", "[render] page_map ({} entries): {:?}", page_map.len(), total_start.elapsed());

    // Per-folder language for folders with no naming convention —
    // declared in the folder's index, else inferred — computed ONCE here (not
    // per page below) and keyed to each folder's file SET so an edited body
    // never moves it; only adding/removing a file, or declaring, does. See
    // `scan::page_map::folder_lang` for the stability mechanism.
    let folder_lang_cache_path = paths.cache_folder_lang();
    let mut folder_lang_cache = FolderLangCache::load(&folder_lang_cache_path);
    // Its declaration rung reads `frontmatter_scan_cache`, which the page-map
    // pass above has just filled for every file — hence the ordering.
    let folder_languages = resolve_folder_languages(
        &project_structure.markdown_files, source_path_buf,
        &mut folder_lang_cache, &frontmatter_scan_cache,
    );
    if let Err(e) = folder_lang_cache.save(&folder_lang_cache_path) {
        log::warn!("failed to persist folder-lang cache: {}", e);
    }
    log::debug!(target: "timing", "[render] folder_languages ({} folders): {:?}", folder_languages.len(), total_start.elapsed());

    // With the build's slug-overrides installed, the graph answers "what URL is
    // this file served at" for every emitter downstream.
    let content_graph = content_graph.with_output_overrides(dir_overrides.clone());

    // Same shape as `page_map` but keyed on source paths whose frontmatter
    // declares an absolute `external_url:` (linkblog pattern).
    // Empty when no page in the site sets the field. Passed into
    // `process_markdown_file` so the wikilink resolver routes cross-references
    // to the external destination instead of the local archive.
    log::debug!(target: "timing", "[render] external_url_map ({} entries): {:?}", external_url_map.len(), total_start.elapsed());

    // Resolved once at the pipeline's config construction and handed down (see
    // `SiteConfig::lang`); here the code becomes a `Language`, English being
    // what "no translation table" means. Reused as the canonical site_lang
    // below, so `process_markdown_file`, `resolve_duplicate_slugs_with_lang`
    // and the layout config all agree.
    let site_lang: crate::i18n::Language =
        crate::i18n::Language::from_code(&site_config.lang).unwrap_or(crate::i18n::Language::En);

    // Resolve site_id for the inline subscribe shortcode. Per-build (not per-file)
    // so we read .moss/config.toml once. None means no published site → renders
    // an empty action path which the form treats as a no-op.
    let site_id = crate::build::site_config::get_domain_config(source_path)
        .ok()
        .and_then(|c| c.site_id);

    // Resolve the seta base URL once per build from the project environment.
    // Passed into `process_markdown_file` so inline `:::subscribe` shortcodes
    // emit the correct action URL for staging/local environments.
    let seta_url_for_build: String = crate::build::site_config::resolve_environment(source_path)
        .seta_url()
        .to_string();

    // Open the Loop A parse cache. Eligibility is
    // `site_config.incremental.parse_cache` — a SIBLING of the render
    // skip, not the same bit: both are off under `MOSS_NO_INCREMENTAL` and for
    // any non-`ContentOnly` trigger (there is still no second kill switch by
    // design), but the parse cache tolerates a batch containing stylesheets,
    // scripts and fonts, which cannot change any page's parse. Images and
    // `config.toml` still disable it. See `build/parse_cache.rs` and
    // `PipelineConfig::allows_parse_cache_reuse`.
    //
    // The fingerprint covers Loop A's inputs that are not any page's own bytes
    // — most importantly `page_map`, which pre-scans every file's frontmatter,
    // so a folder index's `url:` edit reshapes URLs for a whole subtree while
    // moving no other file. A mismatch bypasses the cache for the entire build.
    let parse_session = crate::build::parse_cache::ParseSession::begin(
        Path::new(source_path),
        &paths.cache_hash_index(),
        site_config.incremental.parse_cache,
        crate::build::parse_cache::inputs_fingerprint(
            &project_structure
                .markdown_files
                .iter()
                .map(|f| f.path.clone())
                .collect::<Vec<_>>(),
            &page_map,
            &dir_overrides,
            &external_url_map,
            &home_file_winners,
            root_folder_name,
            // MIRROR OF `process_markdown_file`'s SIGNATURE. Every build-level
            // scalar that reaches that call below — and therefore can change a
            // page's `html_content` without changing its bytes — must appear
            // here, or a cache hit replays HTML rendered under the old value.
            // `heading_anchors` was missing until 2026-08-06: it was masked
            // only by the old markdown-only gate, which a `config.toml` edit
            // could never pass. Add a parameter there, add it here, in the
            // same commit. The `[site]` flags ride as one `SiteMarkdown`, the
            // same value the call takes, so a field added to it needs no edit
            // here.
            &crate::build::parse_cache::site_scalars(
                site_lang,
                site_id.as_deref(),
                seta_url_for_build.as_str(),
                site_config.markdown(),
                emit_source_lines,
                project_structure.has_content_folders,
            ),
        ),
    );

    // Process markdown files. Emits `MossEvent::BackgroundProgress` under task
    // "markdown" every MARKDOWN_PROGRESS_STRIDE files so both the GUI
    // deploy panel and watch-mode preview show a live counter during
    // long renders (~1s/file on large image-heavy sites,
    // where 131 files took ~97s before this instrumentation). The event
    // is emitted by every caller that passes an `AppHandle` — deploy,
    // watch, first-build — so this is the single source of truth for
    // rendering progress.
    const MARKDOWN_PROGRESS_STRIDE: usize = 5;
    let markdown_total = project_structure.markdown_files.len() as u32;
    let reporter = services.map_or(crate::build::ports::reporter::discarding(), |s| s.reporter.as_ref());

    // Build the media-dimension lookup once for the run — markdown emission
    // (via `process_markdown_file`) consults it through the structural-html
    // synthesizer to bake dimensions, LQIP, and the optional
    // `<picture><source>` wrap into the emitted HTML at event-iterator time.
    // The synthesizer in moss-core owns all attribute emission; no downstream
    // regex pass runs at the end of the per-doc loop.
    //
    // Ladders encoded by a previous build, read off disk BEFORE the lookup below
    // folds the registry into its snapshot. Order is the whole point: the
    // snapshot is built once, in the constructor, so a ladder registered after
    // that line is invisible for the rest of the build. See
    // `hls::register_existing_ladders`.
    if let Some(reg) = services
        .and_then(|s| s.assets.as_deref())
        .filter(|_| !project_structure.video_files.is_empty())
    {
        crate::build::media::hls::register_existing_ladders(output_dir, reg);
    }
    let event_level_image_lookup = crate::build::media::dimensions::MediaDimensionLookup::new(
        &project_structure.image_files,
        &project_structure.video_files,
        &dir_overrides,
        services.and_then(|s| s.assets.as_deref()),
    );

    // Pre-fetched asset data (dims/LQIP/dominant-color from
    // MediaDimensionLookup, registered WebP/AVIF variants from AssetRegistry)
    // packaged for moss-core's synthesizer to consume.
    let asset_snapshot = event_level_image_lookup.asset_snapshot();

    // Source-path → hash of the bytes this build actually read, for the pages
    // it actually read. Absent means "skipped" (parse-cache hit, Stage-5b
    // carry, iCloud deferral), which `register_page_source` turns into a
    // carry-forward rather than a gap. See `PendingManifest::register_page_source_hash`.
    let mut page_source_hashes: HashMap<String, crate::build::types::SourceMetadata> = HashMap::new();

    // `deferred_paths` and `missing_media` travel out of the parallel markdown
    // pass with the documents — see their Mutexes' doc comments.
    let (mut documents, deferred_paths, missing_media) = {
        use rayon::prelude::*;
        use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

        // Durable timing, all builds — the cpu-sum log below is debug-only.
        let _phase_markdown = PhaseTrace::start("render_markdown");

        // Parallel markdown: read → resolve → process is independent per file
        // (the dominant blocking-path cost measured on image vaults). The typed-
        // embed renderer registry + Deferred-marker handlers are built once PER
        // RAYON THREAD via map_init — NOT once per file (each handler captures
        // the site-root PathBuf, so per-file would clone it for every file) and
        // NOT shared across threads (MarkerHandler is `Box<dyn Fn>`, !Sync).
        // Results collect in `markdown_files` order (rayon's indexed collect),
        // so `documents` order — which the render loop and the manifest depend
        // on — is identical to the old serial loop. Debug sub-phase timers are
        // atomic sums of per-file CPU (they overlap in wall-clock now).
        let md_read_ns = AtomicU64::new(0);
        let md_resolve_ns = AtomicU64::new(0);
        let md_process_ns = AtomicU64::new(0);
        let processed = AtomicUsize::new(0);
        let md_wall = std::time::Instant::now();

        // Paths skipped this build because their source (or an embedded
        // reference) was a cloud-dataless placeholder. Non-empty means this
        // build is incomplete — callers use it to skip `remove_stale_html`
        // (its previous-generation HTML must survive) and to suppress a
        // false "done" cloud-sync event. A `Mutex<Vec<_>>` (not per-thread +
        // reduce) because entries are rare and the lock is uncontended on
        // the fast path where nothing is evicted.
        let deferred_mutex: std::sync::Mutex<Vec<std::path::PathBuf>> = std::sync::Mutex::new(Vec::new());
        // Media references that resolve to nothing. Same Mutex-over-reduce
        // reasoning as `deferred_mutex`: on a healthy site this is never
        // locked. Non-empty means the site cannot be published.
        let missing_media_mutex: std::sync::Mutex<Vec<crate::build::types::MissingMedia>> =
            std::sync::Mutex::new(Vec::new());

        let rendered: Vec<Option<(ParsedDocument, Option<(std::path::PathBuf, String)>, Option<crate::build::types::SourceMetadata>)>> = project_structure
            .markdown_files
            .par_iter()
            .map_init(
                || moss_core::resolve::registry::RendererRegistry::empty().build(),
                |resolve_registry, file_info| {
                    // Localize embed-error diagnostics (missing notebook/table)
                    // in the page's own language. The language folder is
                    // derivable from the path alone — before any frontmatter
                    // parse — which covers language-tree pages; the handlers are
                    // cheap to rebuild per file.
                    let embed_lang = crate::i18n::path::ancestor_lang_from_path(&file_info.path)
                        .unwrap_or(site_lang);
                    let resolve_marker_handlers = crate::build::embed_handlers::builtin_marker_handlers(
                        Path::new(source_path).to_path_buf(),
                        embed_lang,
                    );

                    // Monotonic progress — items finish out of order under
                    // parallelism, so report a completion count, not an index.
                    let done = processed.fetch_add(1, Ordering::Relaxed) + 1;
                    if markdown_total > 0
                        && (done % MARKDOWN_PROGRESS_STRIDE == 0 || done == markdown_total as usize)
                    {
                        reporter.report(&crate::build::progress::PipelineEvent::BackgroundProgress {
                            task: "markdown".to_string(),
                            current: done as u32,
                            total: markdown_total,
                            message: format!(
                                "Rendering markdown ({}/{})",
                                done,
                                markdown_total
                            ),
                            completed: false,
                            advisories: vec![],
                        });
                    }
                    let source_file_path = Path::new(source_path).join(&file_info.path);

                    // Replay the previous build's parse when
                    // this page's bytes AND every file transcluded into it are
                    // provably unchanged. A hit skips the read, the Obsidian
                    // resolve and the parse — Loop A's whole per-file cost.
                    // The document returned here is a PRE-Reduce snapshot, the
                    // same shape the fresh path below produces, so the
                    // whole-corpus passes downstream cannot tell the two apart.
                    // No bytes are read on this path, so no source hash is
                    // produced — the registration site carries the previous
                    // build's hash forward instead. A page with an output
                    // mapping and no hash is unclassifiable at publish time.
                    if let Some(cached) = parse_session.lookup(&file_info.path) {
                        return Some((cached, None, None));
                    }

                    let t_read = std::time::Instant::now();
                    // A source still in the cloud is DEFERRED, not dropped —
                    // `remove_stale_html` treats a page this build didn't emit
                    // as deleted. See `read_page_source`, which owns that call.
                    let content = crate::build::cloud_readiness::read_page_source(
                        &source_file_path,
                        &deferred_mutex,
                    )?;
                    md_read_ns.fetch_add(t_read.elapsed().as_nanos() as u64, Ordering::Relaxed);
                    // Hash the bytes we actually parsed, before the uid
                    // write-back below can rewrite this file.
                    parse_session.note_source_bytes(&file_info.path, content.as_bytes());
                    // The same bytes, hashed for the durable manifest.
                    let source_meta = source_metadata(&source_file_path, content.as_bytes());

                    // RESOLVE phase: transform Obsidian syntax (wikilinks, embeds,
                    // callouts, block refs) into standard markdown before HTML
                    // conversion.  This is transparent for non-Obsidian content.
                    //
                    // Typed embeds flow through here too:
                    // - Pure renderers (image, iframe, pdf, audio, video, 3D)
                    //   produce final HTML inline during wikilink resolution.
                    // - Deferred renderers (notebook, table, plugins) emit
                    //   markers resolved by MarkerHandlers in a post-pass.
                    let t_resolve = std::time::Instant::now();
                    // Phase 0 Task F1: thread the AssetSnapshot through. The
                    // snapshot is built once above the loop and reused for every
                    // markdown file in this build.
                    let mut resolve_result = moss_core::resolve::resolve_content_with_handlers_and_snapshot(
                        &file_info.path,
                        &content,
                        &content_graph,
                        &|path| {
                            let full_path = Path::new(source_path).join(path);
                            if crate::build::icloud::is_evicted(&full_path) {
                                crate::build::cloud_readiness::request_download(&full_path);
                                deferred_mutex.lock().unwrap().push(full_path);
                                return None;
                            }
                            std::fs::read_to_string(full_path).ok()
                        },
                        resolve_registry,
                        &resolve_marker_handlers,
                        asset_snapshot,
                    );
                    md_resolve_ns.fetch_add(t_resolve.elapsed().as_nanos() as u64, Ordering::Relaxed);

                    let t_process = std::time::Instant::now();
                    // This phase only ever raises advisory diagnostics —
                    // unresolved frontmatter wikilinks, broken transclusions.
                    // The blocking kind (`MissingAsset`) is raised one phase
                    // later, inside `process_markdown_file`, and arrives below
                    // on `doc.missing_media`.
                    for diag in &resolve_result.diagnostics {
                        log::warn!(
                            "Resolve: {} — ref '{}' in '{}'",
                            diag.message,
                            diag.reference,
                            diag.source_path
                        );
                    }

                    let resolved_content = &resolve_result.content_markdown;

                    // This file's folder's inferred language, when
                    // its path carries no naming convention — computed once
                    // above the loop, over the whole folder, never here.
                    let folder_lang = folder_languages.get(&folder_of(&file_info.path)).copied();

                    let mut doc = match process_markdown_file(&file_info.path, resolved_content, root_folder_name, &page_map, emit_source_lines, site_lang, site_id.as_deref(), site_config.markdown(), Some(&event_level_image_lookup), Some(&external_url_map), Some(&content_graph), Some(resolve_registry), project_structure.has_content_folders, Some(&seta_url_for_build), folder_lang) {
                        Ok(doc) => doc,
                        Err(_) => return None,
                    };
                    // Keep the transclusion
                    // edges the resolve phase just computed instead of dropping
                    // them on the floor. These are the ONLY page→page embed
                    // edges in the build — `![[note.md]]` is spliced from disk
                    // bytes here, before the AST dispatcher that fills
                    // `outgoing_links` ever runs — so they feed both the parse
                    // cache's validity check and `DepGraph::back_embeds`, which
                    // sat over an empty relation until now (Stage 5b finding #2).
                    doc.embed_deps = std::mem::take(&mut resolve_result.embed_deps);
                    // RETAIN what this page points at and hasn't got — the
                    // evidence `deploy::refuse_publish` decides on. A
                    // missing image used to be warned about and then dropped,
                    // so the site published with a 404 in it.
                    if !doc.missing_media.is_empty() {
                        missing_media_mutex
                            .lock()
                            .unwrap()
                            .append(&mut std::mem::take(&mut doc.missing_media));
                    }
                    {
                    // Deferred to a sequential post-pass: the uid write-back
                    // below mutates this file's SOURCE, and the resolve phase's
                    // embed reader reads OTHER files' sources concurrently — an
                    // in-loop `fs::write` would let an embed of this file read it
                    // mid-write (torn read). Collect the write and apply it after
                    // the parallel map, when no reads are in flight.
                    let mut uid_writeback: Option<(std::path::PathBuf, String)> = None;
                    // Demote non-winner index files to normal pages.
                    // When multiple files qualify as home files (e.g., index.md and
                    // a self-named file like 山居.md), only the winner keeps kind = Folder.
                    // The loser gets a slug-based URL so it doesn't collide.
                    if doc.kind == PageKind::Folder && !home_file_winners.contains(&file_info.path) {
                        doc.kind = PageKind::Article;

                        // Check if this is a language-suffixed variant of the home file
                        // (e.g., index.zh-hans.md alongside the winning index.md).
                        // These should keep the winner's URL so resolve_duplicate_slugs_with_lang
                        // can properly deduplicate them (e.g., index.html → zh-hans/index.html),
                        // rather than getting a separate directory URL like /index/.
                        let filename_lower = Path::new(&file_info.path)
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("")
                            .to_lowercase();
                        let is_home_translation = moss_core::home::strip_lang_suffix(&filename_lower)
                            .map_or(false, |bare| moss_core::home::is_index_stem(bare));

                        if is_home_translation {
                            // Give it the same URL as the winner (index.html for root,
                            // parent/index.html for nested). resolve_duplicate_slugs_with_lang
                            // will rename the non-default-language version.
                            let parent_path = Path::new(&file_info.path)
                                .parent()
                                .and_then(|p| p.to_str())
                                .unwrap_or("");
                            doc.url_path = if parent_path.is_empty() {
                                "index.html".to_string()
                            } else {
                                format!("{}/index.html", parent_path)
                            };
                            doc.permalink = format!("/{}", doc.url_path); // allow:served-path-url-construct (page permalink field, derived from url_path)
                            // Exclude from auto-nav: this is a translation of the homepage,
                            // not a separate navigable page.
                            doc.is_root_level = false;
                            // Home translations should use the Page template (not Article)
                            // to match the homepage's chrome (no article-style heading).
                            if doc.layout.is_none() {
                                doc.layout = Some("page".to_string());
                            }
                        } else {
                            let slug = crate::build::markdown::generate_slug(&doc.clean_stem);
                            let parent_path = Path::new(&file_info.path)
                                .parent()
                                .and_then(|p| p.to_str())
                                .unwrap_or("");
                            doc.url_path = if parent_path.is_empty() {
                                format!("{}/index.html", slug)
                            } else {
                                format!("{}/{}/index.html", parent_path, slug)
                            };
                            doc.permalink = format!("/{}", doc.url_path); // allow:served-path-url-construct (page permalink field, derived from url_path)
                            // Index files skip relative path adjustment in process_markdown_file.
                            // Now that this is a normal page, we need to apply it.
                            doc.rewrite_body(&|html| crate::build::markdown::adjust_relative_paths_for_pretty_urls_with_overrides(html, &dir_overrides));
                        }
                    }
                    // Root index files have no parent in their relative path.
                    // Apply the same "index title = parent folder" rule using source_path.
                    let is_root_index = moss_core::home::is_index_stem(&doc.title)
                        && !file_info.path.contains('/');
                    if is_root_index && !root.name().is_empty() {
                        doc.title = root.name().to_string();
                    }

                    // Resolve cover paths via ContentGraph.
                    // doc.cover is already resolved if it was a wikilink (via
                    // resolve_frontmatter_wikilinks in crates/moss-core/src/resolve/).
                    // The ContentGraph lookup here is a normalization step for cases
                    // where process_markdown_file may have made the path doc-relative.
                    //
                    // Cover values may be pipe-encoded ("path|attrs") with display
                    // params. Split off attrs before resolving, rejoin after.
                    if let Some(ref cover) = doc.cover {
                        if !cover.starts_with("http") {
                            let (path_part, attrs_str) = moss_core::media::split_pipe(cover);
                            if let Some(resolved) = content_graph.resolve_path(path_part, &file_info.path) {
                                doc.cover = if attrs_str.is_empty() {
                                    Some(resolved)
                                } else {
                                    Some(format!("{}|{}", resolved, attrs_str))
                                };
                            }
                        }
                    }

                    // Auto-assign uid if missing: generate from relative path and write back.
                    // Slot files are excluded — a uid identifies a PUBLISHED page, and
                    // minting one made the footer participate in detect_renames.
                    if doc.uid.is_none() && !doc.slot_only {
                        let uid = crate::build::markdown::generate_uid(&file_info.path);
                        let updated = crate::build::markdown::insert_uid_into_frontmatter(&content, &uid);
                        if updated != content {
                            // Defer the write to the sequential post-pass (see above).
                            uid_writeback = Some((source_file_path.clone(), updated));
                        }
                        doc.uid = Some(uid);
                    }

                    // Slugify inline asset-dir segments in every doc's HTML
                    // (src, srcset, poster, data-placeholder-src). The resolve
                    // phase in moss-core uses source filesystem names; this maps
                    // intermediate directory segments to their URL slugs so the
                    // emitted reference matches the slugified output tree that
                    // copy_deferred_assets writes. An explicit `dir_overrides`
                    // entry wins; with an empty map the base slugification still
                    // fires (`My Photos` → `my-photos`), preserving the leaf and
                    // any `../` prefix. Idempotent on already-slugged paths.
                    doc.rewrite_body(&|html| {
                        crate::build::markdown::apply_dir_overrides_to_asset_paths(
                            html,
                            &dir_overrides,
                        )
                    });

                    // Placeholder attributes (width, height, loading, LQIP/color
                    // background, .mov→.mp4 src rewrite, poster, data-thumb-src)
                    // are now emitted at parse time by the moss-core synthesizers
                    // (`render::{image,video,audio,iframe,model,pdf}`), which read
                    // from the `AssetSnapshot` built once per run. The Stage 3
                    // regex post-pass (`add_placeholder_attributes`) was retired in
                    // Phase 2E v5 PR5 (2026-05-26).

                    md_process_ns.fetch_add(t_process.elapsed().as_nanos() as u64, Ordering::Relaxed);
                    // No-op unless MOSS_PARSE_CACHE_SHADOW is set, in which
                    // case the cache's would-be hits are checked against the
                    // parse that actually ran.
                    parse_session.verify_shadow(&file_info.path, &doc);
                    Some((doc, uid_writeback, Some(source_meta)))
                }
                },
            )
            .collect();
        log::debug!(target: "timing", "[render] markdown ({} files): wall={:?} (cpu sums: read={:?} resolve={:?} process={:?})",
            project_structure.markdown_files.len(),
            md_wall.elapsed(),
            std::time::Duration::from_nanos(md_read_ns.load(Ordering::Relaxed)),
            std::time::Duration::from_nanos(md_resolve_ns.load(Ordering::Relaxed)),
            std::time::Duration::from_nanos(md_process_ns.load(Ordering::Relaxed)));
        if markdown_total > 0 {
            // Final completed event clears the "Rendering markdown" row.
            // Without this, progress-panel's stale timeout would hold the
            // task visible for 15s after the phase actually finished.
            reporter.report(&crate::build::progress::PipelineEvent::BackgroundProgress {
                task: "markdown".to_string(),
                current: markdown_total,
                total: markdown_total,
                message: crate::infra::app_advisory::t("markdown_rendered"),
                completed: true,
                advisories: vec![],
            });
        }
        // Sequential post-pass: apply the deferred uid write-backs now that the
        // parallel reads are done (no torn reads), and collect docs in order.
        let mut docs = Vec::with_capacity(rendered.len());
        for (doc, uid_writeback, mut source_meta) in rendered.into_iter().flatten() {
            if let Some((path, updated)) = uid_writeback {
                // Best-effort; don't fail the build on a source write error.
                let _ = fs::write(&path, &updated);  // allow:raw_write the user's own markdown source (uid write-back), not build output
                // moss just rewrote this file's frontmatter. Record the bytes
                // that are now on disk, not the ones we parsed — otherwise the
                // NEXT build reads a hash it never wrote and reports the
                // author's page as edited when only moss touched it.
                if let Some(meta) = source_meta.as_mut() {
                    *meta = source_metadata(&path, updated.as_bytes());
                }
            }
            if let (Some(meta), Some(src)) = (source_meta, doc.source_path.as_ref()) {
                page_source_hashes.insert(src.clone(), meta);
            }
            crate::build::scan::slug::push_unless_reserved_device_output(&mut docs, doc);
        }
        (
            docs,
            deferred_mutex.into_inner().unwrap(),
            missing_media_mutex.into_inner().unwrap(),
        )
    };

    // THE cache-snapshot boundary. `documents` here is Loop
    // A's per-file output and nothing else — the next statement begins the
    // whole-corpus Reduce chain (uid dedup, slug dedup, cascade, children
    // sorts, marker expansion, translation linking), every pass of which folds
    // OTHER pages' state into a document. Storing a post-Reduce document would
    // replay another page's inherited cascade on a later build; storing it
    // here cannot. Do not move this call down.
    parse_session.finish(&documents);

    send_progress(
        progress_sender,
        "generating",
        &crate::infra::app_advisory::t("processing_markdown"),
        55,
        false,
        None,
        None,
    );

    // Profile the whole-corpus Reduce chain, starting here
    // (uid_dedup is the first pass after Loop A's cache-snapshot boundary).
    let reduce_pre_start = std::time::Instant::now();
    // Detect and fix duplicate UIDs — before slug resolution so each article
    // gets its own URL slot. Duplicating a note in Obsidian copies the
    // frontmatter uid, so collisions are routine; one file has to be given a
    // fresh uid. WHICH one is the whole problem: a uid is random (not derivable
    // from the path) and is the join key for the live comment thread and every
    // signed moderation event, so rewriting the wrong file's uid orphans a
    // published note irreversibly. `uid_dedup` therefore consults the record
    // of what is live first and only falls back to date/btime — both of which
    // iCloud and Obsidian Sync reset — when nothing is live. When that record
    // cannot be READ, it defers instead of falling back.
    {
        let resolution = super::uid_dedup::resolve_duplicate_uids(
            &mut documents,
            source_path_buf,
            || crate::build::manifest::live_baseline::load(&paths),
            |src| previous_hashes.sources.contains_key(src),
        );
        // A deferral is not shown. `resolve_duplicate_uids` logs it; the author
        // hears nothing, because a note ID is moss's to manage and every remedy
        // she could be offered is one moss should have taken itself. What is
        // left after the never-built rule is a case she cannot diagnose and the
        // next publish ends (2026-08-30).
        let reassignments = resolution.reassignments;
        if !reassignments.is_empty() {
            for r in &reassignments {
                let line = format!(
                    "Duplicate note ID '{}': '{}' keeps it, '{}' was given a new one{}",
                    r.uid,
                    r.keeper_path,
                    r.reassigned_path,
                    if r.live_thread_at_risk {
                        " — a LIVE deployment exists under this ID and moss could not \
                         identify the published file; its comments may follow the wrong page"
                    } else {
                        ""
                    }
                );
                log::warn!("{}", line);
                cli_warn!("[warn] {}", line);
            }
            // The app surface, for the at-risk subset only — moss may have sent
            // a live page's comments to the wrong file, and only she can tell.
            if let Some(event) =
                crate::build::progress::make_duplicate_uid_advisory(&reassignments)
            {
                reporter.report(&event);
            }
        }
    }
    log::debug!(target: "timing", "[reduce] uid_dedup: {:?}", reduce_pre_start.elapsed());

    // Empty-folder onboarding:
    // when zero markdown files were discovered, push a synthetic homepage
    // BEFORE the layout-config / sitemap / llms.txt blocks so every
    // downstream `documents`-keyed consumer sees the empty-folder homepage:
    //   - LayoutConfig::new picks up `homepage_title` (~L897)
    //   - homepage_for_analytics filter (~L965)
    //   - sitemap entry generation (~L1767)
    //   - llms.txt site-title fallback (~L1814)
    //   - index.html homepage dispatch (~L1876)
    // `synthesize_empty_homepage` returns a `ParsedDocument` whose
    // `url_path == "index.html"`, `kind == Folder`, and `html_content`
    // carries an empty `html_content` (no injected h1 or placeholder body —
    // Drop-in for the cascade / slug-dedup / translation-links transforms
    // immediately below: each one is a no-op or trivially-correct for a
    // single self-contained synthetic doc with no source_path.
    //
    // Hoisted here (rather than just-before-the-homepage-render) on the
    // architecture-review feedback for T3: a *deployed* empty site would
    // otherwise emit a sitemap with zero entries and an llms.txt titled
    // "site" (i18n fallback) instead of the folder name.
    if documents.is_empty() {
        let folder_name = root
            .name_opt()
            .unwrap_or_else(|| crate::i18n::t(site_lang, "site"));
        let doc = super::empty_home::synthesize_empty_homepage(folder_name, site_lang);
        documents.push(doc);
    }

    // Resolve slugs, build translation links, layout config.
    // `site_lang` was computed eagerly before the markdown loop (above) so that
    // per-document language resolution and slug deduplication agree on the same
    // site default. Reuse it here.
    // Out here because the article map, written far below, carries these to the editor's `url` chip.
    let mut url_collisions: Vec<crate::build::scan::slug::UrlCollision> = Vec::new();
    let (layout_config, site_url, has_rss, show_rss_in_footer, analytics_script, term_index) = {
        // Profile the whole-corpus Reduce chain. Each pass
        // below folds OTHER pages' state into `documents`, so (unlike Loop A)
        // none of it is skippable by the parse cache — this is the floor on
        // a warm content-only rebuild once Loop A is a cache hit.
        let reduce_start = std::time::Instant::now();
        url_collisions = resolve_duplicate_slugs_with_lang(&mut documents, site_lang);
        // Sync permalink with updated url_path (dedup may have changed it)
        for doc in &mut documents {
            doc.permalink = format!("/{}", doc.url_path); // allow:served-path-url-construct (page permalink synced after slug dedup, stored in ParsedDocument)
        }
        log::debug!(target: "timing", "[reduce] slug_dedup ({} docs): {:?}", documents.len(), reduce_start.elapsed());

        // Slot files (reserved-name `footer.md`) STAY in
        // `documents` so the per-page emission loop can see them — they're
        // structurally flagged via `ParsedDocument.slot_only` and the HTML
        // loop below short-circuits on that flag (`if doc.slot_only { continue }`).
        // Keeping the doc in the slice lets the post-build native-slot
        // collection (`pipeline::run` → `generate_native_slots` →
        // `collect_footer_slots_by_language`) reach `footer.md`'s parsed HTML through
        // the SAME data path every other page uses, deleting the
        // filesystem-rescan-and-reparse hack (`render_footer_pages_from_disk`).
        // The previous `retain` was filename-equality-gated (see
        // `is_excluded_from_pages`) so removing it doesn't widen the set of
        // excluded files — the HTML loop's `slot_only` gate enforces the
        // exact same exclusion.
        //
        // History: an earlier design introduced
        // the retain; this change replaces it with the structural flag.

        // Apply frontmatter cascade: parent pages push values to descendants.
        // Drafts now flow through cascade too, so cascade-defined sort/nav/layout
        // settings apply to them uniformly (a draft is rendered, just hidden
        // from listings).
        apply_cascade(&mut documents);
        log::debug!(target: "timing", "[reduce] apply_cascade: {:?}", reduce_start.elapsed());

        // Populate `direct_children_sort` on every folder index. Must run AFTER
        // `apply_cascade` so that cascaded `sort:` values are visible to the
        // inference, and BEFORE any rendering that reads `direct_children_sort`
        // (listing card renderer, series-nav, editor inferred-axis hint).
        populate_direct_children_sorts(&mut documents);
        log::debug!(target: "timing", "[reduce] populate_direct_children_sorts: {:?}", reduce_start.elapsed());

        // Term derivation:
        // every declared kind's fields become membership claims in term
        // pseudo-folders (`authors/<slug>`, `tags/<slug>`, or a declared
        // kind's own namespace) via the `also_in` slot, and their `*_page:`
        // claims resolve into `term_listing`. Must run before the marker
        // expansion just below: a body embed of a pseudo-folder term page
        // (`![[/places/kyoto/]]`, including an ancestor reached only through
        // `also_in` roll-up) is resolved in `folder_embed.rs`'s pseudo-folder
        // branch by matching the embed's target against each doc's
        // `also_in` — which is what this pass fills in. Resolving markers
        // first (the previous order) left `also_in` empty for every doc at
        // that point, so any such embed rendered a "Folder not found" div;
        // a real-folder embed (e.g. `/people/`) only ever worked because it
        // never needed `also_in` in the first place. Also runs before the
        // synthetic-index blocks further down (which seed the unclaimed term
        // keys) and before the incremental verdict (so listing digests see
        // the derived membership).
        let term_index = crate::build::terms::derive_terms(&mut documents, site_config.term_kinds.clone());
        // Design rule 3, byline surface: a name-list field's value that the
        // author also typed in a `byline:` row becomes a markdown link to
        // its term page. Before the verdict for the same reason as above —
        // the linked URL is part of each page's surface, so a claim
        // appearing elsewhere re-renders the bylines that point at it.
        crate::build::terms::link_terms_in_bylines(&mut documents, &term_index);
        // The automatic place line — same neighborhood, same reasoning:
        // before the incremental content hash is taken, so a claim moving
        // invalidates the line's link for free.
        crate::build::terms::set_place_lines(&mut documents, &term_index, site_lang);
        log::debug!(target: "timing", "[reduce] derive_terms: {:?}", reduce_start.elapsed());

        // Resolve folder-list embed markers (`![[/folder/|limit:N,more]]`).
        // moss-core emits a `<!--MOSS_MARKER_FOLDER_LIST:…-->` marker during
        // wikilink resolution; we expand it here, AFTER
        // `populate_direct_children_sorts` so the target folder's resolved axis
        // is available for inheritance, AFTER term derivation so a pseudo-folder
        // term embed's `also_in` is already populated, and BEFORE the HTML
        // render phase (which would otherwise emit the marker comment
        // unchanged into the page).
        crate::build::folder_embed::expand_markers_in_documents(
            &mut documents,
            project_structure,
            &dir_overrides,
            site_config.math,
            &event_level_image_lookup,
            site_config.typesetting.as_deref(),
        );
        log::debug!(target: "timing", "[reduce] expand_markers_in_documents: {:?}", reduce_start.elapsed());

        // Inline `:::subscribe` forms get their page's language scope, judged
        // the same way the footer form's is, now that every page has parsed.
        crate::build::features::email::stamp_inline_subscribe_scopes(
            &mut documents,
            site_id.as_deref(),
            &seta_url_for_build,
        );

        super::lang_roots::fill_missing_lang_tags(&mut documents, &site_config.lang);

        // Build translation links between documents sharing the same stem or translationKey
        {
            use crate::i18n::link::{build_translation_links, DocumentInfo};
            let doc_infos: Vec<DocumentInfo> = documents
                .iter()
                .enumerate()
                .map(|(i, doc)| {
                    // Use source_path (original file path) for directory grouping,
                    // not url_path, because lang-prefix directories (e.g., zh-hans/)
                    // would prevent translations from being grouped together.
                    let directory = doc.source_path.as_ref()
                        .and_then(|sp| std::path::Path::new(sp).parent())
                        .and_then(|p| p.to_str())
                        .map(|s| s.to_string())
                        .unwrap_or_default();
                    DocumentInfo {
                        index: i,
                        clean_stem: doc.clean_stem.clone(),
                        directory,
                        // The tag the doc's OWN page emits, so siblings advertise the
                        // same string back. Filled above; the fallback is only so a
                        // missed fill degrades to the site tag, not `hreflang=""`.
                        lang_tag: doc.lang_tag.clone().unwrap_or_else(|| crate::i18n::lang_tag(&site_config.lang)),
                        url_path: doc.url_path.clone(),
                        translation_key: doc.translation_key.clone(),
                    }
                })
                .collect();
            let all_links = build_translation_links(&doc_infos);
            for (i, links) in all_links.into_iter().enumerate() {
                documents[i].translations = links;
            }
        }
        log::debug!(target: "timing", "[reduce] translation_links: {:?}", reduce_start.elapsed());
        log::debug!(target: "timing", "[reduce] total (uid_dedup..translation_links): {:?}", reduce_pre_start.elapsed());

        // Create layout configuration.
        // Site name is one STRUCTURAL decision — `moss_core::home::site_name`
        // — shared with the bundled-SPA og:title path below so `<title>` and
        // `og:title` always agree. It keys off the home FILENAME stem (index /
        // readme / self-named → folder name), NOT the title VALUE, so a page
        // genuinely titled "Index" is no longer wrongly suppressed.
        let folder_name = root
            .name_opt()
            .unwrap_or_else(|| crate::i18n::t(site_lang, "site"));

        let homepage_title = documents
            .iter()
            .find(|d| d.url_path == "index.html")
            .map(|d| d.title.as_str());

        let resolved_site_name = moss_core::home::site_name(
            project_structure.homepage_file.as_deref(),
            homepage_title,
            folder_name,
        );
        let layout_config = LayoutConfig::new(folder_name, Some(resolved_site_name.as_str()))
            .with_lang_tag(crate::i18n::lang_tag(&site_config.lang));

        // Extract logo from homepage frontmatter (e.g., `logo: assets/logo.svg`)
        let homepage_logo = documents
            .iter()
            .find(|d| d.url_path == "index.html")
            .and_then(|d| d.logo.as_ref())
            .map(|logo| resolve_logo_url(logo));
        let layout_config = match homepage_logo {
            Some(logo) => layout_config.with_logo(logo),
            None => layout_config,
        };
        // Apply site-level config to LayoutConfig
        let layout_config = match site_config.typesetting {
            Some(ref ts) => layout_config.with_typesetting(ts.clone()),
            None => layout_config,
        };
        let layout_config = match site_config.content_width {
            Some(ref cw) => layout_config.with_content_width(cw.clone()),
            None => layout_config,
        };
        let layout_config = match site_config.comments {
            Some(c) => layout_config.with_comments(c),
            None => layout_config,
        };
        // [site].math (default true) rides on LayoutConfig so the per-page
        // render path (generate_html) can gate the math-copy script tag at
        // the site level without widening its signature.
        let layout_config = layout_config.with_math(site_config.math);
        // [site].link_preview / [site].heading_anchors (both default true),
        // same reason as math: the per-page render path gates the preview.js
        // and heading-anchor.js script tags off LayoutConfig, not SiteConfig.
        let layout_config = layout_config.with_link_preview(site_config.link_preview);
        let layout_config = layout_config.with_heading_anchors(site_config.heading_anchors);
        // [site].floating_nav (default false — opt-in) rides along for
        // the same reason: the emitter reads LayoutConfig, not SiteConfig.
        let layout_config = layout_config.with_floating_nav(site_config.floating_nav);

        // Resolve the canonical site URL for this build. Resolution lives in
        // site_url::resolve_for_project — the ONE path, shared with the
        // deploy guard so build and publish can never disagree about what URL
        // a generation embeds. Priority chain (domain, site_url, site_id, …) is
        // documented on resolve_site_url in site_url.rs.
        let site_url = crate::build::site_url::resolve_for_project(
            source_path,
            site_config.site_url_override.as_deref(),
        )?;

        // RSS feed: always generated when a real (https) URL is resolved.
        // Localhost fallbacks (no deployment configured) skip RSS/sitemap.
        // Footer RSS link: additionally controlled by [features].rss_footer toggle.
        let has_rss = site_url.is_deployed();
        // Search does NOT ride RSS's `is_deployed()` gate, and deliberately no
        // longer rides any mode bit: build output is mode-independent
        // by design, so preview and publish index alike, and the latency that
        // justified excluding preview is fixed at the source (indexing is a
        // background-phase worker). `site_config.search` is `[site].search`
        // as resolved in pipeline.rs, and stays the ONE value button and
        // emitter read.
        let has_search = site_config.search;
        let layout_config = layout_config.with_search(has_search);
        let show_rss_in_footer = crate::build::site_config::get_site_rss_footer(source_path)
            .unwrap_or(None)
            .unwrap_or(false);

        // Generate analytics script tag from homepage frontmatter (if configured)
        let homepage_for_analytics = documents.iter().find(|d| d.url_path == "index.html");
        let analytics_script = homepage_for_analytics
            .and_then(|d| d.analytics.as_ref())
            .map(|analytics| analytics.to_script_tag());

        (
            layout_config,
            site_url,
            has_rss,
            show_rss_in_footer,
            analytics_script,
            term_index,
        )
    };

    // Record what this build actually resolved — the deploy guard compares
    // it against a fresh resolution and rebuilds when they disagree.
    pending.set_site_url(site_url.as_str());

    // Compute CSS hash for cache busting BEFORE HTML generation
    // This ensures the preview iframe loads fresh CSS/JS after rebuilds
    //
    // Task 1.3: Prepend the generated @layer order declaration and @layer tokens
    // block (from tokens.json) before minifying. site.css now contains only
    // rule blocks (var(--moss-*) references); token *definitions* live here.
    //
    // Which optional partials this build ships is a site-level fact — the
    // union of every page's parse-time features with the resolved config.
    // `site_assets` is NOT what invalidates the Stage 5b render skip: it
    // changes the sheet's *contents*, and `css_version` — a hash of those
    // contents, folded into `asset_versions` below — is what the skip reads.
    // See the note on `SiteAssets` itself.
    //
    // Whether this build will produce media collection pages. Derived here,
    // from the same pure fold the media loop uses 1,700 lines below, because
    // `fullscreen.js` is gated on it and gates must be known before assets are
    // hashed.
    //
    // This also fixes a shipped bug. The loop used to set a `media_pages_generated`
    // flag as it went and read it for the script tag *inside the same loop*, so
    // the FIRST media collection page always rendered with no `fullscreen.js`
    // — its lightbox silently did nothing. A fact folded from `documents` up
    // front cannot depend on iteration order.
    let media_pages = !crate::build::media_collection::aggregate_media_collections(&documents)
        .iter()
        .all(|(_, items)| items.is_empty());

    let site_assets = crate::build::types::SiteAssets::of(
        &documents,
        layout_config.typesetting.as_deref(),
        crate::build::types::SiteAssets {
            media_pages,
            video_ladder: services
                .and_then(|s| s.assets.as_deref())
                .is_some_and(|r| r.iter_registered_variants().values().any(|k| k.hls)),
            // `search`, `link_preview`, `heading_anchors` and `math` already
            // live here, written by the config phase above. Re-reading them
            // from `site_config` would be a second copy of the same four
            // facts, which is how the tag gate and the CSS gate came to be
            // able to disagree.
            ..layout_config.assets
        },
    );
    let layout_config = layout_config.with_assets(site_assets);
    let css_content = crate::build::emit::stylesheet::assemble(&site_assets);
    let css_version = compute_content_hash(&css_content);
    // Every runtime script's bytes + content hash, from the one registration
    // table that also decides which get written. See `build::emit::scripts`.
    let scripts = crate::build::emit::scripts::ScriptAssets::resolve();
    let js_version = scripts.hash("theme").to_string();
    let share_card_hash = scripts.hash("share-card").to_string();
    let hls_hash = scripts.hash("hls").to_string();
    // `None` keeps `data-hls` off every page of a site with no ladder, which is
    // what keeps those sites byte-identical. See `lazy_chunk_attrs`.
    let hls_attr_hash: Option<&str> = site_assets.video_ladder.then_some(hls_hash.as_str());
    let fullscreen_hash = scripts.hash("fullscreen").to_string();


    // Warn when user placed style.css / script.js at the project root instead
    // of the canonical `.moss/theme/` directory. Silently ignoring these files
    // cost a dogfood user ~3 hours of debugging on a real site — surface it.
    for warning in check_misplaced_theme_files(source_path_buf) {
        log::warn!("{}", warning);
        cli_warn!("[warn] {}", warning);
    }

    // Warn when a language uses BOTH folder-per-language (zh-hans/index.md)
    // AND sibling suffix files (index.zh-hans.md). Folder-per-language is
    // canonical; mixing is ambiguous. We don't forbid either style — just
    // nudge authors toward consistency.
    {
        let md_paths: Vec<String> = project_structure
            .markdown_files
            .iter()
            .map(|f| f.path.clone())
            .collect();
        for warning in check_mixed_multilingual_structure(&md_paths) {
            log::warn!("{}", warning);
            cli_warn!("[warn] {}", warning);
        }
    }

    // Check for user custom CSS and compute its hash for cache busting.
    // Canonical location: .moss/theme/style.css. Files at the project root are
    // ignored; see check_misplaced_theme_files() above for the user-facing warning.
    // The file is copied to the output directory later with CSS/JS assets.
    let user_css_path = {
        let theme_css = source_path_buf.join(".moss").join("theme").join("style.css");
        if theme_css.exists() { Some(theme_css) } else { None }
    };
    // The read, not the stat, decides whether this build has a user theme: a
    // cloud-evicted style.css stats as present but reads as absent, and linking
    // to a stylesheet this build never emits would 404 every page. Unstyled for
    // one build, restyled when the download lands.
    let user_css_content = match user_css_path {
        Some(ref css_path) => cloud_readiness::read_optional_build_input(css_path, "user style.css")?,
        None => None,
    };
    let has_user_css = user_css_content.is_some();
    let user_css_version = user_css_content.as_ref().map(|content| {
        // Phase 1 PR-1d: warn when theme references pre-v1 (Retired) class
        // vocabulary. Non-fatal; one warning per retired class found.
        for warning in crate::build::theme_lint::lint_theme_css(content) {
            log::warn!("{}", warning);
            cli_warn!("[warn] {}", warning);
        }
        compute_content_hash(content)
    });
    // Registered the same way a page's bytes are (`register_page_source`,
    // below) so `watch::compute_source_change_set`'s diff can name an
    // in-place style.css edit in `modified_paths` — see
    // `manifest::is_reload_tracked_source_key`. This read already happened
    // above for `user_css_version`; hashing it again here (rather than
    // reusing that hash) keeps `SourceMetadata`'s mtime/inode fields honest,
    // the same reason `source_metadata` takes `bytes` separately from `path`.
    if let (Some(css_path), Some(content)) = (user_css_path.as_ref(), user_css_content.as_ref()) {
        pending.register_page_source_hash(
            crate::build::manifest::USER_CSS_SOURCE_KEY.to_string(),
            source_metadata(css_path, content.as_bytes()),
        );
    }

    // Check for user custom JS and compute its hash for cache busting.
    // Canonical location: .moss/theme/script.js. Files at the project root are
    // ignored; see check_misplaced_theme_files() above for the user-facing warning.
    let user_js_path = {
        let theme_js = source_path_buf.join(".moss").join("theme").join("script.js");
        if theme_js.exists() { Some(theme_js) } else { None }
    };
    let user_js_content = match user_js_path {
        Some(ref js_path) => cloud_readiness::read_optional_build_input(js_path, "user script.js")?,
        None => None,
    };
    let has_user_js = user_js_content.is_some();
    let user_js_version = user_js_content.as_ref().map(|c| compute_content_hash(c));
    if let (Some(js_path), Some(content)) = (user_js_path.as_ref(), user_js_content.as_ref()) {
        pending.register_page_source_hash(
            crate::build::manifest::USER_JS_SOURCE_KEY.to_string(),
            source_metadata(js_path, content.as_bytes()),
        );
    }

    // `.moss/config.toml`: read fresh here (the many small readers elsewhere
    // in this crate each pull one field; this is the one place the whole
    // file's bytes are hashed) and registered the same way the theme files
    // above are, so an in-place edit — the user's own, or moss's settings UI
    // via `infra::toml_rewrite` — reaches `modified_paths` too. See
    // `manifest::is_reload_tracked_source_key`.
    //
    // `read_managed_toml` (not the `user_css_path.exists()` pattern above):
    // it proves absence from the error rather than from a stat, which is the
    // only correct answer under iCloud eviction (see its doc comment). A
    // read error that is NOT "genuinely absent" (permission denied, a
    // transient I/O fault) must not fail this registration — the rest of the
    // crate already tolerates an unreadable config.toml and still publishes
    // (`an_unreadable_config_toml_still_lets_the_build_publish`); this is
    // one more source of that same field, not a new one, so it degrades the
    // same way: skip the registration, keep building.
    let config_toml_path = source_path_buf.join(".moss").join("config.toml");
    match crate::build::site_config::read_managed_toml(&config_toml_path) {
        Ok(Some(content)) => pending.register_page_source_hash(
            crate::build::manifest::CONFIG_TOML_SOURCE_KEY.to_string(),
            source_metadata(&config_toml_path, content.as_bytes()),
        ),
        Ok(None) => {}
        Err(e) => log::warn!("[modified_paths] config.toml unreadable, not tracked this build: {e}"),
    }

    // Every content-addressed asset whose HASHED FILENAME appears in emitted
    // HTML, folded into one build-global key. A page that the Stage 5b skip
    // carries forward keeps its old `<link href>`/`<script src>`, so if any of
    // these moved, that page now points at a file this build deleted.
    // See `FacadeCache::asset_versions` — this is the value it stores.
    //
    // The user theme's two hashes are in here even though a `.moss/theme/`
    // edit is never a `ContentOnly` markdown trigger today, so the skip cannot
    // currently be live when they move. Relying on that is the exact implicit
    // coupling this key exists to delete.
    let asset_versions = {
        // Script hashes come from the registration table, not a hand-written
        // list: a script added to `SITE_SCRIPTS` joins this key automatically.
        // A list maintained by hand is a list that silently loses an entry,
        // and a lost entry is a page pointing at a deleted file.
        let mut parts: Vec<&str> = vec![css_version.as_str()];
        parts.extend(scripts.all_hashes());
        // Feature sheets (`comments.css` et al.) resolve a phase later, in
        // `generate_native_slots`, so which ones this build LINKS is not known
        // here. Their bytes are, and that is the half this key can cover: the
        // hash is what the filename is made of, and a skipped page's already-
        // injected `<link href>` is never revisited by `inject_slots`.
        //
        // The other half — a sheet joining or leaving the linked set — is
        // closed elsewhere, and it is worth naming exactly where, because the
        // obvious answer is wrong. It is NOT "a set change is a config change,
        // and config changes aren't `ContentOnly`": adding a `:::subscribe` to
        // one markdown file is a `ContentOnly` trigger that adds `email.css`
        // site-wide. What closes it is that `inline_subscribe`/`inline_apply`
        // live in `PageFeatures`, which the facade's SURFACE fingerprint
        // covers — so that edit trips `surface_changed` and forces a full
        // render. Narrowing the surface fingerprint would reopen this.
        let feature_style_hashes = crate::build::emit::feature_styles::all_hashes();
        parts.extend(feature_style_hashes.iter().map(String::as_str));
        parts.push(user_css_version.as_deref().unwrap_or(""));
        parts.push(user_js_version.as_deref().unwrap_or(""));
        crate::build::facade::debug_hash(&parts)
    };


    // Compute root-relative aux-JS paths (used in ShellVars for article/folder pages).
    // These paths are constant for the build (content-hash is deterministic).
    // Built from a temporary PathResolver to leverage the same URL construction logic.
    let aux_path_resolver = PathResolver::new();
    // The shell's whole runtime `<script>` block, from the SITE_SCRIPTS
    // table: order, `defer` and gate all live on the row, so a seventh script
    // is one row rather than a tag `let` here plus a `ShellVars` field plus
    // a `shell.rs` mapping.
    let runtime_js_tags = scripts.shell_tags(&layout_config.assets, &aux_path_resolver);
    // Compute site-wide sidebar layout flag: true when ANY document has `sidebar` set.
    // Used by templates to adjust content width (Task 3).
    let has_sidebar_layout = documents.iter().any(|d| d.sidebar.is_some());

    // Reverse map (slug folder-key → original on-disk leaf name) so every
    // auto-generated folder-index TITLE/H1 preserves the author's directory
    // casing ("Writings") even though the URL slug is lowercased ("writings").
    // Consumed by both folder-index synthesis blocks below.
    let folder_display = folder_display_leaves(&project_structure.dirs, &dir_overrides);

    // Term derivation already ran above, before `expand_markers_in_documents`
    // — see that call site for why. `term_index` is in scope from here on;
    // both synthetic-index loops below consult it for term-page titles.
    // URL keys of every index page the auto-index loop below synthesizes, for
    // `ArticleMap::generated`: the editor's URL index has no other way to learn
    // those pages exist (no source document).
    let mut generated_index_urls: Vec<String> = Vec::new();

    // Synthesize folder index entries for folders that have child documents but
    // no explicit index file. This completes the page tree so parent folder pages
    // can discover auto-generated subfolders as direct children during HTML rendering.
    {
        use std::collections::HashSet;

        // Collect all folder prefixes that have child documents.
        let mut folders_with_children: HashSet<String> = HashSet::new();
        for doc in &documents {
            if doc.url_path == "index.html" {
                continue;
            }
            // Slot files are layout chrome, not content
            // pages; they must not trigger synthetic folder-index creation.
            if doc.slot_only {
                continue;
            }
            let parts: Vec<&str> = doc.url_path.split('/').collect();
            if parts.len() >= 2 {
                let doc_dir_depth = parts.len() - 1;
                for depth in 1..doc_dir_depth {
                    let folder = parts[..depth].join("/");
                    folders_with_children.insert(folder);
                }
            }
        }

        // Seed every index-page directory so folders with no doc footprint at
        // all (completely empty folders, or folders holding only non-page
        // assets) still get a synthetic folder-index document — and therefore
        // appear as children of their parent and in nav/breadcrumb. A childless
        // folder is never an ancestor prefix of any doc's `url_path`, so the
        // loop above alone would miss it.
        for (_, key) in folder_index_keys(&project_structure.dirs, &dir_overrides) {
            folders_with_children.insert(key);
        }

        // Seed every term pseudo-folder — unclaimed terms and the namespace
        // roots in use — so each gets a synthetic folder document (page tree
        // entry, breadcrumb ancestor) and, below, its index page. The roots
        // are documents like any other synthetic folder, flagged `nav: false,
        // listed: false` at creation: a 56-entry author folder is wrong as a
        // nav item or a home-feed entry on every site, and the pages stay
        // reachable through term links, the breadcrumb and search.
        // (Until 2026-09-05 the roots were left out of `documents` entirely,
        // and every breadcrumb under them fell back to the current page's
        // own title: `Site › 林小滿 › 林小滿`.)
        folders_with_children.extend(term_index.synthetic_folder_keys());

        // Filter out lang-prefix directories (e.g., "zh-hans") — structural, not content.
        folders_with_children.retain(|folder| {
            let top_segment = folder.split('/').next().unwrap_or(folder);
            crate::i18n::path::resolve_language_from_folder(top_segment).is_none()
        });

        // Identify folders that already have an explicit index page (from a real .md file).
        let folders_with_explicit_index: HashSet<String> = documents
            .iter()
            .filter(|d| d.url_path.ends_with("/index.html") && d.url_path != "index.html")
            .map(|d| d.url_path.trim_end_matches("/index.html").to_string())
            .collect();

        // Create synthetic ParsedDocument entries for indexless folders.
        for folder in &folders_with_children {
            if folders_with_explicit_index.contains(folder) {
                continue;
            }

            documents.push(synthetic_folder_doc(
                folder,
                &term_index,
                &folder_display,
                &folder_languages,
                site_lang,
            ));
        }
    }

    // The incremental render verdict.
    //
    // The computation itself lives in `render/incremental/verdict.rs`: it was
    // produced here, logged here, and consumed by exactly one `partition`
    // fifty lines below, so nothing else in the build could learn it.
    // `RenderVerdict` is a typed value with one owner
    // and no public constructor from a raw set.
    let verdict = crate::build::render::incremental::verdict::compute(
        &documents,
        &crate::build::render::incremental::verdict::VerdictInputs {
            policy: crate::build::render::incremental::IncrementalPolicy::resolve(&site_config),
            project: project_structure,
            cache_path: &paths.cache_dep_graph(),
            asset_versions: &asset_versions,
            dir_overrides: &dir_overrides,
            site_lang,
            typesetting: layout_config.typesetting.as_deref(),
            math: site_config.math,
        },
    );
    // The carry machinery's own price, and on a real vault the largest single
    // step of a one-page-edit rebuild: 215ms of a 509ms call on a 223-page
    // site with a deep tree, against 13ms of 388ms on a bench vault of 226
    // tiny posts in one flat folder. It fingerprints every document, loads and
    // re-saves the facade cache, and builds the dep graph — all sized by the
    // vault, not by the edit. The partition it feeds costs 1.0ms and the one
    // page it spares costs 3.5ms. Medians of 7 rebuilds.
    log::debug!(target: "timing", "[render] verdict computed: {:?}", total_start.elapsed());

    // The render-prelude cut: the literal homepage (`index.html`)
    // is excluded from the to_render/to_carry partition below (it is looked
    // up via `documents.iter().find`, not iterated) and used to be rendered
    // unconditionally every build — ~230-260ms on the 226-page reference
    // vault, the dominant single cost of the per-build prelude, because
    // `generate_html_collect_og(.., is_homepage: true)` runs `folder_embed`'s
    // full card synthesis (title/excerpt/cover/dimensions per child) over
    // every listed document regardless of what changed.
    //
    // The listing-group model already models the root homepage as listing-group host (a) in
    // `listing::groups_read_by`, so `verdict.may_skip` already proves — via
    // the same per-page facade diff plus listing-group digest every other
    // carried page relies on — whether the homepage's own content AND every
    // field its card listing reads (title, excerpt, cover, sort order, ...)
    // are unchanged. Computed here, before the render loop, so the OG-card
    // carry-forward gate below (keyed on "was anything carried this build")
    // can see it too.
    let homepage_index_sp = ServedPath::from_source("index.html").unwrap();
    let homepage_carried = documents
        .iter()
        .find(|d| d.url_path == "index.html")
        .and_then(|d| d.source_path.as_ref())
        .is_some_and(|src| {
            verdict.may_skip(src)
                && previous_hashes.files.contains_key(homepage_index_sp.as_str())
                && crate::build::io_utils::output_present(&output_dir.join("index.html"))
        });

    // Generate HTML files
    //
    // The completion `BackgroundProgress { completed: true }` event is
    // emitted on every exit path (success OR fs error) so the progress
    // panel doesn't leave the "Rendering pages" row visible for its 15s
    // stale timeout on a mid-loop error. Implemented by capturing the
    // loop result, emitting completion, then propagating with `?`.
    let html_total = documents.len() as u32;
    const HTML_PROGRESS_STRIDE: usize = 5;
    // Shadow verification, opt-in via
    // `MOSS_INCREMENTAL_VERIFY=1`. Only the SNAPSHOT is taken here — the
    // comparison happens in `build::pipeline` after the enhance hook, because
    // what this render phase produces is pre-slot-injection and the bytes on
    // disk are the previous build's post-injection output. See
    // `incremental::carry_verify`.
    let mut carry_verification = crate::build::render::incremental::CarryVerification::start();
    let mut page_count = {
        use rayon::prelude::*;
        use std::sync::atomic::{AtomicUsize, Ordering};

        // Durable timing, all builds.
        let _phase_html_pages = PhaseTrace::start("render_html_pages");

        // A page rendered in phase 1 (parallel). Manifest registration is
        // deferred to phase 2 (sequential) so the `&mut PendingManifest` is
        // never shared across threads. Rayon's indexed `collect` preserves
        // `documents` order, so phase-2 registration runs in the SAME order as
        // the old single-threaded loop — the manifest's HashMap/HashSet
        // internals never observe a different insertion order, and the sealed
        // `hashes.json` is byte-identical. HTML generation (the ~8s cost on a
        // large image site) is embarrassingly parallel: each page writes its
        // own HTML + OG PNGs to distinct paths and reads only immutable inputs.
        struct RenderedPage {
            url_sp: ServedPath,
            html_bytes: Vec<u8>,
            og_cards: Vec<crate::build::page::og_card::CardOutput>,
            source_mapping: Option<(String, ServedPath)>,
            /// This page's title/date, registered into `SiteHashes::page_meta`
            /// alongside `source_mapping` (same `Some` condition — both come
            /// from `doc.source_path.is_some()`). See that field's doc for why.
            page_meta: Option<crate::types::content::PageMeta>,
            /// `(url_path, previous build's final bytes)` — populated only
            /// under `MOSS_INCREMENTAL_VERIFY=1`, for pages this build would
            /// have carried. Read before the re-render overwrites the file.
            carried_previous: Option<(String, Vec<u8>)>,
        }

        // Same emission gates as the old loop: skip the homepage (rendered
        // separately below with is_homepage: true), synthetic folder indexes
        // (source_path == None — rendered by the auto-generate loop below; they
        // live in `documents` only so parent pages discover them as children),
        // and slot-only files (footer.md etc. parse for their HTML
        // but emit no standalone page).
        //
        // Of those that DO emit, the `RenderVerdict` spares the ones nothing can have changed. Two
        // disqualifiers on top of the verdict make page loss structurally
        // impossible rather than merely unlikely: the page must still be on
        // disk in the persistent stage dir, and the previous build's manifest
        // must still carry its entry. Missing either, we render — the skip has
        // to PROVE it is safe, not assume it.
        let (mut to_render, mut to_carry): (Vec<&ParsedDocument>, Vec<&ParsedDocument>) = documents
            .iter()
            .filter(|doc| {
                !(doc.url_path == "index.html"
                    || (doc.kind == PageKind::Folder && doc.source_path.is_none())
                    || doc.slot_only)
            })
            .partition(|doc| {
                let skippable = doc
                    .source_path
                    .as_ref()
                    .is_some_and(|src| verdict.may_skip(src))
                    && ServedPath::from_source(&doc.url_path).is_ok_and(|sp| {
                        previous_hashes.files.contains_key(sp.as_str())
                    })
                    && crate::build::io_utils::output_present(&output_dir.join(&doc.url_path));
                !skippable
            });

        // Shadow verification. Move the carried set into
        // the render set and remember which pages those were; each one snapshots
        // the previous build's final bytes on its way through the loop below,
        // and `pipeline` compares them once slot injection has produced this
        // build's final bytes. Deliberately opt-in: it costs a full render,
        // which is precisely what the verdict exists to avoid.
        let verify_shadow: std::collections::HashSet<String> = if carry_verification.is_some() {
            let shadow: std::collections::HashSet<String> =
                to_carry.iter().map(|d| d.url_path.clone()).collect();
            log::info!(
                target: "incremental",
                "MOSS_INCREMENTAL_VERIFY=1: re-rendering {} carried pages to byte-compare them",
                shadow.len(),
            );
            to_render.append(&mut to_carry);
            shadow
        } else {
            std::collections::HashSet::new()
        };

        // page_rss_link doesn't depend on the doc — compute it once.
        let rss_href = ServedPath::for_rss("").unwrap().to_relative_url();
        let page_rss_link = if has_rss {
            Some(format!(r#"<link rel="alternate" type="application/rss+xml" title="RSS" href="{rss_href}">"#))
        } else { None };

        // Phase 1 — render in parallel. Progress is emitted on a monotonic
        // completion counter (tasks finish out of order). `reporter` is
        // Option<&AppHandle> and AppHandle is Sync, so emitting from worker
        // threads is sound. Errors surface in `documents` order in phase 2, so
        // the first error propagated is identical to the sequential loop's.
        let render_total = to_render.len();
        let rendered_atomic = AtomicUsize::new(0);
        // Open question: how much of this span is per-PAGE work
        // (which a narrower render set reclaims) vs per-BUILD prelude above
        // (which it does not). Answered by the checkpoint below — with the
        // INFO after the loop it cuts the span into before / render / phase-2,
        // and on a one-page edit the render is ~3ms against ~340ms before it.
        log::debug!(target: "timing", "[render] html_pages: partition done ({} to render, {} to carry): {:?}", to_render.len(), to_carry.len(), total_start.elapsed());
        let t_render_pages = std::time::Instant::now();
        let rendered: Vec<Result<RenderedPage, BuildStopped>> = to_render
            .par_iter()
            .map(|doc| {
                let output_file_path = output_dir.join(&doc.url_path);
                if let Some(parent) = output_file_path.parent() {
                    crate::build::io_utils::create_output_dir_all(parent)
                        .map_err(|e| format!("Failed to create directory: {}", e))?;
                }
                let mut og_outputs =
                    crate::build::page::og_card::OgSink::new(&previous_hashes.files);
                let html_page = generate_html_collect_og(
                    Some(doc),
                    &documents,
                    project_structure,
                    &layout_config,
                    false,
                    page_rss_link.as_deref(),
                    analytics_script.as_deref(),
                    site_lang,
                    Some(&css_version),
                    has_user_css,
                    has_sidebar_layout,
                    user_css_version.as_deref(),
                    has_user_js,
                    user_js_version.as_deref(),
                    Some(&content_graph),
                    &dir_overrides,
                    &site_url,
                    show_rss_in_footer,
                    emit_source_lines,
                    &favicon_filename,
                    favicon_has_raster_pngs,
                    Some(output_dir),
                    &mut og_outputs,
                    source_path_buf,
                    &scripts,
                )?;

                // Snapshot BEFORE the write below replaces the file: these are
                // the previous build's FINAL (post-slot-injection) bytes, and
                // they are the only copy of them that survives this build.
                let carried_previous = if verify_shadow.contains(&doc.url_path) {
                    match std::fs::read(&output_file_path) {
                        Ok(prev) => Some((doc.url_path.clone(), prev)),
                        Err(e) => {
                            log::warn!(
                                target: "incremental",
                                "MOSS_INCREMENTAL_VERIFY: cannot read {} to compare: {e}",
                                doc.url_path,
                            );
                            None
                        }
                    }
                } else {
                    None
                };

                crate::build::io_utils::write_output_if_changed(&output_file_path, html_page.as_bytes())
                    .map_err(|e| format!("Failed to write HTML file: {}", e))?;
                let url_sp = ServedPath::from_source(&doc.url_path)
                    .map_err(|e| format!("Failed to construct article URL path: {}", e))?;
                let source_mapping = match doc.source_path.as_ref() {
                    Some(src) => {
                        let url_sp2 = ServedPath::from_source(&doc.url_path)
                            .map_err(|e| format!("Failed to construct article URL path for mapping: {}", e))?;
                        Some((src.clone(), url_sp2))
                    }
                    None => None,
                };
                let page_meta = doc.source_path.as_ref().map(|_| crate::types::content::PageMeta {
                    title: doc.title.clone(),
                    date: doc.date.clone(),
                });

                // Emit BackgroundProgress so the deploy panel shows what
                // `wait_for_in_flight_work` is actually waiting on. Monotonic
                // in completion order (was `doc_index` — non-monotonic under
                // parallelism); total stays `html_total` for continuity with
                // the final completed emit below.
                let done = rendered_atomic.fetch_add(1, Ordering::Relaxed) + 1;
                if html_total > 0 && (done % HTML_PROGRESS_STRIDE == 0 || done == render_total) {
                    reporter.report(&crate::build::progress::PipelineEvent::BackgroundProgress {
                        task: "html_pages".to_string(),
                        current: done as u32,
                        total: html_total,
                        message: crate::infra::app_advisory::fmt("rendering_pages", &[("n", &done.to_string()), ("total", &html_total.to_string())]),
                        completed: false,
                        advisories: vec![],
                    });
                }

                Ok(RenderedPage {
                    url_sp,
                    html_bytes: html_page.into_bytes(),
                    og_cards: og_outputs.into_cards(),
                    source_mapping,
                    page_meta,
                    carried_previous,
                })
            })
            .collect();

        log::info!(
            target: "timing",
            "[render] html pages: parallel render of {} pages took {:?} ({} carried); the rest of this span is per-build prelude",
            render_total,
            t_render_pages.elapsed(),
            to_carry.len(),
        );
        log::debug!(target: "timing", "[render] html_pages: phase1 done: {:?}", total_start.elapsed());

        // Phase 2 — register into `pending` sequentially, in `documents` order.
        // Wrap in a closure so `?` short-circuits to the inner Result and the
        // completion event fires on every exit path (Ok or Err) before we
        // propagate — same rationale as the original loop.
        let mut count: usize = 0;
        let loop_result: Result<(), BuildStopped> = (|count: &mut usize,
                                                      verify: &mut Option<
            crate::build::render::incremental::CarryVerification,
        >| {
            for page in rendered {
                let page = page?;
                if let (Some(verify), Some((url_path, previous))) = (verify.as_mut(), page.carried_previous) {
                    verify.record(url_path, previous);
                }
                // Site 4: register OG cards from the receipts the render
                // returned — the bytes it wrote, or the previous entry it chose
                // to carry. The ImageOutputs bucket routes them to
                // image_outputs + blocking_keys.
                for card in &page.og_cards {
                    card.register(pending);
                }
                pending.register(&page.url_sp, &page.html_bytes, HashBucket::Files);
                // Register source→output mapping so the file watcher's rename-hint
                // resolver can find the output path without re-deriving slug rules.
                // See `build::watch::build_rebuild_event_with_renames`.
                if let Some((src, url_sp2)) = page.source_mapping {
                    pending.register_source_mapping(src.clone(), &url_sp2);
                    register_page_source(pending, &page_source_hashes, &src);
                    if let Some(meta) = page.page_meta {
                        pending.register_page_meta(src.clone(), meta);
                    }
                }
                *count += 1;
            }

            // Manifest carry-forward for the skipped pages.
            // `remove_stale_html` deletes every
            // `index*.html` under the stage dir whose key is absent from THIS
            // build's `blocking_keys`, so a page we chose not to re-render is
            // a page we must re-register or destroy. The previous entry is
            // reused verbatim: the file on disk is literally the previous
            // build's bytes, so its hash is too, and the sealed manifest stays
            // identical to the previous build across the skipped region.
            for doc in &to_carry {
                let url_sp = ServedPath::from_source(&doc.url_path)
                    .map_err(|e| format!("Failed to construct carried URL path: {}", e))?;
                let Some(entry) = previous_hashes.files.get(url_sp.as_str()).cloned() else {
                    // Unreachable: the partition above required this entry.
                    return Err(BuildStopped::from(format!(
                        "carry-forward lost the manifest entry for {}",
                        url_sp.as_str()
                    )));
                };
                pending.register_hashed(&url_sp, &entry, HashBucket::Files);
                if let Some(src) = doc.source_path.as_ref() {
                    pending.register_source_mapping(src.clone(), &url_sp);
                    register_page_source(pending, &page_source_hashes, src);
                    pending.register_page_meta(
                        src.clone(),
                        crate::types::content::PageMeta { title: doc.title.clone(), date: doc.date.clone() },
                    );
                }
                *count += 1;
            }

            // Slot-only sources (`footer.md` and its per-language siblings) are
            // build INPUTS: they render into every page's chrome and own no
            // output, so both loops above — each keyed on an emitted output —
            // skipped them and their hash never entered `sources`. The sweep's
            // drift compare reads that map, and a walked file with no baseline
            // entry reads as NEW, so every 2s pass judged both footers as drift,
            // dispatched a full rebuild that could not clear them, then blamed
            // the watcher and recreated it — twice, after which the folder
            // degraded to sweep-only and the event-driven partial build stopped
            // happening at all (riverbend, 2026-08-19).
            //
            // No `register_source_mapping`: there is no output to map, and the
            // hash-only shape is what keeps a slot invisible to the publish
            // classifier (it filters `sources` through `source_to_output`) and
            // to the seal's deleted-page prune. `register_page_source` rather
            // than a raw insert is what survives `replace_sources`.
            //
            // The one place a hash is read from disk rather than carried. A
            // slot source is only in `page_source_hashes` when THIS build read
            // it, and only carried when the previous MANIFEST held it — so a
            // vault whose manifest already lost its footers (every riverbend
            // vault, before the carry was fixed) would stay lost until some
            // build happened to bypass the parse cache. Hashing the two or
            // three slot files directly makes the entry unconditional, so the
            // vault heals on the next build instead of on the next full parse.
            for doc in &documents {
                if !doc.slot_only {
                    continue;
                }
                let Some(src) = doc.source_path.as_ref() else {
                    continue;
                };
                if let Some(meta) = page_source_hashes.get(src) {
                    pending.register_page_source_hash(src.clone(), meta.clone());
                } else if pending.carry_forward_page_source(src).is_none() {
                    let abs = source_path_buf.join(src);
                    match fs::read(&abs) {
                        Ok(bytes) => pending
                            .register_page_source_hash(src.clone(), source_metadata(&abs, &bytes)),
                        // Unreadable (evicted, permissions): leave it absent,
                        // which is what the sweep already tolerates for a file
                        // it cannot stat. Better one extra rebuild than a hash
                        // of nothing.
                        Err(e) => log::debug!(
                            "slot source {src} could not be hashed for the manifest: {e}"
                        ),
                    }
                }
            }

            // A page's auto-generated OG card is written and registered by
            // the SAME render call the carry above skipped, so the cards of
            // every carried page have to be re-registered too.
            if !to_carry.is_empty() || homepage_carried {
                crate::build::page::og_card::carry_previous_cards(
                    &previous_hashes,
                    output_dir,
                    pending,
                );
            }
            Ok(())
        })(&mut count, &mut carry_verification);
        // Emit the final completed event regardless of whether the loop
        // succeeded — even on an early-return error, we want the progress
        // panel to drop the "Rendering pages" row so the failure surface
        // (toast, notification) is the user's only visible signal. Guarded
        // by `html_total > 0` for symmetry with the in-loop emit; an empty
        // site shouldn't briefly flash a "Rendered 0 pages" row.
        if html_total > 0 {
            // Use the SAME `total` as the in-loop emits so the progress row
            // doesn't visually flip-flop at completion (e.g. "220/240" →
            // "220/220"). Some documents may be skipped (already-rendered
            // index.html, synthetic docs) so `count < html_total`; we report
            // the denominator the user's been watching all along and the
            // numerator the loop actually achieved.
            reporter.report(&crate::build::progress::PipelineEvent::BackgroundProgress {
                task: "html_pages".to_string(),
                current: count as u32,
                total: html_total,
                message: crate::infra::app_advisory::fmt("rendered_pages", &[("n", &count.to_string())]),
                completed: true,
                advisories: vec![],
            });
        }
        loop_result?;
        count
    };
    log::debug!(target: "timing", "[render] html_pages: {:?}", total_start.elapsed());
    send_progress(
        progress_sender,
        "generating",
        &crate::infra::app_advisory::t("generating_pages"),
        60,
        false,
        None,
        None,
    );

    // Auto-generate folder index pages for folders that have child documents
    // but no explicit index.md. This implements Principle 2: "Folders and .md
    // files correspond to HTML pages."
    {
        use std::collections::{HashMap, HashSet};

        // Step 1: Collect all folder prefixes that have child documents.
        // For each document, extract the parent folder from url_path.
        // E.g., "videos/aimeili/index.html" -> "videos"
        let mut folders_with_children: HashMap<String, Vec<usize>> = HashMap::new();
        for (idx, doc) in documents.iter().enumerate() {
            if doc.url_path == "index.html" {
                continue; // Skip root homepage
            }
            // Extract the first-level parent folder
            // url_path is like "videos/aimeili/index.html" or "articles/tutorials/getting-started/index.html"
            // We want ALL ancestor folders, not just the immediate parent.
            let parts: Vec<&str> = doc.url_path.split('/').collect();
            // For "videos/aimeili/index.html", parts = ["videos", "aimeili", "index.html"]
            // Parent folders are: "videos", "videos/aimeili" (but "videos/aimeili" IS the document's own folder)
            // We want to register this doc as a child of all ancestor folders EXCEPT
            // the folder that IS this document (if url_path ends with /index.html).
            //
            // For a doc at "videos/aimeili/index.html": register as child of "videos"
            // For a doc at "articles/tutorials/getting-started/index.html": register as child of
            //   "articles", "articles/tutorials"
            if parts.len() >= 2 {
                // Build ancestor paths, excluding the document's own directory
                // The document's own directory is parts[0..parts.len()-1] joined
                // (e.g., "videos/aimeili" for "videos/aimeili/index.html")
                let doc_dir_depth = parts.len() - 1; // exclude "index.html"
                for depth in 1..doc_dir_depth {
                    let folder = parts[..depth].join("/");
                    folders_with_children
                        .entry(folder)
                        .or_default()
                        .push(idx);
                }
            }
        }

        // Seed every index-page directory (mapped to its page-tree key) with an
        // empty child list so a folder that has no child documents at all still
        // gets its index page emitted below. Block 1 already created a synthetic
        // folder document for these; this registers them for HTML generation.
        // Folders that DO have children were added by the loop above (an empty
        // `or_default()` here is a no-op for them).
        for (_, key) in folder_index_keys(&project_structure.dirs, &dir_overrides) {
            folders_with_children.entry(key).or_default();
        }

        // Term pseudo-folders: register for index-page emission from the
        // same list Block 1 seeded its synthetic docs from. Their members
        // live at unrelated URLs and claim membership via the derived
        // `also_in`, so the ancestor loop above never registers them — and
        // a vault whose every term is CLAIMED has no synthetic term doc at
        // all, so the roots would otherwise silently drop out.
        for key in term_index.synthetic_folder_keys() {
            folders_with_children.entry(key).or_default();
        }

        // Drop empty language-named folders (e.g. an `en/` with no content of
        // its own). Block 1 already excludes lang-prefix dirs from synthetic-doc
        // creation via this same `resolve_language_from_folder` predicate, so a
        // childless lang-named folder that survived here would emit an
        // `en/index.html` with no synthetic doc — hence no nav/parent/breadcrumb
        // entry — an orphan page. Restrict the retain to childless (dirs-seeded)
        // folders: folders that DO have child documents are left to the
        // translation-root filter below, which handles real translation trees.
        folders_with_children.retain(|folder, child_indices| {
            if !child_indices.is_empty() {
                return true;
            }
            let top_segment = folder.split('/').next().unwrap_or(folder);
            crate::i18n::path::resolve_language_from_folder(top_segment).is_none()
        });

        // Filter out translation root directories from auto-index.
        // A translation root is a folder whose index page is a translation of the
        // site homepage (linked via translationKey or stem convention).
        // These are structural URL directories for translations, not content folders.
        folders_with_children.retain(|folder, _| {
            let top_segment = folder.split('/').next().unwrap_or(folder);
            let top_index = format!("{}/index.html", top_segment);
            let is_translation_root = documents.iter()
                .find(|d| d.url_path == top_index)
                .map(|d| d.translations.iter().any(|t| t.url_path == "index.html"))
                .unwrap_or(false);
            !is_translation_root
        });

        // Step 2: Identify folders that already have an explicit index page
        // from a real source file. Synthetic entries (source_path: None) inserted
        // above are excluded — they still need HTML generated by this loop.
        let folders_with_explicit_index: HashSet<String> = documents
            .iter()
            .filter(|d| d.url_path.ends_with("/index.html") && d.url_path != "index.html")
            .filter(|d| d.source_path.is_some())
            .map(|d| {
                d.url_path
                    .trim_end_matches("/index.html")
                    .to_string()
            })
            .collect();

        // Hoist root-homepage lookups (same for every folder)
        let root_doc = documents.iter().find(|d| d.url_path == "index.html");
        let root_analytics_script = root_doc
            .and_then(|d| d.analytics.as_ref())
            .map(|analytics| analytics.to_script_tag());

        // The synthetic folder indexes — real directories with no `index.md`.
        // Outside the skip machinery
        // entirely (no source document → no facade entry → nothing to carry),
        // so all of them re-render on every save. That is the target here. How many
        // there are is a fact about the vault: an earlier count here (171 of
        // 386 pages) matched neither vault measured since — a 226-post bench
        // vault and a real 223-page site each emit exactly ONE.
        let _phase_auto_index = PhaseTrace::start("render_auto_folder_indexes");
        let mut auto_index_count = 0usize;

        // Step 3: For each folder needing an auto-generated index, generate a page.
        for (folder, _child_indices) in &folders_with_children {
            if folders_with_explicit_index.contains(folder) {
                continue; // Folder already has an explicit index.md
            }

            // Filter to direct children through the CANONICAL selector
            // (the listing-group model's rule 3). This loop used to carry an
            // inlined second copy of the membership filter — prefix test,
            // `also_in`, `is_listable`, direct-child remainder — which meant
            // "one selector, therefore no drift" was false and the listing
            // digest could not honour a rule the renderer honours. `folder` is
            // already a URL-space (slugified) key, and the synthetic index
            // always lists direct children with neither homepage filter.
            let folder_docs: Vec<&ParsedDocument> =
                crate::build::folder_embed::select_children_by_slug(
                    folder,
                    false, // depth: direct
                    false, // scope_default_tree: homepage-only filter
                    false, // exclude_nav: homepage-only filter
                    &documents,
                    project_structure,
                );

            // NOTE: an empty `folder_docs` is intentionally NOT skipped anymore.
            // Every real folder gets an index page, even a completely empty one
            // (product decision: no folder 404s). `generate_children` returns
            // "" for an empty slice, so the page degrades to a title-only
            // listing with full nav/chrome. The only folders reaching this loop
            // with no children are real on-disk directories seeded from the
            // scan; doc-derived folders always have ≥1 child.

            // The synthetic document Block 1 created for this folder owns its
            // title, language and breadcrumb, so the H1, the page-tree label
            // and the chrome can never disagree. It is constructed afresh
            // only for a folder that has no document: a language-named
            // folder with content but no index, which Block 1 skips (see the
            // retain there) and this loop keeps — an orphan page. The drift
            // runs the other way too: this loop drops every folder under a
            // translation root, Block 1 only language-named ones, so an
            // arbitrarily named translation root's subfolder has a document
            // and no page. Both are why this loop still computes its own
            // folder set instead of iterating the synthetic documents.
            let synthetic_path = format!("{}/index.md", folder);
            let auto_url_path = format!("{}/index.html", folder);
            let matching_doc = documents.iter().find(|d| d.url_path == auto_url_path);
            let orphan_doc;
            let folder_doc: &ParsedDocument = match matching_doc {
                Some(d) => d,
                None => {
                    orphan_doc = synthetic_folder_doc(folder, &term_index, &folder_display, &folder_languages, site_lang);
                    &orphan_doc
                }
            };
            let folder_lang = folder_doc.lang;
            let page_title = folder_doc.title.clone();

            // Generate the folder index content listing using generate_children
            // (folder-aware: distinguishes subfolders from articles)
            generated_index_urls.push(format!("{}/", folder));

            let all_docs_refs: Vec<&ParsedDocument> = documents.iter().collect();

            let (style_resolved, group_resolved) =
                resolve_children_config(&folder_docs, None, None, site_config.math);
            let children_style = style_resolved.value;
            let children_group = group_resolved.value;
            // group_was_explicit always false when passing None — no override gate
            // needed; the synthetic index always lets Date-axis year grouping fire.

            let render_group = |docs: &[&ParsedDocument]| generate_children(
                docs,
                &all_docs_refs,
                project_structure,
                &children_style,
                &children_group,
                folder_lang,
                false,
                &dir_overrides,
                layout_config.typesetting.as_deref(),
                Some(&event_level_image_lookup),
                // Synthetic folder index — no `ParsedDocument` with a
                // direct_children_sort exists. Date fallback (None → Date) matches
                // the legacy meta-slot behavior.
                None,
                site_config.math, false,
                &Default::default(),
            );
            // The generated half of the same split the claimed term page gets
            // in `folder_embed`, through the same function: a term no page
            // claims still lists its members under the field each was named
            // through. `folder` is the term key here, and the sections come
            // from the index that derived them — `None` for every ordinary
            // folder, and for a term reached through one field only, which
            // then renders exactly as it did before. `site_lang`, not
            // `folder_lang`: a role heading is term chrome, and the term's
            // two pages have to say the same word.
            let article_list = crate::build::terms::render_term_sections(
                term_index.sections(folder),
                &folder_docs,
                site_lang,
                render_group,
            )
            .unwrap_or_else(|| render_group(&folder_docs));
            // The generated half of the same breadcrumb/children chrome the
            // claimed page gets in `folder_embed`, through the same resolved
            // data: `None` (rendered empty) for every non-place term, since
            // `TermSite::parent` is only ever set for a place-typed kind.
            let place_breadcrumb_html = crate::build::components::place_hierarchy::render_breadcrumb(
                term_index.breadcrumb(folder).unwrap_or(&[]),
            )
            .unwrap_or_default();
            let place_children_html = crate::build::components::place_hierarchy::render_children(
                term_index.children(folder).unwrap_or(&[]),
            )
            .unwrap_or_default();
            // Synthetic folder index: no markdown source, so prepend the shared
            // <h1 class="moss-folder-title"> via folder_title::render. Same
            // helper used by folder_cover::render (cover branch) and
            // render/html.rs (no-cover branch).
            //
            // Intentionally NOT nav-gated (unlike the no-cover branch in
            // render/html.rs): a synthetic index has no `ParsedDocument`, and
            // the nav bar lists only `ParsedDocument`s, so it can never be a
            // nav item — there is no title to suppress here. Do not add an
            // `is_nav_bar_item` guard using the document list at this site.
            //
            // No `data-source-fm="title"` either: there is no markdown file
            // behind this heading, so a click on it in the editor preview has
            // no `title` field to point at. Annotating it would promise a
            // destination that does not exist.
            //
            // Breadcrumb/children are spliced in only when non-empty, each on
            // its own line — an empty string here must not add a byte to a
            // non-place site's output, which the byte-identity witness (task
            // A0) treats as a build regression exactly the same as any other.
            let mut content_html = crate::build::components::folder_title::render(&page_title, false);
            if !place_breadcrumb_html.is_empty() {
                content_html.push('\n');
                content_html.push_str(&place_breadcrumb_html);
            }
            content_html.push('\n');
            content_html.push_str(&article_list);
            if !place_children_html.is_empty() {
                content_html.push('\n');
                content_html.push_str(&place_children_html);
            }

            let path_resolver = {
                let pr = match css_version.as_str() {
                    "" => PathResolver::new(),
                    version => PathResolver::new().with_css_version(version),
                };
                let pr = match &user_css_version {
                    Some(v) => pr.with_user_css_version(v),
                    None => pr,
                };
                let pr = match &user_js_version {
                    Some(v) => pr.with_user_js_version(v),
                    None => pr,
                };
                let pr = pr.with_js_version(&js_version);
                pr.with_favicon_filename(&favicon_filename).with_dir_overrides(dir_overrides.clone())
            };

            let nav_builder = NavigationBuilder::new(
                &documents,
                &layout_config.site_name,
                Some(auto_url_path.as_str()),
                folder_lang,
                project_structure.has_content_folders,
            )
            .with_search(layout_config.assets.search);
            let nav_builder = if let Some(ref logo) = layout_config.logo_path {
                nav_builder.with_logo(logo.clone())
            } else {
                nav_builder
            };

            // Breadcrumb: the same enable / opt-out / translation-root rule
            // every authored page gets (`compute_breadcrumb_segments`). Until
            // 2026-09-05 this loop carried its own copy that enabled only on
            // an explicit `breadcrumb: true` and, for an ancestor with no
            // document, printed the current page's title — so a nav-less
            // site's articles had a trail their folder index lacked, and a
            // term page read `Site › 林小滿 › 林小滿`. The home crumb is
            // `site_name`, as it always was here; an authored page under a
            // nested language folder derives a per-language site title
            // instead (`render/html.rs`), a remaining twin noted elsewhere.
            let nav_builder = match compute_breadcrumb_segments(
                folder_doc,
                &documents,
                &layout_config.site_name,
                project_structure.has_content_folders,
            ) {
                Some(segments) => nav_builder.with_breadcrumb(segments),
                None => nav_builder,
            };

            // Same split as an authored page: `folder_lang` picks the interface
            // strings, this picks what `<html lang>` claims about the content.
            // A synthetic index has no authored doc to read it from, so
            // it takes its tree's.
            let page_lang_tag = matching_doc.and_then(|d| d.lang_tag.clone())
                .unwrap_or_else(|| crate::i18n::lang_tag_in_tree(&synthetic_path, folder_lang));

            // Wire language toggle for auto-generated index pages through the
            // same single source of truth as html.rs. Current language is
            // `folder_lang` (the auto-index page's language, not site_lang), so
            // the exclusion is correct for non-default-language folder indexes.
            let nav_builder = {
                let per_article = matching_doc.map(|d| d.translations.clone()).unwrap_or_default();
                let lang_roots = super::lang_roots::site_lang_roots(&documents, site_lang);
                let multi = super::lang_roots::site_publishes_multiple_languages(&documents, &lang_roots, site_lang);
                let links = crate::i18n::site_languages::other_language_links(
                    &page_lang_tag, &per_article, &lang_roots, multi);
                // Always adopt the page's language (empty links → no toggle).
                nav_builder.with_translations(folder_lang, &page_lang_tag, links)
            };

            let footer_html = nav_builder.generate_footer(show_rss_in_footer);
            let navigation = nav_builder.generate_navigation();
            let nav_island = if layout_config.floating_nav {
                nav_builder.generate_nav_island()
            } else {
                String::new()
            };

            let processor = ShellProcessor::new();

            // Use the Page template (same as homepages/index pages)
            let shell_type = ShellType::Page;

            let analytics_script = root_analytics_script.clone();

            let rss_href = ServedPath::for_rss("").unwrap().to_relative_url();
            let page_rss_link = if has_rss {
                Some(format!(r#"<link rel="alternate" type="application/rss+xml" title="RSS" href="{rss_href}">"#))
            } else {
                None
            };

            let vars = ShellVars {
                lazy_chunk_attrs: path_resolver.lazy_chunk_attrs(&share_card_hash, hls_attr_hash),
                title: tab_title(&page_title, &layout_config.site_name, false),
                css_path: path_resolver.css_path(),
                js_path: path_resolver.js_path(),
                navigation,
                nav_island,
                homepage_content: content_html,
                latest_list: None,
                latest_sidebar: None,
                favicon: Some(path_resolver.favicon_link()),
                rss_link: page_rss_link,
                analytics: analytics_script,
                footer: Some(footer_html),
                body_attrs: if emit_source_lines { " data-moss-preview".to_string() } else { String::new() },
                page_wrapper_class: String::new(),
                hero_section: None,
                // A synthetic folder index has no authored doc of its own to
                // carry a cover; `matching_doc` is the authored counterpart
                // when there is one, and it renders through html.rs instead.
                share_cover_attr: String::new(),
                // It does have a URL and a Share button, though, so it names
                // its QR code like any other page. The file is written below.
                share_qr_attr: qr::share_qr_attr(&auto_url_path, true, &site_url),
                main_class: String::new(),
                date: None,
                formatted_date: None,
                date_line: None,
                short_date: None,
                content: None, // Page template, not Article
                description: None,
                og_tags: None,
                // Auto-generated folder indexes don't carry frontmatter, so
                // there's no rich title/description to populate twitter cards
                // or canonical links from.
                twitter_tags: None,
                canonical_link: None,
                // Auto-generated folder indexes are not language-translated;
                // they're collection landing pages built from the directory
                // contents. No hreflang signal applies.
                hreflang_links: None,
                apple_touch_icon: None,
                schema_json_ld: None,
                content_width_attr: resolve_data_attr(None, layout_config.content_width.as_ref(), "content-width", None),
                typesetting_attr: resolve_data_attr(None, layout_config.typesetting.as_ref(), "typesetting", Some("horizontal")),
                comments_attr: resolve_comments_attr(None, layout_config.comments),
                lang: page_lang_tag,
                ui_lang: folder_lang,
                user_css_link: if has_user_css {
                    Some(format!(
                        "\n    <link rel=\"stylesheet\" href=\"{}\" layer=\"themes\">",
                        path_resolver.user_css_path()
                    ))
                } else {
                    None
                },
                user_js_tag: if has_user_js {
                    Some(path_resolver.user_js_tag())
                } else {
                    None
                },
                has_sidebar_layout, // Use site-wide flag (true if any doc has sidebar)
                // Auto-generated folder indexes don't contain typed embeds,
                // so they never need the <model-viewer> head script. Keep
                // empty for consistency with the struct contract.
                embed_head_assets: String::new(),
                // Folder indexes are not article pages, so they never carry
                // series-nav (or other native post-article modules).
                post_article: String::new(),
                runtime_js_tags: runtime_js_tags.clone(),
                // Auto-generated folder indexes are always listable — no noindex.
                robots_meta: None,
            };

            let html_page = processor.process(shell_type, vars);

            // Site 5 (Pattern A): emit auto-generated folder index.html.
            // ctx.emit creates parent dirs, writes the file, and registers in pending (SHA-256).
            let auto_sp = ServedPath::from_source(&auto_url_path)
                .map_err(|e| format!("Failed to construct auto-index path: {}", e))?;
            BuildContext::for_render(output_dir, pending)
                .emit(&auto_sp, html_page.as_bytes(), HashBucket::Files)
                .map_err(|e| format!("Failed to emit auto-generated index: {}", e))?;

            // …and its QR code, here rather than in the loop below, because a
            // synthetic index has no entry in `documents` for that loop to walk.
            // Attribute and file are written together so neither can exist
            // without the other.
            qr::emit_share_qr(&auto_url_path, true, &site_url, output_dir, pending)?;
            page_count += 1;
            auto_index_count += 1;
            // Per-folder DEBUG line removed 2026-09-15 (measured ~650
            // lines/session in a real upload) — redundant
            // with the one INFO summary just below, which already carries
            // the total; no build-affecting behavior depended on it.
        }
        drop(_phase_auto_index);
        log::info!(
            target: "timing",
            "[render] auto folder indexes: {auto_index_count} rendered (none skippable — no source document, so no facade entry to carry)",
        );
        // All vault SHAPE, so measure before optimizing: each index costs in
        // proportion to the children it lists. One flat `posts/` folder of 225
        // children is 259ms of a 388ms call; a real 223-page site with a deep
        // tree is 0.4ms. Medians of 7 rebuilds.
        log::debug!(target: "timing", "[render] auto folder indexes rendered: {:?}", total_start.elapsed());
    }

    // Site 6 (Pattern A): emit _moss/style.<hash>.css (content-hashed filename).
    // css_content is still used above for cache-busting (css_version); emit here.
    BuildContext::for_render(output_dir, pending)
        .emit(&ServedPath::for_default_stylesheet_hashed(&css_version), css_content.as_bytes(), HashBucket::Files)
        .map_err(|e| format!("Failed to emit _moss/style.<hash>.css: {}", e))?;

    // Copy user's custom CSS if present.
    // Canonical location: .moss/theme/style.css. Files at the project root are
    // ignored; see check_misplaced_theme_files() for the user-facing warning.
    // Output is `/_moss/theme/style.css` (verbatim theme mount).
    //
    // Reuses the single read taken near the top of this function rather than
    // reading style.css again. Two reads are two answers: the first decides
    // whether every page *links* the stylesheet, this one decides whether the
    // build *emits* it, and an eviction landing between them makes the site
    // link a file it never wrote — 404 on every page, which is the exact
    // outcome the deferral was added to prevent. One read, one answer.
    if let Some(ref user_css_content) = user_css_content {
        let user_css_hash = compute_binary_hash(user_css_content.as_bytes());
        // Site 7 (Pattern A): emit _moss/theme/style.<hash>.css (content-hashed filename).
        BuildContext::for_render(output_dir, pending)
            .emit(&ServedPath::for_custom_stylesheet_hashed(&user_css_hash), user_css_content.as_bytes(), HashBucket::Files)
            .map_err(|e| format!("Failed to emit _moss/theme/style.<hash>.css: {}", e))?;
    }

    // Copy user's custom JS if present.
    // Canonical location: .moss/theme/script.js. Files at the project root are
    // ignored; see check_misplaced_theme_files() for the user-facing warning.
    // Output is `/_moss/theme/script.js` (verbatim theme mount).
    // Same single-read rule as style.css above.
    if let Some(ref user_js_content) = user_js_content {
        let user_js_hash = compute_binary_hash(user_js_content.as_bytes());
        // Site 8 (Pattern A): emit _moss/theme/script.<hash>.js (content-hashed filename).
        BuildContext::for_render(output_dir, pending)
            .emit(&ServedPath::for_custom_script_hashed(&user_js_hash), user_js_content.as_bytes(), HashBucket::Files)
            .map_err(|e| format!("Failed to emit _moss/theme/script.<hash>.js: {}", e))?;
    }

    // theme.js is emitted with every other runtime script by `scripts.emit()`
    // below — it is the `gate: |_| true` row in SITE_SCRIPTS. The hand-rolled
    // emit that used to sit here wrote the same bytes to the same path a second
    // time.

    // Prelude instrumentation: everything from here to `[render]
    // total` runs unconditionally on the DOCUMENT COUNT, not on what changed —
    // so on a single-page edit it is pure prelude. These checkpoints (added
    // while investigating why that prelude did not shrink) are cumulative
    // from `total_start`, matching the style of every other `timing` line
    // in this function.
    log::debug!(target: "timing", "[render] prelude: css+js emitted: {:?}", total_start.elapsed());

    // Generate RSS feed and sitemap (only if a real https URL was resolved)
    if has_rss {
        let url = site_url.as_str();
        // Generate RSS feed (always when a real https URL is resolved)
        {
            let rss_site_title = documents
                .iter()
                .find(|d| d.url_path == "index.html")
                .map(|d| d.title.as_str())
                .unwrap_or(crate::i18n::t(site_lang, "site"));

            let rss_analytics = documents
                .iter()
                .find(|d| d.url_path == "index.html")
                .and_then(|d| d.analytics.as_ref());

            let rss_content = crate::build::feeds::rss::generate_rss_feed(
                &documents,
                rss_site_title,
                url,
                None,
                rss_analytics,
            );

            // Site 10 (Pattern A): emit rss.xml.
            BuildContext::for_render(output_dir, pending)
                .emit_held(&ServedPath::for_rss("").unwrap(), rss_content.into_bytes(), HashBucket::Files)
                .map_err(|e| format!("Failed to emit rss.xml: {}", e))?;
        }

        // sitemap.xml is emitted further down, once every page is registered.

        // Generate robots.txt (user's own will overwrite during background asset copy)
        let robots_content =
            crate::build::feeds::sitemap::generate_robots_txt(url, site_config.ai_policy.as_deref());
        // Site 12 (Pattern A): emit robots.txt.
        BuildContext::for_render(output_dir, pending)
            .emit(&ServedPath::for_robots_txt(), robots_content.as_bytes(), HashBucket::Files)
            .map_err(|e| format!("Failed to emit robots.txt: {}", e))?;

        // Generate QR codes for article pages
        let qr_dir = output_dir.join("qr");
        crate::build::io_utils::create_output_dir_all(&qr_dir)
            .map_err(|e| format!("Failed to create qr directory: {}", e))?;

        // Which pages get a code, and where it lives, is decided in ONE place —
        // `build/media/qr.rs` — because `render/html.rs` has to reach the same
        // verdict to write `data-share-qr`. Anything decided here instead is
        // invisible to the reader, which is how folder notes ended up with
        // cards missing their QR corner.
        qr::emit_share_qrs(&documents, &site_url, output_dir, pending)?;
    }
    log::debug!(target: "timing", "[render] prelude: rss+sitemap+robots+qr: {:?}", total_start.elapsed());

    // Search indexing (Pagefind) does NOT run here. Pagefind walks rendered
    // HTML on disk, and at this point homepage/media-collection/subscribe
    // pages haven't been written yet and stale pages from deleted/renamed
    // sources haven't been cleaned up — indexing here would miss pages and
    // index 404s. The pipeline caller runs it after `remove_stale_html` and
    // after `generate_blocking_content` returns, using `BackgroundContext::search_enabled`
    // (set below). See `build/feeds/search.rs`.

    // Generate llms.txt — full site content for LLM consumption (unconditional)
    {
        let llms_site_title = documents
            .iter()
            .find(|d| d.url_path == "index.html")
            .map(|d| d.title.as_str())
            .unwrap_or(crate::i18n::t(site_lang, "site"));
        let homepage_desc = documents
            .iter()
            .find(|d| d.url_path == "index.html")
            .and_then(|d| d.description.as_deref());
        let llms_content = crate::build::feeds::llms_txt::generate_llms_txt(
            &documents,
            llms_site_title,
            homepage_desc,
        );
        // Site 14 (Pattern A): emit llms.txt.
        BuildContext::for_render(output_dir, pending)
            .emit_held(&ServedPath::for_llms_txt(), llms_content.into_bytes(), HashBucket::Files)
            .map_err(|e| format!("Failed to emit llms.txt: {}", e))?;
    }
    log::debug!(target: "timing", "[render] prelude: llms.txt: {:?}", total_start.elapsed());
    send_progress(
        progress_sender,
        "generating",
        &crate::infra::app_advisory::t("generating_feeds"),
        65,
        false,
        None,
        None,
    );

    // Generate index.html - either use homepage document or generate auto-index
    // NOTE: This runs BEFORE blocking_keys snapshot so index.html is included
    // in the blocking-phase key set and won't be deleted by stale cleanup.
    //
    // Empty-folder onboarding:
    // a synthetic homepage was pushed into `documents` earlier — just after
    // markdown processing finished and before slug/layout/sitemap loops —
    // so the lookup below finds it identically to a real `index.md`.
    // See the `documents.is_empty()` block above the "Resolve slugs" header.
    if !documents.is_empty() {
        // Try to find a homepage document (index.md -> index.html or README.md -> index.html).
        let homepage_doc = documents.iter().find(|d| d.url_path == "index.html");

        if homepage_carried {
            // `homepage_carried` (computed above, before
            // the render loop) already proved via `verdict.may_skip` that
            // neither the homepage's own facade nor any listing group it
            // reads has moved — reuse the previous build's index.html
            // verbatim instead of re-running folder_embed's card synthesis.
            let Some(entry) = previous_hashes.files.get(homepage_index_sp.as_str()).cloned() else {
                // Unreachable: `homepage_carried` required this entry to exist.
                return Err(BuildStopped::from(
                    "carry-forward lost the manifest entry for index.html".to_string(),
                ));
            };
            pending.register_hashed(&homepage_index_sp, &entry, HashBucket::Files);
        } else {
            let mut homepage_og_outputs =
                crate::build::page::og_card::OgSink::new(&previous_hashes.files);
            let rss_href = ServedPath::for_rss("").unwrap().to_relative_url();
            let index_html = if homepage_doc.is_some() {
                // There's a homepage document - use it as the homepage content
                let homepage_rss = if has_rss {
                    Some(format!(r#"<link rel="alternate" type="application/rss+xml" title="RSS" href="{rss_href}">"#))
                } else { None };
                generate_html_collect_og(
                    homepage_doc,
                    &documents,
                    project_structure,
                    &layout_config,
                    true,
                    homepage_rss.as_deref(),
                    analytics_script.as_deref(),
                    site_lang,
                    Some(&css_version),
                    has_user_css,
                    has_sidebar_layout,
                    user_css_version.as_deref(),
                    has_user_js,
                    user_js_version.as_deref(),
                    Some(&content_graph),
                    &dir_overrides,
                    &site_url,
                    show_rss_in_footer,
                    emit_source_lines,
                    &favicon_filename,
                    favicon_has_raster_pngs,
                    Some(output_dir),
                    &mut homepage_og_outputs,
                    source_path_buf,
                    &scripts,
                )?
            } else {
                // No homepage document found - generate auto-index page with year-grouped article list
                let homepage_rss = if has_rss {
                    Some(format!(r#"<link rel="alternate" type="application/rss+xml" title="RSS" href="{rss_href}">"#))
                } else { None };
                generate_html_collect_og(
                    None,
                    &documents,
                    project_structure,
                    &layout_config,
                    false,
                    homepage_rss.as_deref(),
                    analytics_script.as_deref(),
                    site_lang,
                    Some(&css_version),
                    has_user_css,
                    has_sidebar_layout,
                    user_css_version.as_deref(),
                    has_user_js,
                    user_js_version.as_deref(),
                    Some(&content_graph),
                    &dir_overrides,
                    &site_url,
                    show_rss_in_footer,
                    emit_source_lines,
                    &favicon_filename,
                    favicon_has_raster_pngs,
                    Some(output_dir),
                    &mut homepage_og_outputs,
                    source_path_buf,
                    &scripts,
                )?
            };
            // Site 14b: register the homepage's OG cards from their receipts.
            for card in homepage_og_outputs.into_cards() {
                card.register(pending);
            }

            crate::build::io_utils::write_output(&output_dir.join("index.html"), index_html.as_bytes())
                .map_err(|e| format!("Failed to write index.html: {}", e))?;
            pending.register(&homepage_index_sp, index_html.as_bytes(), HashBucket::Files);
        }
        // Register source→output for the homepage's source markdown (e.g.
        // index.md, readme.md, or a self-named home file) so the rename-hint
        // resolver can find it. The main HTML write loop above skips
        // url_path == "index.html"; this is the only emission site for it.
        // Needed whether the homepage was rendered or carried: a carried
        // homepage still needs its source mapping refreshed so the watcher's
        // rename-hint resolver keeps working across this build.
        let homepage_source = homepage_doc.and_then(|d| d.source_path.clone());
        if let Some(src) = homepage_source {
            pending.register_source_mapping(src.clone(), &homepage_index_sp);
            register_page_source(pending, &page_source_hashes, &src);
            if let Some(doc) = homepage_doc {
                pending.register_page_meta(
                    src.clone(),
                    crate::types::content::PageMeta { title: doc.title.clone(), date: doc.date.clone() },
                );
            }
        }
        page_count += 1;
    }
    log::debug!(target: "timing", "[render] prelude: homepage: {:?}", total_start.elapsed());

    // What sitemap.xml lists: every page a reader is meant to land on. That is
    // every page registered by now, the home page included; then the media
    // collection pages, the pages still in the cloud, the static `.html` pages
    // and the notebook viewers, each added below. Not the redirect stubs and
    // noindex subscribe pages in between. `pending.files()` would also hand
    // back the previous build's pages until seal, a deleted source's among them.
    let mut sitemap_pages: std::collections::BTreeSet<String> = pending.pages_registered().cloned().collect();

    // All asset copying (images, other files, static dirs, colocated assets) is
    // deferred to the background phase. The guiding principle: "every source file
    // either becomes HTML (markdown) or gets copied to the same relative path."
    // The background phase does a single filesystem walk instead of maintaining
    // separate lists per category.
    //
    // blocking_keys is now accumulated in `pending` incrementally (via ctx.emit and
    // `CardOutput::register`). No separate tracking needed.
    //
    // Carry-forward of previous_hashes.files is implicit: PendingManifest was seeded
    // with previous_hashes (via PendingManifest::new in the caller). Blocking-phase
    // emits overwrite individual entries; non-emitted entries survive automatically.
    // The explicit carry-forward loop is no longer needed.

    // Build and save article map for syndication support
    // This creates a mapping from URL paths to article metadata
    generated_index_urls.sort(); // HashMap iteration order above; the persisted map must be stable
    let article_map = build_article_map(
        &documents,
        &project_structure.dirs,
        &dir_overrides,
        &url_collisions,
        &generated_index_urls,
        &term_index,
    );
    if let Err(e) = article_map.save(&moss_dir) {
        log::warn!("⚠️ Failed to save article map: {}", e);
    }

    // `.moss/build.nosync/inventory.json` — what `moss list` reads back. Written here
    // rather than derived from the article map because the signals that answer
    // "why is this page not in the listing?" (draft / listed / nav-item /
    // slot_only) exist together only on `ParsedDocument`, and the article map
    // drops slot-only documents entirely. See `build::emit::inventory`, whose file
    // `moss list` reads back.
    //
    // Non-fatal, like the article map above: a site that built correctly must
    // not be failed by a report file. The cost is real though — `moss list`
    // cannot tell a stale inventory from a current one, so it would report the
    // PREVIOUS build's pages as if they were this build's. Hence the warning:
    // it is the only signal that the two have diverged.
    if let Err(e) = crate::build::emit::inventory::write_inventory(
        &documents,
        &moss_dir,
        project_structure.has_content_folders,
        &layout_config.lang_tag,
    ) {
        log::warn!("⚠️ Failed to save inventory: {}", e);
    }

    // Gap #3 fix: emit redirect stubs into the pending manifest so the seal
    // (and therefore the generation-id) covers them. The baseline is the record
    // of what the last publish put live (`.moss/deploy/`), so it reflects the
    // URLs serving at the point a new build starts.
    // We use the IN-MEMORY `article_map` — not a re-read from disk — because
    // `article_map.save()` may not have flushed yet and, more importantly,
    // the in-memory value is the authoritative result of this build.
    match crate::build::feeds::redirects::emit_redirect_stubs(
        &paths,
        &article_map,
        output_dir,
        pending,
    ) {
        // A build that could not read what is live cannot notice a rename, and
        // an unnoticed rename 404s the old URL for good.
        //
        // Logged, not shown. The author cannot make the record readable, and
        // the risk is conditional on a rename she has not made — so the app
        // panel said only that something she had never heard of was missing.
        // The moment worth her attention is a rename made WHILE the record is
        // unusable, and this function is where that becomes visible: compare
        // the previous `article_map` on disk with `current_article_map` before
        // `article_map.save()` overwrites it. Deferred deliberately, not
        // forgotten (2026-08-29).
        Ok(health) => {
            use crate::build::feeds::redirects::BaselineHealth;
            let reason = match health {
                BaselineHealth::Fine => None,
                BaselineHealth::Unusable(why) => Some(why.describe()),
                BaselineHealth::NoBaselineYet => {
                    Some("this site has no usable record of what is live yet; the next \
                          publish writes it")
                }
            };
            if let Some(reason) = reason {
                log_warn_problem!(
                    "{reason} — a renamed page's old link may 404 until then"
                );
            }
        }
        Err(e) => log::warn!("Failed to emit redirect stubs: {}", e),
    }

    // A term whose namespace moved (`author` declared under `[terms.people]`)
    // keeps serving its old `authors/<slug>/` URL. Separate from the block
    // above because the two answer different questions from different
    // sources: that one merges a persisted rename history keyed on note UIDs,
    // this one is recomputed from the kinds table every build and stored
    // nowhere. Same emit pattern, so both land in `pending` and the seal
    // covers them.
    {
        use crate::build::context::BuildContext;
        use crate::build::feeds::redirects::{
            current_build_urls, generate_redirect_html, pretty_url_to_fs_path,
        };
        use crate::build::manifest::HashBucket;
        use crate::build::served_path::ServedPath;
        // A stub must never aim at a URL a real page already serves this
        // build — the root stub (`authors/`) can collide with an unrelated
        // page at that address, and a per-name stub can collide with a page
        // a member reused its old slug for. The manifest is last-write-wins,
        // so either collision would silently bury the real page.
        let served_urls = current_build_urls(&article_map);
        for (old_url, new_url) in crate::build::terms::kind_move_stubs(&term_index) {
            if served_urls.contains(&old_url) {
                continue;
            }
            let fs_path = pretty_url_to_fs_path(&old_url);
            let html = generate_redirect_html(&new_url);
            match ServedPath::from_source(&fs_path) {
                Ok(sp) => {
                    if let Err(e) = BuildContext::for_render(output_dir, pending)
                        .emit(&sp, html.as_bytes(), HashBucket::Files)
                    {
                        log::warn!("Failed to emit kind-move stub '{}': {}", fs_path, e);
                    }
                }
                Err(e) => log::warn!("Invalid kind-move stub path '{}': {}", fs_path, e),
            }
        }
    }

    // Generate _previews.json for hover link previews
    // Compact JSON mapping permalink → { title, description, preview }
    // Keyed on `is_public_page` (not `is_listable`): a `listed: false` page can
    // be linked (footer / wikilink), so its hover preview should still resolve.
    // Emitted only when [site].link_preview is on — the same gate that
    // controls the <script> tag and preview.js file below, so the runtime,
    // its data file, and the tag that loads it always agree.
    if site_config.link_preview {
        use std::collections::BTreeMap;
        let mut previews: BTreeMap<String, serde_json::Value> = BTreeMap::new();
        for doc in &documents {
            if !doc.is_public_page() {
                continue;
            }
            let preview_text = crate::build::page::meta::extract_preview(
                &doc.content,
                PREVIEW_MAX_CHARS,
                site_config.math,
            );
            let mut entry = serde_json::Map::new();
            // Hover-preview tooltips are chrome; use the plain-text label.
            entry.insert("title".into(), serde_json::Value::String(doc.label.clone()));
            if let Some(ref desc) = doc.description {
                if !desc.is_empty() {
                    let clean_desc = crate::build::page::meta::strip_markdown_inline(desc);
                    entry.insert("description".into(), serde_json::Value::String(clean_desc));
                }
            }
            if !preview_text.is_empty() {
                entry.insert("preview".into(), serde_json::Value::String(preview_text));
            }
            let permalink = format!("/{}", doc.url_path.trim_start_matches('/')); // allow:served-path-url-construct (page permalink key for previews.json index, not an HTML-emitted URL)
            let pretty = crate::build::scan::article_map::to_pretty_url(&permalink);
            let pretty_url = format!("/{}", pretty.trim_start_matches('/')); // allow:served-path-url-construct (pretty URL key for previews.json index, derived from permalink)
            previews.insert(pretty_url, serde_json::Value::Object(entry));
        }
        // Site 14c (Pattern A): emit _moss/previews.json (conditional on serialization success).
        if let Ok(json) = serde_json::to_string(&previews) {
            if let Err(e) = BuildContext::for_render(output_dir, pending)
                .emit_held(&ServedPath::for_previews_manifest(), json.into_bytes(), HashBucket::Files)
            {
                log::warn!("Failed to emit _moss/previews.json: {}", e);
            }
        }
    }

    // Site 16 (Pattern B): emit subscribe landing pages for moss-deployed sites.
    //
    // Seta's /confirm/:token 302-redirects to {siteUrl}/subscribe/confirmed after
    // double-opt-in, so the site always needs these two pages. Emitted for every
    // moss-deployed site regardless of [channels.email] (subscribe form and seta
    // POST /sites/:id/subscribe share the same gate). Pages are <1KB each,
    // noindex/nofollow, harmless on sites that never use them.
    //
    // User-override guard: skip any path whose url_path already appears in the
    // rendered documents list. That means the author created their own source file
    // (content/subscribe/confirmed.md or folder-style equivalent) whose output
    // lands at the same rel_path — their version wins. Checking the documents list
    // is more accurate than a filesystem probe because it covers all source
    // conventions (filename, folder/index.md, slug overrides, etc.).
    //
    // Hash format: xxHash3 via ctx.emit (was sha2::Sha256 in the pipeline.rs version).
    // One-time deploy diff for subscribe/{confirmed,expired}/index.html — this is
    // an expected, documented behavior.
    {
        let domain_cfg =
            crate::build::site_config::get_domain_config(source_path).unwrap_or_default();
        if domain_cfg.site_id.is_some() {
            for (rel_path, html) in
                crate::build::features::email::render_subscribe_landing_pages(site_lang, &css_version)
            {
                // User override: skip if a user-authored document already occupies this URL.
                let already_emitted_by_user =
                    documents.iter().any(|d| d.url_path == rel_path);
                if already_emitted_by_user {
                    continue;
                }
                let subscribe_sp = ServedPath::from_source(&rel_path)
                    .map_err(|e| format!("Failed to construct subscribe page path: {}", e))?;
                BuildContext::for_render(output_dir, pending)
                    .emit(&subscribe_sp, html.as_bytes(), HashBucket::Files)
                    .map_err(|e| format!("Failed to emit subscribe landing page: {}", e))?;
            }
        }
    }
    log::debug!(target: "timing", "[render] prelude: article_map+inventory+redirects+previews+subscribe: {:?}", total_start.elapsed());

    send_progress(
        progress_sender,
        "generating",
        &crate::infra::app_advisory::t("converting_videos"),
        75,
        false,
        None,
        None,
    );

    // Reset before ANY producer registers: the app shares one process-lifetime
    // `Arc`, and a stale `Failed` strips a live `<source>` in build.rs's tail.
    if let Some(registry) = services.and_then(|s| s.assets.as_deref()) {
        registry.clear();
    }

    // Non-blocking video conversion:
    // Collect video items for background processing instead of converting inline.
    // Videos will be converted in a background task AFTER the preview opens.
    let video_items = {
        use crate::build::media::ffmpeg::collect_videos_for_conversion;
        let items = collect_videos_for_conversion(project_structure);
        if !items.is_empty() {
            log::info!(
                "📹 Found {} video(s) for background conversion",
                items.len()
            );

            // Populate AssetRegistry with pending assets for placeholder support.
            // This allows the preview server to serve SVG placeholders while videos convert
            if let Some(registry) = services.and_then(|s| s.assets.as_deref()) {
                for item in &items {
                    // Map source path through dir_overrides, then derive mp4 key.
                    // This ensures registry keys match HTML <video src> paths.
                    let mapped = resolve_path_with_overrides(item, &dir_overrides);
                    let mp4_path = moss_core::asset_paths::to_mp4(&mapped);

                    // Get dimensions and dominant_color from project_structure.video_files
                    let video_meta = project_structure.video_files.iter()
                        .find(|v| &v.path == item);

                    let dimensions = video_meta.and_then(|m| m.dimensions);
                    let dominant_color = video_meta.and_then(|m| m.dominant_color.clone());

                    // Bug 1 — source passthrough: map BOTH the emitted
                    // .mp4 URL and the source's own resolved URL to the
                    // ABSOLUTE source (.mov/.mp4) so the preview server can
                    // serve the playable original (Range-aware) at the .mp4
                    // URL while the transcode runs in the background. The
                    // source path is relative to the project root; join it
                    // the same way run_video_conversion does.
                    let source_abs = std::path::Path::new(source_path).join(item);
                    registry.set_source_passthrough(mp4_path.clone(), source_abs.clone());
                    registry.set_source_passthrough(mapped.clone(), source_abs);

                    registry.set_pending(mp4_path, dimensions, dominant_color);
                }
            }
        }
        items
    };
    log::debug!(target: "timing", "[render] prelude: video_items collected: {:?}", total_start.elapsed());

    // Generate media collection pages (photography, videos, experiments)
    {
        use crate::build::media_collection::{aggregate_media_collections, generate_media_page, MediaType};

        let media_collections = aggregate_media_collections(&documents);

        // Get homepage info for consistent navigation
        let homepage_doc = project_structure
            .homepage_file
            .as_ref()
            .and_then(|_| documents.iter().find(|d| d.url_path == "index.html"));

        let media_site_title = homepage_doc
            .map(|d| d.title.clone())
            .unwrap_or_else(|| crate::i18n::t(site_lang, "site").to_string());

        // Generate navigation HTML for media pages
        let nav_builder = NavigationBuilder::new(
            &documents,
            &media_site_title,
            None,
            site_lang,
            project_structure.has_content_folders,
        )
        .with_search(layout_config.assets.search);
        let nav_builder = if let Some(ref logo) = layout_config.logo_path {
            nav_builder.with_logo(logo.clone())
        } else {
            nav_builder
        };

        let nav_html = nav_builder.generate_navigation();

        for (media_type, items) in &media_collections {
            if items.is_empty() {
                continue;
            }

            let page_slug = match media_type {
                MediaType::Photography => "photography",
                MediaType::Video => "videos",
                MediaType::Interactive => "experiments",
            };

            let media_path_resolver = PathResolver::new().with_css_version(&css_version).with_js_version(&js_version).with_dir_overrides(dir_overrides.clone());
            let fullscreen_url;
            // `media_pages` (folded from `documents` before any rendering), NOT
            // the loop's own `media_pages_generated` accumulator, which is
            // still false on the first iteration — the first media collection
            // page used to render with no fullscreen.js and a dead lightbox.
            let js_fullscreen_path_arg: Option<&str> = if media_pages {
                fullscreen_url = ServedPath::for_runtime_js_hashed("fullscreen", &fullscreen_hash)
                    .unwrap()
                    .to_relative_url();
                Some(fullscreen_url.as_str())
            } else {
                None
            };
            let page_html = generate_media_page(
                media_type,
                items,
                &media_site_title,
                &nav_html,
                &media_path_resolver.css_path(),
                site_lang,
                &layout_config.lang_tag,
                Some(&event_level_image_lookup),
                &media_path_resolver.js_path(),
                &media_path_resolver.lazy_chunk_attrs(&share_card_hash, hls_attr_hash),
                js_fullscreen_path_arg,
                &scripts.tag("search", &layout_config.assets, &media_path_resolver),
            );

            // Create directory and write page
            let page_dir = output_dir.join(page_slug);
            crate::build::io_utils::create_output_dir_all(&page_dir)
                .map_err(|e| format!("Failed to create {} directory: {}", page_slug, e))?;

            // Site 14d (Pattern A): emit {page_slug}/index.html (media pages loop).
            // ctx.emit creates parent dirs, writes, and registers in pending (SHA-256).
            let page_url = format!("{}/index.html", page_slug);
            let page_sp = ServedPath::from_source(&page_url)
                .map_err(|e| format!("Failed to construct media page path: {}", e))?;
            BuildContext::for_render(output_dir, pending)
                .emit(&page_sp, page_html.as_bytes(), HashBucket::Files)
                .map_err(|e| format!("Failed to emit {} page: {}", page_slug, e))?;
            sitemap_pages.insert(page_url);

            page_count += 1;
            log::info!(
                "✅ Generated /{}/index.html with {} items",
                page_slug,
                items.len()
            );
        }

        // fullscreen.js is emitted with every other script, from the
        // `emit::scripts` table, gated on the same `media_pages` fact this
        // loop's `<script>` tag reads.
    }
    log::debug!(target: "timing", "[render] prelude: media collection pages: {:?}", total_start.elapsed());

    // Every runtime script this build ships, from the registration table.
    // Each file's gate is the same fact its `<script>` tag reads, so a tag can
    // never name a file that was skipped. Replaces seven hand-rolled
    // `if gate { emit }` blocks; see `build::emit::scripts`.
    scripts.emit(&site_assets, output_dir, pending)?;

    // Email/RSS math PNG projection. This is only the GATE
    // (the fullscreen.js conditional-emission precedent above) — the emission
    // itself lives emit-shaped in `build::emit::math_png`, never in this file.
    // Runs even with `[site].math = false`: the retention half re-registers
    // PNGs already referenced by sent emails (append-only; rasterization is
    // gated on `math_enabled` inside).
    crate::build::emit::math_png::emit_math_pngs(&documents, site_config.math, output_dir, pending)
        .map_err(|e| format!("Failed to emit math PNGs: {}", e))?;
    log::debug!(target: "timing", "[render] prelude: scripts+math_png: {:?}", total_start.elapsed());

    send_progress(
        progress_sender,
        "generating",
        &crate::infra::app_advisory::t("generating_media_pages"),
        92,
        false,
        None,
        None,
    );

    // Stale cleanup: deferred to background (runs LAST after all assets placed).
    // Running it here would delete deferred assets that haven't been copied yet.

    // Debug-only safety net: every ParsedDocument with a source_path AND an
    // emitted HTML output should have a source_to_output mapping. If this
    // ever fires, an HTML emission path was added without a paired
    // `pending.register_source_mapping(...)` call — that file's renames will
    // silently fall through to redirect-home. The two known emission sites
    // (main HTML write loop ~line 1067 and homepage emission ~line 1751)
    // both register; this asserts no NEW emission site has been added that
    // forgets to. No-op in release builds.
    #[cfg(debug_assertions)]
    {
        let s2o = pending.source_to_output();
        let files = pending.files();
        for doc in &documents {
            if let Some(src) = doc.source_path.as_ref() {
                if files.contains_key(&doc.url_path) && !s2o.contains_key(src) {
                    log::warn!(
                        "rename-event-propagation safety net: ParsedDocument with \
                         source_path={:?} and emitted url_path={:?} is missing \
                         from source_to_output. An HTML emission path was added \
                         without calling pending.register_source_mapping; \
                         in-app renames of this file will fall through to \
                         redirect-home.",
                        src, doc.url_path
                    );
                }
            }
        }
    }

    // Every page this build could not read keeps the HTML it published last
    // time. Without this the mark-and-sweep reads "emitted nothing" as
    // "deleted" and three later passes remove the page — and since
    // `sealed.files()` is what deploy uploads, publishing during an eviction
    // would un-publish a live page. See
    // `PendingManifest::carry_forward_deferred_page`.
    //
    // Both eviction forms are covered, and they arrive by different routes:
    // Sonoma+ leaves the real name in the walk, so the render pass tried it and
    // recorded it in `deferred_paths`; macOS 12-13 replaces the file with a
    // hidden `.name.icloud` sibling, so the render pass never saw the page at
    // all and only the scan's `evicted_paths` knows about it.
    {
        let root_path = Path::new(source_path);
        let deferred_sources = deferred_paths
            .iter()
            .chain(project_structure.evicted_paths.iter())
            .filter_map(|p| p.strip_prefix(root_path).ok())
            .map(|rel| moss_core::slug::normalize_separators(&rel.to_string_lossy()))
            .collect::<std::collections::BTreeSet<_>>();
        // Fires per evicted page on every build. One INFO count; paths at DEBUG.
        let mut carried = 0usize;
        for src in deferred_sources {
            if let Some(key) = pending.carry_forward_deferred_page(&src) {
                // The mapping is reinstated above; its hash has to come with it
                // or the page reads as deleted at the next publish.
                register_page_source(pending, &page_source_hashes, &src);
                carried += 1;
                log::debug!("[build] {} is still in the cloud — keeping its published {}", src, key);
                sitemap_pages.insert(key);
            }
        }
        if carried > 0 { log::info!("[build] {} page(s) still in the cloud — keeping their published output", carried); }
    }

    if has_rss {
        // Static `.html` pages and notebook viewers are written after this, by
        // the asset walk and the notebook step, at served paths the scan
        // already fixes.
        sitemap_pages.extend(project_structure.html_files.iter().filter_map(|f| {
            let sp = ServedPath::from_source(&resolve_path_with_overrides(&f.path, &dir_overrides)).ok()?;
            sp.as_str().ends_with(".html").then(|| sp.as_str().to_string())
        }));
        sitemap_pages.extend(
            project_structure.notebook_files.iter()
                .filter_map(|f| crate::build::notebook::viewer_path(&f.path).ok())
                .map(|sp| sp.as_str().to_string()),
        );
        let entries = crate::build::feeds::sitemap::entries_for_pages(&sitemap_pages, &documents);
        let sitemap_content = crate::build::feeds::sitemap::generate_sitemap(&entries, site_url.as_str());
        // Site 11 (Pattern A): emit sitemap.xml.
        BuildContext::for_render(output_dir, pending)
            .emit_held(&ServedPath::for_sitemap(), sitemap_content.into_bytes(), HashBucket::Files)
            .map_err(|e| format!("Failed to emit sitemap.xml: {}", e))?;
    }

    // Debug-only safety net for the publish classifier's lockstep invariant:
    // every source that got an output mapping also got a source hash. A page
    // with a mapping and no hash has nothing to diff against the last publish,
    // so it can only read as added or deleted — and on a watch-loop rebuild,
    // where most of a large site is carried rather than re-read, that would be
    // most of the site. Placed after the deferred carry-forward above so an
    // evicted page's carry counts. No-op in release builds.
    #[cfg(debug_assertions)]
    {
        let hashed = pending.page_sources();
        let missing: Vec<&String> = pending
            .source_to_output()
            .keys()
            .filter(|src| !hashed.contains(*src))
            .collect();
        if !missing.is_empty() {
            log::warn!(
                "publish-classification safety net: {} source(s) have an output \
                 mapping but no source hash (e.g. {:?}). An emission or carry \
                 path calls register_source_mapping without the paired \
                 register_page_source(...); those pages cannot be classified.",
                missing.len(),
                &missing[..missing.len().min(3)]
            );
        }
    }

    // Decompose pending into (SiteHashes, blocking_keys) for BackgroundContext and SiteResult.
    // `as_parts_clone` clones because `pending` is `&mut` (not owned); the owning
    // form that would let the pipeline caller skip the clone never gained a caller
    // and was removed, so this is the only decomposition.
    let (site_hashes, blocking_keys) = pending.as_parts_clone();

    // `hashes.json` is NOT written here. The seal+persist task in `build.rs`
    // is the sole writer — it runs after deferred phases (image, video,
    // notebook) finish emitting and `PendingManifest::seal` has applied
    // mark-and-sweep pruning to the four output buckets. Writing an
    // intermediate manifest at this point would land a pre-sweep, pre-deferred
    // snapshot on disk; if anything reads it during the window before
    // seal+persist (or seal+persist fails midway), it would observe stale
    // entries and the deploy validation would trip. Confirmed-safe consumers
    // of `hashes.json` during this window: deferred-phase mtime check
    // (file-missing → mtime=0 → safe re-hash); `load_previous_hashes` (next
    // build start only, gated by `FolderSession`); `push_site_inner` (waits
    // for seal+persist via `resolve_sealed_manifest_for_deploy`).
    send_progress(
        progress_sender,
        "generating",
        &crate::infra::app_advisory::t("finalizing"),
        95,
        false,
        None,
        None,
    );

    let site_title = project_structure
        .homepage_file
        .clone()
        .or_else(|| documents.first().map(|d| d.title.clone()))
        .unwrap_or_else(|| crate::i18n::t(site_lang, "untitled_site").to_string());

    log::debug!(target: "timing", "[render] total: {:?}", total_start.elapsed());

    // Build site-level meta defaults for bundled-SPA `<head>` injection
    // (link-preview-defaults plan, Phase 5). The injector preserves anything
    // the SPA author already set, so we always pass these defaults — the SPA
    // wins on conflict.
    //
    // Sources:
    // * description / title  — homepage frontmatter
    // * og:* / twitter:*     — pre-rendered with the homepage's title+description
    // * theme-color          — matches default.css `--moss-color-bg` light/dark
    // * canonical_url        — derived per-folder at injection time from `site_host`
    // * raster_favicons      — `<link rel="icon">` to the same favicon the moss
    //                           templates emit (user's `assets/favicon.{svg,png,ico}`,
    //                           or moss's default logo SVG when none was provided).
    //                           Without this, bundled SPAs fall back to the browser's
    //                           tab default — a real site's bundled SPA shipped without
    //                           a favicon link for exactly this reason.
    // * apple-touch-icon     — left None for now; needs raster PNG sizes (TODO 4.3c).
    let spa_defaults = {
        use crate::build::page::meta::{build_og_tags_website, build_twitter_tags};
        use crate::build::site_meta::spa_inject::SpaDefaultsOwned;

        let homepage_doc = documents.iter().find(|d| d.url_path == "index.html");
        let homepage_description: Option<String> = homepage_doc
            .and_then(|d| d.description.clone())
            .filter(|s| !s.trim().is_empty());
        // og:title routes through the SAME structural decision the static
        // `<title>` uses — `home::site_name` keys off the home filename
        // stem, so a no-`title:` root index yields the folder name here too
        // (previously this path used `doc.title` UNFILTERED and could leak the
        // stem). `<title>` and `og:title` now agree on every path.
        let folder_name = root
            .name_opt()
            .unwrap_or_else(|| crate::i18n::t(site_lang, "site"));
        let homepage_title: String = moss_core::home::site_name(
            project_structure.homepage_file.as_deref(),
            homepage_doc.map(|d| d.title.as_str()),
            folder_name,
        );

        // Extract host for the canonical URL helper. SiteUrl.host() strips the
        // scheme; we strip a possible trailing slash too. Gated on the resolved
        // URL being https — localhost builds (http://localhost...) skip
        // canonical + og:url emission entirely, matching the has_rss gate that
        // skips feed generation for unconfigured sites. Per the design spec,
        // an unconfigured site shipping localhost URLs in canonical/og:url
        // would be useless noise; absence of those tags is the cleaner signal.
        let site_host: Option<String> = if site_url.is_deployed() {
            Some(site_url.host().trim_end_matches('/').to_string())
        } else {
            None
        };

        // For og:url + canonical, use the site root URL (the SPA folder URL is
        // applied per-injection in `as_borrowed`). og:image is left None for
        // now — Phase 4.3 wires the auto-generated card.
        let og_tags = if let Some(ref host) = site_host {
            let site_root_url = format!("https://{}/", host.trim_start_matches("www."));
            Some(build_og_tags_website(
                &homepage_title,
                &site_title,
                homepage_description.as_deref().unwrap_or(""),
                &site_root_url,
                None,
                None,
                "", // locale; empty skips og:locale
                &site_url,
            ))
        } else {
            None
        };
        let twitter_tags = Some(build_twitter_tags(
            &homepage_title,
            homepage_description.as_deref().unwrap_or(""),
            None,
            None,
            &site_url,
        ));

        // Build the favicon `<link>` tag(s) from the resolved favicon. When the
        // favicon is SVG, we rasterized PNG sizes 16/32 — emit those alongside
        // the SVG so older browsers and email/messaging clients (which
        // sometimes don't follow `image/svg+xml`) still get a real icon.
        // Without this, bundled SPAs fall back to /favicon.ico (usually 404 →
        // blank tab icon). The apple-touch-icon (180x180 PNG) is emitted
        // separately below.
        let raster_favicons: Option<String> = {
            let mime = match Path::new(&favicon_filename).extension().and_then(|e| e.to_str()) {
                Some("svg") => "image/svg+xml",
                Some("png") => "image/png",
                Some("ico") => "image/x-icon",
                _ => "image/svg+xml",
            };
            let mut tags = format!(
                r#"<link rel="icon" type="{}" href="/assets/{}">"#,
                mime, favicon_filename
            );
            if favicon_has_raster_pngs {
                tags.push('\n');
                tags.push_str(
                    r#"<link rel="icon" type="image/png" sizes="32x32" href="/assets/favicon-32.png">"#,
                );
                tags.push('\n');
                tags.push_str(
                    r#"<link rel="icon" type="image/png" sizes="16x16" href="/assets/favicon-16.png">"#,
                );
            }
            Some(tags)
        };
        let apple_touch_icon: Option<String> = if favicon_has_raster_pngs {
            Some(
                r#"<link rel="apple-touch-icon" sizes="180x180" href="/assets/favicon-180.png">"#
                    .to_string(),
            )
        } else {
            None
        };

        // Derive theme-color values from the embedded tokens.json (--moss-color-bg)
        // so the SPA meta stays in sync with the CSS; avoids stale hardcoded literals.
        let (tc_light, tc_dark) = {
            use moss_core::contract::tokens::{bg_colors, load_tokens};
            let t = load_tokens().unwrap_or_else(|e| panic!("tokens.json must parse: {}", e));
            let (l, d) = bg_colors(&t);
            (l.to_owned(), d.to_owned())
        };
        let owned = SpaDefaultsOwned {
            description: homepage_description,
            og_tags,
            twitter_tags,
            apple_touch_icon,
            raster_favicons,
            theme_color_light: Some(tc_light),
            theme_color_dark: Some(tc_dark),
            site_host,
            site_www_canonical: false,
        };
        if owned.has_anything() { Some(owned) } else { None }
    };

    // Collect images that need WebP conversion, mirroring `video_items`.
    // Loads a HashIndex here so `collect_images_for_conversion` can resolve
    // `source_oid` cheaply (stat-match hit) without re-hashing the file.
    // Also returns the rung-collision map (built from ProjectStructure,
    // which the background worker doesn't have) for BackgroundContext.
    let (image_items, rung_collisions) = {
        use crate::build::cache::{HashIndex, TransformCache};
        use crate::build::image::{collect_images_for_conversion, ImageCompressionConfig};

        let hash_index_path = paths.cache_hash_index();
        let mut hash_index = HashIndex::load(&hash_index_path);
        let transforms = TransformCache::for_site(&paths);
        let config = ImageCompressionConfig::default();
        let items = collect_images_for_conversion(
            project_structure,
            &transforms,
            &mut hash_index,
            &config,
        );
        if !items.is_empty() {
            log::info!(
                "🖼  Found {} image(s) for background WebP conversion",
                items.len()
            );
        }

        // Collision guard (design § Guards): dir-override-mapped paths of
        // every vault image file → the user's absolute source path, built
        // ONCE (not an O(n) scan per rung). A user's real file named
        // exactly like a generated rung (photo.w800.webp beside photo.jpg)
        // wins — never clobber user content. We skip that rung's
        // registration; the emitted srcset candidate stays live because
        // ServeDir/deploy serves the user's actual file
        // (wrong-image-but-200, warned below). Render-side emission is NOT
        // suppressed: render cannot see collisions and must not
        // (deterministic-agreement contract).
        //
        // Hoisted ABOVE the services gate: the map also rides
        // `BackgroundContext` to the encode worker, which must respect
        // collisions in HEADLESS builds too (no services/registry there) —
        // recomputing it in the worker is forbidden (no ProjectStructure;
        // divergence ⇒ registered-but-never-encoded rung ⇒ sealed-deploy
        // 404).
        let rung_collisions = crate::build::media::rungs::rung_collision_map(
            project_structure,
            &dir_overrides,
        );

        // Every variant URL the synthesizer will emit is promised before first
        // paint, and a source this build cannot encode is settled instead:
        // `media/promise.rs`.
        let items = crate::build::media::promise::promise_image_variants(
            services.and_then(|s| s.assets.as_deref()),
            items,
            project_structure,
            &dir_overrides,
            &rung_collisions,
            source_path,
        );

        (items, rung_collisions)
    };

    // Create BackgroundContext with video items for async processing.
    // canonical_dir is set to None here; build.rs sets it during rebuilds.
    // Thread ffmpeg_bin_path from scan so build.rs doesn't re-download
    let background_ctx = BackgroundContext {
        video_items,
        image_items,
        source_path: source_path.to_string(),
        staging_dir: output_dir.to_path_buf(),
        moss_dir: moss_dir.clone(),
        previous_hashes: previous_hashes.clone(),
        start_time: std::time::Instant::now(),
        dir_overrides,
        blocking_keys,
        spa_defaults,
        passthrough_roots: project_structure.passthrough_roots.clone(),
        search_enabled: layout_config.assets.search,
        ffmpeg_bin_path: project_structure.ffmpeg_bin_path.clone(),
        notebook_files: project_structure.notebook_files.clone(),
        rung_collisions,
        carried_advisories: Vec::new(),
    };

    Ok((
        SiteResult {
            page_count,
            site_build_dir: output_dir.to_string_lossy().to_string(),
            site_title,
            hashes: site_hashes,
            deferred_paths,
            missing_media,
        },
        background_ctx,
        // Surface the parsed page slice to the caller so
        // post-build native-slot generation (`generate_native_slots` in
        // `build.rs`) can consult typed `features.inline_subscribe` flags
        // and pick up slot-only files (root `footer.md`) through the same
        // pipeline the rest of the site uses. Replaces the prior
        // filesystem-scan hacks `project_has_inline_subscribe` and
        // `render_footer_pages_from_disk`.
        documents,
        // Shadow-verification snapshots. `None` unless
        // `MOSS_INCREMENTAL_VERIFY=1`; the comparison belongs to the caller,
        // which is the only place that has run the enhance hook.
        carry_verification,
    ))
}

/// Outcome of [`resolve_favicon`]: tells the caller which file landed at
/// `assets/<filename>` and lets it register the file in the build's hashes
/// manifest for stale-cleanup.
pub(super) struct ResolvedFavicon {
    pub filename: String,
    pub relative_path: String,
    pub content_hash: String,
    /// True when no user favicon was found and the moss-logo default was used.
    /// Locked in by tests so a refactor can't silently disable the fallback.
    #[cfg_attr(not(test), allow(dead_code))]
    pub used_default: bool,
    /// True when the favicon source was an SVG and we successfully rasterized
    /// PNG sizes (16/32/180) under `assets/favicon-<size>.png`. False for
    /// .png/.ico user favicons (we don't upscale rasters) or when
    /// rasterization failed (logged warning, SVG still works).
    pub has_raster_pngs: bool,
}

/// Resolve the site's favicon: copy the user's `assets/favicon.{svg,png,ico}`
/// if present, otherwise write moss's default logo as `assets/favicon.svg`.
///
/// When the resolved favicon is an SVG (user-provided or moss default), also
/// rasterize PNG sizes 16/32/180 into the same `assets/` folder. The 180px
/// PNG doubles as the apple-touch-icon. .png/.ico user favicons are passed
/// through unchanged (no upscaling).
///
/// Returns the filename (relative to `assets/`) for use by the template,
/// plus the relative path and content hash so the caller can register the
/// file in the build's hashes manifest.
pub(super) fn resolve_favicon(
    source_path: &Path,
    output_dir: &Path,
) -> Result<ResolvedFavicon, String> {
    let assets_dir = source_path.join("assets");
    let favicon_dest_dir = output_dir.join("assets");
    crate::build::io_utils::create_output_dir_all(&favicon_dest_dir)
        .map_err(|e| format!("Failed to create assets directory: {}", e))?;

    let candidates = ["favicon.svg", "favicon.png", "favicon.ico"];
    let user_favicon = candidates
        .iter()
        .find(|name| assets_dir.join(name).exists())
        .map(|s| s.to_string());

    // Read it here, because an unreadable favicon must not fail the build.
    // This runs *before* the cloud gate is computed, so a hard error returns
    // Err from the whole pipeline and no gate event is emitted at all — the
    // user would get a build failure instead of the "waiting for cloud sync"
    // screen, over a few bytes of decoration. Both offline forms fall through
    // to the default; the supervisor's arrival detection rebuilds with the
    // real one.
    let user_favicon: Option<(String, Vec<u8>)> = user_favicon.and_then(|name| {
        let path = assets_dir.join(&name);
        let defer = |reason: &str| {
            crate::build::cloud_readiness::request_download(&path);
            log::warn!("favicon {} is {} — using the default until it arrives", name, reason);
            None
        };
        if crate::build::icloud::is_evicted(&path) {
            return defer("still in the cloud");
        }
        match fs::read(&path) {
            Ok(bytes) => Some((name.clone(), bytes)),
            Err(e) if crate::build::icloud::is_offline_not_absent(&path, &e) => {
                defer("unreadable but not gone")
            }
            Err(e) => {
                log::warn!("favicon {} could not be read ({}) — using the default", name, e);
                None
            }
        }
    });

    // Both writes go through `io_utils`. The default-favicon
    // write below is the pipeline's FIRST output write, which is why an evicted
    // `.moss/build.nosync/` surfaced here first: it failed `EDEADLK`, the
    // `?` returned from the whole pipeline, and the cloud gate four hundred
    // lines below never got to speak. The read above already guarded that
    // hazard for the user's favicon; the write did not.
    let (filename, relative_path, content_hash, used_default) = if let Some((name, bytes)) = user_favicon {
        crate::build::io_utils::write_output(&favicon_dest_dir.join(&name), &bytes)
            .map_err(|e| format!("Failed to write favicon: {}", e))?;
        let hash = compute_content_hash(&String::from_utf8_lossy(&bytes));
        let rel = format!("assets/{}", name);
        (name, rel, hash, false)
    } else {
        // Tab-only crop, applied only in this written copy — see
        // `tighten_default_favicon_viewbox`'s doc comment for why.
        let tight_favicon = crate::build::site_meta::favicon::tighten_default_favicon_viewbox(crate::build::page::shell::DEFAULT_FAVICON);
        crate::build::io_utils::write_output(&favicon_dest_dir.join("favicon.svg"), tight_favicon.as_bytes())
            .map_err(|e| format!("Failed to write default favicon: {}", e))?;
        let hash = compute_content_hash(&tight_favicon);
        (
            "favicon.svg".to_string(),
            "assets/favicon.svg".to_string(),
            hash,
            true,
        )
    };

    // When the favicon we just wrote is an SVG, rasterize PNG sizes for legacy
    // browsers, the apple-touch-icon, and bundled-SPA `<link rel="icon">` tags.
    // Fail-open: if rasterization fails (bad SVG, OOM, etc.) we log a warning
    // and continue with just the SVG.
    let has_raster_pngs = if filename == "favicon.svg" {
        let svg_source = favicon_dest_dir.join("favicon.svg");
        match crate::build::site_meta::favicon::generate_favicons(
            &svg_source,
            &favicon_dest_dir,
        ) {
            Ok(_) => true,
            Err(e) => {
                log::warn!(
                    "Failed to rasterize favicon PNGs: {}. Continuing with SVG only.",
                    e
                );
                false
            }
        }
    } else {
        // .png/.ico user favicons already serve as the raster fallback.
        false
    };

    // A raster trio left by an earlier default-SVG build is not removed here:
    // the pages read `has_raster_pngs`, never the directory, so a leftover
    // file ships no `<link>` tag (2026-09-14), and the permitted
    // staging sweep removes it once no manifest names it.

    Ok(ResolvedFavicon {
        filename,
        relative_path,
        content_hash,
        used_default,
        has_raster_pngs,
    })
}

#[cfg(test)]
mod favicon_tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn no_user_favicon_writes_moss_logo() {
        let source = TempDir::new().expect("source tempdir");
        let output = TempDir::new().expect("output tempdir");
        // Source has no assets/favicon.* — simulate a fresh site.
        fs::create_dir_all(source.path().join("assets")).unwrap();

        let result = resolve_favicon(source.path(), output.path()).expect("resolve");

        assert!(result.used_default, "should fall back to moss logo when no user favicon");
        assert_eq!(result.filename, "favicon.svg");

        let written = fs::read_to_string(output.path().join("assets/favicon.svg")).expect("read written");
        assert!(written.contains("<svg"), "moss logo should be SVG");
        // The favicon-emitted copy is DEFAULT_FAVICON with its viewBox
        // tightened to the ink bbox (moss's 2026-09-14 fix) — not the
        // padded box every other mark consumer expects. See
        // `favicon::tighten_default_favicon_viewbox`.
        assert_eq!(
            written,
            crate::build::site_meta::favicon::tighten_default_favicon_viewbox(
                crate::build::page::shell::DEFAULT_FAVICON
            ),
            "fallback should write DEFAULT_FAVICON with a tightened viewBox, path data untouched"
        );
        assert_ne!(
            written,
            crate::build::page::shell::DEFAULT_FAVICON,
            "the emitted favicon must not carry the padded default viewBox verbatim"
        );
    }

    #[test]
    fn user_svg_favicon_takes_priority_over_moss_logo() {
        let source = TempDir::new().expect("source tempdir");
        let output = TempDir::new().expect("output tempdir");
        let assets = source.path().join("assets");
        fs::create_dir_all(&assets).unwrap();
        let user_svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><circle cx="5" cy="5" r="4"/></svg>"#;
        fs::write(assets.join("favicon.svg"), user_svg).unwrap();

        let result = resolve_favicon(source.path(), output.path()).expect("resolve");

        assert!(!result.used_default, "user favicon should win over default");
        assert_eq!(result.filename, "favicon.svg");

        let written = fs::read_to_string(output.path().join("assets/favicon.svg")).expect("read");
        assert_eq!(written, user_svg, "should copy the user's favicon bytes verbatim");
    }

    #[test]
    fn user_png_favicon_takes_priority() {
        let source = TempDir::new().expect("source tempdir");
        let output = TempDir::new().expect("output tempdir");
        let assets = source.path().join("assets");
        fs::create_dir_all(&assets).unwrap();
        // Minimal PNG signature; content doesn't need to be a real image for this test.
        let user_png = b"\x89PNG\r\n\x1a\nfake-png-bytes";
        fs::write(assets.join("favicon.png"), user_png).unwrap();

        let result = resolve_favicon(source.path(), output.path()).expect("resolve");

        assert!(!result.used_default);
        assert_eq!(result.filename, "favicon.png");
    }
}
