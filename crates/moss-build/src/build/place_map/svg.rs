//! Deterministic, self-contained SVG emission for a place map.
//!
//! The emitter deliberately knows nothing about pages, configuration, or CSS
//! files.  Its only inputs are the immutable decoded pack and a resolved map
//! target.  That makes the byte output suitable for page snapshots and keeps
//! the geometry/privacy boundary in [`super::context`] and [`super::geometry`].

use std::fmt::Write;

use sha2::{Digest, Sha256};

mod path;
use path::{serialize_path, snap, BANDS, FINE};
mod text;
use text::{precision_name, xml_escape};
// Re-exported (via place_map.rs) so context.rs's locator can derive a
// profile from every resolved place's precision, the same rule emit_svg
// already applies to the full/aggregate map — see the doc comment on
// render_locator's fix for what picking only the first place's precision
// broke.
pub(crate) use text::precision_rank;
mod locator;
pub use locator::{
    emit_locator, emit_locator_svg, LocatorProfile, LocatorSafetyError, LocatorSvg,
    LOCATOR_Q11_BROTLI_LIMIT, LOCATOR_RAW_SAFETY_LIMIT,
};
mod river;
mod palette;
use palette::{band_tint, relief_height_grey};

use super::geometry::{marker_radius, FrameTier, ProjectedPoint, Projection, TileSelection, REGION_FADE_DEGREES};
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

