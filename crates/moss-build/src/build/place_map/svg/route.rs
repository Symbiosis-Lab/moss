//! `route: true`: a dashed line through a page's `location:` stops, in list
//! order, with numbered badges — the one other thing besides the plain
//! marker layer this crate draws on a place map. See the module docs on
//! [`super`] for where a route may and may not appear; this file owns the
//! privacy gate, the geometry, and the SVG it emits, so `svg.rs` and
//! `context.rs` each call in rather than growing their own copy.
//!
//! Precision is privacy here the same way it is for a marker: a
//! country-precision stop is too coarse a point to commit a line through, so
//! it blocks the WHOLE route rather than letting the line jump past it
//! ([`route_precision_gate`]). A region-precision stop is allowed — the
//! route simply joins it at the centre of the soft halo `markers()` already
//! fades around that stop, disclosing nothing new — but its badge is drawn
//! hollow (an outline, not a filled chip) so the reader can tell "an area"
//! from "a point" at a glance ([`write_badge`]).

use std::fmt::Write;

use crate::build::place_map::{PlaceMapTarget, ProjectedPoint, Projection, ResolvedPlace};
use crate::vault::places::Precision;

use super::{clamp_dot, globe_line, length, serialize_path, snap, xml_escape, Writer, FINE};

/// The dashed line's and the hollow badge's colour — the
/// same muted slate-blue hue `--moss-place-shadow` already uses for the
/// filled badge's own chip, so a route's line and both badge
/// styles read as one quiet hue rather than two. A first value here
/// (`#8a4a32`, a clay tone, about 47% saturated) sat far more vivid than
/// every other `--moss-place-*` token, which stay under 10%. The light
/// value matches `--moss-place-shadow`'s own exactly; the dark value does
/// not (site.css's own comment on it says why: the shadow's dark value is a
/// background fill, too dark to read as a stroke or number against this
/// badge's near-black dark-theme casing). The only new `--moss-place-*`
/// token this feature adds. This literal is only the `var(..., fallback)`
/// default for a stylesheet that hasn't loaded yet — contrast is checked
/// against the real site.css values by the render gate, not against this
/// fallback.
const ROUTE_FALLBACK: &str = "#5a6488";

/// A badge's full width, in viewBox units: big enough to hold a two-digit
/// stop number without the glyphs touching the chip's edge, and — measured
/// live at the locator's ~350px display width, half the 720 viewBox — big
/// enough that its number and (for a hollow badge) its thin outline still
/// anti-alias to the full route colour instead of a paler blend; an 18-unit
/// badge with a 9-unit number measured 4.2:1 there, under the 4.5:1 floor,
/// purely from sub-pixel blending at that render size.
const BADGE_DIAMETER: f64 = 24.0;
const BADGE_RADIUS: f64 = BADGE_DIAMETER / 2.0;
/// Clear space kept between two numbered badges, which covers the filled
/// badge's casing ring.
const BADGE_GAP: f64 = 4.0;
/// The hollow badge's outline and both badges' number, in viewBox units —
/// scaled up from `BADGE_DIAMETER`'s own increase for the same reason.
const BADGE_FONT_SIZE: f64 = 12.0;
const HOLLOW_BADGE_STROKE_WIDTH: f64 = 2.0;

/// Whether a route may be drawn through `places`, in list order. Kept as
/// one small, clearly-named function so relaxing the policy later (a new
/// tier, a different blocking rule) means editing only this. A
/// country-precision stop is too coarse a point to draw a line through and
/// blocks the whole route; `Err` carries that stop's display name for the
/// one diagnostic this gate allows ([`route_blocked_diagnostic`]). Every
/// other precision — including `region`, which the route joins at its own
/// halo centre rather than refusing — is allowed. Only stops that actually
/// appear on the map (those with coordinates) are considered: a named place
/// with no `lat`/`lng` was never going to be drawn anyway.
pub(crate) fn route_precision_gate(places: &[ResolvedPlace]) -> Result<(), &str> {
    match places
        .iter()
        .find(|place| place.point().is_some() && place.precision == Precision::Country)
    {
        Some(place) => Err(place.display.as_str()),
        None => Ok(()),
    }
}

