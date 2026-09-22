//! Markdown processing pipeline.
//!
//! Covers: `process_markdown_file` (the typed-AST production path),
//! `render_markdown_to_html` + `render_markdown_to_html_with` (fragment
//! renderers, also typed-AST since PR7a-fragment, 2026-05-28), URL
//! classification (`classify_url_prod`), URL prettification
//! (`transform_markdown_link`), and private path helpers
//! (`split_path_suffix`, `relative_pretty_url`, `url_path_to_dir`).
//!
//! # Pre-PR7a-fragment architecture (deleted 2026-05-28)
//!
//! Pre-PR7a-fragment, this module also owned `transform_events` (~860
//! LOC) — the legacy pulldown-cmark event-pipeline that did heading-ID
//! assignment, image-caption folding, `data-source-line` injection,
//! wikilink dispatch, and figure promotion via event-window pattern
//! matching. PR7a-fragment routes the last caller
//! (`render_markdown_to_html_with`) through `moss_core::ast::parse` +
//! `render_document`; the AST renderer is now the sole markdown-to-HTML
//! path in moss.

use crate::build::types::ParsedDocument;
use moss_core::PageKind;

use std::collections::HashMap;
use std::path::Path;

use super::frontmatter::{
    compute_url_path, is_simplified_frontmatter, parse_simplified_frontmatter,
};
use super::html_post::{
    accept_criticmarkup, adjust_relative_paths_for_pretty_urls, inject_article_title_h1,
    prepare_markdown_source, resolve_to_root_relative, strip_percent_comments,
};

/// Reproduce gray_matter's body normalization byte-for-byte (ADR-020 Phase 2b).
///
/// The traditional-YAML branch historically took the markdown body from
/// `gray_matter`'s `result.content`. gray_matter (0.2.9) builds the body by
/// re-emitting each line prefixed with `\n` and then calling
/// `trim_start_matches('\n')` — the net effect is:
///   * leading blank lines are removed, and
///   * the trailing newline is dropped (the line-join leaves no terminator).
/// Per-line content (including trailing spaces) is otherwise preserved verbatim.
///
/// `moss_core::frontmatter::parse` instead returns the verbatim slice after the
/// closing `---\n`, keeping both the leading and trailing newlines. To switch to
/// the unified parser without churning any snapshot, run that same transform on
/// `moss_core`'s body. `str::lines()` joined with `\n` reproduces gray_matter's
/// line walk (CRLF already normalized to LF by `moss_core::frontmatter::parse`),
/// and `trim_start_matches('\n')` drops the leading blanks.
///
/// FOLLOW-UP (ADR-020): the verbatim body is the *correct* output; this shim
/// only exists to keep the snapshot fixtures byte-stable across the gray_matter
/// removal. A future cleanup should regenerate the snapshots against the
/// verbatim body and delete this function — do NOT "simplify" the call site to
/// `parsed.body` without that regeneration, or ~200 fixtures will churn.
fn normalize_body_like_gray_matter(body: &str) -> String {
    let joined = body.lines().collect::<Vec<&str>>().join("\n");
    joined.trim_start_matches('\n').to_string()
}

/// Builds a [`moss_core::asset_snapshot::AssetSnapshot`] from src-tauri's
/// in-flight asset state. Phase 0 Task F1 — packages the data moss-core's
/// pure-Rust resolve/synthesize layers need (dimensions, LQIP, dominant
/// color, registered WebP/AVIF variants) into a single typed contract.
///
/// Phase 0 only **threads** the snapshot through the resolve pipeline;
/// nothing reads it yet. Phase 1 wires the consumption side in moss-core's
/// Stage 2 synthesizer. Mirrors the architectural shape of `ContentGraph`:
/// src-tauri does the I/O, moss-core takes typed data IN.
///
/// `MediaMaps` keys are `String` (the path as it appears in markdown/HTML),
/// converted to `PathBuf` at the iteration boundary. Taking the maps and not
/// the whole lookup is what lets this run before the lookup that owns it exists.
///
/// `AssetRegistry::iter_registered_variants` already produces a stem-keyed
/// `HashMap<PathBuf, VariantKindSet>` (per ADR-013 + the asset_snapshot
/// module docs), so its output is folded directly into `snapshot.variants`.
pub(crate) fn build_asset_snapshot(
    maps: &crate::build::media::dimensions::MediaMaps,
    registry: Option<&crate::types::assets::AssetRegistry>,
    dir_overrides: &std::collections::HashMap<String, String>,
) -> moss_core::asset_snapshot::AssetSnapshot {
    use moss_core::asset_snapshot::AssetSnapshot;
    use std::path::PathBuf;

    let mut snapshot = AssetSnapshot::new();

    // BUG 6: dims/lqip/color are keyed here by the RAW source path (as it
    // appears in scan / markdown), but the synthesizer probes by the
    // TRANSFORMED output URL — covers arrive slugified (`/assets/europe-a-
    // prophecy/...`). Without a matching key the probe misses and the 800x600
    // fallback fires for EVERY image. To fix, ADDITIVELY index each entry a
    // SECOND time under its slugified output-URL key (via
    // `resolve_path_with_overrides`, which slugifies intermediate dir segments
    // and preserves the leaf). Even an empty `dir_overrides` yields the base
    // slug (`assets/Europe - A Prophecy/...` → `assets/europe-a-prophecy/...`).
    //
    // STRICTLY ADDITIVE: only insert the slug key when it DIFFERS from the
    // source key AND is not already present — never overwrite the source key,
    // because the percent-decode probe (image.rs `probe_paths`) reverses
    // encoding back to that raw source key.
    //
    // DETERMINISM: two DISTINCT raw source keys can slugify to the SAME
    // output-URL key (e.g. `assets/My Photos/x.jpg` and `assets/My-Photos/x.jpg`
    // both → `assets/my-photos/x.jpg`). Because `.or_insert` keeps whichever
    // colliding source is visited FIRST and the source is a `HashMap` (random
    // SipHash iteration order), a naive iteration would emit non-deterministic
    // `<img width/height>` / lqip / color across builds — a build-determinism
    // regression that could flake CLI snapshot fixtures. Each loop below
    // therefore iterates its source keys in SORTED order, so on a colliding
    // slug key the lexicographically-smallest raw source key deterministically
    // wins (`.or_insert` keeps the first-visited = smallest). A real source key
    // that equals the slug always wins regardless (direct `.insert` is
    // unconditional; see the loops below).
    fn insert_slug<V>(
        map: &mut std::collections::HashMap<PathBuf, V>,
        source_key: &str,
        value: V,
        dir_overrides: &std::collections::HashMap<String, String>,
    ) {
        let slug = moss_core::resolve::output_url::resolve_path_with_overrides(
            source_key,
            dir_overrides,
        );
        if slug != source_key {
            map.entry(PathBuf::from(slug)).or_insert(value);
        }
    }

    // Dimensions: String → PathBuf at the boundary. Sorted by source key so a
    // colliding slug key resolves deterministically (see `insert_slug` doc).
    let mut dim_entries: Vec<(&String, &(u32, u32))> = maps.dimensions.iter().collect();
    dim_entries.sort_by(|a, b| a.0.cmp(b.0));
    for (path, dims) in dim_entries {
        snapshot.dimensions.insert(PathBuf::from(path), *dims);
        insert_slug(&mut snapshot.dimensions, path, *dims, dir_overrides);
    }

    // Animated flag: keyed IDENTICALLY to `dimensions` (same raw source key +
    // same additive output-URL slug key) so Task 12's ladder gate can probe
    // `is_animated(src)` right beside `lookup_dims(src)` with one normalization.
    // Sorted by source key so a colliding slug picks the same deterministic
    // winner as `dimensions` (see `insert_slug` doc).
    let mut animated_entries: Vec<(&String, &bool)> = maps.animated.iter().collect();
    animated_entries.sort_by(|a, b| a.0.cmp(b.0));
    for (path, is_animated) in animated_entries {
        snapshot.animated.insert(PathBuf::from(path), *is_animated);
        insert_slug(&mut snapshot.animated, path, *is_animated, dir_overrides);
    }

    // LQIP: String → PathBuf at the boundary. Sorted (determinism, see above).
    let mut lqip_entries: Vec<(&String, &String)> = maps.lqips.iter().collect();
    lqip_entries.sort_by(|a, b| a.0.cmp(b.0));
    for (path, lqip) in lqip_entries {
        snapshot.lqip.insert(PathBuf::from(path), lqip.clone());
        insert_slug(&mut snapshot.lqip, path, lqip.clone(), dir_overrides);
    }

    // Dominant color: String → PathBuf at the boundary. Sorted (determinism).
    let mut color_entries: Vec<(&String, &String)> = maps.colors.iter().collect();
    color_entries.sort_by(|a, b| a.0.cmp(b.0));
    for (path, color) in color_entries {
        snapshot
            .dominant_color
            .insert(PathBuf::from(path), color.clone());
        insert_slug(
            &mut snapshot.dominant_color,
            path,
            color.clone(),
            dir_overrides,
        );
    }

    // Variants: already stem-keyed by AssetRegistry; fold the whole map.
    // Optional — `--no-plugins` / headless paths may not own a registry yet.
    if let Some(registry) = registry {
        for (stem, kinds) in registry.iter_registered_variants() {
            snapshot.variants.insert(stem, kinds);
        }
    }

    snapshot
}

/// Build the build-warning line for a document whose `---...---` frontmatter
/// block failed to parse as YAML, or `None` when it parsed cleanly (or had no
/// block). Pure so the build-side diagnostic can be unit-tested without
/// capturing stderr — the `cli_warn!` surface in `process_markdown_file`
/// consumes it. Regression guard: on a malformed page the block is now excluded
/// from the rendered body, so this warning is the only user-visible signal that
/// content was dropped (BUG 3).
fn frontmatter_invalid_yaml_warning(
    file_path: &str,
    parsed: &moss_core::frontmatter::ParsedDocument,
) -> Option<String> {
    parsed.frontmatter_error.as_ref().map(|err| {
        format!(
            "[{}] frontmatter: invalid YAML — {} (frontmatter ignored; block not rendered)",
            file_path, err
        )
    })
}

/// Warnings for frontmatter keys that name another generator's field.
///
/// Unknown keys are normally silent, rightly — templates and plugins read their
/// own. The exception is a name that means "do this job" elsewhere and does
/// nothing here: `slug:` (Hugo/Jekyll/Zola/Astro) is moss's `url:` and was
/// ignored without a word. Nothing else covers this at build time —
/// `validate_frontmatter`'s only caller is the editor, so authoring in Obsidian
/// or vim got no signal at all. Table (shared with that editor hint):
/// [`moss_core::validation::foreign_field_suggestion`]. Pure so the wiring is
/// testable; phrased as a question because a template may read its own `image:`.
fn foreign_frontmatter_warnings(file_path: &str, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .filter_map(|key| {
            moss_core::validation::foreign_field_suggestion(key).map(|moss_field| {
                format!(
                    "[{}] frontmatter: '{}' has no meaning to moss and was ignored — did you mean '{}'? (Harmless if it is your own field.)",
                    file_path, key, moss_field
                )
            })
        })
        .collect()
}

/// The built-in schema, materialized once per process.
///
/// `builtin_schema()` walks `BUILTIN_FIELDS` and allocates a `HashMap` of ~34
/// `FieldDefinition`s on every call. `schema_frontmatter_warnings` runs per
/// file, so calling it there would rebuild the whole table for each page of the
/// site. It is immutable once built — hence a `OnceLock`.
///
/// Measured 2026-08-04 (release build): `validate_frontmatter` costs **1.66 µs**
/// per file, so the check itself is 3.3 ms across a 2000-page site — nothing.
/// `builtin_schema()` costs **69.6 µs**, which per-file would have been 139 ms
/// on that same site: the hoist is worth ~42× the thing it is hoisting.
static BUILTIN_SCHEMA: std::sync::OnceLock<moss_core::schema::ContentSchema> =
    std::sync::OnceLock::new();

