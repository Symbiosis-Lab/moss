//! Folder embed marker resolution.
//!
//! Consumes `<!--MOSS_MARKER_FOLDER_LIST:path=…|from=…|limit=N|sort=axis-->`
//! markers emitted by moss-core's `embed_renderer::folder_list`
//! (Task 15, triggered by `![[/folder/]]` wikilinks).
//!
//! Three-branch resolution per marker:
//!
//! 1. **Content folder** (target_doc found in `all_docs` for `{folder}/index.html`).
//!    Render a sorted listing of the folder's markdown children using
//!    `child_summary::render_with_sort` — same pipeline as folder index pages.
//!    The marker pass runs over `doc.html_content` AFTER `populate_direct_children_sorts`,
//!    so the target folder's `direct_children_sort` is available for inheritance. The
//!    per-embed `sort=axis` override (when present in the marker) is merged in by
//!    producing a fresh `ResolvedSort` with the requested axis.
//!
//! 2. **Static-index folder** (no content-folder doc, but the folder has a
//!    source `index.html` or `index.htm` in `project.html_files`). Dispatch
//!    into the canonical Stage 2 iframe synthesizer (`synthesize_iframe_html`)
//!    with an empty `TitleParams` — emission shape is identical to
//!    `![[folder/index.html]]`. This is the form for embedding built static
//!    web apps (Vite/Astro/Observable exports).
//!
//! 3. **Missing folder** (neither). Emit a `<div class="moss-embed-missing">…</div>`
//!    diagnostic; the build keeps going.
//!
//! URL-space lookups (target_doc match, folder_prefix filter, "more" href)
//! use the slugified form of `folder_id` because `ParsedDocument.url_path` is
//! slugified. Source-side lookups (e.g. `project.html_files`) use the raw
//! `folder_id` because those carry on-disk paths.

use std::path::Path;

use moss_core::asset_snapshot::AssetSnapshot;
use moss_core::resolve::embed_renderer::folder_list::{marker_decode, MARKER_END, MARKER_FOLDER_LIST};
use moss_core::resolve::embed_renderer::Sizing;
use moss_core::resolve::title_params::TitleParams;
use moss_core::sort::{ResolvedSort, SortAxis};
use moss_core::PageKind;

use crate::build::components::{self, ArticleListItemProps};
use crate::build::components::child_list::ChildItemProps;
use crate::build::components::date as date_formatters;
use crate::build::media::cover::html_escape;
use crate::i18n::Language;
use crate::{build::types::ParsedDocument, types::content::ProjectStructure};

/// Parsed payload from a folder-list marker.
#[derive(Debug, Default, PartialEq)]
struct ParsedMarker<'a> {
    path: &'a str,
    from: &'a str,
    limit: Option<usize>,
    sort: Option<SortAxis>,
    style: Option<String>,
    depth: Option<String>,
    group: Option<String>,
    /// Raw sizing token (e.g. `"80%"`, `"800x600"`). Parsed to a `Sizing`
    /// and applied ONLY to the static-index iframe branch; the listing
    /// (card-grid) branch ignores it.
    size: Option<String>,
    /// Listing filter: `"only"` keeps pages that have a cover. Applied after
    /// flattening and before `limit`. Mirrors `moss_core`'s
    /// `FolderEmbedParams::covers`, which is where this is actually parsed
    /// for a body embed (`![[/|covers:only]]`) — the key also appears here
    /// because this module re-parses the SAME pipe-encoded marker body that
    /// `emit_marker` produced, same as `limit`/`depth`/`group`.
    covers: Option<&'a str>,
    /// Internal filter: root homepage's own listing — scope to the default language
    /// tree (drop language-prefix subtrees) when the site is multilingual (homepage only).
    scope_default_tree: bool,
    /// Internal filter: exclude top-level nav-item folders (homepage only).
    exclude_nav: bool,
    /// `children_more:` / embed `more:<target>` target — a wikilink (or,
    /// once resolved by the frontmatter-wikilink pass, a plain path) to the
    /// page the truncated listing's More link should point at. Set from
    /// frontmatter synthesis (`synthesize_children_marker`, reading
    /// `children_more`) or directly by a body `![[/|more:Archive]]` embed —
    /// both flow through `FolderEmbedParams::more` (moss-core) into this one
    /// marker field, so the render side doesn't need to know which named it.
    more: Option<String>,
    /// The listing's own width / float / size, read off the embed pothole.
    placement: moss_core::media::Placement,
    /// Caption for the listing, rendered on the figure that wraps it.
    caption: Option<String>,
}

fn warn_map_fallback_once(path: &str, reason: &str) {
    static WARNED: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    let key = format!("{path}\0{reason}");
    if crate::infra::warn_once::should_warn_once(
        key,
        WARNED.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new())),
    ) {
        crate::build::cli_output::log_warn_problem!(
            "map embed '{path}' {reason}; rendering its ordinary listing"
        );
    }
}

fn place_map_with_placement(
    svg: String,
    placement: &moss_core::media::Placement,
    caption: Option<&str>,
) -> String {
    let attrs = moss_core::render::placement::placement_attrs(placement);
    let class = attrs.class_value("moss-place-map-frame");
    let caption = caption.map(|text| format!(
        "<div class=\"moss-place-map-caption\">{}</div>",
        moss_core::media::html_escape(text)
    )).unwrap_or_default();
    format!(
        "<div class=\"{class}\"{}{size}>{svg}{caption}</div>",
        attrs.data_width_attr,
        size = attrs.size_style_attr,
    )
}

/// Parse the pipe-encoded body of a marker. Returns None if `path` is missing.
fn parse_marker_body(body: &str) -> Option<ParsedMarker<'_>> {
    let mut out = ParsedMarker::default();
    for tok in body.split('|') {
        let tok = tok.trim();
        if tok.is_empty() {
            continue;
        }
        if let Some((k, v)) = tok.split_once('=') {
            match k.trim() {
                "path" => out.path = v.trim(),
                "from" => out.from = v.trim(),
                "limit" => out.limit = v.trim().parse().ok(),
                "sort" => {
                    out.sort = match v.trim() {
                        "date" => Some(SortAxis::Date),
                        "weight" => Some(SortAxis::Weight),
                        "title" => Some(SortAxis::Title),
                        _ => None,
                    }
                }
                "style" => out.style = Some(v.trim().to_string()),
                "depth" => out.depth = Some(v.trim().to_string()),
                "group" => out.group = Some(v.trim().to_string()),
                "size" => out.size = Some(v.trim().to_string()),
                "more" => out.more = Some(marker_decode(v.trim())),
                "covers" => out.covers = Some(v.trim()),
                "width" => out.placement.width = moss_core::media::match_width_token(v.trim()),
                "align" => {
                    out.placement.align = match v.trim() {
                        "left" => Some(moss_core::media::AlignSide::Left),
                        "right" => Some(moss_core::media::AlignSide::Right),
                        _ => None,
                    }
                }
                "pct" => out.placement.size = Some(v.trim().to_string()),
                "caption" => out.caption = Some(marker_decode(v.trim())),
                _ => {}
            }
        } else if tok == "scope_default_tree" {
            out.scope_default_tree = true;
        } else if tok == "exclude_nav" {
            out.exclude_nav = true;
        }
        // unknown bare flags (e.g. legacy "more") silently ignored
    }
    if out.path.is_empty() {
        return None;
    }
    Some(out)
}

