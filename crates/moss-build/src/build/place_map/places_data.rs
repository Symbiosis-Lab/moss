//! `places.<hash>.json`: the places-explorer's data file — one entry per
//! work (grouped exactly as [`crate::build::terms::inherit`] groups a
//! work's companions) plus every standalone located page, alongside the
//! flat list of places their `places` ids resolve to.
//!
//! # The one-dot-per-work rule
//!
//! A work is a folder whose home page is its self-named file — the same
//! test [`group_by_work`] already applies to decide which companions
//! inherit the home's location (`inherit.rs`'s own module doc names this
//! emitter as its forward contract). This pass reuses that exact grouping
//! rather than re-deriving it: **only the home page's own `location:`
//! drives a work's entry**, even when a companion names a place of its
//! own — `inherit.rs` calls this the explorer's "one-dot-per-work" display
//! choice. A companion is folded into its work's `companions` list only when
//! the work is itself on the map (public, located, with coordinates) and the
//! companion's location equals the home's; every other located page — one
//! with a place of its own, one under a home that is not on the map, one
//! with no work folder, or the home page of an ordinary section that isn't
//! a work — becomes its own standalone entry with no companions.
//!
//! A folder whose self-named home page names no place is not a work at all
//! ([`group_by_work`] leaves it out): its pages are evaluated one by one, so
//! a located page inside it is its own entry and an unlocated one is simply
//! not on the map.
//!
//! A work (or standalone page) whose declared locations resolve to no
//! coordinates at all is skipped outright, and its located companions are
//! then judged on their own — the same rule
//! [`super::PlaceMapTarget::has_coordinates`] gates the static map
//! renderer with, so a place line that only ever showed as text never
//! grows a dot with no coordinate behind it.
//!
//! # Precision is privacy
//!
//! A place's `lat`/`lng` are rounded to its own precision's resolution
//! before they ever reach this JSON — never the raw gazetteer decimal
//! degrees. See [`round_to_precision`].

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use moss_core::terms::term_folder_key;

use super::{PlaceMapContext, PlaceMapRenderContext, ResolvedPlace};
use crate::build::assets::paths::compute_content_hash;
use crate::build::context::BuildContext;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::media::cover::{detect_cover_type, CoverType};
use crate::build::page::meta::{description_from_content, strip_markdown_inline};
use crate::build::scan::article_map::to_pretty_url;
use crate::build::served_path::ServedPath;
use crate::build::terms::inherit::group_by_work;
use crate::build::types::ParsedDocument;
use crate::vault::places::{Gazetteer, Precision};

#[derive(serde::Serialize)]
struct PlacesData {
    works: Vec<WorkEntry>,
    places: Vec<PlaceEntry>,
}

#[derive(serde::Serialize)]
struct WorkEntry {
    id: String,
    title: String,
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    date: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    byline: Vec<String>,
    /// The page's `author:` names — what a collapsed card shows. The byline
    /// is a free-form credit line and stays for the expanded detail.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    authors: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cover: Option<String>,
    places: Vec<String>,
    companions: Vec<Companion>,
}

#[derive(serde::Serialize)]
struct Companion {
    title: String,
    url: String,
}

/// `precision`/`lat`/`lng` are absent on a GROUPING node: an ancestor
/// [`fold_ancestors`] had to invent so a chip menu can still dig through
/// it, reachable only by `parent` reference and never resolved to a point
/// (no gazetteer row at all, or a row with no coordinates). The runtime
/// never draws a marker for one — [`fold_ancestors`]'s own doc names the
/// two cases — so there is nothing to round or privacy-tier for it.
#[derive(Debug, serde::Serialize)]
struct PlaceEntry {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    precision: Option<Precision>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lat: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lng: Option<f64>,
}

/// Root-relative, pretty-URL form of a page's `url_path` — the same
/// `format!("/{}")` + [`to_pretty_url`] dance `render/blocking.rs`'s
/// `_moss/previews.json` emitter uses, so a raw `.html`-suffixed
/// `url_path` (if one ever reaches here) normalizes the same way a hover
/// preview's key does.
///
/// `pub(crate)`: `context.rs`'s `render_locator` calls this directly to
/// compute the SAME id this module gives a work's own `Work.id` below, so
/// the article locator embed's `article=` URL param can never drift from
/// what `places.<hash>.json` actually keys works by.
pub(crate) fn page_url(url_path: &str) -> String {
    let permalink = format!("/{}", url_path.trim_start_matches('/'));
    let pretty = to_pretty_url(&permalink);
    format!("/{}", pretty.trim_start_matches('/'))
}

/// A page's cover, resolved to a URL the build actually serves. `None`
/// given no cover.
///
/// Image covers resolve to their own plain resolved path — the ORIGINAL,
/// never a guessed `.webp` sibling. The responsive ladder's webp re-encode
/// (`moss_core::asset_paths::to_webp`) only ever exists as a `<source>`
/// inside a `<picture>` moss-core's own synthesizer builds
/// (`render/image.rs`'s `synthesize_image_html`, the same path
/// `build::media::cover::render_cover_html` routes an ordinary listing
/// card's cover through): that synthesizer's own `<img>` fallback — the one
/// element this emitter can actually reproduce, since it builds a card with
/// plain JS from this JSON, never a `<picture>`’s `<source>` set — keeps the
/// ORIGINAL path. Pointing a card here at the guessed webp path instead
/// produced a 404 on every raster cover on a real build (0 of 25): the
/// asset pipeline copies and sizes the original at its own resolved path
/// regardless of format, and only ADDS the webp re-encode beside it for a
/// `<picture>` to pick from — it never serves a bare `<img src>` at that
/// webp path on its own. A non-raster source (svg, gif, avif, ...) already
/// took this branch before this fix (`is_raster_original`'s own exact
/// extension set never gave those a webp at all); rasters now follow it
/// too. Video covers resolve to their poster JPEG
/// (`moss_core::asset_paths::to_thumb`), produced for every video cover.
/// Iframe covers have no processed image asset at all.
///
/// The resolved path is root-relative, via the same
/// `pinned_url`-over-`split_pipe` dance `build::components::child_list`
/// uses for card covers, including that call site's own `dir_overrides`
/// table (`overrides` here) — a folder whose `url:` frontmatter renames its
/// served directory resolves to the RENAMED path, matching where the asset
/// actually landed. Without it, every cover under such a folder pointed at
/// the pre-rename path instead and 404'd outright (0 of 25 on a real site
/// whose folders carry non-ASCII names with an ASCII `url:` override).
fn resolve_cover(
    cover: Option<&str>,
    cover_type_override: Option<&str>,
    overrides: &HashMap<String, String>,
) -> Option<String> {
    let raw = cover?.trim();
    if raw.is_empty() {
        return None;
    }
    let (path_part, _attrs) = moss_core::media::split_pipe(raw);
    let resolved = moss_core::resolve::output_url::pinned_url(path_part, overrides);
    match detect_cover_type(&resolved, cover_type_override) {
        CoverType::Video => Some(moss_core::asset_paths::to_thumb(&resolved)),
        CoverType::Iframe => None,
        CoverType::Image => Some(resolved),
    }
}

