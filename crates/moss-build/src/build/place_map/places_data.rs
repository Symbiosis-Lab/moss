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
//! choice. A companion is folded into its work's `companions` list and is
//! never independently evaluated; every other located page — one with no
//! work folder, or the home page of an ordinary section that isn't a work
//! — becomes its own standalone entry with no companions.
//!
//! A work (or standalone page) whose declared locations resolve to no
//! coordinates at all is skipped outright — the same rule
//! [`super::PlaceMapTarget::has_coordinates`] gates the static map
//! renderer with, so a place line that only ever showed as text never
//! grows a dot with no coordinate behind it.
//!
//! # Precision is privacy
//!
//! A place's `lat`/`lng` are rounded to its own precision's resolution
//! before they ever reach this JSON — never the raw gazetteer decimal
//! degrees. See [`round_to_precision`].

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use moss_core::terms::term_folder_key;

use super::{PlaceMapContext, PlaceMapRenderContext, ResolvedPlace};
use crate::build::assets::paths::compute_content_hash;
use crate::build::context::BuildContext;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::media::cover::{detect_cover_type, CoverType};
use crate::build::page::meta::resolve_page_description;
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

#[derive(serde::Serialize)]
struct PlaceEntry {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent: Option<String>,
    precision: Precision,
    lat: f64,
    lng: f64,
}

/// Root-relative, pretty-URL form of a page's `url_path` — the same
/// `format!("/{}")` + [`to_pretty_url`] dance `render/blocking.rs`'s
/// `_moss/previews.json` emitter uses, so a raw `.html`-suffixed
/// `url_path` (if one ever reaches here) normalizes the same way a hover
/// preview's key does.
fn page_url(url_path: &str) -> String {
    let permalink = format!("/{}", url_path.trim_start_matches('/'));
    let pretty = to_pretty_url(&permalink);
    format!("/{}", pretty.trim_start_matches('/'))
}

