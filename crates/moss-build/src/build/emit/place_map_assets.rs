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
//! `NOTICE.txt` beside them carries the map data's acknowledgements
//! ([`NOTICE`]); a site that emits map assets always emits it.
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
use crate::build::cache::{ObjectStore, TransformCache, TransformEntry};
use crate::build::context::BuildContext;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::place_map;
use crate::build::place_map::layer_warp::LayerPlan;
use crate::build::place_map::{TILE_COLUMNS, TILE_K, TILE_ROWS};
use crate::build::served_path::ServedPath;
use crate::config::map_layers::MapLayer;
use crate::moss_paths::MossPaths;
use crate::vault::places::Gazetteer;

/// The map data's acknowledgements, shipped as `NOTICE.txt` in every
/// `_moss/map.<hash>/` directory. The GEBCO grid is public domain on the
/// condition that its source is acknowledged, so the text is the same file
/// the pack's source manifest ships beside it.
const NOTICE: &str = include_str!("../../../data/place-map/NOTICE");

/// `tiles.json`'s own shape: `k` and the grid's `columns`/`rows` alongside
/// the emitted cells, so the runtime never has to hardcode or re-derive the
/// factor a tile was rendered at or the grid it was cut from (see this
/// module's own doc).
/// `origins` maps `"x,y"` to the tile's canvas top-left in whole canvas
/// units (`1 / k` world units): every tile rounds its vertices to integers
/// from an origin on that one lattice, so the runtime must place it exactly
/// there.
/// `fit` and `edgeFade` are the fit and outer-edge fade the opening view's
/// cells were chosen for (`place_map::relevant_tiles`).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TileIndex<'a> {
    k: f64,
    fit: place_map::ExplorerFit,
    edge_fade: place_map::EdgeFade,
    columns: u32,
    rows: u32,
    cells: &'a [(i16, i16)],
    origins: std::collections::BTreeMap<String, (i64, i64)>,
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
/// 4: `TileSelection::for_frame`'s own `FrameTier::Tile` case now draws a
/// regional tile from the ONE pack bucket matching its own cell, instead of
/// every bucket a wider margin reached into — a fine feature registered in
/// several buckets at pack-build time was drawing two independently
/// simplified copies of itself across the shared edge, visible as a seam —
/// and `TILE_BLEED` widened from 0.1 to 0.2 world units to also close the
/// sub-pixel disagreement that same seam was partly made of. Neither change
/// touches the compiled-in pack bytes, so a pre-existing site's cached
/// tiles would otherwise keep serving the seam forever.
/// 5: a tile's canvas now starts on a whole canvas unit of the world's
/// lattice and is a whole number of units wide and tall, and `tiles.json`
/// carries each tile's origin; tiles cached at the old fractional origin
/// would be placed a fraction of a unit off.
/// 6: regional tiles now carry the world's relief lighting and band shadows,
/// with filter bounds derived from each tile's own height; cached tiles from
/// the former flat-terrain output must not survive the upgrade.
pub const GENERATOR_VERSION: u32 = 6;

#[cfg(test)]
mod generator_version_guard {
    use super::*;

    /// Pins the emitter's output against `GENERATOR_VERSION`. A site that
    /// rebuilds keeps every cached asset whose key (which folds the version
    /// in) is unchanged, so output that changes under an unchanged version
    /// leaves old tiles in place beneath the new manifest.
    #[test]
    fn the_generator_version_must_change_when_the_emitters_output_does() {
        let context = place_map::PlaceMapContext::embedded().unwrap();
        let tile = compute_binary_hash(place_map::emit_tile_svg(&context, 20, 12).as_bytes());
        let world = compute_binary_hash(place_map::emit_world_svg(&context).as_bytes());
        assert_eq!(
            (GENERATOR_VERSION, tile.as_str(), world.as_str()),
            (6, "8a942696130d9d71", "6b198ee3228d905f"),
            "the emitter's output changed: bump GENERATOR_VERSION before updating the pinned digests, \
             because every site that rebuilds would otherwise keep its old tiles under the new manifest"
        );
    }
}