/// Schema diagnostics worth printing at build time.
///
/// `validate_frontmatter` is the full check — type mismatches, enum violations,
/// bad date formats, missing required fields, unknown fields. Its only other
/// caller is the editor (`editor/commands.rs`), which shows every diagnostic it
/// returns. The build shows a deliberately narrower set, because a build warning
/// that fires on correct content is worse than no warning at all:
///
/// - **Only fields the author actually wrote.** A diagnostic whose `path` names
///   a key absent from the frontmatter is a missing-required-field report, and
///   the only required field is `title` — which moss legitimately falls back to
///   the filename for (see `docs/reference/title-rendering.md`). Warning "required
///   field 'title' is missing" would fire on most correct pages. Filtering on
///   key presence rather than on the message text also keeps this from breaking
///   when the wording changes.
/// - **No bare `Hint`s.** Every unknown field is a `Hint`, and templates and
///   plugins legitimately read their own keys — printing those would bury the
///   real signals under one line per custom key. The actionable subset of that
///   set is already covered by `foreign_frontmatter_warnings`, which is narrowed
///   to a curated table.
///
/// The editor keeps the full diagnostic set: it renders them inline against the
/// field, where a hint costs nothing. This is about widening the build, not
/// narrowing the editor.
///
/// Pure, so the mapping is unit-testable without running a build.
fn schema_frontmatter_warnings(
    file_path: &str,
    frontmatter: &std::collections::HashMap<String, serde_yaml::Value>,
) -> Vec<String> {
    use moss_core::validation::Severity;

    let schema = BUILTIN_SCHEMA.get_or_init(moss_core::schema::builtin_schema);
    moss_core::validation::validate_frontmatter(frontmatter, schema)
        .into_iter()
        .filter(|d| matches!(d.severity, Severity::Error | Severity::Warning))
        .filter(|d| {
            // `path` is either a field name or `field[i]` for an array item;
            // both map back to a top-level key the author wrote.
            d.path.as_deref().is_some_and(|p| {
                let base = p.split('[').next().unwrap_or(p);
                frontmatter.contains_key(base)
            })
        })
        .map(|d| format!("[{}] frontmatter: {}", file_path, d.message))
        .collect()
}

