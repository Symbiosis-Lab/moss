//! Page-independent base-map SVGs for the places explorer: one canonical
//! world map, identical on every page that references it, and one detail
//! tile — for zooming past the world ceiling — per cell that matters to
//! THIS site: a cell a gazetteer place falls in, or one of its eight
//! neighbours, so panning one cell away from a place still has detail. A
//! reader zooms past the world ceiling only where the site actually has
//! places; the world SVG already covers everywhere else, end to end, at
//! that ceiling. Both are themed only by the `--moss-place-*` custom
//! properties `svg.rs`'s emitter already uses, so one file serves every
//! page and both themes.
//!
//! Unlike [`emit_svg`](super::emit_svg), neither the output bytes nor the
//! element ids here depend on which page references the file — two calls
//! for the same target are byte-identical, which a shared, content-hashed
//! asset must be (see `pipeline.rs`'s emission of these under
//! `_moss/map.<hash>/`). Markers are left to the explorer at runtime: the
//! `<g data-map-layer="marker">` group `render_svg` always emits is present
//! but empty here, the same contract a page's own map gives it.

use super::geometry::{tile_x, tile_y, world_viewbox_width, Frame, FrameTier, ProjectedPoint};
use super::svg::{render_svg, Ids, Writer, SVG_WIDTH};
use super::{Pack, PlaceMapContext, PlaceMapTarget};
use crate::vault::places::Gazetteer;

/// The frame `emit_world_svg` renders: centred on the prime meridian and
/// equator, the world tier. `longitude_span`/`latitude_span` are unread by
/// the Patterson projection the world tier selects (it always spans the
/// whole globe — see `geometry::PattersonProjection`) and by tile selection
/// (the `FrameTier::World` branch of `TileSelection::for_frame` already
/// takes every pack tile regardless of frame span); they carry the frame's
/// full nominal extent so a reader of this code never has to know that.
fn world_frame() -> Frame {
    Frame {
        center_longitude: 0.0,
        center_latitude: 0.0,
        longitude_span: 360.0,
        latitude_span: 180.0,
        tier: FrameTier::World,
    }
}

/// The frame centred on tile `(x, y)`'s own 10x10 degree cell — the same
/// grid `TileSelection` buckets the pack's fine-tier features into
/// (`geometry::tile_x`/`tile_y`, `Pack::tiles`). `x` runs 0..=35 west to
/// east from -180 degrees, `y` 0..=17 south to north from -90.
fn tile_frame(x: i16, y: i16) -> Frame {
    Frame {
        center_longitude: f64::from(x) * 10.0 - 175.0,
        center_latitude: f64::from(y) * 10.0 - 85.0,
        longitude_span: 10.0,
        latitude_span: 10.0,
        tier: FrameTier::Local,
    }
}

/// Emit the one canonical world SVG: full extent, no globe inset (the globe
/// shows where a frame sits in the world; the world map is its own answer,
/// the same rule the page-embedded map already applies), no markers (the
/// explorer places them at runtime). Byte-identical on every call for the
/// same pack, and carries no page-path-derived id — see
/// `emit_world_svg_carries_no_page_path_derived_id` below.
pub fn emit_world_svg(context: &PlaceMapContext) -> String {
    let target = PlaceMapTarget {
        places: Vec::new(),
        frame: Some(world_frame()),
        aggregate_name: None,
        route: false,
    };
    let ids = Ids::for_seed("world");
    let mut writer = Writer::for_explorer_asset(&ids, world_viewbox_width(), true);
    render_svg(&mut writer, context, &target, false);
    writer.output
}

/// Emit one regional detail tile. Local tier, so it keeps the same fine-tier
/// detail a place-term page's own local map does — the world map's
/// rank/layer thinning never applies to a 10-degree frame. No globe inset,
/// no markers, same reasons as `emit_world_svg`.
pub fn emit_tile_svg(context: &PlaceMapContext, x: i16, y: i16) -> String {
    let target = PlaceMapTarget {
        places: Vec::new(),
        frame: Some(tile_frame(x, y)),
        aggregate_name: None,
        route: false,
    };
    let ids = Ids::for_seed(&format!("tile-{x}-{y}"));
    let mut writer = Writer::for_explorer_asset(&ids, f64::from(SVG_WIDTH), false);
    render_svg(&mut writer, context, &target, false);
    writer.output
}