/// The hash naming this build's `_moss/map.<hash>/` directory: the pack's
/// own fingerprint (its source manifest digest, from the embedded
/// `MOSSPLM1` header — changes only when the compiled-in pack data
/// changes), [`GENERATOR_VERSION`], and the bytes of `tiles.json`, which
/// list the regional cells `place_map::relevant_tiles` derives from
/// `gazetteer` (sorted, so iteration order never matters). The explorer
/// caches `tiles.json` for good under this name, so any change to it must
/// move the directory. Never a function of site config beyond
/// the gazetteer, or of page content at all, so a page edit never moves
/// this directory and a `.moss/places.toml` edit moves it only when it
/// actually changes which cells matter — see
/// `directory_name_is_unmoved_by_an_unrelated_page_edit` and
/// `directory_name_moves_when_a_places_toml_edit_moves_a_place_to_a_new_cell`
/// below.
pub fn assets_hash(context: &place_map::PlaceMapContext, gazetteer: &Gazetteer) -> String {
    assets_hash_with_layers(context, gazetteer, &[], &[])
}

/// [`assets_hash`] for a site that declares detail layers: the bytes of
/// `layers.json` join the hash, so editing a layer moves the directory. With
/// no layers the hash is exactly `assets_hash`'s.
pub fn assets_hash_with_layers(context: &place_map::PlaceMapContext, gazetteer: &Gazetteer, layers: &[MapLayer], plans: &[LayerPlan]) -> String {
    let mut buf = Vec::new();
    buf.extend_from_slice(&context.pack_fingerprint());
    buf.extend_from_slice(&GENERATOR_VERSION.to_le_bytes());
    // `labels.json` (`emit::place_map_labels`) lands in this SAME
    // directory, so its own change fingerprint belongs in this SAME hash —
    // see `PlaceMapContext::labels_bytes`'s own doc for why raw label
    // bytes, not the pack fingerprint above, are what catches a
    // generator-only change.
    buf.extend_from_slice(context.labels_bytes());
    buf.extend_from_slice(&tile_index_json(&place_map::relevant_tiles(gazetteer, context.pack())));
    if let Some(json) = super::place_map_layers::layers_json(layers) {
        buf.extend_from_slice(&json);
    }
    // What the layers' tiles are drawn from: a plate or place edit moves the directory.
    for plan in plans {
        buf.extend_from_slice(plan.digest.as_bytes());
    }
    compute_binary_hash(&buf)
}

