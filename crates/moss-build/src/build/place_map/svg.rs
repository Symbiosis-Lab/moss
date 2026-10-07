//! Deterministic, self-contained SVG emission for a place map.
//!
//! The emitter deliberately knows nothing about pages, configuration, or CSS
//! files.  Its only inputs are the immutable decoded pack and a resolved map
//! target.  That makes the byte output suitable for page snapshots and keeps
//! the geometry/privacy boundary in [`super::context`] and [`super::geometry`].

use std::fmt::Write;

use sha2::{Digest, Sha256};

mod path;
use path::{serialize_path, snap, BY_AREA, FINE};
mod text;
use text::{precision_name, xml_escape};
// Re-exported (via place_map.rs) so context.rs's locator can derive a
// profile from every resolved place's precision, the same rule emit_svg
// already applies to the full/aggregate map — see the doc comment on
// render_locator's fix for what picking only the first place's precision
// broke.
pub(crate) use text::precision_rank;
mod locator;
use locator::{keep_alternate_bands, LOCATOR_DISPLAY_SCALE};
pub use locator::{
    emit_locator, emit_locator_svg, LocatorProfile, LocatorSafetyError, LocatorSvg,
    LOCATOR_Q11_BROTLI_LIMIT, LOCATOR_RAW_SAFETY_LIMIT, WIDE_LOCATOR_Q11_BROTLI_LIMIT,
};
mod river;
mod palette;
use palette::{band_ladder, band_tint, relief_height_grey};
mod route;
use route::{draw_badges, draw_globe_line, draw_line};
// Re-exported (via place_map.rs) for context.rs's privacy gate; see route.rs.
pub(crate) use route::{route_blocked_diagnostic, route_precision_gate};

use super::geometry::{marker_radius, FrameTier, TILE_K, TILE_STROKE_BASE_K, ProjectedPoint, Projection, TileSelection, REGION_FADE_DEGREES, REGION_FADE_MIN_RADIUS};
use super::globe::{globe_line, globe_marker, globe_rings};
use super::{Feature, Pack, PlaceMapContext, PlaceMapTarget, ResolvedPlace};
use crate::vault::places::Precision;

pub const SVG_WIDTH: u32 = 720;
pub const SVG_HEIGHT: u32 = 480;

const GLOBE_X: f64 = 584.0;
const GLOBE_Y: f64 = 8.0;
const GLOBE_SIZE: f64 = 128.0;
const GLOBE_CENTER_X: f64 = GLOBE_X + GLOBE_SIZE / 2.0;
const GLOBE_CENTER_Y: f64 = GLOBE_Y + GLOBE_SIZE / 2.0;
const GLOBE_RADIUS: f64 = 63.0;

/// The small amount of caller-owned identity needed to make every SVG ID
/// stable without exposing the page path in the output.
#[derive(Debug, Clone, Copy)]
pub struct SvgMapOptions<'a> {
    pub page_path: &'a str,
    pub ordinal: usize,
    pub location: &'a str,
    pub precision: Precision,
    pub href: Option<&'a str>,
}

impl<'a> SvgMapOptions<'a> {
    pub fn new(
        page_path: &'a str,
        ordinal: usize,
        location: &'a str,
        precision: Precision,
    ) -> Self {
        Self {
            page_path,
            ordinal,
            location,
            precision,
            href: None,
        }
    }
}

/// Emit one complete semantic map figure using the stable page/ordinal ID
/// namespace.  This convenience form derives the label and broadest privacy
/// value from the target; callers that need a link use
/// [`emit_svg_with_options`].
pub fn emit_svg(
    context: &PlaceMapContext,
    target: &PlaceMapTarget,
    page_path: &str,
    ordinal: usize,
) -> String {
    let location = target
        .aggregate_name
        .as_deref()
        .or_else(|| target.places.first().map(|place| place.display.as_str()))
        .unwrap_or("");
    let precision = target
        .places
        .iter()
        .map(|place| place.precision)
        .max_by_key(precision_rank)
        .unwrap_or(Precision::Country);
    emit_svg_with_options(
        context,
        target,
        SvgMapOptions::new(page_path, ordinal, location, precision),
    )
}

/// Emit one map figure with an explicit accessible label/link.
pub fn emit_svg_with_options(
    context: &PlaceMapContext,
    target: &PlaceMapTarget,
    options: SvgMapOptions<'_>,
) -> String {
    emit_svg_with_mode(context, target, options, false)
}

fn emit_svg_with_mode(
    context: &PlaceMapContext,
    target: &PlaceMapTarget,
    options: SvgMapOptions<'_>,
    compact: bool,
) -> String {
    let ids = Ids::new(options.page_path, options.ordinal);
    // A listing map's places each keep their own precision, so its label
    // names only what it lists.
    let label = if target.aggregate_name.is_some() {
        format!("Map of {}", options.location)
    } else if options.location.is_empty() {
        format!("Place map, {} precision", precision_name(options.precision))
    } else {
        format!(
            "Map of {}, {} precision",
            options.location,
            precision_name(options.precision)
        )
    };
    let projection = target.frame.as_ref().map(Projection::new);
    let wide = projection.as_ref().is_some_and(Projection::is_wide);
    let wide_locator = compact && wide;
    let mut writer = Writer {
        output: String::with_capacity(32 * 1024),
        ids: &ids,
        height_uses: Vec::new(),
        land_paths: Vec::new(),
        has_href: false,
        effect_scale: projection.as_ref().map_or(1.0, Projection::effect_scale),
        detail: if wide_locator { LOCATOR_DISPLAY_SCALE } else { 1.0 },
        locator_profile: compact.then_some(match options.precision {
            Precision::Exact | Precision::City => LocatorProfile::ExactCity,
            Precision::Region => LocatorProfile::Region,
            Precision::Country => LocatorProfile::Country,
        }),
        canvas_width: f64::from(SVG_WIDTH),
        canvas_height: f64::from(SVG_HEIGHT),
        full_extent: false,
        tile: false,
    };
    writer.open_figure(
        target,
        options.location,
        options.precision,
        &label,
        options.href,
    );
    render_svg(&mut writer, context, target, true);
    writer.close_figure();
    writer.output
}

/// `render_svg`'s projection: the explorer world SVG's full-extent
/// Patterson for `writer.full_extent`, else the target's own per-frame one
/// — never inferred from `canvas_width`, which a same-width asset could share.
fn choose_projection(writer: &Writer<'_>, target: &PlaceMapTarget) -> Option<Projection> {
    if writer.full_extent {
        Some(Projection::new_world_full_extent())
    } else {
        target.frame.as_ref().map(Projection::new)
    }
}

/// Render one complete `<svg width="720" height="480" ...>...</svg>`: defs,
/// water, the physical layers in the approved paint order, lighting, a
/// marker group (left empty when `target.places` carries none — see
/// `markers()`), and the globe inset when `draw_globe` and the frame is not
/// already the world tier (the globe shows where a frame sits in the world;
/// a world map is its own answer).
///
/// Shared by the per-page figure (`emit_svg_with_mode`, which wraps this in
/// `<figure>`) and `explorer::emit_world_svg`/`emit_tile_svg`, which call it
/// directly: a page-independent asset fetched or `<img src>`-referenced at
/// runtime has no page-relative accessible label to carry, so it skips the
/// figure wrapper rather than carrying an empty one.
pub(super) fn render_svg(
    writer: &mut Writer<'_>,
    context: &PlaceMapContext,
    target: &PlaceMapTarget,
    draw_globe: bool,
) {
    writer.tile = target.frame.as_ref().is_some_and(|frame| frame.tier.is_tile());
    writer.open_svg();
    writer.defs();
    writer.water();

    let projection = choose_projection(writer, target);
    // A `Tile` frame is drawn in the world's own Patterson projection at
    // `TILE_K` times its scale (`PattersonProjection::for_tile`), so its
    // projection `scale` reads as geographically "wide" under
    // `Projection::is_wide()`'s ratio — that heuristic conflates a
    // projection's own internal scale constant with geographic extent, a
    // conflation that happens to hold for `FlatProjection` and the
    // per-page/full-extent Patterson projections but not for a
    // `TILE_K`-multiplied one. A tile is always exactly a 10-degree cell —
    // the same span a `Local` frame never trips `is_wide` for — so it never
    // gets the world map's rank/layer thinning either.
    let wide = !writer.tile && projection.as_ref().is_some_and(Projection::is_wide);
    let wide_locator = writer.locator_profile.is_some() && wide;
    let grouped = target.frame.as_ref().map(|frame| {
        let selected = TileSelection::for_frame(context.pack(), frame);
        let mut grouped = grouped_features(context.pack(), &selected);
        if wide {
            keep_world_scale_layers(&mut grouped);
        }
        if wide_locator {
            keep_alternate_bands(&mut grouped);
        }
        grouped
    });
    let quantisation = context.pack().header.quantisation;
    // A locator draws the same layers as a full map. Only a country-level
    // locator, whose frame spans half the globe, keeps to land alone so it
    // stays within the locator byte budget.
    let sparse = writer.locator_profile == Some(LocatorProfile::Country);
    match (projection.as_ref(), grouped.as_ref()) {
        (Some(projection), Some(grouped)) if sparse => {
            writer.emit_sparse_layers(quantisation, projection, grouped)
        }
        (Some(projection), Some(grouped)) => writer.emit_layers(quantisation, projection, grouped),
        // A missing coordinate is deliberate no-map input. Keeping a valid
        // empty figure here avoids inventing a point or leaking source data.
        _ => writer.empty_layers(),
    }
    writer.lighting();
    match (projection.as_ref(), grouped.as_ref()) {
        (Some(projection), Some(grouped)) if !sparse => {
            writer.emit_surface_layers(quantisation, projection, grouped)
        }
        _ => writer.empty_layers_after_lighting(),
    }
    draw_line(writer, target, projection.as_ref());
    writer.markers(target);
    draw_badges(writer, target, projection.as_ref());
    if draw_globe && target.frame.as_ref().map_or(true, |frame| frame.tier != FrameTier::World) {
        writer.globe(context, target);
    }
    writer.close_svg_tag();
}

#[derive(Debug, Clone)]
pub(super) struct Ids {
    base: String,
}

impl Ids {
    fn new(page_path: &str, ordinal: usize) -> Self {
        let mut digest = Sha256::new();
        digest.update(page_path.as_bytes());
        digest.update([0]);
        digest.update(ordinal.to_string().as_bytes());
        let digest: [u8; 32] = digest.finalize().into();
        let mut hex = String::with_capacity(64);
        for byte in digest {
            write!(hex, "{byte:02x}").expect("writing to String cannot fail");
        }
        Self {
            base: format!("moss-place-map-{hex}"),
        }
    }

    /// A fixed, human-readable seed in place of a page-derived hash: for
    /// `explorer`'s page-independent world/tile SVGs there is no page path
    /// to hide (unlike the per-page figure, see `Ids::new`'s doc), and the
    /// whole point is a STABLE id shared by every build of the same pack.
    pub(super) fn for_seed(seed: &str) -> Self {
        Self {
            base: format!("moss-place-map-{seed}"),
        }
    }

    fn get(&self, name: &str) -> String {
        format!("{}-{name}", self.base)
    }
}

pub(super) struct Writer<'a> {
    pub(super) output: String,
    pub(super) ids: &'a Ids,
    pub(super) height_uses: Vec<String>,
    pub(super) land_paths: Vec<String>,
    pub(super) has_href: bool,
    pub(super) effect_scale: f64,
    /// How many times smaller than its viewBox the map is shown, which
    /// scales the render-time simplification.
    pub(super) detail: f64,
    pub(super) locator_profile: Option<LocatorProfile>,
    /// This figure's own canvas width, in viewBox units: `SVG_WIDTH` for
    /// every page figure, the wider `geometry::world_viewbox_width()` for
    /// the explorer's shared world SVG, or a tile's own `TILE_K`-scaled
    /// cell width for a regional tile (`explorer::emit_tile_svg`, via
    /// `Projection::canvas_size`).
    pub(super) canvas_width: f64,
    /// This figure's own canvas height, in viewBox units: `SVG_HEIGHT` for
    /// every page figure and the world SVG, or a tile's own `TILE_K`-scaled
    /// cell height for a regional tile — shorter than `SVG_HEIGHT` away
    /// from the equator, since Patterson itself compresses a high-latitude
    /// cell vertically (see `PattersonProjection::for_tile`).
    pub(super) canvas_height: f64,
    /// Whether `render_svg` draws with the explorer world SVG's full-extent
    /// Patterson projection rather than the target's own per-frame one. Set
    /// explicitly by each caller — never inferred from `canvas_width`, which
    /// is only a width.
    pub(super) full_extent: bool,
    /// The figure is one of the explorer's regional detail tiles
    /// (`FrameTier::Tile`), set by `render_svg`. The tile carries the same
    /// terrain lighting and band shadows as the world; the runtime bakes
    /// those effects into a fixed-pixel canvas before camera transforms.
    pub(super) tile: bool,
}

