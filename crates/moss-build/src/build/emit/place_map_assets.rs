//! Content-addressed world/tile base-map SVGs for the places explorer
//! (`place_map::emit_world_svg`/`emit_tile_svg`), emitted only when the site
//! has a place-typed term kind — a site with no places pays nothing.
//!
//! The world SVG is always emitted; which regional tiles are worth
//! rendering depends on the site's own gazetteer (`place_map::
//! relevant_tiles` — a place's cell plus its eight neighbours, so panning
//! one cell away from a place still has detail, without shipping tiles for
//! parts of the world this site never mentions). Rendering even that
//! bounded set is expensive enough that it must not run on every build:
//! each asset goes through the transform cache
//! (`build::cache::TransformCache`, the same content-addressed store the
//! image/video pipeline reuses) under its OWN key — the pack fingerprint,
//! [`GENERATOR_VERSION`] and, for a tile, its coordinates (`Asset::
//! cache_key`) — never the full regional cell set. [`assets_hash`] folds
//! in that whole set too, but only to name the served `_moss/map.<hash>/`
//! directory; keying the cache itself by it would mean a gazetteer edit
//! that adds one place in a new region, which changes `relevant_tiles` and
//! so moves `assets_hash`, evicts the world map and every already-rendered
//! tile along with it. A warm cache never calls back into
//! `place_map::emit_world_svg`/`emit_tile_svg` and
//! never re-reads or re-hashes a blob's bytes to materialize it into the
//! stage: it carries the xxHash3 manifest digest computed at RENDER time
//! inside the cached transform entry's `params` (alongside the invalidation
//! key, `generator_version`) and hands it straight to
//! `PendingManifest::register_hashed`, while the bytes themselves move
//! stage-ward through `ObjectStore::link_to` (a filesystem clone/hardlink,
//! not a byte copy through this process). Only the stage write itself — the
//! one thing every build must do regardless, the stage being rebuilt fresh
//! every time, the same reasoning `media::remote_cover`'s module doc gives —
//! runs on every build.
//!
//! `tiles.json` lists the emitted regional tiles' `[x, y]` pairs plus `k`,
//! the factor a tile is drawn at over the world's own scale
//! (`place_map::geometry::TILE_K`), so the explorer knows which cells exist
//! without probing for a 404 per candidate, and derives its own tile
//! detail ceiling and placement transform from the SAME `k` this build
//! rendered the tiles at — never a separately-maintained copy that could
//! drift from it (`js-src/site/places-explorer/camera.ts`'s
//! `tileDetailMaxZoom`, `tiles.ts`'s `TileLayer`).

use std::path::Path;

use crate::build::assets::paths::compute_binary_hash;
use crate::build::cache::{ObjectStore, TransformCache, TransformEntry, TransformRecord};
use crate::build::context::BuildContext;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::place_map;
use crate::build::place_map::{TILE_BLEED, TILE_K};
use crate::build::served_path::ServedPath;
use crate::moss_paths::MossPaths;
use crate::vault::places::Gazetteer;

/// `tiles.json`'s own shape: `k` and `bleed` alongside the emitted cells, so
/// the runtime never has to hardcode or re-derive the factor a tile was
/// rendered at, or the margin its own canvas was padded by beyond its
/// nominal cell (see this module's own doc, and `geometry::TILE_BLEED`).
#[derive(serde::Serialize)]
struct TileIndex<'a> {
    k: f64,
    bleed: f64,
    cells: &'a [(i16, i16)],
}

/// Bumped whenever `place_map::emit_world_svg`/`emit_tile_svg` changes in a
/// way that must invalidate every cached SVG even though the embedded pack
/// bytes did not — a new layer, a paint-order fix, a projection change.
/// 2: `emit_tile_svg` now draws a tile in the world's own Patterson
/// projection at `TILE_K` times its scale, clipped to the cell's own
/// rectangle, instead of a separately-cropped `FlatProjection` — a
/// pre-existing site's own cached/staged tiles (and the directory name
/// itself, since `assets_hash` folds this constant in) must not survive the
/// upgrade unrendered.
/// 3: a tile's own canvas now bleeds `TILE_BLEED` past its nominal cell on
/// every edge, so its declared size (and `tiles.json`'s new `bleed` field)
/// both changed — a pre-existing site's cached tiles were rendered one
/// `TILE_BLEED` narrower and must not survive the upgrade unrendered.
pub const GENERATOR_VERSION: u32 = 3;

