//! Deterministic, self-contained SVG emission for a place map.
//!
//! The emitter deliberately knows nothing about pages, configuration, or CSS
//! files.  Its only inputs are the immutable decoded pack and a resolved map
//! target.  That makes the byte output suitable for page snapshots and keeps
//! the geometry/privacy boundary in [`super::context`] and [`super::geometry`].

use std::fmt::Write;

use sha2::{Digest, Sha256};

use super::geometry::{marker_radius, FrameTier, ProjectedPoint, Projection, TileSelection};
use super::{Feature, PlaceMapContext, PlaceMapTarget};
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

/// Return the deterministic uncompressed byte size of an emitted locator.
/// Brotli is intentionally left to the build consumer: this crate has no
/// Brotli dependency, and the consumer owns the final compression policy and
/// 16 KiB budget gate.
pub fn raw_svg_bytes(svg: &str) -> usize {
    svg.len()
}

/// Emit one map figure with an explicit accessible label/link.
pub fn emit_svg_with_options(
    context: &PlaceMapContext,
    target: &PlaceMapTarget,
    options: SvgMapOptions<'_>,
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

    if let Some(frame) = target.frame.as_ref() {
        let projection = Projection::new(frame);
        let selected = TileSelection::for_frame(context.pack(), frame);
        writer.emit_layers(context, &projection, &selected);
    } else {
        // A missing coordinate is deliberate no-map input.  Keeping a valid
        // empty figure here lets the caller preserve surrounding hierarchy
        // without inventing a point or leaking source values.
        writer.empty_layers();
    }
    writer.lighting();
    if target.frame.is_none() {
        writer.empty_layers_after_lighting();
    } else if let Some(frame) = target.frame.as_ref() {
        let projection = Projection::new(frame);
        let selected = TileSelection::for_frame(context.pack(), frame);
        writer.emit_surface_layers(context, &projection, &selected);
    }
    writer.markers(target);
    writer.globe(context, target);
    writer.close_svg();
    writer.close_figure();
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
            write!(self.output, "<a href=\"{}\">", xml_escape(href))
                .expect("writing to String cannot fail");
        }
    }

    fn close_figure(&mut self) {
        // The optional anchor is closed by the SVG's sibling marker.  A
        // marker byte makes this deterministic without carrying another bool
        // through every writer method; href users receive the balanced close.
        if self.output.ends_with("</svg></a>") {
            self.output.push_str("</figure>");
        } else {
            self.output.push_str("</figure>");
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
        if self.output.ends_with("</svg>") {
            // An anchor, when present, is opened immediately before the SVG.
            // Closing it here is safe because no other element may follow the
            // map SVG inside the figure.
            let marker = "</a>";
            if !self.output.ends_with(marker) {
                // The caller's href is represented by this exact opening
                // marker, and `close_svg` is the sole place that can balance
                // it.  `</a>` is harmless for no-href figures only if absent;
                // use the opening scan below to avoid that case.
                let figure_start = self.output.rfind("<figure ").unwrap_or(0);
                let figure = &self.output[figure_start..];
                if figure.contains("<a href=") {
                    self.output.push_str("</a>");
                }
            }
        }
    }

    fn defs(&mut self) {
        let height_filter = self.ids.get("height-filter");
        let marker_gradient = self.ids.get("marker-fade");
        let globe_clip = self.ids.get("globe-clip");
        let height_empty = self.ids.get("height-empty");
        write!(
            self.output,
            "<defs><filter id=\"{height_filter}\" color-interpolation-filters=\"sRGB\"><feGaussianBlur in=\"SourceGraphic\" stdDeviation=\"9\" result=\"height-blur-9\"/><feGaussianBlur in=\"SourceGraphic\" stdDeviation=\"4\" opacity=\"0.3\" result=\"height-blur-4\"/><feBlend in=\"height-blur-9\" in2=\"height-blur-4\" mode=\"screen\" result=\"height-field\"/><feColorMatrix in=\"height-field\" type=\"luminanceToAlpha\" result=\"height-alpha\"/><feDiffuseLighting in=\"height-alpha\" surfaceScale=\"1\" diffuseConstant=\"1\" lighting-color=\"var(--moss-place-light)\" result=\"diffuse\"><feDistantLight azimuth=\"240\" elevation=\"45\"/></feDiffuseLighting><feComposite in=\"diffuse\" in2=\"SourceGraphic\" operator=\"in\"/></filter><radialGradient id=\"{marker_gradient}\"><stop offset=\"0\" stop-color=\"var(--moss-place-marker)\"/><stop offset=\"1\" stop-color=\"var(--moss-place-marker)\" stop-opacity=\"0\"/></radialGradient><clipPath id=\"{globe_clip}\"><circle cx=\"{GLOBE_CENTER_X:.0}\" cy=\"{GLOBE_CENTER_Y:.0}\" r=\"{GLOBE_RADIUS:.0}\"/></clipPath><path id=\"{height_empty}\" d=\"m0 0l0 0\"/></defs>",
        )
        .expect("writing to String cannot fail");
    }

    fn water(&mut self) {
        let id = self.ids.get("layer-water");
        write!(
            self.output,
            "<g id=\"{id}\" data-map-layer=\"water\"><rect width=\"{SVG_WIDTH}\" height=\"{SVG_HEIGHT}\" fill=\"var(--moss-place-water)\"/></g>",
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
        context: &PlaceMapContext,
        projection: &Projection,
        selected: &TileSelection,
    ) {
        // Sea floor is painted shallow to deep, then land and relief low to
        // high.  The source pack has stable IDs; no administrative layer is
        // accepted or emitted here.
        self.emit_band_layer(
            context,
            projection,
            selected,
            10,
            "seafloor",
            true,
            "var(--moss-place-seafloor)",
        );
        self.emit_filled_layer(
            context,
            projection,
            selected,
            2,
            "land",
            "var(--moss-place-land)",
        );
        self.emit_band_layer(
            context,
            projection,
            selected,
            9,
            "relief",
            false,
            "var(--moss-place-relief)",
        );
    }

    fn emit_surface_layers(
        &mut self,
        context: &PlaceMapContext,
        projection: &Projection,
        selected: &TileSelection,
    ) {
        self.emit_line_layer(
            context,
            projection,
            selected,
            1,
            "coast",
            "var(--moss-place-coast)",
        );
        self.emit_filled_layer(
            context,
            projection,
            selected,
            3,
            "lakes",
            "var(--moss-place-lakes)",
        );
        self.emit_line_layer(
            context,
            projection,
            selected,
            4,
            "rivers",
            "var(--moss-place-rivers)",
        );
        self.emit_filled_layer(
            context,
            projection,
            selected,
            5,
            "ice",
            "var(--moss-place-ice)",
        );
        self.emit_filled_layer(
            context,
            projection,
            selected,
            6,
            "reefs",
            "var(--moss-place-reefs)",
        );
        self.emit_filled_layer(
            context,
            projection,
            selected,
            7,
            "salt",
            "var(--moss-place-salt)",
        );
        self.emit_filled_layer(
            context,
            projection,
            selected,
            8,
            "built-up",
            "var(--moss-place-built-up)",
        );
    }

    fn emit_band_layer(
        &mut self,
        context: &PlaceMapContext,
        projection: &Projection,
        selected: &TileSelection,
        layer_id: u8,
        name: &str,
        shallow_first: bool,
        color: &str,
    ) {
        let mut features: Vec<&Feature> = selected
            .features(context.pack())
            .into_iter()
            .filter_map(|(id, feature)| (id == layer_id).then_some(feature))
            .collect();
        features.sort_by(|a, b| {
            if shallow_first {
                b.band.cmp(&a.band)
            } else {
                a.band.cmp(&b.band)
            }
        });
        self.output.push_str(&format!(
            "<g id=\"{}\" data-map-layer=\"{name}\">",
            self.ids.get(&format!("layer-{name}"))
        ));
        for (index, feature) in features.into_iter().enumerate() {
            self.emit_filled_feature(
                projection,
                feature,
                context.pack().header.quantisation,
                color,
                &format!("{}-{name}-{index}", self.ids.base),
            );
        }
        self.output.push_str("</g>");
    }

    fn emit_filled_layer(
        &mut self,
        context: &PlaceMapContext,
        projection: &Projection,
        selected: &TileSelection,
        layer_id: u8,
        name: &str,
        color: &str,
    ) {
        let features: Vec<&Feature> = selected
            .features(context.pack())
            .into_iter()
            .filter_map(|(id, feature)| (id == layer_id).then_some(feature))
            .collect();
        self.output.push_str(&format!(
            "<g id=\"{}\" data-map-layer=\"{name}\">",
            self.ids.get(&format!("layer-{name}"))
        ));
        for (index, feature) in features.into_iter().enumerate() {
            self.emit_filled_feature(
                projection,
                feature,
                context.pack().header.quantisation,
                color,
                &format!("{}-{name}-{index}", self.ids.base),
            );
        }
        self.output.push_str("</g>");
    }

    fn emit_line_layer(
        &mut self,
        context: &PlaceMapContext,
        projection: &Projection,
        selected: &TileSelection,
        layer_id: u8,
        name: &str,
        color: &str,
    ) {
        let features: Vec<&Feature> = selected
            .features(context.pack())
            .into_iter()
            .filter_map(|(id, feature)| (id == layer_id).then_some(feature))
            .collect();
        self.output.push_str(&format!(
            "<g id=\"{}\" data-map-layer=\"{name}\">",
            self.ids.get(&format!("layer-{name}"))
        ));
        for (index, feature) in features.into_iter().enumerate() {
            let mut paths = Vec::new();
            for part in &feature.parts {
                paths.extend(projection.project_part(part, context.pack().header.quantisation));
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
            "<path id=\"{id}\" d=\"{path}\" fill=\"{color}\" fill-rule=\"evenodd\"/>"
        )
        .expect("writing to String cannot fail");
        self.height_uses.push(id.to_string());
    }

    fn lighting(&mut self) {
        let filter = self.ids.get("height-filter");
        let id = self.ids.get("lighting");
        write!(self.output, "<g id=\"{id}\" data-map-layer=\"lighting\" filter=\"url(#{filter})\" aria-hidden=\"true\"><g id=\"{}\" opacity=\"0.3\">", self.ids.get("height-field")).expect("writing to String cannot fail");
        for path_id in &self.height_uses {
            write!(self.output, "<use href=\"#{path_id}\" height=\"100%\"/>")
                .expect("writing to String cannot fail");
        }
        if self.height_uses.is_empty() {
            write!(
                self.output,
                "<use href=\"#{}\" height=\"100%\"/>",
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
                    "var(--moss-place-marker)".to_string()
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
        write!(self.output, "<g id=\"{id}\" data-map-layer=\"globe\" clip-path=\"url(#{clip})\"><circle cx=\"{GLOBE_CENTER_X:.0}\" cy=\"{GLOBE_CENTER_Y:.0}\" r=\"{GLOBE_RADIUS:.0}\" fill=\"var(--moss-place-globe-water)\"/>").expect("writing to String cannot fail");
        let tier = &context.pack().tiers[0];
        for layer in &tier.layers {
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
                        write!(self.output, "<path d=\"{path}\" fill=\"none\" stroke=\"var(--moss-place-globe-coast)\" stroke-width=\"0.5\" data-globe-feature=\"{index}\"/>").expect("writing to String cannot fail");
                    }
                } else {
                    let rings: Vec<Vec<(f64, f64)>> = feature
                        .parts
                        .iter()
                        .filter_map(|part| {
                            globe_ring(part, center, context.pack().header.quantisation)
                        })
                        .collect();
                    if let Some(path) = serialize_path(&rings, true) {
                        write!(self.output, "<path d=\"{path}\" fill=\"var(--moss-place-globe-land)\" fill-rule=\"evenodd\" data-globe-feature=\"{index}\"/>").expect("writing to String cannot fail");
                    }
                }
            }
        }
        write!(self.output, "</g><circle cx=\"{GLOBE_CENTER_X:.0}\" cy=\"{GLOBE_CENTER_Y:.0}\" r=\"{GLOBE_RADIUS:.0}\" fill=\"none\" stroke=\"var(--moss-place-globe-edge)\" stroke-width=\"1\" data-map-globe-inset=\"true\"/>").expect("writing to String cannot fail");
    }
}

fn precision_rank(precision: &Precision) -> u8 {
    match precision {
        Precision::Exact => 0,
        Precision::City => 1,
        Precision::Region => 2,
        Precision::Country => 3,
    }
}

fn precision_name(precision: Precision) -> &'static str {
    match precision {
        Precision::Exact => "exact",
        Precision::City => "city",
        Precision::Region => "region",
        Precision::Country => "country",
    }
}