/// A page's cover, resolved to the smaller asset the media pipeline
/// produces for it — never the full-size original. `None` given no cover.
///
/// Image covers resolve to the deployed WebP re-encode
/// (`moss_core::asset_paths::to_webp`): the build always produces this
/// file for every raster cover regardless of its dimensions, unlike a
/// specific responsive rung (`w800`/`w1600`), which `ladder_rungs` omits
/// entirely once a cover's deployed width is at or under that rung's
/// width — and this pure emitter never sees a cover's natural dimensions
/// to know whether a given rung was ever produced. Guessing one would risk
/// a 404; the deployed base never does. Video covers resolve to their
/// poster JPEG (`moss_core::asset_paths::to_thumb`), produced for every
/// video cover. Iframe covers have no processed image asset at all.
///
/// The resolved path is root-relative, via the same
/// `pinned_url`-over-`split_pipe` dance `build::components::child_list`
/// uses for card covers — minus that call site's `dir_overrides` table,
/// which this emitter has no access to; a folder whose `url:` frontmatter
/// renames its served directory is the one case this can point at the
/// pre-rename path. Rare, and never a broken link on its own: `pinned_url`
/// still slugs and percent-encodes the path, only the override table is
/// absent.
fn resolve_cover(cover: Option<&str>, cover_type_override: Option<&str>) -> Option<String> {
    let raw = cover?.trim();
    if raw.is_empty() {
        return None;
    }
    let (path_part, _attrs) = moss_core::media::split_pipe(raw);
    let resolved =
        moss_core::resolve::output_url::pinned_url(path_part, &std::collections::HashMap::new());
    match detect_cover_type(&resolved, cover_type_override) {
        CoverType::Video => Some(moss_core::asset_paths::to_thumb(&resolved)),
        CoverType::Iframe => None,
        CoverType::Image => Some(moss_core::asset_paths::to_webp(&resolved)),
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
fn build_entry(
    maps: &PlaceMapContext,
    gazetteer: &Gazetteer,
    namespace: &str,
    documents: &[ParsedDocument],
    home_idx: usize,
    companion_indices: &[usize],
    math: bool,
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
        byline: home.byline.clone(),
        description: resolve_page_description(home.description.as_deref(), &home.content, math),
        cover: resolve_cover(home.cover.as_deref(), home.cover_type.as_deref()),
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
pub fn emit_places_data(documents: &[ParsedDocument], context: &PlaceMapRenderContext, math: bool) -> String {
    let gazetteer = context.gazetteer();
    let namespace = context.namespace();
    let maps = context.maps();
    let parents = context.parents();
    let works = group_by_work(documents);
    let mut claimed: HashSet<usize> = HashSet::new();
    for group in works.values() {
        claimed.insert(group.home_idx);
        claimed.extend(group.companions.iter().copied());
    }

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
                    precision: place.precision,
                    lat,
                    lng,
                }
            });
        }
    };

    for group in works.values() {
        if !documents[group.home_idx].is_public_page() {
            continue;
        }
        if let Some((entry, resolved)) =
            build_entry(maps, gazetteer, namespace, documents, group.home_idx, &group.companions, math)
        {
            fold_places(resolved, &mut places);
            entries.push(entry);
        }
    }

    for (i, doc) in documents.iter().enumerate() {
        if claimed.contains(&i) || !doc.is_public_page() || doc.location.is_empty() {
            continue;
        }
        if let Some((entry, resolved)) = build_entry(maps, gazetteer, namespace, documents, i, &[], math) {
            fold_places(resolved, &mut places);
            entries.push(entry);
        }
    }

    entries.sort_by(|a, b| a.id.cmp(&b.id));
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
pub fn emit(
    documents: &[ParsedDocument],
    context: &PlaceMapRenderContext,
    math: bool,
    output_dir: &Path,
    pending: &mut PendingManifest,
) -> std::io::Result<()> {
    let json = emit_places_data(documents, context, math);
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
            parents: Default::default(), explorer: None,
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

    #[test]
    fn a_places_parent_reaches_a_roll_up_only_ancestor_with_no_gazetteer_row() {
        // Japan has no row of its own in `gazetteer()` — only Kyoto's and
        // Osaka's `parent = "Japan"` links name it. The site still builds a
        // real "places/japan" page from those links alone, so Kyoto's
        // entry here must name it too, not silently drop `parent`.
        let docs = vec![standalone("posts/kyoto-report.md", "posts/kyoto-report/", "Kyoto Report", &["Kyoto"])];
        let json = emit_places_data(&docs, &context(gazetteer()), true);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let places = parsed["places"].as_array().unwrap();
        let kyoto = places.iter().find(|p| p["id"] == "places/kyoto").expect("Kyoto entry present");
        assert_eq!(kyoto["parent"], "places/japan", "{json}");
    }

    #[test]
    fn a_work_is_one_entry_with_its_companions_and_the_homes_places_only() {
        let docs = vec![
            home("works/kyoto-walk/kyoto-walk.md", "works/kyoto-walk/", "Kyoto Walk", &["Kyoto"]),
            companion("works/kyoto-walk/morning.md", "works/kyoto-walk/morning/", "Morning"),
            companion("works/kyoto-walk/evening.md", "works/kyoto-walk/evening/", "Evening"),
        ];
        let json = emit_places_data(&docs, &context(gazetteer()), true);
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
        let json = emit_places_data(&docs, &context(gazetteer()), true);
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
        let json = emit_places_data(&docs, &context(gazetteer()), true);
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
        let json = emit_places_data(&docs, &context(gazetteer()), true);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let works = parsed["works"].as_array().unwrap();
        assert_eq!(works.len(), 1);
        assert_eq!(works[0]["places"], serde_json::json!(["places/kyoto"]), "Osaka never leaks in: {json}");
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
        let before = emit_places_data(&docs, &context(gazetteer()), true);

        // An unrelated page: no `location:`, so it never enters the output
        // at all. Only append it — don't touch the located page above.
        docs.push(doc("about.md", "about/", "About", PageKind::Article));
        let after = emit_places_data(&docs, &context(gazetteer()), true);

        assert_eq!(before, after, "a page with no resolved place must never move the emitted bytes");
    }

    #[test]
    fn a_places_toml_edit_that_moves_a_resolved_coordinate_changes_the_output() {
        let docs = vec![standalone("posts/kyoto-report.md", "posts/kyoto-report/", "Kyoto Report", &["Kyoto"])];
        let before = emit_places_data(&docs, &context(gazetteer()), true);

        let moved: toml::value::Table = toml::from_str(
            "[\"Kyoto\"]\nlat = 36.0\nlng = 136.0\nprecision = \"city\"\nparent = \"Japan\"\n",
        )
        .unwrap();
        let moved_gazetteer = crate::vault::places::parse_gazetteer(&moved);
        let after = emit_places_data(&docs, &context(moved_gazetteer), true);

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
        let json = emit_places_data(&docs, &context(gazetteer()), true);

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
