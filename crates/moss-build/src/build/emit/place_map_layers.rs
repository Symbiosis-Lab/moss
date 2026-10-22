//! `layers.json`: the detail layers a site declares in `[map.layers]`, in the
//! same `_moss/map.<hash>/` directory as the world map and `tiles.json`, for
//! the places explorer to read. Written only when at least one layer is
//! declared, so a site without layers is byte-for-byte what it was.
//!
//! The directory hash folds these bytes in ([`layers_json`] is called by
//! `place_map_assets::assets_hash_with_layers`), because the explorer caches
//! `layers.json` for good under that name.

use std::path::Path;

use crate::build::context::BuildContext;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::place_map::layer_warp::LayerPlan;
use crate::build::place_map::PlaceMapRenderContext;
use crate::build::served_path::ServedPath;
use crate::config::map_layers::MapLayer;

#[derive(serde::Serialize)]
struct LayersJson<'a> {
    version: u32,
    layers: Vec<LayerJson<'a>>,
}

#[derive(serde::Serialize)]
struct LayerJson<'a> {
    name: &'a str,
    kind: &'static str,
    /// `[west, south, east, north]` in degrees: where the layer has pixels.
    extent: [f64; 4],
    /// `[west, south, east, north]` in degrees: the collection's frame, where the
    /// explorer may zoom as deep as `max_zoom` whether or not a plate covers the spot.
    frame: [f64; 4],
    max_zoom: u8,
    credit: &'a str,
    /// Where the drawn raster tiles are indexed; `null` until a layer is drawn.
    tiles: Option<&'a str>,
}

/// The bytes of `layers.json` as the config alone gives them, or `None` when
/// there are no layers. The map directory's hash folds these in.
pub(crate) fn layers_json(layers: &[MapLayer]) -> Option<Vec<u8>> {
    layers_json_with(layers, &[], &[])
}

/// `layers.json` with each layer's drawn extent, credit and tile index: `plans`
/// and `tiles` are indexed like `layers`; a missing entry leaves the config's
/// own value.
fn layers_json_with(layers: &[MapLayer], plans: &[LayerPlan], tiles: &[Option<String>]) -> Option<Vec<u8>> {
    if layers.is_empty() {
        return None;
    }
    let body = LayersJson {
        version: 1,
        layers: layers
            .iter()
            .enumerate()
            .map(|(n, layer)| {
                let drawn = tiles.get(n).and_then(|t| t.as_deref());
                let plan = plans.get(n).filter(|_| drawn.is_some());
                let frame = layer.bbox.expect("resolve_layers gives every layer a bbox");
                LayerJson {
                    name: &layer.name,
                    kind: layer.kind.as_str(),
                    extent: plan.map_or(frame, |p| p.extent),
                    frame,
                    max_zoom: layer.max_zoom,
                    credit: plan.map_or_else(|| layer.credit.as_deref().unwrap_or(""), |p| p.credit.as_str()),
                    tiles: drawn,
                }
            })
            .collect(),
    };
    Some(serde_json::to_vec(&body).expect("layer fields serialize"))
}

/// The pipeline's one call: write `layers.json` beside the map assets when the
/// place-map context carries layers. Errors are logged, never fatal, like the
/// other map assets.
/// `used` is the places pages name, with their coordinates.
pub fn emit_if_place_typed(render_context: Option<&PlaceMapRenderContext>, used: &[(String, (f64, f64))], output_dir: &Path, pending: &mut PendingManifest) {
    let Some(render_context) = render_context else { return };
    let layers = render_context.layers();
    let plans = render_context.layer_plans();
    let hash = render_context.map_assets_hash();
    let uncovered = crate::build::place_map::layer_warp::note_coverage(plans, used);
    let tiles: Vec<Option<String>> = layers
        .iter()
        .zip(plans)
        .zip(&uncovered)
        .map(|((layer, plan), uncovered)| super::place_map_layer_tiles::emit_layer(layer, plan, uncovered, hash, output_dir, pending))
        .collect();
    let Some(json) = layers_json_with(layers, plans, &tiles) else { return };
    let result = ServedPath::for_place_map_asset(hash, "layers.json")
        .map_err(|e| format!("Invalid layers path: {e}"))
        .and_then(|served| {
            BuildContext::for_render(output_dir, pending)
                .emit(&served, &json, HashBucket::Files)
                .map_err(|e| format!("Failed to emit layers.json: {e}"))
        });
    if let Err(error) = result {
        crate::build::cli_output::log_warn_problem!("map layers could not be emitted: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::map_layers::LayerKind;

    fn layer(credit: Option<&str>) -> MapLayer {
        MapLayer {
            name: "atlas-1930".into(),
            kind: LayerKind::Historical,
            source: "maps/atlas-1930".into(),
            bbox: Some([-74.03, 40.68, -73.9, 40.88]),
            max_zoom: 17,
            budget: Some(40_000_000),
            credit: credit.map(str::to_string),
            annotations: Vec::new(),
        }
    }

    #[test]
    fn no_layers_means_no_file() {
        assert!(layers_json(&[]).is_none());
    }

    #[test]
    fn the_file_has_the_shape_the_explorer_reads() {
        let json = String::from_utf8(layers_json(&[layer(Some("Atlas of the City, 1930"))]).unwrap()).unwrap();
        assert_eq!(
            json,
            r#"{"version":1,"layers":[{"name":"atlas-1930","kind":"historical","extent":[-74.03,40.68,-73.9,40.88],"frame":[-74.03,40.68,-73.9,40.88],"max_zoom":17,"credit":"Atlas of the City, 1930","tiles":null}]}"#
        );
    }

    #[test]
    fn a_missing_credit_is_the_empty_string() {
        let json = String::from_utf8(layers_json(&[layer(None)]).unwrap()).unwrap();
        assert!(json.contains(r#""credit":"","#), "{json}");
    }
}