fn xml_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '\"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

fn snap(value: f64) -> i32 {
    value
        .round()
        .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

fn serialize_path(paths: &[Vec<(f64, f64)>], closed: bool) -> Option<String> {
    let mut output = String::new();
    let mut cursor = (0i32, 0i32);
    let mut wrote = false;
    for path in paths {
        if path.len() < if closed { 3 } else { 2 } {
            continue;
        }
        let mut points: Vec<(i32, i32)> = path.iter().map(|&(x, y)| (snap(x), snap(y))).collect();
        points.dedup();
        if closed && points.first() == points.last() {
            points.pop();
        }
        if points.len() < if closed { 3 } else { 2 } {
            continue;
        }
        let start = points[0];
        write!(output, "m{} {}", start.0 - cursor.0, start.1 - cursor.1)
            .expect("writing to String cannot fail");
        cursor = start;
        for point in points.iter().skip(1) {
            write!(output, "l{} {}", point.0 - cursor.0, point.1 - cursor.1)
                .expect("writing to String cannot fail");
            cursor = *point;
        }
        if closed {
            output.push('z');
        }
        wrote = true;
    }
    wrote.then_some(output)
}

fn globe_point(point: (i32, i32), center: ProjectedPoint, quantisation: u32) -> Option<(f64, f64)> {
    let longitude = (f64::from(point.0) / f64::from(quantisation)).to_radians();
    let latitude = (f64::from(point.1) / f64::from(quantisation)).to_radians();
    let center_longitude = center.longitude.to_radians();
    let center_latitude = center.latitude.to_radians();
    let delta = longitude - center_longitude;
    let visible = center_latitude.sin() * latitude.sin()
        + center_latitude.cos() * latitude.cos() * delta.cos();
    (visible >= 0.0).then_some((
        GLOBE_CENTER_X + GLOBE_RADIUS * latitude.cos() * delta.sin(),
        GLOBE_CENTER_Y
            - GLOBE_RADIUS
                * (center_latitude.cos() * latitude.sin()
                    - center_latitude.sin() * latitude.cos() * delta.cos()),
    ))
}

fn globe_ring(
    points: &[(i32, i32)],
    center: ProjectedPoint,
    quantisation: u32,
) -> Option<Vec<(f64, f64)>> {
    let projected: Vec<_> = points
        .iter()
        .filter_map(|&point| globe_point(point, center, quantisation))
        .collect();
    (projected.len() >= 3 && projected.len() == points.len()).then_some(projected)
}

fn globe_line(
    points: &[(i32, i32)],
    center: ProjectedPoint,
    quantisation: u32,
) -> Vec<Vec<(f64, f64)>> {
    let mut paths = Vec::new();
    let mut current = Vec::new();
    for &point in points {
        if let Some(projected) = globe_point(point, center, quantisation) {
            current.push(projected);
        } else if current.len() >= 2 {
            paths.push(std::mem::take(&mut current));
        } else {
            current.clear();
        }
    }
    if current.len() >= 2 {
        paths.push(current);
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