/// The hash naming this build's `_moss/map.<hash>/` directory: the pack's
/// own fingerprint (its source manifest digest, from the embedded
/// `MOSSPLM1` header — changes only when the compiled-in pack data
/// changes), [`GENERATOR_VERSION`], and the set of regional cells
/// `place_map::relevant_tiles` derives from `gazetteer` (sorted, so
/// iteration order never matters). Never a function of site config beyond
/// the gazetteer, or of page content at all, so a page edit never moves
/// this directory and a `.moss/places.toml` edit moves it only when it
/// actually changes which cells matter — see
/// `directory_name_is_unmoved_by_an_unrelated_page_edit` and
/// `directory_name_moves_when_a_places_toml_edit_moves_a_place_to_a_new_cell`
/// below.
pub fn assets_hash(context: &place_map::PlaceMapContext, gazetteer: &Gazetteer) -> String {
    let mut buf = Vec::new();
    buf.extend_from_slice(&context.pack_fingerprint());
    buf.extend_from_slice(&GENERATOR_VERSION.to_le_bytes());
    // `labels.json` (`emit::place_map_labels`) lands in this SAME
    // directory, so its own change fingerprint belongs in this SAME hash —
    // see `PlaceMapContext::labels_bytes`'s own doc for why raw label
    // bytes, not the pack fingerprint above, are what catches a
    // generator-only change.
    buf.extend_from_slice(context.labels_bytes());
    for (x, y) in place_map::relevant_tiles(gazetteer, context.pack()) {
        buf.extend_from_slice(&x.to_le_bytes());
        buf.extend_from_slice(&y.to_le_bytes());
    }
    compute_binary_hash(&buf)
}

/// One asset this module can emit, and how to render it when the cache
/// misses. Kept as a closed enum (not a string parsed back into `x`/`y`)
/// so a malformed cached name can never be misread as a tile.
enum Asset {
    World,
    Tile(i16, i16),
}

impl Asset {
    fn name(&self) -> String {
        match self {
            Self::World => "world".to_string(),
            Self::Tile(x, y) => format!("tile-{x}-{y}"),
        }
    }

    fn render(&self, context: &place_map::PlaceMapContext) -> String {
        match self {
            Self::World => place_map::emit_world_svg(context),
            Self::Tile(x, y) => place_map::emit_tile_svg(context, *x, *y),
        }
    }

    /// This asset's own transform-cache key: the pack fingerprint,
    /// [`GENERATOR_VERSION`], and — for a tile — its coordinates. Never the
    /// rest of the gazetteer's relevant-cell set [`assets_hash`] folds in,
    /// so adding a place in a new region (which changes `relevant_tiles`
    /// and therefore `assets_hash`) does not change the key the world map
    /// or an already-rendered tile looks itself up under. `assets_hash`
    /// still names the served `_moss/map.<hash>/` directory; this is the
    /// cache's own key, used nowhere else.
    fn cache_key(&self, context: &place_map::PlaceMapContext) -> String {
        let mut buf = Vec::new();
        buf.extend_from_slice(&context.pack_fingerprint());
        buf.extend_from_slice(&GENERATOR_VERSION.to_le_bytes());
        match self {
            Self::World => buf.push(0),
            Self::Tile(x, y) => {
                buf.push(1);
                buf.extend_from_slice(&x.to_le_bytes());
                buf.extend_from_slice(&y.to_le_bytes());
            }
        }
        compute_binary_hash(&buf)
    }
}