/// Every tile the pack actually has fine-tier features in, `(x, y)`. An
/// empty ocean cell has no `Tile` entry with features, so rendering one
/// would cost bytes for a blank map.
fn populated_tiles(pack: &Pack) -> std::collections::HashSet<(i16, i16)> {
    pack.tiles
        .iter()
        .filter(|tile| !tile.features.is_empty())
        .map(|tile| (tile.x, tile.y))
        .collect()
}

/// The regional tiles worth emitting for this site: the cell each gazetteer
/// place falls in, plus its eight neighbours, intersected with the pack's
/// own populated cells — so panning one cell away from a place still has
/// detail, without paying to render or ship a tile nowhere near anything
/// this site names. Sorted, so the result (and the hash fed from it, see
/// `emit::place_map_assets::assets_hash`) is deterministic regardless of
/// the gazetteer's own (name-ordered) iteration.
///
/// Reads `gazetteer` directly rather than only the names some page's
/// `location:` resolves: it is the one input the caller has in hand before
/// any page-level term resolution runs, and in practice a maintained
/// `.moss/places.toml` entry is one the site means to use. A place missing
/// coordinates, or one whose coordinates don't parse, contributes nothing —
/// the same silent skip `PlaceMapContext::resolve_locations` gives it.
pub fn relevant_tiles(gazetteer: &Gazetteer, pack: &Pack) -> Vec<(i16, i16)> {
    let populated = populated_tiles(pack);
    let mut cells = std::collections::BTreeSet::new();
    for (_, record) in gazetteer.iter() {
        let Some((latitude, longitude)) = record.coords else {
            continue;
        };
        let Some(point) = ProjectedPoint::new(longitude, latitude) else {
            continue;
        };
        let (center_x, center_y) = (tile_x(point.longitude), tile_y(point.latitude));
        for delta_y in -1..=1i32 {
            let y = i32::from(center_y) + delta_y;
            if !(0..18).contains(&y) {
                continue;
            }
            for delta_x in -1..=1i32 {
                // Longitude wraps (tile 35's east neighbour is tile 0);
                // latitude does not (no tile sits beyond the poles).
                let x = (i32::from(center_x) + delta_x).rem_euclid(36) as i16;
                let cell = (x, y as i16);
                if populated.contains(&cell) {
                    cells.insert(cell);
                }
            }
        }
    }
    cells.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::place_map::geometry::{world_viewbox_width, TileSelection, VIEWBOX_HEIGHT};
    use crate::build::place_map::{ProjectedPoint, Projection};

    /// Step 1 of the task: a `FrameTier::World` target already draws the
    /// fine tier across every pack tile — `TileSelection::for_frame`'s
    /// `FrameTier::World` branch takes every `pack.tiles` entry regardless
    /// of frame span, and every `Tile.features` entry indexes into
    /// `pack.tiers[1]`, the fine tier (`Pack.tiers[1]` holds layers 2..=10;
    /// see `place_map.rs`'s `FINE_LAYERS`). So the world SVG is not a new
    /// selection rule, only a page-independent way to call the existing one.
    #[test]
    fn a_world_tier_target_already_selects_every_tile_of_the_fine_tier() {
        let pack = super::super::embedded().unwrap();
        let frame = world_frame();
        let selection = TileSelection::for_frame(&pack, &frame);
        assert_eq!(
            selection.tiles.len(),
            pack.tiles.len(),
            "a World-tier frame must select every pack tile, not a spatial subset"
        );
        let selected_features = selection.features(&pack).len();
        let fine_tier_total: usize = pack.tiers[1]
            .layers
            .iter()
            .map(|layer| layer.feature_count as usize)
            .sum();
        assert_eq!(
            selected_features, fine_tier_total,
            "a World-tier frame must draw every fine-tier feature, the same total \
             emit_svg's own world_map() test pins"
        );
    }

    #[test]
    fn emit_world_svg_is_byte_identical_across_calls() {
        let context = PlaceMapContext::embedded().unwrap();
        let first = emit_world_svg(&context);
        let second = emit_world_svg(&context);
        assert_eq!(first, second);
    }

    #[test]
    fn emit_world_svg_carries_no_page_path_derived_id() {
        let context = PlaceMapContext::embedded().unwrap();
        let svg = emit_world_svg(&context);
        assert!(svg.contains("moss-place-map-world-"), "{svg:.200}");
        // The per-page emitter's ids are a 64-hex-char sha256 digest; the
        // world map's fixed seed is neither that shape nor derived from one.
        let base_start = svg.find("moss-place-map-").unwrap() + "moss-place-map-".len();
        let rest = &svg[base_start..];
        let id_end = rest.find(|c: char| !(c.is_ascii_hexdigit())).unwrap_or(rest.len());
        assert_ne!(id_end, 64, "a 64-hex-char id here would mean a page-derived hash leaked in");
    }

    #[test]
    fn emit_world_svg_has_no_globe_and_an_empty_marker_group() {
        let context = PlaceMapContext::embedded().unwrap();
        let svg = emit_world_svg(&context);
        assert!(!svg.contains("data-map-layer=\"globe\""));
        let marker_open = svg.find("data-map-layer=\"marker\"").expect("marker group present");
        assert!(svg[marker_open..].starts_with("data-map-layer=\"marker\" aria-label=\"markers\"></g>"));
    }

    #[test]
    fn emit_tile_svg_is_byte_identical_and_carries_its_own_coordinates_in_its_id() {
        let context = PlaceMapContext::embedded().unwrap();
        let first = emit_tile_svg(&context, 20, 12);
        let second = emit_tile_svg(&context, 20, 12);
        assert_eq!(first, second);
        assert!(first.contains("moss-place-map-tile-20-12-"));
        assert!(!first.contains("data-map-layer=\"globe\""));
    }

    fn gazetteer(entries: &str) -> Gazetteer {
        let table: toml::value::Table = toml::from_str(entries).unwrap();
        crate::vault::places::parse_gazetteer(&table)
    }

    /// A single place, well inside the pack's grid (not near a pole or the
    /// antimeridian), yields exactly its cell and its eight neighbours —
    /// all nine populated, since the pack's seafloor data reaches
    /// everywhere — and nothing further out.
    #[test]
    fn relevant_tiles_covers_a_place_cell_and_its_eight_neighbours() {
        let pack = super::super::embedded().unwrap();
        let gaz = gazetteer("[\"Beirut\"]\nlat = 33.89\nlng = 35.5\nprecision = \"city\"\n");
        let tiles = relevant_tiles(&gaz, &pack);
        let (center_x, center_y) = (tile_x(35.5), tile_y(33.89));
        let mut expected: Vec<(i16, i16)> = (-1..=1i32)
            .flat_map(|dy| (-1..=1i32).map(move |dx| (dx, dy)))
            .map(|(dx, dy)| (((i32::from(center_x) + dx).rem_euclid(36)) as i16, (i32::from(center_y) + dy) as i16))
            .collect();
        expected.sort();
        let mut actual = tiles.clone();
        actual.sort();
        assert_eq!(actual, expected, "expected the 3x3 neighbourhood around ({center_x}, {center_y})");
    }

    /// A place just west of the antimeridian wraps its neighbourhood
    /// through column 35 back to column 0 (`relevant_tiles`'s
    /// `rem_euclid(36)`), rather than running off the grid or stopping
    /// short at 35 — the earlier neighbourhood test (Beirut) sits nowhere
    /// near this edge and can't exercise the wrap.
    #[test]
    fn relevant_tiles_wraps_the_antimeridian() {
        let pack = super::super::embedded().unwrap();
        let gaz = gazetteer("[\"Near Dateline\"]\nlat = 0.0\nlng = 179.9\nprecision = \"city\"\n");
        let tiles = relevant_tiles(&gaz, &pack);
        let (center_x, center_y) = (tile_x(179.9), tile_y(0.0));
        assert_eq!(center_x, 35, "fixture assumption: 179.9 degrees sits in the last column");
        let mut expected: Vec<(i16, i16)> = (-1..=1i32)
            .flat_map(|dy| (-1..=1i32).map(move |dx| (dx, dy)))
            .map(|(dx, dy)| (((i32::from(center_x) + dx).rem_euclid(36)) as i16, (i32::from(center_y) + dy) as i16))
            .collect();
        expected.sort();
        let mut actual = tiles.clone();
        actual.sort();
        assert_eq!(actual, expected, "expected the wrapped 3x3 neighbourhood around ({center_x}, {center_y})");
        assert!(actual.contains(&(0, center_y)), "the eastward wrap must reach column 0, got {actual:?}");
    }

    /// A place near the south pole clamps its neighbourhood at row 0
    /// (`relevant_tiles`'s `0..18` bounds check dropping `y < 0`) instead
    /// of yielding a negative row — the earlier neighbourhood test sits
    /// nowhere near either pole and can't exercise the clamp.
    #[test]
    fn relevant_tiles_has_no_row_below_the_south_pole() {
        let pack = super::super::embedded().unwrap();
        let gaz = gazetteer("[\"Near South Pole\"]\nlat = -89.0\nlng = 0.0\nprecision = \"city\"\n");
        let tiles = relevant_tiles(&gaz, &pack);
        let (center_x, center_y) = (tile_x(0.0), tile_y(-89.0));
        assert_eq!(center_y, 0, "fixture assumption: -89 degrees sits in the bottom row");
        let mut expected: Vec<(i16, i16)> = (-1..=1i32)
            .filter(|&dy| (0..18).contains(&(i32::from(center_y) + dy)))
            .flat_map(|dy| (-1..=1i32).map(move |dx| (dx, dy)))
            .map(|(dx, dy)| (((i32::from(center_x) + dx).rem_euclid(36)) as i16, (i32::from(center_y) + dy) as i16))
            .collect();
        expected.sort();
        let mut actual = tiles.clone();
        actual.sort();
        assert_eq!(actual, expected, "expected only rows >= 0 around ({center_x}, {center_y})");
        assert!(actual.iter().all(|&(_, y)| y >= 0), "no row below 0 may appear, got {actual:?}");
    }

    /// Two places far apart contribute two separate (non-overlapping)
    /// neighbourhoods, each nine cells — proof the result is a union, not a
    /// single place's worth of detail.
    #[test]
    fn relevant_tiles_unions_every_gazetteer_place() {
        let pack = super::super::embedded().unwrap();
        let gaz = gazetteer(
            "[\"Beirut\"]\nlat = 33.89\nlng = 35.5\nprecision = \"city\"\n\
             [\"Bangkok\"]\nlat = 13.75\nlng = 100.5\nprecision = \"city\"\n",
        );
        let tiles = relevant_tiles(&gaz, &pack);
        assert_eq!(tiles.len(), 18, "{tiles:?}");
    }

    /// A gazetteer entry with no coordinates (the warned-about, kept-anyway
    /// shape `vault::places::parse_gazetteer` produces) contributes no
    /// cell — the same silent skip `PlaceMapContext::resolve_locations`
    /// gives a coordinate-less place.
    #[test]
    fn relevant_tiles_skips_a_place_with_no_coordinates() {
        let pack = super::super::embedded().unwrap();
        let gaz = gazetteer("[\"Nowhere\"]\nprecision = \"city\"\n");
        assert!(relevant_tiles(&gaz, &pack).is_empty());
    }

    /// How "meets at the shared edge" is measured: adjacent same-row tiles
    /// (x, y) and (x+1, y) share a `center_latitude`, and a 10x10-degree
    /// frame's `FlatProjection` always ends up height-bound on the 720x480
    /// (3:2) viewBox (the height-bound scale, `VIEWBOX_HEIGHT /
    /// latitude_span_radians`, is smaller than the width-bound one at every
    /// latitude, since `cos(latitude) <= 1`) — so both tiles derive a
    /// point's y from the identical formula, and the SAME (longitude,
    /// latitude) point at the boundary meridian must project to the same y
    /// in both (exact, not approximate) and to x values one closed-form
    /// pixel step apart: `scale * longitude_span_radians *
    /// cos(center_latitude)`. This renders the boundary point through each
    /// tile's own `Projection` — the same `project` call a coastline vertex
    /// on the boundary goes through inside `render_svg` — and checks both
    /// predictions hold to within a pixel, at three rows spanning the
    /// pack's latitude range.
    #[test]
    fn adjacent_tiles_meet_at_their_shared_edge_within_a_pixel() {
        for y in [0i16, 9, 16] {
            let (x_a, x_b) = (10i16, 11i16);
            let frame_a = tile_frame(x_a, y);
            let frame_b = tile_frame(x_b, y);
            assert_eq!(frame_a.center_latitude, frame_b.center_latitude, "same-row tiles share a center latitude");
            let projection_a = Projection::new(&frame_a);
            let projection_b = Projection::new(&frame_b);

            let boundary_longitude = f64::from(x_b) * 10.0 - 180.0;
            let boundary_point = ProjectedPoint::new(boundary_longitude, frame_a.center_latitude).unwrap();
            let (x_a_edge, y_a_edge) = projection_a.project(boundary_point).unwrap();
            let (x_b_edge, y_b_edge) = projection_b.project(boundary_point).unwrap();
            assert!((y_a_edge - y_b_edge).abs() < 1.0, "row {y}: y disagrees by {}", (y_a_edge - y_b_edge).abs());

            // Height-bound scale, in viewBox px per radian: the only scale
            // a 10x10-degree frame can reach on this viewBox (see doc).
            let scale = f64::from(VIEWBOX_HEIGHT) / frame_a.latitude_span.to_radians();
            let step = scale * frame_a.longitude_span.to_radians() * frame_a.center_latitude.to_radians().cos();
            assert!(
                (x_b_edge - (x_a_edge - step)).abs() < 1.0,
                "row {y}: x_a={x_a_edge} x_b={x_b_edge} expected step={step}, got {}",
                x_a_edge - x_b_edge
            );
        }
    }

    // -- Full-extent world projection (places explorer only) ------------

    /// Follow-up to task 3 of the places explorer plan: the per-page
    /// World-tier map (`geometry`'s `PattersonProjection::new`) fits the
    /// world to the 480-tall viewBox and crops the sides to a fixed
    /// 720-wide canvas — right for a page's own static map, wrong for this
    /// shared world SVG, which a runtime pans and zooms across the whole
    /// globe. The emitted SVG must instead carry the full, uncropped
    /// extent: a viewBox as wide as the Patterson projection's own scale
    /// makes the world, with 180°W at its left edge and 180°E at its
    /// right.
    #[test]
    fn emit_world_svg_carries_the_full_patterson_extent_with_no_crop() {
        let width = world_viewbox_width();
        // The emitted viewBox rounds to the thousandth of a pixel the same
        // way every other coordinate in this crate is serialized
        // (`svg.rs`'s private `length` helper); replicate that rounding
        // here rather than reach across modules for it.
        let rounded_width = (width * 1000.0).round() / 1000.0;
        let context = PlaceMapContext::embedded().unwrap();
        let svg = emit_world_svg(&context);
        let expected_viewbox = format!("viewBox=\"0 0 {rounded_width} {}\"", VIEWBOX_HEIGHT as u32);
        assert!(svg.contains(&expected_viewbox), "expected {expected_viewbox:?} in the emitted SVG: {:.200}", svg);

        let projection = Projection::new_world_full_extent();
        let project = |longitude: f64, latitude: f64| {
            projection.project(ProjectedPoint::new(longitude, latitude).unwrap()).unwrap()
        };
        let (west_x, _) = project(-180.0, 0.0);
        let (east_x, _) = project(180.0 - 1e-9, 0.0);
        let (center_x, _) = project(0.0, 0.0);
        let (_, pole_y) = project(0.0, 90.0);
        assert!(west_x.abs() < 1e-6, "180W should land at x=0, got {west_x}");
        assert!((east_x - width).abs() < 1e-6, "180E should land at x={width}, got {east_x}");
        assert!(
            (center_x - width / 2.0).abs() < 1e-6,
            "the prime meridian should land at x={}, got {center_x}",
            width / 2.0
        );
        assert!(pole_y.abs() < 1e-6, "the north pole should land at y=0, got {pole_y}");
    }

    /// Pins the explorer's full-extent projection to literal numbers, to
    /// six decimals, so the runtime's own tests (which reimplement this
    /// same projection in TypeScript to pan and zoom the shared world SVG)
    /// can hardcode the same values instead of re-deriving them.
    #[test]
    fn full_extent_projection_matches_pinned_literals_to_six_decimals() {
        let projection = Projection::new_world_full_extent();
        let assert_point = |longitude: f64, latitude: f64, expected_x: f64, expected_y: f64| {
            let (x, y) = projection.project(ProjectedPoint::new(longitude, latitude).unwrap()).unwrap();
            assert!((x - expected_x).abs() < 1e-6, "lat={latitude} lon={longitude}: x={x}, expected {expected_x}");
            assert!((y - expected_y).abs() < 1e-6, "lat={latitude} lon={longitude}: y={y}, expected {expected_y}");
        };
        assert_point(0.0, 0.0, 421.017513, 240.000000);
        assert_point(90.0, 45.0, 631.526269, 127.117606);
        assert_point(-120.0, -30.0, 140.339171, 312.230777);
    }
}