/// Round a coordinate to its precision's own resolution — the privacy
/// control, not a display nicety. An exact place keeps 3 decimals (~110m
/// at the equator), a city 2 (~1.1km), a region or country 1 (~11km) — the
/// same centre the static map's fade already draws a region/country at.
/// Never emit a digit beyond this.
fn round_to_precision(value: f64, precision: Precision) -> f64 {
    let decimals = match precision {
        Precision::Exact => 3,
        Precision::City => 2,
        Precision::Region | Precision::Country => 1,
    };
    let factor = 10f64.powi(decimals);
    (value * factor).round() / factor
}

/// A resolved place's parent id, read off the SAME cycle-repaired
/// `key -> parent display name` map the term-listing breadcrumb/roll-up
/// resolves through (`build::terms::places::attach_parents`,
/// `PlaceMapRenderContext.parents`'s own doc comment) — never a second,
/// gazetteer-only lookup. A gazetteer-only lookup would miss a roll-up-only
/// ancestor that has no row of its own (a gazetteer entry can name a parent
/// that itself is never declared — the site still gets a generated page for
/// it, built from the parent link alone), which is exactly the case this
/// emitter must not silently drop `parent` for. One hop only, matching
/// [`PlaceEntry::parent`]'s own shape — never a full walk to the namespace
/// root, so there is no cycle to guard against the way
/// `PlaceMapRenderContext::lies_under` must for an unbounded one.
fn parent_id(parents: &BTreeMap<String, String>, namespace: &str, key: &str) -> Option<String> {
    parents.get(key).map(|display| term_folder_key(namespace, display))
}

/// Walk every place already in `places` up its parent chain, inserting an
/// entry for each ancestor that is missing one — an intermediate place no
/// work names directly (a country whose city is the only one a work is
/// located at) otherwise gets a `parent` id [`parent_id`] still points at,
/// but that id names no entry in the emitted `places` list at all: the
/// runtime's root menu can then only ever offer the handful of places that
/// happen to be parentless, and a roll-up count under an unreachable
/// ancestor has nothing to roll up onto.
///
/// An ancestor's own name/precision/coordinates come from the gazetteer
/// (`.moss/places.toml`), by the SAME display name [`parents`] already
/// names it with — never invented. Two cases never resolve to a point, and
/// NEITHER ends the walk there any more (it used to: a reviewer found the
/// early `continue` in both dropped every ancestor above the unresolved
/// one, not just that one entry):
///
/// - **No gazetteer row at all** — a country used only as a grouping level,
///   declared nowhere but a `parent =` reference. `parent_id` already
///   tolerates this dangling id for the page the term pipeline still
///   generates from the link alone; this now gives it a matching entry too
///   — a GROUPING node (`PlaceEntry`'s own doc), `id`/`name` from the
///   parent reference itself, carrying no `precision`/`lat`/`lng`.
/// - **A row with no coordinates** — the gazetteer knows the name but was
///   never given a point for it. Same grouping-node treatment, except
///   `precision` IS known (the row has one) even though the point isn't.
///
/// Either way the walk still climbs past it: the loop always re-queues
/// `parent_key`, so a grouping node's OWN further ancestor (if `parents`
/// names one) still gets its chance to resolve, including back to a real
/// point further up. Iterative with a worklist, not recursive on
/// `BTreeMap` — `break_cycles` already fixed `parents` to a finite chain
/// before this runs, and `places.contains_key` stops re-walking a chain two
/// leaves share a common ancestor through (and is what still terminates a
/// cycle `break_cycles` somehow missed: each key can only ever be inserted,
/// and therefore only ever re-queued, once).
fn fold_ancestors(
    places: &mut BTreeMap<String, PlaceEntry>,
    parents: &BTreeMap<String, String>,
    gazetteer: &Gazetteer,
    namespace: &str,
) {
    let mut queue: Vec<String> = places.keys().cloned().collect();
    while let Some(key) = queue.pop() {
        let Some(parent_display) = parents.get(&key) else { continue };
        let parent_key = term_folder_key(namespace, parent_display);
        if places.contains_key(&parent_key) {
            continue;
        }
        let record = gazetteer.get(parent_display);
        let point = record.and_then(|r| r.coords);
        let (precision, lat, lng) = match point {
            Some((lat, lng)) => {
                // `record` is `Some` whenever `point` is — `coords` is a
                // field on the row `point` was read off.
                let precision = record.unwrap().precision;
                (Some(precision), Some(round_to_precision(lat, precision)), Some(round_to_precision(lng, precision)))
            }
            None => (record.map(|r| r.precision), None, None),
        };
        places.insert(
            parent_key.clone(),
            PlaceEntry {
                id: parent_key.clone(),
                name: parent_display.clone(),
                parent: parent_id(parents, namespace, &parent_key),
                precision,
                lat,
                lng,
            },
        );
        queue.push(parent_key);
    }
}

/// Resolve one page's declared locations to the subset that actually has
/// coordinates — [`PlaceMapContext::resolve_locations`], the exact path
/// the static map renderer resolves through, so this emitter's "does this
/// place have a dot" answer can never disagree with the locator's.
/// `None` when none of the declared names resolve to a point at all (the
/// "unresolvable location is absent" rule); otherwise the place ids to
/// attach to the entry, plus the resolved places to fold into the flat
/// places list.
fn resolve_entry_places(
    maps: &PlaceMapContext,
    gazetteer: &Gazetteer,
    namespace: &str,
    names: &[String],
) -> Option<(Vec<String>, Vec<ResolvedPlace>)> {
    // `route` is irrelevant to this data feed — the places-explorer never
    // draws a route (rule: listing/explorer surfaces don't) — so this always
    // resolves with `false` regardless of the page's own `route:` flag.
    let target = maps.resolve_locations(namespace, gazetteer, names, false);
    if !target.has_coordinates() {
        return None;
    }
    let marked: Vec<ResolvedPlace> = target.marker_places().cloned().collect();
    let ids = marked.iter().map(|place| place.key.clone()).collect();
    Some((ids, marked))
}