/// Alias used by render callers that prefer the verb used by other SVG
/// components in the build crate.
pub fn render_svg(
    context: &PlaceMapContext,
    target: &PlaceMapTarget,
    page_path: &str,
    ordinal: usize,
) -> String {
    emit_svg(context, target, page_path, ordinal)
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
    let label = if options.location.is_empty() {
        format!("Place map, {} precision", precision_name(options.precision))
    } else {
        format!(
            "Map of {}, {} precision",
            options.location,
            precision_name(options.precision)
        )
    };
    let mut writer = Writer {
        output: String::with_capacity(32 * 1024),
        ids: &ids,
        height_uses: Vec::new(),
        land_paths: Vec::new(),
        has_href: false,
        locator_profile: compact.then_some(match options.precision {
            Precision::Exact | Precision::City => LocatorProfile::ExactCity,
            Precision::Region => LocatorProfile::Region,
            Precision::Country => LocatorProfile::Country,
        }),
    };
    writer.open_figure(
        target,
        options.location,
        options.precision,
        &label,
        options.href,
    );
    writer.open_svg();
    writer.defs();
    writer.water();

    let projection = target.frame.as_ref().map(Projection::new);
    let grouped = target.frame.as_ref().map(|frame| {
        let selected = TileSelection::for_frame(context.pack(), frame);
        let mut grouped = grouped_features(context.pack(), &selected);
        if frame.tier == FrameTier::World {
            keep_world_scale_layers(&mut grouped);
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
    writer.markers(target);
    writer.globe(context, target);
    writer.close_svg();
    writer.output
}

#[derive(Debug, Clone)]
struct Ids {
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

    fn get(&self, name: &str) -> String {
        format!("{}-{name}", self.base)
    }
}

struct Writer<'a> {
    output: String,
    ids: &'a Ids,
    height_uses: Vec<String>,
    land_paths: Vec<String>,
    has_href: bool,
    locator_profile: Option<LocatorProfile>,
}

/// Natural Earth scalerank of the smallest river a world map still draws.
const WORLD_MAX_RIVER_RANK: i16 = 3;

/// At world scale the river taper's 0.5 px floor still draws every
/// tributary as a thread, and reef lines simplified for a whole-globe frame
/// read as scratches across the ocean. A world map keeps trunk rivers only
/// and no reefs.
fn keep_world_scale_layers(grouped: &mut [Vec<&Feature>]) {
    grouped[4].retain(|feature| feature.band <= WORLD_MAX_RIVER_RANK);
    grouped[6].clear();
}

/// Whether any part of a projected ring falls inside the 720x480 view,
/// rather than only in the clip margin around it.
fn on_screen(ring: &[(f64, f64)]) -> bool {
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    for &(x, y) in ring {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    min_x < f64::from(SVG_WIDTH) && max_x > 0.0 && min_y < f64::from(SVG_HEIGHT) && max_y > 0.0
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
        write!(
            self.output,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{SVG_WIDTH}\" height=\"{SVG_HEIGHT}\" viewBox=\"0 0 {SVG_WIDTH} {SVG_HEIGHT}\" aria-hidden=\"true\" focusable=\"false\">",
        )
        .expect("writing to String cannot fail");
    }

    fn close_svg(&mut self) {
        self.output.push_str("</svg>");
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
        write!(
            self.output,
            "<defs><filter id=\"{height_filter}\" color-interpolation-filters=\"sRGB\"><feColorMatrix in=\"SourceGraphic\" type=\"luminanceToAlpha\" result=\"height-alpha\"/><feGaussianBlur in=\"height-alpha\" stdDeviation=\"9\" result=\"height-blur-9\"/><feColorMatrix in=\"height-blur-9\" type=\"matrix\" values=\"0 0 0 1 0  0 0 0 1 0  0 0 0 1 0  0 0 0 1 0\" result=\"height-coverage\"/><feDiffuseLighting in=\"height-blur-9\" surfaceScale=\"78\" diffuseConstant=\"1\" lighting-color=\"#ffffff\" result=\"lit-steep\"><feDistantLight azimuth=\"240\" elevation=\"45\"/></feDiffuseLighting><feColorMatrix in=\"lit-steep\" type=\"matrix\" values=\"0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  1 0 0 0 0\" result=\"lit-steep-alpha\"/><feGaussianBlur in=\"height-alpha\" stdDeviation=\"4\" result=\"height-blur-4\"/><feDiffuseLighting in=\"height-blur-4\" surfaceScale=\"34\" diffuseConstant=\"1\" lighting-color=\"#ffffff\" result=\"lit-soft\"><feDistantLight azimuth=\"240\" elevation=\"45\"/></feDiffuseLighting><feColorMatrix in=\"lit-soft\" type=\"matrix\" values=\"0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  1 0 0 0 0\" result=\"lit-soft-alpha\"/><feComposite in=\"lit-steep-alpha\" in2=\"lit-soft-alpha\" operator=\"arithmetic\" k1=\"0\" k2=\"0.7\" k3=\"0.3\" k4=\"0\" result=\"lit-mix\"/><feComponentTransfer in=\"lit-mix\" result=\"lit-hi-mask\"><feFuncA type=\"linear\" slope=\"1.877817459305202\" intercept=\"-1.327817459305202\"/></feComponentTransfer><feComponentTransfer in=\"lit-mix\" result=\"lit-lo-mask\"><feFuncA type=\"linear\" slope=\"-0.5656854249492381\" intercept=\"0.4\"/></feComponentTransfer><feFlood flood-color=\"var(--moss-place-light-warm, #fff3d0)\" result=\"lit-hi-flood\"/><feComposite in=\"lit-hi-flood\" in2=\"lit-hi-mask\" operator=\"in\" result=\"lit-hi-tint\"/><feFlood flood-color=\"var(--moss-place-light-cool, #5a6488)\" result=\"lit-lo-flood\"/><feComposite in=\"lit-lo-flood\" in2=\"lit-lo-mask\" operator=\"in\" result=\"lit-lo-tint\"/><feMerge result=\"lit-combined\"><feMergeNode in=\"lit-lo-tint\"/><feMergeNode in=\"lit-hi-tint\"/></feMerge><feGaussianBlur in=\"lit-combined\" stdDeviation=\"2\" result=\"lit-smooth\"/><feComposite in=\"lit-smooth\" in2=\"height-coverage\" operator=\"arithmetic\" k1=\"1\" k2=\"0\" k3=\"0\" k4=\"0\"/></filter><radialGradient id=\"{marker_gradient}\"><stop offset=\"0\" stop-color=\"var(--moss-place-marker, #2d5a2d)\" stop-opacity=\"0.28\"/><stop offset=\"1\" stop-color=\"var(--moss-place-marker, #2d5a2d)\" stop-opacity=\"0\"/></radialGradient><clipPath id=\"{globe_clip}\"><circle cx=\"{GLOBE_CENTER_X:.0}\" cy=\"{GLOBE_CENTER_Y:.0}\" r=\"{GLOBE_RADIUS:.0}\"/></clipPath><path id=\"{height_empty}\" d=\"m0 0l0 0\"/><filter id=\"{}\" filterUnits=\"userSpaceOnUse\" x=\"-24\" y=\"-24\" width=\"768\" height=\"528\"><feGaussianBlur stdDeviation=\"2.4\"/></filter></defs>", self.ids.get("soft"),
        )
        .expect("writing to String cannot fail");
        // The approved design's two cut-paper shadows, one per band family,
        // shared by every band group: relief casts a firmer shadow (and, in
        // the dark theme, a faint warm top-left edge) than the sea floor.
        // The land fill itself casts none.
        write!(
            self.output,
            "<defs><filter id=\"{}\" color-interpolation-filters=\"sRGB\" filterUnits=\"userSpaceOnUse\" x=\"-12\" y=\"-12\" width=\"744\" height=\"504\"><feDropShadow in=\"SourceGraphic\" dx=\"1.4\" dy=\"1.4\" stdDeviation=\"0.7\" flood-color=\"var(--moss-place-shadow, #5a6488)\" flood-opacity=\"0.35\" result=\"with-shadow\"/><feDropShadow in=\"SourceGraphic\" dx=\"-0.7\" dy=\"-0.7\" stdDeviation=\"0.49\" flood-color=\"var(--moss-place-light-warm, #fff3d0)\" flood-opacity=\"var(--moss-place-relief-edge-opacity, 0)\" result=\"with-edge\"/><feMerge><feMergeNode in=\"with-shadow\"/><feMergeNode in=\"with-edge\"/></feMerge></filter><filter id=\"{}\" color-interpolation-filters=\"sRGB\" filterUnits=\"userSpaceOnUse\" x=\"-12\" y=\"-12\" width=\"744\" height=\"504\"><feDropShadow in=\"SourceGraphic\" dx=\"1.0\" dy=\"1.0\" stdDeviation=\"0.6\" flood-color=\"var(--moss-place-shadow, #5a6488)\" flood-opacity=\"0.22\"/></filter>",
            self.ids.get("shadow-relief"),
            self.ids.get("shadow-seafloor"),
        )
        .expect("writing to String cannot fail");
        self.output.push_str("</defs>");
    }

    fn water(&mut self) {
        let id = self.ids.get("layer-water");
        write!(
            self.output,
            "<g id=\"{id}\" data-map-layer=\"water\"><rect width=\"{SVG_WIDTH}\" height=\"{SVG_HEIGHT}\" fill=\"var(--moss-place-water, #e9eff2)\"/></g>",
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
        self.coast();
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
            if let Some(path) = serialize_path(&projection.project_feature(&rings, quantisation), true, FINE) {
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
            write!(self.output, "<use href=\"#{id}\" fill=\"none\" stroke=\"var(--moss-place-coast, #f5f6f4)\" stroke-opacity=\"var(--moss-place-coast-opacity, 0.6)\" stroke-width=\"10\"/>")
                .expect("writing to String cannot fail");
        }
        self.output.push_str("</g>");
    }

    pub(super) fn land(&mut self) {
        write!(self.output, "<g id=\"{}\" data-map-layer=\"land\">", self.ids.get("layer-land"))
            .expect("writing to String cannot fail");
        for id in &self.land_paths {
            write!(self.output, "<use href=\"#{id}\" fill=\"var(--moss-place-land, #d7d5c9)\"/>")
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
        self.emit_merged_layer(quantisation, projection, grouped, 3, "lakes", &fill("var(--moss-place-lakes, #a9c6d8)"));
        self.emit_river_layer(quantisation, projection, grouped, "var(--moss-place-rivers, #5b93bd)");
        // Natural Earth's reefs are lines, not areas.
        self.emit_merged_layer(quantisation, projection, grouped, 6, "reefs", "fill=\"none\" stroke=\"var(--moss-place-reefs, #b9d6cf)\" stroke-width=\"1.2\"");
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
        if let Some(path) = serialize_path(&paths, filled, FINE) {
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
                let on_screen = rings.iter().any(|ring| on_screen(ring));
                (feature, on_screen.then(|| serialize_path(&rings, true, BANDS)).flatten())
            })
            .filter(|(_, path)| path.is_some())
            .collect();
        let mut present: Vec<i16> = projected.iter().map(|(feature, _)| feature.band).collect();
        present.dedup();
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
                    "<g data-map-band=\"{}\" style=\"fill:{}\" filter=\"url(#{})\">",
                    feature.band,
                    band_tint(name, feature.band, &present),
                    self.ids.get(&format!("shadow-{name}"))
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
            for place in target.marker_places() {
                let Some(point) = place.point().and_then(|point| projection.project(point)) else {
                    continue;
                };
                let (x, y) = (snap(point.0), snap(point.1));
                let key = xml_escape(&place.key);
                match place.precision {
                    Precision::Exact | Precision::City => {
                        let radius = marker_radius(place.precision);
                        write!(self.output, "<circle cx=\"{x}\" cy=\"{y}\" r=\"{:.0}\" fill=\"var(--moss-place-marker-casing, #ffffff)\"/><circle cx=\"{x}\" cy=\"{y}\" r=\"{radius:.0}\" fill=\"var(--moss-place-marker, #2d5a2d)\" data-map-marker=\"{key}\"/>", radius + 1.0).expect("writing to String cannot fail");
                    }
                    Precision::Region | Precision::Country => {
                        let fade_edge = place.point().filter(|_| place.precision == Precision::Region).and_then(|centre| {
                            ProjectedPoint::new(centre.longitude, (centre.latitude + REGION_FADE_DEGREES).min(89.9))
                        });
                        let radius = fade_edge
                            .and_then(|edge| projection.project(edge))
                            .map_or(marker_radius(place.precision), |edge| (edge.0 - point.0).hypot(edge.1 - point.1));
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
        write!(self.output, "<g id=\"{id}\" data-map-layer=\"globe\" clip-path=\"url(#{clip})\"><circle cx=\"{GLOBE_CENTER_X:.0}\" cy=\"{GLOBE_CENTER_Y:.0}\" r=\"{GLOBE_RADIUS:.0}\" fill=\"var(--moss-place-globe-water, #e9eff2)\"/>").expect("writing to String cannot fail");
        let tier = &context.pack().tiers[0];
        for layer in &tier.layers {
            let include_coast = self.locator_profile != Some(LocatorProfile::Country);
            if layer.id != 2 && !(include_coast && layer.id == 1) {
                continue;
            }
            for (index, feature) in layer.features.iter().enumerate() {
                if layer.id == 1 {
                    let mut paths = Vec::new();
                    for part in &feature.parts {
                        paths.extend(globe_line(part, center, context.pack().header.quantisation));
                    }
                    if let Some(path) = serialize_path(&paths, false, FINE) {
                        write!(self.output, "<path d=\"{path}\" fill=\"none\" stroke=\"var(--moss-place-globe-coast, #f5f6f4)\" stroke-width=\"0.5\" data-globe-feature=\"{index}\"/>").expect("writing to String cannot fail");
                    }
                } else {
                    let rings: Vec<Vec<(f64, f64)>> = feature
                        .parts
                        .iter()
                        .flat_map(|part| {
                            globe_rings(part, center, context.pack().header.quantisation)
                        })
                        .collect();
                    if let Some(path) = serialize_path(&rings, true, FINE) {
                        write!(self.output, "<path d=\"{path}\" fill=\"var(--moss-place-globe-land, #d7d5c9)\" fill-rule=\"evenodd\" data-globe-feature=\"{index}\"/>").expect("writing to String cannot fail");
                    }
                }
            }
        }
        for place in target.marker_places() {
            let Some(point) = place.point() else {
                continue;
            };
            let Some((x, y)) = globe_marker(point, center) else {
                continue;
            };
            write!(
                self.output,
                "<circle cx=\"{}\" cy=\"{}\" r=\"3\" fill=\"var(--moss-place-marker, #2d5a2d)\" data-map-globe-marker=\"true\" data-map-marker=\"{}\"/>",
                snap(x), snap(y), xml_escape(&place.key)
            )
            .expect("writing to String cannot fail");
        }
        write!(self.output, "</g><circle cx=\"{GLOBE_CENTER_X:.0}\" cy=\"{GLOBE_CENTER_Y:.0}\" r=\"{GLOBE_RADIUS:.0}\" fill=\"none\" stroke=\"var(--moss-place-globe-edge, #d7d5c9)\" stroke-width=\"1\" data-map-globe-inset=\"true\"/>").expect("writing to String cannot fail");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
                aggregate_member: false,
            }],
            frame: Some(frame),
            aggregate_name: None,
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
            locator_profile: None,
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
        assert!(filter("seafloor").contains("dx=\"1.0\"") && filter("seafloor").contains("flood-opacity=\"0.22\""));
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
        assert!(top.contains("--moss-place-land-high, #f6f1e4) 100%"), "{top}");
        let deepest = last_group("seafloor");
        assert!(deepest.contains("--moss-place-sea-deep, #bfd0dc) 100%"), "{deepest}");
    }

    /// The approved design's markers: an exact or city place is a 4 px dot
    /// on a 5 px casing; a region is a soft fade from 28% out to 1.1
    /// degrees of latitude (about 78 px at the 10-degree frame); the globe
    /// marks either with a 3 px dot.
    #[test]
    fn markers_match_the_approved_dot_fade_and_globe_sizes() {
        let exact = locator_for(35.5, 33.89, Precision::Exact);
        let marker = layer_body(&exact, "marker");
        assert!(marker.contains("r=\"5\" fill=\"var(--moss-place-marker-casing, #ffffff)\""), "{marker}");
        assert!(marker.contains("r=\"4\" fill=\"var(--moss-place-marker, #2d5a2d)\""), "{marker}");
        let region = locator_for(40.5, 36.0, Precision::Region);
        let marker = layer_body(&region, "marker");
        let radius: f64 = marker[marker.find(" r=\"").unwrap() + 4..].split('"').next().unwrap().parse().unwrap();
        assert!((70.0..=85.0).contains(&radius), "region fade radius {radius}");
        assert!(region.contains("stop-opacity=\"0.28\""));
        for svg in [&exact, &region] {
            assert!(svg.contains("r=\"3\" fill=\"var(--moss-place-marker, #2d5a2d)\" data-map-globe-marker=\"true\""));
        }
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
        assert!(output.contains("data-map-layer=\"marker\""));
        assert!(output.contains("r=\"4\""));
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
        let css = include_str!("../../assets/css/site.css");

        // Call-site literals: `var(--moss-place-NAME, #hex)`. The
        // name is cut at the first character outside `[a-z0-9-]` rather than
        // at the next comma, so this can't misfire on this very function's
        // own source text (this file's `include_str!` of itself) the way a
        // bare `find(',')` would.
        let mut tokens: Vec<String> = Vec::new();
        for source in [svg_source, locator_source, palette_source] {
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
        let expected = [1.6, 1.46, 1.18, 0.76, 0.62, 0.5, 0.5];
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
        for width in ["1.46", "0.76"] {
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

    #[test]
    fn aggregate_target_has_no_centroid_marker() {
        let mut target = populated_target(Precision::Region);
        target.aggregate_name = Some("Region".to_string());
        target.places[0].aggregate_member = true;
        let output = emit_svg(&PlaceMapContext::embedded().unwrap(), &target, "p", 0);
        let markers = output
            .split("data-map-layer=\"marker\"")
            .nth(1)
            .unwrap()
            .split("</g>")
            .next()
            .unwrap();
        assert!(!markers.contains("<circle"));
        assert!(!output.contains("data-map-globe-marker=\"true\""));
    }

    #[test]
    fn globe_inset_marks_only_real_target_points() {
        let context = PlaceMapContext::embedded().unwrap();
        let output = emit_svg(&context, &populated_target(Precision::City), "p", 0);
        assert!(output.contains("data-map-globe-marker=\"true\""));
        let mut aggregate = populated_target(Precision::City);
        aggregate.aggregate_name = Some("Kyoto area".to_string());
        aggregate.places[0].aggregate_member = true;
        let aggregate_output = emit_svg(&context, &aggregate, "p", 0);
        assert!(!aggregate_output.contains("data-map-globe-marker=\"true\""));
    }

    #[test]
    fn marker_radii_are_fixed_by_precision() {
        assert_eq!(marker_radius(Precision::Exact), 4.0);
        assert_eq!(marker_radius(Precision::City), 4.0);
        assert_eq!(marker_radius(Precision::Country), 20.0);
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
                        aggregate_member: false,
                    }],
                    frame: Some(frame),
                    aggregate_name: None,
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
                aggregate_member: false,
            }],
            frame: Some(frame),
            aggregate_name: None,
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

    #[test]
    fn the_world_map_keeps_trunk_rivers_and_no_reefs() {
        let polar = ProjectedPoint::new(0.0, 89.0).unwrap();
        let frame = Frame::from_points(&[polar], [Precision::Exact].into_iter()).unwrap();
        assert_eq!(frame.tier, FrameTier::World);
        let target = PlaceMapTarget { places: vec![], frame: Some(frame), aggregate_name: None };
        let svg = emit_svg(&PlaceMapContext::embedded().unwrap(), &target, "p", 0);
        let rivers = layer_body(&svg, "rivers");
        let widths: Vec<f64> = rivers
            .match_indices("stroke-width=\"")
            .map(|(index, _)| {
                let rest = &rivers[index + "stroke-width=\"".len()..];
                rest[..rest.find('"').unwrap()].parse().unwrap()
            })
            .collect();
        assert!(!widths.is_empty(), "the world map lost its trunk rivers");
        let floor = river::river_width(WORLD_MAX_RIVER_RANK);
        assert!(widths.iter().all(|&width| width >= floor - 1e-9), "{widths:?}");
        assert!(!layer_body(&svg, "reefs").contains("<path"));
    }

    #[test]
    fn xml_escape_replaces_xml_forbidden_controls() {
        let escaped = xml_escape("ok\u{0}\u{8}\u{b}\u{c}\u{1f}");
        assert_eq!(escaped, "ok�����");
    }
}