/// Resolve a user-written folder path (possibly with leading `/` or trailing
/// `/`) into the absolute site folder ID — the path of the folder's URL
/// without a trailing slash.
///
/// Examples (from = `index.md`):
///   `/journal/` → `journal`
///   `journal/`  → `journal`
/// Examples (from = `zh-hans/about/index.md`):
///   `/news/`    → `news`      (absolute)
///   `subdir/`   → `zh-hans/about/subdir`  (relative to `from`'s directory)
///   `../news/`  → `zh-hans/news`           (relative, with `..`)
fn resolve_folder_id(path: &str, from: &str) -> String {
    // The bare root `/` must be caught before trailing-slash stripping: once
    // stripped it is "", which no longer `starts_with('/')`, so the absolute
    // check below would miss it and fall through to the relative branch —
    // resolving the site root to `from`'s own directory instead of "".
    if path == "/" {
        return String::new();
    }

    // Strip trailing slash for the folder ID. Trailing slash is purely the
    // embed-trigger syntax — it never reaches the URL path.
    let path = path.trim_end_matches('/');

    if let Some(abs) = path.strip_prefix('/') {
        return abs.trim_start_matches('/').to_string();
    }

    // Relative: anchor at `from`'s parent directory.
    let from_dir = Path::new(from)
        .parent()
        .and_then(|p| p.to_str())
        .unwrap_or("");

    let mut parts: Vec<&str> = if from_dir.is_empty() {
        Vec::new()
    } else {
        from_dir.split('/').filter(|s| !s.is_empty()).collect()
    };
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        if seg == ".." {
            parts.pop();
        } else if seg != "." {
            parts.push(seg);
        }
    }
    parts.join("/")
}

/// Auto-detect children rendering style and grouping.
///
/// Checks whether children have "rich" metadata (covers or descriptions)
/// and whether any child carries a date to pick the best defaults.
/// Author-provided overrides (frontmatter or cascade) take priority
/// over auto-detection; the returned `Resolved<String>` carries the
/// origin so downstream renderers can branch on `is_explicit()`. The
/// axis-driven group override (auto-detected `year` suppressed under a
/// non-Date axis) lives in `effective_group_for_axis` below.
pub(crate) fn resolve_children_config(
    docs: &[&ParsedDocument],
    style_override: Option<&moss_core::Resolved<String>>,
    group_override: Option<&moss_core::Resolved<String>>,
    math: bool,
) -> (moss_core::Resolved<String>, moss_core::Resolved<String>) {
    let style = match style_override {
        Some(r) => r.clone(),
        None => {
            // Bulk, not exceptions: one claimed term must not turn 55 bare labels
            // into empty archive rows. Mostly-rich is an archive at any size, and
            // so is a rich page beside at most 3 bare ones; what summary cannot
            // survive is a screenful of them. Boundary pinned by
            // bulk_style_tests.
            let rich = docs.iter().filter(|d| d.cover.is_some()
                || crate::build::page::meta::resolve_page_description(
                    d.description.as_deref(), &d.content, math).is_some()).count();
            let has_rich = rich > 0 && (rich * 2 > docs.len() || docs.len() - rich <= 3);
            let any_has_date = docs.iter().any(|d| d.date.is_some());
            // Nothing at all is an INDEX of bare labels, not an archive, and "summary" lays
            // those out one per row.
            let auto_value = match (has_rich, any_has_date) {
                (false, false) => "grid".to_string(),
                (true, _) => "summary".to_string(),
                (false, true) => "list".to_string(),
            };
            moss_core::Resolved::auto(auto_value)
        }
    };
    let group = match group_override {
        Some(r) => r.clone(),
        None => {
            // Any dated child under a non-summary style auto-groups by year
            // (matching the schema default "year (default for list)" and the
            // contract). A single dated post gets a year heading too; a
            // dateless listing stays flat. Auto-detected `year` is still
            // suppressed under a non-Date sort axis in `effective_group_for_axis`.
            let auto_value = if style.value != "summary" && docs.iter().any(|d| d.date.is_some()) {
                "year".to_string()
            } else {
                "none".to_string()
            };
            moss_core::Resolved::auto(auto_value)
        }
    };
    (style, group)
}

/// Decide the effective `children_group` for rendering, honoring author
/// intent across non-Date sort axes. Explicit (frontmatter or cascade)
/// values always survive; auto-detected values are overridden to "none"
/// when the axis isn't Date (alphabetical/weight sorting doesn't pair
/// well with auto-year-grouping).
///
/// Consumes the `Resolved<String>` directly so callers don't need to
/// destructure-then-recheck (`value.clone()` + separate `is_explicit()`).
pub(crate) fn effective_group_for_axis(
    group_resolved: &moss_core::Resolved<String>,
    axis: moss_core::sort::SortAxis,
) -> String {
    let axis_suppresses_auto_year = !matches!(axis, moss_core::sort::SortAxis::Date);
    if axis_suppresses_auto_year && !group_resolved.is_explicit() {
        "none".to_string()
    } else {
        group_resolved.value.clone()
    }
}

/// Compute the latest article date within a folder.
///
/// Finds all documents whose url_path is under the folder's prefix and returns
/// the maximum date string (ISO comparison works for chronological ordering).
pub(crate) fn folder_latest_date<D: std::borrow::Borrow<ParsedDocument>>(
    folder_doc: &ParsedDocument,
    all_docs: &[D],
    root_path: &str,
) -> Option<String> {
    let folder_prefix = folder_doc.url_path.trim_end_matches("index.html");
    all_docs
        .iter()
        .map(|d| d.borrow())
        .filter(|d| {
            d.url_path != folder_doc.url_path
                // Direct children only: at most one path segment of remainder
                && d.url_path.strip_prefix(folder_prefix).is_some_and(|rem| {
                    !rem.trim_end_matches("/index.html")
                        .trim_end_matches('/')
                        .contains('/')
                })
        })
        .filter_map(|d| {
            // For sub-folders, recurse to get their latest child date
            let is_subfolder = d.kind == PageKind::Folder && {
                let prefix = d.url_path.trim_end_matches("index.html");
                all_docs.iter().map(|o| o.borrow()).any(|other| {
                    other.url_path != d.url_path && other.url_path.starts_with(prefix)
                })
            };
            if is_subfolder {
                folder_latest_date(d, all_docs, root_path)
            } else {
                // Leaf article: use extract_date_from_doc for filesystem fallback
                let (_display, raw, _explicit) =
                    crate::build::components::date::extract_date_from_doc(d, root_path);
                raw
            }
        })
        .max()
}