/// Build one [`WorkEntry`] from a home page and its (possibly empty)
/// companions. `None` when the home's declared locations resolve to no
/// coordinates at all — the caller skips the whole work in that case, home
/// and companions alike, matching the static renderer's own skip rule.
/// A work's `description:`/body-excerpt, reduced to plain text — the same
/// two-rung precedence [`crate::build::page::meta::resolve_page_description_with_fallbacks`]'s
/// first two rungs give a share card (explicit `description:` beats the
/// body's own first paragraph), but through [`strip_markdown_inline`]
/// instead of [`crate::build::page::meta::extract_description_markdown`]:
/// a card here is read as plain text by the explorer's own markup (never
/// rendered as HTML the way a share card's `<meta content>` or a grid
/// card's description slot is), so a `description:` with an inline link
/// must not reach the JSON as `[text](url)`.
fn plain_description(description: Option<&str>, content: &str, math: bool) -> Option<String> {
    description
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(strip_markdown_inline)
        .filter(|d| !d.trim().is_empty())
        .or_else(|| description_from_content(Some(content), math))
}

fn build_entry(
    maps: &PlaceMapContext,
    gazetteer: &Gazetteer,
    namespace: &str,
    documents: &[ParsedDocument],
    home_idx: usize,
    companion_indices: &[usize],
    math: bool,
    dir_overrides: &HashMap<String, String>,
) -> Option<(WorkEntry, Vec<ResolvedPlace>)> {
    let home = &documents[home_idx];
    let (place_ids, resolved) = resolve_entry_places(maps, gazetteer, namespace, &home.location)?;
    let companions = companion_indices
        .iter()
        .copied()
        .filter(|&i| documents[i].is_public_page())
        .map(|i| Companion { title: documents[i].title.clone(), url: page_url(&documents[i].url_path) })
        .collect();
    let entry = WorkEntry {
        id: page_url(&home.url_path),
        title: home.title.clone(),
        url: page_url(&home.url_path),
        date: home.date.clone(),
        // Byline is authored as inline markdown (often a linked author
        // name, `[Name](/people/name/)`) for the page's own credit line;
        // the explorer's compact card reads it as plain text, never HTML.
        byline: home.byline.iter().map(|b| strip_markdown_inline(b)).collect(),
        authors: home.author.clone(),
        description: plain_description(home.description.as_deref(), &home.content, math),
        cover: resolve_cover(home.cover.as_deref(), home.cover_type.as_deref(), dir_overrides),
        places: place_ids,
        companions,
    };
    Some((entry, resolved))
}

