use super::{Feature, Pack};
use crate::vault::places::Precision;

mod tile_grid;
pub(crate) use tile_grid::TileGrid;
#[cfg(test)]
mod selection_tests;

pub const VIEWBOX_WIDTH: f64 = 720.0;
pub const VIEWBOX_HEIGHT: f64 = 480.0;
const POLAR_LIMIT: f64 = 85.0;
const MIN_FRAME_DEGREES: f64 = 10.0;
/// A frame's latitude floor as a share of its longitude floor, so the
/// narrowest frame fills the 3:2 canvas.
const LATITUDE_SHARE: f64 = 0.68;
/// The zoom the approved design's shadows, coast halo and lighting were
/// drawn at: the narrowest frame, whose 6.8-degree height fills the 480 px
/// canvas, about 70.6 px to the degree.
pub(crate) const DESIGN_PIXELS_PER_DEGREE: f64 = VIEWBOX_HEIGHT / (MIN_FRAME_DEGREES * LATITUDE_SHARE);
/// The zoom, as a share of the design's, at or under which a map counts as
/// wide: a frame about 20 degrees across or more, where elevation steps
/// and small features tuned for a 10-degree frame crowd together.
pub(crate) const WIDE_ZOOM: f64 = 0.5;
/// How much wider and taller than the span of its places a frame is drawn,
/// so places at its edges sit inside the map instead of on its border.
pub(crate) const FRAME_PADDING: f64 = 1.25;
const CLIP_MARGIN: f64 = 24.0;
const CLIP_MIN_X: f64 = -CLIP_MARGIN;
const CLIP_MAX_X: f64 = VIEWBOX_WIDTH + CLIP_MARGIN;
const CLIP_MIN_Y: f64 = -CLIP_MARGIN;
const CLIP_MAX_Y: f64 = VIEWBOX_HEIGHT + CLIP_MARGIN;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameTier {
    Local,
    Wide,
    World,
    /// One explorer regional detail tile: a 10x10 degree cell drawn in the
    /// SAME Patterson projection as the world map (not `FlatProjection`),
    /// at `TILE_K` times the world's own scale and clipped to the cell's
    /// own rectangle — see `PattersonProjection::for_tile`. Only
    /// `explorer::tile_frame` ever builds a `Frame` with this tier; no
    /// per-page figure does.
    Tile,
}