/// Processes a markdown file with frontmatter into a ParsedDocument.
///
/// `site_lang` is the site's default language, used as the final fallback
/// when a document has no `lang:` frontmatter, no filename suffix, no
/// ancestor lang folder, and content too short for `whatlang` to detect.
/// The five-step priority chain (frontmatter > filename suffix > ancestor
/// folder > content detection > site default) lives in
/// [`crate::i18n::resolve_document_language`].
pub fn process_markdown_file(
    file_path: &str,
    content: &str,
    root_folder_name: &str,
    page_map: &HashMap<String, String>,
    emit_source_lines: bool,
    site_lang: crate::i18n::Language,
    site_id: Option<&str>,
    implicit_figure: bool,
    // 2026-07 (ADR-030): `[site].math`, resolved on `SiteConfig` (absent
    // key => true). When false, `$` stays an ordinary character and the
    // parser never emits math events — the escape hatch for prose where
    // `$` pairs up by accident. Threaded alongside `implicit_figure`
    // because both are site-level answers to "what does this character
    // mean", which only the config owner can answer.
    math: bool,
    // 2026-07: `[site].hard_line_breaks`, resolved on `SiteConfig` (absent
    // key => true — Obsidian parity). Third member of the "what does this
    // character mean" family with `implicit_figure`/`math`: a single
    // newline renders as `<br>` when true, as a space (CommonMark) when
    // false. See crates/moss-core/src/ast/line_breaks.rs.
    hard_line_breaks: bool,
    // 2026-07 (ADR-030): `[site].heading_anchors`, resolved on `SiteConfig`
    // (absent key => true). Controls `RenderHooks::emit_heading_anchors` —
    // when false, headings render without the trailing `#` permalink
    // anchor.
    heading_anchors: bool,
    // 2026-05 (structural-html-emission migration, Step 2): when `Some`,
    // markdown `Tag::Image` events route through `image_render::synthesize_image_html`
    // at the event-iterator level, producing HTML with dimensions, LQIP,
    // dominant-color background, loading="lazy", and optional
    // `<picture><source>` wrap baked in. Phase 2E v5 PR5 (2026-05-26) retired
    // the Stage 3 regex post-pass — the synthesizer now owns every
    // image-attribute emission and every `<picture>` wrap. When `None`, the
    // function emits bare `<img>` events through pulldown-cmark's default
    // serializer (the test/fragment-rendering fallback); production call sites
    // at `build/render/blocking.rs` always pass `Some`.
    media_lookup: Option<&crate::build::media::dimensions::MediaDimensionLookup>,
    // 2026-05 (linkblog / `external_url` frontmatter, moss#679): maps
    // `source_path → external_url` for pages declaring an absolute external
    // destination (JSON Feed 1.1 linkblog pattern). When provided, the
    // wikilink resolver substitutes the external URL for any `moss-resolved:`
    // target that has an entry. The page is still built locally — direct
    // visits to its slug still work — but internal references route to the
    // outlet. Tests pass `None`; the production call site in `blocking.rs`
    // builds the map from the same frontmatter pre-scan as `build_page_map`.
    external_url_map: Option<&HashMap<String, String>>,
    // Phase 3 PR2 (native wikilinks): the page's content graph + plugin
    // renderer registry. When provided, pulldown-cmark's
    // `LinkType::WikiLink` events get dispatched through
    // `moss_core::resolve::wikilink_dispatch::dispatch_wikilink_embed`
    // (extension-routing via the EmbedRenderer registry + ContentGraph
    // resolution + obsidian-heading-anchor normalization). When `None`,
    // wikilinks fall through to pulldown-cmark's default emission (tests
    // only — slot files such as footer.md ride the main document pipeline
    // and do get a graph, which is what makes their wikilinks resolve).
    graph: Option<&moss_core::content_graph::ContentGraph>,
    renderer_registry: Option<&moss_core::resolve::registry::RendererRegistry>,
    // Project-level signal (true when the site has content subfolders). Feeds
    // `is_nav_bar_item` so a root-level nav page suppresses its auto-injected
    // article title.
    has_content_folders: bool,
    // Pre-resolved seta base URL for subscribe form `action=` attributes.
    // When `Some`, overrides the production default ("https://api.mosspub.com").
    // The production call site in `blocking.rs` passes
    // `Some(resolve_environment(source_path).seta_url())`.
    // Test and fragment-render callers pass `None` → production URL is used,
    // preserving existing test behavior.
    seta_url: Option<&str>,
    // ADR-065: the language inferred for this file's FOLDER, computed once
    // in the scan/reduce phase (`scan::page_map::folder_lang`) over every
    // file in the folder, not per-page here. Only consulted when the path
    // itself carries no naming convention (`ancestor_lang_from_path` is
    // `None`) — a folder named `en/` still wins outright. The production
    // call site in `blocking.rs` passes the precomputed map's lookup for
    // this file's directory; tests pass `None`, matching the old rung-4
    // behavior of "no folder signal available".
    folder_lang: Option<crate::i18n::Language>,
) -> Result<ParsedDocument, String> {
    // Check if using simplified frontmatter syntax
    // `frontmatter_line_count`: how many leading lines of `content` the
    // frontmatter block (incl. its closing delimiter) occupies. Computed
    // structurally from the VERBATIM body suffix (`content[fm_end..]`, before
    // any normalization) — `content.lines() − verbatim_body.lines()` is exact
    // because the frontmatter always ends in a newline, so line-counting a
    // suffix split is additive. Used below to make the source-line-offset search
    // skip the frontmatter region (so a body line that happens to equal a
    // frontmatter value line is not matched too early). 0 when no frontmatter.
    let (mut frontmatter, markdown_content_raw, raw_frontmatter, frontmatter_line_count) =
        if is_simplified_frontmatter(content) {
            let (fm, body) = parse_simplified_frontmatter(content);
            // Both dialects get the same foreign-field warning; the simplified
            // parser drops unknown keys, so ask for them separately.
            let keys = moss_core::frontmatter_typed::simplified_frontmatter_keys(content);
            let key_refs: Vec<&str> = keys.iter().map(String::as_str).collect();
            for warning in foreign_frontmatter_warnings(file_path, &key_refs) {
                crate::build::cli_output::cli_warn!("{}", warning);
            }
            let fm_lines = content.lines().count().saturating_sub(body.lines().count());
            (fm, body, std::collections::BTreeMap::new(), fm_lines)
        } else {
            // Traditional YAML frontmatter. ONE parser (ADR-020): the same
            // moss_core::frontmatter::parse the editor/chips use, projected to
            // the typed FrontMatter via project_typed. gray_matter is gone.
            let parsed = moss_core::frontmatter::parse(content);
            // Surface malformed YAML instead of letting it leak verbatim into
            // HTML (ADR-020 authoritative warning surface; mirrors the
            // project_typed loop below). The block is excluded from the render
            // body via `render_body()` so nothing reaches `markdown_content_raw`.
            // The warning text is built by the pure `frontmatter_invalid_yaml_warning`
            // so the build-side diagnostic is unit-testable (the malformed block
            // now silently disappears from output — this warning is the ONLY
            // signal that content was dropped, so it must not regress unnoticed).
            if let Some(warning) = frontmatter_invalid_yaml_warning(file_path, &parsed) {
                crate::build::cli_output::cli_warn!("{}", warning);
            }
            // Render-only view: excludes a delimited block that FAILED to parse
            // (equal to `body` on success). Line-count + body both derive from
            // it so a malformed page yields a nonzero, correct `fm_lines`.
            let render_body = parsed.render_body();
            // Frontmatter line count from the verbatim body (before normalize
            // strips leading/trailing blanks); see the comment on the binding.
            let fm_lines = content
                .lines()
                .count()
                .saturating_sub(render_body.lines().count());
            let mut mapping = serde_yaml::Mapping::new();
            for (k, v) in &parsed.frontmatter {
                mapping.insert(serde_yaml::Value::String(k.clone()), v.clone());
            }
            let (mut fm, warnings) = moss_core::frontmatter_typed::project_typed(&mapping);
            for w in &warnings {
                crate::build::cli_output::cli_warn!("[{}] frontmatter: {}", file_path, w.message);
            }
            // Keys another generator uses for a job moss also does. See
            // `foreign_frontmatter_warnings` for why these are not silent.
            let keys: Vec<&str> = mapping.keys().filter_map(|k| k.as_str()).collect();
            for warning in foreign_frontmatter_warnings(file_path, &keys) {
                crate::build::cli_output::cli_warn!("{}", warning);
            }
            // Schema validation — type mismatches, enum violations, bad dates.
            // Until this ran here, `weight: high` was silent at build and only
            // moss's own editor reported it; most authoring happens elsewhere.
            // Only the YAML dialect: the simplified parser yields a typed
            // `FrontMatter` with the raw values already coerced away, so there
            // is nothing left to type-check by the time it returns.
            for warning in schema_frontmatter_warnings(file_path, &parsed.frontmatter) {
                crate::build::cli_output::cli_warn!("{}", warning);
            }
            // Body reconciliation (ADR-020 Phase 2b): gray_matter's `result.content`
            // — which the downstream pipeline + every snapshot fixture were built
            // against — strips leading blank lines and drops the trailing newline,
            // because it reconstructs the body line-by-line and then
            // `trim_start_matches('\n')` (see gray_matter-0.2.9 matter.rs). moss_core's
            // `parsed.body` is the verbatim `content[fm_end..]` (leading + trailing
            // newlines intact). Reproduce gray_matter's exact transform so the body
            // text is byte-identical and snapshots don't churn.
            let body = normalize_body_like_gray_matter(render_body);
            // Plugin fields (e.g. `syndicated`) + children normalization come from
            // the raw serde_yaml map exactly as before.
            let raw_fm = crate::build::scan::article_map::parse_frontmatter(content);
            // Extract children_source from the authored value via the shared
            // normalizer (handles wikilink "[[News]]" AND resolved path forms).
            // raw_fm holds serde_json::Value; the normalizer takes serde_yaml::Value
            // — re-express through serde_yaml::to_value (cheap in-memory hop).
            if let Some(children_val) = raw_fm.get("children") {
                if let Ok(yaml_val) = serde_yaml::to_value(children_val) {
                    let norm = moss_core::frontmatter_union::normalize_children(&yaml_val);
                    fm.children = Some(norm.children);
                    fm.children_source = norm.source;
                }
            }
            (fm, body, raw_fm, fm_lines)
        };

    // Normalize legacy `series: [list]` form into `sort: List + series: Flag(true)`.
    // Task 5: defensive conversion for future authors who might write the old form.
    frontmatter.normalize();

    // Translate the deprecated `sidebar:` field for both simplified and YAML
    // paths. Runs after frontmatter is fully parsed so it sees the final state
    // of children_source. See `apply_sidebar_alias` for the conflict rules.
    for warning in crate::build::markdown::frontmatter::apply_sidebar_alias(&mut frontmatter) {
        crate::build::cli_output::cli_warn!("[{}] {}", file_path, warning);
    }

    // Pre-parse source hygiene, in the one place that owns its ordering:
    // CriticMarkup accept, `%%` comment stripping, and the unterminated-`<!--`
    // warning. See `html_post::prepare_markdown_source`.
    let markdown_content_raw =
        prepare_markdown_source(&markdown_content_raw, file_path, frontmatter_line_count);

    // Phase 2F: warn on author-typed raw <img>/<video> HTML in markdown source.
    // After Phase 2E retires placeholder.rs, raw HTML in markdown becomes
    // fully opaque to moss (no LQIP/dims/lazy-load injection). The scanner
    // emits a log::warn! per occurrence so the author can opt back into
    // moss enhancement by switching to ![alt](src) / ![[file]] syntax. Pure
    // side-effect; never alters the build output.
    crate::build::media::raw_img_warning::scan_for_raw_media_warnings(
        &markdown_content_raw,
        std::path::Path::new(file_path),
    );

    // D5: `list` alias removed — no merge needed

    // Visible heading text — filename with hyphens/underscores → spaces, with
    // folder notes resolving to the parent folder name. Source of truth is
    // `moss_core::heading`, shared with the editor's pinned heading element so
    // the two never drift. Root-aware (#775): a root `index.md` (no path
    // parent) resolves to the project folder name, not the bare "index" stem —
    // this feeds the index-page `title`/`label` fallback below and must agree
    // with `heading.text` (which `compute` resolves with the same root name).
    // See docs/reference/title-rendering.md.
    let filename_title =
        moss_core::heading::filename_text_with_root(file_path, Some(root_folder_name));


    // Shortcode processor order is LOAD-BEARING for nesting.
    // Order: toc → (link resolver built) → typed-AST (subscribe, buttons,
    // gallery, hero) → grid.
    //
    // Why: the grid processor is the "last mile" and uses a fence-stack parser
    // that treats other shortcode output as opaque cell content. Running
    // buttons and gallery BEFORE grid means `::::buttons` inside `:::grid`
    // gets pre-rendered to HTML before grid scans the cell. Reordering breaks
    // nested shortcodes. See Task 2.5 in
    // docs/archive/2026-04-20-moss-dogfood-tier-1-fixes.md
    //
    // Hero extraction moved into the typed-AST path in Step 2 of
    // docs/archive/2026-05-02-shortcode-grammar-design.md. apply_typed_shortcodes
    // intercepts :::hero blocks: it renders them through the resolver and
    // returns the HTML separately for hoisting into the article template,
    // substituting the body placeholder with empty.
    //
    // Grid shortcodes are processed AFTER the link resolver is built (below),
    // so grid card links can resolve through the PageMap.
    // Note: callouts use `> [!type]` syntax handled by callouts.rs (see ADR-011)
    //
    // `:::toc` was removed in Step 2c of the unified grammar migration
    // (#613). Authors who write `:::toc` now see the moss-unknown-shortcode
    // fallback and a build warning. TOCs become a theme/template concern.
    let markdown_content = markdown_content_raw.clone();

    // Resolve document language early (needed for url_path computation in link resolver).
    let filename_stem = Path::new(file_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("untitled");
    // ADR-065: `ancestor_lang` is the folder's NAME (when an ancestor
    // component parses as a language code) or else the language INFERRED for
    // the folder upstream in scan/reduce (`folder_lang`, computed once per
    // folder, never from this file's own content). `resolve_document_language`
    // treats both the same and no longer reads `content` at all.
    let ancestor_lang = crate::i18n::path::ancestor_lang_from_path(file_path).or(folder_lang);
    let (doc_lang, clean_stem) = crate::i18n::resolve_document_language(
        frontmatter.lang.as_deref(),
        filename_stem,
        site_lang,
        ancestor_lang,
    );
    // `doc_lang` is the INTERFACE language, three values; this is what the page
    // says its CONTENT is, in any language moss recognizes — `None` only when
    // the page declares nothing at all (#977).
    let fm_lang = frontmatter.lang.as_deref();
    let doc_lang_tag =
        crate::i18n::declared_lang_tag(fm_lang, filename_stem, file_path, ancestor_lang);

    // Detect if this is an index file (needed for url_path computation).
    let filename_lower = Path::new(file_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let parent_name = if file_path.contains('/') {
        Path::new(file_path)
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("")
    } else {
        root_folder_name
    };
    // Index detection: a doc is a folder index iff (a) its filename
    // matches a recognized index pattern (INDEX_STEMS or self-named
    // folder note), OR (b) it's been promoted to its folder's home via
    // the `home: true` marker (issue #587 — see
    // `compute_home_overrides`).
    //
    // We must detect (a) by filename, *not* by page_map URL: a folder
    // index with a slug override (e.g. `posts/index.md` carrying
    // `url: blog` → URL `blog/index.html`) belongs in (a), but its URL
    // parent (`blog`) differs from the source parent (`posts`). The
    // filename heuristic catches it; a URL-only check would not.
    //
    // (b) is detected via the page_map: a promoted file gets URL
    // `<source_parent>/index.html` (mirroring the source location) while
    // a regular article gets `<source_parent>/<slug>/index.html` (one
    // extra path segment). We only check (b) when (a) is false, so a
    // self-named folder note with a slug override stays in (a).
    let is_index_by_filename = moss_core::home::is_home_file(&filename_lower, parent_name);
    let is_index_by_home_override = !is_index_by_filename
        && page_map.get(file_path).is_some_and(|url| {
            let source_parent = std::path::Path::new(file_path)
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or("");
            if source_parent.is_empty() {
                url == "index.html"
            } else {
                url == &format!("{}/index.html", source_parent)
            }
        });
    let is_index_file = is_index_by_filename || is_index_by_home_override;

    // Build the link resolver closure.
    // Handles `moss-resolved:` markers from the resolve pipeline and falls back
    // to `transform_markdown_link` for URL prettification. Content-graph
    // resolution lives in moss-core; see `resolve_link` below.
    let self_url_path_ref = page_map.get(file_path).cloned().unwrap_or_else(|| {
        compute_url_path(
            file_path,
            is_index_file,
            frontmatter.url.as_deref(),
            &clean_stem,
        )
    });

    // Slot HTML (`footer.md`, or a `slot:`-marked page) is rendered once and
    // injected byte-identically into every page at every depth, so a URL relative
    // to this file's own position is wrong everywhere it lands — page links go out
    // root-absolute. Asset links already do (`pinned_url`), hence only one branch.
    let injects_into_every_page =
        crate::build::footer::targets_a_slot(file_path, frontmatter.slot.as_deref());

    let resolve_link = |href: &str| -> String {
        // By the time hrefs reach this closure, `moss-core`'s resolve pipeline
        // has already routed every resolvable link through
        // `ContentGraph::resolve_path` (see ast/resolve_urls.rs and
        // resolve/wikilinks.rs). We see one of three shapes:
        //   1. `moss-resolved:<resolved-path>[?query][#frag]` — pre-resolved.
        //      Two sub-cases:
        //      a. target IS in page_map (markdown page) — emit relative pretty URL.
        //      b. target NOT in page_map (HTML/binary asset) — emit relative
        //         filesystem path adjusted for pretty-URL nesting depth.
        //   2. External / anchor / mailto / tel / data — pass through.
        //   3. Already-pretty or absolute URLs — apply URL prettification only.
        //
        // This closure MUST NOT perform content-graph lookups. The single
        // source of truth for target resolution is
        // `moss_core::content_graph::ContentGraph::resolve_path`.

        if let Some(rest) = href.strip_prefix("moss-resolved:") {
            let (target_source, suffix) = split_path_suffix(rest);
            // `external_url:` linkblog override — when the target page
            // declares an absolute external URL (JSON Feed 1.1 pattern),
            // every internal reference (wikilinks, link rewrites) routes
            // there instead of the local archive. http(s) only; anchors
            // (#frag) and queries get appended after substitution so e.g.
            // `[[Post#section]]` still deep-links into the outlet copy.
            if let Some(map) = external_url_map {
                if let Some(target_external) = map.get(target_source) {
                    if target_external.starts_with("http://")
                        || target_external.starts_with("https://")
                    {
                        let resolved = match suffix {
                            Some(s) => format!("{}{}", target_external, s),
                            None => target_external.clone(),
                        };
                        // No `wikilink:` sentinel — see the page_map branch
                        // below for why.
                        return resolved;
                    }
                }
            }
            if let Some(target_url_path) = page_map.get(target_source) {
                let rel = if injects_into_every_page {
                    absolute_pretty_url(target_url_path)
                } else {
                    relative_pretty_url(&self_url_path_ref, target_url_path)
                };
                let resolved = match suffix {
                    Some(s) => format!("{}{}", rel, s),
                    None => rel,
                };
                // No `wikilink:` sentinel here (nor in the external_url_map
                // branch above): a link to an internal page is a plain
                // internal link. The hover-preview `class="wikilink"` (see
                // frontend/site/link-preview.ts) must reflect the link's
                // SYNTAX — `[[wikilink]]` vs `[text](page)` — not the fact
                // that its target resolves to a page. Genuine wikilinks carry
                // the AST `is_wikilink` flag (parser sets it for
                // LinkType::WikiLink) straight through to `render_link`, so
                // they still get the class. Tagging every page target
                // `wikilink:` here made plain Markdown links (and
                // `:::buttons` items) show a spurious popover.
                return resolved;
            }
            // Target not in page_map — non-markdown asset (e.g. .html, .pdf).
            // Emit the target's pinned URL. This replaced a three-step
            // re-derivation (relative path from the referencing file →
            // `slugify_dir_path` case-folding → `../` if the page is served one
            // level deeper), each step guessing at something the graph already
            // knew: moss#903 bug 3, and the case-normalization bug in
            // docs/archive/2026-05-07-output-path-normalization.md.
            let pinned = match graph {
                Some(g) => g.pinned_url(target_source),
                // No graph (fragment / test callers): pin without the build's
                // directory overrides, never off the referencing page.
                None => moss_core::resolve::output_url::pinned_url(
                    target_source,
                    &std::collections::HashMap::new(),
                ),
            };
            let with_suffix = match suffix {
                Some(s) => format!("{}{}", pinned, s),
                None => pinned,
            };
            // Prefix with sentinel so post-processing can add target="_blank":
            // asset links open in a new tab so the reading flow isn't lost.
            return format!("moss-newtab:{}", with_suffix);
        }

        transform_markdown_link(href)
    };

    // ----------------------------------------------------------------
    // Phase 4 PR7a-flip-core-C (2026-05-28): THE production flip.
    //
    // Replace `apply_typed_shortcodes` + `transform_events` + post-passes
    // with the typed-AST pipeline: `parse → dispatch_wikilink_embeds →
    // resolve_urls → extract_hero → find_first_block_image →
    // classify_remaining_urls → render_document`.
    //
    // The call order is LOAD-BEARING — validated by three constraints:
    //   1. `dispatch_wikilink_embeds` must run BEFORE `resolve_urls`. It
    //      reads `Inline::Image.src` as raw `Url::Unresolved`; its internal
    //      `resolve_reference` would double-resolve if given `Url::Resolved`.
    //   2. `extract_hero` must run AFTER `resolve_urls`. The function
    //      `debug_assert!`s on `Url::Resolved` for the Hero image (see
    //      `extract_hero.rs:102-106`); running before resolve_urls trips
    //      the assert.
    //   3. `find_first_block_image` must run AFTER `extract_hero`. The
    //      query descends into the Hero overlay (query.rs:145-153); if
    //      hero is still in `doc.blocks`, an overlay image could be
    //      returned as the body cover, double-counting against the OG
    //      image already captured from `hero_extraction.image_url`.
    //
    // DO NOT reorder these without revisiting the constraint set.
    let subscribe_site = site_id.unwrap_or("");
    let seta_base_resolved: &str = seta_url.unwrap_or("https://api.mosspub.com");
    // Real dir_overrides, so the snapshot's output-URL keys are the strings the
    // synthesizer probes with. An empty map re-fires the 800x600 fallback.
    let asset_snapshot = media_lookup.map(|l| l.asset_snapshot());
    let pipeline_hooks = PipelineHooks {
        site_id: subscribe_site,
        lang: doc_lang,
        seta_base: seta_base_resolved,
        assets: asset_snapshot,
        media_lookup,
        permalink_section_label: crate::i18n::t(doc_lang, "permalink_section"),
        heading_anchors,
    };

    // 0. Parse markdown into the typed AST. Sees `:::shortcode` blocks
    //    natively as `Shortcode::*` variants; sees the Stage 1 callout
    //    output (`<div class="callout">`) as `Block::Other` raw HTML.
    //
    //    `ParseConfig` threads two runtime flags through the parser:
    //    - `emit_source_lines`: populates `BlockMeta::source_line` for
    //      preview scroll-sync (`data-source-line="N"` on the rendered
    //      open tag). Restored 2026-05-28 (Phase 4 cleanup): the legacy
    //      `transform_events` pipeline emitted this attribute before
    //      PR7a's deletion; the AST renderer now owns the emission.
    //    - `implicit_figure`: when false, undoes the parser's
    //      `try_promote_to_figure` for image-only paragraphs so they
    //      render as bare `<p><img></p>`. Default behavior is true,
    //      matching the always-on shape that callers assumed before this
    //      flag existed.
    //    - `source_line_offset`: `data-source-line`/`-range` must match the
    //      line the EDITOR shows a block on, because the whole scroll-sync +
    //      click-to-source protocol passes a single bare line number between
    //      the panes (editor-main.ts `onEditorScroll`, `editor:goto-line`).
    //      The editor loads `moss_core::frontmatter::parse(content).body` into
    //      CM6 (editor-main.ts `new CmEditor({ content: body })`) — the
    //      frontmatter-stripped body, NOT the raw file — so annotations must be
    //      relative to THAT buffer. The parser here sees a further-processed
    //      body (`normalize_body_like_gray_matter`, which drops leading blanks
    //      + the trailing newline), so we bridge the two in two steps:
    //
    //        raw-body offset  — lines of `content` before the parser body's
    //          first non-blank line, leading-blank-corrected. Locates the body
    //          past the frontmatter so a body line equal to a frontmatter value
    //          line isn't matched inside it (#771 edge b); leading-only so it is
    //          immune to the trailing-newline drop.
    //        − editor strip   — lines the EDITOR's `frontmatter::parse` removed
    //          (`frontmatter_range`). On `---`-fenced files this is the
    //          frontmatter length, cancelling the raw offset down to the body-
    //          relative line the editor actually reports. On the SIMPLIFIED
    //          `key: value` path `frontmatter::parse` detects no block and strips
    //          nothing, so this is 0 and the annotation stays raw-relative —
    //          which is correct there, because the editor also shows the whole
    //          file (its parser didn't strip the pseudo-frontmatter either).
    //
    //      Historical note: commit 52a564963 made these REAL FILE lines to match
    //      an editor believed to hold "the raw file (frontmatter + body)". That
    //      premise was never true (the editor has been body-only since its first
    //      commit, 7b4c79b30); the raw offset was a constant frontmatter-sized
    //      error → "preview always lower". See docs/reference/editor-preview-sync.md.
    //
    //      KNOWN LIMITATION (#771 edge a): a single scalar offset assumes the
    //      body's interior line count is preserved. Shortcode placeholders
    //      preserve it, but MULTI-LINE CriticMarkup that `accept_criticmarkup`
    //      collapses to fewer lines shifts every annotation AFTER the span. A
    //      correct fix is a raw↔accepted line MAP, not a scalar; deferred —
    //      CriticMarkup is a rare, transient authoring layer and the failure
    //      degrades to a slightly-off scroll-sync, never a panic.
    let source_line_offset = {
        let lookup_leading_blanks = markdown_content
            .lines()
            .take_while(|l| l.trim().is_empty())
            .count();
        let raw_body_offset = markdown_content
            .lines()
            .find(|l| !l.trim().is_empty())
            .and_then(|first| {
                content
                    .lines()
                    .skip(frontmatter_line_count)
                    .position(|l| l.trim_end() == first.trim_end())
                    .map(|p| p + frontmatter_line_count)
            })
            .map(|pos| pos.saturating_sub(lookup_leading_blanks))
            .unwrap_or(0);
        // Lines the EDITOR's frontmatter parser strips before CM6 line 1.
        // Derive it from the ACTUAL buffer the editor loads —
        // `frontmatter::parse(content).body` — not from `frontmatter_range`:
        // on the MALFORMED-YAML path the parser returns `frontmatter_range =
        // Some(..)` but `body = the WHOLE file` (kept verbatim so the author can
        // repair the bad block — see frontmatter.rs + editor-main.ts "the raw
        // block stays in CM6 body"), so the editor strips NOTHING there. The
        // line-count delta captures that exactly: 0 on the malformed path,
        // the frontmatter length on the clean `---` path (body is a suffix
        // split on a line boundary), and 0 on the simplified/no-frontmatter
        // path. CRLF is pre-normalized so both counts are over the same string.
        let normalized_content = content.replace("\r\n", "\n");
        let editor_body = moss_core::frontmatter::parse(&normalized_content).body;
        let editor_stripped_lines = normalized_content
            .lines()
            .count()
            .saturating_sub(editor_body.lines().count());
        raw_body_offset.saturating_sub(editor_stripped_lines)
    };
    let parse_config = moss_core::ast::ParseConfig {
        emit_source_lines,
        implicit_figure,
        source_line_offset,
        math,
        hard_line_breaks,
    };
    let mut doc = moss_core::ast::parse_with_config(&markdown_content, &parse_config);
    // Mirrors the `frontmatter:` warning line above: a misspelled shortcode
    // used to leave no trace but a `moss-unknown-shortcode` div in the output.
    for w in &doc.warnings {
        crate::build::cli_output::cli_warn!("[{}] {}", file_path, w);
    }

    // Inline #tags (issue #649 P1) are read from the author's OWN parsed
    // body — before dispatch_wikilink_embeds splices in blocks from other
    // files, whose tags belong to their source doc. Merged into doc.tags
    // below, where frontmatter tags are unpacked.
    let inline_tags = moss_core::ast::extract_inline_tags(&doc);

    // 1. Dispatch wikilink embeds.
    //    LOCKED ORDER: must run BEFORE `resolve_urls` — see constraint (1)
    //    above. Reads `Inline::Image.src` as `Url::Unresolved(raw)`; the
    //    dispatcher's internal `resolve_reference` would double-resolve
    //    if given `Url::Resolved`.
    let mut wikilink_outgoing: Vec<moss_core::resolve::OutgoingLink> = Vec::new();
    let mut wikilink_diagnostics: Vec<moss_core::resolve::Diagnostic> = Vec::new();
    if let (Some(g), Some(reg)) = (graph, renderer_registry) {
        let snapshot_for_dispatch =
            crate::build::media::dimensions::snapshot_or_empty(media_lookup);
        let dispatch_result = moss_core::ast::dispatch_wikilink_embeds(
            &mut doc,
            snapshot_for_dispatch,
            g,
            reg,
            file_path,
        );
        wikilink_outgoing.extend(dispatch_result.outgoing_links);
        wikilink_diagnostics.extend(dispatch_result.diagnostics);
        // `implicit_figure = false` has to reach the figures the dispatcher
        // just made, not only the ones the parser made. A `![[tile.png]]`
        // becomes a `Block::Figure` here — after `parse_with_config` ran its
        // own unwrap pass — so on an opted-out site the wikilink spelling of
        // an image kept a `<figure class="moss-image">` that the markdown
        // spelling of the same image did not have. One authorial intent, two
        // boxes, and any theme rule keyed on `.moss-image` hit only half the
        // images on the page.
        if !implicit_figure {
            moss_core::ast::unwrap_implicit_figures(&mut doc);
        }
    }

    // 2. Resolve URLs through the content graph.
    //    LOCKED ORDER: must run BEFORE `extract_hero` (constraint 2) and
    //    AFTER `dispatch_wikilink_embeds` (constraint 1). Resolves
    //    `Url::Unresolved → Url::Resolved` across all Image.src and
    //    Link.url, INCLUDING the fresh blocks spliced in by
    //    `dispatch_wikilink_embeds` via EmitKind::Inline / EmitKind::Link
    //    re-parsing.
    let graph_resolution = if let Some(g) = graph {
        moss_core::ast::resolve_urls(&mut doc, g, file_path)
    } else {
        moss_core::ast::UrlResolution::default()
    };
    // Both resolve streams, one surface. An unresolvable reference kept the
    // author's bytes rather than becoming a guessed URL, and a typo'd
    // `![[image.png]]` is the mistake `--strict` most needs to catch, so each
    // one counts as a build problem as well as a log line.
    let resolve_diagnostics = || wikilink_diagnostics.iter().chain(&graph_resolution.diagnostics);
    for diag in resolve_diagnostics() {
        crate::build::cli_output::log_warn_problem!(
            "Resolve: {} — ref '{}' in '{}'",
            diag.message,
            diag.reference,
            diag.source_path
        );
    }
    // …and KEEP the blocking ones. These streams are the build's only producers
    // of `MissingAsset`, so logging and dropping left the publish gate nothing
    // to refuse on and the site shipped with a 404 in it.
    let missing_media = crate::build::types::MissingMedia::from_diagnostics(resolve_diagnostics());
    let mut outgoing_links: Vec<moss_core::resolve::OutgoingLink> = wikilink_outgoing;
    outgoing_links.extend(graph_resolution.outgoing);

    // 2b. Host-context URL classification. `resolve_urls` only handles the
    //     content-graph subset (bare image refs + standard markdown links).
    //     URLs prefixed with `moss-resolved:` / `moss-newtab:` / `wikilink:`
    //     by the upstream resolve pipeline are intentionally left as
    //     `Url::Unresolved` for the host to decode via the production
    //     `resolve_link` closure.
    moss_core::ast::visit_urls_mut(&mut doc, |url| {
        if let moss_core::ast::Url::Unresolved(raw) = url {
            *url = classify_url_prod(raw, &resolve_link);
        }
    });

    // 3. Extract the Hero shortcode for hoisting.
    //    LOCKED ORDER: must run AFTER `resolve_urls` (constraint 2) and
    //    BEFORE `find_first_block_image` (constraint 3). Removes the
    //    first top-level `Block::Shortcode(Hero)` from `doc.blocks` and
    //    returns the rendered HTML + OG-fallback fields. The hook
    //    delegates to `render_hero_html_typed` for byte parity with the
    //    legacy upstream hoist.
    let hero_extraction = moss_core::ast::extract_hero(&mut doc, &pipeline_hooks);
    let hero_html = hero_extraction.as_ref().map(|h| h.html.clone());
    let hero_image_url = hero_extraction.as_ref().and_then(|h| h.image_url.clone());
    let hero_overlay_text = hero_extraction
        .as_ref()
        .and_then(|h| h.overlay_text.clone());

    // 4. Capture the first body image for the cover chain.
    //    LOCKED ORDER: must run AFTER `extract_hero` (constraint 3). The
    //    query descends into Hero overlay (query.rs:145-153) — if hero is
    //    still in `doc.blocks`, an overlay image could be returned as the
    //    body cover, double-counting against the OG image already captured
    //    from `hero_extraction.image_url`.
    //
    //    Mirrors the legacy `transform_events` filter (raster extensions
    //    only, no folder/non-image trailing slash) so the field stays
    //    byte-equivalent to today's `ParsedDocument.body_cover_path`.
    let body_cover_path: Option<String> =
        moss_core::ast::find_first_block_image(&doc).and_then(|img| match img {
            moss_core::ast::Inline::Image {
                src: moss_core::ast::Url::Resolved(r),
                ..
            } => {
                let href = &r.href;
                if href.ends_with('/') {
                    return None;
                }
                let bare = href.split(&['#', '?'][..]).next().unwrap_or("");
                let ext = bare.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
                let is_raster = matches!(
                    ext.as_str(),
                    "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "avif"
                );
                if is_raster {
                    Some(href.clone())
                } else {
                    None
                }
            }
            _ => None,
        });

    // 5. Final safety net: any `Url::Unresolved` that escaped phases 1-2
    //    (e.g., a future URL-bearing variant added before its phase wires
    //    in) gets a best-effort classification — the renderer requires
    //    every URL be `Url::Resolved` at emission time.
    moss_core::ast::classify_remaining_urls(&mut doc);

    // 6. Render the typed AST to HTML through `PipelineHooks`. This is THE
    //    production HTML emission — `transform_events` no longer runs for
    //    `process_markdown_file`.
    //
    //    `emit_source_lines` and `implicit_figure` are now threaded
    //    through the parser (see step 0 above); the renderer reads
    //    `BlockMeta::source_line` and emits `data-source-line="N"` on
    //    each top-level block's opening tag when set. The ship-stage
    //    `apply_strip` regex scrubs the attribute from the published
    //    site/ tree (preview only).
    //    ADR-034: emitted in segments, byte-identical to `render_document`
    //    when re-joined, so the render phase gets typed grid cells and a lede
    //    boundary instead of having to re-parse this HTML.
    let mut body_plan = super::body_plan::render_segmented(&doc, &pipeline_hooks);

    // Subscribe-form presence: read from the typed AST (descends into
    // shortcode bodies). Replaces the `has_inline_subscribe` flag the
    // legacy `apply_typed_shortcodes` returned.
    let has_inline_subscribe =
        moss_core::ast::has_shortcode_recursive(&doc, moss_core::ast::ShortcodeKind::Subscribe);
    // Apply-form presence: same mechanism, same asset-injection trigger.
    let has_inline_apply =
        moss_core::ast::has_shortcode_recursive(&doc, moss_core::ast::ShortcodeKind::Apply);
    // Callout presence: same mechanism, gating the `callouts` stylesheet
    // partial (`build/emit/stylesheet.rs`) rather than an injected asset.
    let has_callout = moss_core::ast::has_callout_recursive(&doc);
    // Footnote presence: same mechanism again, but this one gates a runtime
    // SCRIPT (the sidenotes bundle) rather than a stylesheet partial.
    let has_footnotes = !moss_core::ast::footnotes::FootnoteIndex::build(&doc.blocks).is_empty();

    // Defensive sweep: `Block::Other` raw HTML (e.g. `<div class="callout">`
    // from upstream Stage 1) may still contain `href="moss-resolved:..."` /
    // `wikilink:` / `moss-newtab:` prefixes that the AST renderer didn't
    // see (they're opaque pass-through). Route any leakage through
    // `resolve_link`. Idempotent on already-decoded hrefs.
    body_plan.map_html(&|html| sweep_unresolved_hrefs(html, &resolve_link));

    // `:::toc` was removed in Step 2c of issue #613; the post-pass that
    // replaced its placeholder is gone. Tables of contents are now a
    // theme/template concern.

    // Title resolution — single source of truth in moss_core::heading::compute.
    //
    //   1. `title:` (when set, non-empty after trim) drives both the visible
    //      `<h1 class="moss-article-title">` and the chrome label cascade.
    //      Missing `title:` falls back to filename. `title: ""` (or
    //      whitespace-only) suppresses the auto-injected H1.
    //   2. A `:::hero` block at the top of the body owns the heading slot —
    //      no auto-injection regardless of title.
    //   3. Index/folder pages never get auto-injection.
    //   4. NO dedup (Obsidian-match, 2026-05-30): an authored body `# H1` is
    //      content and is kept verbatim. A `title: Custom` article whose body
    //      opens with `# Custom` renders BOTH the injected title and the body
    //      H1 — like Obsidian with "Show inline title" on.
    //
    // The injected `<h1 class="moss-article-title">` carries a stable class so
    // themes/sites can target it. See docs/reference/title-rendering.md.


    // Single source of truth: compute() resolves heading text + visibility +
    // source. Both the injected H1 and the chrome label cascade for ARTICLE
    // pages read from state.text below.
    // Reserved-name slot files (root `footer.md`) flow through the standard
    // pipeline so their HTML is available for `collect_footer_slots_by_language`, but
    // they must never receive the auto-injected `<h1 class="moss-article-title">`
    // — the fragment lands inside a `<footer>` slot, and an article-level
    // heading there is structurally wrong. PR7b (moss#599) routes that
    // suppression through `HeadingInputs::slot_only` rather than the
    // pre-2026-05-28 frontmatter-synthesis hack (`title: ""` injected by the
    // now-deleted `render_footer_pages_from_disk`).
    let is_slot_only = crate::build::footer::is_excluded_from_pages(file_path);
    let heading = moss_core::heading::compute(moss_core::heading::HeadingInputs {
        file_path,
        frontmatter_title: frontmatter.title.as_deref(),
        body_markdown: &markdown_content_raw,
        root_folder_name: Some(root_folder_name),
        is_home_override: is_index_by_home_override,
        slot_only: is_slot_only,
    });

    // Chrome label cascade — `state.text` (from compute(): `title:` else
    // filename/folder name) when non-empty, filename otherwise (state.text is
    // empty only in the title:"" case, which we ALWAYS chrome to filename to
    // avoid <title></title>). Never sourced from body content for any page type
    // (Obsidian-match, 2026-05-30).
    let label = if heading.text.is_empty() {
        filename_title.clone()
    } else {
        heading.text.clone()
    };

    // Title source: NEVER the document body. For index/folder pages,
    // `title:` if set, else the filename/folder name (`filename_title`,
    // which resolves index/self-named notes to their folder name). For
    // articles, the chrome `label` (which itself derives from
    // `title:`/filename via heading::compute). Matches Obsidian: a body
    // `# H1` is content, not the page title. See
    // docs/reference/title-rendering.md.
    let title = if is_index_file {
        match frontmatter.title.as_deref() {
            Some(t) if !t.trim().is_empty() => t.trim().to_string(),
            _ => filename_title.clone(),
        }
    } else {
        label.clone()
    };

    // Detect root-level files (no directory component). Used for the nav
    // membership check below and for auto-navigation downstream.
    let is_root_level = !file_path.contains('/');

    // A page that appears in the top nav bar already shows its title there, so
    // we suppress the auto-injected body heading — the nav owns the title
    // surface for that page. The title still drives the nav label and chrome.
    // `is_index_file && is_root_level` stands in for `url_path == "index.html"`
    // (the form `is_nav_bar_item_doc` uses at the render sites). It is EXACT for
    // every page this gate can actually affect: the gate below also requires
    // `heading.visible`, which moss-core sets to false for all index/folder
    // pages — so only non-index articles reach it, and for an article both this
    // proxy and `url_path == "index.html"` are always false. The two forms can
    // only disagree on index pages (e.g. a root index with a `url:` override),
    // which this gate never injects for. Keeping the proxy avoids depending on
    // `url_path`, computed a few lines below.
    let is_nav_page = crate::build::components::nav::is_nav_bar_item(
        frontmatter.nav,
        frontmatter.draft == Some(true),
        is_slot_only,
        is_root_level,
        is_index_file && is_root_level,
        &clean_stem,
        has_content_folders,
    );

    // Article pages inject `<h1 class="moss-article-title">` from the
    // resolved heading text. We do NOT dedup a matching body `# H1` — an
    // authored heading is content and stays put (Obsidian-match). A page
    // that opens with `# Title` therefore renders both the injected title
    // and the body H1, exactly like Obsidian with inline-title on.
    // Index/folder pages inject `<h1 class="moss-folder-title">` downstream
    // (render/html.rs, render/blocking.rs) from `doc.title`; the body is
    // passed through untouched here. Nav pages suppress the injected title
    // because the nav bar already surfaces it.
    if heading.visible && !is_nav_page {
        // A pure prepend, so it becomes a leading segment — no flat copy to
        // drift from the pieces it was built from.
        body_plan.prepend_html(inject_article_title_h1("", &heading.text, emit_source_lines));
    }

    // Look up pre-computed url_path from PageMap, falling back to inline computation
    let url_path = page_map.get(file_path).cloned().unwrap_or_else(|| {
        compute_url_path(
            file_path,
            is_index_file,
            frontmatter.url.as_deref(),
            &clean_stem,
        )
    });

    // Extract date from frontmatter
    let date = frontmatter.date;

    // Extract weight from frontmatter
    let weight = frontmatter.weight;

    // Extract analytics configuration from frontmatter
    let analytics = frontmatter.analytics;

    // Extract cover image from frontmatter and resolve to root-relative path.
    // After the resolve phase, wikilinks are already resolved to paths
    // (possibly with |attrs appended). The wikilink fallback handles direct
    // process_markdown_file calls that bypass the resolve phase.
    //
    // Pipe-aware: split on | to separate path from display attrs, resolve
    // only the path part, then rejoin with attrs for downstream rendering.
    let cover = frontmatter.cover.map(|c| {
        let trimmed = c.trim();
        // Strip optional surrounding quotes. A lone `"` is not a quoted empty
        // string — `strip_prefix`/`strip_suffix` require two distinct quotes,
        // where the old start/end test accepted one and then sliced `1..0`.
        let unquoted = trimmed
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .or_else(|| {
                trimmed
                    .strip_prefix('\'')
                    .and_then(|s| s.strip_suffix('\''))
            })
            .unwrap_or(trimmed);
        // Embed wikilink: strip ![[...]] — pipe content = display params (preserved)
        // Regular wikilink: strip [[...]] — pipe content = alias (discarded per Obsidian convention)
        let (is_embed, wikilink_inner) = if let Some(inner) = unquoted
            .strip_prefix("![[")
            .and_then(|s| s.strip_suffix("]]"))
        {
            (true, inner)
        } else if let Some(inner) = unquoted
            .strip_prefix("[[")
            .and_then(|s| s.strip_suffix("]]"))
        {
            (false, inner)
        } else {
            (false, "")
        };
        let resolved_path = if !wikilink_inner.is_empty() {
            let inner = wikilink_inner;
            // Split on | to separate path from pipe content
            let (path_part, attrs_str) = moss_core::media::split_pipe(inner);
            let resolved = path_part.trim_start_matches('/').to_string();
            // Only preserve pipe attrs for embed syntax (![[...|attrs]])
            // For regular wikilinks ([[...|alias]]), discard the alias
            if is_embed && !attrs_str.is_empty() {
                format!("{}|{}", resolved, attrs_str)
            } else {
                resolved
            }
        } else if let Some((alt_section, url_part)) = unquoted
            // Inline image syntax: ![alt|params](url)
            .strip_prefix("![")
            .and_then(|s| s.strip_suffix(')'))
            .and_then(|s| s.rsplit_once("]("))
        {
            // Extract attrs from alt text (after pipe)
            let attrs_str = alt_section.split_once('|').map_or("", |(_, attrs)| attrs);

            let resolved = resolve_to_root_relative(url_part, &url_path);
            if attrs_str.is_empty() {
                resolved
            } else {
                format!("{}|{}", resolved, attrs_str)
            }
        } else {
            // Plain or relative path — split on | before resolving
            let (path_part, attrs_str) = moss_core::media::split_pipe(unquoted);
            let resolved = resolve_to_root_relative(path_part, &url_path);
            if attrs_str.is_empty() {
                resolved
            } else {
                format!("{}|{}", resolved, attrs_str)
            }
        };
        resolved_path
    });

    // Extract page tree fields from frontmatter
    let nav = frontmatter.nav;
    let draft = frontmatter.draft;
    let listed = frontmatter.listed;
    let description = frontmatter.description;
    // Doc-level tag union — rules owned by `moss_core::terms::merge_tag_lists`
    // (frontmatter first, inline #tags appended case-insensitively; both
    // absent stays None so the folder cascade still fills). The pre-merge
    // frontmatter list is kept apart because only IT derives term pages:
    // inline hashtags are prose, not cataloguing — measured on harbor,
    // every one of its six inline tags was rhetorical emphasis mid-sentence,
    // and deriving pages from them shipped six junk pages.
    let fm_tags = frontmatter.tags.clone();
    let tags = moss_core::terms::merge_tag_lists(frontmatter.tags, inline_tags);
    let children = frontmatter.children;
    let children_source = frontmatter.children_source;
    let sidebar = frontmatter.sidebar;
    // Frontmatter struct keeps `Option<String>` (matches YAML). Wrap
    // here so ParsedDocument carries the origin (Frontmatter) into the
    // renderer; the cascade pass tags inherited values with Cascade
    // origin, and the renderer's auto-detection produces Auto origin.
    // Renderers branch on `is_explicit()` to gate axis-driven overrides.
    let children_style = frontmatter
        .children_style
        .map(moss_core::Resolved::frontmatter);
    let children_group = frontmatter
        .children_group
        .map(moss_core::Resolved::frontmatter);
    let children_depth = frontmatter.children_depth;
    let children_in = frontmatter.children_in;
    let children_limit = frontmatter.children_limit;
    let children_more = frontmatter.children_more;
    let children_covers = frontmatter.children_covers;
    let from_sidebar_alias = frontmatter._from_sidebar_alias;
    let series = frontmatter.series;
    let breadcrumb = frontmatter.breadcrumb;
    let footer = frontmatter.footer;
    // BTreeMap, not HashMap (moss#922 PageFacade determinism — see the doc
    // comment on ParsedDocument::cascade); moss_core's typed FrontMatter
    // keeps HashMap since it's a broader public/frontend-facing type.
    let cascade = frontmatter.cascade.map(|m| m.into_iter().collect());
    let also_in = frontmatter.also_in;
    let comments = frontmatter.comments;

    // Calculate reading time (200 words per minute)
    let word_count = markdown_content_raw.split_whitespace().count();
    let reading_time = std::cmp::max(1, (word_count / 200) as u32);

    // Generate enhanced fields following SSG best practices
    let slug = super::frontmatter::generate_slug(&title);
    let permalink = format!("/{}", url_path); // allow:served-path-url-construct (page permalink derived from doc url_path, stored in ParsedDocument)
    let media_items = crate::build::media_collection::extract_media_items(&body_plan.to_html(), &url_path, doc_lang);

    // Adjust relative paths for pretty URLs: non-index files get wrapped in a subdirectory
    // (e.g., article.md → article/index.html), so relative paths need one extra "../"
    if !is_index_file {
        body_plan.map_html(&adjust_relative_paths_for_pretty_urls);
    }

    // The plan, flattened. Consumers that only want bytes read this; the render
    // phase reads `body_plan` when it needs to know what the bytes are made of.
    let html_content = body_plan.to_html();

    // Apply the same adjustment to hero HTML (hero images also need "../" for pretty URLs)
    let hero_html = if !is_index_file {
        hero_html.map(|h| adjust_relative_paths_for_pretty_urls(&h))
    } else {
        hero_html
    };

    Ok(ParsedDocument {
        label,
        title,
        content: markdown_content_raw,
        html_content,
        url_path,
        date,
        reading_time,
        slug,
        permalink,
        weight,
        analytics,
        logo: frontmatter.logo,
        cover,
        cover_type: frontmatter.cover_type,
        nav,
        is_root_level,
        // Centralized home-override signal: the ONE place this is derived.
        // Render code (html.rs) reads `doc.is_home_override` instead of
        // re-deriving from `translation_key == "home"`. Same signal that
        // drives `is_index_by_home_override` above (page_map URL shape).
        is_home_override: is_index_by_home_override,
        draft,
        listed,
        description,
        // Byline rows are already normalized (one row per line, trimmed,
        // blanks dropped) by the field's deserializer, which shares
        // `frontmatter_union::normalize_byline` with the editor.
        byline: frontmatter.byline.unwrap_or_default(),
        colophon: frontmatter.colophon.unwrap_or_default(),
        tags,
        fm_tags,
        author: frontmatter.author.unwrap_or_default(),
        editor: frontmatter.editor.unwrap_or_default(),
        jury: frontmatter.jury.unwrap_or_default(),
        author_page: frontmatter.author_page,
        tag_page: frontmatter.tag_page,
        editor_page: frontmatter.editor_page,
        jury_page: frontmatter.jury_page,
        // Resolved later by `build::terms::derive_terms` (claim winners only).
        term_listing: None,
        term_sections: None,
        children,
        children_source,
        sidebar,
        children_style,
        children_group,
        children_depth,
        children_in,
        children_limit,
        children_more,
        children_covers,
        from_sidebar_alias,
        series,
        breadcrumb,
        footer,
        footer_align: None,
        cascade,
        also_in,
        comments,
        media_items,
        lang: doc_lang,
        lang_tag: doc_lang_tag,
        clean_stem,
        translation_key: frontmatter.translation_key,
        translations: vec![],
        hero_html,
        hero_image_url,
        body_cover_path,
        hero_overlay_text,
        features: crate::build::types::PageFeatures {
            inline_subscribe: has_inline_subscribe,
            inline_apply: has_inline_apply,
            callouts: has_callout,
            footnotes: has_footnotes,
        },
        slot: frontmatter.slot.clone(),
        // Reserved-name convention: `footer.md` at the project root parses
        // through the standard pipeline but is NEVER emitted as a page;
        // `collect_footer_slots_by_language` picks up its `html_content` for the
        // `footer-left` slot. The flag is set structurally (by filename),
        // not from any YAML key. See PR7b in
        // `docs/archive/2026-05-27-phase4-typed-ast-completion.md` for the
        // typed-AST mirror on `moss_core::ast::Document::slot_only`.
        slot_only: crate::build::footer::is_excluded_from_pages(file_path),
        kind: if is_index_file {
            PageKind::Folder
        } else {
            PageKind::Article
        },
        uid: frontmatter.uid,
        typesetting: frontmatter.typesetting,
        content_width: frontmatter.content_width,
        layout: frontmatter.layout,
        source_path: Some(file_path.to_string()),
        url_override: frontmatter.url,
        raw_frontmatter,
        sort: frontmatter.sort.clone(),
        direct_children_sort: None,
        body_plan: Some(body_plan),
        outgoing_links,
        // Transclusion edges are produced one phase earlier, by the resolve
        // pass whose OUTPUT this function parses — `process_markdown_file`
        // never sees an `![[...]]` marker. The Loop A call site in
        // `render/blocking.rs` fills this in from `ResolveResult::embed_deps`.
        embed_deps: Vec::new(),
        missing_media,
    })
}

/// Defensive sweep: scan rendered HTML for `href="..."` values that still
/// carry a resolver prefix (`moss-resolved:`, `wikilink:`, `moss-newtab:`),
/// route each through the supplied `resolve_link` closure, and apply the
/// same post-resolver conversions (wikilink → `class="wikilink"`, moss-newtab
/// → `target="_blank"`).
///
/// Why this exists: shortcodes like `:::buttons` and `:::hero` emit raw
/// HTML (not markdown) with hrefs taken directly from the source. moss-core's
/// resolve pipeline rewrites markdown link sources from `[text](docs/)` to
/// `[text](moss-resolved:docs/index.md)` BEFORE shortcodes run. Shortcode
/// renderers that bypass the markdown parser also bypass `resolve_link`,
/// so the resolver-prefix leaks into the final HTML and breaks the link.
///
/// This sweep catches that leakage. Idempotent: hrefs without a resolver
/// prefix are passed through unchanged.
fn sweep_unresolved_hrefs(html: &str, resolve_link: &dyn Fn(&str) -> String) -> String {
    const PREFIXES: &[&str] = &["moss-resolved:", "wikilink:", "moss-newtab:"];

    // Fast path: no leakage to fix.
    if !PREFIXES
        .iter()
        .any(|p| html.contains(&format!("href=\"{}", p)))
    {
        return html.to_string();
    }

    let mut result = String::with_capacity(html.len());
    let mut cursor = 0;
    let bytes = html.as_bytes();
    while cursor < bytes.len() {
        // Find the next `href="`.
        let needle = b"href=\"";
        // Every cursor position below is either 0 or one past an ASCII `"`, so
        // `get` always yields a tail — it is used instead of a slice so that a
        // future cursor bug degrades instead of aborting the build.
        let Some(from_cursor) = html.get(cursor..) else { break };
        let Some(rel) = from_cursor.find("href=\"") else {
            result.push_str(from_cursor);
            break;
        };
        let href_start = cursor + rel + needle.len();
        // Find the closing quote.
        let Some(from_href) = html.get(href_start..) else { break };
        let Some(close_rel) = from_href.find('"') else {
            // Malformed HTML — emit the rest verbatim.
            result.push_str(from_cursor);
            break;
        };
        let href_end = href_start + close_rel;
        let raw_href = from_href.get(..close_rel).unwrap_or_default();

        // Emit everything up to (but not including) the href value.
        result.push_str(from_cursor.get(..rel + needle.len()).unwrap_or_default());

        // If the href carries a resolver prefix, route through resolve_link
        // and apply the standard post-conversions.
        let needs_resolution = PREFIXES.iter().any(|p| raw_href.starts_with(p));
        if needs_resolution {
            let resolved = resolve_link(raw_href);
            // resolve_link can return wikilink:URL or moss-newtab:URL or a
            // bare URL. Apply the same conversions as the post-parser passes
            // above. The order matters: emit attributes before/after href
            // depending on the prefix.
            if let Some(url) = resolved.strip_prefix("wikilink:") {
                // Need to inject class="wikilink" — but we've already emitted
                // up through `href="`. Backtrack: chop off the `href="` we
                // emitted and re-emit with class first.
                let prelude_end = result.len() - needle.len();
                result.truncate(prelude_end);
                result.push_str("class=\"wikilink\" href=\"");
                result.push_str(url);
            } else if let Some(url) = resolved.strip_prefix("moss-newtab:") {
                let prelude_end = result.len() - needle.len();
                result.truncate(prelude_end);
                result.push_str("target=\"_blank\" rel=\"noopener\" href=\"");
                result.push_str(url);
            } else {
                result.push_str(&resolved);
            }
        } else {
            result.push_str(raw_href);
        }

        // Move cursor past the closing quote.
        result.push('"');
        cursor = href_end + 1;
    }
    result
}

/// Split a URL into (path, suffix) where `suffix` is `?query` and/or `#fragment`
/// in source order. Mirrors `moss_core::ast::resolve_urls::split_path_suffix`
/// (the typed-AST visitor's local helper) — the path is what's looked up
/// in `page_map`; the suffix passes through verbatim to the final href so
/// query strings survive resolution.
///
/// Suffix is opaque — round-trip parity with `classify_url_prod` is the
/// contract; both implementations must share this exact shape.
fn split_path_suffix(url: &str) -> (&str, Option<&str>) {
    // `find` over a char set returns the FIRST of either delimiter, which is
    // what "in source order" means here.
    match url.find(['?', '#']) {
        Some(pos) => (url.get(..pos).unwrap_or(url), url.get(pos..)),
        None => (url, None),
    }
}

/// Root-absolute pretty URL for a url_path — `"awards/index.html"` → `"/awards/"`,
/// `"index.html"` → `"/"`. Depth-independent by construction, which is what slot
/// HTML needs: it is injected into pages at every depth.
fn absolute_pretty_url(to_url_path: &str) -> String {
    match url_path_to_dir(to_url_path).trim_matches('/') {
        "" => "/".to_string(),
        dir => format!("/{}/", dir), // allow:served-path-url-construct (page permalink for slot HTML, same class as the url_path permalink above)
    }
}

/// Compute relative pretty URL between two url_paths.
/// Both are like "folder/slug/index.html".
/// Result is a relative URL like "../sibling/" or "child/".
fn relative_pretty_url(from_url_path: &str, to_url_path: &str) -> String {
    // Convert url_paths to their pretty URL directories
    let from_dir = url_path_to_dir(from_url_path);
    let to_dir = url_path_to_dir(to_url_path);

    let from_parts: Vec<&str> = if from_dir.is_empty() {
        vec![]
    } else {
        from_dir.split('/').collect()
    };
    let to_parts: Vec<&str> = if to_dir.is_empty() {
        vec![]
    } else {
        to_dir
            .trim_end_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect()
    };

    let common = from_parts
        .iter()
        .zip(to_parts.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let ups = from_parts.len() - common;
    let remaining = &to_parts[common..];

    let mut result = String::new();
    if ups == 0 && remaining.is_empty() {
        return "./".to_string();
    }
    for _ in 0..ups {
        result.push_str("../");
    }
    for (i, part) in remaining.iter().enumerate() {
        if i > 0 {
            result.push('/');
        }
        result.push_str(part);
    }
    if !result.ends_with('/') {
        result.push('/');
    }
    result
}

/// Convert a url_path like "posts/hello/index.html" to its directory.
/// "posts/hello/index.html" -> "posts/hello"
/// "index.html" -> ""
fn url_path_to_dir(url_path: &str) -> String {
    if url_path.ends_with("/index.html") {
        url_path
            .strip_suffix("/index.html")
            .unwrap_or("")
            .to_string()
    } else if url_path == "index.html" {
        String::new()
    } else {
        // e.g. "posts/hello.html" -> "posts/hello" (strip .html, treat as dir)
        url_path
            .strip_suffix(".html")
            .unwrap_or(url_path)
            .to_string()
    }
}

// Phase 4 PR7a-fragment (2026-05-28): the legacy event-pipeline block
// that lived here — `byte_offset_to_line`, `transform_events`
// (~860 LOC), and its caption-folding / image-detection / raw-HTML-media
// helpers (`emit_standalone_figure_image`, `raw_html_media_kind`,
// `strip_resolver_sentinel`, `render_inline_md_for_dispatch`,
// `is_image_then_emphasis`, `collect_image_alt`, `bare_image_paragraph_alt`,
// `extract_html_attr`, `image_paragraph_has_non_empty_alt`,
// `is_image_only_paragraph`, `EmphasisParagraph`,
// `peek_emphasis_paragraph`) — deleted in one shot. `render_markdown_to_html_with`
// is now the only fragment renderer, and it routes through
// `moss_core::ast::parse` + `render_document`, matching the production
// path used by `process_markdown_file`.
//
// `data-source-line` annotations (scroll-sync) and implicit-figure flag
// gating are not wired through the AST renderer yet; both were no-ops
// on the production path before PR7a-fragment too (see the `let _ = …`
// at the AST render call in `process_markdown_file`). They will return
// as Block-level span carriers + `ParseConfig` in a follow-up if the
// scroll-sync feature returns.

/// Render markdown to HTML through the typed-AST renderer using a
/// default link resolver (`transform_markdown_link` — `.md` → pretty URL).
///
/// Used by in-file regression tests that pin the fragment-path byte
/// shape (no media synth, no figure synthesizer, no `<picture>` wrap):
/// `moss:` title pass-through, plain anchor emission, bare-image
/// preservation, raw-`<img>` opaque HTML. Production page rendering
/// flows through `process_markdown_file`, which threads the full
/// resolution chain (graph + registry + asset snapshot + media lookup).
pub fn render_markdown_to_html(markdown: &str) -> String {
    let default_resolver = |href: &str| -> String { transform_markdown_link(href) };
    render_markdown_to_html_with(markdown, &default_resolver, None, true)
}

/// Like `render_markdown_to_html` but with a custom link resolver, an
/// optional media-dimension lookup, and the site's `heading_anchors`
/// preference (the public no-config wrapper above always passes `true`;
/// `Shortcode::Recent`'s fallback-markdown call site passes the real
/// `self.heading_anchors` it was constructed with — see moss#915).
///
/// Phase 4 PR7a-fragment (2026-05-28): the body now routes through the
/// typed AST (`moss_core::ast::parse` → `render_document`), matching the
/// production path used by `process_markdown_file`. Before PR7a-fragment,
/// this function called the legacy `transform_events` event-pipeline; now
/// every markdown-to-HTML path in moss flows through one renderer.
///
/// Fragment context (no ContentGraph, no `renderer_registry`):
/// - **No wikilink dispatch.** The AST visitor `dispatch_wikilink_embeds`
///   needs a graph + registry; with neither, wikilink-form refs reach the
///   renderer as `Url::Unresolved` and the host's `resolve_link` closure
///   takes over via `classify_url_prod`. Fragment callers (the in-file
///   tests that exercise `render_markdown_to_html` directly) do not feed
///   wikilink syntax through this path today.
/// - **No source-line / implicit-figure flags.** The `_with` signature
///   never accepted those — they were always-false in the legacy call.
///   Implicit-figure promotion lives in the AST parser unconditionally
///   (parser.rs's `try_promote_to_figure` over image-only paragraphs);
///   the parity-probe tests treat that as page-level behavior matching
///   today's `process_markdown_file`.
///
/// Phase 4 PR4.5 (2026-05-28): the grid-shortcode caller (`render_card_html`)
/// was deleted; grid cells now render through the typed AST in
/// `moss_core::ast::hooks::DefaultHooks::render_shortcode`. Hero overlay
/// HTML likewise routes through `render_blocks` inside `DefaultHooks`.
/// moss#915: `Shortcode::Recent`'s fallback-markdown render (this file's
/// `RenderHooks::render_shortcode`) is a production caller again — it
/// previously hand-rolled a bare `pulldown_cmark::Parser::new` scan, which
/// silently mishandled footnotes/wikilinks/tables (ADR-036's "parse once"
/// bug class). This helper's parse->resolve->render pipeline is the
/// sanctioned fix, not a new one-off parser.
pub fn render_markdown_to_html_with(
    markdown: &str,
    resolve_link: &dyn Fn(&str) -> String,
    media_lookup: Option<&crate::build::media::dimensions::MediaDimensionLookup>,
    heading_anchors: bool,
) -> String {
    // Idempotent pass: `process_markdown_file` already cleans page-level
    // markdown, but this helper is also callable from `render_markdown_to_html`
    // (with a default resolver). The `!text.contains('{')` fast path makes
    // the double-apply free when the input is already clean.
    let accepted = accept_criticmarkup(markdown);
    // Strip %%...%% Obsidian-style comments. Cheap no-op when no `%%` present.
    let accepted = strip_percent_comments(&accepted);

    // Phase 4 PR7a-fragment: parse markdown into the typed AST. Mirrors
    // the call order in `process_markdown_file` minus the production-only
    // passes (`dispatch_wikilink_embeds`, `resolve_urls`, `extract_hero`,
    // `find_first_block_image`) which require a `ContentGraph` and apply
    // only to page-level documents.
    let mut doc = moss_core::ast::parse(&accepted);

    // Host-context URL classification. Apply the caller's `resolve_link`
    // closure to every `Url::Unresolved` in the AST. Same shape as the
    // production path's `classify_url_prod` step (see `process_markdown_file`).
    moss_core::ast::visit_urls_mut(&mut doc, |url| {
        if let moss_core::ast::Url::Unresolved(raw) = url {
            *url = classify_url_prod(raw, &resolve_link);
        }
    });

    // Build PipelineHooks with the optional asset snapshot. When
    // `media_lookup` is `Some`, the snapshot drives `<picture>`/dims/LQIP
    // byte shape for images that reach `render_image`. When `None`, the
    // hooks delegate to `DefaultHooks::new()` and images emit the bare
    // `<img>` shape — same as the legacy `transform_events` fragment path
    // with `media_lookup=None`.
    // Real dir_overrides, so the snapshot's output-URL keys are the strings the
    // synthesizer probes with. An empty map re-fires the 800x600 fallback.
    let asset_snapshot = media_lookup.map(|l| l.asset_snapshot());
    let pipeline_hooks = PipelineHooks {
        // No site context in the fragment path; `Subscribe` shortcodes
        // inside fragments are not a supported use case (the typed-AST
        // tests in this module exercise fragment URLs, not shortcodes).
        site_id: "",
        lang: crate::i18n::Language::En,
        seta_base: "https://api.mosspub.com",
        assets: asset_snapshot,
        media_lookup,
        permalink_section_label: crate::i18n::t(crate::i18n::Language::En, "permalink_section"),
        heading_anchors,
    };

    moss_core::ast::render_document(&doc, &pipeline_hooks)
}

/// URL-prettification only. Converts `.md` / `.markdown` hrefs to pretty
/// URLs (trailing slash or folder-index collapse) and leaves external /
/// anchor / mailto URLs untouched.
///
/// This function does NOT perform content-graph resolution. By the time
/// an href reaches it, `moss-core`'s resolve pipeline has already rewritten
/// any resolvable reference through `ContentGraph::resolve_path`. Any href
/// here is either external, pre-resolved (then stripped of the
/// `moss-resolved:` prefix), or an already-absolute-like path that only
/// needs format normalization.
///
/// # Examples
/// - `transform_markdown_link("./roadmap.md")` → `"./roadmap/"`
/// - `transform_markdown_link("file.md#anchor")` → `"file/#anchor"`
/// - `transform_markdown_link("https://example.com/file.md")` → unchanged
pub fn transform_markdown_link(url: &str) -> String {
    // Don't transform external URLs
    if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("//") {
        return url.to_string();
    }

    // Split URL into path and fragment (the `#` stays with the fragment)
    let (path, fragment) = match url.find('#') {
        Some(pos) => (url.get(..pos).unwrap_or(url), url.get(pos..)),
        None => (url, None),
    };

    // Transform .md or .markdown to pretty URL format
    let transformed_path = if path.ends_with(".md") || path.ends_with(".markdown") {
        let stem = if path.ends_with(".md") {
            path.strip_suffix(".md").unwrap()
        } else {
            path.strip_suffix(".markdown").unwrap()
        };
        let filename = stem.rsplit('/').next().unwrap_or(stem);
        if moss_core::home::is_index_stem(filename) {
            let dir = stem.strip_suffix(filename).unwrap_or("");
            if dir.is_empty() {
                "./".to_string()
            } else {
                dir.to_string()
            }
        } else {
            format!("{}/", stem)
        }
    } else {
        path.to_string()
    };

    // Recombine with fragment if present
    if let Some(frag) = fragment {
        transformed_path + frag
    } else {
        transformed_path
    }
}

// ---------------------------------------------------------------------------
// Phase B production path (typed-ast-migration)
// ---------------------------------------------------------------------------

/// Render hooks for the moss build pipeline. Knows site context
/// (site_id, lang) needed to render shortcodes whose HTML depends on
/// per-site state (subscribe form's API endpoint, etc.).
///
/// Phase 2E v5 PR3 (2026-05-26): also carries an optional
/// [`AssetSnapshot`] reference, forwarded into
/// `DefaultHooks::with_snapshot` at the Gallery delegation point so
/// gallery items emit the canonical `<picture>`/dims/LQIP byte shape
/// via [`moss_core::render::image::synthesize_image_html`]. The
/// trait's `gallery_assets` override is exposed on `DefaultHooks`
/// only, not on `PipelineHooks` — `PipelineHooks` delegates to
/// `DefaultHooks::with_snapshot(self.assets)` for the Gallery arm.
///
/// After PR3 this struct reduces to a **Subscribe-only override**
/// (Hero + Grid are intercepted upstream as `unreachable!`; Buttons
/// + Gallery delegate to `DefaultHooks`). Full deletion is a
/// follow-up if Subscribe's `site_id`/`lang` can move into
/// `DefaultHooks` state too.
struct PipelineHooks<'a> {
    site_id: &'a str,
    lang: crate::i18n::Language,
    /// The seta base URL to use in subscribe form `action=` attributes.
    ///
    /// Resolved from the project's `environment` field in `.moss/config.toml`
    /// via `resolve_environment(project_path).seta_url()`. Defaults to the
    /// production URL (`"https://api.mosspub.com"`) when `project_path` is not
    /// available (test / fragment-render paths) — those callers pass `None` to
    /// `process_markdown_file`'s `seta_url` parameter and the default is applied.
    seta_base: &'a str,
    /// `Some` in the production path (an `AssetSnapshot` is built from
    /// the `MediaDimensionLookup` in `apply_typed_shortcodes`); `None`
    /// in test / fragment-render paths where no snapshot is in scope.
    /// Drives the gallery synth's snapshot-aware byte shape.
    assets: Option<&'a moss_core::asset_snapshot::AssetSnapshot>,
    /// `Some` in the production path; `None` in test / fragment-render
    /// paths. Phase 4 PR7a-flip-core-A (2026-05-28): plumbed through so
    /// the `Hero` arm of [`render_shortcode`] can delegate to
    /// [`crate::build::markdown::typed_renderers::render_hero_html_typed`]
    /// for full Hero byte shape (synth-emitted `<img>`, overlay
    /// rendering, width attrs).
    ///
    /// The Hero arm is **still unreachable from current production**:
    /// `apply_typed_shortcodes` intercepts Hero entries upstream and
    /// hoists them to the article-template hero slot via a direct call
    /// to `render_hero_html_typed`. PR7a-flip-core-B will remove that
    /// upstream hoist and rely on this arm. The wiring lands here so the
    /// flip is a one-spot change.
    media_lookup: Option<&'a crate::build::media::dimensions::MediaDimensionLookup>,
    /// Localized `aria-label` for the per-heading permalink anchor.
    ///
    /// moss-core's `render_heading` emits `<a class="moss-heading-anchor"
    /// aria-label="…">` on every slugged heading; moss-core is pure Rust
    /// with no i18n table, so the caller resolves the string here via
    /// `crate::i18n::strings::t(lang, "permalink_section")` and the
    /// `permalink_section_label` trait override hands it back to moss-core.
    permalink_section_label: &'a str,
    /// `[site].heading_anchors`, resolved on `SiteConfig` (absent key =>
    /// true). Consumed by the `emit_heading_anchors` hook override below —
    /// when false, headings render without the trailing `#` permalink
    /// anchor.
    heading_anchors: bool,
}

impl<'a> PipelineHooks<'a> {
    /// `DefaultHooks` bound to this render's asset snapshot, for the shortcode
    /// arms that delegate their whole byte shape to moss-core.
    fn asset_bound_hooks(&self) -> moss_core::ast::DefaultHooks<'a> {
        match self.assets {
            Some(a) => moss_core::ast::DefaultHooks::with_snapshot(a),
            None => moss_core::ast::DefaultHooks::new(),
        }
    }
}

impl<'a> moss_core::ast::RenderHooks for PipelineHooks<'a> {
    fn permalink_section_label(&self) -> &str {
        self.permalink_section_label
    }

    fn emit_heading_anchors(&self) -> bool {
        self.heading_anchors
    }

    /// The single place grid rendering is bound to the asset snapshot.
    ///
    /// Grid cells render their images through the synth (`<picture>`, dims,
    /// LQIP, `sizes=`), which needs `self.assets`. Recursing through `self`
    /// instead would drop the snapshot-derived `sizes=`, so both the flat
    /// `Shortcode::Grid` arm and the segmented render funnel through here — one
    /// binding, one byte shape.
    ///
    /// `grid_cells` rather than `asset_bound_hooks`: a heading in a cell is a
    /// card title, so it carries no `#` permalink (see
    /// [`DefaultHooks::grid_cells`]).
    fn render_grid_parts(
        &self,
        args: &moss_core::ast::GridShortcode,
        source_line: Option<usize>,
    ) -> moss_core::ast::GridParts {
        let hooks = moss_core::ast::DefaultHooks::grid_cells(self.assets);
        moss_core::ast::render_grid_parts(&hooks, args, source_line)
    }

    /// Phase 4 PR1 (2026-05-27): route the inline `Inline::Image`
    /// emission through `DefaultHooks::with_snapshot(self.assets)` so
    /// production renders via `synthesize_image_html` with the populated
    /// asset snapshot (real dims/LQIP/`<picture>` wrap). When no snapshot
    /// is in scope (test / fragment-render paths), fall through to
    /// `DefaultHooks::new()`'s bare-`<img>` default — same shape as
    /// before PR1 for those paths.
    ///
    /// This override exists so that the moment `render_document` becomes
    /// the production path (PR7a), inline image emission is asset-aware
    /// without code changes at the call site. Until then the wikilink
    /// dispatcher in `transform_events` carries the load — this hook is
    /// reachable only via `observe_typed_ast` / future test paths.
    ///
    /// `img_style` (fit/position from a parameterized wikilink embed) and
    /// `width` (the figure's `data-width` token) ride the same hook, so the
    /// snapshot-aware synth path threads them onto the `<img>` and figures
    /// with fit/position cannot silently lose them.
    fn render_image(
        &self,
        out: &mut String,
        src: &moss_core::ast::ResolvedUrl,
        alt: &str,
        title: Option<&str>,
        img_style: Option<&str>,
        width: Option<&str>,
    ) {
        let inner = self.asset_bound_hooks();
        <moss_core::ast::DefaultHooks<'_> as moss_core::ast::RenderHooks>::render_image(
            &inner, out, src, alt, title, img_style, width,
        );
    }

    /// Phase 4 PR1 (2026-05-27): delegate to `DefaultHooks` for link
    /// emission. The default impl already handles every UrlKind variant
    /// (`Wikilink` → `class="wikilink"`, `AssetNewtab` → `target="_blank"
    /// rel="noopener"`, others → plain `<a href>`); the override here is
    /// for symmetry with `render_image` (so a future change to the
    /// default's byte shape lands in one place — `DefaultHooks` — without
    /// risking diverged PipelineHooks output).
    ///
    /// Phase 4 PR7a-flip-core-A (2026-05-28): signature now carries
    /// `is_wikilink: bool` — the parse-time wikilink discriminator from
    /// pulldown-cmark's `LinkType::WikiLink`. Passed through to the
    /// default impl so a wikilink that resolves to an asset-newtab kind
    /// emits BOTH `class="wikilink"` AND `target="_blank" rel="noopener"`
    /// (orthogonal flags). Pre-flip-core-A, the renderer synthesized a
    /// wikilink-kinded `ResolvedUrl` to coax the wikilink class — that
    /// workaround dropped the asset-newtab kind silently.
    fn render_link(
        &self,
        out: &mut String,
        url: &moss_core::ast::ResolvedUrl,
        is_wikilink: bool,
        content: &str,
    ) {
        let inner = moss_core::ast::DefaultHooks::new();
        <moss_core::ast::DefaultHooks<'_> as moss_core::ast::RenderHooks>::render_link(
            &inner,
            out,
            url,
            is_wikilink,
            content,
        );
    }

    /// ADR-030 P2: typeset the equation to an inline SVG via RaTeX, behind the
    /// crash-prevention envelope in [`crate::build::markdown::math`]. On any
    /// refusal — a guard rejection (over-length, deep nesting, CJK/emoji, a
    /// dangerous macro), a parse failure, or output validation — fall back to
    /// the P1 escaped-source `<code class="moss-math">` node, so an equation is
    /// never blank and the build never aborts. Only *surprising* refusals
    /// (parse/render/validation) warn; expected guard rejections (the CJK
    /// currency false positive, deliberately-adversarial input) stay quiet and
    /// are surfaced by `moss doctor --math` instead.
    fn render_math(&self, out: &mut String, tex: &str, display: bool, fallback_html: &str) {
        match crate::build::markdown::math::render_math(tex, display) {
            Ok(svg) => out.push_str(&svg),
            Err(refusal) => {
                if refusal.is_unexpected() {
                    log::warn!("math typeset refused ({refusal:?}); falling back to source: {tex:?}");
                }
                out.push_str(fallback_html);
            }
        }
    }

    fn render_shortcode(
        &self,
        out: &mut String,
        sc: &moss_core::ast::Shortcode,
        source_line: Option<usize>,
    ) {
        match sc {
            moss_core::ast::Shortcode::Subscribe(args) => {
                // Task 1.7 deferred-inline-scope follow-up:
                //
                // The footer auto-injected form (in `generate_native_slots`)
                // ships per-page scope tagging in this commit. The inline
                // `:::subscribe` shortcode below still emits scope="" — the
                // legacy v0 behavior — because deriving the page's scope
                // here requires both `page_url_path` (per-file, available)
                // AND `supported_scopes` (per-build, derived from the full
                // ParsedDocument set, NOT in scope at `process_markdown_file`
                // time per the v1 plan's architectural note). Threading
                // `supported_scopes` into `process_markdown_file` would touch
                // every caller (footer.rs, tests, etc.) for a payload that
                // a small minority of sites use. Footer covers the v1 happy
                // path; inline `:::subscribe` users get scope="" until the
                // follow-up restructures the pipeline to mint
                // `supported_scopes` once per build and pass it through.
                //
                // See docs/archive/2026-05-27-newsletter-v1-implementation-v2.md
                // Task 1.7 Step 4 for the deferral discussion.
                // Unpublished sites (empty site_id) render the pending form,
                // consistent with the auto-injected footer form.
                let html = crate::build::features::email::render_hosted_subscribe_form(
                    Some(self.site_id).filter(|s| !s.is_empty()),
                    self.lang,
                    "",
                    args.placeholder.as_deref(),
                    args.button.as_deref(),
                    self.seta_base,
                );
                out.push_str(&html);
            }
            moss_core::ast::Shortcode::Grid(args) => {
                // moss-core's own Grid arm, verbatim: the asset binding lives
                // in `render_grid_parts` above, so the flat and segmented
                // renders of a grid cannot diverge.
                out.push_str(&self.render_grid_parts(args, source_line).to_html());
            }
            moss_core::ast::Shortcode::Buttons(_) | moss_core::ast::Shortcode::Gallery(_) => {
                // Delegate to DefaultHooks. For Buttons, this is a pure
                // byte-for-byte handoff. For Gallery, Phase 2E v5 PR3
                // (2026-05-26): with a snapshot in scope the synth path emits
                // the canonical `<picture><source srcset=*.webp>` shape with
                // dims/LQIP/loading="lazy"; without one, the bare-`<img>`
                // shape.
                let inner = self.asset_bound_hooks();
                <moss_core::ast::DefaultHooks<'_> as moss_core::ast::RenderHooks>::render_shortcode(
                    &inner,
                    out,
                    sc,
                    source_line,
                );
            }
            moss_core::ast::Shortcode::Hero(args) => {
                // Phase 4 PR7a-flip-core-A (2026-05-28): delegate to
                // `render_hero_html_typed` — the existing typed Hero
                // renderer used by the upstream hoist path in
                // `apply_typed_shortcodes`. Forward-prep for
                // PR7a-flip-core-B which will REMOVE the upstream hoist
                // and rely on this arm to produce the body's Hero HTML.
                //
                // Reachability today: Hero is still hoisted upstream
                // (line ~2932) and the placeholder is replaced with
                // empty string, so the AST never carries a `Shortcode::Hero`
                // by the time `render_document` walks the body. This arm
                // is reachable only via direct calls to
                // `RenderHooks::render_shortcode` (the parity probe in
                // `observe_typed_ast` runs on the pre-hoist AST and exercises
                // this branch).
                //
                // `render_hero_html_typed`'s `_resolve_link` parameter
                // is unread (URL resolution already happened in
                // `resolve_shortcode_urls`). We pass an identity closure
                // to satisfy the signature without coupling
                // `PipelineHooks` to a generic resolver type.
                let resolver_noop = |s: &str| s.to_string();
                let hero_mobile_bg: Option<String> = self.media_lookup.and_then(|lookup| {
                    match args.image.as_ref() {
                        Some(moss_core::ast::Url::Resolved(r)) => lookup.get_cover_color_muted(&r.href),
                        _ => None,
                    }
                });
                let html = crate::build::markdown::typed_renderers::render_hero_html_typed(
                    args,
                    &resolver_noop,
                    self.media_lookup,
                    source_line,
                    hero_mobile_bg.as_deref(),
                );
                out.push_str(&html);
            }
            moss_core::ast::Shortcode::Recent(args) => {
                out.push_str(&crate::build::markdown::recent::render_web(
                    args,
                    self.media_lookup,
                    self.heading_anchors,
                ));
            }
            moss_core::ast::Shortcode::Apply(args) => {
                // Empty scope ("") is an intentional Task-1.7 deferral — mirroring
                // the Subscribe arm above. See the comment there for the full rationale.
                let html = crate::build::features::email::render_inline_apply_form(
                    self.site_id,
                    self.lang,
                    "",
                    args.placeholder.as_deref(),
                    args.button.as_deref(),
                    self.seta_base,
                );
                out.push_str(&html);
            }
        }
    }
}

