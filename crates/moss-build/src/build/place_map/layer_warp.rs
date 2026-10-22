//! Historical map layers: georeferenced plate scans warped into the places
//! explorer's world coordinates, as raster tiles. Reading and fitting a plate
//! is [`plate`]; the polynomial algebra is [`fit`]; deciding which tiles
//! exist and drawing one is [`tiles`]; [`plan`] ties a layer's plates to its
//! extent, credit and cache key. Writing the tiles is `emit::place_map_layer_tiles`.

mod fit;
mod plan;
mod plate;
pub(crate) mod tiles;

pub use plan::{plan_layers, LayerPlan};

#[cfg(test)]
mod warp_tests;