/// Generates a unified children listing for folder index pages and folder embeds.
///
/// Builds `ChildItemProps` for each document, separates folders from articles,
/// sorts each group by date descending, and renders using the specified style.
///
/// # Arguments
/// * `folder_docs` - Direct children documents to render
/// * `all_folder_docs` - All documents in the folder (for computing child counts)
/// * `project` - Project structure for date extraction
/// * `style` - "list", "summary", or "grid" (collection cards)
/// * `group` - "year" or "none"
/// * `lang` - Language for UI strings
/// * `skip_resort` - If true, skip the internal date-desc resort and render docs
///   in the order they arrive. Set true by callers for non-Date axes (Title /
///   Weight / explicit order) — the caller has already sorted via
///   `sort_by_resolved`, and the date resort would scramble that order.
/// * `typesetting` - Typesetting mode ("vertical" or "horizontal"); used for CJK numeral formatting
/// * `is_embed` - true only for a body `![[folder/|…]]` embed; stamps
///   `data-embed` on the `.moss-cards-container` built here so CSS can give
///   an embedded listing block rhythm distinct from the trailing automatic
///   listing.
#[allow(clippy::too_many_arguments)]
pub(crate) fn generate_children(
    folder_docs: &[&ParsedDocument],
    all_folder_docs: &[&ParsedDocument],
    project: &ProjectStructure,
    style: &str,
    group: &str,
    lang: Language,
    skip_resort: bool,
    dir_overrides: &std::collections::HashMap<String, String>,
    typesetting: Option<&str>,
    media_lookup: Option<&crate::build::media::dimensions::MediaDimensionLookup>,
    parent_sort_axis: Option<moss_core::sort::SortAxis>,
    math: bool,
    is_embed: bool,
    placement: &moss_core::media::Placement,
) -> String {
    if folder_docs.is_empty() {
        return String::new();
    }

    // Build ChildItemProps for each doc. The shared reading lives beside the
    // struct; the one thing the row shapes want on top is the auto-extracted
    // excerpt fallback for `description`, which the hand-picked card paths
    // deliberately do not take.
    let items: Vec<ChildItemProps> = folder_docs
        .iter()
        .map(|doc| {
            let mut props = components::child_list::props_for_document(
                doc,
                all_folder_docs,
                &project.root_path,
                dir_overrides,
                typesetting,
            );
            // The grid card is one of those paths: its description slot is
            // frontmatter-only, like the `:::grid` card it shares a renderer
            // with, so the excerpt stays out of it.
            if style != "grid" {
                props.description = crate::build::page::meta::resolve_page_description(
                    doc.description.as_deref(),
                    &doc.content,
                    math,
                );
            }
            props
        })
        .collect();

    // Lift `media_lookup_ref` out of the grid-only branch so the
    // summary-style child_summary::render_with_sort calls below can route
    // their cover images through the synthesizer too.
    //
    // Same fallback as the grid branch had: prefer the caller-provided
    // lookup, else construct one from project scan data. The owned
    // fallback lives at this scope's lifetime so all subsequent branches
    // can borrow it.
    let owned_lookup;
    let media_lookup_ref: &crate::build::media::dimensions::MediaDimensionLookup = match media_lookup {
        Some(l) => l,
        None => {
            // BUG 6: the lookup carries `dir_overrides` so `build_asset_snapshot`
            // (invoked downstream via `render_cover_html`) can index cover dims
            // under the OVERRIDE-slug output-URL key. Without it, a card/folder
            // cover under an overridden dir misses the probe → 800x600 fallback.
            owned_lookup = crate::build::media::dimensions::MediaDimensionLookup::new(
                &project.image_files,
                &project.video_files,
                dir_overrides,
                // Fallback only; the caller normally passes a registry-ful one.
                None,
            );
            &owned_lookup
        }
    };

    // Parent's resolved sort axis drives the card's meta slot
    // (see child_summary::render_with_sort). Date is the historical
    // fallback when the caller hasn't populated `direct_children_sort` —
    // matches the same fallback used in render/html.rs callsites and
    // preserves legacy test behavior.
    let parent_axis = parent_sort_axis.unwrap_or(moss_core::sort::SortAxis::Date);

    // Unless the caller already ordered them (a non-Date axis or an explicit
    // order), every style lists by date through the same comparator as the
    // folder's series chain. It re-sorts here because a card's date can come
    // from somewhere the chain doesn't look: a folder's newest article, a
    // date in the filename, or the file's creation time.
    let mut sorted: Vec<&ChildItemProps> = items.iter().collect();
    if !skip_resort {
        sorted.sort_by(|a, b| {
            // Undated children list first here on purpose: in practice they are
            // the folder's subfolders, which lead its page. The series chain
            // puts undated pages last, but it never contains subfolders, so the
            // two disagree only over an undated article inside a series.
            a.date_raw.is_some().cmp(&b.date_raw.is_some())
                .then_with(|| moss_core::sort::cmp_date_axis(&a.date_sort_key(), &b.date_sort_key()))
        });
    }

    // Grid style: render as collection cards.
    // Bypasses folder/article separation and year grouping — card grids are flat.
    if style == "grid" {
        use crate::build::components::grid_card::render_list_with_typesetting;

        return render_list_with_typesetting(
            &sorted,
            Some(std::path::Path::new(&project.root_path)),
            lang,
            typesetting,
            Some(media_lookup_ref),
            parent_axis,
            is_embed,
            placement,
        );
    }

    // Render
    let mut html = String::new();

    // Partition once, unconditionally: folders always render above articles.
    // That is a layout decision independent of which axis sorted the
    // children; the partition keeps the sorted order within each side.
    // Previously this filter was written out three times, once per branch,
    // and skipped entirely for a plain skip_resort (Weight/Title/explicit
    // order) listing — so a manually-ordered folder lost its folder/article
    // split, which is what a `children_style: summary` folder of writings,
    // ordered by `weight:` and holding a subfolder or two, hit.
    let (folders, articles): (Vec<&ChildItemProps>, Vec<&ChildItemProps>) =
        sorted.into_iter().partition(|i| i.child_count.is_some());

    // Folders always render flat, regardless of group setting.
    for folder in &folders {
        if style == "summary" {
            html.push_str(&components::child_summary::render_with_sort(folder, lang, typesetting, Some(media_lookup_ref), parent_axis));
        } else {
            html.push_str(&components::child_list::render_child(folder, lang, typesetting));
        }
        html.push('\n');
    }

    // Subtle divider between folders and articles (summary style only)
    if !folders.is_empty() && !articles.is_empty() && style == "summary" {
        html.push_str("<hr class=\"moss-child-section-divider\" />\n");
    }

    // Articles: year-grouped if group == "year", otherwise flat in
    // partition order (already caller order or date-desc, set above).
    if group == "year" && !articles.is_empty() {
        if style == "summary" {
            // Summary style: group articles by year, render summary cards
            // within year sections. `bucket_articles_by_year` is find-or-
            // append (stable) rather than assuming pre-sorted, contiguous
            // input — needed now that a non-Date axis (skip_resort) reaches
            // this branch too, not just the Date-sorted case that used to
            // be the only caller.
            let sections: Vec<String> = bucket_articles_by_year(&articles)
                .into_iter()
                .map(|(year, arts)| {
                    let heading = year
                        .map(|y| format!("<h2>{}</h2>\n", date_formatters::format_year_heading(y, lang, typesetting)))
                        .unwrap_or_default();
                    let items_html = arts
                        .iter()
                        .map(|a| components::child_summary::render_with_sort(a, lang, typesetting, Some(media_lookup_ref), parent_axis))
                        .collect::<Vec<_>>()
                        .join("\n");
                    format!(
                        "<section class=\"moss-cards-minimal-year-group moss-cards-minimal-year-group--summary\">\n{}{}\n</section>",
                        heading, items_html
                    )
                })
                .collect();
            html.push_str(&sections.join("\n"));
        } else if skip_resort {
            // Non-Date axis (Weight/Title/explicit order): items are NOT
            // date-sorted, so buckets must be stable (find-or-append,
            // first-appearance order), not `render_year_grouped_list`'s own
            // date resort — this is what the flattened home infers when its
            // section-leaf articles carry `weight:`, and the author opted
            // into year sections that must survive even though the caller
            // pre-sorted by weight, not date. Markup mirrors year_group.rs
            // (`moss-cards-minimal-year-group minimal`) so the minimal-item
            // CSS keyed off data-layout="minimal" applies; items use
            // child_list::render (the `title`-classed span), not render_child.
            let sections: Vec<String> = bucket_articles_by_year(&articles)
                .into_iter()
                .map(|(year, arts)| render_minimal_year_section(year, &arts, lang, typesetting))
                .collect();
            html.push_str(&sections.join("\n"));
        } else {
            // Date axis: `render_year_grouped_list` re-sorts (url-path
            // tiebreak on equal dates, as the series chain) and buckets in
            // one pass.
            let article_props: Vec<ArticleListItemProps> = articles
                .iter()
                .map(|item| ArticleListItemProps {
                    date_display: item.date_display.clone().unwrap_or_default(),
                    date_raw: item.date_raw.clone(),
                    url: item.url.clone(),
                    title: item.title.clone(),
                    url_path: item.url_path.clone(),
                })
                .collect();
            html.push_str(&components::render_year_grouped_list(&article_props, true, lang, typesetting));
        }
    } else {
        for article in &articles {
            if style == "summary" {
                html.push_str(&components::child_summary::render_with_sort(article, lang, typesetting, Some(media_lookup_ref), parent_axis));
            } else {
                html.push_str(&components::child_list::render_child(article, lang, typesetting));
            }
            html.push('\n');
        }
    }

    // The wrapper class is always plain `.moss-cards`. The earlier
    // `moss-summary-layout` co-class for `style == "summary"` is dropped:
    // the default CSS doesn't style it (summary styling rides on
    // `.moss-cards[data-layout="list"]` and `.moss-card-description`
    // visibility, both of which are still emitted), and its presence
    // breaks themes that inherited the pre-v1 vocabulary (one site theme's
    // `.moss/theme/style.css` hid anything tagged with the class, erasing the
    // entire listing).
    //
    // `list` is the compact date+title index: one-line `[date] [title]` rows
    // (year-grouped when dated) styled by the item CSS keyed off
    // `.moss-cards[data-layout="minimal"]` (site.css:1580-1591) — title
    // color/hover/underline. So `list` (and any legacy non-summary value like
    // the retired "minimal") ALWAYS emits data-layout="minimal", whether or not
    // the listing is year-grouped. Summary keeps data-layout="list"; its cards
    // render through `child_summary` and don't depend on the minimal item CSS.
    // (Grid returns earlier with its own data-layout="grid".)
    let data_layout = if style != "summary" { "minimal" } else { "list" };
    components::cards_container(data_layout, is_embed, placement, html.trim())
}