/// `pipeline.rs`'s one call site, matching this crate's `emit/`-family
/// convention of a single build-phase entry point the caller merely gates
/// (see `emit.rs`'s module doc). `render_context` is `Some` exactly when
/// the caller's own `kind.is_place` test built a `PlaceMapRenderContext` —
/// `None` gates this out the same way a bool would, but also hands over
/// the gazetteer already bundled inside it, so there is no second
/// `load_gazetteer` call at the call site. Decode and emission errors are
/// logged, never fatal: a broken place-map pack must not fail the whole
/// build.
pub fn emit_if_place_typed(
    render_context: Option<&place_map::PlaceMapRenderContext>,
    paths: &MossPaths,
    output_dir: &Path,
    pending: &mut PendingManifest,
) {
    let Some(render_context) = render_context else {
        return;
    };
    match place_map::PlaceMapContext::embedded() {
        Ok(context) => {
            if let Err(error) = emit(&context, render_context.gazetteer(), paths, output_dir, pending) {
                crate::build::cli_output::log_warn_problem!("place-map explorer assets could not be emitted: {error}");
            }
        }
        Err(error) => {
            crate::build::cli_output::log_warn_problem!(
                "bundled place-map data could not be decoded ({error:?}); omitting explorer assets"
            );
        }
    }
}

