//! Deterministic, self-contained SVG emission for a place map.
//!
//! The emitter deliberately knows nothing about pages, configuration, or CSS
//! files.  Its only inputs are the immutable decoded pack and a resolved map
//! target.  That makes the byte output suitable for page snapshots and keeps
//! the geometry/privacy boundary in [`super::context`] and [`super::geometry`].

use std::fmt::Write;

use sha2::{Digest, Sha256};

mod path;
use path::{serialize_path, snap, BAND_PX, FINE_PX};
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
use palette::{band_color, relief_height_grey};

use super::geometry::{marker_radius, FrameTier, ProjectedPoint, Projection, TileSelection};
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
            "<defs><filter id=\"{height_filter}\" color-interpolation-filters=\"sRGB\"><feColorMatrix in=\"SourceGraphic\" type=\"luminanceToAlpha\" result=\"height-alpha\"/><feGaussianBlur in=\"height-alpha\" stdDeviation=\"9\" result=\"height-blur-9\"/><feColorMatrix in=\"height-blur-9\" type=\"matrix\" values=\"0 0 0 1 0  0 0 0 1 0  0 0 0 1 0  0 0 0 1 0\" result=\"height-coverage\"/><feDiffuseLighting in=\"height-blur-9\" surfaceScale=\"78\" diffuseConstant=\"1\" lighting-color=\"#ffffff\" result=\"lit-steep\"><feDistantLight azimuth=\"240\" elevation=\"45\"/></feDiffuseLighting><feColorMatrix in=\"lit-steep\" type=\"matrix\" values=\"0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  1 0 0 0 0\" result=\"lit-steep-alpha\"/><feGaussianBlur in=\"height-alpha\" stdDeviation=\"4\" result=\"height-blur-4\"/><feDiffuseLighting in=\"height-blur-4\" surfaceScale=\"34\" diffuseConstant=\"1\" lighting-color=\"#ffffff\" result=\"lit-soft\"><feDistantLight azimuth=\"240\" elevation=\"45\"/></feDiffuseLighting><feColorMatrix in=\"lit-soft\" type=\"matrix\" values=\"0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  1 0 0 0 0\" result=\"lit-soft-alpha\"/><feComposite in=\"lit-steep-alpha\" in2=\"lit-soft-alpha\" operator=\"arithmetic\" k1=\"0\" k2=\"0.7\" k3=\"0.3\" k4=\"0\" result=\"lit-mix\"/><feComponentTransfer in=\"lit-mix\" result=\"lit-hi-mask\"><feFuncA type=\"linear\" slope=\"1.877817459305202\" intercept=\"-1.327817459305202\"/></feComponentTransfer><feComponentTransfer in=\"lit-mix\" result=\"lit-lo-mask\"><feFuncA type=\"linear\" slope=\"-0.5656854249492381\" intercept=\"0.4\"/></feComponentTransfer><feFlood flood-color=\"var(--moss-place-light-warm, #fff3d0)\" result=\"lit-hi-flood\"/><feComposite in=\"lit-hi-flood\" in2=\"lit-hi-mask\" operator=\"in\" result=\"lit-hi-tint\"/><feFlood flood-color=\"var(--moss-place-light-cool, #5a6488)\" result=\"lit-lo-flood\"/><feComposite in=\"lit-lo-flood\" in2=\"lit-lo-mask\" operator=\"in\" result=\"lit-lo-tint\"/><feMerge result=\"lit-combined\"><feMergeNode in=\"lit-lo-tint\"/><feMergeNode in=\"lit-hi-tint\"/></feMerge><feGaussianBlur in=\"lit-combined\" stdDeviation=\"2\" result=\"lit-smooth\"/><feComposite in=\"lit-smooth\" in2=\"height-coverage\" operator=\"arithmetic\" k1=\"1\" k2=\"0\" k3=\"0\" k4=\"0\"/></filter><filter id=\"{}\" color-interpolation-filters=\"sRGB\"><feDropShadow dx=\"1.4\" dy=\"1.4\" stdDeviation=\"0.7\" flood-color=\"var(--moss-place-shadow, #5a6488)\" flood-opacity=\"0.35\" result=\"land-shadow\"/><feMerge><feMergeNode in=\"SourceGraphic\"/><feMergeNode in=\"land-shadow\"/></feMerge></filter><filter id=\"{}\" color-interpolation-filters=\"sRGB\"><feDropShadow dx=\"1.0\" dy=\"1.0\" stdDeviation=\"0.6\" flood-color=\"var(--moss-place-shadow, #5a6488)\" flood-opacity=\"0.22\" result=\"sea-shadow\"/><feMerge><feMergeNode in=\"SourceGraphic\"/><feMergeNode in=\"sea-shadow\"/></feMerge></filter><radialGradient id=\"{marker_gradient}\"><stop offset=\"0\" stop-color=\"var(--moss-place-marker, #2d5a2d)\"/><stop offset=\"1\" stop-color=\"var(--moss-place-marker, #2d5a2d)\" stop-opacity=\"0\"/></radialGradient><clipPath id=\"{globe_clip}\"><circle cx=\"{GLOBE_CENTER_X:.0}\" cy=\"{GLOBE_CENTER_Y:.0}\" r=\"{GLOBE_RADIUS:.0}\"/></clipPath><path id=\"{height_empty}\" d=\"m0 0l0 0\"/><filter id=\"{}\" filterUnits=\"userSpaceOnUse\" x=\"-24\" y=\"-24\" width=\"768\" height=\"528\"><feGaussianBlur stdDeviation=\"2.4\"/></filter></defs>", self.ids.get("shadow-seafloor"), self.ids.get("shadow-land"), self.ids.get("soft"),
        )
        .expect("writing to String cannot fail");
        self.output.push_str("<defs>");
        for (name, bands) in [
            (
                "relief",
                [
                    100, 200, 400, 700, 1000, 1500, 2000, 2500, 3000, 4000, 5000, 6000,
                ]
                .as_slice(),
            ),
            (
                "seafloor",
                [
                    -10, -20, -30, -50, -100, -200, -1000, -2000, -3000, -4000, -5000, -6000,
                ]
                .as_slice(),
            ),
        ] {
            // Same small cut-paper offset the shared shadow-land/shadow-seafloor
            // filters above use, not the earlier dx=3/dy=4/stdDeviation=0: at
            // that larger, perfectly hard-edged offset a small island (a
            // handful of points wide) reads as a doubled "ghost" shape rather
            // than a subtle drop shadow.
            let (dx, dy, std, opacity) = if name == "relief" {
                ("1.4", "1.4", "0.7", "0.35")
            } else {
                ("1.0", "1.0", "0.6", "0.22")
            };
            for band in bands {
                write!(self.output, "<filter id=\"{}\" color-interpolation-filters=\"sRGB\"><feDropShadow dx=\"{dx}\" dy=\"{dy}\" stdDeviation=\"{std}\" flood-color=\"var(--moss-place-shadow-{name}-{band}, #5a6488)\" flood-opacity=\"{opacity}\" result=\"shadow\"/><feMerge><feMergeNode in=\"SourceGraphic\"/><feMergeNode in=\"shadow\"/></feMerge></filter>", self.ids.get(&format!("shadow-{name}-{band}"))).expect("writing to String cannot fail");
            }
        }
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
            if let Some(path) = serialize_path(&projection.project_feature(&rings, quantisation), true, FINE_PX) {
                let id = format!("{}-land-{index}", self.ids.base);
                write!(self.output, "<path id=\"{id}\" d=\"{path}\" fill-rule=\"evenodd\"/>")
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
        write!(self.output, "<g id=\"{}\" data-map-layer=\"land\" filter=\"url(#{})\">", self.ids.get("layer-land"), self.ids.get("shadow-land"))
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
    /// Filled layers keep even-odd holes; line layers (reefs) stay open.
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
        if let Some(path) = serialize_path(&paths, filled, FINE_PX) {
            let rule = if filled { " fill-rule=\"evenodd\"" } else { "" };
            write!(self.output, "<path d=\"{path}\" {paint}{rule}/>").expect("writing to String cannot fail");
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
        write!(
            self.output,
            "<g id=\"{}\" data-map-layer=\"{name}\">",
            self.ids.get(&format!("layer-{name}"))
        )
        .expect("writing to String cannot fail");
        let mut active_band = None;
        for (index, feature) in features.into_iter().enumerate() {
            if active_band != Some(feature.band) {
                if active_band.is_some() {
                    self.output.push_str("</g>");
                }
                active_band = Some(feature.band);
                write!(
                    self.output,
                    "<g data-map-band=\"{}\" data-map-fill=\"{}\" filter=\"url(#{})\">",
                    feature.band,
                    band_color(name, feature.band),
                    self.ids.get(&format!("shadow-{name}-{}", feature.band))
                )
                .expect("writing to String cannot fail");
            }
            self.emit_filled_feature(
                projection,
                feature,
                quantisation,
                &band_color(name, feature.band),
                &format!("{}-{name}-{index}", self.ids.base),
                name == "relief",
            );
        }
        if active_band.is_some() {
            self.output.push_str("</g>");
        }
        self.output.push_str("</g>");
    }

    fn emit_filled_feature(
        &mut self,
        projection: &Projection,
        feature: &Feature,
        quantisation: u32,
        color: &str,
        id: &str,
        is_relief: bool,
    ) {
        let rings: Vec<&[(i32, i32)]> = feature.parts.iter().map(Vec::as_slice).collect();
        // One project_feature call per source feature is intentional: all
        // rings stay in one even-odd path, preserving holes and multipart
        // topology through clipping.
        let projected = projection.project_feature(&rings, quantisation);
        let Some(path) = serialize_path(&projected, true, BAND_PX) else {
            return;
        };
        write!(
            self.output,
            "<path id=\"{id}\" d=\"{path}\" fill=\"{color}\" fill-rule=\"evenodd\" data-map-band=\"{}\"/>"
            , feature.band
        )
        .expect("writing to String cannot fail");
        if is_relief {
            self.height_uses
                .push(format!("{id}|{}", relief_height_grey(feature.band)));
        }
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
        // 0.5, not the two-pass diffuse blend weight (0.3, further down)
        // this was copied from by mistake: the height field's own opacity
        // is fixed at 0.5, and this opacity is the alpha the filter's
        // luminanceToAlpha step reads as height, so 0.3 understated every
        // highlight and shadow tint in both themes.
        write!(self.output, "<g id=\"{id}\" data-map-layer=\"lighting\" filter=\"url(#{filter})\" clip-path=\"url(#{land_clip})\" aria-hidden=\"true\"><g id=\"{}\" opacity=\"0.5\">", self.ids.get("height-field")).expect("writing to String cannot fail");
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
                let radius = marker_radius(place.precision);
                let fill = if matches!(place.precision, Precision::Region | Precision::Country) {
                    format!("url(#{fade})")
                } else {
                    "var(--moss-place-marker, #2d5a2d)".to_string()
                };
                write!(self.output, "<circle cx=\"{}\" cy=\"{}\" r=\"{radius:.0}\" fill=\"{fill}\" data-map-marker=\"{}\"/>", snap(point.0), snap(point.1), xml_escape(&place.key)).expect("writing to String cannot fail");
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
                    if let Some(path) = serialize_path(&paths, false, FINE_PX) {
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
                    if let Some(path) = serialize_path(&rings, true, FINE_PX) {
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
            let radius = (marker_radius(place.precision) * 0.4).max(2.0);
            write!(
                self.output,
                "<circle cx=\"{}\" cy=\"{}\" r=\"{radius:.0}\" fill=\"var(--moss-place-marker, #2d5a2d)\" data-map-globe-marker=\"true\" data-map-marker=\"{}\"/>",
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

    /// A square whose edges bow 2 px outward at their midpoints: detail land
    /// keeps at its 1 px tolerance and a band, at 2 px, starts dropping.
    /// (Which bows survive depends on where Douglas-Peucker splits a closed
    /// ring, so the band's count is only bounded, not pinned.)
    #[test]
    fn bands_simplify_at_two_px_and_land_at_one() {
        let origin = ProjectedPoint::new(0.0, 0.0).unwrap();
        let frame = Frame::from_points(&[origin], [Precision::Exact].into_iter()).unwrap();
        let projection = Projection::new(&frame);
        // About 70.6 px per degree in this frame: the 0.034 degree bow
        // lands 2 px off each 141 px edge once snapped to whole pixels.
        let (edge, bow) = (10_000, 10_340);
        let ring = vec![
            (-edge, -edge), (0, -bow), (edge, -edge), (bow, 0),
            (edge, edge), (0, bow), (-edge, edge), (-bow, 0), (-edge, -edge),
        ];
        let feature = Feature { bounds: [-bow, -bow, bow, bow], band: 100, parts: vec![ring] };
        let mut grouped: Vec<Vec<&Feature>> = (0..=10).map(|_| Vec::new()).collect();
        grouped[2].push(&feature);
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
        let vertices = |svg: &str| {
            let d = &svg[svg.find(" d=\"").unwrap() + 4..];
            d[..d.find('"').unwrap()].matches(['m', 'l']).count()
        };
        writer.land_defs(10_000, &projection, &grouped);
        let land = vertices(&writer.output);
        writer.output.clear();
        writer.emit_band_layer(10_000, &projection, &grouped, 9, "relief", false);
        let band = vertices(&writer.output);
        assert_eq!(land, 8, "land at 1 px keeps every bow");
        assert!(band < land, "a band at 2 px must drop bows land keeps: band {band}, land {land}");
    }

    #[test]
    fn path_serializer_closes_fills_but_not_lines() {
        let fill = serialize_path(&[vec![(1.2, 2.8), (4.1, 2.8), (4.1, 5.0)]], true, FINE_PX).unwrap();
        let line = serialize_path(&[vec![(1.2, 2.8), (4.1, 2.8)]], false, FINE_PX).unwrap();
        assert!(fill.ends_with('z'));
        assert!(!line.contains('z'));
        assert!(fill
            .chars()
            .all(|character| !character.is_ascii_digit() || character.is_ascii()));
    }

    #[test]
    fn populated_pack_emits_layers_bands_even_odd_paths_and_a_marker() {
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
        assert!(output.contains("r=\"9\""));
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
        let relief_colors: Vec<_> = [
            100, 200, 400, 700, 1000, 1500, 2000, 2500, 3000, 4000, 5000, 6000,
        ]
        .into_iter()
        .map(|band| band_color("relief", band))
        .collect();
        assert_eq!(
            relief_colors
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            relief_colors.len()
        );
        let seafloor_colors: Vec<_> = [
            -10, -20, -30, -50, -100, -200, -1000, -2000, -3000, -4000, -5000, -6000,
        ]
        .into_iter()
        .map(|band| band_color("seafloor", band))
        .collect();
        assert_eq!(
            seafloor_colors
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            seafloor_colors.len()
        );
        for band in [
            100, 200, 400, 700, 1000, 1500, 2000, 2500, 3000, 4000, 5000, 6000,
        ] {
            assert!(output.contains(&format!("shadow-relief-{band}\"")));
        }
        for band in [-10, -20, -30, -50, -100, -200, -1000, -2000, -3000, -4000, -5000, -6000] {
            assert!(output.contains(&format!("shadow-seafloor-{band}\"")));
        }
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
    /// or as one of `band_color`'s relief/seafloor tokens) must be defined
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
        let css = include_str!("../../assets/css/site.css");

        // Call-site literals: `var(--moss-place-NAME, #hex)`. band_color's
        // own dynamically-formatted token (`--moss-place-relief-{band}`) has
        // no such literal in source, so this pass can't accidentally pick up
        // its dead `_ => ("--moss-place-relief".to_string(), ...)` arm. The
        // name is cut at the first character outside `[a-z0-9-]` rather than
        // at the next comma, so this can't misfire on this very function's
        // own source text (this file's `include_str!` of itself) the way a
        // bare `find(',')` would.
        let mut tokens: Vec<String> = Vec::new();
        for source in [svg_source, locator_source] {
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

        // The two dynamically-named families, generated the same way the
        // emitter generates them.
        for band in [
            100, 200, 400, 700, 1000, 1500, 2000, 2500, 3000, 4000, 5000, 6000,
        ] {
            tokens.push(extract_token_name(&band_color("relief", band)));
        }
        for band in [-10, -20, -30, -50, -100, -200, -1000, -2000, -3000, -4000, -5000, -6000] {
            tokens.push(extract_token_name(&band_color("seafloor", band)));
        }

        // Also every band the checked-in pack actually contains, not just
        // the fixed lists above: those two lists and band_color's own match
        // arms are three hand-kept copies of the same set, and a pack
        // regeneration can add a new band value (as it did 2026-09-28, three
        // shallow shelf steps and three deep abyssal steps) without any of
        // the three being told. When band_color doesn't recognise a band it
        // falls through to an untranslated catch-all token, which still
        // renders (site.css defines that catch-all token) but silently
        // ignores the real depth and, for any band between two catch-alls,
        // can point at the wrong one entirely -- a real render, not a build
        // error. Scanning the pack directly is what would have caught it.
        let pack = crate::build::place_map::embedded().expect("checked-in pack must decode");
        for (layer_id, name) in [(9u8, "relief"), (10u8, "seafloor")] {
            let bands: std::collections::BTreeSet<i16> = pack
                .tiers
                .iter()
                .flat_map(|tier| tier.layers.iter())
                .filter(|layer| layer.id == layer_id)
                .flat_map(|layer| layer.features.iter().map(|feature| feature.band))
                .collect();
            assert!(!bands.is_empty(), "pack layer {layer_id} has no bands");
            for band in bands {
                tokens.push(extract_token_name(&band_color(name, band)));
            }
        }

        tokens.sort();
        tokens.dedup();
        assert!(tokens.len() > 30, "expected a rich token set, got {tokens:?}");

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

    /// Pulls the property name back out of a `band_color`-shaped
    /// `var(--NAME, #hex)` string — deliberately not spelled with a literal
    /// `var(--moss-place-` prefix in this comment, so the scan above (which
    /// reads this very file's source) can't mistake the example for a call
    /// site.
    fn extract_token_name(var_call: &str) -> String {
        let name = var_call.trim_start_matches("var(--");
        name[..name.find(',').unwrap()].to_string()
    }

    /// The fallback hex color out of a `var(--token, #hex)` call-site
    /// string, as opposed to [`extract_token_name`]'s token half.
    fn extract_fallback(var_call: &str) -> String {
        let after_comma = &var_call[var_call.find(',').unwrap() + 1..];
        after_comma.trim_end_matches(')').trim().to_string()
    }

    /// Regression for a 2026-09-28 defect: band_color's seafloor match only
    /// recognised the pack's original eight depths, so the four bands a
    /// pack regeneration added (-20/-30/-50/-200/-3000/-5000, alongside the
    /// unchanged -10/-100/-1000/-2000/-4000/-6000) all fell through to the
    /// deepest band's catch-all fallback and were visually indistinguishable
    /// from -6000. That is invisible to a test asserting the whole
    /// `var(--token, #fallback)` string is unique per band, because the
    /// token half (built from the raw, unmatched band value) still differed
    /// even though the fallback color repeated -- the token and fallback
    /// were two independently-wrong computations that happened to disagree
    /// with each other, not just with the truth. Checking the fallback in
    /// isolation is what catches it.
    #[test]
    fn seafloor_bands_use_distinct_fallbacks_not_a_shared_catch_all() {
        let fallbacks: Vec<String> = [
            -10, -20, -30, -50, -100, -200, -1000, -2000, -3000, -4000, -5000, -6000,
        ]
        .into_iter()
        .map(|band| extract_fallback(&band_color("seafloor", band)))
        .collect();
        assert_eq!(
            fallbacks
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            fallbacks.len(),
            "expected 12 distinct seafloor fallbacks, got {fallbacks:?}"
        );
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
        assert_eq!(marker_radius(Precision::Exact), 6.0);
        assert_eq!(marker_radius(Precision::City), 9.0);
        assert_eq!(marker_radius(Precision::Region), 14.0);
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