/// Bucket articles by year using find-or-append: one section per year, in
/// FIRST-APPEARANCE order, no resort. Needed wherever the input is not
/// (and must not be) date-sorted — a non-Date axis (Weight/Title/explicit
/// order) with `children_group: year` still owes its author year sections,
/// but re-sorting by date (as `components::render_year_grouped_list` does)
/// would silently discard the very order they asked for.
///
/// The single undated bucket, if any, is moved to the end regardless of
/// when it first appeared — matching `year_group.rs`'s "None-dated items
/// sort last" rule, so the two bucketing implementations agree on where an
/// undated run lands even though one resorts and the other doesn't.
///
/// Also correct for already date-sorted input (the summary-style caller):
/// sorted-descending input naturally yields contiguous year runs, so
/// find-or-append never needs to reopen a bucket it already closed.
fn bucket_articles_by_year<'a>(
    articles: &[&'a ChildItemProps],
) -> Vec<(Option<i32>, Vec<&'a ChildItemProps>)> {
    let mut buckets: Vec<(Option<i32>, Vec<&ChildItemProps>)> = Vec::new();
    for article in articles {
        let year = article.date_raw.as_ref().and_then(|d| date_formatters::extract_year(d));
        match buckets.iter_mut().find(|(y, _)| *y == year) {
            Some((_, bucket)) => bucket.push(*article),
            None => buckets.push((year, vec![*article])),
        }
    }
    if let Some(idx) = buckets.iter().position(|(y, _)| y.is_none()) {
        let undated = buckets.remove(idx);
        buckets.push(undated);
    }
    buckets
}

/// Render one `list`/`minimal`-style year section: the compact date+title
/// rows, matching `year_group.rs`'s markup (`moss-cards-minimal-year-group
/// minimal`) so the minimal-item CSS keyed off `data-layout="minimal"`
/// applies. Items use `child_list::render` (the `title`-classed span), not
/// `render_child`.
fn render_minimal_year_section(
    year: Option<i32>,
    arts: &[&ChildItemProps],
    lang: Language,
    typesetting: Option<&str>,
) -> String {
    let items_html = arts
        .iter()
        .map(|a| {
            let props = ArticleListItemProps {
                date_display: a.date_display.clone().unwrap_or_default(),
                date_raw: a.date_raw.clone(),
                url: a.url.clone(),
                title: a.title.clone(),
                url_path: a.url_path.clone(),
            };
            components::child_list::render(&props, true, false, lang, typesetting)
        })
        .collect::<Vec<_>>()
        .join("\n");
    match year {
        Some(y) => format!(
            "<section class=\"moss-cards-minimal-year-group minimal\">\n<h2>{}</h2>\n{}\n</section>",
            date_formatters::format_year_heading(y, lang, typesetting), items_html
        ),
        // Undated bucket: headingless trailing section (mirrors year_group.rs).
        None => format!(
            "<section class=\"moss-cards-minimal-year-group minimal\">\n{}\n</section>",
            items_html
        ),
    }
}

/// Expand every folder-list marker in the whole build, in place.
///
/// The between-phases home of [`resolve_markers`]: it runs after
/// `populate_direct_children_sorts` (so a target folder's resolved sort axis is
/// available to inherit) and before the render phase (which would otherwise
/// emit the marker comment into the page verbatim).
///
/// Iterating by index is what makes the borrows work: expanding one document's
/// markers reads `&documents` to find the target folder and its children, so
/// the body being rewritten is taken OUT of the vec for the duration and put
/// back after. Both shapes of that body move together — `html_content` is
/// `body_plan` flattened, and the render phase reads the plan, so
/// leaving either one behind would silently drop the expansion.
///
/// `media_lookup` is the caller's already-built `MediaDimensionLookup` — the
/// same one `generate_blocking_content` builds once from
/// `project.image_files`/`project.video_files`/`dir_overrides` for the
/// markdown pass. Building a second copy here used to cost a full pass over
/// every image and video's dimensions/color/LQIP on EVERY build this
/// function ran at all, regardless of whether any listing actually changed —
/// corpus-scaled work identical in shape to `page_map`/`external_url_map`'s.
pub fn expand_markers_in_documents(
    documents: &mut [ParsedDocument],
    project: &ProjectStructure,
    dir_overrides: &std::collections::HashMap<String, String>,
    math: bool,
    media_lookup: &crate::build::media::dimensions::MediaDimensionLookup,
    site_typesetting: Option<&str>,
) {
    expand_markers_in_documents_with_place_maps(documents, project, dir_overrides, math, media_lookup, site_typesetting, None)
}

#[allow(clippy::too_many_arguments)]
pub fn expand_markers_in_documents_with_place_maps(
    documents: &mut [ParsedDocument],
    project: &ProjectStructure,
    dir_overrides: &std::collections::HashMap<String, String>,
    math: bool,
    media_lookup: &crate::build::media::dimensions::MediaDimensionLookup,
    site_typesetting: Option<&str>,
    place_maps: Option<&crate::build::place_map::PlaceMapRenderContext>,
) {
    let has_marker = |d: &ParsedDocument| d.html_content.contains(MARKER_FOLDER_LIST);
    if !documents.iter().any(has_marker) {
        return;
    }
    for i in 0..documents.len() {
        if !has_marker(&documents[i]) {
            continue;
        }
        let from = documents[i]
            .source_path
            .clone()
            .unwrap_or_else(|| documents[i].url_path.clone());
        let lang = documents[i].lang; // hosting page's language, not site default
        let typesetting = crate::build::render::config::effective_typesetting(
            documents[i].typesetting.as_deref(),
            site_typesetting,
        )
        .map(str::to_owned);
        let mut plan = documents[i].body_plan.take();
        // `tag_embed: true` — every marker this scan finds came from a
        // literal body `![[folder/|…]]`, never from `synthesize_children_marker`
        // (that path calls `resolve_markers` directly on a lone marker, before
        // this pass ever runs; see `resolve_markers_impl`'s doc comment).
        let expand = |html: &str| {
            resolve_markers_impl(
                html,
                &from,
                documents,
                project,
                dir_overrides,
                lang,
                typesetting.as_deref(),
                Some(media_lookup),
                math,
                true,
                place_maps,
            )
        };
        let resolved = match &mut plan {
            Some(p) => {
                p.map_html(&expand);
                p.to_html()
            }
            None => expand(&documents[i].html_content),
        };
        documents[i].body_plan = plan;
        documents[i].html_content = resolved;
    }
}

