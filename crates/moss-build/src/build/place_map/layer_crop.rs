//! A figure whose places all lie on a historical layer is framed to them, and
//! draws that layer over a flat ground: one picture of the layer per distinct
//! frame, warped by the layer's own warper when the build emits the map
//! directory, not while the page renders.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use super::geometry::{VIEWBOX_HEIGHT, VIEWBOX_WIDTH};
use super::layer_warp::tiles::{inside, overlaps};
use super::layer_warp::{LayerPlan, WARP_VERSION};
use super::{world_coords, Frame, PlaceMapRenderContext, PlaceMapTarget, ProjectedPoint, Projection};
use crate::build::assets::paths::compute_binary_hash;
use crate::build::served_path::ServedPath;
use crate::config::map_layers::MapLayer;

/// Pixels of a picture per CSS pixel the figure is shown at.
const DENSITY: f64 = 2.0;
/// The world map's data stays clean down to this many degrees of longitude
/// per CSS pixel and fades out over half again as much depth: the explorer's
/// `WORLD_CLEAN_DEGREES_PER_PX` and `WORLD_DETAIL_FADE_BAND`. A figure shown
/// deeper than the end of that fade draws a flat ground instead.
const WORLD_CLEAN_DEGREES_PER_PX: f64 = 0.26;
const WORLD_DETAIL_FADE_BAND: f64 = 0.5;

/// One picture a figure needs: the layer's plates over the frame.
#[derive(Debug, Clone)]
pub(crate) struct CropJob {
    pub layer: usize,
    pub frame: Frame,
    pub width: u32,
    pub height: u32,
}

/// The pictures figures asked for during rendering, by file name, shared by
/// every clone of the render context so the emitter sees them all.
pub(crate) type CropRegistry = Arc<Mutex<BTreeMap<String, CropJob>>>;

/// A layer as one figure draws it.
#[derive(Debug, Clone)]
pub(crate) struct LayerImage {
    pub href: String,
    /// The footprints of the plates in the frame, in viewBox coordinates.
    pub footprints: Vec<Vec<(f64, f64)>>,
}

#[derive(Debug)]
pub(crate) struct StreetGround {
    pub images: Vec<LayerImage>,
    pub credit: String,
}

/// The file name (without extension) of the picture `job` asks for.
fn crop_name(layer: &MapLayer, plan: &LayerPlan, job: &CropJob) -> String {
    let frame = &job.frame;
    let mut key = format!(
        "{WARP_VERSION}|{}|{}x{}|{:.7},{:.7},{:.7},{:.7}|",
        layer.name, job.width, job.height, frame.center_longitude, frame.center_latitude, frame.longitude_span, frame.latitude_span
    );
    for plate in &plan.plates {
        key.push_str(&format!("{}:{}|", plate.id, plate.digest));
    }
    compute_binary_hash(key.as_bytes()).chars().take(16).collect()
}

/// The longitudes (radians) of the picture's columns and the latitudes
/// (degrees) of its rows.
pub(crate) fn crop_axes(job: &CropJob) -> Option<(Vec<f64>, Vec<f64>)> {
    let projection = Projection::new(&job.frame);
    let across = |count: u32, extent: f64| (0..count).map(move |n| (f64::from(n) + 0.5) / f64::from(count) * extent);
    let longitudes = across(job.width, VIEWBOX_WIDTH).map(|x| projection.longitude_at(x).map(f64::to_radians)).collect::<Option<_>>()?;
    let latitudes = across(job.height, VIEWBOX_HEIGHT).map(|y| projection.latitude_at(y)).collect::<Option<_>>()?;
    Some((longitudes, latitudes))
}

impl PlaceMapRenderContext {
    /// When every place of `target` is exact and lies on one layer's plates,
    /// the closest frame for them and the layers a figure shown `css_width`
    /// pixels wide draws in it; `target`'s frame becomes that frame. Coarser places never get a street-scale frame
    /// (`Frame::for_streets`), and the picture is asked for here, written
    /// when the map directory is emitted.
    pub(super) fn street_ground(&self, target: &mut PlaceMapTarget, css_width: f64) -> Option<StreetGround> {
        let plans = self.layer_plans();
        if plans.iter().all(|plan| plan.plates.is_empty()) {
            return None;
        }
        let placed: Vec<_> = target.marker_places().filter_map(|place| Some((place.precision, place.point()?))).collect();
        let points: Vec<ProjectedPoint> = placed.iter().map(|&(_, point)| point).collect();
        let frame = Frame::for_streets(&points, placed.iter().map(|&(precision, _)| precision))?;
        let spots: Vec<(f64, f64)> = points.iter().map(|point| world_coords::project(point.latitude, point.longitude)).collect();
        // A place needs a pixel of its own on a plate, else the picture could come out empty and never be written.
        let on_one_layer = |pixels: bool| plans.iter().any(|plan| spots.iter().all(|&(x, y)| plan.plates.iter().any(|plate| inside(&plate.footprint, x, y) && (!pixels || plate.has_pixels_at(x, y)))));
        if !on_one_layer(true) {
            if on_one_layer(false) {
                crate::build::cli_output::log_warn_problem!("a map's places lie on a historical map plate outside the part of its image that was fetched, so the map is drawn on the moss landscape");
            }
            return None;
        }

        let projection = Projection::new(&frame);
        let (west, east) = (projection.longitude_at(0.0)?, projection.longitude_at(VIEWBOX_WIDTH)?);
        if (east - west) / css_width > WORLD_CLEAN_DEGREES_PER_PX / (1.0 + WORLD_DETAIL_FADE_BAND) {
            return None;
        }
        let (north, south) = (projection.latitude_at(0.0)?, projection.latitude_at(VIEWBOX_HEIGHT)?);
        let (x0, y0) = world_coords::project(north, west);
        let (x1, y1) = world_coords::project(south, east);

        let (width, height) = ((css_width * DENSITY).round() as u32, (css_width * DENSITY * VIEWBOX_HEIGHT / VIEWBOX_WIDTH).round() as u32);
        let mut images = Vec::new();
        let mut credits: Vec<&str> = Vec::new();
        for (n, (layer, plan)) in self.layers().iter().zip(plans).enumerate() {
            let plates: Vec<_> = plan.plates.iter().filter(|plate| overlaps(plate.footprint_world(), [x0, y0, x1, y1])).collect();
            if plates.is_empty() {
                continue;
            }
            let job = CropJob { layer: n, frame, width, height };
            let name = crop_name(layer, plan, &job);
            let Ok(path) = ServedPath::for_place_map_layer_file(self.map_assets_hash(), &layer.name, &format!("crops/{name}.jpg")) else { continue };
            let footprints = plates
                .iter()
                .map(|plate| {
                    plate
                        .footprint
                        .iter()
                        .filter_map(|&(x, y)| projection.project(ProjectedPoint::new(world_coords::unproject_longitude(x), world_coords::unproject_latitude(y))?))
                        .collect()
                })
                .collect();
            self.crops().lock().expect("the crop registry is never held across a panic").entry(name).or_insert(job);
            images.push(LayerImage { href: path.to_relative_url(), footprints });
            if !plan.credit.is_empty() && !credits.contains(&plan.credit.as_str()) {
                credits.push(&plan.credit);
            }
        }
        let ground = (!images.is_empty()).then(|| StreetGround { images, credit: credits.join(" · ") });
        if ground.is_some() {
            target.frame = Some(frame);
        }
        ground
    }
}
