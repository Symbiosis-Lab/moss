//! The raster tiles of a historical layer, in the `_moss/map.<hash>/layers/<name>/`
//! folder beside `layers.json`, and the `index.json` the explorer reads to
//! learn which tiles exist and where the plates are.
//!
//! The map directory's hash already folds in the layer's key (its plates'
//! content, its config, the places the deep levels are drawn around), so a
//! tile found there from an earlier build was drawn from the same inputs and
//! is registered as it is instead of drawn again.

use std::collections::BTreeMap;
use std::path::Path;

use rayon::prelude::*;

use crate::build::cli_output::{log_note, log_warn_problem};
use crate::build::context::BuildContext;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::place_map::layer_crop::{crop_axes, CropJob};
use crate::build::place_map::layer_warp::tiles::{candidates, encode, level_range, near_places_only, overlaps, render_crop, render_tile, tile_rect, Loaded, TILE_SIZE};
use crate::build::place_map::layer_warp::{LayerPlan, Plate};
use crate::build::place_map::world_coords::written;
use crate::build::served_path::ServedPath;
use crate::config::map_layers::MapLayer;

#[derive(serde::Serialize, serde::Deserialize)]
struct Footprint {
    plate: String,
    polygon: Vec<[f64; 2]>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct TileIndex {
    min_level: u8,
    max_level: u8,
    tile_size: u32,
    levels: BTreeMap<String, Vec<[u32; 2]>>,
    footprints: Vec<Footprint>,
}

/// Where the layer's index is, relative to the map directory.
pub(crate) fn index_url(layer: &MapLayer) -> String {
    format!("layers/{}/index.json", layer.name)
}

fn tile_path(hash: &str, layer: &MapLayer, level: u8, i: u32, j: u32) -> Result<ServedPath, String> {
    ServedPath::for_place_map_layer_file(hash, &layer.name, &format!("{level}/{i}/{j}.jpg")).map_err(|e| e.to_string())
}

fn size_text(bytes: u64) -> String {
    if bytes >= 1_000_000 { format!("{:.1} MB", bytes as f64 / 1e6) } else { format!("{:.0} KB", bytes as f64 / 1e3) }
}

struct TileSize {
    level: u8,
    i: u32,
    j: u32,
    bytes: u64,
}

/// Bytes of the tiles that only plate `id` reaches: what dropping it would save.
fn sole_cost(plan: &LayerPlan, id: &str, sizes: &[TileSize]) -> u64 {
    let reaches = |plate: &Plate, tile: &TileSize| overlaps(plate.footprint_world(), tile_rect(tile.level, tile.i, tile.j));
    let Some(plate) = plan.plates.iter().find(|p| p.id == id) else { return 0 };
    sizes.iter().filter(|t| reaches(plate, t) && !plan.plates.iter().any(|o| o.id != id && reaches(o, t))).map(|t| t.bytes).sum()
}

/// Writes the layer's tiles and index, and the `crops` figures asked for (by
/// file name); the URL of the index when the layer has any tile. Over budget
/// is a problem (a failure under `--strict`), the crops counting toward it.
/// `uncovered` names the plates that cover none of the site's places.
pub(crate) fn emit_layer(layer: &MapLayer, plan: &LayerPlan, uncovered: &[String], crops: &[(&String, &CropJob)], hash: &str, output_dir: &Path, pending: &mut PendingManifest) -> Option<String> {
    if plan.plates.is_empty() {
        return None;
    }
    match emit_tiles(layer, plan, hash, output_dir, pending) {
        Ok(sizes) => {
            let total: u64 = sizes.iter().map(|t| t.bytes).sum::<u64>() + emit_crops(layer, plan, crops, hash, output_dir, pending);
            for id in uncovered {
                log_note!("plate {id} covers none of the site's places; it is kept, and its tiles cost {}", size_text(sole_cost(plan, id, &sizes)));
            }
            if let Some(budget) = layer.budget.filter(|b| total > *b) {
                log_warn_problem!("map layer {} draws {} of tiles, over its budget of {}; lower max_zoom, narrow the bbox, or raise the budget", layer.name, size_text(total), size_text(budget));
            }
            Some(index_url(layer))
        }
        Err(error) => {
            log_warn_problem!("map layer {} could not be drawn: {error}", layer.name);
            None
        }
    }
}

/// Writes each picture of `crops` (file name and job) not already in the
/// output from the earlier build; the bytes of all of them. A picture drawn
/// from the same plates over the same frame has the same name, so it is reused.
fn emit_crops(layer: &MapLayer, plan: &LayerPlan, crops: &[(&String, &CropJob)], hash: &str, output_dir: &Path, pending: &mut PendingManifest) -> u64 {
    let mut loaded: Option<Vec<Loaded>> = None;
    let mut total = 0;
    for (name, job) in crops {
        let Ok(path) = ServedPath::for_place_map_layer_file(hash, &layer.name, &format!("crops/{name}.jpg")) else { continue };
        let bytes = match std::fs::read(output_dir.join(path.as_str())) {
            Ok(bytes) => {
                pending.register(&path, &bytes, HashBucket::Files);
                bytes
            }
            Err(_) => {
                let plates = loaded.get_or_insert_with(|| plan.plates.iter().filter_map(|p| Loaded::open(p).map_err(|why| log_warn_problem!("map plate {} skipped: {why}", p.id)).ok()).collect());
                let Some(picture) = crop_axes(job).and_then(|(longitudes, latitudes)| render_crop(plates, &longitudes, &latitudes)) else {
                    log_warn_problem!("map layer {} could not be drawn into a figure: no plate has pixels in its frame", layer.name);
                    continue;
                };
                let bytes = encode(&picture);
                if let Err(error) = BuildContext::for_render(output_dir, pending).emit(&path, &bytes, HashBucket::Files) {
                    log_warn_problem!("map layer {} figure {name}: {error}", layer.name);
                    continue;
                }
                bytes
            }
        };
        total += bytes.len() as u64;
    }
    total
}

fn emit_tiles(layer: &MapLayer, plan: &LayerPlan, hash: &str, output_dir: &Path, pending: &mut PendingManifest) -> Result<Vec<TileSize>, String> {
    let index_path = ServedPath::for_place_map_layer_file(hash, &layer.name, "index.json").map_err(|e| e.to_string())?;
    let (min, max) = level_range(plan.extent, layer.max_zoom);

    let earlier: Option<TileIndex> = std::fs::read(output_dir.join(index_path.as_str())).ok().and_then(|b| serde_json::from_slice(&b).ok());
    let mut drawn: BTreeMap<u8, Vec<(u32, u32)>> = BTreeMap::new();
    let mut sizes: Vec<TileSize> = Vec::new();
    let reused = earlier.is_some_and(|index| {
        let mut staged = Vec::new();
        for (level, tiles) in &index.levels {
            let Ok(level) = level.parse::<u8>() else { return false };
            for &[i, j] in tiles {
                let Ok(path) = tile_path(hash, layer, level, i, j) else { return false };
                let Ok(bytes) = std::fs::read(output_dir.join(path.as_str())) else { return false };
                sizes.push(TileSize { level, i, j, bytes: bytes.len() as u64 });
                staged.push((path, bytes));
                drawn.entry(level).or_default().push((i, j));
            }
        }
        for (path, bytes) in staged {
            pending.register(&path, &bytes, HashBucket::Files);
        }
        true
    });
    if !reused {
        drawn.clear();
        sizes.clear();
        let loaded: Vec<Loaded> = plan
            .plates
            .iter()
            .filter_map(|p| match Loaded::open(p) {
                Ok(l) => Some(l),
                Err(why) => {
                    log_warn_problem!("map plate {} skipped: {why}", p.id);
                    None
                }
            })
            .collect();
        for level in min..=max {
            let restrict = near_places_only(level, min, max);
            let wanted = candidates(level, plan.extent, restrict, &plan.places);
            let tiles: Vec<(u32, u32, Vec<u8>)> =
                wanted.par_iter().filter_map(|&(i, j)| render_tile(&loaded, level, i, j).map(|t| (i, j, encode(&t)))).collect();
            for (i, j, bytes) in tiles {
                let path = tile_path(hash, layer, level, i, j)?;
                BuildContext::for_render(output_dir, pending).emit(&path, &bytes, HashBucket::Files).map_err(|e| format!("tile {level}/{i}/{j}: {e}"))?;
                sizes.push(TileSize { level, i, j, bytes: bytes.len() as u64 });
                drawn.entry(level).or_default().push((i, j));
            }
        }
    }

    let index = TileIndex {
        min_level: min,
        max_level: max,
        tile_size: TILE_SIZE,
        levels: drawn.into_iter().map(|(l, mut t)| {
            t.sort_unstable();
            (l.to_string(), t.into_iter().map(|(i, j)| [i, j]).collect())
        }).collect(),
        footprints: plan.plates.iter().map(|p| Footprint { plate: p.id.clone(), polygon: p.footprint.iter().map(|&(x, y)| [written(x), written(y)]).collect() }).collect(),
    };
    let json = serde_json::to_vec(&index).expect("a tile index serializes");
    BuildContext::for_render(output_dir, pending).emit(&index_path, &json, HashBucket::Files).map_err(|e| format!("index: {e}"))?;
    Ok(sizes)
}