/// Scan `html` for folder-list markers and replace each with rendered HTML.
///
/// Inputs:
/// - `html`: rendered page HTML containing zero or more markers
/// - `from_md_path`: source markdown path of the page (used as the default
///   `from` for relative resolution — the marker carries its own `from=` too,
///   from the original markdown source)
/// - `all_docs` / `project` / `dir_overrides` / `site_lang`: pipeline state
///   forwarded to `generate_children`
pub fn resolve_markers(
    html: &str,
    from_md_path: &str,
    all_docs: &[ParsedDocument],
    project: &ProjectStructure,
    dir_overrides: &std::collections::HashMap<String, String>,
    site_lang: crate::i18n::Language,
    typesetting: Option<&str>,
    media_lookup: Option<&crate::build::media::dimensions::MediaDimensionLookup>,
    math: bool,
) -> String {
    resolve_markers_with_place_maps(
        html, from_md_path, all_docs, project, dir_overrides, site_lang,
        typesetting, media_lookup, math, None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_markers_with_place_maps(
    html: &str,
    from_md_path: &str,
    all_docs: &[ParsedDocument],
    project: &ProjectStructure,
    dir_overrides: &std::collections::HashMap<String, String>,
    site_lang: crate::i18n::Language,
    typesetting: Option<&str>,
    media_lookup: Option<&crate::build::media::dimensions::MediaDimensionLookup>,
    math: bool,
    place_maps: Option<&crate::build::place_map::PlaceMapRenderContext>,
) -> String {
    resolve_markers_impl(
        html,
        from_md_path,
        all_docs,
        project,
        dir_overrides,
        site_lang,
        typesetting,
        media_lookup,
        math,
        false,
        place_maps,
    )
}

/// Shared implementation behind [`resolve_markers`]. `is_embed` threads to
/// [`render_one`]/[`generate_children`], which stamp `data-embed` at the
/// point the container is first built (never by re-scanning emitted HTML —
/// ratchet row `(p)`), distinguishing a body `![[folder/|…]]` embed
/// from the frontmatter listing `synthesize_children_marker` produces, since
/// both share this same marker format and `render_one`. Only
/// [`expand_markers_in_documents`] passes `true`; `resolve_markers` always
/// passes `false`, so its own callers are unaffected.
#[allow(clippy::too_many_arguments)]
fn resolve_markers_impl(
    html: &str,
    from_md_path: &str,
    all_docs: &[ParsedDocument],
    project: &ProjectStructure,
    dir_overrides: &std::collections::HashMap<String, String>,
    site_lang: crate::i18n::Language,
    typesetting: Option<&str>,
    media_lookup: Option<&crate::build::media::dimensions::MediaDimensionLookup>,
    math: bool,
    is_embed: bool,
    place_maps: Option<&crate::build::place_map::PlaceMapRenderContext>,
) -> String {
    if !html.contains(MARKER_FOLDER_LIST) {
        return html.to_string();
    }

    let mut out = String::with_capacity(html.len());
    let mut remaining = html;

    loop {
        let Some((before, after_prefix)) = remaining.split_once(MARKER_FOLDER_LIST) else {
            out.push_str(remaining);
            break;
        };
        out.push_str(before);
        let Some((body, rest)) = after_prefix.split_once(MARKER_END) else {
            // Unterminated marker — copy the rest verbatim and bail.
            out.push_str(MARKER_FOLDER_LIST);
            out.push_str(after_prefix);
            break;
        };

        let rendered = match parse_marker_body(body) {
            Some(parsed) => render_one(
                &parsed,
                from_md_path,
                all_docs,
                project,
                dir_overrides,
                site_lang,
                typesetting,
                media_lookup,
                math,
                is_embed,
                place_maps,
            ),
            None => format!(
                r#"<div class="moss-embed-missing">Invalid folder-embed marker: {}</div>"#,
                html_escape(body),
            ),
        };
        out.push_str(&rendered);
        remaining = rest;
    }

    out
}

/// If the resolved folder has a source `index.html` (or `.htm`) in
/// `project.html_files`, render it as an iframe (the same emission used by
/// `![[file.html]]` wikilinks). Returns `None` when neither is present
/// — the caller falls back to the missing-folder diagnostic.
///
/// `folder_id` is the URL-space, forward-slash-separated, no-trailing-slash
/// project-relative path (as produced by `resolve_folder_id`). It matches
/// `FileInfo.path`, which is also forward-slash + project-relative.
///
/// Dispatching into `synthesize_iframe_html` (the canonical Stage 2 iframe
/// synthesizer) rather than inlining the emission means any future change
/// to the iframe shape (sandbox attribute, default sizing) flows here
/// automatically.
///
/// **Pretty-URL adjustment.** `synthesize_iframe_html` consumes a URL string
/// computed by `relative_asset_path(from_path, target_path)` — which returns
/// a path relative to `from_path`'s filesystem parent. The marker pass runs
/// AFTER `adjust_relative_paths_for_pretty_urls` (the pass that adds `../`
/// for pretty-URL-wrapped non-index pages), so newly spliced iframes need
/// the same adjustment applied locally. The file form (`![[file.html]]`)
/// gets the adjustment automatically because wikilink resolution happens
/// BEFORE the pretty-URL pass; folder embeds, resolved via this marker,
/// need it here. Translation-home pages are not handled — they're a niche
/// enough case that we accept the false positive (extra `../`) for v1.
fn try_render_folder_index_iframe(
    folder_id: &str,
    from_path: &str,
    project: &ProjectStructure,
    dir_overrides: &std::collections::HashMap<String, String>,
    size: Option<&str>,
    all_docs: &[ParsedDocument],
) -> Option<String> {
    // Locate which index file exists. The lookup uses the case-preserving
    // `folder_id` because `project.html_files` carries raw on-disk paths.
    let index_name = ["index.html", "index.htm"].iter().find(|name| {
        let path = if folder_id.is_empty() {
            (**name).to_string()
        } else {
            format!("{}/{}", folder_id, name)
        };
        project.html_files.iter().any(|f| f.path == path)
    })?;

    // Compute the iframe src in OUTPUT space. moss writes copied files —
    // including passthrough subtrees (copy_deferred_assets) — to the path
    // produced by `resolve_path_with_overrides`, which slugifies intermediate
    // directory segments (e.g. "Resources/" → "resources/", "my app/" →
    // "my-app/") while preserving the leaf filename. The src must be the
    // relative path between the OUTPUT paths of the embedding page and the
    // target index — emitting the raw case-preserving `folder_id` leaks
    // source-case directories into the URL and 404s on case-sensitive servers
    // (it only resolves on case-insensitive macOS APFS, which is why preview
    // passes but deploy breaks). Mapping `from_path` through the same function
    // keeps the relative computation in output space, so a shared mixed-case
    // prefix still cancels (e.g. embedding /Resources/app/ from Resources/).
    let target_src = if folder_id.is_empty() {
        (*index_name).to_string()
    } else {
        format!("{}/{}", folder_id, index_name)
    };
    // Folder-as-iframe runs in the desktop app's build pipeline outside the
    // markdown emission path (no pulldown-cmark, no Stage-2 dispatcher).
    // Call the canonical Stage 2 synthesizer directly. Folder embeds carry
    // a `|size` token (e.g. `![[/app/|80%]]`) which we translate into the
    // iframe's `width=`/`height=` attributes — mirroring the wikilink
    // dispatcher's `build_synth_params` Iframe arm. Any other per-link
    // grammar (?query/#fragment/title) is still unsupported here.
    // `AssetSnapshot` is unused by the iframe synthesizer (iframes target
    // HTML, which moss does not transform), so an empty snapshot suffices.
    let url = moss_core::resolve::output_url::reference_output_url(from_path, &target_src, dir_overrides);
    let mut params = TitleParams::default();
    // `Sizing::Width(d)` → width only; `Sizing::Box(w, h)` → both. An
    // unparseable token falls through to a default (unsized) iframe.
    if let Some(sizing) = size.and_then(Sizing::parse) {
        match sizing {
            Sizing::Width(w) => {
                params.insert("width", w.to_css());
            }
            Sizing::Box(w, h) => {
                params.insert("width", w.to_css());
                params.insert("height", h.to_css());
            }
        }
    }
    let assets = AssetSnapshot::new();
    let html = moss_core::render::iframe::synthesize_iframe_html(
        &params,
        &moss_core::media::Placement::default(),
        &url,
        &assets,
    );

    // Apply the pretty-URL `../` adjustment if the embedding page is
    // pretty-URL-wrapped (non-index). See doc-comment above.
    let adjusted = if is_index_source(from_path, all_docs) {
        html
    } else {
        crate::build::markdown::html_post::adjust_relative_paths_for_pretty_urls(&html)
    };
    Some(adjusted)
}

/// Whether the markdown at `from_path` becomes its own folder's index page
/// (no pretty-URL wrap). True for `index.md`, `README.md`, same-name
/// children (`recipes/recipes.md`), language-suffixed home stems, the root
/// self-named home, and `home: true` / translation-inherited homes.
///
/// Ground truth is the home election: a resolved home's `kind` is
/// `PageKind::Folder`, so a matching `source_path` settles it — covering the
/// `is_home_override`/inherited cases and the root (`parent_name == ""`) case the
/// filename heuristic alone misses. The filename check remains as a fallback for
/// synthesized `from` paths that have no backing doc (e.g. `"<folder>/index.md"`,
/// which the `children:` render path passes as `from` for a folder index).
fn is_index_source(from_path: &str, all_docs: &[ParsedDocument]) -> bool {
    if all_docs
        .iter()
        .any(|d| d.source_path.as_deref() == Some(from_path) && d.kind == PageKind::Folder)
    {
        return true;
    }
    let path = std::path::Path::new(from_path);
    let stem_lower = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let parent_name = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();
    moss_core::home::is_home_file(&stem_lower, &parent_name)
}

/// Select the child documents a listing resolves to — the SINGLE source of
/// truth for "which docs does this listing contain", keyed on an
/// ALREADY-SLUGIFIED folder id (`""` for the site root).
///
/// Three consumers: `render_one` (the marker path), the synthetic
/// folder-index loop in `render/blocking.rs`, and the listing-group digest in
/// `render/incremental/listing.rs`. The blocking loop used to
/// carry an inlined second copy — so "one selector, therefore no drift" was
/// false, and the rule that the digest honours every rule the renderer
/// honours, structurally, could not hold.
pub(crate) fn select_children_by_slug<'a>(
    folder_id_slug: &str,
    is_flatten: bool,
    scope_default_tree: bool,
    exclude_nav: bool,
    all_docs: &'a [ParsedDocument],
    project: &ProjectStructure,
) -> Vec<&'a ParsedDocument> {
    let target_url = if folder_id_slug.is_empty() {
        "index.html".to_string()
    } else {
        format!("{}/index.html", folder_id_slug)
    };

    // For root folder (folder_id_slug = ""), prefix is empty so that all
    // non-homepage docs are considered candidates; starts_with("") is always true.
    // For non-root, prefix includes the trailing slash to avoid partial matches
    // (e.g. "blog/" doesn't match "blog-extra/index.html").
    let folder_prefix = if folder_id_slug.is_empty() {
        String::new()
    } else {
        format!("{}/", folder_id_slug)
    };
    all_docs
        .iter()
        .filter(|d| {
            if d.url_path == target_url {
                return false;
            }
            // Folder embeds publish a list of articles — `is_listable`
            // covers draft/slot_only in one shot
            // (PR7b routed slot files through this path).
            if !d.is_listable() {
                return false;
            }
            // scope_default_tree: the root homepage lists only the default language
            // tree. On a multilingual site (gated by `has_language_trees`), drop docs
            // under any language-prefix folder (`en/`, …) so the default-language home
            // never lists other-language articles. Single-language sites: gate is off,
            // so this is a no-op.
            // Set by synthesize_children_marker for homepage default-mode only.
            if scope_default_tree
                && project.has_language_trees
                && moss_core::home::lang_tree_prefix(&d.url_path).is_some()
            {
                return false;
            }
            // exclude_nav: skip folder pages that act as top-level nav items.
            // Only set by synthesize_children_marker for homepage default-mode.
            if exclude_nav
                && crate::build::components::nav::is_listing_nav_item(d, project.has_content_folders)
            {
                return false;
            }
            // also_in: include docs that declare membership in this folder
            let in_folder = d.url_path.starts_with(&folder_prefix)
                || d.also_in.as_ref().map_or(false, |folders| {
                    folders.contains(&folder_id_slug.to_string())
                });
            if !in_folder {
                return false;
            }
            if is_flatten {
                d.kind.is_listable_at_depth_all()
            } else {
                if !d.kind.is_listable_at_depth_direct() {
                    return false;
                }
                // An `also_in` doc need not live under this folder at all — its
                // declaration IS the membership claim, nothing to depth-check.
                d.url_path
                    .strip_prefix(folder_prefix.as_str())
                    .is_none_or(|rem| {
                        !rem.trim_end_matches("/index.html")
                            .trim_end_matches('/')
                            .contains('/')
                    })
            }
        })
        .collect()
}

