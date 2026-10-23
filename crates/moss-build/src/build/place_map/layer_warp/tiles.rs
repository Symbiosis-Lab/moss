//! Which tiles a historical layer has, and how one is drawn: each output
//! pixel is carried from world coordinates through the inverse projection and
//! the plate's fit into the fetched image, and sampled bilinearly.

use image::RgbImage;

use super::fit::mercator;
use super::plate::Plate;
use crate::build::place_map::world_coords::{self, east_units_per_radian, north_units_per_radian, world_width};

pub(crate) const TILE_SIZE: u32 = 256;
/// The deepest two levels are cut to this radius around a place.
pub(crate) const NEAR_METRES: f64 = 300.0;
const JPEG_QUALITY: u8 = 80;

/// World units per tile pixel at `level`.
pub(crate) fn units_per_pixel(level: u8) -> f64 {
    world_width() / (f64::from(TILE_SIZE) * 2f64.powi(i32::from(level)))
}

/// `[x0, y0, x1, y1]` of tile `(i, j)` of `level` in world coordinates.
pub(crate) fn tile_rect(level: u8, i: u32, j: u32) -> [f64; 4] {
    let side = f64::from(TILE_SIZE) * units_per_pixel(level);
    [f64::from(i) * side, f64::from(j) * side, f64::from(i + 1) * side, f64::from(j + 1) * side]
}

/// Whether two `[min_x, min_y, max_x, max_y]` boxes touch.
pub(crate) fn overlaps(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1]
}

/// A place the deepest levels are drawn around.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NearPlace {
    pub x: f64,
    pub y: f64,
    metres_per_unit: (f64, f64),
}

impl NearPlace {
    pub(crate) fn new(latitude: f64, longitude: f64) -> Self {
        let (x, y) = world_coords::project(latitude, longitude);
        let metres_per_radian = 6_371_008.8;
        Self {
            x,
            y,
            metres_per_unit: (
                metres_per_radian * latitude.to_radians().cos() / east_units_per_radian(),
                metres_per_radian / north_units_per_radian(latitude),
            ),
        }
    }

    fn within(&self, rect: [f64; 4]) -> bool {
        let dx = (rect[0] - self.x).max(self.x - rect[2]).max(0.0) * self.metres_per_unit.0;
        let dy = (rect[1] - self.y).max(self.y - rect[3]).max(0.0) * self.metres_per_unit.1;
        dx.hypot(dy) <= NEAR_METRES
    }
}

/// `(min_level, max_level)` for an `extent` of `[west, south, east, north]`:
/// from the level at which the extent spans about 512 px down to the level
/// matching `max_zoom` (degrees of latitude per pixel) at the extent's centre.
pub(crate) fn level_range(extent: [f64; 4], max_zoom: u8) -> (u8, u8) {
    let (x0, y1) = world_coords::project(extent[1], extent[0]);
    let (x1, y0) = world_coords::project(extent[3], extent[2]);
    let span = (x1 - x0).max(y1 - y0).max(1e-9);
    let centre = (extent[1] + extent[3]) / 2.0;
    let stretch = north_units_per_radian(centre) / east_units_per_radian();
    let max = (f64::from(max_zoom) - stretch.log2()).round().clamp(0.0, 22.0) as u8;
    let min = ((2.0 * world_width() / span).log2().round().clamp(0.0, 22.0) as u8).min(max);
    (min, max)
}

/// How many of the deepest levels are cut to the neighbourhood of a place.
const NEAR_LEVELS: u8 = 2;

/// Whether `level` of a layer drawn from `min` to `max` keeps only the tiles near a place: the deepest two levels, except the shallowest, which always covers the whole extent so the explorer has something to draw (and enlarge) wherever no deeper tile exists.
pub(crate) fn near_places_only(level: u8, min: u8, max: u8) -> bool {
    level > min && level + NEAR_LEVELS > max
}

/// The tiles of `level` overlapping `extent`; at the deepest two levels
/// (`restrict`) only those near one of `places`.
pub(crate) fn candidates(level: u8, extent: [f64; 4], restrict: bool, places: &[NearPlace]) -> Vec<(u32, u32)> {
    let side = f64::from(TILE_SIZE) * units_per_pixel(level);
    let (x0, y1) = world_coords::project(extent[1], extent[0]);
    let (x1, y0) = world_coords::project(extent[3], extent[2]);
    let last = (1u32 << level) - 1;
    let range = |lo: f64, hi: f64| ((lo / side).floor().max(0.0) as u32)..=((hi / side).floor().max(0.0) as u32).min(last);
    let mut out = Vec::new();
    for j in range(y0, y1) {
        for i in range(x0, x1) {
            let rect = [f64::from(i) * side, f64::from(j) * side, f64::from(i + 1) * side, f64::from(j + 1) * side];
            if !restrict || places.iter().any(|p| p.within(rect)) {
                out.push((i, j));
            }
        }
    }
    out
}

/// A plate with its image decoded.
pub(crate) struct Loaded<'a> {
    plate: &'a Plate,
    image: RgbImage,
    /// World bounding box of the footprint.
    bounds: [f64; 4],
    mask_bounds: [f64; 4],
}