impl FrameTier {
    /// `svg.rs`'s own `wide` heuristic (`Projection::is_wide`) conflates a
    /// regional tile's narrow 10-degree span with a genuinely wide frame —
    /// see that call site's own comment — so both it and
    /// `TileSelection::for_frame`'s tile branch need this same check; one
    /// method keeps them from drifting into two different spellings of it.
    pub fn is_tile(self) -> bool {
        self == FrameTier::Tile
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectedPoint {
    pub longitude: f64,
    pub latitude: f64,
}

impl ProjectedPoint {
    pub fn new(longitude: f64, latitude: f64) -> Option<Self> {
        if !longitude.is_finite()
            || !latitude.is_finite()
            || !(-180.0..=180.0).contains(&longitude)
            || !(-90.0..=90.0).contains(&latitude)
        {
            return None;
        }
        Some(Self {
            longitude: normalize_longitude(longitude),
            latitude,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub center_longitude: f64,
    pub center_latitude: f64,
    pub longitude_span: f64,
    pub latitude_span: f64,
    pub tier: FrameTier,
}

impl Frame {
    pub fn from_points<I>(points: &[ProjectedPoint], precisions: I) -> Option<Self>
    where
        I: Iterator<Item = Precision>,
    {
        if points.is_empty() {
            return None;
        }
        let floor = precisions
            .map(privacy_floor)
            .fold(MIN_FRAME_DEGREES, f64::max);
        let latitude_min = points
            .iter()
            .map(|point| point.latitude)
            .fold(90.0, f64::min);
        let latitude_max = points
            .iter()
            .map(|point| point.latitude)
            .fold(-90.0, f64::max);
        let (center_longitude, longitude_span) = circular_longitude_bounds(points);
        // Padding is applied here, before `tier` below is classified, so the
        // tier reflects the padded frame that projection and tile selection
        // will actually draw, not the raw points' tighter extent.
        let latitude_span = ((latitude_max - latitude_min) * FRAME_PADDING).max(floor * LATITUDE_SHARE).min(170.0);
        let longitude_span = (longitude_span * FRAME_PADDING).max(floor).min(360.0);
        let center_latitude =
            ((latitude_min + latitude_max) / 2.0).clamp(-POLAR_LIMIT, POLAR_LIMIT);
        let tier = if points
            .iter()
            .any(|point| point.latitude.abs() > POLAR_LIMIT)
            || longitude_span > 180.0
        {
            FrameTier::World
        } else if longitude_span > 60.0 || floor >= privacy_floor(Precision::Country) {
            FrameTier::Wide
        } else {
            FrameTier::Local
        };
        Some(Self {
            center_longitude,
            center_latitude,
            longitude_span,
            latitude_span,
            tier,
        })
    }

    pub fn contains(&self, point: ProjectedPoint) -> bool {
        let delta = shortest_longitude_delta(point.longitude - self.center_longitude).abs();
        delta <= self.longitude_span / 2.0
            && (point.latitude - self.center_latitude).abs() <= self.latitude_span / 2.0
    }
}

/// The narrowest frame a place of this precision may be shown in. The
/// closest frame is about 10 degrees wide, which is already coarse enough
/// for exact, city and region places alike: their precision is carried by
/// the marker (a dot, a larger dot, a soft fade), not by zooming out. Only
/// a country-level place widens the frame.
pub fn privacy_floor(precision: Precision) -> f64 {
    match precision {
        Precision::Exact | Precision::City | Precision::Region => MIN_FRAME_DEGREES,
        Precision::Country => 180.0,
    }
}

/// The main map's marker radius. An exact or city place is a 4 px dot on a
/// 5 px casing, in screen px: the dot keeps that size at whatever width
/// the map is shown. A country, whose frame spans half the globe, is a
/// 20 viewBox px soft fade. A region's fade is sized in degrees instead
/// (`REGION_FADE_DEGREES`); this is its fallback where that can't be
/// projected.
pub fn marker_radius(precision: Precision) -> f64 {
    match precision {
        Precision::Exact | Precision::City => 4.0,
        Precision::Region | Precision::Country => 20.0,
    }
}

/// A region marker fades out over this much latitude, the approved
/// design's soft area marker: about 78 px on a 10-degree frame.
pub const REGION_FADE_DEGREES: f64 = 1.1;

/// The smallest radius, in viewBox px, a region's fade is drawn at: on the
/// world map 1.1 degrees is under 3 px, too small for a fade to read.
pub const REGION_FADE_MIN_RADIUS: f64 = 12.0;

/// Equirectangular crop of a Local/Wide frame, longitude scaled by
/// cos(center_latitude) (a local standard parallel) so a small crop's
/// shapes stay close to true instead of stretching east-west away from the
/// equator. There is no visibility/horizon concept the way there was under
/// the azimuthal projection this replaced: every point projects, and a
/// ring or line is clipped to the frame's screen rectangle afterward with
/// plain Sutherland–Hodgman / segment clipping, not horizon-arc closing.
#[derive(Debug, Clone, Copy, PartialEq)]
struct FlatProjection {
    center_longitude: f64,
    center_latitude: f64,
    cos_center_latitude: f64,
    scale: f64,
}

impl FlatProjection {
    fn new(frame: &Frame) -> Self {
        let center_latitude = frame.center_latitude;
        let cos_center_latitude = center_latitude.to_radians().cos();
        let half_width = (frame.longitude_span / 2.0).to_radians() * cos_center_latitude;
        let half_height = (frame.latitude_span / 2.0).to_radians();
        let scale = (VIEWBOX_WIDTH / 2.0 / half_width.max(f64::EPSILON))
            .min(VIEWBOX_HEIGHT / 2.0 / half_height.max(f64::EPSILON));
        Self {
            center_longitude: frame.center_longitude,
            center_latitude,
            cos_center_latitude,
            scale,
        }
    }

    fn project_point(&self, point: ProjectedPoint) -> (f64, f64) {
        let longitude = unwrap_longitude(point.longitude, self.center_longitude);
        self.project_unwrapped(longitude, point.latitude)
    }

    fn project_unwrapped(&self, longitude: f64, latitude: f64) -> (f64, f64) {
        let x = (longitude - self.center_longitude).to_radians() * self.cos_center_latitude;
        let y = (latitude - self.center_latitude).to_radians();
        (
            VIEWBOX_WIDTH / 2.0 + self.scale * x,
            VIEWBOX_HEIGHT / 2.0 - self.scale * y,
        )
    }

    /// West, east, south and north edges, in degrees, of everything this
    /// projection can draw: the viewBox plus the clip margin. The frame's
    /// own span is only a floor; fitting a 3:2 canvas usually shows more
    /// longitude than the frame asked for.
    fn visible_bounds(&self) -> (f64, f64, f64, f64) {
        let half_width = (CLIP_MAX_X - VIEWBOX_WIDTH / 2.0)
            / (self.scale * self.cos_center_latitude.max(f64::EPSILON));
        let half_height = (CLIP_MAX_Y - VIEWBOX_HEIGHT / 2.0) / self.scale;
        let (half_width, half_height) = (half_width.to_degrees(), half_height.to_degrees());
        (
            self.center_longitude - half_width,
            self.center_longitude + half_width,
            (self.center_latitude - half_height).max(-90.0),
            (self.center_latitude + half_height).min(90.0),
        )
    }

    /// The pack's rings are already planar in [-180, 180] (see
    /// `raw_screen_copies`), so a frame that reaches past the antimeridian
    /// draws the copy of each ring shifted a turn east or west.
    fn copies(&self, points: &[(i32, i32)], quantisation: u32) -> Vec<f64> {
        let (west, east, ..) = self.visible_bounds();
        let quantisation = f64::from(quantisation);
        let min = points.iter().map(|point| f64::from(point.0)).fold(f64::INFINITY, f64::min) / quantisation;
        let max = points.iter().map(|point| f64::from(point.0)).fold(f64::NEG_INFINITY, f64::max) / quantisation;
        [-360.0, 0.0, 360.0]
            .into_iter()
            .filter(|offset| min + offset <= east && max + offset >= west)
            .collect()
    }

    fn project_ring(&self, points: &[(i32, i32)], quantisation: u32) -> Vec<Vec<(f64, f64)>> {
        raw_screen_copies(points, quantisation, &self.copies(points, quantisation), |longitude, latitude| {
            self.project_unwrapped(longitude, latitude)
        })
        .into_iter()
        .map(|screen| clip_closed_ring(&screen, CLIP_MIN_X, CLIP_MAX_X, CLIP_MIN_Y, CLIP_MAX_Y))
        .filter(|ring| ring.len() >= 4)
        .collect()
    }

    fn project_part(&self, points: &[(i32, i32)], quantisation: u32) -> Vec<Vec<(f64, f64)>> {
        raw_screen_copies(points, quantisation, &self.copies(points, quantisation), |longitude, latitude| {
            self.project_unwrapped(longitude, latitude)
        })
        .iter()
        .flat_map(|screen| clip_polyline(screen, CLIP_MIN_X, CLIP_MAX_X, CLIP_MIN_Y, CLIP_MAX_Y))
        .collect()
    }
}

const PATTERSON_K1: f64 = 1.0148;
const PATTERSON_K2: f64 = 0.23185;
const PATTERSON_K3: f64 = -0.14499;
const PATTERSON_K4: f64 = 0.02406;

/// Patterson (2014) cylindrical projection: longitude is linear, and
/// y = K1·φ + K2·φ⁵ + K3·φ⁷ + K4·φ⁹, the published polynomial, tuned to
/// keep high-latitude area distortion well below plate carrée's without
/// the complexity of a true equal-area or conformal projection.
fn patterson_y(latitude_radians: f64) -> f64 {
    let squared = latitude_radians * latitude_radians;
    let fourth = squared * squared;
    latitude_radians
        * (PATTERSON_K1
            + fourth * (PATTERSON_K2 + squared * (PATTERSON_K3 + squared * PATTERSON_K4)))
}

/// Patterson cylindrical over the whole world, used for the places-root
/// World-tier map. The geometry stays centred on the prime meridian, where
/// the pack cuts its rings. The world is fitted to the viewBox height and
/// its sides are cropped, keeping the projection's aspect: fitted by width
/// instead, a 3:2 viewBox leaves empty strips above and below. The crop
/// window slides toward the frame's centre, by at most the width the crop
/// removes, so places near the antimeridian stay on the map where they can.
/// The Patterson projection's full-world canvas width at the fixed design
/// scale (`VIEWBOX_HEIGHT`): every meridian from the antimeridian west to
/// the antimeridian east, with no crop. Derived from the same scale
/// `PattersonProjection::new` computes to find how much it must crop away —
/// not a hand-typed number, so a change to the design scale or the
/// polynomial keeps both in step. Used only by the places explorer's
/// shared, runtime-panned world SVG (`explorer::emit_world_svg`), via
/// `PattersonProjection::full_extent`.
pub(crate) fn world_viewbox_width() -> f64 {
    VIEWBOX_HEIGHT * std::f64::consts::PI / patterson_y(std::f64::consts::FRAC_PI_2)
}

/// How many times the world map's own scale a regional detail tile is drawn
/// at (`PattersonProjection::for_tile`). A 10-degree cell is about 23.4
/// world units wide, so at `K = 44` its canvas is about 1030 units and one
/// integer coordinate is about 0.0097 degrees (about 1 km), which is also
/// where the pack's own vertices stop carrying detail. The canvas used to
/// be about 95 units wide, which rounded every shape to about 0.1 degree
/// and drew a visible staircase on each coast and lake at the zoom readers
/// use. The serializer snaps to integers and simplifies at one unit, so a
/// bigger `K` is what carries the finer quantum into the tile SVG's
/// coordinates. Exposed to the runtime through `tiles.json`'s own `k`
/// field (`emit::place_map_assets::emit`) so the two can never drift apart.
pub(crate) const TILE_K: f64 = 44.0;

/// The tile grid's size in cells: `tile_x`/`tile_y` bucket the world into
/// 36 columns of 10 degrees by 18 rows. Written to `tiles.json` so the
/// runtime never keeps its own copy.
pub(crate) const TILE_COLUMNS: u32 = 36;
pub(crate) const TILE_ROWS: u32 = 18;

/// The tile scale the line widths (`river_width`, the reef stroke) were
/// tuned at: a tile at `TILE_K` multiplies them by
/// `TILE_K / TILE_STROKE_BASE_K` so a river keeps the same weight against
/// the map however finely the tile is quantised.
pub(crate) const TILE_STROKE_BASE_K: f64 = 4.0;

/// How far, in world units (the same space `tileCellBounds` and
/// `tile_viewbox_equals_its_cell_rectangle_times_k` both work in — see
/// `for_tile` below), a tile's canvas extends past its own nominal cell on
/// every edge. `tileCellBounds` gives two adjacent tiles the exact same
/// shared-edge value to float precision, but each tile's own serialized
/// canvas size (`svg.rs`'s `length`, rounded to a thousandth of a canvas
/// unit) and its own CSS transform are computed and rasterized
/// independently, which can disagree by a fraction of a device pixel at the
/// shared edge — a gap with the world layer's own colour showing through.
/// A cell is about 23 world units wide at the equator; `TILE_BLEED` is
/// small enough to cost nothing in file size or render time but, at the
/// tile's own native screen scale, covers that disagreement with overlap
/// instead of a gap. The runtime never reads it: it places a tile from the
/// `origins` in `tiles.json`, which already include the bleed.
pub(crate) const TILE_BLEED: f64 = 0.2;

#[derive(Debug, Clone, Copy, PartialEq)]
struct PattersonProjection {
    scale: f64,
    offset_x: f64,
    /// Added to `canvas_height / 2` before the scaled y term, mirroring
    /// `offset_x`: zero for the page map and the full-extent world copy
    /// (both vertically centred on the equator), non-zero only for
    /// `for_tile`, whose cell is rarely centred there.
    offset_y: f64,
    /// This projection's own canvas width, in viewBox units: `VIEWBOX_WIDTH`
    /// for the cropped per-page map (`new`), `world_viewbox_width()` for
    /// the explorer's uncropped copy (`full_extent`), or a cell's own
    /// `TILE_K`-scaled width for a regional tile (`for_tile`). Centres
    /// `project_unwrapped` and bounds the ring/line clip margin below, so
    /// one projection formula and one clip routine serve all three.
    canvas_width: f64,
    /// This projection's own canvas height, in viewBox units: `VIEWBOX_HEIGHT`
    /// for `new`/`full_extent` (both fixed to the page canvas height), or a
    /// cell's own `TILE_K`-scaled height for a regional tile — shorter than
    /// `VIEWBOX_HEIGHT` at a cell away from the equator, since Patterson
    /// itself compresses high-latitude cells vertically. See `for_tile`.
    canvas_height: f64,
}

impl PattersonProjection {
    fn new(frame: &Frame) -> Self {
        let scale = VIEWBOX_HEIGHT / 2.0 / patterson_y(std::f64::consts::FRAC_PI_2);
        let cropped = (scale * std::f64::consts::PI - VIEWBOX_WIDTH / 2.0).max(0.0);
        let offset_x = (-scale * frame.center_longitude.to_radians()).clamp(-cropped, cropped);
        Self { scale, offset_x, offset_y: 0.0, canvas_width: VIEWBOX_WIDTH, canvas_height: VIEWBOX_HEIGHT }
    }

    /// The whole globe, no crop and no slide toward any frame's centre:
    /// used only by the places explorer's shared world SVG and its clip
    /// bounds (`explorer::emit_world_svg`), which a reader pans and zooms
    /// across the whole globe at runtime — unlike a page's own World-tier
    /// map (`new`, above), which always stays cropped to the fixed
    /// `VIEWBOX_WIDTH` canvas.
    fn full_extent() -> Self {
        let scale = VIEWBOX_HEIGHT / 2.0 / patterson_y(std::f64::consts::FRAC_PI_2);
        Self { scale, offset_x: 0.0, offset_y: 0.0, canvas_width: world_viewbox_width(), canvas_height: VIEWBOX_HEIGHT }
    }

    /// One regional detail tile (`FrameTier::Tile`, built by
    /// `explorer::tile_frame`): the SAME Patterson projection as the world
    /// map, at `TILE_K` times its scale, with its own viewBox clipped to the
    /// cell's rectangle padded by `TILE_BLEED` on every edge — not the
    /// world's. `frame`'s
    /// `center_longitude`/`center_latitude`/`longitude_span`/`latitude_span`
    /// already describe that 10x10 degree cell (`explorer::tile_frame`), so
    /// the cell's west/east/north/south edges come straight from them.
    ///
    /// Scaling `full_extent`'s own `scale` by `TILE_K` and re-deriving
    /// `offset_x`/`offset_y`/`canvas_width`/`canvas_height` from the SAME
    /// (bled) cell edges, rather than composing a second transform on top of
    /// the world's own output, is what makes a tile overlay the world
    /// exactly: any point this projects and any point `full_extent` projects
    /// for the same longitude/latitude differ by exactly the affine map the
    /// runtime also applies (`translate(cellX - TILE_BLEED, cellY -
    /// TILE_BLEED) scale(1/TILE_K)`, `js-src/site/places-explorer/tiles.ts`'s
    /// `tileOverlayTransform`), to float precision — see
    /// `world_and_a_tile_agree_on_a_known_points_position_after_the_runtimes_transform`.
    ///
    /// The box is `TileGrid`'s: the padded cell snapped outward to whole
    /// canvas units, so the canvas starts at `tile_origin_units` and is a
    /// whole number of units wide and tall.
    fn for_tile(frame: &Frame) -> Self {
        let full = Self::full_extent();
        let grid = TileGrid::of(frame);
        // The canvas is the grid box: its corner sits on a whole canvas
        // unit of the world's lattice, so offsets derive from the box's
        // own centre.
        let (min_x, max_x) = (grid.left / TILE_K, (grid.left + grid.width) / TILE_K);
        let (min_y, max_y) = (grid.top / TILE_K, (grid.top + grid.height) / TILE_K);
        let scale = full.scale * TILE_K;
        let offset_x = TILE_K * (full.canvas_width / 2.0 - (min_x + max_x) / 2.0);
        let offset_y = TILE_K * (full.canvas_height / 2.0 - (min_y + max_y) / 2.0);
        Self { scale, offset_x, offset_y, canvas_width: grid.width, canvas_height: grid.height }
    }

    fn project_point(&self, point: ProjectedPoint) -> (f64, f64) {
        self.project_unwrapped(point.longitude, point.latitude)
    }

    fn project_unwrapped(&self, longitude: f64, latitude: f64) -> (f64, f64) {
        let x = longitude.to_radians();
        let y = patterson_y(latitude.to_radians());
        (
            self.canvas_width / 2.0 + self.offset_x + self.scale * x,
            self.canvas_height / 2.0 + self.offset_y - self.scale * y,
        )
    }

    /// The world map spans exactly one turn, so each ring is drawn once,
    /// from its raw coordinates (see `raw_screen_copies`). The clip bounds
    /// follow this projection's own `canvas_width`/`canvas_height`, not the
    /// page-map's fixed `CLIP_MIN_X`/`CLIP_MAX_X`/`CLIP_MIN_Y`/`CLIP_MAX_Y`,
    /// so the explorer's full-extent copy and a regional tile each clip at
    /// their own edge instead of the cropped page canvas's.
    fn project_ring(&self, points: &[(i32, i32)], quantisation: u32) -> Vec<Vec<(f64, f64)>> {
        raw_screen_copies(points, quantisation, &[0.0], |longitude, latitude| {
            self.project_unwrapped(longitude, latitude)
        })
        .into_iter()
        .map(|screen| {
            clip_closed_ring(
                &screen,
                -CLIP_MARGIN,
                self.canvas_width + CLIP_MARGIN,
                -CLIP_MARGIN,
                self.canvas_height + CLIP_MARGIN,
            )
        })
        .filter(|ring| ring.len() >= 4)
        .collect()
    }

    fn project_part(&self, points: &[(i32, i32)], quantisation: u32) -> Vec<Vec<(f64, f64)>> {
        raw_screen_copies(points, quantisation, &[0.0], |longitude, latitude| {
            self.project_unwrapped(longitude, latitude)
        })
        .iter()
        .flat_map(|screen| {
            clip_polyline(
                screen,
                -CLIP_MARGIN,
                self.canvas_width + CLIP_MARGIN,
                -CLIP_MARGIN,
                self.canvas_height + CLIP_MARGIN,
            )
        })
        .collect()
    }
}

/// Either cylindrical projection this crate uses for a main map: an
/// equirectangular crop for a Local/Wide locator or place-term page, or a
/// world-spanning Patterson cylindrical for the places-root aggregate. The
/// small globe inset (`globe.rs`) is unrelated to both — it keeps its own
/// orthographic projection regardless of which of these renders the main
/// map on the same figure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Projection {
    Flat(FlatProjection),
    Patterson(PattersonProjection),
}

impl Projection {
    pub fn new(frame: &Frame) -> Self {
        match frame.tier {
            FrameTier::World => Self::Patterson(PattersonProjection::new(frame)),
            FrameTier::Local | FrameTier::Wide => Self::Flat(FlatProjection::new(frame)),
            FrameTier::Tile => Self::Patterson(PattersonProjection::for_tile(frame)),
        }
    }

    /// The full-extent world projection (see `PattersonProjection::full_extent`),
    /// for the places explorer's shared world SVG only.
    pub fn new_world_full_extent() -> Self {
        Self::Patterson(PattersonProjection::full_extent())
    }

    /// This projection's own `(canvas_width, canvas_height)`, in viewBox
    /// units — `(VIEWBOX_WIDTH, VIEWBOX_HEIGHT)` for every `FlatProjection`
    /// (every page figure keeps the fixed 720x480 canvas), or the
    /// `PattersonProjection`'s own stored pair otherwise. Exposed so
    /// `explorer::emit_tile_svg` can size its `Writer` from the SAME
    /// projection `render_svg` goes on to draw with, rather than
    /// re-deriving a tile's canvas dimensions a second way.
    pub(crate) fn canvas_size(&self) -> (f64, f64) {
        match self {
            Self::Flat(_) => (VIEWBOX_WIDTH, VIEWBOX_HEIGHT),
            Self::Patterson(projection) => (projection.canvas_width, projection.canvas_height),
        }
    }

    /// Whether the map is drawn at or under half the design's zoom.
    pub fn is_wide(&self) -> bool {
        self.effect_scale() <= WIDE_ZOOM
    }

    /// How much the approved design's effect sizes (cut-paper shadows,
    /// coast halo, lighting blur and relief) shrink for this frame: 1 at
    /// the narrowest frame, and in proportion to the zoom for a wider one.
    /// Drawn at full size on a world map, every small band ring casts a
    /// shadow about 0.6 degrees long, and the terrain reads as shards.
    pub fn effect_scale(&self) -> f64 {
        let scale = match self {
            Self::Flat(projection) => projection.scale,
            Self::Patterson(projection) => projection.scale,
        };
        // `scale` is viewBox px per radian; `.to_radians()` here is not an
        // angle conversion, just a convenient stand-in for the same
        // constant (pi/180) that turns "per radian" into "per degree", so
        // it can be compared against DESIGN_PIXELS_PER_DEGREE below.
        let ratio = scale.to_radians() / DESIGN_PIXELS_PER_DEGREE;
        (ratio.min(1.0) * 1000.0).round() / 1000.0
    }

    /// Project one point. Always succeeds: neither projection has a
    /// visibility concept, unlike the azimuthal projection this replaced.
    /// `Option` stays in the signature so callers (markers) don't change.
    pub fn project(&self, point: ProjectedPoint) -> Option<(f64, f64)> {
        Some(match self {
            Self::Flat(projection) => projection.project_point(point),
            Self::Patterson(projection) => projection.project_point(point),
        })
    }

    pub fn project_part(&self, points: &[(i32, i32)], quantisation: u32) -> Vec<Vec<(f64, f64)>> {
        match self {
            Self::Flat(projection) => projection.project_part(points, quantisation),
            Self::Patterson(projection) => projection.project_part(points, quantisation),
        }
    }

    /// Project one encoded polygon ring, clipped to the frame's screen
    /// rectangle. Every returned inner vector is one closed ring: the
    /// pack's rings are already cut at the antimeridian, so each projects
    /// from its raw coordinates (see `raw_screen_copies`).
    pub fn project_ring(&self, points: &[(i32, i32)], quantisation: u32) -> Vec<Vec<(f64, f64)>> {
        match self {
            Self::Flat(projection) => projection.project_ring(points, quantisation),
            Self::Patterson(projection) => projection.project_ring(points, quantisation),
        }
    }

    /// Project every ring belonging to one source feature without changing
    /// their grouping. The returned rings must be emitted together in one
    /// SVG path, whose nonzero fill keeps interior rings as holes.
    pub fn project_feature(
        &self,
        rings: &[&[(i32, i32)]],
        quantisation: u32,
    ) -> Vec<Vec<(f64, f64)>> {
        rings
            .iter()
            .flat_map(|ring| self.project_ring(ring, quantisation))
            .collect()
    }
}

/// Close a horizon-crossing run by walking the visible-hemisphere boundary
/// circle from `start` to `end`. Shared by the globe inset (`globe.rs`,
/// centered on its own small circle) and this file's tests exercising it
/// directly — the main map no longer has a horizon to clip against (it
/// uses the flat/Patterson projections above instead), but the globe inset
/// still does.
///
/// `desired_winding` fixes one rotational sense (the source ring's own
/// signed area) for every crossing in a ring, rather than re-deciding per
/// crossing: for a simple polygon clipped against a convex disc, the arcs
/// that stitch its chords back together all go around the boundary the
/// same way. A `desired_winding` of (near) zero has no ring orientation to
/// anchor a direction, so it falls back to the shorter of the two arcs.
///
/// The direction is resolved from `short_delta`, the numerically stable
/// signed delta in `(-pi, pi]`, rather than from `start_angle - end_angle`
/// directly. When `start` and `end` sit within float noise of the same
/// horizon point — a ring grazing the visibility cutoff more than once in
/// close succession, the way a real coastline does — the raw subtraction's
/// sign is arbitrary (decided by an unrelated bisection search elsewhere),
/// and feeding that sign through `rem_euclid` used to send one sign to a
/// near-zero arc and the other to a near-2*pi arc that loops almost all the
/// way around the circle, rendering as a crescent bulging past the disc.
/// `DEGENERATE_ANGLE` treats any delta under ~0.6 degrees as already
/// matching the desired direction, so a near-zero gap always closes
/// near-zero regardless of which side of zero float noise landed it on.
pub(super) fn append_horizon_arc(
    ring: &mut Vec<(f64, f64)>,
    center: (f64, f64),
    start: (f64, f64),
    end: (f64, f64),
    radius: f64,
) {
    let start_angle = (start.1 - center.1).atan2(start.0 - center.0);
    let end_angle = (end.1 - center.1).atan2(end.0 - center.0);
    let tau = 2.0 * std::f64::consts::PI;
    // Always the minor (shorter, < 180 degree) arc. A "major arc" choice
    // driven by the source ring's winding — global or local — was tried
    // and measured wrong on real data: Beirut's own merged-landmass
    // feature and Ireland's coastline (viewed from the globe inset
    // centred near Hainan) each have a real, non-degenerate run whose
    // winding-selected "major" arc closes to 99% of the horizon disc
    // instead of the sliver the run's own entry/exit gap actually spans.
    // Every case measured across both the old main-map horizon clipper and
    // the globe inset — tiny near-duplicate crossings and substantial
    // real ones alike — wants the minor arc; `short_delta` already can't
    // suffer the old near-2*pi sign-flip (Defect 1's mechanism), since it
    // is one continuous formula in `(-pi, pi]`, not a branch choosing
    // between two representations of the same angle.
    let arc_delta = (end_angle - start_angle + std::f64::consts::PI).rem_euclid(tau)
        - std::f64::consts::PI;
    let steps = ((arc_delta.abs() / (std::f64::consts::PI / 12.0)).ceil() as usize)
        .clamp(1, 128);
    for step in 1..=steps {
        let angle = start_angle + arc_delta * step as f64 / steps as f64;
        push_unique(
            ring,
            if step == steps {
                end
            } else {
                (center.0 + radius * angle.cos(), center.1 + radius * angle.sin())
            },
        );
    }
}

#[cfg(test)]
fn signed_area(points: &[(f64, f64)]) -> f64 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(&(x1, y1), &(x2, y2))| x1 * y2 - x2 * y1)
        .sum::<f64>()
        / 2.0
}

/// Project a pack ring or line from its raw coordinates, once per longitude
/// offset. The generator cuts every ring and line at the antimeridian and
/// closes a ring around a pole with explicit edges along the seam and the
/// pole, so in [-180, 180] the raw coordinates are already a correct
/// planar shape (`pack_rings_cross_the_antimeridian_only_along_a_pole`
/// checks the pack). Developing longitudes the short way at each edge
/// instead reads the 360-degree edge along a pole as zero width, which
/// lost the sector of Antarctica between a ring's first vertex and the
/// antimeridian.
fn raw_screen_copies(
    points: &[(i32, i32)],
    quantisation: u32,
    offsets: &[f64],
    project: impl Fn(f64, f64) -> (f64, f64),
) -> Vec<Vec<(f64, f64)>> {
    let seam = 180 * quantisation as i32;
    let quantisation = f64::from(quantisation);
    offsets
        .iter()
        .map(|offset| {
            points
                .iter()
                .map(|&(longitude, latitude)| {
                    let (x, y) = project(f64::from(longitude) / quantisation + offset, f64::from(latitude) / quantisation);
                    // Where a frame shows both halves of a ring the pack cut
                    // at the antimeridian, abutting edges leave an
                    // anti-aliased seam that every band's shadow deepens.
                    // Each half reaches half a pixel across the cut so the
                    // two overlap instead.
                    let overlap = if longitude == seam {
                        SEAM_OVERLAP
                    } else if longitude == -seam {
                        -SEAM_OVERLAP
                    } else {
                        0.0
                    };
                    (x + overlap, y)
                })
                .collect()
        })
        .collect()
}

const SEAM_OVERLAP: f64 = 0.5;

fn clip_polyline(points: &[(f64, f64)], min_x: f64, max_x: f64, min_y: f64, max_y: f64) -> Vec<Vec<(f64, f64)>> {
    let mut paths: Vec<Vec<(f64, f64)>> = Vec::new();
    for pair in points.windows(2) {
        let Some((start, end)) = clip_segment(pair[0], pair[1], min_x, max_x, min_y, max_y) else {
            continue;
        };
        if let Some(path) = paths.last_mut() {
            if same_point(path.last().copied(), start) {
                path.push(end);
                continue;
            }
        }
        paths.push(vec![start, end]);
    }
    paths
}

/// `min_x`/`max_x`/`min_y`/`max_y` let the Patterson full-extent and
/// regional-tile projections clip against their own canvas instead of the
/// page-map's fixed `CLIP_MIN_X`/`CLIP_MAX_X`/`CLIP_MIN_Y`/`CLIP_MAX_Y`
/// (see `PattersonProjection::project_part`).
fn clip_segment(
    start: (f64, f64),
    end: (f64, f64),
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
) -> Option<((f64, f64), (f64, f64))> {
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let mut lower: f64 = 0.0;
    let mut upper: f64 = 1.0;
    for (coordinate, delta, minimum, maximum) in [
        (start.0, dx, min_x, max_x),
        (start.1, dy, min_y, max_y),
    ] {
        if delta.abs() < f64::EPSILON {
            if coordinate < minimum || coordinate > maximum {
                return None;
            }
            continue;
        }
        let mut low = (minimum - coordinate) / delta;
        let mut high = (maximum - coordinate) / delta;
        if low > high {
            std::mem::swap(&mut low, &mut high);
        }
        lower = lower.max(low);
        upper = upper.min(high);
        if lower > upper {
            return None;
        }
    }
    Some((
        (start.0 + dx * lower, start.1 + dy * lower),
        (start.0 + dx * upper, start.1 + dy * upper),
    ))
}

fn push_unique(points: &mut Vec<(f64, f64)>, point: (f64, f64)) {
    if !same_point(points.last().copied(), point) {
        points.push(point);
    }
}

fn same_point(first: Option<(f64, f64)>, second: (f64, f64)) -> bool {
    first
        .is_some_and(|point| (point.0 - second.0).abs() < 1e-7 && (point.1 - second.1).abs() < 1e-7)
}

/// Sutherland–Hodgman clipping for a closed screen-space ring.
///
/// Unlike `clip_segment`, this keeps the entering and leaving intersections in
/// one ordered ring and explicitly closes the result. It is deliberately
/// private: emitters should receive already-clipped rings through
/// [`Projection::project_ring`], not assemble their own topology.
fn clip_closed_ring(
    points: &[(f64, f64)],
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
) -> Vec<(f64, f64)> {
    let mut ring = points.to_vec();
    if ring.len() < 3 {
        return Vec::new();
    }
    if same_point(ring.first().copied(), *ring.last().unwrap()) {
        ring.pop();
    }
    for (axis, boundary, keep_greater) in [
        (0usize, min_x, true),
        (0usize, max_x, false),
        (1usize, min_y, true),
        (1usize, max_y, false),
    ] {
        if ring.len() < 3 {
            return Vec::new();
        }
        let mut clipped = Vec::new();
        let mut previous = *ring.last().unwrap();
        let mut previous_inside = inside(previous, axis, boundary, keep_greater);
        for &current in &ring {
            let current_inside = inside(current, axis, boundary, keep_greater);
            if current_inside != previous_inside {
                clipped.push(intersection(previous, current, axis, boundary));
            }
            if current_inside {
                clipped.push(current);
            }
            previous = current;
            previous_inside = current_inside;
        }
        ring = clipped;
        if ring.len() < 3 {
            return Vec::new();
        }
    }
    ring.push(ring[0]);
    ring
}

fn inside(point: (f64, f64), axis: usize, boundary: f64, keep_greater: bool) -> bool {
    if keep_greater {
        axis_value(point, axis) >= boundary - 1e-9
    } else {
        axis_value(point, axis) <= boundary + 1e-9
    }
}

fn axis_value(point: (f64, f64), axis: usize) -> f64 {
    if axis == 0 { point.0 } else { point.1 }
}

fn intersection(start: (f64, f64), end: (f64, f64), axis: usize, boundary: f64) -> (f64, f64) {
    let start_value = axis_value(start, axis);
    let end_value = axis_value(end, axis);
    let fraction = if (end_value - start_value).abs() < f64::EPSILON {
        0.0
    } else {
        (boundary - start_value) / (end_value - start_value)
    };
    (start.0 + (end.0 - start.0) * fraction, start.1 + (end.1 - start.1) * fraction)
}

/// The fine-tier tiles a main map draws. The world map draws them all,
/// simplified on screen like any wide frame: the world tier, simplified by
/// about a degree for the globe inset, facets coasts, lakes and bands when
/// drawn across the whole canvas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileSelection {
    pub tiles: Vec<(i16, i16)>,
}

impl TileSelection {
    pub fn for_frame(pack: &Pack, frame: &Frame) -> Self {
        if frame.tier == FrameTier::World {
            return Self {
                tiles: pack.tiles.iter().map(|tile| (tile.x, tile.y)).collect(),
            };
        }
        if frame.tier.is_tile() {
            // Exactly the cell `explorer::tile_frame` built this frame
            // around — never its neighbours, even the sliver the margin
            // below would otherwise reach into. A fine-tier feature whose
            // real extent spans several cells is registered once per
            // intersecting bucket at pack-build time (the same contract the
            // Local/Wide/World branches below already lean on), each
            // bucket's own copy independently simplified — close enough
            // that two neighbouring copies of the same coastline agree to
            // drawing precision, but not pixel-for-pixel identical. A
            // regional tile draws from ONE bucket only, so it never
            // composites two slightly different copies of the same feature
            // into one canvas; a Local/Wide frame's own equirectangular crop
            // has no bucket of its own to stay inside, so it still reaches
            // across the margin below for whichever tiles its wider view
            // touches.
            let cell = (tile_x(frame.center_longitude), tile_y(frame.center_latitude));
            return Self {
                tiles: pack.tiles.iter().map(|tile| (tile.x, tile.y)).filter(|&xy| xy == cell).collect(),
            };
        }
        let (west, east, south, north) = FlatProjection::new(frame).visible_bounds();
        let x_start = tile_x(west);
        let x_end = tile_x(east);
        let y_start = tile_y(south);
        let y_end = tile_y(north);
        let every_longitude = east - west >= 360.0;
        let mut tiles = Vec::new();
        for tile in &pack.tiles {
            let in_x = if every_longitude {
                true
            } else if x_start <= x_end {
                tile.x >= x_start && tile.x <= x_end
            } else {
                tile.x >= x_start || tile.x <= x_end
            };
            if in_x && tile.y >= y_start && tile.y <= y_end {
                tiles.push((tile.x, tile.y));
            }
        }
        Self { tiles }
    }