/// Render a single resolved marker. Falls back to a `moss-embed-missing` div
/// on lookup failure. `is_embed` — see [`resolve_markers_impl`] — passes
/// straight through to [`generate_children`], which is where it actually
/// shapes output.
#[allow(clippy::too_many_arguments)]
fn render_one(
    parsed: &ParsedMarker<'_>,
    from_md_path: &str,
    all_docs: &[ParsedDocument],
    project: &ProjectStructure,
    dir_overrides: &std::collections::HashMap<String, String>,
    site_lang: crate::i18n::Language,
    typesetting: Option<&str>,
    media_lookup: Option<&crate::build::media::dimensions::MediaDimensionLookup>,
    math: bool,
    is_embed: bool,
    place_maps: Option<&crate::build::place_map::PlaceMapRenderContext>,
) -> String {
    // Prefer the marker's `from=` (the original markdown source); the
    // page-level `from_md_path` is a safe fallback when older markers omit it.
    let from = if parsed.from.is_empty() {
        from_md_path
    } else {
        parsed.from
    };
    let folder_id = resolve_folder_id(parsed.path, from);
    // `folder_id` is case-preserving (e.g. "Resources/cities-heat-map-app"),
    // while `ParsedDocument.url_path` is slugified — lowercased and
    // punctuation-stripped via `moss_core::content_graph::generate_slug`.
    // Compute the slugified form for URL-space lookups (target_doc match,
    // folder_prefix filter, "more" href). Keep `folder_id` for source-side
    // lookups (e.g. `project.html_files` carries raw on-disk paths).
    let folder_id_slug = if folder_id.is_empty() {
        String::new()
    } else {
        moss_core::content_graph::generate_slug(&folder_id)
    };
    // Route the three-branch decision through moss-core's `classify_reference`:
    // `dir_has_markdown_index` → FolderListing (Branch 1), else
    // `dir_has_static_index` → FolderIndexIframe (Branch 2), else NotFound.
    // The asset/url stubs are never consulted.
    let folder_idx = crate::build::folder_index::BuildFolderIndex {
        docs: all_docs,
        html_files: &project.html_files,
    };
    let no_assets = crate::build::folder_index::NoAssetIndex;
    let no_urls = crate::build::folder_index::NoUrlIndex;
    let ctx = moss_core::resolve::reference::ReferenceContext {
        assets: &no_assets,
        folders: &folder_idx,
        urls: &no_urls,
    };
    let classified = moss_core::resolve::reference::classify_reference(parsed.path, from, true, &ctx);
    use moss_core::resolve::reference::ReferenceKind;

    let target_doc: Option<&ParsedDocument> = match classified.kind {
        ReferenceKind::FolderListing => {
            // Branch 1: re-find the doc `dir_has_markdown_index` found, the
            // same way it found it (root by identity, else by `url_path` —
            // see that method's comment).
            let target_slug = moss_core::content_graph::generate_slug(
                classified.target_path.as_deref().unwrap_or(&folder_id_slug),
            );
            let found = if target_slug.is_empty() {
                all_docs.iter().find(|d| d.kind == PageKind::Folder && crate::build::folder_index::is_root_source(d))
            } else {
                let target_url = format!("{}/index.html", target_slug);
                all_docs.iter().find(|d| d.url_path == target_url)
            };
            match found {
                Some(d) => Some(d),
                // Unreachable: dir_has_markdown_index ⇒ this find succeeds. Keep
                // the missing-div fallback so a future predicate drift is safe.
                None => {
                    return format!(
                        r#"<div class="moss-embed-missing">Folder not found: {}</div>"#,
                        html_escape(parsed.path),
                    );
                }
            }
        }
        ReferenceKind::FolderIndexIframe => {
            // Branch 2: no content-folder doc, but a source index.html exists.
            // Pass the raw `folder_id` (for the on-disk html_files lookup) plus
            // `dir_overrides`, so the iframe src is computed in output space and
            // matches the slugified destination directory.
            if let Some(iframe) = try_render_folder_index_iframe(
                &folder_id,
                from,
                project,
                dir_overrides,
                parsed.size.as_deref(),
                all_docs,
            ) {
                return iframe;
            }
            // Defensive: predicate said iframe but the synthesizer declined.
            return format!(
                r#"<div class="moss-embed-missing">Folder not found: {}</div>"#,
                html_escape(parsed.path),
            );
        }
        _ => {
            // Branch 3: no folder doc and no static index. One more shape is
            // still listable: a pseudo-folder that exists only through
            // `also_in` memberships — a CLAIMED term (`build::terms`), whose
            // page is a real doc elsewhere so no `<key>/index.html` doc
            // exists. The memberships are real; list them with default sort.
            let has_members = all_docs.iter().any(|d| {
                d.also_in
                    .as_ref()
                    .is_some_and(|f| f.iter().any(|k| *k == folder_id_slug))
            });
            if !has_members {
                return format!(
                    r#"<div class="moss-embed-missing">Folder not found: {}</div>"#,
                    html_escape(parsed.path),
                );
            }
            None
        }
    };

    // Collect children respecting the depth param via the shared selector.
    // "all" flattens all descendants; "direct" (default) is one level only.
    let is_flatten = parsed.depth.as_deref() == Some("all");
    let folder_docs = select_children_by_slug(
        &folder_id_slug,
        is_flatten,
        parsed.scope_default_tree,
        parsed.exclude_nav,
        all_docs,
        project,
    );

    // A page that hosts a listing of a folder it also sits inside (a
    // `children:`/`children_more` whole-site listing at the root, where
    // every page's prefix is "") must not list itself. The folder's own
    // index already excludes itself because its url IS `target_url` inside
    // `select_children_by_slug`; a page that merely POINTS at a folder
    // (including the root) has no such coincidence, so exclude it here by
    // matching `from` against `source_path` — the one thing that identifies
    // "this listing's host page" regardless of where it lives.
    let folder_docs: Vec<&ParsedDocument> = match all_docs
        .iter()
        .find(|d| d.source_path.as_deref() == Some(from))
    {
        Some(host) => folder_docs
            .into_iter()
            .filter(|d| d.url_path != host.url_path)
            .collect(),
        None => folder_docs,
    };

    // `covers:only` keeps pages that have a cover. Applied right after
    // flattening/self-exclusion and before sort+limit, so `limit` counts off
    // the covered set rather than the full set. A folder entry is covered
    // when its own index page (the same `ParsedDocument`, `cover` is that
    // page's own field) has a cover — no separate lookup needed.
    let folder_docs: Vec<&ParsedDocument> = if parsed.covers == Some("only") {
        folder_docs.into_iter().filter(|d| d.cover.is_some()).collect()
    } else {
        folder_docs
    };

    if folder_docs.is_empty() {
        return String::new();
    }

    if parsed.style.as_deref() == Some("map") {
        if let Some(map) = place_maps.filter(|map| map.is_place_key(&folder_id_slug)) {
            if let Some(svg) = map.render_term_map(&folder_id_slug, folder_docs.iter().copied(), from, 0) {
                return place_map_with_placement(svg, &parsed.placement, parsed.caption.as_deref());
            }
            warn_map_fallback_once(parsed.path, "has no coordinate-bearing places");
        } else {
            warn_map_fallback_once(parsed.path, "is not a place term");
        }
    }

    // Resolve sort: flatten uses resolve_for_flatten; direct uses resolve_for_direct_children.
    // Per-embed sort= override wins over both. A pseudo-folder has no target
    // doc to carry sort intent, so it gets the same default a folder with no
    // sort cache gets (date-descending).
    let mut resolved = match target_doc {
        Some(t) if is_flatten => t.resolve_for_flatten(&folder_docs),
        Some(t) => t.resolve_for_direct_children(),
        None => moss_core::sort::ResolvedSort {
            axis: moss_core::sort::SortAxis::Date,
            explicit_order: None,
            series_default: false,
        },
    };
    if let Some(axis) = parsed.sort {
        // Override: drop explicit_order (sort:axis is a user-typed scalar,
        // not a list) and recompute series_default to match.
        resolved = ResolvedSort {
            axis,
            explicit_order: None,
            series_default: matches!(axis, SortAxis::Weight),
        };
    }

    // Auto-detect style and group when not specified in embed params.
    let style_override: Option<moss_core::Resolved<String>> = parsed
        .style
        .as_ref()
        .filter(|style| style.as_str() != "map")
        .map(|s| moss_core::Resolved::frontmatter(s.clone()));
    let group_override: Option<moss_core::Resolved<String>> = parsed
        .group
        .as_ref()
        .map(|g| moss_core::Resolved::frontmatter(g.clone()));
    let (style_resolved, group_resolved) = resolve_children_config(
        &folder_docs,
        style_override.as_ref(),
        group_override.as_ref(),
        math,
    );
    let children_style = style_resolved.value.clone();
    // An explicit `order:`/`sort:` list is a manual sequence the author chose;
    // it must survive even when the inferred axis is Date (which would otherwise
    // re-sort the children date-descending below and discard the order). The
    // upstream `sort_by_resolved` already placed listed children in the explicit
    // order, so render that order verbatim — no date re-sort, no year grouping.
    let skip_resort = !matches!(resolved.axis, moss_core::sort::SortAxis::Date)
        || resolved.explicit_order.is_some();
    let effective_group = effective_group_for_axis(&group_resolved, resolved.axis);

    let sorted = moss_core::sort::sort_by_resolved(&folder_docs, &resolved);

    let (limited, truncated): (Vec<&ParsedDocument>, bool) = match parsed.limit {
        Some(n) if n > 0 && n < sorted.len() => (sorted.iter().take(n).copied().collect(), true),
        _ => (sorted.clone(), false),
    };

    let all_docs_refs: Vec<&ParsedDocument> = all_docs.iter().collect();

    // With a caption, the whole placement — width, float AND size — moves
    // out to the figure wrapping the listing (the width escape is a
    // direct-child selector, and the figure has no width of its own to size
    // itself by otherwise); without one the container wears it itself.
    let container_placement = match parsed.caption {
        Some(_) => moss_core::media::Placement::default(),
        None => parsed.placement.clone(),
    };

    // Dispatch rendering based on style.
    let render_group = |docs: &[&ParsedDocument]| {
        generate_children(
            docs,
            &all_docs_refs,
            project,
            &children_style,
            &effective_group,
            site_lang,
            skip_resort,
            dir_overrides,
            typesetting,
            media_lookup,
            Some(resolved.presentation_axis()),
            math,
            is_embed,
            &container_placement,
        )
    };
    // A page that won a term claim splits its listing by the field each
    // member was named through — one group under "Author", another under
    // "Editor". The split is read off the claiming page, where
    // `build::terms::derive_terms` resolved it; this file has no `TermIndex`
    // and re-deriving it here is exactly the drift that would let the
    // claimed and generated pages of one term disagree. `folder_id_slug` is
    // the term key on this path, so the claimant is the one document whose
    // `term_listing` names it. Every other listing — and a term whose
    // members all came through one field — gets `None` and renders as one
    // unlabelled listing, byte for byte as before.
    let claiming_doc =
        all_docs.iter().find(|d| d.term_listing.as_deref() == Some(folder_id_slug.as_str()));
    let term_sections = claiming_doc.and_then(|d| d.term_sections.as_deref());
    let listing =
        crate::build::terms::render_term_sections(term_sections, &limited, site_lang, render_group)
            .unwrap_or_else(|| render_group(&limited));
    // The claimed half of the same breadcrumb/children chrome the generated
    // page gets in `render/blocking.rs`, read off the claiming document
    // beside `term_sections` above — `None` (rendered empty) for every
    // non-place term, since `derive_terms` only ever sets these two fields
    // from a place-typed kind's resolved data.
    let place_breadcrumb_html = claiming_doc
        .and_then(|d| d.place_breadcrumb.as_deref())
        .and_then(crate::build::components::place_hierarchy::render_breadcrumb)
        .unwrap_or_default();
    let place_children_html = claiming_doc
        .and_then(|d| d.place_children.as_deref())
        .and_then(crate::build::components::place_hierarchy::render_children)
        .unwrap_or_default();
    let listing = format!("{}{}{}", place_breadcrumb_html, listing, place_children_html);

    // Suppress the More link when the embed is on the folder's own index page
    // (self-referential listing). A "More →" link pointing to the page the
    // user is already reading is meaningless. Uses `from` (the resolved path
    // with fallback) and `is_index_source` so README.md and self-named home
    // files are handled correctly alongside index.md.
    let from_parent = std::path::Path::new(from)
        .parent()
        .and_then(|p| p.to_str())
        .unwrap_or("");
    let is_self_listing = is_index_source(from, all_docs) && from_parent == folder_id_slug;

    // An explicit target — `children_more:` frontmatter or the embed's own
    // `more:<target>` param, unified into `parsed.more` — always wins, and
    // fires even on a self-listing: its whole point is to send the reader
    // somewhere OTHER than the page they're on, which is exactly the case
    // the default More link suppresses. Its link text is the target page's
    // own title ("Archive →") rather than the localised "More →" below,
    // since the target is a named page, not "the rest of this folder".
    // An unresolvable target is a broken reference: `children_more`'s
    // bracket form is also caught by the frontmatter-wikilink pass (same
    // path `cover`/`sidebar`/`children_source` go through), but the embed's
    // `more:` has no such pass, so the warning below is this field's only
    // one — omitted either way rather than falling back to the folder's own URL.
    //
    // Absent that, fall back to the folder's own page — suppressed on a
    // self-listing (meaningless: a "More →" back to the page you're already
    // on) and on a pseudo-folder (`target_doc.is_some()`; a claimed term has
    // no page at `/<key>/` for More to land on — a dead link would 404).
    let path_resolver = crate::build::assets::paths::PathResolver::new()
        .with_dir_overrides(dir_overrides.clone());
    let more_link: Option<(String, String)> = if !truncated {
        None
    } else if let Some(more_ref) = parsed.more.as_deref() {
        match resolve_more_link_target(more_ref, all_docs) {
            Some(doc) => {
                let stem = doc.url_path.trim_end_matches("index.html").trim_end_matches('/');
                let href = if stem.is_empty() {
                    "/".to_string()
                } else {
                    path_resolver.resolve_url(&format!("{}/", stem))
                };
                Some((href, format!("{} →", html_escape(&doc.title))))
            }
            None => {
                eprintln!(
                    "Warning: the More link target \"{}\" does not resolve to a page. Omitting the More link.",
                    more_ref
                );
                None
            }
        }
    } else if !is_self_listing && target_doc.is_some() {
        Some((
            if folder_id_slug.is_empty() {
                "/".to_string()
            } else {
                path_resolver.resolve_url(&format!("{}/", folder_id_slug))
            },
            crate::i18n::t(site_lang, "more_link").to_string(),
        ))
    } else {
        None
    };

    let listing = match more_link {
        Some((href, text)) => format!(
            "{}\n<p class=\"moss-embed-more\"><a href=\"{}\">{}</a></p>",
            listing,
            html_escape(&href),
            text,
        ),
        None => listing,
    };
    match parsed.caption.as_deref() {
        Some(caption) => moss_core::render::placement::wrap_embed_with_caption(
            &listing,
            &parsed.placement,
            caption,
        ),
        None => listing,
    }
}