impl<'a> Writer<'a> {
    /// Line widths are drawn in canvas units, so a canvas drawn at a finer
    /// quantum (a tile, `TILE_K`) multiplies them to keep the same weight
    /// on screen. 1 for every other figure.
    pub(super) fn stroke_scale(&self) -> f64 {
        if self.tile { TILE_K / TILE_STROKE_BASE_K } else { 1.0 }
    }

    /// The shared literal for a page-independent places-explorer base-map
    /// asset (`explorer::emit_world_svg`/`emit_tile_svg`): full capacity, no
    /// globe inset, no href, no locator, undetailed (never shown smaller
    /// than its own viewBox) — the three emitters differ only in `ids`,
    /// `canvas_width`/`canvas_height`, and `full_extent`.
    pub(super) fn for_explorer_asset(ids: &'a Ids, canvas_width: f64, canvas_height: f64, full_extent: bool) -> Self {
        Self {
            output: String::with_capacity(32 * 1024),
            ids,
            height_uses: Vec::new(),
            land_paths: Vec::new(),
            has_href: false,
            effect_scale: 1.0,
            detail: 1.0,
            locator_profile: None,
            canvas_width,
            canvas_height,
            full_extent,
            tile: false,
        }
    }
}

/// A round dot of `radius` screen px at a viewBox point, drawn as a
/// zero-length stroke that does not scale with the SVG: sized in viewBox
/// units, the approved 4 px dot for a 720 px map came out at 1.5 px on a
/// floated locator.
fn dot(x: i32, y: i32, radius: f64, color: &str, attributes: &str) -> String {
    format!(
        "<path d=\"M{x} {y}h0\" stroke=\"{color}\" stroke-width=\"{}\" stroke-linecap=\"round\" vector-effect=\"non-scaling-stroke\"{attributes}/>",
        length(radius * 2.0)
    )
}

/// A viewBox length for an SVG attribute, to a thousandth of a pixel.
fn length(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// The narrowest a map is shown, in CSS px: a phone column is about 270
/// and a floated locator about 350; a map embedded narrower still can crop
/// an edge dot by a pixel or two.
const NARROWEST_MAP_PX: f64 = 240.0;

/// How many viewBox units a dot of `radius` screen px covers on the
/// narrowest map: a non-scaling stroke spans more of the viewBox the
/// narrower the map is shown, over 10 units for a 5 px casing on a phone.
fn dot_reach(radius: f64) -> f64 {
    radius * f64::from(SVG_WIDTH) / NARROWEST_MAP_PX
}

/// Pull a dot's centre in from the viewBox edge by its reach, so its
/// casing is drawn whole at any width a map is shown.
fn clamp_dot(point: (f64, f64), radius: f64) -> (f64, f64) {
    let reach = dot_reach(radius);
    (point.0.clamp(reach, f64::from(SVG_WIDTH) - reach), point.1.clamp(reach, f64::from(SVG_HEIGHT) - reach))
}

/// Pull a point inside a circle of `radius` round `center`, so a globe dot
/// near the horizon is not sliced by the globe's own clip-path.
fn clamp_to_circle(point: (f64, f64), center: (f64, f64), radius: f64) -> (f64, f64) {
    let (dx, dy) = (point.0 - center.0, point.1 - center.1);
    let distance = dx.hypot(dy);
    if distance <= radius || distance == 0.0 {
        point
    } else {
        let scale = radius / distance;
        (center.0 + dx * scale, center.1 + dy * scale)
    }
}

/// Natural Earth scalerank of the smallest river a world map still draws.
const WORLD_MAX_RIVER_RANK: i16 = 3;

/// On any frame wider than a local one (over 60 degrees) the river taper's
/// 0.5 px floor draws every tributary as a thread, reef lines read as
/// scratches across the ocean, and salt flats and built-up areas shrink to
/// specks a few pixels wide. Such a map keeps trunk rivers, lakes and ice,
/// as the approved world map does.
fn keep_world_scale_layers(grouped: &mut [Vec<&Feature>]) {
    grouped[4].retain(|feature| feature.band <= WORLD_MAX_RIVER_RANK);
    for layer in [6, 7, 8] {
        grouped[layer].clear();
    }
}

/// Whether any part of a projected ring falls inside the view, rather than
/// only in the clip margin around it. `width`/`height` are the figure's own
/// `canvas_width`/`canvas_height` (720x480 for every page figure, wider for
/// the explorer's full-extent world SVG, a cell's own smaller size for a
/// regional tile), never the fixed `SVG_WIDTH`/`SVG_HEIGHT` — a band beyond
/// those bounds on a differently-sized canvas is still on screen and must
/// not be culled here.
fn on_screen(ring: &[(f64, f64)], width: f64, height: f64) -> bool {
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    for &(x, y) in ring {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    min_x < width && max_x > 0.0 && min_y < height && max_y > 0.0
}

fn grouped_features<'a>(pack: &'a Pack, selected: &TileSelection) -> Vec<Vec<&'a Feature>> {
    let mut grouped: Vec<Vec<&Feature>> = (0..=10).map(|_| Vec::new()).collect();
    for (layer_id, feature) in selected.features(pack) {
        if let Some(layer) = grouped.get_mut(usize::from(layer_id)) {
            layer.push(feature);
        }
    }
    grouped
}