impl<'a> Loaded<'a> {
    pub(crate) fn open(plate: &'a Plate) -> Result<Self, String> {
        let image = image::open(&plate.image_path).map_err(|e| format!("plate {} image cannot be decoded: {e}", plate.id))?.to_rgb8();
        let fold = |points: &[(f64, f64)]| {
            points.iter().fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, &(x, y)| [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)])
        };
        Ok(Self { plate, image, bounds: fold(&plate.footprint), mask_bounds: fold(&plate.mask) })
    }

    fn covers(&self, rect: [f64; 4]) -> bool {
        overlaps(self.bounds, rect)
    }

    /// The colour at a full-image pixel, or `None` outside the mask or the fetched region.
    fn sample(&self, u: f64, v: f64) -> Option<[u8; 3]> {
        let p = self.plate;
        let b = self.mask_bounds;
        if u < b[0] || u > b[2] || v < b[1] || v > b[3] || !inside(&p.mask, u, v) {
            return None;
        }
        let (fx, fy) = ((u - p.region[0]) * p.pixels_per_unit.0, (v - p.region[1]) * p.pixels_per_unit.1);
        let (w, h) = self.image.dimensions();
        if u < p.region[0] || v < p.region[1] || fx >= f64::from(w) || fy >= f64::from(h) {
            return None;
        }
        let (sx, sy) = (fx - 0.5, fy - 0.5);
        let (x0, y0) = (sx.floor(), sy.floor());
        let (tx, ty) = (sx - x0, sy - y0);
        let at = |x: f64, y: f64| self.image.get_pixel((x.max(0.0) as u32).min(w - 1), (y.max(0.0) as u32).min(h - 1)).0;
        let (a, b, c, d) = (at(x0, y0), at(x0 + 1.0, y0), at(x0, y0 + 1.0), at(x0 + 1.0, y0 + 1.0));
        let mut out = [0u8; 3];
        for k in 0..3 {
            let top = f64::from(a[k]) * (1.0 - tx) + f64::from(b[k]) * tx;
            let bottom = f64::from(c[k]) * (1.0 - tx) + f64::from(d[k]) * tx;
            out[k] = (top * (1.0 - ty) + bottom * ty).round().clamp(0.0, 255.0) as u8;
        }
        Some(out)
    }
}

/// Even-odd point-in-polygon.
pub(crate) fn inside(polygon: &[(f64, f64)], x: f64, y: f64) -> bool {
    let mut inside = false;
    let mut j = polygon.len() - 1;
    for i in 0..polygon.len() {
        let ((xi, yi), (xj, yj)) = (polygon[i], polygon[j]);
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Tile `(i, j)` of `level` drawn from `plates` (painted in order, later over
/// earlier), white outside every mask; `None` when no pixel was drawn.
pub(crate) fn render_tile(plates: &[Loaded], level: u8, i: u32, j: u32) -> Option<RgbImage> {
    let upp = units_per_pixel(level);
    let side = f64::from(TILE_SIZE) * upp;
    let rect = [f64::from(i) * side, f64::from(j) * side, f64::from(i + 1) * side, f64::from(j + 1) * side];
    let longitudes: Vec<f64> = (0..TILE_SIZE).map(|c| world_coords::unproject_longitude(rect[0] + (f64::from(c) + 0.5) * upp).to_radians()).collect();
    let latitudes: Vec<f64> = (0..TILE_SIZE).map(|row| world_coords::unproject_latitude(rect[1] + (f64::from(row) + 0.5) * upp)).collect();
    sample_grid(plates, rect, &longitudes, &latitudes)
}

/// The picture of `plates` over a grid of pixels, drawn the way a tile is:
/// `longitudes` (radians) and `latitudes` (degrees) are the centres of its
/// columns and rows, west to east and north to south.
pub(crate) fn render_crop(plates: &[Loaded], longitudes: &[f64], latitudes: &[f64]) -> Option<RgbImage> {
    let (west, north) = world_coords::project(*latitudes.first()?, longitudes.first()?.to_degrees());
    let (east, south) = world_coords::project(*latitudes.last()?, longitudes.last()?.to_degrees());
    sample_grid(plates, [west, north, east, south], longitudes, latitudes)
}

/// The pixels of a grid, one per longitude column and latitude row, each from
/// the topmost of the `plates` that reach `rect` (world coordinates).
fn sample_grid(plates: &[Loaded], rect: [f64; 4], longitudes: &[f64], latitudes: &[f64]) -> Option<RgbImage> {
    let relevant: Vec<&Loaded> = plates.iter().rev().filter(|p| p.covers(rect)).collect();
    if relevant.is_empty() {
        return None;
    }
    let mut tile = RgbImage::from_pixel(longitudes.len() as u32, latitudes.len() as u32, image::Rgb([255, 255, 255]));
    let mut drawn = false;
    for (row, &latitude) in latitudes.iter().enumerate() {
        let my = mercator(latitude, 0.0).1;
        for (col, &mx) in longitudes.iter().enumerate() {
            for loaded in &relevant {
                let (u, v) = loaded.plate.to_pixel.eval(mx, my);
                if let Some(rgb) = loaded.sample(u, v) {
                    tile.put_pixel(col as u32, row as u32, image::Rgb(rgb));
                    drawn = true;
                    break;
                }
            }
        }
    }
    drawn.then_some(tile)
}

/// The picture as a JPEG.
pub(crate) fn encode(tile: &RgbImage) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, JPEG_QUALITY)
        .encode(tile.as_raw(), tile.width(), tile.height(), image::ExtendedColorType::Rgb8)
        .expect("an RGB buffer encodes");
    bytes
}