/// The diagnostic text for a route the gate above refused. Style matches the
/// gazetteer's own precision diagnostic (`vault::places::precision_diagnostic`):
/// name the page, the stop, and why — allowed set included, so the author
/// can tell "move this stop to city/region precision" from "routes don't
/// work". Pure and separately testable so a test pins the wording, not only
/// that some diagnostic fired.
pub(crate) fn route_blocked_diagnostic(page_path: &str, stop: &str) -> String {
    format!(
        "{page_path}: route not drawn — '{stop}' resolves to country precision, too coarse a point for a route stop (allowed: exact, city, region)"
    )
}

/// Which stops get a numbered badge, as indices into `stops`. Every stop
/// keeps its dot (the marker layer draws those); a number is only shown
/// where its badge fits, on the stop itself. The first and last stops are
/// always numbered. The rest are placed greedily in list order and dropped
/// when they would touch a badge already placed — the same
/// drop-on-collision approach the places-explorer's map labels take, so a
/// dense route reads as dots with numbers spaced along it rather than
/// badges pushed off their stops.
fn numbered_stops(stops: &[(f64, f64)]) -> Vec<usize> {
    let Some(last) = stops.len().checked_sub(1) else { return Vec::new() };
    let mut shown = vec![0];
    if last > 0 {
        shown.push(last);
    }
    for index in 1..last {
        if shown.iter().all(|&placed| distance(stops[placed], stops[index]) >= BADGE_DIAMETER + BADGE_GAP) {
            shown.push(index);
        }
    }
    shown.sort_unstable();
    shown
}

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// Every coordinate-bearing stop, in declared order, projected to this
/// figure's viewBox — the geometry every drawing function here shares.
fn projected_stops<'a>(target: &'a PlaceMapTarget, projection: &Projection) -> Vec<(&'a ResolvedPlace, (f64, f64))> {
    target
        .places
        .iter()
        .filter_map(|place| place.point().and_then(|point| projection.project(point)).map(|projected| (place, projected)))
        .collect()
}

/// The dashed route line, straight segments only (no curves, at every
/// tier), drawn under the marker layer: callers sequence this before
/// [`draw_badges`] and before `Writer::markers`. `projection` is `None` for
/// a target with no frame (nothing to project against) — same no-op as an
/// unset `route` flag, folded in here so callers don't carry their own copy
/// of this check.
pub(super) fn draw_line(writer: &mut Writer<'_>, target: &PlaceMapTarget, projection: Option<&Projection>) {
    if !target.route {
        return;
    }
    let Some(projection) = projection else { return };
    let stops = projected_stops(target, projection);
    if stops.len() < 2 {
        return;
    }
    let mut path = String::new();
    for (index, &(_, (x, y))) in stops.iter().enumerate() {
        let (x, y) = (snap(x), snap(y));
        write!(path, "{}{x} {y}", if index == 0 { "M" } else { "L" }).expect("writing to String cannot fail");
    }
    write!(
        writer.output,
        "<path d=\"{path}\" fill=\"none\" stroke=\"var(--moss-place-route, {ROUTE_FALLBACK})\" stroke-width=\"1.5\" stroke-dasharray=\"5 4\" vector-effect=\"non-scaling-stroke\" data-map-route=\"line\"/>"
    ).expect("writing to String cannot fail");
}

/// The numbered badges (1..N, in list order) drawn OVER
/// the marker layer: callers sequence this after `Writer::markers`. Same
/// `projection: None` no-op as [`draw_line`].
pub(super) fn draw_badges(writer: &mut Writer<'_>, target: &PlaceMapTarget, projection: Option<&Projection>) {
    if !target.route {
        return;
    }
    let Some(projection) = projection else { return };
    let stops = projected_stops(target, projection);
    if stops.is_empty() {
        return;
    }
    let points: Vec<(f64, f64)> = stops.iter().map(|&(_, point)| point).collect();
    for index in numbered_stops(&points) {
        let (place, center) = stops[index];
        write_badge(writer, index + 1, center, place.precision);
    }
}