impl Writer<'_> {
    /// `data-map-tier` reports `FrameTier`, the span-based tier used for
    /// data selection (which tiles are in bounds, and whether the globe
    /// inset and Patterson projection apply). It is not the drawing
    /// treatment: that is chosen separately, by zoom, through
    /// `Projection::is_wide()`. A `local`-tagged frame can still cross
    /// `WIDE_ZOOM` and be drawn with the wide treatment — the attribute and
    /// the rendered detail can disagree, and both are correct.
    fn open_figure(
        &mut self,
        target: &PlaceMapTarget,
        location: &str,
        precision: Precision,
        label: &str,
        href: Option<&str>,
    ) {
        let tier = match target.tier() {
            FrameTier::Local => "local",
            FrameTier::Wide => "wide",
            FrameTier::World => "world",
            // Unreachable in practice: a `Tile` frame only ever reaches
            // `render_svg` directly (`explorer::emit_tile_svg`), which
            // skips `open_figure` entirely (see that function's own doc) —
            // kept here only so this match stays exhaustive.
            FrameTier::Tile => "tile",
        };
        write!(
            self.output,
            "<figure class=\"moss-place-map\" role=\"img\" aria-label=\"{}\" data-map-tier=\"{tier}\" data-map-precision=\"{}\" data-map-location=\"{}\">",
            xml_escape(label),
            precision_name(precision),
            xml_escape(location),
        )
        .expect("writing to String cannot fail");
        if let Some(href) = href {
            write!(
                self.output,
                "<a href=\"{}\" aria-label=\"{}\">",
                xml_escape(href),
                xml_escape(label)
            )
            .expect("writing to String cannot fail");
            self.has_href = true;
        }
    }

    fn open_svg(&mut self) {
        let width = length(self.canvas_width);
        let height = length(self.canvas_height);
        write!(
            self.output,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\" aria-hidden=\"true\" focusable=\"false\">",
        )
        .expect("writing to String cannot fail");
        // A tile is composited at a fractional size. WebKit clips the
        // stacked band polygons at the image edge one draw at a time, so the
        // last partly covered row blends every band over a lighter one and
        // reads as a light line. Isolating the whole drawing clips it once.
        if self.tile {
            self.output.push_str("<g style=\"isolation:isolate\">");
        }
    }

    fn close_svg_tag(&mut self) {
        if self.tile {
            self.output.push_str("</g>");
        }
        self.output.push_str("</svg>");
    }

    fn close_figure(&mut self) {
        if self.has_href {
            self.output.push_str("</a>");
        }
        self.output.push_str("</figure>");
    }

    fn defs(&mut self) {
        let height_filter = self.ids.get("height-filter");
        let marker_gradient = self.ids.get("marker-fade");
        let globe_clip = self.ids.get("globe-clip");
        let height_empty = self.ids.get("height-empty");
        // The highlight and shadow masks carry the larger of the two
        // themes' gains, and each theme scales its tints down through
        // flood-opacity, which CSS can set where a filter's own numbers
        // cannot be themed. Neither mask ever reaches 1, so scaling the
        // flood is exactly scaling the mask.
        //
        // Every effect length below was drawn for the narrowest frame and
        // shrinks with a wider one (`Projection::effect_scale`). The
        // lighting's surface heights shrink with its blur, so its slopes,
        // and the strength of the light, stay the same.
        let scale = self.effect_scale;
        let (wide, narrow, smooth) = (length(9.0 * scale), length(4.0 * scale), length(2.0 * scale));
        let (steep, soft, halo_blur) = (length(78.0 * scale), length(34.0 * scale), length(2.4 * scale));
        // These two filter regions are sized off this figure's own
        // `canvas_width`, not a fixed 720: on the explorer's wider
        // full-extent world SVG, a region fixed at the page-map's width
        // would itself re-clip the relief/seafloor bands and coast halo
        // the wider ring-clip bounds (geometry.rs) were just fixed to let
        // through.
        let halo_width = length(self.canvas_width + 48.0);
        let halo_height = length(self.canvas_height + 48.0);
        let page_only_defs = if self.tile {
            format!("<path id=\"{height_empty}\" d=\"m0 0l0 0\"/>")
        } else {
            format!(
                "<radialGradient id=\"{marker_gradient}\"><stop offset=\"0\" stop-color=\"var(--moss-place-marker, #2d5a2d)\" stop-opacity=\"0.28\"/><stop offset=\"1\" stop-color=\"var(--moss-place-marker, #2d5a2d)\" stop-opacity=\"0\"/></radialGradient><clipPath id=\"{globe_clip}\"><circle cx=\"{GLOBE_CENTER_X:.0}\" cy=\"{GLOBE_CENTER_Y:.0}\" r=\"{GLOBE_RADIUS:.0}\"/></clipPath><path id=\"{height_empty}\" d=\"m0 0l0 0\"/><filter id=\"{}\" filterUnits=\"userSpaceOnUse\" x=\"-24\" y=\"-24\" width=\"{halo_width}\" height=\"{halo_height}\"><feGaussianBlur stdDeviation=\"{halo_blur}\"/></filter>",
                self.ids.get("soft"),
            )
        };
        write!(
            self.output,
            "<defs><filter id=\"{height_filter}\" color-interpolation-filters=\"sRGB\"><feColorMatrix in=\"SourceGraphic\" type=\"luminanceToAlpha\" result=\"height-alpha\"/><feGaussianBlur in=\"height-alpha\" stdDeviation=\"{wide}\" result=\"height-blur-9\"/><feColorMatrix in=\"height-blur-9\" type=\"matrix\" values=\"0 0 0 1 0  0 0 0 1 0  0 0 0 1 0  0 0 0 1 0\" result=\"height-coverage\"/><feDiffuseLighting in=\"height-blur-9\" surfaceScale=\"{steep}\" diffuseConstant=\"1\" lighting-color=\"#ffffff\" result=\"lit-steep\"><feDistantLight azimuth=\"240\" elevation=\"45\"/></feDiffuseLighting><feColorMatrix in=\"lit-steep\" type=\"matrix\" values=\"0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  1 0 0 0 0\" result=\"lit-steep-alpha\"/><feGaussianBlur in=\"height-alpha\" stdDeviation=\"{narrow}\" result=\"height-blur-4\"/><feDiffuseLighting in=\"height-blur-4\" surfaceScale=\"{soft}\" diffuseConstant=\"1\" lighting-color=\"#ffffff\" result=\"lit-soft\"><feDistantLight azimuth=\"240\" elevation=\"45\"/></feDiffuseLighting><feColorMatrix in=\"lit-soft\" type=\"matrix\" values=\"0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  1 0 0 0 0\" result=\"lit-soft-alpha\"/><feComposite in=\"lit-steep-alpha\" in2=\"lit-soft-alpha\" operator=\"arithmetic\" k1=\"0\" k2=\"0.7\" k3=\"0.3\" k4=\"0\" result=\"lit-mix\"/><feComponentTransfer in=\"lit-mix\" result=\"lit-hi-mask\"><feFuncA type=\"linear\" slope=\"1.877817459305202\" intercept=\"-1.327817459305202\"/></feComponentTransfer><feComponentTransfer in=\"lit-mix\" result=\"lit-lo-mask\"><feFuncA type=\"linear\" slope=\"-0.6363961030678928\" intercept=\"0.45\"/></feComponentTransfer><feFlood flood-color=\"var(--moss-place-light-warm, #fff3d0)\" flood-opacity=\"var(--moss-place-light-warm-strength, 1)\" result=\"lit-hi-flood\"/><feComposite in=\"lit-hi-flood\" in2=\"lit-hi-mask\" operator=\"in\" result=\"lit-hi-tint\"/><feFlood flood-color=\"var(--moss-place-light-cool, #5a6488)\" flood-opacity=\"var(--moss-place-light-cool-strength, 0.8889)\" result=\"lit-lo-flood\"/><feComposite in=\"lit-lo-flood\" in2=\"lit-lo-mask\" operator=\"in\" result=\"lit-lo-tint\"/><feMerge result=\"lit-combined\"><feMergeNode in=\"lit-lo-tint\"/><feMergeNode in=\"lit-hi-tint\"/></feMerge><feGaussianBlur in=\"lit-combined\" stdDeviation=\"{smooth}\" result=\"lit-smooth\"/><feComposite in=\"lit-smooth\" in2=\"height-coverage\" operator=\"arithmetic\" k1=\"1\" k2=\"0\" k3=\"0\" k4=\"0\"/></filter>{page_only_defs}</defs>",
        )
        .expect("writing to String cannot fail");
        // The approved design's two cut-paper shadows, one per band family,
        // shared by every band group: relief casts a firmer shadow (and, in
        // the dark theme, a faint warm top-left edge) than the sea floor.
        // The land fill itself casts none.
        let (relief, relief_blur) = (length(1.4 * scale), length(0.7 * scale));
        let (edge, edge_blur) = (length(0.7 * scale), length(0.49 * scale));
        let (sea, sea_blur) = (length(1.0 * scale), length(0.6 * scale));
        let filter_width = length(self.canvas_width + 24.0);
        let filter_height = length(self.canvas_height + 24.0);
        write!(
            self.output,
            "<defs><filter id=\"{}\" color-interpolation-filters=\"sRGB\" filterUnits=\"userSpaceOnUse\" x=\"-12\" y=\"-12\" width=\"{filter_width}\" height=\"{filter_height}\"><feDropShadow in=\"SourceGraphic\" dx=\"{relief}\" dy=\"{relief}\" stdDeviation=\"{relief_blur}\" flood-color=\"var(--moss-place-shadow, #5a6488)\" flood-opacity=\"0.35\" result=\"with-shadow\"/><feDropShadow in=\"SourceGraphic\" dx=\"-{edge}\" dy=\"-{edge}\" stdDeviation=\"{edge_blur}\" flood-color=\"var(--moss-place-light-warm, #fff3d0)\" flood-opacity=\"var(--moss-place-relief-edge-opacity, 0)\" result=\"with-edge\"/><feMerge><feMergeNode in=\"with-shadow\"/><feMergeNode in=\"with-edge\"/></feMerge></filter><filter id=\"{}\" color-interpolation-filters=\"sRGB\" filterUnits=\"userSpaceOnUse\" x=\"-12\" y=\"-12\" width=\"{filter_width}\" height=\"{filter_height}\"><feDropShadow in=\"SourceGraphic\" dx=\"{sea}\" dy=\"{sea}\" stdDeviation=\"{sea_blur}\" flood-color=\"var(--moss-place-shadow, #5a6488)\" flood-opacity=\"0.22\"/></filter>",
            self.ids.get("shadow-relief"),
            self.ids.get("shadow-seafloor"),
        )
        .expect("writing to String cannot fail");
        self.output.push_str("</defs>");
    }

    fn water(&mut self) {
        let id = self.ids.get("layer-water");
        let width = length(self.canvas_width);
        let height = length(self.canvas_height);
        write!(
            self.output,
            "<g id=\"{id}\" data-map-layer=\"water\"><rect width=\"{width}\" height=\"{height}\" fill=\"var(--moss-place-water, #dbe7ea)\"/></g>",
        )
        .expect("writing to String cannot fail");
    }

    fn empty_layers(&mut self) {
        for name in ["coast", "seafloor", "land", "relief"] {
            write!(
                self.output,
                "<g id=\"{}\" data-map-layer=\"{name}\"/>",
                self.ids.get(&format!("layer-{name}"))
            )
            .expect("writing to String cannot fail");
        }
        // The remaining empty semantic layers follow the lighting group,
        // matching the non-empty emission order.
    }

    fn empty_layers_after_lighting(&mut self) {
        for name in ["ice", "salt", "lakes", "rivers", "reefs", "built-up"] {
            write!(
                self.output,
                "<g id=\"{}\" data-map-layer=\"{name}\"/>",
                self.ids.get(&format!("layer-{name}"))
            )
            .expect("writing to String cannot fail");
        }
    }

    fn emit_layers(
        &mut self,
        quantisation: u32,
        projection: &Projection,
        grouped: &[Vec<&Feature>],
    ) {
        // The coast halo first, then the sea floor painted shallow to deep,
        // then land and relief low to high. The source pack has stable IDs;
        // no administrative layer is accepted or emitted here.
        self.land_defs(quantisation, projection, grouped);
        // The coast is a blurred halo, which a flat figure has no filter for.
        if !self.tile {
            self.coast();
        }
        self.emit_band_layer(quantisation, projection, grouped, 10, "seafloor", true);
        self.land();
        self.emit_band_layer(quantisation, projection, grouped, 9, "relief", false);
    }

    /// Land is defined once and drawn twice through `<use>`: as the coast's
    /// soft halo under the sea floor, then as the land fill over it.
    pub(super) fn land_defs(
        &mut self,
        quantisation: u32,
        projection: &Projection,
        grouped: &[Vec<&Feature>],
    ) {
        self.output.push_str("<defs>");
        for (index, feature) in grouped[2].iter().enumerate() {
            let rings: Vec<&[(i32, i32)]> = feature.parts.iter().map(Vec::as_slice).collect();
            if let Some(path) = serialize_path(&projection.project_feature(&rings, quantisation), true, FINE.scaled(self.detail)) {
                let id = format!("{}-land-{index}", self.ids.base);
                write!(self.output, "<path id=\"{id}\" d=\"{path}\"/>")
                    .expect("writing to String cannot fail");
                self.land_paths.push(id);
            }
        }
        self.output.push_str("</defs>");
    }

    /// The land outline stroked wide, pale and blurred, under the sea
    /// floor. The sea-floor bands cover it everywhere deeper than the
    /// shallowest band, so what stays visible is a pale band on the sea
    /// side of the shore.
    pub(super) fn coast(&mut self) {
        write!(self.output, "<g id=\"{}\" data-map-layer=\"coast\" filter=\"url(#{})\">", self.ids.get("layer-coast"), self.ids.get("soft"))
            .expect("writing to String cannot fail");
        for id in &self.land_paths {
            write!(self.output, "<use href=\"#{id}\" fill=\"none\" stroke=\"var(--moss-place-coast, #f5f6f4)\" stroke-opacity=\"var(--moss-place-coast-opacity, 0.6)\" stroke-width=\"{}\"/>", length(10.0 * self.effect_scale))
                .expect("writing to String cannot fail");
        }
        self.output.push_str("</g>");
    }

    pub(super) fn land(&mut self) {
        write!(self.output, "<g id=\"{}\" data-map-layer=\"land\">", self.ids.get("layer-land"))
            .expect("writing to String cannot fail");
        for id in &self.land_paths {
            write!(self.output, "<use href=\"#{id}\" fill=\"var(--moss-place-land, #e3e6d5)\"/>")
                .expect("writing to String cannot fail");
        }
        self.output.push_str("</g>");
    }

    /// The physical layers over the lit terrain, in the approved design's
    /// paint order: ice, salt flats, lakes, rivers, reefs, then built-up
    /// areas at 70% so terrain still reads through a city.
    fn emit_surface_layers(
        &mut self,
        quantisation: u32,
        projection: &Projection,
        grouped: &[Vec<&Feature>],
    ) {
        let fill = |color: &str| format!("fill=\"{color}\"");
        self.emit_merged_layer(quantisation, projection, grouped, 5, "ice", &fill("var(--moss-place-ice, #fbfcfd)"));
        self.emit_merged_layer(quantisation, projection, grouped, 7, "salt", &fill("var(--moss-place-salt, #e3dcc8)"));
        self.emit_merged_layer(quantisation, projection, grouped, 3, "lakes", &fill("var(--moss-place-lakes, #bcd3de)"));
        self.emit_river_layer(quantisation, projection, grouped, "var(--moss-place-rivers, #7fa6bd)");
        // Natural Earth's reefs are lines, not areas.
        self.emit_merged_layer(quantisation, projection, grouped, 6, "reefs", &format!("fill=\"none\" stroke=\"var(--moss-place-reefs, #b9d6cf)\" stroke-width=\"{}\"", length(1.2 * self.stroke_scale())));
        self.emit_merged_layer(quantisation, projection, grouped, 8, "built-up", "fill=\"var(--moss-place-built-up, #c9bdb4)\" fill-opacity=\"0.7\"");
    }

    /// Every feature of one layer in a single path: these layers are never
    /// referenced by id, so one element per feature would only add bytes.
    /// Filled layers keep their holes; line layers (reefs) stay open.
    fn emit_merged_layer(
        &mut self,
        quantisation: u32,
        projection: &Projection,
        grouped: &[Vec<&Feature>],
        layer_id: u8,
        name: &str,
        paint: &str,
    ) {
        let filled = !paint.starts_with("fill=\"none\"");
        let mut paths = Vec::new();
        for feature in &grouped[usize::from(layer_id)] {
            for part in &feature.parts {
                paths.extend(if filled {
                    projection.project_ring(part, quantisation)
                } else {
                    projection.project_part(part, quantisation)
                });
            }
        }
        write!(self.output, "<g id=\"{}\" data-map-layer=\"{name}\">", self.ids.get(&format!("layer-{name}")))
            .expect("writing to String cannot fail");
        if let Some(path) = serialize_path(&paths, filled, FINE.scaled(self.detail)) {
            write!(self.output, "<path d=\"{path}\" {paint}/>").expect("writing to String cannot fail");
        }
        self.output.push_str("</g>");
    }

    fn emit_band_layer(
        &mut self,
        quantisation: u32,
        projection: &Projection,
        grouped: &[Vec<&Feature>],
        layer_id: u8,
        name: &str,
        shallow_first: bool,
    ) {
        let mut features: Vec<&Feature> = grouped[usize::from(layer_id)].clone();
        features.sort_by(|a, b| {
            if shallow_first {
                b.band.cmp(&a.band)
            } else {
                a.band.cmp(&b.band)
            }
        });
        // Project first: a band's tint is stretched to the bands actually on
        // screen, which only the projected rings can tell.
        let projected: Vec<(&Feature, Option<String>)> = features
            .into_iter()
            .map(|feature| {
                let rings: Vec<&[(i32, i32)]> = feature.parts.iter().map(Vec::as_slice).collect();
                // One project_feature call per source feature: all rings
                // stay in one path, preserving holes through
                // clipping.
                let rings = projection.project_feature(&rings, quantisation);
                let on_screen = rings.iter().any(|ring| on_screen(ring, self.canvas_width, self.canvas_height));
                (feature, on_screen.then(|| serialize_path(&rings, true, BY_AREA.scaled(self.detail))).flatten())
            })
            .filter(|(_, path)| path.is_some())
            .collect();
        let mut present: Vec<i16> = projected.iter().map(|(feature, _)| feature.band).collect();
        present.dedup();
        // A tile's tint is not stretched to the bands it happens to hold:
        // two tiles overlap by their bleed and must paint one band the same
        // colour, or whichever sits on top decides the colour of the strip.
        // The whole ladder is the world layer's own stretch, too.
        if self.tile {
            present = band_ladder(name).to_vec();
        }
        // Relief is traced from an elevation grid that does not follow the
        // Natural Earth shoreline, so about 5% of the lowest band lies over
        // the sea; the approved design clips relief to the land outline.
        let clip = if name == "relief" {
            format!(" clip-path=\"url(#{})\"", self.ids.get("land-clip"))
        } else {
            String::new()
        };
        write!(
            self.output,
            "<g id=\"{}\" data-map-layer=\"{name}\"{clip}>",
            self.ids.get(&format!("layer-{name}"))
        )
        .expect("writing to String cannot fail");
        let band_shadow = format!(" filter=\"url(#{})\"", self.ids.get(&format!("shadow-{name}")));
        let mut active_band = None;
        for (index, (feature, path)) in projected.into_iter().enumerate() {
            if active_band != Some(feature.band) {
                if active_band.is_some() {
                    self.output.push_str("</g>");
                }
                active_band = Some(feature.band);
                // The tint sits on the group, not on the band paths: the
                // lighting reuses the relief paths through <use> with a grey
                // per band, and a path's own fill would override it.
                write!(
                    self.output,
                    "<g data-map-band=\"{}\" style=\"fill:{}\"{band_shadow}>",
                    feature.band,
                    band_tint(name, feature.band, &present),
                )
                .expect("writing to String cannot fail");
            }
            let id = format!("{}-{name}-{index}", self.ids.base);
            write!(
                self.output,
                "<path id=\"{id}\" d=\"{}\" data-map-band=\"{}\"/>",
                path.unwrap_or_default(),
                feature.band
            )
            .expect("writing to String cannot fail");
            if name == "relief" {
                self.height_uses.push(format!("{id}|{}", relief_height_grey(feature.band)));
            }
        }
        if active_band.is_some() {
            self.output.push_str("</g>");
        }
        self.output.push_str("</g>");
    }

    fn lighting(&mut self) {
        let filter = self.ids.get("height-filter");
        let id = self.ids.get("lighting");
        let land_clip = self.ids.get("land-clip");
        write!(self.output, "<clipPath id=\"{land_clip}\">")
            .expect("writing to String cannot fail");
        for path_id in &self.land_paths {
            write!(
                self.output,
                "<use href=\"#{path_id}\" height=\"100%\" data-map-role=\"land-clip\"/>"
            )
            .expect("writing to String cannot fail");
        }
        self.output.push_str("</clipPath>");
        // The approved design lights the terrain at half strength: the
        // opacity sits on the filtered group, so it fades the filter's
        // output. On an element inside the filter it would change nothing,
        // because luminanceToAlpha reads a pixel's colour, not its alpha.
        write!(self.output, "<g id=\"{id}\" data-map-layer=\"lighting\" clip-path=\"url(#{land_clip})\" aria-hidden=\"true\"><g id=\"{}\" filter=\"url(#{filter})\" opacity=\"0.5\">", self.ids.get("height-field")).expect("writing to String cannot fail");
        for item in &self.height_uses {
            let (path_id, grey) = item.split_once('|').unwrap_or((item.as_str(), "#f2f2f2"));
            write!(self.output, "<use href=\"#{path_id}\" height=\"100%\" fill=\"{grey}\" fill-opacity=\"1\" data-map-role=\"height-field\"/>").expect("writing to String cannot fail");
        }
        if self.height_uses.is_empty() {
            write!(
                self.output,
                "<use href=\"#{}\" height=\"100%\" fill=\"rgb(128 128 128)\" fill-opacity=\"1\" data-map-role=\"height-field\"/>",
                self.ids.get("height-empty")
            )
            .expect("writing to String cannot fail");
        }
        self.output.push_str("</g></g>");
    }

    fn markers(&mut self, target: &PlaceMapTarget) {
        let id = self.ids.get("markers");
        let fade = self.ids.get("marker-fade");
        self.output.push_str(&format!(
            "<g id=\"{id}\" data-map-layer=\"marker\" aria-label=\"markers\">"
        ));
        if let Some(frame) = target.frame.as_ref() {
            let projection = Projection::new(frame);
            // Soft fades first, so none covers a dot. A marker that would
            // draw the same shape on the same pixel as one already drawn is
            // left out, so several places in one city make one dot.
            let mut places: Vec<&ResolvedPlace> = target.marker_places().collect();
            places.sort_by_key(|place| matches!(place.precision, Precision::Exact | Precision::City));
            let mut drawn = std::collections::HashSet::new();
            for place in places {
                let Some(point) = place.point().and_then(|point| projection.project(point)) else {
                    continue;
                };
                let (x, y) = (snap(point.0), snap(point.1));
                let shape = match place.precision {
                    Precision::Exact | Precision::City => 0,
                    Precision::Region => 1,
                    Precision::Country => 2,
                };
                if !drawn.insert((x, y, shape)) {
                    continue;
                }
                let key = xml_escape(&place.key);
                match place.precision {
                    Precision::Exact | Precision::City => {
                        let radius = marker_radius(place.precision);
                        // A fade below is a geographic extent and may crop at
                        // the frame; a dot is a fixed-size marker and may not.
                        let (cx, cy) = clamp_dot(point, radius + 1.0);
                        let (x, y) = (snap(cx), snap(cy));
                        write!(self.output, "{}{}", dot(x, y, radius + 1.0, "var(--moss-place-marker-casing, #ffffff)", ""), dot(x, y, radius, "var(--moss-place-marker, #2d5a2d)", &format!(" data-map-marker=\"{key}\""))).expect("writing to String cannot fail");
                    }
                    Precision::Region | Precision::Country => {
                        let fade_edge = place.point().filter(|_| place.precision == Precision::Region).and_then(|centre| {
                            ProjectedPoint::new(centre.longitude, (centre.latitude + REGION_FADE_DEGREES).min(89.9))
                        });
                        let radius = fade_edge
                            .and_then(|edge| projection.project(edge))
                            .map_or(marker_radius(place.precision), |edge| (edge.0 - point.0).hypot(edge.1 - point.1).max(REGION_FADE_MIN_RADIUS));
                        write!(self.output, "<circle cx=\"{x}\" cy=\"{y}\" r=\"{radius:.0}\" fill=\"url(#{fade})\" data-map-marker=\"{key}\"/>").expect("writing to String cannot fail");
                    }
                }
            }
        }
        self.output.push_str("</g>");
    }

    fn globe(&mut self, context: &PlaceMapContext, target: &PlaceMapTarget) {
        let clip = self.ids.get("globe-clip");
        let id = self.ids.get("globe");
        let center = target
            .frame
            .as_ref()
            .map(|frame| {
                ProjectedPoint::new(frame.center_longitude, frame.center_latitude).unwrap()
            })
            .unwrap_or(ProjectedPoint {
                longitude: 0.0,
                latitude: 0.0,
            });
        write!(self.output, "<g id=\"{id}\" data-map-layer=\"globe\" clip-path=\"url(#{clip})\"><circle cx=\"{GLOBE_CENTER_X:.0}\" cy=\"{GLOBE_CENTER_Y:.0}\" r=\"{GLOBE_RADIUS:.0}\" fill=\"var(--moss-place-globe-water, #dbe7ea)\"/>").expect("writing to String cannot fail");
        // The world tier holds land alone: its rings are the globe's land
        // and, stroked first, its coast.
        let quantisation = context.pack().header.quantisation;
        let land = context.pack().tiers[0]
            .layers
            .iter()
            .find(|layer| layer.id == 2)
            .map_or(&[][..], |layer| layer.features.as_slice());
        if self.locator_profile != Some(LocatorProfile::Country) {
            for (index, feature) in land.iter().enumerate() {
                let paths: Vec<Vec<(f64, f64)>> = feature
                    .parts
                    .iter()
                    .flat_map(|part| globe_line(part, center, quantisation))
                    .collect();
                if let Some(path) = serialize_path(&paths, false, FINE) {
                    write!(self.output, "<path d=\"{path}\" fill=\"none\" stroke=\"var(--moss-place-globe-coast, #f5f6f4)\" stroke-width=\"0.5\" data-globe-feature=\"{index}\"/>").expect("writing to String cannot fail");
                }
            }
        }
        for (index, feature) in land.iter().enumerate() {
            let rings: Vec<Vec<(f64, f64)>> = feature
                .parts
                .iter()
                .flat_map(|part| globe_rings(part, center, quantisation))
                .collect();
            if let Some(path) = serialize_path(&rings, true, FINE) {
                write!(self.output, "<path d=\"{path}\" fill=\"var(--moss-place-globe-land, #e3e6d5)\" fill-rule=\"evenodd\" data-globe-feature=\"{index}\"/>").expect("writing to String cannot fail");
            }
        }
        draw_globe_line(self, target, center, quantisation);
        let mut drawn = std::collections::HashSet::new();
        for place in target.marker_places() {
            let Some(point) = place.point() else {
                continue;
            };
            let Some((x, y)) = globe_marker(point, center) else {
                continue;
            };
            let (x, y) = clamp_to_circle((x, y), (GLOBE_CENTER_X, GLOBE_CENTER_Y), GLOBE_RADIUS - dot_reach(3.0));
            let (x, y) = (snap(x), snap(y));
            if !drawn.insert((x, y)) {
                continue;
            }
            write!(
                self.output,
                "{}",
                dot(x, y, 3.0, "var(--moss-place-marker, #2d5a2d)", &format!(" data-map-globe-marker=\"true\" data-map-marker=\"{}\"", xml_escape(&place.key)))
            )
            .expect("writing to String cannot fail");
        }
        write!(self.output, "</g><circle cx=\"{GLOBE_CENTER_X:.0}\" cy=\"{GLOBE_CENTER_Y:.0}\" r=\"{GLOBE_RADIUS:.0}\" fill=\"none\" stroke=\"var(--moss-place-globe-edge, #e3e6d5)\" stroke-width=\"1\" data-map-globe-inset=\"true\"/>").expect("writing to String cannot fail");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::geometry::{world_viewbox_width, DESIGN_PIXELS_PER_DEGREE, FRAME_PADDING, WIDE_ZOOM};
    use crate::build::place_map::Frame;
    use std::io::Write;

    fn populated_target(precision: Precision) -> PlaceMapTarget {
        let point = ProjectedPoint::new(139.7, 35.7).unwrap();
        let frame = Frame::from_points(&[point], [precision].into_iter()).unwrap();
        PlaceMapTarget {
            places: vec![ResolvedPlace {
                key: "places/kyoto".to_string(),
                display: "Kyoto".to_string(),
                longitude: Some(point.longitude),
                latitude: Some(point.latitude),
                precision,
            }],
            frame: Some(frame),
            aggregate_name: None,
            route: false,
        }
    }

    #[test]
    fn ids_are_sha256_namespaced_by_path_and_ordinal() {
        let first = emit_svg(
            &PlaceMapContext::embedded().unwrap(),
            &PlaceMapTarget {
                places: vec![],
                frame: None,
                aggregate_name: None,
                route: false,
            },
            "a/page.md",
            0,
        );
        let second = emit_svg(
            &PlaceMapContext::embedded().unwrap(),
            &PlaceMapTarget {
                places: vec![],
                frame: None,
                aggregate_name: None,
                route: false,
            },
            "a/page.md",
            1,
        );
        assert_ne!(first, second);
        assert!(first.contains("moss-place-map-"));
        assert!(!first.contains("a/page.md"));
    }

    #[test]
    fn output_is_deterministic_and_accessible() {
        let context = PlaceMapContext::embedded().unwrap();
        let target = PlaceMapTarget {
            places: vec![],
            frame: None,
            aggregate_name: None,
            route: false,
        };
        let first = emit_svg_with_options(
            &context,
            &target,
            SvgMapOptions::new("p", 2, "A < B & C", Precision::Country),
        );
        let second = emit_svg_with_options(
            &context,
            &target,
            SvgMapOptions::new("p", 2, "A < B & C", Precision::Country),
        );
        assert_eq!(first, second);
        assert!(first.contains("role=\"img\""));
        assert!(first.contains("aria-hidden=\"true\""));
        assert!(first.contains("A &lt; B &amp; C"));
        assert!(first.contains("width=\"720\" height=\"480\""));
    }

    #[test]
    fn fixed_layer_order_filters_and_globe_are_present() {
        let output = emit_svg(
            &PlaceMapContext::embedded().unwrap(),
            &PlaceMapTarget {
                places: vec![],
                frame: None,
                aggregate_name: None,
                route: false,
            },
            "p",
            0,
        );
        let order = [
            "data-map-layer=\"water\"",
            "data-map-layer=\"coast\"",
            "data-map-layer=\"seafloor\"",
            "data-map-layer=\"land\"",
            "data-map-layer=\"relief\"",
            "data-map-layer=\"lighting\"",
            "data-map-layer=\"marker\"",
            "data-map-layer=\"globe\"",
        ];
        let mut previous = 0;
        for item in order {
            let index = output.find(item).unwrap();
            assert!(index >= previous);
            previous = index;
        }
        for item in [
            "stdDeviation=\"9\"",
            "stdDeviation=\"4\"",
            "luminanceToAlpha",
            "azimuth=\"240\"",
            "elevation=\"45\"",
            "color-interpolation-filters=\"sRGB\"",
            "<use ",
            "height=\"100%\"",
        ] {
            assert!(output.contains(item), "missing {item}");
        }
        assert!(!output.contains("border"));
        assert!(output.contains("data-map-globe-inset=\"true\""));
    }

    /// The coast is the approved design's halo: the land outline, 10 px
    /// wide, 60% opaque and blurred by 2.4, drawn after the water and under
    /// the sea floor so only the shallows' edge of it shows.
    #[test]
    fn coast_is_a_soft_halo_of_the_land_outline_under_the_sea_floor() {
        let svg = locator_for(35.5, 33.89, Precision::Exact);
        let position = |name: &str| svg.find(&format!("data-map-layer=\"{name}\"")).unwrap();
        assert!(position("water") < position("coast") && position("coast") < position("seafloor"));
        let coast = layer_body(&svg, "coast");
        let soft = format!("url(#{}-soft)", &svg[svg.find("moss-place-map-").unwrap()..][..79]);
        assert!(coast.starts_with(&format!(" filter=\"{soft}\">")), "{coast:.200}");
        assert!(svg.contains("<feGaussianBlur stdDeviation=\"2.4\"/>"));
        let halos: Vec<&str> = coast.split("<use ").skip(1).collect();
        assert!(!halos.is_empty());
        for halo in halos {
            assert!(halo.contains("stroke-width=\"10\"") && halo.contains("0.6)"), "{halo}");
            let target = &halo[halo.find("href=\"#").unwrap() + 7..];
            let id = &target[..target.find('"').unwrap()];
            assert!(id.contains("-land-") && svg.contains(&format!("<path id=\"{id}\"")), "{id}");
        }
    }

    /// Band paths go through the area simplifier: a gently curving edge (a
    /// 5-degree circle, every point within 2 px of a chord more than 70 px
    /// long) keeps its curve instead of becoming those chords.
    #[test]
    fn band_edges_keep_their_curves_through_render_time_simplification() {
        let origin = ProjectedPoint::new(0.0, 0.0).unwrap();
        let frame = Frame::from_points(&[origin], [Precision::Exact].into_iter()).unwrap();
        let projection = Projection::new(&frame);
        let ring: Vec<(i32, i32)> = (0..=628)
            .map(|step| {
                let angle = f64::from(step) / 100.0;
                ((50_000.0 * angle.cos()).round() as i32, (50_000.0 * angle.sin()).round() as i32)
            })
            .collect();
        let feature = Feature { bounds: [-50_000, -50_000, 50_000, 50_000], band: 100, parts: vec![ring] };
        let mut grouped: Vec<Vec<&Feature>> = (0..=10).map(|_| Vec::new()).collect();
        grouped[9].push(&feature);
        let ids = Ids::new("p", 0);
        let mut writer = Writer {
            output: String::new(),
            ids: &ids,
            height_uses: Vec::new(),
            land_paths: Vec::new(),
            has_href: false,
            effect_scale: 1.0,
            detail: 1.0,
            locator_profile: None,
            canvas_width: f64::from(SVG_WIDTH),
            canvas_height: f64::from(SVG_HEIGHT),
            full_extent: false,
            tile: false,
        };
        writer.emit_band_layer(10_000, &projection, &grouped, 9, "relief", false);
        let d = &writer.output[writer.output.find(" d=\"").unwrap() + 4..];
        let mut longest = 0.0_f64;
        for step in d[..d.find('"').unwrap()].split(['m', 'l']).skip(2) {
            let mut numbers = step.trim_end_matches('z').split(' ').map(|v| v.parse::<f64>().unwrap());
            let (dx, dy) = (numbers.next().unwrap(), numbers.next().unwrap());
            // Edges along the clip rectangle are not band edges.
            if dx != 0.0 && dy != 0.0 {
                longest = longest.max(dx.hypot(dy));
            }
        }
        assert!(longest < 50.0, "a curved band edge became a {longest} px chord");
    }

    /// Rivers leave the pack simplified by area, so a river winding a
    /// fraction of a pixel either side of its course keeps its bends; a
    /// render-time pass by distance would put the long chords back.
    #[test]
    fn river_bends_survive_render_time_simplification() {
        let origin = ProjectedPoint::new(0.0, 0.0).unwrap();
        let frame = Frame::from_points(&[origin], [Precision::Exact].into_iter()).unwrap();
        let projection = Projection::new(&frame);
        // Six degrees of river, 0.6 px either side of its course, one bend
        // every 0.3 degrees.
        let line: Vec<(i32, i32)> = (-300..=300)
            .map(|step| (step * 100, (85.0 * (f64::from(step) * std::f64::consts::PI / 30.0).sin()).round() as i32))
            .collect();
        let feature = Feature { bounds: [-30_000, -85, 30_000, 85], band: 2, parts: vec![line] };
        let mut grouped: Vec<Vec<&Feature>> = (0..=10).map(|_| Vec::new()).collect();
        grouped[4].push(&feature);
        let ids = Ids::new("p", 0);
        let mut writer = Writer {
            output: String::new(),
            ids: &ids,
            height_uses: Vec::new(),
            land_paths: Vec::new(),
            has_href: false,
            effect_scale: 1.0,
            detail: 1.0,
            locator_profile: None,
            canvas_width: f64::from(SVG_WIDTH),
            canvas_height: f64::from(SVG_HEIGHT),
            full_extent: false,
            tile: false,
        };
        writer.emit_river_layer(10_000, &projection, &grouped, "#5b93bd");
        let d = &writer.output[writer.output.find(" d=\"").unwrap() + 4..];
        let longest = d[..d.find('"').unwrap()]
            .split(['m', 'l'])
            .skip(2)
            .map(|step| {
                let mut numbers = step.split(' ').map(|value| value.parse::<f64>().unwrap());
                numbers.next().unwrap().hypot(numbers.next().unwrap())
            })
            .fold(0.0_f64, f64::max);
        assert!(longest < 50.0, "a winding river became a {longest} px chord");
    }

    /// The approved design's cut-paper shadows: relief bands cast the
    /// firmer 1.4 px / 35% shadow, sea-floor bands the lighter 1.0 px / 22%
    /// one, and the land fill casts none.
    #[test]
    fn relief_and_sea_floor_cast_their_own_shadows_and_land_casts_none() {
        let svg = locator_for(35.5, 33.89, Precision::Exact);
        let base = &svg[svg.find("moss-place-map-").unwrap()..][..79];
        let filter = |name: &str| {
            let start = svg.find(&format!("<filter id=\"{base}-shadow-{name}\"")).unwrap_or_else(|| panic!("no {name} shadow"));
            &svg[start..start + svg[start..].find("</filter>").unwrap()]
        };
        assert!(filter("relief").contains("dx=\"1.4\"") && filter("relief").contains("flood-opacity=\"0.35\""));
        assert!(filter("seafloor").contains("dx=\"1\"") && filter("seafloor").contains("flood-opacity=\"0.22\""));
        for name in ["relief", "seafloor"] {
            let body = layer_body(&svg, name);
            let groups = body.matches("<g data-map-band=").count();
            assert!(groups > 0);
            assert_eq!(body.matches(&format!("filter=\"url(#{base}-shadow-{name})\"")).count(), groups, "{name}");
        }
        let land_open = &svg[svg.find("data-map-layer=\"land\"").unwrap()..];
        assert!(!land_open[..land_open.find('>').unwrap()].contains("filter"), "the land fill casts a shadow");
    }

    /// Terrain lighting at the approved half strength. The opacity has to
    /// fade the filter's output: set on the source inside the filter it is
    /// discarded, since luminanceToAlpha reads colour and ignores alpha.
    #[test]
    fn terrain_lighting_is_faded_after_the_filter_not_before() {
        let svg = locator_for(100.5, 13.75, Precision::Exact);
        let lighting = &svg[svg.find("data-map-layer=\"lighting\"").unwrap()..];
        let lighting = &lighting[..lighting.find("</g></g>").unwrap()];
        let filtered = lighting.find("-height-filter)\"").expect("the lighting group applies the height filter");
        let tag = &lighting[lighting[..filtered].rfind('<').unwrap()..];
        let tag = &tag[..tag.find('>').unwrap()];
        assert!(tag.contains("opacity=\"0.5\""), "the filtered element is not faded: {tag}");
        assert_eq!(lighting.matches(" opacity=").count(), 1, "an opacity inside the filter does nothing");
    }

    #[test]
    fn relief_is_clipped_to_the_land_outline() {
        let svg = locator_for(35.5, 33.89, Precision::Exact);
        let base = &svg[svg.find("moss-place-map-").unwrap()..][..79];
        let open = &svg[svg.find("data-map-layer=\"relief\"").unwrap()..];
        assert!(open[..open.find('>').unwrap()].contains(&format!("clip-path=\"url(#{base}-land-clip)\"")));
        assert!(svg.contains(&format!("<clipPath id=\"{base}-land-clip\">")));
    }

    /// The lighting draws each relief path again through <use> with a grey
    /// that encodes its height. A fill on the path itself would override
    /// the use's grey, so every band would light as its own tint instead.
    #[test]
    fn the_height_field_sees_each_band_as_its_grey() {
        let svg = locator_for(100.5, 13.75, Precision::Exact);
        let relief = layer_body(&svg, "relief");
        let paths: Vec<&str> = relief.split("<path ").skip(1).map(|tag| &tag[..tag.find('>').unwrap()]).collect();
        assert!(!paths.is_empty());
        for tag in paths {
            assert!(!tag.contains(" fill=\""), "a relief path fills itself: {tag:.120}");
        }
        assert!(relief.contains("<g data-map-band=\"100\" style=\"fill:"), "the band colour belongs on the group");
    }

    /// The tints stretch to the bands on screen: the highest relief band in
    /// the frame reaches the palette's top tint and the deepest sea band its
    /// deepest, however modest the frame's own relief and depths are.
    #[test]
    fn band_tints_span_the_whole_ramp_within_each_frame() {
        let svg = locator_for(100.5, 13.75, Precision::Exact);
        let last_group = |name: &str| {
            let from = svg.find(&format!("data-map-layer=\"{name}\"")).unwrap();
            let body = &svg[from..from + svg[from + 1..].find("data-map-layer=").unwrap()];
            let start = body.rfind("<g data-map-band=").unwrap();
            body[start..start + body[start..].find('>').unwrap()].to_string()
        };
        let top = last_group("relief");
        assert!(top.contains("--moss-place-land-high, #f7f2e6) 100%"), "{top}");
        let deepest = last_group("seafloor");
        assert!(deepest.contains("--moss-place-sea-deep, #b9cdd6) 100%"), "{deepest}");
    }

    /// The approved design's markers: an exact or city place is a 4 px dot
    /// on a 5 px casing and the globe marks any place with a 3 px dot, all
    /// in screen px however wide the map is shown; a region is a soft fade
    /// from 28% out to 1.1 degrees of latitude (about 78 px at the
    /// 10-degree frame), and never under 12 px.
    #[test]
    fn markers_match_the_approved_dot_fade_and_globe_sizes() {
        let exact = locator_for(35.5, 33.89, Precision::Exact);
        let marker = layer_body(&exact, "marker");
        let dot = |width: &str, color: &str| format!("stroke=\"{color}\" stroke-width=\"{width}\" stroke-linecap=\"round\" vector-effect=\"non-scaling-stroke\"");
        assert!(marker.contains(&dot("10", "var(--moss-place-marker-casing, #ffffff)")), "{marker}");
        assert!(marker.contains(&dot("8", "var(--moss-place-marker, #2d5a2d)")), "{marker}");
        let region = locator_for(40.5, 36.0, Precision::Region);
        let marker = layer_body(&region, "marker");
        let radius: f64 = marker[marker.find(" r=\"").unwrap() + 4..].split('"').next().unwrap().parse().unwrap();
        assert!((70.0..=85.0).contains(&radius), "region fade radius {radius}");
        assert!(region.contains("stop-opacity=\"0.28\""));
        for svg in [&exact, &region] {
            assert!(svg.contains(&format!("{} data-map-globe-marker=\"true\"", dot("6", "var(--moss-place-marker, #2d5a2d)"))));
        }
        let place = ResolvedPlace { key: "places/kansai".into(), display: "Kansai".into(), longitude: Some(135.5), latitude: Some(34.7), precision: Precision::Region };
        let world = PlaceMapTarget { places: vec![place], frame: world_frame(), aggregate_name: Some("places".into()), route: false };
        let svg = emit_svg(&PlaceMapContext::embedded().unwrap(), &world, "p", 0);
        assert!(layer_body(&svg, "marker").contains(" r=\"12\""), "{}", layer_body(&svg, "marker"));
    }

    /// Every dot a layer draws: its centre and its radius in screen px.
    fn dots(layer: &str) -> Vec<(f64, f64, f64)> {
        layer
            .split("<path d=\"M")
            .skip(1)
            .map(|rest| {
                let mut centre = rest[..rest.find("h0").unwrap()].split(' ').map(|value| value.parse::<f64>().unwrap());
                let width = &rest[rest.find("stroke-width=\"").unwrap() + 14..];
                let width: f64 = width[..width.find('"').unwrap()].parse().unwrap();
                (centre.next().unwrap(), centre.next().unwrap(), width / 2.0)
            })
            .collect()
    }

    /// The places root of a site with places all round the world, two of
    /// them either side of the antimeridian, where the world map's edge
    /// falls. Every dot, casing included, stays inside the viewBox at the
    /// narrowest width a map is shown: a dot is drawn in screen px, so there
    /// it reaches furthest into the viewBox.
    #[test]
    fn every_dot_on_the_world_listing_stays_whole() {
        let table: toml::value::Table = toml::from_str(
            "[\"Lima\"]\nlat = -12.05\nlng = -77.03\nprecision = \"exact\"\n\
             [\"Lisbon\"]\nlat = 38.722\nlng = -9.139\nprecision = \"city\"\n\
             [\"Beirut\"]\nlat = 33.89\nlng = 35.5\nprecision = \"exact\"\n\
             [\"Bangkok\"]\nlat = 13.75\nlng = 100.5\nprecision = \"exact\"\n\
             [\"McMurdo\"]\nlat = -77.85\nlng = 166.67\nprecision = \"exact\"\n\
             [\"Taveuni\"]\nlat = -16.85\nlng = 179.95\nprecision = \"exact\"\n\
             [\"Wrangel Island\"]\nlat = 71.23\nlng = -179.9\nprecision = \"exact\"\n",
        )
        .unwrap();
        let gazetteer = crate::vault::places::parse_gazetteer(&table);
        let names: Vec<String> = ["Lima", "Lisbon", "Beirut", "Bangkok", "McMurdo", "Taveuni", "Wrangel Island"].map(String::from).to_vec();
        let context = PlaceMapContext::embedded().unwrap();
        let target = context.resolve_aggregate("places", &gazetteer, "places", &names);
        assert_eq!(target.tier(), FrameTier::World);
        let svg = emit_svg(&context, &target, "places/index.html", 0);
        let dots = dots(layer_body(&svg, "marker"));
        assert_eq!(dots.len(), 2 * names.len(), "a casing and a dot for each place");
        for (x, y, radius) in dots {
            let reach = radius * 720.0 / NARROWEST_MAP_PX;
            assert!(x - reach >= 0.0 && x + reach <= 720.0, "a dot at x={x} reaches {reach} past the side");
            assert!(y - reach >= 0.0 && y + reach <= 480.0, "a dot at y={y} reaches {reach} past the top or bottom");
        }
    }

    /// A guard for the places explorer's separate full-extent world SVG
    /// (`explorer::emit_world_svg`, `geometry::world_viewbox_width`): a
    /// page's own World-tier aggregate map must keep the fixed 720-wide
    /// viewBox and keep cropping, since it is never panned at runtime the
    /// way the explorer's shared copy is.
    #[test]
    fn world_listing_page_map_keeps_the_fixed_720_viewbox() {
        let table: toml::value::Table = toml::from_str(
            "[\"Near Pole\"]\nlat = 89.0\nlng = 0.0\nprecision = \"exact\"\n",
        )
        .unwrap();
        let gazetteer = crate::vault::places::parse_gazetteer(&table);
        let context = PlaceMapContext::embedded().unwrap();
        let names: Vec<String> = vec!["Near Pole".to_string()];
        let target = context.resolve_aggregate("places", &gazetteer, "places", &names);
        assert_eq!(target.tier(), FrameTier::World);
        let svg = emit_svg(&context, &target, "places/index.html", 0);
        assert!(svg.contains("viewBox=\"0 0 720 480\""), "a page's own world map must keep the fixed 720-wide viewBox: {svg:.200}");
    }

    /// A future explorer asset whose own `canvas_width` happens to equal
    /// `SVG_WIDTH` must still draw full extent when `Writer::full_extent`
    /// says so — the flag decides, not a width comparison that a same-width
    /// asset could pass by accident.
    #[test]
    fn full_extent_flag_wins_even_at_the_page_canvas_width() {
        let ids = Ids::new("p", 0);
        let writer = Writer {
            output: String::new(),
            ids: &ids,
            height_uses: Vec::new(),
            land_paths: Vec::new(),
            has_href: false,
            effect_scale: 1.0,
            detail: 1.0,
            locator_profile: None,
            canvas_width: f64::from(SVG_WIDTH),
            canvas_height: f64::from(SVG_HEIGHT),
            full_extent: true,
            tile: false,
        };
        let target = PlaceMapTarget { places: vec![], frame: world_frame(), aggregate_name: None, route: false };
        let projection = choose_projection(&writer, &target).unwrap();
        let width = world_viewbox_width();
        let (east_x, _) = projection.project(ProjectedPoint::new(180.0 - 1e-9, 0.0).unwrap()).unwrap();
        assert!((east_x - width).abs() < 1e-6, "180E should land at the full-extent width {width:.3}, got {east_x}");
    }

    /// The pack winds every outer ring one way and every hole the other,
    /// so fills use the nonzero rule: holes still cut out, and the two
    /// halves of a ring cut at the antimeridian, which overlap by a pixel,
    /// fill their overlap instead of cancelling it.
    #[test]
    fn main_map_fills_use_the_nonzero_rule() {
        let svg = locator_for(179.95, -16.85, Precision::Exact);
        let globe = svg.find("data-map-layer=\"globe\"").unwrap();
        assert!(!svg[..globe].contains("evenodd"), "a main-map fill uses even-odd");
    }

    #[test]
    fn path_serializer_closes_fills_but_not_lines() {
        let fill = serialize_path(&[vec![(1.2, 2.8), (4.1, 2.8), (4.1, 5.0)]], true, FINE).unwrap();
        let line = serialize_path(&[vec![(1.2, 2.8), (4.1, 2.8)]], false, FINE).unwrap();
        assert!(fill.ends_with('z'));
        assert!(!line.contains('z'));
        assert!(fill
            .chars()
            .all(|character| !character.is_ascii_digit() || character.is_ascii()));
    }

    #[test]
    fn populated_pack_emits_layers_bands_and_a_marker() {
        let context = PlaceMapContext::embedded().unwrap();
        let output = emit_svg(
            &context,
            &populated_target(Precision::City),
            "places/kyoto",
            0,
        );
        assert!(output.contains("data-map-layer=\"land\""));
        assert!(output.contains("data-map-layer=\"relief\""));
        assert!(output.contains("data-map-band=\"100\""));
        assert!(output.contains("fill-rule=\"evenodd\""));
        assert!(output.contains("data-map-marker=\"places/kyoto\""));
        assert!(!output.contains("country-border"));
        let document = scraper::Html::parse_fragment(&output);
        let svg_selector = scraper::Selector::parse("svg").unwrap();
        assert_eq!(document.select(&svg_selector).count(), 1);
    }

    #[test]
    fn tokens_have_deterministic_fallbacks_and_lighting_contract() {
        let output = emit_svg(
            &PlaceMapContext::embedded().unwrap(),
            &populated_target(Precision::Exact),
            "p",
            0,
        );
        for token in output.match_indices("var(--") {
            let end = output[token.0..].find(')').unwrap() + token.0;
            assert!(output[token.0..=end].contains(','), "token lacks fallback");
        }
        assert!(output.contains("feDropShadow"));
        // The approved cut-paper offset: small enough that a several-point
        // island does not read as a doubled "ghost" shape the way the
        // larger dx=3/dy=4/stdDeviation=0 this replaced did.
        assert!(output.contains("dx=\"1.4\" dy=\"1.4\" stdDeviation=\"0.7\""));
        assert!(output.contains("stdDeviation=\"2\""));
        // Two separately weighted diffuse passes (surfaceScale 78/34, the
        // approved design's own values) through feComponentTransfer hi/lo
        // masks — see
        // height_weighting_zeroes_flat_ground_and_matches_the_reference
        // below for the behavioural half of this contract.
        assert!(output.contains("surfaceScale=\"78\""));
        assert!(output.contains("surfaceScale=\"34\""));
        assert!(output.contains("feComponentTransfer"));
        assert!(output.contains("feFuncA"));
        assert!(output.contains("result=\"lit-hi-tint\""));
        assert!(output.contains("result=\"lit-lo-tint\""));
        assert!(output.contains("clip-path=\"url(#"));
        assert!(output.contains("data-map-layer=\"relief\""));
        let lighting = output.split("data-map-layer=\"lighting\"").nth(1).unwrap();
        assert!(!lighting.contains("layer-land-"));
        // The height-field source's own opacity, distinct from the 0.3
        // two-pass diffuse blend weight above (see the emitter's comment on
        // this literal for the defect that conflated the two).
        assert!(lighting.contains("opacity=\"0.5\""));
        // Height-field fill encodes elevation as luminance directly (a
        // per-band grey ramp) rather than a per-feature opacity over one
        // constant mid-grey, which could never read brighter than that
        // grey even at the top band. This frame (Kyoto) has real relief,
        // so at least one of the twelve ramp colors must actually appear
        // as a fill.
        let relief_greys: Vec<&str> = [
            100, 200, 400, 700, 1000, 1500, 2000, 2500, 3000, 4000, 5000, 6000,
        ]
        .into_iter()
        .map(relief_height_grey)
        .collect();
        assert!(
            relief_greys
                .iter()
                .any(|grey| lighting.contains(&format!("fill=\"{grey}\""))),
            "expected one of {relief_greys:?} as a height-field fill"
        );
    }

    /// The lighting filter's whole point, read off the filter's own
    /// numbers rather than eyeballed: a perfectly flat area (zero alpha
    /// gradient, so surfaceScale cannot matter) has diffuse intensity
    /// sin(elevation) — the same figure for both the steep and soft passes,
    /// so their 0.7/0.3 mix lands there too — and the hi/lo
    /// feComponentTransfer masks must evaluate to exactly zero at that
    /// point. That is what "flat ground carries no tone" means in this
    /// filter: not a separate flatness check, but the linear masks' own
    /// zero-crossing landing on the physically-flat baseline.
    #[test]
    fn height_weighting_zeroes_flat_ground_and_matches_the_reference() {
        let output = emit_svg(
            &PlaceMapContext::embedded().unwrap(),
            &populated_target(Precision::Exact),
            "p",
            0,
        );
        assert!(output.contains("azimuth=\"240\" elevation=\"45\""));
        let baseline = 45.0_f64.to_radians().sin();
        for mask_name in ["lit-hi-mask", "lit-lo-mask"] {
            let anchor = format!("result=\"{mask_name}\"><feFuncA type=\"linear\" slope=\"");
            let start = output.find(&anchor).unwrap_or_else(|| panic!("{mask_name} not found")) + anchor.len();
            let rest = &output[start..];
            let slope: f64 = rest[..rest.find('"').unwrap()].parse().unwrap();
            let rest = &rest[rest.find("intercept=\"").unwrap() + "intercept=\"".len()..];
            let intercept: f64 = rest[..rest.find('"').unwrap()].parse().unwrap();
            let at_baseline = slope * baseline + intercept;
            assert!(
                at_baseline.abs() < 1e-6,
                "{mask_name} (slope={slope}, intercept={intercept}) must zero out at the \
                 flat-ground baseline sin(45deg)={baseline}, got {at_baseline}"
            );
        }
    }

    /// The approved design lights each theme with its own gains: the
    /// highlight mask's slope is 1.878 in the light theme and 1.536 in the
    /// dark one, the shadow's 0.566 and 0.636. The filter carries one set,
    /// so each theme's strength tokens must bring it to that theme's gains.
    #[test]
    fn each_theme_lights_the_terrain_with_its_own_gains() {
        let output = emit_svg(&PlaceMapContext::embedded().unwrap(), &populated_target(Precision::Exact), "p", 0);
        let slope = |mask: &str| -> f64 {
            let anchor = format!("result=\"{mask}\"><feFuncA type=\"linear\" slope=\"");
            let rest = &output[output.find(&anchor).unwrap() + anchor.len()..];
            rest[..rest.find('"').unwrap()].parse::<f64>().unwrap().abs()
        };
        for (mask, token) in [("lit-hi-mask", "light-warm-strength"), ("lit-lo-mask", "light-cool-strength")] {
            assert!(output.contains(&format!("flood-opacity=\"var(--moss-place-{token}, ")), "{token} is not on its flood");
        }
        let css = include_str!("../../assets/css/site.css");
        let token = |block: &str, name: &str| -> f64 {
            let body = css_block(css, block);
            let rest = &body[body.find(&format!("--moss-place-{name}:")).unwrap_or_else(|| panic!("{name} missing")) + name.len() + 14..];
            rest[..rest.find(';').unwrap()].trim().parse().unwrap()
        };
        for (block, warm, cool) in [(".moss-place-map {", 1.8778, 0.5657), ("[data-theme=\"dark\"] .moss-place-map {", 1.5364, 0.6364)] {
            let hi = slope("lit-hi-mask") * token(block, "light-warm-strength");
            let lo = slope("lit-lo-mask") * token(block, "light-cool-strength");
            assert!((hi - warm).abs() < 1e-3 && (lo - cool).abs() < 1e-3, "{block} lights at {hi}/{lo}, not {warm}/{cool}");
        }
    }

    /// Every `--moss-place-*` custom property this emitter can reference
    /// (as a literal `var(--moss-place-NAME, #fallback)` call-site string,
    /// here or in the band palette) must be defined
    /// in the shipped stylesheet, in both the light and dark blocks — the
    /// defect this guards was that the emitter used tokens (rivers, ice,
    /// every relief/seafloor band, the globe-* trio...) `site.css` never
    /// declared, so they silently rendered as their Rust fallback colour
    /// instead of the approved theme.
    ///
    /// Source is scanned rather than the rendered SVG because an optional
    /// layer (rivers, lakes, salt...) only appears in the output when the
    /// test pack actually has data for it in the probed frame; the call-site
    /// string exists regardless of what data is present.
    #[test]
    fn every_place_token_the_emitter_can_produce_is_defined_in_site_css() {
        let svg_source = include_str!("svg.rs");
        let locator_source = include_str!("svg/locator.rs");
        let palette_source = include_str!("svg/palette.rs");
        let route_source = include_str!("svg/route.rs");
        let css = include_str!("../../assets/css/site.css");

        // Call-site literals: `var(--moss-place-NAME, #hex)`. The
        // name is cut at the first character outside `[a-z0-9-]` rather than
        // at the next comma, so this can't misfire on this very function's
        // own source text (this file's `include_str!` of itself) the way a
        // bare `find(',')` would.
        let mut tokens: Vec<String> = Vec::new();
        for source in [svg_source, locator_source, palette_source, route_source] {
            let mut rest = source;
            while let Some(start) = rest.find("var(--moss-place-") {
                let name_start = start + "var(--".len();
                let after = &rest[name_start..];
                let name_len = after
                    .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
                    .unwrap_or(after.len());
                if after[name_len..].starts_with(',') {
                    tokens.push(after[..name_len].to_string());
                }
                rest = &after[name_len.max(1)..];
            }
        }

        tokens.sort();
        tokens.dedup();
        assert!(tokens.len() > 20, "expected a rich token set, got {tokens:?}");

        let light_block = css_block(css, "\n.moss-place-map {");
        let dark_block = css_block(css, "\n[data-theme=\"dark\"] .moss-place-map {");
        for token in &tokens {
            let declaration = format!("--{token}:");
            assert!(
                light_block.contains(&declaration),
                "{token} has no light definition in .moss-place-map"
            );
            // --moss-place-marker deliberately inherits var(--moss-color-accent)'s
            // own dark override rather than repeating a second dark value.
            if token != "moss-place-marker" {
                assert!(
                    dark_block.contains(&declaration),
                    "{token} has no dark definition in [data-theme=\"dark\"] .moss-place-map"
                );
            }
        }
    }

    /// The property list of one `selector {`, up to its closing brace. Panics
    /// (loudly, with the selector) if the selector or its close is missing —
    /// a silent empty block would make every token assertion above pass for
    /// the wrong reason.
    fn css_block<'a>(css: &'a str, selector_with_brace: &str) -> &'a str {
        let start = css
            .find(selector_with_brace)
            .unwrap_or_else(|| panic!("{selector_with_brace} not found in site.css"))
            + selector_with_brace.len();
        let end = css[start..]
            .find('}')
            .unwrap_or_else(|| panic!("{selector_with_brace} has no closing brace"));
        &css[start..start + end]
    }

    #[test]
    fn river_width_tapers_by_rank_to_a_floor() {
        let widths: Vec<f64> = [0, 1, 3, 6, 7, 8, 12].into_iter().map(river::river_width).collect();
        let expected = [0.8, 0.73, 0.59, 0.38, 0.31, 0.25, 0.25];
        for (width, expected) in widths.iter().zip(expected) {
            assert!((width - expected).abs() < 1e-9, "{widths:?}");
        }
    }

    /// The Levant frame holds the Nile's delta branches (rank 1) and the
    /// Jordan (rank 6): their strokes must taper by the pack's own rank.
    #[test]
    fn locator_rivers_taper_by_their_scalerank() {
        let svg = locator_for(35.5, 33.89, Precision::Exact);
        let rivers = layer_body(&svg, "rivers");
        for width in ["0.73", "0.38"] {
            assert!(rivers.contains(&format!("stroke-width=\"{width}\"")), "no {width} stroke in {rivers:.300}");
        }
    }

    #[test]
    fn href_is_escaped_and_accessibly_named() {
        let context = PlaceMapContext::embedded().unwrap();
        let mut options = SvgMapOptions::new("p", 0, "A < place", Precision::City);
        options.href = Some("/places/a?x=1&y=\"two\"");
        let output = emit_svg_with_options(&context, &populated_target(Precision::City), options);
        assert!(output.contains("href=\"/places/a?x=1&amp;y=&quot;two&quot;\""));
        assert!(output.contains("<a href=\"/places/a?x=1&amp;y=&quot;two&quot;\" aria-label=\"Map of A &lt; place, city precision\">"));
        assert!(output.ends_with("</a></figure>") || output.ends_with("</figure>"));
    }

    /// A listing map marks every member place by its own precision, soft
    /// fades under dots, and several places on one pixel with one shape
    /// make one marker. Its label names the listing, not a precision.
    #[test]
    fn listing_maps_mark_each_member_once_by_its_own_precision() {
        let place = |key: &str, longitude: f64, latitude: f64, precision| ResolvedPlace {
            key: key.to_string(),
            display: key.to_string(),
            longitude: Some(longitude),
            latitude: Some(latitude),
            precision,
        };
        let places = vec![
            place("places/old-town", 135.77, 35.01, Precision::Exact),
            place("places/kyoto", 135.77, 35.01, Precision::City),
            place("places/kansai", 135.5, 34.7, Precision::Region),
            place("places/nara", 135.8, 34.68, Precision::City),
        ];
        let points: Vec<ProjectedPoint> = places.iter().filter_map(ResolvedPlace::point).collect();
        let frame = Frame::from_points(&points, places.iter().map(|place| place.precision)).unwrap();
        let target = PlaceMapTarget { places, frame: Some(frame), aggregate_name: Some("places".to_string()), route: false };
        let svg = emit_svg(&PlaceMapContext::embedded().unwrap(), &target, "p", 0);
        let markers = layer_body(&svg, "marker");
        let marked: Vec<&str> = markers
            .match_indices("data-map-marker=\"")
            .map(|(index, _)| markers[index + 17..].split('"').next().unwrap())
            .collect();
        assert_eq!(marked, ["places/kansai", "places/old-town", "places/nara"], "{markers}");
        assert!(svg.contains("aria-label=\"Map of places\""));
    }

    /// The globe's coast is its land rings stroked: every land feature it
    /// fills has a coast line of its own.
    #[test]
    fn globe_strokes_its_coast_from_the_land_rings() {
        let svg = locator_for(35.5, 33.89, Precision::Exact);
        let globe = &svg[svg.find("data-map-layer=\"globe\"").unwrap()..];
        let features = |paint: &str| -> Vec<String> {
            globe
                .split("<path ")
                .filter(|tag| tag.contains(paint))
                .map(|tag| tag[tag.find("data-globe-feature=\"").unwrap() + 20..].split('"').next().unwrap().to_string())
                .collect()
        };
        let coasts = features("stroke=\"var(--moss-place-globe-coast, #f5f6f4)\"");
        let lands = features("fill=\"var(--moss-place-globe-land, #e3e6d5)\"");
        assert!(!lands.is_empty());
        assert!(lands.iter().all(|land| coasts.contains(land)), "coast {coasts:?}, land {lands:?}");
    }

    #[test]
    fn globe_inset_marks_only_real_target_points() {
        let context = PlaceMapContext::embedded().unwrap();
        let output = emit_svg(&context, &populated_target(Precision::City), "p", 0);
        assert!(output.contains("data-map-globe-marker=\"true\""));
        let empty = PlaceMapTarget { places: vec![], frame: None, aggregate_name: None, route: false };
        assert!(!emit_svg(&context, &empty, "p", 0).contains("data-map-globe-marker=\"true\""));
    }

    #[test]
    fn globe_clips_horizon_crossings_and_uses_short_dateline_edges() {
        let center = ProjectedPoint::new(180.0, 0.0).unwrap();
        let line = globe_line(&[(1790000, 0), (-1790000, 0)], center, 10000);
        assert!(!line.is_empty());
        assert!(line.iter().flatten().all(|&(x, y)| {
            (x - GLOBE_CENTER_X).hypot(y - GLOBE_CENTER_Y) <= GLOBE_RADIUS + 1.0
        }));
        let horizon = globe_line(
            &[(0, 0), (1800000, 0)],
            ProjectedPoint::new(0.0, 0.0).unwrap(),
            10000,
        );
        assert!(horizon
            .iter()
            .flatten()
            .any(|&(x, y)| (x - GLOBE_CENTER_X).hypot(y - GLOBE_CENTER_Y) > GLOBE_RADIUS - 1.0));
        let rings = globe_rings(
            &[(0, 0), (1200000, 700000), (1200000, -700000), (0, 0)],
            ProjectedPoint::new(0.0, 0.0).unwrap(),
            10000,
        );
        assert!(!rings.is_empty());
    }

    #[test]
    fn representative_local_locator_stays_under_brotli_budget() {
        let output = emit_svg(
            &PlaceMapContext::embedded().unwrap(),
            &populated_target(Precision::City),
            "places/kyoto",
            0,
        );
        let mut compressor = brotli::CompressorWriter::new(Vec::new(), 4096, 11, 22);
        compressor.write_all(output.as_bytes()).unwrap();
        let compressed = compressor.into_inner();
        assert!(
            compressed.len() <= LOCATOR_Q11_BROTLI_LIMIT,
            "raw={} brotli-q11={}",
            output.len(),
            compressed.len()
        );
    }

    #[test]
    fn locator_q11_matrix_covers_dense_dateline_and_polar_targets() {
        let context = PlaceMapContext::embedded().unwrap();
        for (case_name, point) in [
            ("beirut-dense", ProjectedPoint::new(35.5, 33.9).unwrap()),
            ("dateline", ProjectedPoint::new(179.8, 0.0).unwrap()),
            ("polar", ProjectedPoint::new(179.8, 84.0).unwrap()),
        ] {
            for precision in [
                Precision::Exact,
                Precision::City,
                Precision::Region,
                Precision::Country,
            ] {
                let frame = Frame::from_points(&[point], [precision].into_iter()).unwrap();
                let target = PlaceMapTarget {
                    places: vec![ResolvedPlace {
                        key: format!("places/{case_name}&b"),
                        display: "A\u{1} & B".to_string(),
                        longitude: Some(point.longitude),
                        latitude: Some(point.latitude),
                        precision,
                    }],
                    frame: Some(frame),
                    aggregate_name: None,
                    route: false,
                };
                let options = SvgMapOptions::new(
                    case_name,
                    precision_rank(&precision) as usize,
                    "A\u{1}",
                    precision,
                );
                let first = emit_locator(&context, &target, options).unwrap();
                let second = emit_locator(&context, &target, options).unwrap();
                assert_eq!(first, second);
                let mut compressor = brotli::CompressorWriter::new(Vec::new(), 4096, 11, 22);
                compressor.write_all(first.svg.as_bytes()).unwrap();
                let compressed = compressor.into_inner();
                assert!(
                    compressed.len() <= LOCATOR_Q11_BROTLI_LIMIT,
                    "{case_name}/{precision:?}: raw={} brotli-q11={}",
                    first.svg.len(),
                    compressed.len()
                );
                let document = roxmltree::Document::parse(&first.svg).unwrap();
                let is_geography_path = |node: roxmltree::Node<'_, '_>| {
                    node.has_tag_name("path")
                        && node.ancestors().any(|ancestor| {
                            matches!(
                                ancestor.attribute("data-map-layer"),
                                Some("land" | "coast" | "seafloor" | "relief" | "globe")
                            )
                        })
                };
                assert!(
                    document.descendants().any(is_geography_path),
                    "{case_name}/{precision:?} must retain real basemap geometry"
                );
                assert!(document.descendants().any(|node| {
                    node.has_tag_name("path")
                        && node.ancestors().any(|ancestor| {
                            ancestor.attribute("data-map-layer") == Some("globe")
                        })
                }), "{case_name}/{precision:?} must retain globe geography");
                if case_name == "beirut-dense"
                    && matches!(precision, Precision::Exact | Precision::City)
                {
                    assert!(
                        document
                            .descendants()
                            .any(|node| node.attribute("data-map-band").is_some()),
                        "exact/city locator must retain terrain bands"
                    );
                }
                assert!(first.svg.contains("data-map-globe-marker=\"true\""));
                assert!(first.svg.contains("A�"));
            }
        }
    }

    fn locator_for(longitude: f64, latitude: f64, precision: Precision) -> String {
        let point = ProjectedPoint::new(longitude, latitude).unwrap();
        let frame = Frame::from_points(&[point], [precision].into_iter()).unwrap();
        let target = PlaceMapTarget {
            places: vec![ResolvedPlace {
                key: "places/probe".to_string(),
                display: "Probe".to_string(),
                longitude: Some(point.longitude),
                latitude: Some(point.latitude),
                precision,
            }],
            frame: Some(frame),
            aggregate_name: None,
            route: false,
        };
        let options = SvgMapOptions::new("p", 0, "Probe", precision);
        emit_locator(&PlaceMapContext::embedded().unwrap(), &target, options).unwrap().svg
    }

    fn layer_body<'a>(svg: &'a str, name: &str) -> &'a str {
        let marker = format!("data-map-layer=\"{name}\"");
        let start = svg.find(&marker).unwrap_or_else(|| panic!("no {name} layer")) + marker.len();
        let body = &svg[start..];
        if body.starts_with("/>") { "" } else { &body[..body.find("</g>").unwrap()] }
    }

    /// The Levant frame holds the Sea of Galilee, the Dead Sea, the Jordan
    /// and Beirut's built-up area: a locator must draw them, above the lit
    /// terrain, in the approved paint order.
    #[test]
    fn locators_draw_the_physical_layers_in_the_approved_order() {
        let svg = locator_for(35.5, 33.89, Precision::Exact);
        for name in ["lakes", "rivers", "built-up"] {
            assert!(layer_body(&svg, name).contains("<path"), "{name} is empty in the Beirut locator");
        }
        let order = ["lighting", "ice", "salt", "lakes", "rivers", "reefs", "built-up", "marker"];
        let positions: Vec<usize> = order
            .iter()
            .map(|name| svg.find(&format!("data-map-layer=\"{name}\"")).unwrap())
            .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]), "{order:?} at {positions:?}");
    }

    /// A region locator shares the exact/city frame, so it keeps the same
    /// terrain: its precision shows in the marker, not in missing layers.
    #[test]
    fn region_locators_keep_relief_and_sea_floor() {
        let svg = locator_for(40.5, 36.0, Precision::Region);
        assert!(svg.contains("data-map-locator-profile=\"region\""));
        for name in ["seafloor", "relief"] {
            assert!(layer_body(&svg, name).contains("data-map-band"), "{name} is empty");
        }
    }

    fn world_frame() -> Option<Frame> {
        let polar = ProjectedPoint::new(0.0, 89.0).unwrap();
        let frame = Frame::from_points(&[polar], [Precision::Exact].into_iter()).unwrap();
        assert_eq!(frame.tier, FrameTier::World);
        Some(frame)
    }

    fn world_map() -> String {
        let target = PlaceMapTarget { places: vec![], frame: world_frame(), aggregate_name: None, route: false };
        emit_svg(&PlaceMapContext::embedded().unwrap(), &target, "p", 0)
    }

    /// The first number an attribute takes after `anchor`.
    fn number_after(svg: &str, anchor: &str, attribute: &str) -> f64 {
        let from = svg.find(anchor).unwrap_or_else(|| panic!("no {anchor}"));
        let rest = &svg[from..];
        let rest = &rest[rest.find(&format!("{attribute}=\"")).unwrap() + attribute.len() + 2..];
        rest[..rest.find('"').unwrap()].parse().unwrap()
    }

    /// The cut-paper shadows, the coast halo and the lighting blur were
    /// drawn for a 10-degree frame, about 70 px to the degree. The world
    /// map has about 2.3, so each shrinks by the same ratio: at full size,
    /// every small band ring on the world map casts a shadow 0.6 degrees
    /// long, and the terrain reads as shards.
    #[test]
    fn effects_shrink_with_the_frame_on_the_world_map() {
        let locator = locator_for(35.5, 33.89, Precision::Exact);
        let world = world_map();
        for (anchor, attribute, full) in [
            ("<feDropShadow", "dx", 1.4),
            ("data-map-layer=\"coast\"", "stroke-width", 10.0),
            ("-height-filter\"", "stdDeviation", 9.0),
            ("-height-filter\"", "surfaceScale", 78.0),
        ] {
            assert_eq!(number_after(&locator, anchor, attribute), full, "{attribute} after {anchor}");
            let ratio = number_after(&world, anchor, attribute) / full;
            assert!((0.02..0.05).contains(&ratio), "{attribute} after {anchor} kept {ratio} of its size");
        }
    }

    /// A multi-place story's target, its places named by their compass
    /// direction.
    fn story_target(places: &[(&str, f64, f64, Precision)], tier: FrameTier) -> PlaceMapTarget {
        let places: Vec<ResolvedPlace> = places
            .iter()
            .map(|&(key, longitude, latitude, precision)| ResolvedPlace {
                key: format!("places/{key}"),
                display: key.to_string(),
                longitude: Some(longitude),
                latitude: Some(latitude),
                precision,
            })
            .collect();
        let points: Vec<ProjectedPoint> = places.iter().filter_map(ResolvedPlace::point).collect();
        let frame = Frame::from_points(&points, places.iter().map(|place| place.precision)).unwrap();
        assert_eq!(frame.tier, tier);
        PlaceMapTarget { places, frame: Some(frame), aggregate_name: None, route: false }
    }

    /// From London to Hong Kong by way of Kerala: over a hundred degrees.
    fn continental_target() -> PlaceMapTarget {
        story_target(&[("north", -0.128, 51.507, Precision::City), ("east", 114.177, 22.302, Precision::City), ("south", 76.271, 10.851, Precision::Region)], FrameTier::Wide)
    }

    /// From Bangkok to Shanghai: a local frame by span, under 60 degrees,
    /// but at a third of the design's zoom.
    fn regional_target() -> PlaceMapTarget {
        story_target(&[("south", 100.5, 13.75, Precision::City), ("north", 121.474, 31.23, Precision::City)], FrameTier::Local)
    }

    /// A story set in three places across two continents gets its locator:
    /// drawn for its display size and with every other elevation step, it
    /// fits the wide locator budget, where it used to pass the raw ceiling
    /// and be dropped.
    #[test]
    fn a_continental_locator_is_emitted_within_its_budget() {
        for (target, precision) in [(continental_target(), Precision::Region), (regional_target(), Precision::City)] {
            let options = SvgMapOptions::new("p", 0, "north", precision);
            let locator = emit_locator(&PlaceMapContext::embedded().unwrap(), &target, options).expect("the locator is emitted, not dropped");
            let mut compressor = brotli::CompressorWriter::new(Vec::new(), 4096, 11, 22);
            compressor.write_all(locator.svg.as_bytes()).unwrap();
            let compressed = compressor.into_inner();
            assert!(compressed.len() <= WIDE_LOCATOR_Q11_BROTLI_LIMIT, "raw={} brotli-q11={}", locator.svg.len(), compressed.len());
            let relief = &locator.svg[locator.svg.find("data-map-layer=\"relief\"").unwrap()..locator.svg.find("data-map-layer=\"lighting\"").unwrap()];
            assert!(relief.contains("data-map-band=\"6000\"") && !relief.contains("data-map-band=\"5000\""), "the highest step and every other one below it");
        }
    }

    /// The zoom cut-off between the local and wide drawing treatments sits
    /// at a padded longitude span of `SVG_WIDTH / (WIDE_ZOOM *
    /// DESIGN_PIXELS_PER_DEGREE)` degrees (see `Projection::effect_scale`).
    /// Both frames built here keep `FrameTier::Local` (well under the
    /// 60-degree tier boundary), so the two locators differ only by zoom,
    /// never by which data the frame selects — the split `open_figure`'s
    /// doc comment describes for `data-map-tier`.
    #[test]
    fn a_locator_switches_treatment_at_the_wide_zoom_cutoff() {
        let cutoff = f64::from(SVG_WIDTH) / (WIDE_ZOOM * DESIGN_PIXELS_PER_DEGREE);
        // Half a degree of padded span clears `effect_scale`'s 3-decimal
        // rounding on either side, so which side of the cut-off each case
        // lands on isn't a coin flip.
        let margin = 0.5;
        for (padded_span, wide, limit) in [
            (cutoff - margin, false, LOCATOR_Q11_BROTLI_LIMIT),
            (cutoff + margin, true, WIDE_LOCATOR_Q11_BROTLI_LIMIT),
        ] {
            let raw_span = padded_span / FRAME_PADDING;
            let target = story_target(
                &[
                    ("west", 20.0 - raw_span / 2.0, 0.0, Precision::City),
                    ("east", 20.0 + raw_span / 2.0, 0.0, Precision::City),
                ],
                FrameTier::Local,
            );
            assert_eq!(
                Projection::new(target.frame.as_ref().unwrap()).is_wide(),
                wide,
                "padded span {padded_span:.2} against cutoff {cutoff:.2}"
            );
            let options = SvgMapOptions::new("p", 0, "west", Precision::City);
            let locator = emit_locator(&PlaceMapContext::embedded().unwrap(), &target, options)
                .expect("the locator is emitted, not dropped");
            let mut compressor = brotli::CompressorWriter::new(Vec::new(), 4096, 11, 22);
            compressor.write_all(locator.svg.as_bytes()).unwrap();
            let compressed = compressor.into_inner();
            assert!(
                compressed.len() <= limit,
                "padded span {padded_span:.2}: raw={} brotli-q11={} limit={limit}",
                locator.svg.len(),
                compressed.len()
            );
        }
    }

    /// A multi-place frame is wider and taller than its places span, so the
    /// places at its edges are drawn inside the map: spanned exactly, the
    /// westernmost and easternmost sat on its border.
    #[test]
    fn places_at_a_frames_edges_sit_inside_the_map() {
        let svg = emit_svg(&PlaceMapContext::embedded().unwrap(), &continental_target(), "p", 0);
        let markers = layer_body(&svg, "marker");
        let mut centres: Vec<(f64, f64)> = dots(markers).into_iter().map(|(x, y, _)| (x, y)).collect();
        centres.extend(markers.split("<circle cx=\"").skip(1).map(|rest| {
            let mut numbers = rest.split('"').step_by(2).map(|value| value.parse::<f64>().unwrap());
            (numbers.next().unwrap(), numbers.next().unwrap())
        }));
        assert_eq!(centres.len(), 5, "two dots with their casings and one fade: {markers}");
        for (x, y) in centres {
            assert!((60.0..=660.0).contains(&x) && (40.0..=440.0).contains(&y), "a marker at ({x}, {y}) sits on the map's border");
        }
    }

    #[test]
    fn wide_maps_keep_trunk_rivers_and_no_specks() {
        let context = PlaceMapContext::embedded().unwrap();
        let wide = [continental_target(), regional_target()].map(|target| emit_svg(&context, &target, "p", 0));
        for svg in [vec![world_map()], wide.to_vec()].concat() {
            let rivers = layer_body(&svg, "rivers");
            let widths: Vec<f64> = rivers
                .match_indices("stroke-width=\"")
                .map(|(index, _)| {
                    let rest = &rivers[index + "stroke-width=\"".len()..];
                    rest[..rest.find('"').unwrap()].parse().unwrap()
                })
                .collect();
            assert!(!widths.is_empty(), "the map lost its trunk rivers");
            let floor = river::river_width(WORLD_MAX_RIVER_RANK);
            assert!(widths.iter().all(|&width| width >= floor - 1e-9), "{widths:?}");
            for name in ["reefs", "salt", "built-up"] {
                assert!(!layer_body(&svg, name).contains("<path"), "the map draws {name}");
            }
        }
    }

    /// The places root inlines its world map, so the map's size is page
    /// weight on every visit: 82.3 KB of brotli today, drawn from the
    /// fine tier. It may only grow past 96 KiB with a stated reason.
    #[test]
    fn the_world_map_stays_under_its_inline_budget() {
        const WORLD_MAP_Q11_BROTLI_LIMIT: usize = 96 * 1024;
        let output = world_map();
        let mut compressor = brotli::CompressorWriter::new(Vec::new(), 4096, 11, 22);
        compressor.write_all(output.as_bytes()).unwrap();
        let compressed = compressor.into_inner();
        assert!(compressed.len() <= WORLD_MAP_Q11_BROTLI_LIMIT, "raw={} brotli-q11={}", output.len(), compressed.len());
    }

    #[test]
    fn the_world_map_has_no_globe_inset() {
        assert!(!world_map().contains("data-map-layer=\"globe\""));
        assert!(locator_for(35.5, 33.89, Precision::Exact).contains("data-map-layer=\"globe\""));
    }

    #[test]
    fn xml_escape_replaces_xml_forbidden_controls() {
        let escaped = xml_escape("ok\u{0}\u{8}\u{b}\u{c}\u{1f}");
        assert_eq!(escaped, "ok�����");
    }
}