/// Resolve a `children_more:` reference to the page it names. Mirrors
/// `children_source`'s stem matching (`frontmatter_ref_to_stem` + slug
/// comparison against `ParsedDocument::url_path`), generalized from folders to
/// any listable page — `children_more` sends the reader to an ordinary page,
/// not a folder. `more_ref` is already a resolved path by the time it reaches
/// here in a real build (the frontmatter-wikilink pass runs first); the raw
/// `[[bracket]]` form is handled too, for callers that construct a marker
/// directly.
fn resolve_more_link_target<'a>(
    more_ref: &str,
    all_docs: &'a [ParsedDocument],
) -> Option<&'a ParsedDocument> {
    let stem = crate::build::markdown::frontmatter_ref_to_stem(more_ref);
    let target_slug = moss_core::content_graph::generate_slug(&stem);
    all_docs.iter().find(|d| {
        let d_stem = d
            .url_path
            .trim_end_matches("/index.html")
            .trim_end_matches("index.html")
            .trim_end_matches('/');
        let d_leaf = d_stem.rsplit('/').next().unwrap_or(d_stem);
        d_leaf == target_slug
    })
}

/// Synthesize a folder-list marker from a page's `children_*` frontmatter fields.
///
/// Called from `html.rs` for homepage and folder-index pages, replacing the
/// direct `generate_children()` call so both frontmatter and embed paths
/// flow through the same `resolve_markers()` + `render_one()` pipeline.
///
/// `folder_path` — the target folder's site path without leading/trailing slash
/// (e.g. `"news"` for `children: '[[News]]'`, `"articles"` for a folder index
/// listing its own children, `""` for the root). An absolute leading `/` is
/// added by this function so `resolve_folder_id` never misinterprets the path
/// as relative.
///
/// `from_md_path` — source markdown path for relative path resolution in render_one.
///
/// `is_homepage` — true for the homepage path: applies depth="all" default and
/// enables the lang-tree + nav-item filters. False for folder-index pages.
pub fn synthesize_children_marker(
    doc: &crate::build::types::ParsedDocument,
    folder_path: &str,
    from_md_path: &str,
    is_homepage: bool,
) -> String {
    use moss_core::resolve::embed_renderer::folder_list::{FolderEmbedParams, emit_marker};

    // Homepage defaults depth to "all" when unset; folder index defaults to "direct".
    let depth = doc.children_depth.clone().or_else(|| {
        if is_homepage {
            Some("all".to_string())
        } else {
            None // render_one defaults to "direct" when absent
        }
    });

    // Homepage default-mode (the root home listing its own tree): scope to the
    // default language tree and exclude top-level nav-item folders. Only the root
    // home is `is_homepage` (other folder homes go through the folder-index path and
    // are scoped by their folder prefix), so its tree is always the default tree.
    // Cross-folder mode (children_source set) explicitly targets another folder —
    // user intent overrides these filters.
    let has_children_source = doc.children_source.is_some();
    let homepage_default_mode = is_homepage && !has_children_source;

    let params = FolderEmbedParams {
        limit: doc.children_limit.map(|n| n as usize),
        sort: None,  // sort comes from target doc's direct_children_sort cache
        style: doc.children_style.as_ref().map(|r| r.value.clone()),
        depth,
        group: doc.children_group.as_ref().map(|r| r.value.clone()),
        // `children:` frontmatter listings always render the card-grid branch,
        // never an iframe, so size is irrelevant here.
        size: None,
        covers: doc.children_covers.clone(),
        // Empty-string guard matches the embed grammar's own `more:`
        // key, which never carries an empty value either.
        more: doc.children_more.clone().filter(|s| !s.is_empty()),
        scope_default_tree: homepage_default_mode,
        exclude_nav: homepage_default_mode,
        // A frontmatter listing has no pothole to read placement or a
        // caption out of; both are body-embed vocabulary.
        placement: Default::default(),
        caption: None,
    };
    // Always emit an absolute path (leading /) so resolve_folder_id doesn't
    // interpret the path as relative to from_md_path's parent directory.
    // A relative "blog/" emitted from "blog/index.md" would resolve to "blog/blog".
    let path_with_slash = if folder_path.is_empty() {
        "/".to_string()
    } else {
        format!("/{}/", folder_path) // allow:served-path-url-construct
    };
    emit_marker(&path_with_slash, from_md_path, &params)
}

#[cfg(test)]
#[path = "folder_embed_tests.rs"]
pub(crate) mod tests;