/// `tiles.json` for `cells`.
fn tile_index_json(cells: &[(i16, i16)]) -> Vec<u8> {
    serde_json::to_vec(&TileIndex {
        k: TILE_K,
        fit: place_map::EXPLORER_FIT,
        edge_fade: place_map::EDGE_FADE,
        columns: TILE_COLUMNS,
        rows: TILE_ROWS,
        cells,
        origins: cells.iter().map(|&(x, y)| (format!("{x},{y}"), place_map::tile_origin_units(x, y))).collect(),
    })
    .expect("a tile index of integers and finite numbers serializes")
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
            if let Err(error) = emit_with_layers(&context, render_context.gazetteer(), render_context.layers(), render_context.layer_plans(), paths, output_dir, pending) {
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
    emit_with_layers(context, gazetteer, &[], &[], paths, output_dir, pending)
}

/// [`emit`] into the directory a site with `layers` is named after.
pub fn emit_with_layers(
    context: &place_map::PlaceMapContext,
    gazetteer: &Gazetteer,
    layers: &[MapLayer],
    plans: &[LayerPlan],
    paths: &MossPaths,
    output_dir: &Path,
    pending: &mut PendingManifest,
) -> Result<usize, String> {
    let objects = ObjectStore::for_site(paths);
    let transforms = TransformCache::for_site(paths);
    let hash = assets_hash_with_layers(context, gazetteer, layers, plans);

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
        let cached_entry = transforms.get_with(&cache_key, crate::build::cache::RecordMode::Request).and_then(|record| record.transforms.get(&name).cloned());

        // A cached entry carries its own xxHash3 manifest digest (stashed in
        // `params` at render time below), so a hit needs neither a re-read
        // of the blob nor a re-hash of its bytes to register — see the
        // module doc.
        let cached_xxh3 = cached_entry.as_ref().and_then(|entry| {
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
            let oid = cached_entry.as_ref().map(|entry| entry.oid.clone())?;
            objects.ready_blob(&oid).is_some().then_some((oid, xxh3))
        });

        if let Some((oid, xxh3)) = cached {
            match objects.link_to(&oid, &target) {
                Ok(()) => {
                    pending.register_hashed(&served, &xxh3, HashBucket::Files);
                    continue;
                }
                Err(error) => log::debug!("place-map asset '{name}' cache unavailable: {error}"),
            }
        }

        let svg = asset.render(context);
        let bytes = svg.as_bytes();
        let xxh3 = compute_binary_hash(bytes);
        BuildContext::for_render(output_dir, pending)
            .emit(&served, bytes, HashBucket::Files)
            .map_err(|e| format!("Failed to emit place-map asset '{name}': {e}"))?;
        // Cache persistence may request a cloud-only folder and refuse this
        // attempt. The rendered bytes still belong to this complete build.
        let cached = objects.store_bytes(bytes, crate::build::cache::RecordMode::Request)
            .and_then(|oid| {
                transforms.merge(&cache_key, 0, crate::build::cache::RecordMode::Request, |record| {
                    record.transforms.insert(
                        name.clone(),
                        TransformEntry {
                            oid,
                            size: bytes.len() as u64,
                            params: serde_json::json!({ "generator_version": GENERATOR_VERSION, "xxh3": xxh3 }),
                        },
                    );
                })
            });
        if let Err(error) = cached {
            log::debug!("place-map asset '{name}' cache not saved: {error}");
        }
        rendered += 1;
    }

    // The index is cheap to (re-)compute — a few hundred small integers at
    // most — so it is simply written every build rather than threaded
    // through the transform cache like the SVGs: nothing here is worth
    // caching on its own.
    let index_sp = ServedPath::for_place_map_asset(&hash, "tiles.json")
        .map_err(|e| format!("Invalid place-map tile index path: {e}"))?;
    let index_json = tile_index_json(&regional_tiles);
    BuildContext::for_render(output_dir, pending)
        .emit(&index_sp, &index_json, HashBucket::Files)
        .map_err(|e| format!("Failed to emit place-map tile index: {e}"))?;

    let notice_sp = ServedPath::for_place_map_asset(&hash, "NOTICE.txt")
        .map_err(|e| format!("Invalid place-map notice path: {e}"))?;
    let mut notice = NOTICE.to_string();
    for plan in plans {
        notice.push_str(&plan.notice);
    }
    BuildContext::for_render(output_dir, pending)
        .emit(&notice_sp, notice.as_bytes(), HashBucket::Files)
        .map_err(|e| format!("Failed to emit place-map notice: {e}"))?;

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

    /// `assets_hash` (above) only renames the served directory; the per-asset
    /// `cache_key` is the OTHER thing a generator change must move, since
    /// that is what `emit`'s own `transforms.get(&cache_key)` looks a tile up
    /// under (see this module's own doc). A generator-version bump that
    /// moved one but not the other would still serve a stale render: the new
    /// directory would exist, but an asset whose bytes changed only because
    /// the DRAWING logic changed (not the pack fingerprint) would hit a
    /// cache entry carrying the OLD bytes under an unmoved key.
    #[test]
    fn cache_key_changes_with_generator_version() {
        let context = place_map::PlaceMapContext::embedded().unwrap();
        let first = Asset::World.cache_key(&context);
        // Same simulation the hash test above uses: the real constant cannot
        // change at test time, so hash with a different version directly,
        // matching `cache_key`'s own byte layout.
        let mut buf = Vec::new();
        buf.extend_from_slice(&context.pack_fingerprint());
        buf.extend_from_slice(&(GENERATOR_VERSION + 1).to_le_bytes());
        buf.push(0);
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

    /// The explorer fetches `tiles.json` with `force-cache`, so a change to
    /// any of its fields, not only to its cells, must move the directory:
    /// otherwise a returning reader pairs a cached index with a new explorer.
    #[test]
    fn directory_name_covers_every_byte_of_tiles_json() {
        let context = place_map::PlaceMapContext::embedded().unwrap();
        let gaz = gazetteer("[\"Port\"]\nlat = 33.89\nlng = 35.5\nprecision = \"city\"\n");
        let (_dir, paths) = scratch_paths("index-bytes");
        let output_dir = tempfile::tempdir().unwrap();
        let mut pending = PendingManifest::new(SiteHashes::default());
        emit(&context, &gaz, &paths, output_dir.path(), &mut pending).unwrap();
        let dir = output_dir.path().join("_moss").join(format!("map.{}", assets_hash(&context, &gaz)));
        let index = std::fs::read(dir.join("tiles.json")).unwrap();
        let mut expected = Vec::new();
        expected.extend_from_slice(&context.pack_fingerprint());
        expected.extend_from_slice(&GENERATOR_VERSION.to_le_bytes());
        expected.extend_from_slice(context.labels_bytes());
        expected.extend_from_slice(&index);
        assert_eq!(assets_hash(&context, &gaz), compute_binary_hash(&expected));
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
            columns: u32,
            rows: u32,
            cells: Vec<(i16, i16)>,
        }
        let index: Owned = serde_json::from_slice(&std::fs::read(dir.join("tiles.json")).unwrap()).unwrap();
        assert!(index.cells.is_empty());
        assert_eq!(index.k, TILE_K);
        assert_eq!((index.columns, index.rows), (36, 18));
        assert_eq!(std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).count(), 3, "world.svg + tiles.json + NOTICE.txt only");
    }

    /// GEBCO's terms require the grid's source to be acknowledged, so every
    /// site that ships map assets ships the acknowledgement with them.
    #[test]
    fn map_assets_ship_with_the_data_acknowledgements() {
        let context = place_map::PlaceMapContext::embedded().unwrap();
        let (_dir, paths) = scratch_paths("notice");
        let output_dir = tempfile::tempdir().unwrap();
        let mut pending = PendingManifest::new(SiteHashes::default());

        emit(&context, &empty_gazetteer(), &paths, output_dir.path(), &mut pending).unwrap();
        let dirs = emitted_map_dirs(output_dir.path());
        let notice = std::fs::read_to_string(output_dir.path().join("_moss").join(&dirs[0]).join("NOTICE.txt")).unwrap();
        for needed in [
            "Natural Earth",
            "GEBCO Compilation Group (2025) GEBCO 2025 Grid (doi:10.5285/37c52e96-24ea-67ce-e063-7086abc05f29)",
            "placed in the public domain",
            "not endorsed by GEBCO",
            "NOT be used for navigation",
        ] {
            assert!(notice.contains(needed), "NOTICE.txt lacks {needed:?}");
        }
    }

    /// Waits until no owned cache publication holds the cache, as the next build would find it.
    #[cfg(target_os = "macos")]
    fn wait_for_cache_publications(paths: &MossPaths, what: &str) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while crate::build::lifecycle::try_begin_cache_gc(paths).is_err() {
            assert!(std::time::Instant::now() < deadline, "{what} did not settle");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cloud_only_cache_folders_do_not_remove_maps_from_the_build() {
        use crate::build::icloud::pretend;
        use sha2::{Digest, Sha256};
        let context = place_map::PlaceMapContext::embedded().unwrap();
        let gaz = gazetteer("[\"Port\"]\nlat = 33.89\nlng = 35.5\nprecision = \"city\"\n");
        let cells = place_map::relevant_tiles(&gaz, context.pack());
        assert!(!cells.is_empty());
        let (x, y) = cells[0];
        let tile = Asset::Tile(x, y).render(&context);
        let svg = Asset::World.render(&context);
        let oid = format!("{:x}", Sha256::digest(tile.as_bytes()));
        for object_folder in [true, false] {
            let (_dir, paths) = scratch_paths("cloud-map-cache");
            let objects = ObjectStore::for_site(&paths);
            let transforms = TransformCache::for_site(&paths);
            // The scratch root represents a managed provider, including a
            // refusal below its object shard rather than at the store root.
            let _provider_context = crate::build::cloud_readiness::storage::TestFault::install(
                objects.root(), objects.root(), crate::build::cloud_readiness::StorageOperation::CacheMkdir,
                0, std::time::Duration::from_millis(20),
            );
            let folder = if object_folder {
                objects.blob_path(&oid).parent().unwrap().to_path_buf()
            } else {
                transforms.root().to_path_buf()
            };
            std::fs::create_dir_all(&folder).unwrap();
            let sibling = folder.join("unrelated");
            std::fs::write(&sibling, b"preserve shared data").unwrap();
            let cloud = pretend::evicted_until_requested(&folder);
            let output = tempfile::tempdir().unwrap();
            let mut pending = PendingManifest::new(SiteHashes::default());
            assert_eq!(emit(&context, &gaz, &paths, output.path(), &mut pending).unwrap(), cells.len() + 1);
            let manifest = pending.seal();
            let map_dir = output.path().join("_moss").join(format!("map.{}", assets_hash(&context, &gaz)));
            assert_eq!(std::fs::read(map_dir.join("world.svg")).unwrap(), svg.as_bytes());
            let index: serde_json::Value = serde_json::from_slice(&std::fs::read(map_dir.join("tiles.json")).unwrap()).unwrap();
            assert_eq!(index["cells"], serde_json::json!(cells));
            for (x, y) in &cells {
                assert!(map_dir.join(format!("tile-{x}-{y}.svg")).is_file());
            }
            assert_eq!(manifest.site_hashes_view().files.len(), cells.len() + 3, "both outputs registered");
            assert_eq!(pretend::requests_for(&folder), 0, "directory writes never request a file-byte download");
            assert_eq!(std::fs::read(&sibling).unwrap(), b"preserve shared data");
            if object_folder {
                assert_eq!(std::fs::read(ObjectStore::new(paths.cache_local_objects()).blob_path(&oid)).unwrap(), tile.as_bytes(), "the complete local CAS supplies the emitted map while the shared publication waits");
            }
            assert!(crate::build::lifecycle::try_begin_cache_gc(&paths).is_err(), "the admitted owned publication retains its local source through completion");
            drop(cloud);
            wait_for_cache_publications(&paths, "owned map cache publication after the actual write became available");
            assert_eq!(std::fs::read(objects.blob_path(&oid)).unwrap(), tile.as_bytes(), "the shared object is available only after verified owned publication");
            let record = transforms.get_with(&Asset::Tile(x, y).cache_key(&context), crate::build::cache::RecordMode::Request).expect("owned record publication settles without another build");
            assert_eq!(record.transforms[&Asset::Tile(x, y).name()].oid, oid);
            // Record publications past the background queue's bound are skipped, not failed: the next build renders those tiles again and caches them.
            let mut rendered = Vec::new();
            for _ in 0..3 {
                let next = tempfile::tempdir().unwrap();
                let mut pending = PendingManifest::new(SiteHashes::default());
                rendered.push(emit(&context, &gaz, &paths, next.path(), &mut pending).unwrap());
                assert_eq!(pending.seal().site_hashes_view().files.len(), cells.len() + 3);
                wait_for_cache_publications(&paths, "the rebuild's cache publication");
            }
            assert!(rendered[0] < cells.len(), "the settled records spare the first rebuild: {rendered:?}");
            assert_eq!(rendered[1..], [0, 0], "{rendered:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_cached_map_is_rendered_into_the_complete_build() {
        use std::os::unix::fs::PermissionsExt;
        use sha2::{Digest, Sha256};
        let context = place_map::PlaceMapContext::embedded().unwrap();
        let gaz = empty_gazetteer();
        let (_dir, paths) = scratch_paths("unreadable-map-cache");
        let initial = tempfile::tempdir().unwrap();
        let mut pending = PendingManifest::new(SiteHashes::default());
        emit(&context, &gaz, &paths, initial.path(), &mut pending).unwrap();
        let svg = Asset::World.render(&context);
        let oid = format!("{:x}", Sha256::digest(svg.as_bytes()));
        let blob = ObjectStore::for_site(&paths).blob_path(&oid);
        struct Restore(std::path::PathBuf);
        impl Drop for Restore {
            fn drop(&mut self) {
                let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o644));
            }
        }
        let _restore = Restore(blob.clone());
        std::fs::set_permissions(&blob, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read(&blob).is_ok() { return; } // Root ignores file permissions.
        let output = tempfile::tempdir().unwrap();
        let mut pending = PendingManifest::new(SiteHashes::default());
        assert_eq!(emit(&context, &gaz, &paths, output.path(), &mut pending).unwrap(), 1);
        let map_dir = output.path().join("_moss").join(format!("map.{}", assets_hash(&context, &gaz)));
        assert_eq!(std::fs::read(map_dir.join("world.svg")).unwrap(), svg.as_bytes());
        assert!(map_dir.join("tiles.json").is_file());
        assert!(map_dir.join("NOTICE.txt").is_file());
        assert_eq!(pending.seal().site_hashes_view().files.len(), 3);
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