    pub fn features<'a>(&self, pack: &'a Pack) -> Vec<(u8, &'a Feature)> {
        let tier = &pack.tiers[1];
        let tile_ids: std::collections::BTreeSet<u32> = pack
            .tiles
            .iter()
            .filter(|tile| self.tiles.contains(&(tile.x, tile.y)))
            .flat_map(|tile| tile.features.iter().copied())
            .collect();
        let mut global_index = 0u32;
        let mut features = Vec::with_capacity(tile_ids.len());
        for layer in &tier.layers {
            // Decoding validates dense global IDs across the layer boundaries.
            // Keep their registry order without scanning unselected features.
            for feature_id in tile_ids.range(global_index..global_index + layer.feature_count) {
                #[cfg(test)]
                selection_tests::FEATURE_VISITS.with(|visits| visits.set(visits.get() + 1));
                features.push((
                    layer.id,
                    &layer.features[(*feature_id - global_index) as usize],
                ));
            }
            global_index += layer.feature_count;
        }
        features
    }
}

pub(super) fn tile_x(longitude: f64) -> i16 {
    (((normalize_longitude(longitude) + 180.0) / 10.0).floor() as i16).clamp(0, 35)
}

pub(super) fn tile_y(latitude: f64) -> i16 {
    (((latitude.clamp(-90.0, 90.0) + 90.0) / 10.0).floor() as i16).clamp(0, 17)
}