/// Classify a raw URL into [`moss_core::ast::Url`], decoding the three
/// sentinel prefixes the upstream `resolve_link` closure can emit
/// (`wikilink:`, `moss-newtab:`, bare URL) into [`UrlKind`] variants.
///
/// Called from `process_markdown_file` via `visit_urls_mut` to classify
/// host-context URLs the typed-AST `resolve_urls` pass left as
/// `Url::Unresolved`.
fn classify_url_prod<R: Fn(&str) -> String>(raw: &str, resolve_link: &R) -> moss_core::ast::Url {
    use moss_core::ast::{ResolvedUrl, Url, UrlKind};

    if let Some(rest) = raw.strip_prefix("mailto:") {
        return Url::Resolved(ResolvedUrl::new(format!("mailto:{rest}"), UrlKind::Mailto));
    }
    if let Some(rest) = raw.strip_prefix("tel:") {
        return Url::Resolved(ResolvedUrl::new(format!("tel:{rest}"), UrlKind::Tel));
    }
    if raw.starts_with('#') {
        return Url::Resolved(ResolvedUrl::new(raw.to_string(), UrlKind::Anchor));
    }
    if raw.starts_with("http://")
        || raw.starts_with("https://")
        || raw.starts_with("//")
        || raw.starts_with("data:")
    {
        return Url::Resolved(ResolvedUrl::new(raw.to_string(), UrlKind::External));
    }

    let resolved = resolve_link(raw);
    if let Some(href) = resolved.strip_prefix("wikilink:") {
        Url::Resolved(ResolvedUrl::new(href.to_string(), UrlKind::Wikilink))
    } else if let Some(href) = resolved.strip_prefix("moss-newtab:") {
        Url::Resolved(ResolvedUrl::new(href.to_string(), UrlKind::AssetNewtab))
    } else {
        Url::Resolved(ResolvedUrl::new(resolved, UrlKind::Internal))
    }
}

// Tests for the pipeline module are co-located with their subjects in
// build/markdown/{frontmatter,pipeline,html_post}.rs (split per Phase 3 restructure).

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod tests;