fn write_badge(writer: &mut Writer<'_>, number: usize, center: (f64, f64), precision: Precision) {
    let (cx, cy) = clamp_dot(center, BADGE_RADIUS + 1.0);
    let (cx, cy) = (snap(cx), snap(cy));
    let label = xml_escape(&number.to_string());
    if precision == Precision::Region {
        // An area, not a point: an outline chip in the route's own hue
        // rather than the filled dark chip below, over a solid backing
        // (the same marker-casing token the plain dot already relies on
        // for contrast against whatever terrain sits underneath).
        write!(
            writer.output,
            "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"var(--moss-place-marker-casing, #ffffff)\" stroke=\"var(--moss-place-route, {ROUTE_FALLBACK})\" stroke-width=\"{stroke}\" data-map-route-badge=\"{number}\" data-map-route-badge-style=\"hollow\"/><text x=\"{cx}\" y=\"{cy}\" text-anchor=\"middle\" dominant-baseline=\"central\" font-size=\"{font_size}\" font-weight=\"700\" font-family=\"sans-serif\" fill=\"var(--moss-place-route, {ROUTE_FALLBACK})\">{label}</text>",
            r = length(BADGE_RADIUS),
            stroke = length(HOLLOW_BADGE_STROKE_WIDTH),
            font_size = length(BADGE_FONT_SIZE),
        ).expect("writing to String cannot fail");
    } else {
        write!(
            writer.output,
            "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r1}\" fill=\"var(--moss-place-marker-casing, #ffffff)\"/><circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r2}\" fill=\"var(--moss-place-shadow, #5a6488)\" data-map-route-badge=\"{number}\" data-map-route-badge-style=\"filled\"/><text x=\"{cx}\" y=\"{cy}\" text-anchor=\"middle\" dominant-baseline=\"central\" font-size=\"{font_size}\" font-weight=\"700\" font-family=\"sans-serif\" fill=\"#ffffff\">{label}</text>",
            r1 = length(BADGE_RADIUS + 1.5),
            r2 = length(BADGE_RADIUS),
            font_size = length(BADGE_FONT_SIZE),
        ).expect("writing to String cannot fail");
    }
}

/// The globe inset's thin version of the same route, no numerals — at
/// about 60px across there is no room for a legible digit. Reuses
/// `globe_line`'s own lon/lat interpolation and horizon clipping, the same
/// primitive a coastline draws with, rather than a second sampler: the
/// route is a straight line in geographic space either way, drawn on a
/// sphere, which is why it reads as a gentle curve there and nowhere else.
pub(super) fn draw_globe_line(writer: &mut Writer<'_>, target: &PlaceMapTarget, center: ProjectedPoint, quantisation: u32) {
    if !target.route {
        return;
    }
    let quant = f64::from(quantisation);
    let points: Vec<(i32, i32)> = target
        .places
        .iter()
        .filter_map(ResolvedPlace::point)
        .map(|point| (snap(point.longitude * quant), snap(point.latitude * quant)))
        .collect();
    if points.len() < 2 {
        return;
    }
    let paths = globe_line(&points, center, quantisation);
    if let Some(path) = serialize_path(&paths, false, FINE) {
        write!(
            writer.output,
            "<path d=\"{path}\" fill=\"none\" stroke=\"var(--moss-place-route, {ROUTE_FALLBACK})\" stroke-width=\"0.75\" vector-effect=\"non-scaling-stroke\" data-map-route=\"globe-line\"/>"
        ).expect("writing to String cannot fail");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::place_map::Frame;
    use super::super::{Ids, SVG_HEIGHT, SVG_WIDTH};

    fn place(display: &str, lon: f64, lat: f64, precision: Precision) -> ResolvedPlace {
        ResolvedPlace {
            key: format!("places/{}", display.to_lowercase()),
            display: display.to_string(),
            longitude: Some(lon),
            latitude: Some(lat),
            precision,
        }
    }

    /// A minimal `Writer` for calling a drawing function directly in a
    /// test, with no page-derived id (the id is irrelevant to every test
    /// that uses this).
    fn bare_writer(ids: &Ids) -> Writer<'_> {
        Writer {
            output: String::new(),
            ids,
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
            layers: &[],
        }
    }

    // --- route_precision_gate ---

    #[test]
    fn gate_allows_a_mix_of_exact_city_and_region() {
        let places = [
            place("A", 0.0, 0.0, Precision::Exact),
            place("B", 1.0, 1.0, Precision::City),
            place("C", 2.0, 2.0, Precision::Region),
        ];
        assert_eq!(route_precision_gate(&places), Ok(()));
    }

    #[test]
    fn gate_blocks_on_a_country_precision_stop_and_names_it() {
        let places = [
            place("A", 0.0, 0.0, Precision::City),
            place("B", 1.0, 1.0, Precision::Country),
            place("C", 2.0, 2.0, Precision::Region),
        ];
        assert_eq!(route_precision_gate(&places), Err("B"));
    }

    #[test]
    fn gate_ignores_a_country_stop_with_no_coordinates() {
        // A named place absent from the gazetteer, or missing lat/lng,
        // coarsens to Country but was never going to be drawn — the gate
        // only judges stops that actually appear on the map.
        let mut uncharted = place("Nowhere", 0.0, 0.0, Precision::Country);
        uncharted.longitude = None;
        uncharted.latitude = None;
        let places = [place("A", 0.0, 0.0, Precision::City), uncharted];
        assert_eq!(route_precision_gate(&places), Ok(()));
    }

    // --- route_blocked_diagnostic ---

    #[test]
    fn blocked_diagnostic_names_the_page_the_stop_and_the_allowed_set() {
        let message = route_blocked_diagnostic("essays/28-to-spain/index.html", "Madrid");
        assert!(message.contains("essays/28-to-spain/index.html"), "{message}");
        assert!(message.contains("Madrid"), "{message}");
        for allowed in ["exact", "city", "region"] {
            assert!(message.contains(allowed), "{message}");
        }
        assert!(!message.contains("country,"), "allowed set excludes the blocking tier: {message}");
    }

    // --- numbered_stops ---

    #[test]
    fn spaced_stops_are_all_numbered() {
        let stops = [(0.0, 0.0), (100.0, 0.0), (200.0, 50.0)];
        assert_eq!(numbered_stops(&stops), vec![0, 1, 2]);
    }

    #[test]
    fn crowded_stops_keep_first_and_last_and_drop_numbers_that_touch() {
        // Stops 10 units apart: a 24-unit badge covers its neighbours.
        let stops: Vec<(f64, f64)> = (0..30).map(|i| (f64::from(i) * 10.0, 0.0)).collect();
        let shown = numbered_stops(&stops);
        assert_eq!(shown.first(), Some(&0));
        assert_eq!(shown.last(), Some(&29));
        assert!(shown.len() < stops.len() && shown.len() > 4, "{shown:?}");
        for pair in shown.windows(2) {
            let gap = distance(stops[pair[0]], stops[pair[1]]);
            assert!(gap >= BADGE_DIAMETER, "badges {pair:?} overlap: {gap}");
        }
    }

    #[test]
    fn a_single_stop_is_numbered_once() {
        assert_eq!(numbered_stops(&[(5.0, 5.0)]), vec![0]);
        assert!(numbered_stops(&[]).is_empty());
    }

    /// 55 stops along a 500 km road: every badge that is drawn sits on its
    /// own stop (no leaders, no displacement), and the ends are numbered.
    #[test]
    fn a_dense_route_numbers_only_what_fits_and_keeps_each_badge_on_its_stop() {
        let waypoints = [(139.77, 35.68), (139.10, 35.25), (138.38, 34.97), (137.73, 34.71), (137.00, 35.05), (136.30, 34.90), (135.77, 35.01)];
        let total: usize = 55;
        let places: Vec<ResolvedPlace> = (0..total)
            .map(|i| {
                let t = i as f64 / (total - 1) as f64 * (waypoints.len() - 1) as f64;
                let seg = (t.floor() as usize).min(waypoints.len() - 2);
                let f = t - seg as f64;
                let (a, b) = (waypoints[seg], waypoints[seg + 1]);
                place(&format!("Stop {i}"), a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f, Precision::Exact)
            })
            .collect();
        let points: Vec<ProjectedPoint> = places.iter().filter_map(ResolvedPlace::point).collect();
        let frame = Frame::from_points(&points, places.iter().map(|p| p.precision)).expect("frame");
        let projection = Projection::new(&frame);
        let target = PlaceMapTarget { places: places.clone(), frame: Some(frame.clone()), aggregate_name: None, route: true };
        let ids = Ids::for_seed("dense-route-test");
        let mut writer = bare_writer(&ids);
        draw_badges(&mut writer, &target, Some(&projection));

        let out = &writer.output;
        assert!(!out.contains("data-map-route=\"leader\""), "no leader lines: {out}");
        let mut numbers = Vec::new();
        for chunk in out.split("data-map-route-badge=\"").skip(1) {
            let number: usize = chunk[..chunk.find('"').unwrap()].parse().unwrap();
            // The circle's own cx/cy precede its attribute; read them from the text before it.
            numbers.push(number);
        }
        assert_eq!(numbers.first(), Some(&1));
        assert_eq!(numbers.last(), Some(&total));
        assert!(numbers.len() < total && numbers.len() >= 4, "{numbers:?}");
        for number in numbers {
            let stop = projection.project(places[number - 1].point().unwrap()).unwrap();
            let (sx, sy) = clamp_dot(stop, BADGE_RADIUS + 1.0);
            let needle = format!("cx=\"{}\" cy=\"{}\" r=\"", snap(sx), snap(sy));
            let badge = out.find(&format!("data-map-route-badge=\"{number}\"")).unwrap();
            assert!(out[..badge].rfind(&needle).is_some(), "badge {number} is not on its stop ({sx}, {sy}): {out}");
        }
    }

    // --- write_badge ---

    /// A badge centred off-canvas gets pulled in by `clamp_dot`; the chip is
    /// drawn at that clamped point. Chosen far enough off-canvas
    /// (`SVG_WIDTH` is 720) that clamping is certain to move it.
    #[test]
    fn a_badge_near_the_edge_is_drawn_at_its_clamped_position() {
        let ids = Ids::for_seed("badge-clamp-test");
        let mut writer = bare_writer(&ids);
        let raw_centre = (-500.0, 240.0);
        write_badge(&mut writer, 1, raw_centre, Precision::City);

        let (clamped_x, clamped_y) = clamp_dot(raw_centre, BADGE_RADIUS + 1.0);
        let (expected_x, expected_y) = (snap(clamped_x), snap(clamped_y));
        assert_ne!(
            (expected_x, expected_y),
            (snap(raw_centre.0), snap(raw_centre.1)),
            "test is only meaningful if clamping actually moves the centre"
        );
        let chip_centre = format!("cx=\"{expected_x}\" cy=\"{expected_y}\"");
        assert!(writer.output.contains(&chip_centre), "{}", writer.output);
    }

    // --- badge sizing ---

    /// A budget check, not a screenshot: a sans-serif digit is roughly 0.6x
    /// its font-size wide, so two digits at `BADGE_FONT_SIZE` must still
    /// clear the chip's own diameter with margin — this is what the render
    /// gate's contrast check cannot see (it measures colour, not layout),
    /// and would catch `BADGE_DIAMETER` shrinking back toward
    /// `BADGE_FONT_SIZE`'s own scale the way the pre-fix 18/9 pair did.
    #[test]
    fn a_two_digit_badge_number_fits_inside_the_chip_and_renders_in_full() {
        let two_digit_width = BADGE_FONT_SIZE * 0.6 * 2.0;
        let budget = BADGE_DIAMETER * 0.9;
        assert!(
            two_digit_width <= budget,
            "a two-digit number (~{two_digit_width:.1} units) must fit well inside the {BADGE_DIAMETER}-unit chip (budget {budget:.1})"
        );

        let ids = Ids::for_seed("two-digit-badge-test");
        let mut writer = bare_writer(&ids);
        write_badge(&mut writer, 12, (100.0, 100.0), Precision::City);
        assert!(writer.output.contains(">12<"), "the label must render both digits, not truncate: {}", writer.output);
    }

    // --- known limitation: the antimeridian ---

    /// KNOWN LIMITATION, pinned rather than fixed in this landing:
    /// `Projection` does not unwrap longitude, so two stops that are
    /// geographically close but straddle +/-180 degrees project to opposite
    /// edges of the frame, and the straight line between them (no curves,
    /// at every tier) draws all the way across the map instead of the short
    /// way around. A later fix should unwrap a route's own stops (or teach
    /// the projection to) before drawing; when it does, this test's
    /// assertion flips from "still far apart" to "close together" and
    /// should be rewritten rather than deleted.
    #[test]
    fn a_route_crossing_the_antimeridian_draws_a_chord_not_a_wrap_today() {
        let west = place("West", 179.0, 10.0, Precision::City);
        let east = place("East", -179.0, 10.0, Precision::City);
        let points: Vec<ProjectedPoint> = [&west, &east].iter().filter_map(|p| p.point()).collect();
        let frame = Frame::from_points(&points, [west.precision, east.precision].into_iter())
            .expect("two valid points make a frame");
        let projection = Projection::new(&frame);
        let projected: Vec<(f64, f64)> = points
            .iter()
            .filter_map(|&point| projection.project(point))
            .collect();
        assert_eq!(projected.len(), 2, "both antimeridian-straddling stops must still project to a point");
        let gap = (projected[0].0 - projected[1].0).abs();
        assert!(
            gap > 50.0,
            "today's known limitation: two stops 2 degrees apart in reality still project {gap:.0} viewBox units apart (measured 139 when this test was written), not wrapped — if this now fails, the projection has started unwrapping and this test should be rewritten to assert the stops project close together instead of deleted"
        );
    }
}
