//! Deterministic, self-contained SVG emission for a place map.
//!
//! The emitter deliberately knows nothing about pages, configuration, or CSS
//! files.  Its only inputs are the immutable decoded pack and a resolved map
//! target.  That makes the byte output suitable for page snapshots and keeps
//! the geometry/privacy boundary in [`super::context`] and [`super::geometry`].

use std::fmt::Write;

use sha2::{Digest, Sha256};

mod path;
use path::{serialize_path, snap};
mod text;
use text::{precision_name, precision_rank, xml_escape};

use super::geometry::{marker_radius, Frame, FrameTier, ProjectedPoint, Projection, TileSelection};
use super::globe::{globe_line, globe_marker, globe_rings};
use super::{Feature, Pack, PlaceMapContext, PlaceMapTarget, ResolvedPlace};
use crate::vault::places::Precision;

pub const SVG_WIDTH: u32 = 720;
pub const SVG_HEIGHT: u32 = 480;
pub const LOCATOR_BROTLI_LIMIT: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocatorProfile {
    ExactCity,
    Region,
    Country,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocatorSvg {
    pub svg: String,
    pub profile: LocatorProfile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocatorBudgetError {
    pub brotli_bytes: usize,
}

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

/// Emit a locator-sized map and enforce its Brotli quality-11 budget. A wide
/// map may use the explicitly reported simplified globe/local contract; the
/// ordinary full emitter remains available for larger body maps.
pub fn emit_locator_svg(
    context: &PlaceMapContext,
    target: &PlaceMapTarget,
    options: SvgMapOptions<'_>,
) -> Result<LocatorSvg, LocatorBudgetError> {
    let profile = match options.precision {
        Precision::Exact | Precision::City => LocatorProfile::ExactCity,
        Precision::Region => LocatorProfile::Region,
        Precision::Country => LocatorProfile::Country,
    };
    // Locator output is deliberately the bounded, sparse profile. The full
    // emitter remains available for body maps, whose geometry is unrestricted.
    let mut svg = emit_svg_with_mode(context, target, options, true);
    let profile_name = match profile {
        LocatorProfile::ExactCity => "exact-city",
        LocatorProfile::Region => "region",
        LocatorProfile::Country => "country",
    };
    svg = svg.replacen(
        "<figure ",
        &format!("<figure data-map-locator-profile=\"{profile_name}\" "),
        1,
    );
    if svg.len() > 256 * 1024 {
        return Err(LocatorBudgetError {
            brotli_bytes: svg.len(),
        });
    }
    Ok(LocatorSvg { svg, profile })
}

pub use emit_locator_svg as emit_locator;

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
        compact,
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
        grouped_features(context.pack(), &selected)
    });
    if !compact {
        if let (Some(projection), Some(grouped)) = (projection.as_ref(), grouped.as_ref()) {
            writer.emit_layers(context.pack().header.quantisation, projection, grouped);
        } else {
            // A missing coordinate is deliberate no-map input. Keeping a valid
            // empty figure here avoids inventing a point or leaking source data.
            writer.empty_layers();
        }
    } else {
        writer.empty_layers();
    }
    writer.lighting();
    if target.frame.is_none() {
        writer.empty_layers_after_lighting();
    } else if !compact {
        if let Some(projection) = projection.as_ref() {
            writer.emit_surface_layers(
                context.pack().header.quantisation,
                projection,
                grouped.as_ref().unwrap(),
            );
        }
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
    compact: bool,
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
            "<defs><filter id=\"{height_filter}\" color-interpolation-filters=\"sRGB\"><feGaussianBlur in=\"SourceGraphic\" stdDeviation=\"9\" result=\"height-blur-9\"/><feGaussianBlur in=\"SourceGraphic\" stdDeviation=\"4\" opacity=\"0.3\" result=\"height-blur-4\"/><feBlend in=\"height-blur-9\" in2=\"height-blur-4\" mode=\"screen\" result=\"height-field\"/><feColorMatrix in=\"height-field\" type=\"luminanceToAlpha\" result=\"height-alpha\"/><feDiffuseLighting in=\"height-alpha\" surfaceScale=\"1\" diffuseConstant=\"1\" lighting-color=\"var(--moss-place-light-warm, #f2c078)\" result=\"warm\"><feDistantLight azimuth=\"240\" elevation=\"45\"/></feDiffuseLighting><feDiffuseLighting in=\"height-alpha\" surfaceScale=\"0.7\" diffuseConstant=\"0.7\" lighting-color=\"var(--moss-place-light-cool, #5c83aa)\" result=\"cool\"><feDistantLight azimuth=\"240\" elevation=\"45\"/></feDiffuseLighting><feBlend in=\"warm\" in2=\"cool\" mode=\"screen\" result=\"warm-cool\"/><feGaussianBlur in=\"warm-cool\" stdDeviation=\"2\" result=\"soft-light\"/><feBlend in=\"soft-light\" in2=\"SourceGraphic\" mode=\"multiply\" result=\"lit-terrain\"/><feComposite in=\"lit-terrain\" in2=\"SourceGraphic\" operator=\"in\"/></filter><filter id=\"{}\" color-interpolation-filters=\"sRGB\"><feDropShadow dx=\"3\" dy=\"4\" stdDeviation=\"0\" flood-color=\"var(--moss-place-shadow, #26343d)\" flood-opacity=\"0.22\" result=\"land-shadow\"/><feMerge><feMergeNode in=\"SourceGraphic\"/><feMergeNode in=\"land-shadow\"/></feMerge></filter><filter id=\"{}\" color-interpolation-filters=\"sRGB\"><feDropShadow dx=\"3\" dy=\"4\" stdDeviation=\"0\" flood-color=\"var(--moss-place-shadow, #26343d)\" flood-opacity=\"0.16\" result=\"sea-shadow\"/><feMerge><feMergeNode in=\"SourceGraphic\"/><feMergeNode in=\"sea-shadow\"/></feMerge></filter><radialGradient id=\"{marker_gradient}\"><stop offset=\"0\" stop-color=\"var(--moss-place-marker, #c45b48)\"/><stop offset=\"1\" stop-color=\"var(--moss-place-marker, #c45b48)\" stop-opacity=\"0\"/></radialGradient><clipPath id=\"{globe_clip}\"><circle cx=\"{GLOBE_CENTER_X:.0}\" cy=\"{GLOBE_CENTER_Y:.0}\" r=\"{GLOBE_RADIUS:.0}\"/></clipPath><path id=\"{height_empty}\" d=\"m0 0l0 0\"/></defs>", self.ids.get("shadow-seafloor"), self.ids.get("shadow-land"),
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
                [-10, -100, -250, -500, -1000, -2000, -4000, -6000].as_slice(),
            ),
        ] {
            for band in bands {
                write!(self.output, "<filter id=\"{}\" color-interpolation-filters=\"sRGB\"><feDropShadow dx=\"3\" dy=\"4\" stdDeviation=\"0\" flood-color=\"var(--moss-place-shadow-{name}-{band}, #26343d)\" flood-opacity=\"0.2\" result=\"shadow\"/><feMerge><feMergeNode in=\"SourceGraphic\"/><feMergeNode in=\"shadow\"/></feMerge></filter>", self.ids.get(&format!("shadow-{name}-{band}"))).expect("writing to String cannot fail");
            }
        }
        self.output.push_str("</defs>");
    }

    fn water(&mut self) {
        let id = self.ids.get("layer-water");
        write!(
            self.output,
            "<g id=\"{id}\" data-map-layer=\"water\"><rect width=\"{SVG_WIDTH}\" height=\"{SVG_HEIGHT}\" fill=\"var(--moss-place-water, #9cc7d8)\"/></g>",
        )
        .expect("writing to String cannot fail");
    }

    fn empty_layers(&mut self) {
        for name in ["seafloor", "land", "relief"] {
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
        for name in [
            "coast", "lakes", "rivers", "ice", "reefs", "salt", "built-up",
        ] {
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
        // Sea floor is painted shallow to deep, then land and relief low to
        // high.  The source pack has stable IDs; no administrative layer is
        // accepted or emitted here.
        self.emit_band_layer(quantisation, projection, grouped, 10, "seafloor", true);
        self.emit_filled_layer(
            quantisation,
            projection,
            grouped,
            2,
            "land",
            "var(--moss-place-land, #d6c89c)",
        );
        self.emit_band_layer(quantisation, projection, grouped, 9, "relief", false);
    }

    fn emit_surface_layers(
        &mut self,
        quantisation: u32,
        projection: &Projection,
        grouped: &[Vec<&Feature>],
    ) {
        self.emit_line_layer(
            quantisation,
            projection,
            grouped,
            1,
            "coast",
            "var(--moss-place-coast, #7d684b)",
        );
        self.emit_filled_layer(
            quantisation,
            projection,
            grouped,
            3,
            "lakes",
            "var(--moss-place-lakes, #80b9c8)",
        );
        self.emit_line_layer(
            quantisation,
            projection,
            grouped,
            4,
            "rivers",
            "var(--moss-place-rivers, #5793ae)",
        );
        self.emit_filled_layer(
            quantisation,
            projection,
            grouped,
            5,
            "ice",
            "var(--moss-place-ice, #eaf2ed)",
        );
        self.emit_filled_layer(
            quantisation,
            projection,
            grouped,
            6,
            "reefs",
            "var(--moss-place-reefs, #d4a77e)",
        );
        self.emit_filled_layer(
            quantisation,
            projection,
            grouped,
            7,
            "salt",
            "var(--moss-place-salt, #e5d9b4)",
        );
        self.emit_filled_layer(
            quantisation,
            projection,
            grouped,
            8,
            "built-up",
            "var(--moss-place-built-up, #aa8065)",
        );
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

    fn emit_filled_layer(
        &mut self,
        quantisation: u32,
        projection: &Projection,
        grouped: &[Vec<&Feature>],
        layer_id: u8,
        name: &str,
        color: &str,
    ) {
        let features = grouped[usize::from(layer_id)].clone();
        self.output.push_str(&format!(
            "<g id=\"{}\" data-map-layer=\"{name}\"{}>",
            self.ids.get(&format!("layer-{name}")),
            if name == "land" {
                format!(" filter=\"url(#{})\"", self.ids.get("shadow-land"))
            } else {
                String::new()
            }
        ));
        for (index, feature) in features.into_iter().enumerate() {
            self.emit_filled_feature(
                projection,
                feature,
                quantisation,
                color,
                &format!("{}-{name}-{index}", self.ids.base),
                false,
            );
        }
        self.output.push_str("</g>");
    }

    fn emit_line_layer(
        &mut self,
        quantisation: u32,
        projection: &Projection,
        grouped: &[Vec<&Feature>],
        layer_id: u8,
        name: &str,
        color: &str,
    ) {
        let features = grouped[usize::from(layer_id)].clone();
        self.output.push_str(&format!(
            "<g id=\"{}\" data-map-layer=\"{name}\">",
            self.ids.get(&format!("layer-{name}"))
        ));
        for (index, feature) in features.into_iter().enumerate() {
            let mut paths = Vec::new();
            for part in &feature.parts {
                paths.extend(projection.project_part(part, quantisation));
            }
            if let Some(path) = serialize_path(&paths, false) {
                write!(self.output, "<path id=\"{}\" d=\"{path}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"1\" stroke-linecap=\"round\"/>", format!("{}-{name}-{index}", self.ids.base)).expect("writing to String cannot fail");
            }
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
        let Some(path) = serialize_path(&projected, true) else {
            return;
        };
        write!(
            self.output,
            "<path id=\"{id}\" d=\"{path}\" fill=\"{color}\" fill-rule=\"evenodd\" data-map-band=\"{}\"/>"
            , feature.band
        )
        .expect("writing to String cannot fail");
        if is_relief {
            let opacity = (f64::from(feature.band).abs() / 6000.0).clamp(0.2, 1.0);
            self.height_uses.push(format!("{id}|{opacity:.3}"));
        }
        if id.contains("-land-") {
            self.land_paths.push(id.to_string());
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
        write!(self.output, "<g id=\"{id}\" data-map-layer=\"lighting\" filter=\"url(#{filter})\" clip-path=\"url(#{land_clip})\" aria-hidden=\"true\"><g id=\"{}\" opacity=\"0.3\">", self.ids.get("height-field")).expect("writing to String cannot fail");
        for item in &self.height_uses {
            let (path_id, opacity) = item.split_once('|').unwrap_or((item.as_str(), "1"));
            write!(self.output, "<use href=\"#{path_id}\" height=\"100%\" fill=\"rgb(128 128 128)\" fill-opacity=\"1\" opacity=\"{opacity}\" data-map-role=\"height-field\"/>").expect("writing to String cannot fail");
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
                    "var(--moss-place-marker, #c45b48)".to_string()
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
        write!(self.output, "<g id=\"{id}\" data-map-layer=\"globe\" clip-path=\"url(#{clip})\"><circle cx=\"{GLOBE_CENTER_X:.0}\" cy=\"{GLOBE_CENTER_Y:.0}\" r=\"{GLOBE_RADIUS:.0}\" fill=\"var(--moss-place-globe-water, #83afc1)\"/>").expect("writing to String cannot fail");
        let tier = &context.pack().tiers[0];
        for layer in &tier.layers {
            if self.compact {
                break;
            }
            if layer.id != 1 && layer.id != 2 {
                continue;
            }
            for (index, feature) in layer.features.iter().enumerate() {
                if layer.id == 1 {
                    let mut paths = Vec::new();
                    for part in &feature.parts {
                        paths.extend(globe_line(part, center, context.pack().header.quantisation));
                    }
                    if let Some(path) = serialize_path(&paths, false) {
                        write!(self.output, "<path d=\"{path}\" fill=\"none\" stroke=\"var(--moss-place-globe-coast, #80684d)\" stroke-width=\"0.5\" data-globe-feature=\"{index}\"/>").expect("writing to String cannot fail");
                    }
                } else {
                    let rings: Vec<Vec<(f64, f64)>> = feature
                        .parts
                        .iter()
                        .flat_map(|part| {
                            globe_rings(part, center, context.pack().header.quantisation)
                        })
                        .collect();
                    if let Some(path) = serialize_path(&rings, true) {
                        write!(self.output, "<path d=\"{path}\" fill=\"var(--moss-place-globe-land, #d6c89c)\" fill-rule=\"evenodd\" data-globe-feature=\"{index}\"/>").expect("writing to String cannot fail");
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
                "<circle cx=\"{}\" cy=\"{}\" r=\"{radius:.0}\" fill=\"var(--moss-place-marker, #c45b48)\" data-map-globe-marker=\"true\" data-map-marker=\"{}\"/>",
                snap(x), snap(y), xml_escape(&place.key)
            )
            .expect("writing to String cannot fail");
        }
        write!(self.output, "</g><circle cx=\"{GLOBE_CENTER_X:.0}\" cy=\"{GLOBE_CENTER_Y:.0}\" r=\"{GLOBE_RADIUS:.0}\" fill=\"none\" stroke=\"var(--moss-place-globe-edge, #63737b)\" stroke-width=\"1\" data-map-globe-inset=\"true\"/>").expect("writing to String cannot fail");
    }
}

fn band_color(name: &str, band: i16) -> String {
    let (token, fallback) = match name {
        "relief" => {
            let fallback = match band {
                100 => "#cfc092",
                200 => "#c8b88a",
                400 => "#c0ad82",
                700 => "#b9a47a",
                1000 => "#b19b72",
                1500 => "#aa926a",
                2000 => "#a18a62",
                2500 => "#99805a",
                3000 => "#907852",
                4000 => "#876e4a",
                5000 => "#7d6544",
                _ => "#735c3d",
            };
            (format!("--moss-place-relief-{band}"), fallback)
        }
        "seafloor" => {
            let fallback = match band {
                -10 => "#8ab8c5",
                -100 => "#82b0c0",
                -250 => "#79a8bb",
                -500 => "#719fb5",
                -1000 => "#6896ae",
                -2000 => "#608da7",
                -4000 => "#57849f",
                _ => "#4e7a96",
            };
            (format!("--moss-place-seafloor-{}", band.abs()), fallback)
        }
        _ => ("--moss-place-relief".to_string(), "#b8a878"),
    };
    format!("var({token}, {fallback})")
}

#[cfg(test)]
mod tests {
    use super::*;
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
            "data-map-layer=\"seafloor\"",
            "data-map-layer=\"land\"",
            "data-map-layer=\"relief\"",
            "data-map-layer=\"lighting\"",
            "data-map-layer=\"coast\"",
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

    #[test]
    fn path_serializer_closes_fills_but_not_lines() {
        let fill = serialize_path(&[vec![(1.2, 2.8), (4.1, 2.8), (4.1, 5.0)]], true).unwrap();
        let line = serialize_path(&[vec![(1.2, 2.8), (4.1, 2.8)]], false).unwrap();
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
        assert!(output.contains("data-map-layer=\"coast\"><path"));
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
        assert!(output.contains("dx=\"3\" dy=\"4\" stdDeviation=\"0\""));
        assert!(output.contains("stdDeviation=\"2\""));
        assert!(output.contains("result=\"warm\""));
        assert!(output.contains("result=\"cool\""));
        assert!(output.contains("clip-path=\"url(#"));
        assert!(output.contains("fill=\"rgb(128 128 128)\""));
        assert!(output.contains("data-map-layer=\"relief\""));
        let lighting = output.split("data-map-layer=\"lighting\"").nth(1).unwrap();
        assert!(!lighting.contains("layer-land-"));
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
        let seafloor_colors: Vec<_> = [-10, -100, -250, -500, -1000, -2000, -4000, -6000]
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
        for band in [-10, -100, -250, -500, -1000, -2000, -4000, -6000] {
            assert!(output.contains(&format!("shadow-seafloor-{band}\"")));
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
            compressed.len() <= 16 * 1024,
            "raw={} brotli-q11={}",
            output.len(),
            compressed.len()
        );
    }

    #[test]
    fn locator_profiles_are_deterministic_for_all_precisions_and_xml_is_strict() {
        let context = PlaceMapContext::embedded().unwrap();
        let point = ProjectedPoint::new(179.8, 84.0).unwrap();
        for precision in [
            Precision::Exact,
            Precision::City,
            Precision::Region,
            Precision::Country,
        ] {
            let frame = Frame::from_points(&[point], [precision].into_iter()).unwrap();
            let target = PlaceMapTarget {
                places: vec![ResolvedPlace {
                    key: "places/a&b".to_string(),
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
                "locator",
                precision_rank(&precision) as usize,
                "A\u{1}",
                precision,
            );
            let first = emit_locator(&context, &target, options).unwrap();
            let second = emit_locator(&context, &target, options).unwrap();
            assert_eq!(first, second);
            let mut compressor = brotli::CompressorWriter::new(Vec::new(), 4096, 11, 22);
            compressor.write_all(first.svg.as_bytes()).unwrap();
            assert!(compressor.into_inner().len() <= LOCATOR_BROTLI_LIMIT);
            assert!(roxmltree::Document::parse(&first.svg).is_ok());
            assert!(first.svg.contains("data-map-globe-marker=\"true\""));
            assert!(first.svg.contains("A�"));
        }
    }

    #[test]
    fn xml_escape_replaces_xml_forbidden_controls() {
        let escaped = xml_escape("ok\u{0}\u{8}\u{b}\u{c}\u{1f}");
        assert_eq!(escaped, "ok�����");
    }
}
