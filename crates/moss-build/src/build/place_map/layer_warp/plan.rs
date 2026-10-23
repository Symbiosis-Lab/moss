//! What a site's layers are before any tile is drawn: the usable plates in
//! painting order, the extent their footprints give, the credit, the notice
//! text, and the key the tiles depend on.

use std::path::Path;

use super::plate::{load_plates, Plate};
use super::tiles::{inside, overlaps, NearPlace};
use crate::build::assets::paths::compute_binary_hash;
use crate::build::cli_output::log_note;
use crate::build::place_map::world_coords;
use crate::config::map_layers::{LayerKind, MapLayer};
use crate::vault::places::{Gazetteer, Precision};

/// Changes when the way tiles are drawn does, so tiles drawn the old way are not reused.
pub(crate) const WARP_VERSION: u32 = 2;
/// A plate whose control points disagree with its own fit by more than this is noted.
const RMS_NOTE_METRES: f64 = 30.0;

#[derive(Debug)]
pub struct LayerPlan {
    pub(crate) plates: Vec<Plate>,
    /// Hex digest of everything the layer's tiles depend on; empty for a layer without plates.
    pub(crate) digest: String,
    /// `[west, south, east, north]` in degrees.
    pub(crate) extent: [f64; 4],
    /// The configured credit, else the plates' own.
    pub(crate) credit: String,
    /// The layer's section of `NOTICE.txt`, empty without plates.
    pub(crate) notice: String,
    pub(crate) places: Vec<NearPlace>,
}

fn plate_rank(layer: &MapLayer, id: &str) -> usize {
    layer.annotations.iter().position(|url| url.trim_end_matches('/').rsplit('/').next() == Some(id)).unwrap_or(usize::MAX)
}

/// One plan per layer, in the layers' order. Logs the notes and warnings
/// about plates; does not draw anything.
pub fn plan_layers(layers: &[MapLayer], site_root: &Path, gazetteer: &Gazetteer) -> Vec<LayerPlan> {
    let spots: Vec<(&String, (f64, f64))> =
        gazetteer.iter().filter(|(_, r)| r.precision == Precision::Exact).filter_map(|(name, r)| Some((name, r.coords?))).collect();
    layers.iter().map(|layer| plan_layer(layer, site_root, &spots)).collect()
}

/// Notes on how the plates meet the places pages use (`used`: name and
/// `(lat, lng)`): each place no plate covers, and, per layer, each plate that
/// covers none. Returns the ids of the latter, per layer, for the tile cost.
pub fn note_coverage(plans: &[LayerPlan], used: &[(String, (f64, f64))]) -> Vec<Vec<String>> {
    let spots: Vec<(&String, f64, f64)> = used.iter().map(|(name, (lat, lng))| { let (x, y) = world_coords::project(*lat, *lng); (name, x, y) }).collect();
    if plans.iter().all(|p| p.plates.is_empty()) {
        return vec![Vec::new(); plans.len()];
    }
    for (name, x, y) in &spots {
        if !plans.iter().flat_map(|p| &p.plates).any(|plate| inside(&plate.footprint, *x, *y)) {
            log_note!("place '{name}' lies outside every historical map plate, so it is drawn on the moss landscape");
        }
    }
    plans
        .iter()
        .map(|plan| plan.plates.iter().filter(|plate| !spots.iter().any(|(_, x, y)| inside(&plate.footprint, *x, *y))).map(|p| p.id.clone()).collect())
        .collect()
}

fn plan_layer(layer: &MapLayer, site_root: &Path, spots: &[(&String, (f64, f64))]) -> LayerPlan {
    let bbox = layer.bbox.expect("resolve_layers gives every layer a bbox");
    let empty = LayerPlan {
        plates: Vec::new(),
        digest: String::new(),
        extent: bbox,
        credit: layer.credit.clone().unwrap_or_default(),
        notice: String::new(),
        places: Vec::new(),
    };
    if layer.kind != LayerKind::Historical {
        return empty;
    }
    let mut plates = load_plates(&site_root.join(&layer.source));
    if plates.is_empty() {
        log_note!("map layer {} has no plates in {}", layer.name, layer.source);
        return empty;
    }
    plates.sort_by_key(|p| plate_rank(layer, &p.id));
    plates.retain(|p| {
        if overlaps(p.footprint_degrees(), bbox) {
            true
        } else {
            log_note!("map plate {} lies outside the bbox of layer {}, so it is not drawn", p.id, layer.name);
            false
        }
    });
    for p in &plates {
        if p.rms_metres > RMS_NOTE_METRES {
            log_note!("map plate {} fits its control points to within {:.0} m (root mean square); street edges may be off by that much", p.id, p.rms_metres);
        }
    }
    if plates.is_empty() {
        return empty;
    }
    let mut extent = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for p in &plates {
        let f = p.footprint_degrees();
        extent = [extent[0].min(f[0]), extent[1].min(f[1]), extent[2].max(f[2]), extent[3].max(f[3])];
    }
    let extent = [extent[0].max(bbox[0]), extent[1].max(bbox[1]), extent[2].min(bbox[2]), extent[3].min(bbox[3])];

    let mut credits: Vec<String> = Vec::new();
    for text in plates.iter().map(|p| &p.credit).chain(plates.iter().filter_map(|p| p.title.as_ref())) {
        if !text.is_empty() && !credits.contains(text) {
            credits.push(text.clone());
        }
    }
    let credit = layer.credit.clone().unwrap_or_else(|| credits.join(" · "));
    let mut notice = format!("\nHistorical map layer \"{}\"\nCredit: {credit}\n", layer.name);
    for p in &plates {
        let rights = if p.rights.is_empty() { "not stated" } else { &p.rights };
        notice.push_str(&format!("Plate {}: {}\n  Rights: {rights}\n  Fit: {:.0} m root mean square\n", p.id, if p.source.is_empty() { "source not stated" } else { &p.source }, p.rms_metres));
        if let Some(by) = &p.georeferenced_by {
            notice.push_str(&format!("  Georeferenced by: {by}\n"));
        }
    }

    let places: Vec<NearPlace> = spots.iter().map(|(_, (lat, lng))| NearPlace::new(*lat, *lng)).collect();
    let mut key = format!("{WARP_VERSION}|{}|{}|{bbox:?}|", layer.name, layer.max_zoom);
    for p in &plates {
        key.push_str(&format!("{}:{}|", p.id, p.digest));
    }
    for (_, (lat, lng)) in spots {
        key.push_str(&format!("{lat:.6},{lng:.6}|"));
    }
    LayerPlan { digest: compute_binary_hash(key.as_bytes()), plates, extent, credit, notice, places }
}