/// Emit the places-explorer's whole data file as a JSON string. See this
/// module's doc for the shape and the one-dot-per-work rule.
///
/// `context` is the same [`PlaceMapRenderContext`] the build's own
/// locator/term-map rendering resolves through (`build/pipeline.rs`'s
/// `place_maps` construction, cloned once rather than re-decoded) — its
/// bundled [`PlaceMapContext`], gazetteer and place-typed namespace key mean
/// this emitter can never answer "does this place have a dot" differently
/// than the pages it describes already did, and its cycle-repaired
/// `key -> parent display name` map (`attach_parents` already built it onto
/// the place-typed `TermKind` before the context was constructed) is what
/// [`parent_id`] reads. `math` is the site's `[site].math` setting, the same
/// value the pages render with (see `extract_description`'s own doc for why
/// a math-blind parse here would corrupt a footnote-bearing excerpt).
pub fn emit_places_data(
    documents: &[ParsedDocument],
    context: &PlaceMapRenderContext,
    math: bool,
    dir_overrides: &HashMap<String, String>,
) -> String {
    let gazetteer = context.gazetteer();
    let namespace = context.namespace();
    let maps = context.maps();
    let parents = context.parents();
    let works = group_by_work(documents);
    let mut claimed: HashSet<usize> = HashSet::new();

    let mut entries = Vec::new();
    let mut places: BTreeMap<String, PlaceEntry> = BTreeMap::new();

    let fold_places = |resolved: Vec<ResolvedPlace>, places: &mut BTreeMap<String, PlaceEntry>| {
        for place in resolved {
            places.entry(place.key.clone()).or_insert_with(|| {
                // `marker_places()` already filtered to `point().is_some()`,
                // so these are never the fallback — `unwrap_or` only keeps
                // this from ever being a panic path.
                let lat = round_to_precision(place.latitude.unwrap_or(0.0), place.precision);
                let lng = round_to_precision(place.longitude.unwrap_or(0.0), place.precision);
                PlaceEntry {
                    id: place.key.clone(),
                    name: place.display.clone(),
                    parent: parent_id(parents, namespace, &place.key),
                    precision: Some(place.precision),
                    lat: Some(lat),
                    lng: Some(lng),
                }
            });
        }
    };

    for group in works.values() {
        let home = &documents[group.home_idx];
        if !home.is_public_page() {
            continue;
        }
        // Only a companion at the home's own place is folded into the work;
        // one that names a place of its own is judged as its own work below.
        let folded: Vec<usize> =
            group.companions.iter().copied().filter(|&i| documents[i].location.is_empty() || documents[i].location == home.location).collect();
        if let Some((entry, resolved)) = build_entry(
            maps,
            gazetteer,
            namespace,
            documents,
            group.home_idx,
            &folded,
            math,
            dir_overrides,
        ) {
            fold_places(resolved, &mut places);
            entries.push(entry);
            claimed.insert(group.home_idx);
            claimed.extend(folded);
        }
    }

    for (i, doc) in documents.iter().enumerate() {
        if claimed.contains(&i) || !doc.is_public_page() || doc.location.is_empty() {
            continue;
        }
        if let Some((entry, resolved)) = build_entry(maps, gazetteer, namespace, documents, i, &[], math, dir_overrides) {
            fold_places(resolved, &mut places);
            entries.push(entry);
        }
    }

    entries.sort_by(|a, b| a.id.cmp(&b.id));
    fold_ancestors(&mut places, parents, gazetteer, namespace);
    let places: Vec<PlaceEntry> = places.into_values().collect();

    let data = PlacesData { works: entries, places };
    serde_json::to_string(&data).unwrap_or_else(|_| r#"{"works":[],"places":[]}"#.to_string())
}

/// Emit `places.<hash>.json` into the build, content-hashed over exactly
/// what [`emit_places_data`] returns: the gazetteer plus every located
/// page's resolved places and card fields. An unrelated page — one with
/// no resolved place, never part of the emitted JSON at all — cannot move
/// the hash. `feature_styles::emit`'s `wrapped()` hash earns the same
/// property the same way: by hashing an output that is already a pure
/// function of what belongs in it, rather than by a second, narrower
/// fingerprint function.
///
/// The caller gates this on already knowing the site declares a
/// place-typed kind — `pipeline.rs` only calls it when `context` is
/// `Some`, which `place_maps`'s own construction already requires one for.
/// `dir_overrides` is `BackgroundContext::dir_overrides`, already resolved
/// by the time this runs — the same directory-slug table a card cover
/// (`resolve_cover`) needs to point at a `url:`-renamed folder's actual
/// served path instead of its pre-rename one.
pub fn emit(
    documents: &[ParsedDocument],
    context: &PlaceMapRenderContext,
    math: bool,
    output_dir: &Path,
    pending: &mut PendingManifest,
    dir_overrides: &HashMap<String, String>,
) -> std::io::Result<()> {
    let json = emit_places_data(documents, context, math, dir_overrides);
    let hash = compute_content_hash(&json);
    BuildContext::for_render(output_dir, pending).emit(
        &ServedPath::for_places_data_hashed(&hash),
        json.as_bytes(),
        HashBucket::Files,
    )
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use moss_core::PageKind;

    /// A raster cover (png/jpg/jpeg/webp) resolves to its own plain
    /// resolved path, NEVER a guessed `.webp` sibling — the real build
    /// never serves a bare `<img src>` at that guessed path (it only ever
    /// exists as a `<picture><source>` moss-core's own synthesizer builds),
    /// so a card pointed there 404'd on every raster cover on a real site
    /// (0 of 25). The plain path is what the asset pipeline actually
    /// copies and sizes at, regardless of source format.
    #[test]
    fn resolve_cover_serves_a_raster_sources_own_resolved_path_never_a_guessed_webp() {
        let overrides = HashMap::new();
        assert_eq!(resolve_cover(Some("cover.jpg"), None, &overrides).as_deref(), Some("/cover.jpg"));
        assert_eq!(resolve_cover(Some("cover.PNG"), None, &overrides).as_deref(), Some("/cover.PNG"));
    }

    /// An SVG cover must serve its own raw path too — `is_raster_original`'s
    /// own extension set never gave a non-raster source a webp at all, even
    /// before the raster fix above; this pins that the two branches still
    /// agree now that they're one branch.
    #[test]
    fn resolve_cover_serves_an_svg_sources_own_raw_path() {
        assert_eq!(resolve_cover(Some("cover.svg"), None, &HashMap::new()).as_deref(), Some("/cover.svg"));
    }

    /// `cover_type_override`/the file extension still route video and
    /// iframe covers the way they always did — this test only pins that
    /// the raster fix above didn't fold Image's branch into those.
    #[test]
    fn resolve_cover_still_routes_video_and_iframe_covers() {
        let overrides = HashMap::new();
        assert_eq!(resolve_cover(Some("clip.mp4"), None, &overrides).as_deref(), Some("/clip.thumb.jpg"));
        assert_eq!(resolve_cover(Some("embed.html"), None, &overrides), None);
    }

    /// A cover inside a folder whose index carries a `url:` override (a
    /// non-ASCII folder name served at an ASCII slug, the real-site case
    /// that motivated threading `dir_overrides` through at all) resolves to
    /// the OVERRIDDEN path — where the asset pipeline actually placed the
    /// file — not the pre-rename source path, which 404s.
    #[test]
    fn resolve_cover_honours_a_folder_url_override() {
        let mut overrides = HashMap::new();
        overrides.insert("日記".to_string(), "diary".to_string());
        assert_eq!(
            resolve_cover(Some("日記/cover.jpg"), None, &overrides).as_deref(),
            Some("/diary/cover.jpg"),
        );
    }

    fn gazetteer() -> Gazetteer {
        let table: toml::value::Table = toml::from_str(
            "[\"Kyoto\"]\nlat = 35.0116\nlng = 135.7681\nprecision = \"city\"\nparent = \"Japan\"\n\n\
             [\"Osaka\"]\nlat = 34.6937\nprecision = \"city\"\nparent = \"Japan\"\n\n\
             [\"Nara\"]\nlat = 34.6851\nlng = 135.8048\nprecision = \"exact\"\n",
        )
        .unwrap();
        crate::vault::places::parse_gazetteer(&table)
    }

    fn maps() -> PlaceMapContext {
        PlaceMapContext::embedded().unwrap()
    }

    /// A full render context built from a gazetteer parsed fresh from
    /// `toml_str`, with `parents` derived from THAT SAME gazetteer via
    /// `attach_parents` — unlike [`context`] below, which always pairs a
    /// caller-supplied gazetteer with the one, fixed [`parents`] derived
    /// from the module's own default [`gazetteer`]. A multi-level chain
    /// test needs its own gazetteer's own parent links reflected in
    /// `parents` too, which `context` cannot give it.
    fn context_from_gazetteer_toml(toml_str: &str) -> PlaceMapRenderContext {
        let table: toml::value::Table = toml::from_str(toml_str).unwrap();
        let gaz = crate::vault::places::parse_gazetteer(&table);
        let mut kinds = vec![crate::build::terms::TermKind {
            key: "places".to_string(),
            fields: vec!["location".to_string()],
            title: "Places".to_string(),
            is_place: true,
            parents: Default::default(),
            explorer: None, line: None,
        }];
        crate::build::terms::places::attach_parents(&mut kinds, &gaz);
        let parents = kinds.into_iter().next().unwrap().parents;
        PlaceMapRenderContext::new(maps(), gaz, "places".to_string(), crate::build::place_map::LocatorPlacement::None, parents)
    }

    /// The cycle-repaired `key -> parent display name` map
    /// `attach_parents` would build from [`gazetteer`]'s `parent = "Japan"`
    /// links — Japan itself has no row, the roll-up-only case `parent_id`
    /// must still surface.
    fn parents() -> BTreeMap<String, String> {
        let mut kinds = vec![crate::build::terms::TermKind {
            key: "places".to_string(),
            fields: vec!["location".to_string()],
            title: "Places".to_string(),
            is_place: true,
            parents: Default::default(), explorer: None, line: None,
        }];
        crate::build::terms::places::attach_parents(&mut kinds, &gazetteer());
        kinds.into_iter().next().unwrap().parents
    }

    /// The render context `emit_places_data` now takes whole, built from a
    /// caller-supplied gazetteer (so the hash-movement tests below can swap
    /// it) plus the standing `maps()`/`parents()` fixtures.
    fn context(gazetteer: Gazetteer) -> PlaceMapRenderContext {
        PlaceMapRenderContext::new(
            maps(),
            gazetteer,
            "places".to_string(),
            crate::build::place_map::LocatorPlacement::None,
            parents(),
        )
    }

    fn doc(source_path: &str, url_path: &str, title: &str, kind: PageKind) -> ParsedDocument {
        ParsedDocument {
            source_path: Some(source_path.to_string()),
            url_path: url_path.to_string(),
            title: title.to_string(),
            kind,
            ..Default::default()
        }
    }

    fn home(source_path: &str, url_path: &str, title: &str, location: &[&str]) -> ParsedDocument {
        let mut d = doc(source_path, url_path, title, PageKind::Folder);
        d.location = location.iter().map(|s| s.to_string()).collect();
        d
    }

    fn companion(source_path: &str, url_path: &str, title: &str) -> ParsedDocument {
        doc(source_path, url_path, title, PageKind::Article)
    }

    fn standalone(source_path: &str, url_path: &str, title: &str, location: &[&str]) -> ParsedDocument {
        let mut d = doc(source_path, url_path, title, PageKind::Article);
        d.location = location.iter().map(|s| s.to_string()).collect();
        d
    }

    /// Pins the wire contract `types.ts`'s `WorkWire`/`normalizePlacesData`
    /// is built to tolerate: a work with no byline serializes with the key
    /// ABSENT, not `"byline":[]` or `"byline":null` — the same
    /// `skip_serializing_if` shape `date`/`description`/`cover` already had.
    /// `types.ts` once declared `byline` required when the wire can
    /// truthfully omit it this way, which crashed the explorer's first
    /// render on any unauthored work; this test is what would have caught
    /// the two sides drifting apart again, from the Rust side.
    #[test]
    fn a_work_with_no_byline_serializes_with_the_key_absent() {
        let docs = vec![standalone("posts/kyoto-report.md", "posts/kyoto-report/", "Kyoto Report", &["Kyoto"])];
        let json = emit_places_data(&docs, &context(gazetteer()), true, &HashMap::new());
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let work = &parsed["works"][0];
        assert!(work.get("byline").is_none(), "byline must be ABSENT, not null or [], when a work has none: {json}");
        // date/description/cover already behave this way; pinned alongside
        // byline so a future reviewer sees all four together.
        assert!(work.get("date").is_none(), "{json}");
        assert!(work.get("cover").is_none(), "{json}");
    }

    /// `byline:` is authored as inline markdown — a linked author name is
    /// the common case (`[Name](/people/name/)`) — and `description:` can
    /// carry one too. The explorer's card reads both as plain text, never
    /// HTML, so neither may reach the wire with its markdown intact: a raw
    /// `](` in a collapsed card is exactly the defect this pins.
    #[test]
    fn byline_and_description_markdown_links_are_reduced_to_plain_text() {
        let mut work = home("works/kyoto-walk/kyoto-walk.md", "works/kyoto-walk/", "Kyoto Walk", &["Kyoto"]);
        work.byline = vec!["[黃毛](/people/黃毛/)".to_string(), "editor: [蘇美智](/people/蘇美智/)".to_string()];
        work.description = Some("See [this report](/posts/other/) for more.".to_string());
        let docs = vec![work];

        let json = emit_places_data(&docs, &context(gazetteer()), true, &HashMap::new());
        assert!(!json.contains("]("), "no raw markdown link syntax may reach the wire: {json}");

        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let w = &parsed["works"][0];
        let byline: Vec<&str> = w["byline"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(byline, vec!["黃毛", "editor: 蘇美智"], "{json}");
        assert_eq!(w["description"], "See this report for more.", "{json}");
    }

    /// `author:` names reach the wire as their own list, apart from the
    /// byline credit line, and the key is absent when a page has none.
    #[test]
    fn authors_serialize_apart_from_the_byline_and_are_absent_when_empty() {
        let mut authored = standalone("posts/kyoto-report.md", "posts/kyoto-report/", "Kyoto Report", &["Kyoto"]);
        authored.author = vec!["Ana Reyes".to_string(), "Bo Lind".to_string()];
        authored.byline = vec!["Photographs: [Cy](/people/cy/)".to_string()];
        let unauthored = standalone("posts/nara-diary.md", "posts/nara-diary/", "Nara Diary", &["Nara"]);
        let json = emit_places_data(&[authored, unauthored], &context(gazetteer()), true, &HashMap::new());
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let works = parsed["works"].as_array().unwrap();
        let kyoto = works.iter().find(|w| w["title"] == "Kyoto Report").unwrap();
        assert_eq!(kyoto["authors"], serde_json::json!(["Ana Reyes", "Bo Lind"]), "{json}");
        assert_eq!(kyoto["byline"], serde_json::json!(["Photographs: Cy"]), "{json}");
        let nara = works.iter().find(|w| w["title"] == "Nara Diary").unwrap();
        assert!(nara.get("authors").is_none(), "authors must be absent, not [] or null: {json}");
    }

    #[test]
    fn a_places_parent_reaches_a_roll_up_only_ancestor_with_no_gazetteer_row() {
        // Japan has no row of its own in `gazetteer()` — only Kyoto's and
        // Osaka's `parent = "Japan"` links name it. The site still builds a
        // real "places/japan" page from those links alone, so Kyoto's
        // entry here must name it too, not silently drop `parent`.
        let docs = vec![standalone("posts/kyoto-report.md", "posts/kyoto-report/", "Kyoto Report", &["Kyoto"])];
        let json = emit_places_data(&docs, &context(gazetteer()), true, &HashMap::new());
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let places = parsed["places"].as_array().unwrap();
        let kyoto = places.iter().find(|p| p["id"] == "places/kyoto").expect("Kyoto entry present");
        assert_eq!(kyoto["parent"], "places/japan", "{json}");

        // Japan itself must now ALSO be a reachable entry — a name-only
        // GROUPING node, not a dangling id the chip menu can never offer:
        // the hierarchy used to stay flat here because the ancestor walk
        // ended the moment it found no gazetteer row for "Japan", instead
        // of still emitting a grouping node and continuing upward.
        let japan = places.iter().find(|p| p["id"] == "places/japan").expect("Japan must be a reachable entry, not just a dangling parent id");
        assert_eq!(japan["name"], "Japan", "{json}");
        assert!(japan.get("parent").is_none(), "Japan has no further ancestor in this fixture: {json}");
        assert!(japan.get("lat").is_none(), "a grouping node with no gazetteer row has no coordinates to draw a marker from: {json}");
        assert!(japan.get("lng").is_none(), "{json}");
        assert!(japan.get("precision").is_none(), "no gazetteer row means no precision tier either: {json}");
    }

    /// Three levels: Japan (country) → Kansai (region) → Kyoto (city), and
    /// no work is ever located at Kansai itself — only at Kyoto, through it.
    /// Before the ancestor walk, Kansai never entered the emitted `places`
    /// list at all (only places a work directly resolves to did), so Kyoto's
    /// `parent` id named an entry the runtime could never look up: the root
    /// menu could offer Kyoto or Japan but never dig through Kansai, and
    /// Japan's own roll-up count had no middle rung to climb. This is 31 of
    /// 56 places on a real site this emitter was built against.
    #[test]
    fn an_intermediate_place_with_no_direct_work_still_gets_its_own_entry() {
        let ctx = context_from_gazetteer_toml(
            "[\"Kyoto\"]\nlat = 35.0116\nlng = 135.7681\nprecision = \"city\"\nparent = \"Kansai\"\n\n\
             [\"Kansai\"]\nlat = 34.75\nlng = 135.5\nprecision = \"region\"\nparent = \"Japan\"\n\n\
             [\"Japan\"]\nlat = 36.0\nlng = 138.0\nprecision = \"country\"\n",
        );

        let docs = vec![standalone("posts/kyoto-report.md", "posts/kyoto-report/", "Kyoto Report", &["Kyoto"])];
        let json = emit_places_data(&docs, &ctx, true, &HashMap::new());
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let places = parsed["places"].as_array().unwrap();

        let kyoto = places.iter().find(|p| p["id"] == "places/kyoto").expect("Kyoto entry present");
        assert_eq!(kyoto["parent"], "places/kansai", "{json}");

        let kansai = places.iter().find(|p| p["id"] == "places/kansai");
        assert!(kansai.is_some(), "the work-less middle place must still get its own entry: {json}");
        let kansai = kansai.unwrap();
        assert_eq!(kansai["name"], "Kansai", "{json}");
        assert_eq!(kansai["parent"], "places/japan", "{json}");

        let japan = places.iter().find(|p| p["id"] == "places/japan");
        assert!(japan.is_some(), "the root ancestor must also be emitted: {json}");
        assert!(japan.unwrap().get("parent").is_none(), "Japan has no parent of its own: {json}");
    }

    /// Same three-level chain, except Kansai's OWN gazetteer row now has no
    /// `lat`/`lng` at all (a region whose boundary was declared but never
    /// given a point) — the second of the two "must not end the walk" cases
    /// a reviewer called out: the old code's `let Some((lat, lng)) = ...
    /// else { continue }` stopped climbing the instant Kansai failed to
    /// resolve, so Japan — a real point one hop further up — never got
    /// folded in either.
    #[test]
    fn a_three_level_chain_whose_middle_has_no_coordinates_still_reaches_the_top() {
        let ctx = context_from_gazetteer_toml(
            "[\"Kyoto\"]\nlat = 35.0116\nlng = 135.7681\nprecision = \"city\"\nparent = \"Kansai\"\n\n\
             [\"Kansai\"]\nprecision = \"region\"\nparent = \"Japan\"\n\n\
             [\"Japan\"]\nlat = 36.0\nlng = 138.0\nprecision = \"country\"\n",
        );

        let docs = vec![standalone("posts/kyoto-report.md", "posts/kyoto-report/", "Kyoto Report", &["Kyoto"])];
        let json = emit_places_data(&docs, &ctx, true, &HashMap::new());
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let places = parsed["places"].as_array().unwrap();

        let kyoto = places.iter().find(|p| p["id"] == "places/kyoto").expect("Kyoto entry present");
        assert_eq!(kyoto["parent"], "places/kansai", "{json}");

        let kansai = places.iter().find(|p| p["id"] == "places/kansai")
            .expect("Kansai must still get a grouping entry even with no coordinates of its own");
        assert_eq!(kansai["name"], "Kansai", "{json}");
        assert_eq!(kansai["parent"], "places/japan", "a coordinate-less middle must not end the walk — Japan is still its parent: {json}");
        assert!(kansai.get("lat").is_none(), "no coordinates to draw a marker from: {json}");
        assert!(kansai.get("lng").is_none(), "{json}");
        assert_eq!(kansai["precision"], "region", "the row's own precision is still known, even without a point: {json}");

        let japan = places.iter().find(|p| p["id"] == "places/japan")
            .expect("the walk must reach past the coordinate-less middle to the real point above it");
        assert_eq!(japan["lat"], 36.0, "{json}");
        assert_eq!(japan["lng"], 138.0, "{json}");
    }

    /// `fold_ancestors` itself, directly: `attach_parents`'s own
    /// `break_cycles` already fixes a cycle in `parents` before this ever
    /// runs (the function's own doc), but this pins the walk's OWN
    /// termination regardless of that upstream guarantee — the same
    /// defensive posture `scope.ts`'s `placeMatchesScope` takes on the
    /// runtime side, for the identical unbounded-walk risk, and for the
    /// same reason: a second bug upstream should not turn into a hung
    /// build. A hand-built `parents` map (not `attach_parents`'s output) is
    /// the only way to feed this function an actual cycle to terminate.
    #[test]
    fn fold_ancestors_terminates_even_if_parents_contains_a_cycle() {
        let mut places: BTreeMap<String, PlaceEntry> = BTreeMap::new();
        places.insert(
            "places/alpha".to_string(),
            PlaceEntry {
                id: "places/alpha".to_string(),
                name: "Alpha".to_string(),
                parent: None,
                precision: None,
                lat: None,
                lng: None,
            },
        );
        let mut cyclic_parents = BTreeMap::new();
        cyclic_parents.insert("places/alpha".to_string(), "Beta".to_string());
        cyclic_parents.insert("places/beta".to_string(), "Alpha".to_string());

        fold_ancestors(&mut places, &cyclic_parents, &Gazetteer::default(), "places");

        assert!(places.contains_key("places/alpha"));
        assert!(places.contains_key("places/beta"));
        assert_eq!(places.len(), 2, "the cycle must not re-walk and re-insert forever: {places:?}");
    }

    #[test]
    fn a_work_is_one_entry_with_its_companions_and_the_homes_places_only() {
        let docs = vec![
            home("works/kyoto-walk/kyoto-walk.md", "works/kyoto-walk/", "Kyoto Walk", &["Kyoto"]),
            companion("works/kyoto-walk/morning.md", "works/kyoto-walk/morning/", "Morning"),
            companion("works/kyoto-walk/evening.md", "works/kyoto-walk/evening/", "Evening"),
        ];
        let json = emit_places_data(&docs, &context(gazetteer()), true, &HashMap::new());
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let works = parsed["works"].as_array().unwrap();
        assert_eq!(works.len(), 1, "one entry for the whole work, not one per page: {json}");
        let work = &works[0];
        assert_eq!(work["id"], "/works/kyoto-walk/");
        assert_eq!(work["places"], serde_json::json!(["places/kyoto"]));
        let companions: Vec<(String, String)> = work["companions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| (c["title"].as_str().unwrap().to_string(), c["url"].as_str().unwrap().to_string()))
            .collect();
        assert_eq!(
            companions,
            vec![
                ("Morning".to_string(), "/works/kyoto-walk/morning/".to_string()),
                ("Evening".to_string(), "/works/kyoto-walk/evening/".to_string()),
            ]
        );
    }

    #[test]
    fn a_standalone_located_page_is_one_entry_with_no_companions() {
        let docs = vec![standalone("posts/kyoto-report.md", "posts/kyoto-report/", "Kyoto Report", &["Kyoto"])];
        let json = emit_places_data(&docs, &context(gazetteer()), true, &HashMap::new());
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let works = parsed["works"].as_array().unwrap();
        assert_eq!(works.len(), 1);
        assert_eq!(works[0]["companions"], serde_json::json!([]));
        assert_eq!(works[0]["places"], serde_json::json!(["places/kyoto"]));
    }

    #[test]
    fn a_work_with_an_unresolvable_location_is_absent() {
        // Osaka's gazetteer row has no `lng` — it never resolves to a point,
        // so the whole work (home AND its companion) must be absent, not
        // emitted with an empty `places` list.
        let docs = vec![
            home("works/osaka-notes/osaka-notes.md", "works/osaka-notes/", "Osaka Notes", &["Osaka"]),
            companion("works/osaka-notes/detail.md", "works/osaka-notes/detail/", "Detail"),
        ];
        let json = emit_places_data(&docs, &context(gazetteer()), true, &HashMap::new());
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["works"].as_array().unwrap().len(), 0, "{json}");
        assert_eq!(parsed["places"].as_array().unwrap().len(), 0, "{json}");
    }

    #[test]
    fn a_companions_own_location_never_drives_the_work_entry() {
        let docs = vec![
            home("works/kyoto-walk/kyoto-walk.md", "works/kyoto-walk/", "Kyoto Walk", &["Kyoto"]),
            {
                let mut c = companion("works/kyoto-walk/evening.md", "works/kyoto-walk/evening/", "Evening");
                c.location = vec!["Osaka".to_string()];
                c
            },
        ];
        let json = emit_places_data(&docs, &context(gazetteer()), true, &HashMap::new());
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let works = parsed["works"].as_array().unwrap();
        assert_eq!(works.len(), 1);
        assert_eq!(works[0]["places"], serde_json::json!(["places/kyoto"]), "Osaka never leaks in: {json}");
    }

    fn work_ids(json: &str) -> Vec<String> {
        let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
        parsed["works"].as_array().unwrap().iter().map(|w| w["id"].as_str().unwrap().to_string()).collect()
    }

    fn located_companion(source_path: &str, url_path: &str, title: &str, location: &[&str]) -> ParsedDocument {
        standalone(source_path, url_path, title, location)
    }

    #[test]
    fn a_companion_with_a_different_location_is_its_own_work_under_a_located_home() {
        let docs = vec![
            home("works/kyoto-walk/kyoto-walk.md", "works/kyoto-walk/", "Kyoto Walk", &["Kyoto"]),
            located_companion("works/kyoto-walk/detour.md", "works/kyoto-walk/detour/", "Detour", &["Nara"]),
        ];
        let json = emit_places_data(&docs, &context(gazetteer()), true, &HashMap::new());
        assert_eq!(work_ids(&json), vec!["/works/kyoto-walk/", "/works/kyoto-walk/detour/"], "{json}");
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["works"][0]["companions"], serde_json::json!([]), "the detour is not also a companion: {json}");
    }

    #[test]
    fn a_located_companion_under_a_non_public_home_is_its_own_work() {
        let mut work = home("works/kyoto-walk/kyoto-walk.md", "works/kyoto-walk/", "Kyoto Walk", &["Kyoto"]);
        work.draft = Some(true);
        let docs = vec![
            work,
            located_companion("works/kyoto-walk/morning.md", "works/kyoto-walk/morning/", "Morning", &["Kyoto"]),
        ];
        let json = emit_places_data(&docs, &context(gazetteer()), true, &HashMap::new());
        assert_eq!(work_ids(&json), vec!["/works/kyoto-walk/morning/"], "{json}");
    }

    #[test]
    fn a_located_companion_under_a_home_with_no_coordinates_is_its_own_work() {
        // Osaka has no `lng`, so the home never resolves to a point.
        let docs = vec![
            home("works/osaka-notes/osaka-notes.md", "works/osaka-notes/", "Osaka Notes", &["Osaka"]),
            located_companion("works/osaka-notes/day-trip.md", "works/osaka-notes/day-trip/", "Day Trip", &["Nara"]),
        ];
        let json = emit_places_data(&docs, &context(gazetteer()), true, &HashMap::new());
        assert_eq!(work_ids(&json), vec!["/works/osaka-notes/day-trip/"], "{json}");
    }

    /// A section folder (self-named index note with no `location:`) holding
    /// located essays: each essay is its own work, the unlocated sibling is
    /// not on the map, and the section's home is not either.
    #[test]
    fn a_located_page_under_an_unlocated_home_is_its_own_work() {
        let docs = vec![
            home("essays/essays.md", "essays/", "Essays", &[]),
            standalone("essays/kyoto-essay.md", "essays/kyoto-essay/", "Kyoto Essay", &["Kyoto"]),
            companion("essays/preface.md", "essays/preface/", "Preface"),
        ];
        let json = emit_places_data(&docs, &context(gazetteer()), true, &HashMap::new());
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let works = parsed["works"].as_array().unwrap();
        assert_eq!(works.len(), 1, "{json}");
        assert_eq!(works[0]["id"], "/essays/kyoto-essay/", "{json}");
        assert_eq!(works[0]["companions"], serde_json::json!([]), "{json}");
    }

    #[test]
    fn an_unlocated_page_under_an_unlocated_home_is_not_on_the_map() {
        let docs = vec![
            home("essays/essays.md", "essays/", "Essays", &[]),
            companion("essays/preface.md", "essays/preface/", "Preface"),
        ];
        let json = emit_places_data(&docs, &context(gazetteer()), true, &HashMap::new());
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["works"].as_array().unwrap().len(), 0, "{json}");
    }

    #[test]
    fn an_unlocated_chapter_under_a_located_home_stays_its_companion() {
        let docs = vec![
            home("works/kyoto-walk/kyoto-walk.md", "works/kyoto-walk/", "Kyoto Walk", &["Kyoto"]),
            companion("works/kyoto-walk/morning.md", "works/kyoto-walk/morning/", "Morning"),
        ];
        let json = emit_places_data(&docs, &context(gazetteer()), true, &HashMap::new());
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let works = parsed["works"].as_array().unwrap();
        assert_eq!(works.len(), 1, "{json}");
        assert_eq!(works[0]["companions"].as_array().unwrap().len(), 1, "{json}");
    }

    #[test]
    fn precision_rounds_to_its_own_digit_count() {
        assert_eq!(round_to_precision(35.011_678_9, Precision::Exact), 35.012);
        assert_eq!(round_to_precision(35.011_678_9, Precision::City), 35.01);
        assert_eq!(round_to_precision(35.011_678_9, Precision::Region), 35.0);
        assert_eq!(round_to_precision(35.011_678_9, Precision::Country), 35.0);

        // Never more digits than the precision allows, whatever the input.
        for (value, precision, expected_decimals) in [
            (139.766_084, Precision::Exact, 3),
            (139.766_084, Precision::City, 2),
            (139.766_084, Precision::Region, 1),
            (139.766_084, Precision::Country, 1),
        ] {
            let rounded = round_to_precision(value, precision);
            let text = format!("{rounded}");
            let decimals = text.split_once('.').map(|(_, frac)| frac.len()).unwrap_or(0);
            assert!(
                decimals <= expected_decimals,
                "{precision:?} must keep at most {expected_decimals} decimals, got {decimals} ({text})"
            );
        }
    }

    #[test]
    fn an_unrelated_page_edit_never_moves_the_output() {
        let mut docs = vec![standalone("posts/kyoto-report.md", "posts/kyoto-report/", "Kyoto Report", &["Kyoto"])];
        let before = emit_places_data(&docs, &context(gazetteer()), true, &HashMap::new());

        // An unrelated page: no `location:`, so it never enters the output
        // at all. Only append it — don't touch the located page above.
        docs.push(doc("about.md", "about/", "About", PageKind::Article));
        let after = emit_places_data(&docs, &context(gazetteer()), true, &HashMap::new());

        assert_eq!(before, after, "a page with no resolved place must never move the emitted bytes");
    }

    #[test]
    fn a_places_toml_edit_that_moves_a_resolved_coordinate_changes_the_output() {
        let docs = vec![standalone("posts/kyoto-report.md", "posts/kyoto-report/", "Kyoto Report", &["Kyoto"])];
        let before = emit_places_data(&docs, &context(gazetteer()), true, &HashMap::new());

        let moved: toml::value::Table = toml::from_str(
            "[\"Kyoto\"]\nlat = 36.0\nlng = 136.0\nprecision = \"city\"\nparent = \"Japan\"\n",
        )
        .unwrap();
        let moved_gazetteer = crate::vault::places::parse_gazetteer(&moved);
        let after = emit_places_data(&docs, &context(moved_gazetteer), true, &HashMap::new());

        assert_ne!(before, after, "a gazetteer edit that moves a resolved place's coordinates must move the hash-bearing bytes");
    }

    #[test]
    fn a_realistic_fixture_stays_well_under_its_brotli_budget() {
        // Measured on the `places-site` snapshot fixture's own `places.json`
        // (two standalone located posts plus one work with two companions,
        // the same document shapes built below): 986 raw bytes, 349 bytes
        // brotli q11. The ceiling leaves headroom for new optional fields
        // without masking a real regression — e.g. accidentally
        // pretty-printing the JSON, or a card field growing unbounded.
        const BROTLI_Q11_BUDGET: usize = 1024;

        let docs = vec![
            standalone("posts/kyoto-report.md", "posts/kyoto-report/", "Kyoto Report", &["Kyoto"]),
            standalone("posts/nara-diary.md", "posts/nara-diary/", "Nara Diary", &["Nara", "Kyoto"]),
            home("works/kyoto-walk/kyoto-walk.md", "works/kyoto-walk/", "Kyoto Walk", &["Kyoto"]),
            companion("works/kyoto-walk/morning.md", "works/kyoto-walk/morning/", "Morning"),
            companion("works/kyoto-walk/evening.md", "works/kyoto-walk/evening/", "Evening"),
        ];
        let json = emit_places_data(&docs, &context(gazetteer()), true, &HashMap::new());

        let mut compressor = brotli::CompressorWriter::new(Vec::new(), 4096, 11, 22);
        compressor.write_all(json.as_bytes()).unwrap();
        let compressed = compressor.into_inner();
        assert!(
            compressed.len() <= BROTLI_Q11_BUDGET,
            "raw={} brotli-q11={} exceeds the {}-byte budget",
            json.len(),
            compressed.len(),
            BROTLI_Q11_BUDGET
        );
    }
}