/// Emit the world SVG and every populated tile's SVG under
/// `_moss/map.<assets_hash(context)>/`. Returns the number of assets that
/// were actually (re-)rendered this build (0 on a fully warm cache) —
/// informational, callers don't need to act on it.
pub fn emit(
    context: &place_map::PlaceMapContext,
    gazetteer: &Gazetteer,
    paths: &MossPaths,
    output_dir: &Path,
    pending: &mut PendingManifest,
) -> Result<usize, String> {
    let objects = ObjectStore::for_site(paths);
    let transforms = TransformCache::for_site(paths);
    let hash = assets_hash(context, gazetteer);

    let regional_tiles = place_map::relevant_tiles(gazetteer, context.pack());
    let mut assets = vec![Asset::World];
    assets.extend(regional_tiles.iter().map(|&(x, y)| Asset::Tile(x, y)));

    let mut rendered = 0usize;
    for asset in &assets {
        let name = asset.name();
        let served = ServedPath::for_place_map_asset(&hash, &format!("{name}.svg"))
            .map_err(|e| format!("Invalid place-map asset path for '{name}': {e}"))?;
        let target = output_dir.join(served.as_str());

        // Looked up by this asset's OWN identity, not by `hash` — see the
        // module doc. A gazetteer edit that adds a place elsewhere changes
        // `hash` but never this record's key, so the lookup still hits.
        let cache_key = asset.cache_key(context);
        let mut record = transforms.get(&cache_key).unwrap_or_else(|| TransformRecord {
            source_oid: cache_key.clone(),
            source_size: 0,
            transforms: std::collections::HashMap::new(),
        });

        // A cached entry carries its own xxHash3 manifest digest (stashed in
        // `params` at render time below), so a hit needs neither a re-read
        // of the blob nor a re-hash of its bytes to register — see the
        // module doc.
        let cached_xxh3 = record.transforms.get(&name).and_then(|entry| {
            let generator_matches = entry.params.get("generator_version").and_then(serde_json::Value::as_u64)
                == Some(u64::from(GENERATOR_VERSION));
            generator_matches.then(|| entry.params.get("xxh3")?.as_str().map(str::to_string)).flatten()
        });

        // The path is content-addressed by `hash` (pack + generator
        // version) and `name` alone, so a file already sitting at `target`
        // — left there by `sweep_staging` carrying the previous build's
        // stage forward, since this build hasn't registered it as stale
        // yet — is already the right bytes: register it without touching
        // the object store at all. This is what makes an unchanged-pack
        // rebuild cost a stat per asset instead of a blob-readiness check
        // plus a file copy.
        if let (true, Some(xxh3)) = (target.exists(), &cached_xxh3) {
            pending.register_hashed(&served, xxh3, HashBucket::Files);
            continue;
        }

        let cached = cached_xxh3.and_then(|xxh3| {
            let oid = record.transforms.get(&name).map(|entry| entry.oid.clone())?;
            objects.ready_blob(&oid).is_some().then_some((oid, xxh3))
        });

        match cached {
            Some((oid, xxh3)) => {
                objects
                    .link_to(&oid, &target)
                    .map_err(|e| format!("Failed to link cached place-map asset '{name}': {e}"))?;
                pending.register_hashed(&served, &xxh3, HashBucket::Files);
            }
            None => {
                let svg = asset.render(context);
                let bytes = svg.as_bytes();
                let xxh3 = compute_binary_hash(bytes);
                let oid = objects
                    .store_bytes(bytes)
                    .map_err(|e| format!("Failed to store place-map asset '{name}': {e}"))?;
                record.transforms.insert(
                    name.clone(),
                    TransformEntry {
                        oid,
                        size: bytes.len() as u64,
                        params: serde_json::json!({ "generator_version": GENERATOR_VERSION, "xxh3": xxh3 }),
                    },
                );
                BuildContext::for_render(output_dir, pending)
                    .emit(&served, bytes, HashBucket::Files)
                    .map_err(|e| format!("Failed to emit place-map asset '{name}': {e}"))?;
                transforms
                    .put(&record)
                    .map_err(|e| format!("Failed to save place-map asset cache record for '{name}': {e}"))?;
                rendered += 1;
            }
        }
    }

    // The index is cheap to (re-)compute — a few hundred small integers at
    // most — so it is simply written every build rather than threaded
    // through the transform cache like the SVGs: nothing here is worth
    // caching on its own.
    let index_sp = ServedPath::for_place_map_asset(&hash, "tiles.json")
        .map_err(|e| format!("Invalid place-map tile index path: {e}"))?;
    let index_json = serde_json::to_vec(&TileIndex { k: TILE_K, bleed: TILE_BLEED, cells: &regional_tiles })
        .map_err(|e| format!("Failed to serialize tiles.json: {e}"))?;
    BuildContext::for_render(output_dir, pending)
        .emit(&index_sp, &index_json, HashBucket::Files)
        .map_err(|e| format!("Failed to emit place-map tile index: {e}"))?;

    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::content::SiteHashes;

    fn scratch_paths(label: &str) -> (tempfile::TempDir, MossPaths) {
        let dir = tempfile::tempdir().unwrap();
        let moss_dir = dir.path().join(format!(".moss-{label}"));
        std::fs::create_dir_all(&moss_dir).unwrap();
        let paths = MossPaths::from_moss_dir(moss_dir);
        (dir, paths)
    }

    fn gazetteer(entries: &str) -> Gazetteer {
        let table: toml::value::Table = toml::from_str(entries).unwrap();
        crate::vault::places::parse_gazetteer(&table)
    }

    fn empty_gazetteer() -> Gazetteer {
        gazetteer("")
    }

    #[test]
    fn assets_hash_changes_with_generator_version_and_is_stable_otherwise() {
        let context = place_map::PlaceMapContext::embedded().unwrap();
        let gaz = empty_gazetteer();
        let first = assets_hash(&context, &gaz);
        let second = assets_hash(&context, &gaz);
        assert_eq!(first, second);
        // The hash must react to a generator-version bump — simulated here
        // by hashing with a different version constant directly, since the
        // real constant cannot change at test time.
        let mut buf = Vec::new();
        buf.extend_from_slice(&context.pack_fingerprint());
        buf.extend_from_slice(&(GENERATOR_VERSION + 1).to_le_bytes());
        buf.extend_from_slice(context.labels_bytes());
        let bumped = compute_binary_hash(&buf);
        assert_ne!(first, bumped);
    }

    /// A label-only change must move the shared directory even though the
    /// pack fingerprint (the source-manifest digest) does not: a generator
    /// change that encodes different label bytes from the SAME pinned
    /// sources is exactly the case `pack_fingerprint()` alone would miss
    /// (see `PlaceMapContext::labels_bytes`'s own doc).
    #[test]
    fn assets_hash_reacts_to_a_labels_only_change_even_when_the_pack_fingerprint_does_not() {
        let context = place_map::PlaceMapContext::embedded().unwrap();
        let mut pack = context.pack().clone();
        assert_ne!(pack.labels_bytes, vec![0xaa, 0xbb], "fixture must actually change the bytes");
        pack.labels_bytes = vec![0xaa, 0xbb];
        let changed = place_map::PlaceMapContext::new(pack);
        let gaz = empty_gazetteer();
        assert_eq!(context.pack_fingerprint(), changed.pack_fingerprint());
        assert_ne!(assets_hash(&context, &gaz), assets_hash(&changed, &gaz));
    }

    /// The directory name must react only to the pack/generator/gazetteer,
    /// never to page content: emitting twice, with an unrelated
    /// PendingManifest mutation (a page's own file registered) between
    /// calls, produces the SAME `_moss/map.<hash>/` directory both times.
    #[test]
    fn directory_name_is_unmoved_by_an_unrelated_page_edit() {
        let context = place_map::PlaceMapContext::embedded().unwrap();
        let gaz = gazetteer("[\"Beirut\"]\nlat = 33.89\nlng = 35.5\nprecision = \"city\"\n");
        let (_dir, paths) = scratch_paths("dir-stable");
        let output_dir = tempfile::tempdir().unwrap();
        let mut pending = PendingManifest::new(SiteHashes::default());

        emit(&context, &gaz, &paths, output_dir.path(), &mut pending).unwrap();
        let first_dirs = emitted_map_dirs(output_dir.path());
        assert_eq!(first_dirs.len(), 1, "{first_dirs:?}");

        // An unrelated page write — stands in for an ordinary page edit,
        // which never touches the pack or the gazetteer.
        pending.register(
            &ServedPath::from_source("unrelated-page/index.html").unwrap(),
            b"<html></html>",
            HashBucket::Files,
        );

        emit(&context, &gaz, &paths, output_dir.path(), &mut pending).unwrap();
        let second_dirs = emitted_map_dirs(output_dir.path());
        assert_eq!(first_dirs, second_dirs, "the map directory moved for an unrelated page edit");
    }

    /// A `.moss/places.toml` edit that moves a place far enough to land in
    /// a different pack cell changes the directory — the regional tile set
    /// `assets_hash` folds in has genuinely changed.
    #[test]
    fn directory_name_moves_when_a_places_toml_edit_moves_a_place_to_a_new_cell() {
        let context = place_map::PlaceMapContext::embedded().unwrap();
        let before = gazetteer("[\"Beirut\"]\nlat = 33.89\nlng = 35.5\nprecision = \"city\"\n");
        // Bangkok: a different 10-degree cell entirely, not merely a
        // within-cell nudge.
        let after = gazetteer("[\"Beirut\"]\nlat = 13.75\nlng = 100.5\nprecision = \"city\"\n");
        assert_ne!(assets_hash(&context, &before), assets_hash(&context, &after));
    }

    /// A `.moss/places.toml` edit that leaves every place in the same cell
    /// — here, changing only `precision`, which `relevant_tiles` never
    /// reads — leaves the directory exactly where it was.
    #[test]
    fn directory_name_is_unmoved_by_a_places_toml_edit_that_does_not_move_a_cell() {
        let context = place_map::PlaceMapContext::embedded().unwrap();
        let before = gazetteer("[\"Beirut\"]\nlat = 33.89\nlng = 35.5\nprecision = \"city\"\n");
        let after = gazetteer("[\"Beirut\"]\nlat = 33.89\nlng = 35.5\nprecision = \"country\"\n");
        assert_eq!(assets_hash(&context, &before), assets_hash(&context, &after));
    }

    /// The second call (warm transform cache) renders nothing; the first
    /// renders the world map plus the gazetteer's regional tiles.
    #[test]
    fn a_second_emit_reuses_the_cached_render() {
        let context = place_map::PlaceMapContext::embedded().unwrap();
        let gaz = gazetteer("[\"Beirut\"]\nlat = 33.89\nlng = 35.5\nprecision = \"city\"\n");
        let (_dir, paths) = scratch_paths("warm-cache");
        let output_dir = tempfile::tempdir().unwrap();
        let mut pending = PendingManifest::new(SiteHashes::default());

        let first = emit(&context, &gaz, &paths, output_dir.path(), &mut pending).unwrap();
        assert!(first > 1, "expected the world map plus at least one tile, got {first}");

        let mut pending2 = PendingManifest::new(SiteHashes::default());
        let second = emit(&context, &gaz, &paths, output_dir.path(), &mut pending2).unwrap();
        assert_eq!(second, 0, "a warm cache must render nothing");
    }

    /// Adding a gazetteer place in a new region moves the directory
    /// (`assets_hash` folds in the whole regional cell set), but must not
    /// evict the cache: the world map and every tile the first build
    /// already rendered are cached under their OWN identity
    /// (`Asset::cache_key`), not under `assets_hash`, so only the newly
    /// relevant cells actually render.
    #[test]
    fn adding_a_place_in_a_new_region_does_not_evict_unrelated_cache_entries() {
        let context = place_map::PlaceMapContext::embedded().unwrap();
        let before = gazetteer("[\"Beirut\"]\nlat = 33.89\nlng = 35.5\nprecision = \"city\"\n");
        let (_dir, paths) = scratch_paths("region-add");
        let output_dir = tempfile::tempdir().unwrap();
        let mut pending = PendingManifest::new(SiteHashes::default());

        let first = emit(&context, &before, &paths, output_dir.path(), &mut pending).unwrap();
        assert!(first > 1, "expected the world map plus at least one tile, got {first}");
        let before_tiles: std::collections::HashSet<(i16, i16)> =
            place_map::relevant_tiles(&before, context.pack()).into_iter().collect();

        // Bangkok: a different 10-degree cell entirely, so the gazetteer
        // edit genuinely moves the directory and adds cells the first
        // build never touched.
        let after = gazetteer(
            "[\"Beirut\"]\nlat = 33.89\nlng = 35.5\nprecision = \"city\"\n\
             [\"Bangkok\"]\nlat = 13.75\nlng = 100.5\nprecision = \"city\"\n",
        );
        assert_ne!(
            assets_hash(&context, &before),
            assets_hash(&context, &after),
            "the region add must move the served directory"
        );
        let after_tiles: std::collections::HashSet<(i16, i16)> =
            place_map::relevant_tiles(&after, context.pack()).into_iter().collect();
        let new_cells = after_tiles.difference(&before_tiles).count();
        assert!(new_cells > 0, "expected at least one new cell from the region add");

        let mut pending2 = PendingManifest::new(SiteHashes::default());
        let second = emit(&context, &after, &paths, output_dir.path(), &mut pending2).unwrap();
        assert_eq!(
            second, new_cells,
            "only the {new_cells} newly relevant cell(s) should render; the world map and the {} \
             previously-present tile(s) must come from the per-asset cache",
            before_tiles.len()
        );
    }

    /// A site with no gazetteer places still gets the world map and a
    /// `tiles.json`, just with an empty tile list — not zero assets.
    #[test]
    fn a_site_with_no_places_still_gets_the_world_map_and_an_empty_index() {
        let context = place_map::PlaceMapContext::embedded().unwrap();
        let gaz = empty_gazetteer();
        let (_dir, paths) = scratch_paths("no-places");
        let output_dir = tempfile::tempdir().unwrap();
        let mut pending = PendingManifest::new(SiteHashes::default());

        emit(&context, &gaz, &paths, output_dir.path(), &mut pending).unwrap();
        let dirs = emitted_map_dirs(output_dir.path());
        assert_eq!(dirs.len(), 1);
        let dir = output_dir.path().join("_moss").join(&dirs[0]);
        assert!(dir.join("world.svg").exists());
        #[derive(serde::Deserialize)]
        struct Owned {
            k: f64,
            bleed: f64,
            cells: Vec<(i16, i16)>,
        }
        let index: Owned = serde_json::from_slice(&std::fs::read(dir.join("tiles.json")).unwrap()).unwrap();
        assert!(index.cells.is_empty());
        assert_eq!(index.k, TILE_K);
        assert_eq!(index.bleed, TILE_BLEED);
        assert_eq!(std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).count(), 2, "world.svg + tiles.json only");
    }

    fn emitted_map_dirs(output_dir: &Path) -> Vec<String> {
        let moss_dir = output_dir.join("_moss");
        let mut names: Vec<String> = std::fs::read_dir(&moss_dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("map."))
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}