pub fn normalize_longitude(longitude: f64) -> f64 {
    let mut normalized = (longitude + 180.0).rem_euclid(360.0) - 180.0;
    if normalized == 180.0 {
        normalized = -180.0;
    }
    normalized
}

fn shortest_longitude_delta(delta: f64) -> f64 {
    (delta + 180.0).rem_euclid(360.0) - 180.0
}

fn unwrap_longitude(longitude: f64, center: f64) -> f64 {
    center + shortest_longitude_delta(longitude - center)
}

fn circular_longitude_bounds(points: &[ProjectedPoint]) -> (f64, f64) {
    let mut longitudes: Vec<f64> = points
        .iter()
        .map(|point| normalize_longitude(point.longitude))
        .collect();
    longitudes.sort_by(f64::total_cmp);
    if longitudes.len() == 1 {
        return (longitudes[0], 0.0);
    }
    let mut largest_gap = (0usize, -1.0f64);
    for index in 0..longitudes.len() {
        let next = if index + 1 < longitudes.len() {
            longitudes[index + 1]
        } else {
            longitudes[0] + 360.0
        };
        let gap = next - longitudes[index];
        if gap > largest_gap.1 {
            largest_gap = (index, gap);
        }
    }
    let start_index = (largest_gap.0 + 1) % longitudes.len();
    let start = longitudes[start_index];
    let end = longitudes[largest_gap.0]
        + if largest_gap.0 < start_index {
            360.0
        } else {
            0.0
        };
    let span = end - start;
    let center = normalize_longitude(start + span / 2.0);
    (center, span)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longitude_normalisation_is_half_open() {
        assert_eq!(normalize_longitude(180.0), -180.0);
        assert_eq!(normalize_longitude(-540.0), -180.0);
        assert_eq!(normalize_longitude(540.0), -180.0);
    }

    #[test]
    fn date_line_points_use_the_short_arc() {
        let points = [
            ProjectedPoint::new(179.0, 10.0).unwrap(),
            ProjectedPoint::new(-179.0, 11.0).unwrap(),
        ];
        let frame =
            Frame::from_points(&points, [Precision::Exact, Precision::Exact].into_iter()).unwrap();
        assert!(frame.longitude_span < 20.0);
    }

    #[test]
    fn exact_city_and_region_places_share_the_ten_degree_frame() {
        let point = ProjectedPoint::new(40.5, 36.0).unwrap();
        for precision in [Precision::Exact, Precision::City, Precision::Region] {
            let frame = Frame::from_points(&[point], [precision].into_iter()).unwrap();
            assert_eq!(frame.longitude_span, 10.0, "{precision:?}");
            assert_eq!(frame.tier, FrameTier::Local, "{precision:?}");
        }
    }

    #[test]
    fn privacy_floor_wins_over_a_tiny_coordinate_span() {
        let points = [
            ProjectedPoint::new(10.0, 10.0).unwrap(),
            ProjectedPoint::new(10.1, 10.1).unwrap(),
        ];
        let frame = Frame::from_points(&points, [Precision::Country].into_iter()).unwrap();
        assert!(frame.longitude_span >= privacy_floor(Precision::Country));
        assert_eq!(frame.tier, FrameTier::Wide);
    }

    #[test]
    fn polar_points_select_world_tier() {
        let points = [ProjectedPoint::new(0.0, 85.1).unwrap()];
        let frame = Frame::from_points(&points, [Precision::Exact].into_iter()).unwrap();
        assert_eq!(frame.tier, FrameTier::World);
    }

    // -- FlatProjection: Local/Wide locators and place-term maps --------

    #[test]
    fn flat_projection_centers_the_frame_point_on_the_viewbox() {
        let point = ProjectedPoint::new(12.0, 41.0).unwrap();
        let frame = Frame::from_points(&[point], [Precision::Exact].into_iter()).unwrap();
        assert_eq!(frame.tier, FrameTier::Local);
        let projection = Projection::new(&frame);
        let (x, y) = projection.project(point).unwrap();
        assert!((x - VIEWBOX_WIDTH / 2.0).abs() < 0.001);
        assert!((y - VIEWBOX_HEIGHT / 2.0).abs() < 0.001);
    }

    /// The whole point of the cos(center_latitude) correction: a locator
    /// framed at a high latitude (Stockholm) must compress the on-screen
    /// spacing between two points a fixed number of *longitude* degrees
    /// apart, relative to the same number of *latitude* degrees, by
    /// cos(center_latitude) — otherwise east-west shapes stretch at
    /// locator scale. Verified against an independent formula, not by
    /// re-deriving the implementation's own arithmetic.
    #[test]
    fn flat_projection_scales_longitude_by_cosine_of_center_latitude() {
        let center_latitude = 59.33_f64; // Stockholm
        let west = ProjectedPoint::new(17.0, center_latitude).unwrap();
        let east = ProjectedPoint::new(19.0, center_latitude).unwrap();
        let south = ProjectedPoint::new(18.0, center_latitude - 1.0).unwrap();
        let north = ProjectedPoint::new(18.0, center_latitude + 1.0).unwrap();
        let frame = Frame::from_points(
            &[west, east, south, north],
            [Precision::Exact; 4].into_iter(),
        )
        .unwrap();
        assert_eq!(frame.tier, FrameTier::Local);
        let projection = Projection::new(&frame);
        let (west_x, _) = projection.project(west).unwrap();
        let (east_x, _) = projection.project(east).unwrap();
        let (_, south_y) = projection.project(south).unwrap();
        let (_, north_y) = projection.project(north).unwrap();
        let x_per_degree = (east_x - west_x).abs() / 2.0;
        let y_per_degree = (south_y - north_y).abs() / 2.0;
        let expected_ratio = center_latitude.to_radians().cos();
        let actual_ratio = x_per_degree / y_per_degree;
        assert!(
            (actual_ratio - expected_ratio).abs() < 0.001,
            "x/y degree ratio {actual_ratio} should match cos({center_latitude}) = {expected_ratio}"
        );
    }

    #[test]
    fn flat_projection_fits_a_wide_frame_by_width_and_a_tall_frame_by_height() {
        let wide_points = [
            ProjectedPoint::new(-10.0, 0.0).unwrap(),
            ProjectedPoint::new(10.0, 0.0).unwrap(),
        ];
        let wide_frame =
            Frame::from_points(&wide_points, [Precision::Exact, Precision::Exact].into_iter())
                .unwrap();
        let wide_projection = Projection::new(&wide_frame);
        let (west_x, _) = wide_projection.project(wide_points[0]).unwrap();
        let (east_x, _) = wide_projection.project(wide_points[1]).unwrap();
        // The places span the padded frame's middle 1 / FRAME_PADDING.
        let inset = VIEWBOX_WIDTH * (1.0 - 1.0 / FRAME_PADDING) / 2.0;
        assert!((west_x - inset).abs() < 1.0, "the west place should sit {inset} in from the edge: {west_x}");
        assert!((east_x - (VIEWBOX_WIDTH - inset)).abs() < 1.0, "the east place should sit {inset} in from the edge: {east_x}");

        let tall_points = [
            ProjectedPoint::new(0.0, -40.0).unwrap(),
            ProjectedPoint::new(0.0, 40.0).unwrap(),
        ];
        let tall_frame =
            Frame::from_points(&tall_points, [Precision::Exact, Precision::Exact].into_iter())
                .unwrap();
        let tall_projection = Projection::new(&tall_frame);
        let (_, south_y) = tall_projection.project(tall_points[0]).unwrap();
        let (_, north_y) = tall_projection.project(tall_points[1]).unwrap();
        let inset = VIEWBOX_HEIGHT * (1.0 - 1.0 / FRAME_PADDING) / 2.0;
        assert!((south_y - (VIEWBOX_HEIGHT - inset)).abs() < 1.0, "the south place should sit {inset} in from the edge: {south_y}");
        assert!((north_y - inset).abs() < 1.0, "the north place should sit {inset} in from the edge: {north_y}");
    }

    /// A ring that straddles the antimeridian (a Fiji-shaped locator) must
    /// close into one simple shape with the expected small enclosed area —
    /// not a chord straight across the frame connecting +179° to -179° the
    /// long way.
    #[test]
    fn flat_projection_joins_the_halves_the_pack_cuts_at_the_antimeridian() {
        let points = [
            ProjectedPoint::new(179.0, -1.0).unwrap(),
            ProjectedPoint::new(-179.0, -1.0).unwrap(),
        ];
        let frame =
            Frame::from_points(&points, [Precision::Exact, Precision::Exact].into_iter()).unwrap();
        let projection = Projection::new(&frame);
        // A 2-degree box straddling 180, as the pack stores it: cut into a
        // western half ending at 180 and an eastern half starting at -180.
        let west = projection.project_ring(&[(1790, -20), (1800, -20), (1800, 20), (1790, 20), (1790, -20)], 10);
        let east = projection.project_ring(&[(-1800, -20), (-1790, -20), (-1790, 20), (-1800, 20), (-1800, -20)], 10);
        assert_eq!((west.len(), east.len()), (1, 1));
        let right_edge = west[0].iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
        let left_edge = east[0].iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
        assert!((right_edge - left_edge - 1.0).abs() < 1e-6, "halves overlap from {left_edge} to {right_edge}");
        assert!((signed_area(&west[0]).abs() - signed_area(&east[0]).abs()).abs() < 1.0);
    }

    #[test]
    fn flat_projection_line_clips_at_the_frame_rectangle() {
        let center = ProjectedPoint::new(0.0, 0.0).unwrap();
        let frame = Frame::from_points(&[center], [Precision::Exact].into_iter()).unwrap();
        let projection = Projection::new(&frame);
        // Far outside the Local-tier crop (~10 degree span) on both ends,
        // but still valid degrees and on the frame-center side of the
        // globe (unlike -170/170, whose short way is via the antipodal
        // meridian, not through this frame at all).
        let paths = projection.project_part(&[(-60, 0), (60, 0)], 1);
        assert_eq!(paths.len(), 1);
        let clip_min_x = -CLIP_MARGIN;
        let clip_max_x = VIEWBOX_WIDTH + CLIP_MARGIN;
        assert!(paths[0].iter().all(|point| {
            point.0 >= clip_min_x - 0.01 && point.0 <= clip_max_x + 0.01
        }));
        assert!((paths[0][0].0 - clip_min_x).abs() < 0.5);
        assert!((paths[0][1].0 - clip_max_x).abs() < 0.5);
    }

    #[test]
    fn flat_projection_ring_and_feature_preserve_holes_for_one_path() {
        // Frame::from_points floors a single Exact point's span at
        // MIN_FRAME_DEGREES (10), so keep the outer ring comfortably
        // inside that, or it clips down smaller than the hole.
        let center = ProjectedPoint::new(0.0, 0.0).unwrap();
        let frame = Frame::from_points(&[center], [Precision::Exact].into_iter()).unwrap();
        let projection = Projection::new(&frame);
        let outer = [(-40, -40), (40, -40), (40, 40), (-40, 40), (-40, -40)];
        let hole = [(-10, -10), (-10, 10), (10, 10), (10, -10), (-10, -10)];
        let rings = projection.project_feature(&[&outer, &hole], 10);
        assert_eq!(rings.len(), 2);
        assert!(rings.iter().all(|ring| ring.first() == ring.last()));
        let outer_area = signed_area(&rings[0]).abs();
        let hole_area = signed_area(&rings[1]).abs();
        assert!(outer_area > hole_area);
    }

    // -- PattersonProjection: the places-root World-tier map ------------

    #[test]
    fn patterson_y_matches_the_published_polynomial() {
        let phi: f64 = 0.5;
        let expected = PATTERSON_K1 * phi
            + PATTERSON_K2 * phi.powi(5)
            + PATTERSON_K3 * phi.powi(7)
            + PATTERSON_K4 * phi.powi(9);
        assert!((patterson_y(phi) - expected).abs() < 1e-12);
        // The pole's y, as published with the projection.
        assert!((patterson_y(std::f64::consts::FRAC_PI_2) - 1.790857183).abs() < 1e-9);
        // Odd function: patterson_y(-phi) == -patterson_y(phi).
        assert!((patterson_y(-phi) + patterson_y(phi)).abs() < 1e-12);
        assert_eq!(patterson_y(0.0), 0.0);
    }

    #[test]
    fn patterson_projection_centers_the_prime_meridian_and_equator() {
        let point = ProjectedPoint::new(0.0, 0.0).unwrap();
        // A single equatorial point stays Wide, not World; force World via
        // a companion point beyond the polar limit so this exercises the
        // actual Patterson path Projection::new selects for World frames.
        let polar = ProjectedPoint::new(0.0, 89.0).unwrap();
        let world_frame =
            Frame::from_points(&[point, polar], [Precision::Exact, Precision::Exact].into_iter())
                .unwrap();
        assert_eq!(world_frame.tier, FrameTier::World);
        let projection = Projection::new(&world_frame);
        let (x, y) = projection.project(point).unwrap();
        assert!((x - VIEWBOX_WIDTH / 2.0).abs() < 0.5);
        assert!((y - VIEWBOX_HEIGHT / 2.0).abs() < 0.5);
    }

    #[test]
    fn world_map_fills_the_viewbox_height_and_crops_the_sides() {
        let polar = ProjectedPoint::new(0.0, 89.0).unwrap();
        let frame = Frame::from_points(&[polar], [Precision::Exact].into_iter()).unwrap();
        assert_eq!(frame.tier, FrameTier::World);
        let projection = Projection::new(&frame);
        let project = |longitude, latitude| {
            projection.project(ProjectedPoint::new(longitude, latitude).unwrap()).unwrap()
        };
        assert!(project(0.0, 90.0).1.abs() < 1e-9, "north pole at {}", project(0.0, 90.0).1);
        assert!((project(0.0, -90.0).1 - VIEWBOX_HEIGHT).abs() < 1e-9);
        assert!(project(-180.0, 0.0).0 < 0.0 && project(179.999, 0.0).0 > VIEWBOX_WIDTH);
    }

    #[test]
    fn world_map_crop_slides_toward_places_near_the_antimeridian() {
        let points = [
            ProjectedPoint::new(174.8, -41.3).unwrap(),
            ProjectedPoint::new(170.0, 89.0).unwrap(),
        ];
        let frame =
            Frame::from_points(&points, [Precision::Exact, Precision::Exact].into_iter()).unwrap();
        assert_eq!(frame.tier, FrameTier::World);
        let (x, _) = Projection::new(&frame).project(points[0]).unwrap();
        assert!((0.0..=VIEWBOX_WIDTH).contains(&x), "a place at 174.8E is cropped off at x={x}");
    }

    /// A guard for `world_viewbox_width`/`full_extent` (added for the
    /// places explorer's shared world SVG): a page's own World-tier map's
    /// drawn RINGS — not a raw, unclipped `project()` point query — must
    /// keep cropping to the fixed `VIEWBOX_WIDTH` canvas's clip bounds.
    /// Only `Projection::new_world_full_extent` widens the canvas and its
    /// clip bounds together.
    #[test]
    fn per_page_world_map_still_crops_to_the_fixed_viewbox_width() {
        let polar = ProjectedPoint::new(0.0, 89.0).unwrap();
        let frame = Frame::from_points(&[polar], [Precision::Exact].into_iter()).unwrap();
        assert_eq!(frame.tier, FrameTier::World);
        let projection = Projection::new(&frame);
        // A ring reaching the far eastern edge of the world (170E..179.9E):
        // unclipped it would project well past CLIP_MAX_X.
        let ring = projection.project_ring(&[(1700, -20), (1799, -20), (1799, 20), (1700, 20), (1700, -20)], 10);
        let max_x = ring.iter().flatten().map(|point| point.0).fold(f64::NEG_INFINITY, f64::max);
        assert!(
            max_x <= CLIP_MAX_X + 1e-6,
            "a page's world map's clipped ring reached x={max_x}, past the fixed CLIP_MAX_X={CLIP_MAX_X}"
        );
        assert!(
            world_viewbox_width() > CLIP_MAX_X + 50.0,
            "the explorer's full extent should be meaningfully wider than the cropped canvas's clip bound"
        );
    }

    /// The world tier's Antarctic rings reach every longitude: the
    /// generator closes each one along the seam and the pole, and the
    /// projected ring must keep all of it, not only the sector between its
    /// first vertex and the antimeridian.
    #[test]
    fn world_map_keeps_every_longitude_of_antarctica() {
        let pack = super::super::embedded().unwrap();
        let polar = ProjectedPoint::new(0.0, 89.0).unwrap();
        let frame = Frame::from_points(&[polar], [Precision::Exact].into_iter()).unwrap();
        let projection = Projection::new(&frame);
        let quantisation = pack.header.quantisation;
        let land = pack.tiers[0].layers.iter().find(|layer| layer.id == 2).unwrap();
        let antarctica = land
            .features
            .iter()
            .flat_map(|feature| feature.parts.iter())
            .find(|part| part.iter().any(|&(_, latitude)| latitude <= -89 * quantisation as i32))
            .expect("the world tier's land reaches the south pole");
        let rings = projection.project_ring(antarctica, quantisation);
        assert_eq!(rings.len(), 1);
        let min_x = rings[0].iter().map(|point| point.0).fold(f64::INFINITY, f64::min);
        let max_x = rings[0].iter().map(|point| point.0).fold(f64::NEG_INFINITY, f64::max);
        assert!(
            min_x <= 0.0 && max_x >= VIEWBOX_WIDTH,
            "Antarctica spans only [{min_x}, {max_x}] of the {VIEWBOX_WIDTH}-wide map"
        );
    }

    /// Projecting raw coordinates is only exact because the generator cuts
    /// every ring, in both tiers, at the antimeridian. The one place a ring
    /// may jump across the map is along a pole, where both ends are the
    /// same point.
    #[test]
    fn pack_rings_cross_the_antimeridian_only_along_a_pole() {
        let pack = super::super::embedded().unwrap();
        let near_pole = (89.5 * f64::from(pack.header.quantisation)) as i32;
        let half_turn = 180 * pack.header.quantisation as i32;
        for layer in pack.tiers.iter().flat_map(|tier| tier.layers.iter()) {
            for part in layer.features.iter().flat_map(|feature| feature.parts.iter()) {
                for pair in part.windows(2) {
                    if (pair[1].0 - pair[0].0).abs() > half_turn {
                        assert!(
                            pair[0].1.abs() >= near_pole && pair[1].1.abs() >= near_pole,
                            "layer {} jumps across the map away from a pole: {:?}",
                            layer.id,
                            pair
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn append_horizon_arc_closes_a_near_duplicate_crossing_tight_not_the_long_way() {
        let center = (VIEWBOX_WIDTH / 2.0, VIEWBOX_HEIGHT / 2.0);
        let radius = 60.0;
        let angle = 1.234_f64;
        let start = (center.0 + radius * angle.cos(), center.1 + radius * angle.sin());
        for noise in [1e-9, -1e-9] {
            let end_angle = angle + noise;
            let end = (
                center.0 + radius * end_angle.cos(),
                center.1 + radius * end_angle.sin(),
            );
            let mut ring = vec![start];
            append_horizon_arc(&mut ring, center, start, end, radius);
            for &(x, y) in &ring {
                let point_angle = (y - center.1).atan2(x - center.0);
                let delta = (point_angle - angle + std::f64::consts::PI)
                    .rem_euclid(2.0 * std::f64::consts::PI)
                    - std::f64::consts::PI;
                assert!(
                    delta.abs() < 0.01,
                    "noise={noise}: closing a near-duplicate crossing produced a point \
                     {delta:.6} radians from the shared angle — a near-full circle loop \
                     instead of a no-op close"
                );
            }
        }
    }

    /// A run's own entry/exit gap, however far apart, always closes with
    /// the shorter of the two possible arcs — confirmed on real pack data
    /// (see the commit introducing this test): a landmass feature whose
    /// winding-selected "major" arc closed to 99% of the horizon disc
    /// instead of the actual sliver its entry/exit gap spans.
    #[test]
    fn horizon_arc_always_takes_the_shorter_of_the_two_boundary_arcs() {
        let center = (VIEWBOX_WIDTH / 2.0, VIEWBOX_HEIGHT / 2.0);
        let radius = 60.0;
        for (start_angle, end_angle) in [(-0.6_f64, 0.6_f64), (0.6, -0.6), (3.0, -3.0)] {
            let start = (
                center.0 + radius * start_angle.cos(),
                center.1 + radius * start_angle.sin(),
            );
            let end = (
                center.0 + radius * end_angle.cos(),
                center.1 + radius * end_angle.sin(),
            );
            let mut ring = vec![start];
            append_horizon_arc(&mut ring, center, start, end, radius);
            for &(x, y) in &ring {
                let radius_here = (x - center.0).hypot(y - center.1);
                assert!((radius_here - radius).abs() < 0.5, "arc point left the circle");
            }
            let swept: f64 = ring
                .windows(2)
                .map(|pair| {
                    let a = (pair[0].1 - center.1).atan2(pair[0].0 - center.0);
                    let b = (pair[1].1 - center.1).atan2(pair[1].0 - center.0);
                    ((b - a + std::f64::consts::PI).rem_euclid(2.0 * std::f64::consts::PI)
                        - std::f64::consts::PI)
                        .abs()
                })
                .sum();
            assert!(
                swept < std::f64::consts::PI + 0.01,
                "arc from {start_angle} to {end_angle} swept {swept} radians, more than the \
                 minor arc's at-most-pi"
            );
        }
    }

    #[test]
    fn clip_closed_ring_closes_each_sutherland_hodgman_result() {
        let clipped = clip_closed_ring(
            &[(-100.0, 100.0), (820.0, 100.0), (820.0, 380.0), (-100.0, 380.0)],
            0.0,
            720.0,
            0.0,
            480.0,
        );
        assert_eq!(clipped.first(), clipped.last());
        assert_eq!(clipped.len(), 5);
        assert!(clipped
            .iter()
            .all(|(x, y)| (0.0..=720.0).contains(x) && (0.0..=480.0).contains(y)));
    }

    // -- TileSelection ----------------------------------------------------

    #[test]
    fn tile_rows_follow_the_generator_south_to_north_index() {
        assert_eq!(tile_y(-90.0), 0);
        assert_eq!(tile_y(-80.0), 1);
        assert_eq!(tile_y(0.0), 9);
        assert_eq!(tile_y(80.0), 17);
        assert_eq!(tile_y(90.0), 17);
    }

    #[test]
    fn dateline_frame_selects_tiles_on_both_longitude_edges() {
        let pack = super::super::embedded().unwrap();
        let points = [
            ProjectedPoint::new(179.0, 0.0).unwrap(),
            ProjectedPoint::new(-179.0, 0.0).unwrap(),
        ];
        let frame =
            Frame::from_points(&points, [Precision::Exact, Precision::Exact].into_iter()).unwrap();
        let selection = TileSelection::for_frame(&pack, &frame);
        assert!(selection.tiles.iter().any(|(x, _)| *x == 0));
        assert!(selection.tiles.iter().any(|(x, _)| *x == 35));
        assert!(selection.tiles.iter().all(|(x, _)| *x == 0 || *x == 35));
    }

    /// A 10-degree frame at Beirut's latitude fits the 3:2 canvas by height
    /// and so shows about 13 degrees of longitude, reaching past 30E into
    /// tile column 20. Features there are on screen and must be selected.
    #[test]
    fn tile_selection_covers_the_visible_canvas_not_just_the_frame() {
        let pack = super::super::embedded().unwrap();
        let point = ProjectedPoint::new(35.5, 33.89).unwrap();
        let frame = Frame::from_points(&[point], [Precision::Exact].into_iter()).unwrap();
        let (west, ..) = FlatProjection::new(&frame).visible_bounds();
        assert!(west < 30.0, "visible canvas starts at {west}");
        let selection = TileSelection::for_frame(&pack, &frame);
        assert!(
            selection.tiles.contains(&(tile_x(west), 12)),
            "tile column {} is on screen but not selected: {:?}",
            tile_x(west),
            selection.tiles
        );
    }

    #[test]
    fn world_frames_draw_every_fine_feature() {
        let pack = super::super::embedded().unwrap();
        let points = [ProjectedPoint::new(0.0, 85.1).unwrap()];
        let frame = Frame::from_points(&points, [Precision::Exact].into_iter()).unwrap();
        let selection = TileSelection::for_frame(&pack, &frame);
        let expected: usize = pack.tiers[1]
            .layers
            .iter()
            .map(|layer| layer.feature_count as usize)
            .sum();
        assert_eq!(selection.features(&pack).len(), expected);
    }

    #[test]
    fn tile_references_are_global_across_layer_boundaries() {
        let pack = super::super::embedded().unwrap();
        let first_layer_count = pack.tiers[1].layers[0].feature_count;
        let tile = pack
            .tiles
            .iter()
            .find(|tile| tile.features.iter().any(|id| *id >= first_layer_count))
            .unwrap();
        let selection = TileSelection {
            tiles: vec![(tile.x, tile.y)],
        };
        let actual = selection.features(&pack);
        let mut offset = 0u32;
        for layer in &pack.tiers[1].layers {
            for (index, _) in layer.features.iter().enumerate() {
                if tile.features.contains(&(offset + index as u32)) {
                    assert!(actual.iter().any(|(id, feature)| *id == layer.id
                        && std::ptr::eq(*feature, &layer.features[index])));
                }
            }
            offset += layer.feature_count;
        }
    }

    // -- Real-pack shape regression: Beirut and Bangkok ------------------

    /// A Fiji-shaped frame straddles the antimeridian, which is also the
    /// boundary between pack tiles x=35 and x=0. Bathymetry band features
    /// there are pre-split at that tile edge in the source pack (visible
    /// as a rendered seam: two same-depth "-6000" rings whose bounding
    /// boxes meet exactly at the boundary's screen x, one ending there and
    /// the next starting there). That split is a pack-data characteristic
    /// this crate only renders, not a defect in the geometry pipeline —
    /// but the geometry pipeline is exactly what would be at fault if the
    /// two pre-split pieces failed to *meet*, leaving a gap of open water
    /// with no band coverage on either side of the seam. This pins that
    /// they still abut cleanly for the one depth this frame has data on
    /// both sides of.
    #[test]
    fn tile_boundary_seafloor_bands_meet_without_a_gap() {
        let pack = super::super::embedded().unwrap();
        let point = ProjectedPoint::new(178.4419, -18.1416).unwrap();
        let frame = Frame::from_points(&[point], [Precision::City].into_iter()).unwrap();
        let projection = Projection::new(&frame);
        let selection = TileSelection::for_frame(&pack, &frame);
        let quantisation = pack.header.quantisation;

        let target_band = -6000_i16;
        let mut rings: Vec<Vec<(f64, f64)>> = Vec::new();
        for (layer_id, feature) in selection.features(&pack) {
            if layer_id != 10 || feature.band != target_band {
                continue;
            }
            let parts: Vec<&[(i32, i32)]> = feature.parts.iter().map(Vec::as_slice).collect();
            rings.extend(projection.project_feature(&parts, quantisation));
        }
        assert!(!rings.is_empty(), "expected some -6000 seafloor coverage in the Fiji frame");

        // The tile boundary between x=35 (170E-180) and x=0 (180-170W) is
        // the antimeridian itself.
        let boundary_x = projection
            .project(ProjectedPoint::new(180.0, frame.center_latitude).unwrap())
            .unwrap()
            .0;
        let west_rings: Vec<&Vec<(f64, f64)>> = rings
            .iter()
            .filter(|ring| ring.iter().any(|p| (p.0 - boundary_x).abs() < 1.0))
            .collect();
        assert!(
            !west_rings.is_empty(),
            "no -6000 ring touches the tile boundary at x={boundary_x}; the frame or pack \
             changed shape — update this test's premise rather than deleting the check"
        );
        // A fixed +/-2px probe would false-positive on an ordinary polygon
        // vertex near the boundary (coverage can legitimately start or end
        // within a couple of pixels of x=boundary_x without any gap). Instead,
        // bisect for the true transition point on each side of every sampled
        // y and assert *that* is within a tight tolerance of the boundary —
        // this only fails for an actual missing strip between the two
        // pre-split pieces, not for their polygons simply having a vertex
        // near the seam.
        for ring in &west_rings {
            let ys: Vec<f64> = ring
                .iter()
                .filter(|p| (p.0 - boundary_x).abs() < 1.0)
                .map(|p| p.1)
                .collect();
            for &y in &ys {
                let west_covered = point_in_any_ring(&rings, (boundary_x - 2.0, y));
                let east_covered = point_in_any_ring(&rings, (boundary_x + 2.0, y));
                if west_covered == east_covered {
                    continue;
                }
                let (mut lo, mut hi) = (boundary_x - 2.0, boundary_x + 2.0);
                let lo_covered = point_in_any_ring(&rings, (lo, y));
                for _ in 0..30 {
                    let mid = (lo + hi) / 2.0;
                    if point_in_any_ring(&rings, (mid, y)) == lo_covered {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                let gap = (hi - lo).abs();
                assert!(
                    gap < 0.01,
                    "at y={y}, coverage transitions at x={lo:.3}..{hi:.3} ({gap:.3}px wide), \
                     {:.3}px from the tile boundary at x={boundary_x:.3} — a real gap between \
                     the pre-split pieces, not just a polygon vertex near the seam",
                    (lo - boundary_x).abs()
                );
            }
        }
    }

    /// A close-up that straddles the antimeridian near a pole: centred on
    /// 180E, 83S it shows Antarctica from about 138E to 138W. Antarctica's
    /// ring runs along the pole and the antimeridian, and its land must
    /// still show on both sides of the dateline.
    #[test]
    fn halves_cut_at_the_antimeridian_overlap_instead_of_abutting() {
        let point = ProjectedPoint::new(179.9, -16.8).unwrap();
        let frame = Frame::from_points(&[point], [Precision::Exact].into_iter()).unwrap();
        let projection = Projection::new(&frame);
        let west = projection.project_ring(&[(1790, -170), (1800, -170), (1800, -160), (1790, -160), (1790, -170)], 10);
        let east = projection.project_ring(&[(-1800, -170), (-1790, -170), (-1790, -160), (-1800, -160), (-1800, -170)], 10);
        let west_edge = west[0].iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
        let east_edge = east[0].iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
        assert!(west_edge - east_edge >= 0.99, "halves overlap by {} px", west_edge - east_edge);
    }

    #[test]
    fn a_polar_close_up_across_the_antimeridian_keeps_its_land() {
        let pack = super::super::embedded().unwrap();
        let (frame, rings) = land_rings_for(&pack, 180.0, -83.0);
        let projection = Projection::new(&frame);
        for (longitude, latitude) in [(165.0, -85.0), (-165.0, -85.0)] {
            let point = projection.project(ProjectedPoint::new(longitude, latitude).unwrap()).unwrap();
            assert!((0.0..=VIEWBOX_WIDTH).contains(&point.0) && (0.0..=VIEWBOX_HEIGHT).contains(&point.1));
            assert!(point_in_any_ring(&rings, point), "{longitude},{latitude} at {point:?} is not land");
        }
    }

    fn land_rings_for(pack: &Pack, longitude: f64, latitude: f64) -> (Frame, Vec<Vec<(f64, f64)>>) {
        let point = ProjectedPoint::new(longitude, latitude).unwrap();
        let frame = Frame::from_points(&[point], [Precision::City].into_iter()).unwrap();
        assert_eq!(frame.tier, FrameTier::Local);
        let projection = Projection::new(&frame);
        let selection = TileSelection::for_frame(pack, &frame);
        let quantisation = pack.header.quantisation;
        let mut rings = Vec::new();
        for (layer_id, feature) in selection.features(pack) {
            if layer_id != 2 {
                continue;
            }
            let parts: Vec<&[(i32, i32)]> = feature.parts.iter().map(Vec::as_slice).collect();
            rings.extend(projection.project_feature(&parts, quantisation));
        }
        (frame, rings)
    }

    fn point_in_any_ring(rings: &[Vec<(f64, f64)>], point: (f64, f64)) -> bool {
        rings.iter().any(|ring| point_in_ring(ring, point))
    }

    fn point_in_ring(ring: &[(f64, f64)], point: (f64, f64)) -> bool {
        let mut inside = false;
        for pair in ring.windows(2) {
            let (x1, y1) = pair[0];
            let (x2, y2) = pair[1];
            if (y1 > point.1) != (y2 > point.1) {
                let x_at_y = x1 + (point.1 - y1) / (y2 - y1) * (x2 - x1);
                if point.0 < x_at_y {
                    inside = !inside;
                }
            }
        }
        inside
    }

    #[test]
    fn coastal_local_frames_never_paint_the_full_clip_rectangle_as_land() {
        let pack = super::super::embedded().unwrap();
        for (longitude, latitude) in [(35.495, 33.888), (100.502, 13.756)] {
            let (_, rings) = land_rings_for(&pack, longitude, latitude);
            let clip_min_x = -CLIP_MARGIN;
            let clip_max_x = VIEWBOX_WIDTH + CLIP_MARGIN;
            let clip_min_y = -CLIP_MARGIN;
            let clip_max_y = VIEWBOX_HEIGHT + CLIP_MARGIN;
            for ring in &rings {
                let min_x = ring.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
                let max_x = ring.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
                let min_y = ring.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
                let max_y = ring.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
                let is_full_clip_rectangle = ring.len() <= 5
                    && (min_x - clip_min_x).abs() < 0.5
                    && (max_x - clip_max_x).abs() < 0.5
                    && (min_y - clip_min_y).abs() < 0.5
                    && (max_y - clip_max_y).abs() < 0.5;
                assert!(
                    !is_full_clip_rectangle,
                    "({longitude}, {latitude}): a land ring degenerated into the full clip \
                     rectangle {ring:?}"
                );
            }
        }
    }

    /// Beirut is on the Mediterranean coast: a point a few tenths of a
    /// degree out to sea must not be inside any emitted land ring. Unlike
    /// an area-fraction budget, this pins one concrete, independently
    /// verifiable fact about the coastline's shape (west of Beirut is
    /// open water) that a land-fill/coastline mismatch would violate
    /// directly, regardless of how the rest of the frame is covered.
    #[test]
    fn beirut_frame_leaves_the_open_mediterranean_uncovered_by_land() {
        let pack = super::super::embedded().unwrap();
        let (frame, rings) = land_rings_for(&pack, 35.495, 33.888);
        let sea_point = ProjectedPoint::new(34.0, 34.0).unwrap();
        assert!(frame.contains(sea_point), "34.0E 34.0N should be within the Beirut frame");
        let projection = Projection::new(&frame);
        let (x, y) = projection.project(sea_point).unwrap();
        assert!(
            !point_in_any_ring(&rings, (x, y)),
            "a known open-sea point (34.0E 34.0N) fell inside a land ring at ({x}, {y})"
        );
    }

    #[test]
    fn bangkok_frame_land_area_stays_within_a_plausible_band() {
        let pack = super::super::embedded().unwrap();
        let (_, rings) = land_rings_for(&pack, 100.502, 13.756);
        let total_area: f64 = rings.iter().map(|ring| signed_area(ring).abs()).sum();
        let frame_area = VIEWBOX_WIDTH * VIEWBOX_HEIGHT;
        let fraction = total_area / frame_area;
        assert!(
            fraction < 0.85,
            "Bangkok land covers {:.0}% of the frame; the Gulf of Thailand and its coastline \
             should leave a clearly visible fraction as open water",
            fraction * 100.0
        );
    }
}
