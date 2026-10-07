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

use super::geometry::{tile_x, tile_y, world_viewbox_width, Frame, FrameTier, ProjectedPoint, Projection, TileGrid, TILE_COLUMNS, TILE_ROWS};
use super::svg::{render_svg, Ids, Writer, SVG_HEIGHT};
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
/// east from -180 degrees, `y` 0..=17 south to north from -90. `tier` is
/// `Tile`, not `Local`: a regional tile draws in the SAME Patterson
/// projection as the world map, at `TILE_K` times its scale
/// (`geometry::PattersonProjection::for_tile`) — not the equirectangular
/// `FlatProjection` a per-page local map uses — so a tile overlays the
/// world exactly instead of showing a different crop of a different
/// projection at the same screen position.
fn tile_frame(x: i16, y: i16) -> Frame {
    Frame {
        center_longitude: f64::from(x) * 10.0 - 175.0,
        center_latitude: f64::from(y) * 10.0 - 85.0,
        longitude_span: 10.0,
        latitude_span: 10.0,
        tier: FrameTier::Tile,
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
    let mut writer = Writer::for_explorer_asset(&ids, world_viewbox_width(), f64::from(SVG_HEIGHT), true);
    render_svg(&mut writer, context, &target, false);
    writer.output
}

/// Emit one regional detail tile: the same fine-tier detail a place-term
/// page's own local map draws (the world map's rank/layer thinning never
/// applies to a `Tile` frame — see `render_svg`'s own `wide` guard), but in
/// the world's own Patterson projection at `TILE_K` times its scale rather
/// than a separately-cropped equirectangular frame, so the tile overlays
/// the world exactly wherever the runtime places it. The viewBox is the
/// cell's own Patterson rectangle scaled by `TILE_K`
/// (`Projection::canvas_size`, backed by `PattersonProjection::for_tile`) —
/// computed from the SAME projection `render_svg` draws through below, so
/// the two can never disagree about the tile's own canvas size. No globe
/// inset, no markers, same reasons as `emit_world_svg`.
pub fn emit_tile_svg(context: &PlaceMapContext, x: i16, y: i16) -> String {
    let frame = tile_frame(x, y);
    let (canvas_width, canvas_height) = Projection::new(&frame).canvas_size();
    let target = PlaceMapTarget {
        places: Vec::new(),
        frame: Some(frame),
        aggregate_name: None,
        route: false,
    };
    let ids = Ids::for_seed(&format!("tile-{x}-{y}"));
    let mut writer = Writer::for_explorer_asset(&ids, canvas_width, canvas_height, false);
    render_svg(&mut writer, context, &target, false);
    writer.output
}

/// Where tile `(x, y)`'s canvas starts on the world's lattice of
/// `1 / TILE_K` units, in whole canvas units: the top-left corner the
/// runtime places the tile's own `(0, 0)` at (`tiles.json`'s `origins`).
pub fn tile_origin_units(x: i16, y: i16) -> (i64, i64) {
    TileGrid::of(&tile_frame(x, y)).origin_units()
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
            if !(0..TILE_ROWS as i32).contains(&y) {
                continue;
            }
            for delta_x in -1..=1i32 {
                // Longitude wraps (tile 35's east neighbour is tile 0);
                // latitude does not (no tile sits beyond the poles).
                let x = (i32::from(center_x) + delta_x).rem_euclid(TILE_COLUMNS as i32) as i16;
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
    use crate::build::place_map::geometry::{world_viewbox_width, TileSelection, TILE_BLEED, TILE_K, VIEWBOX_HEIGHT};
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

    /// A tile carries the world layer's relief lighting and band shadows.
    /// Runtime rasterization bakes them into a fixed-pixel canvas before the
    /// camera transform, so WebKit does not resample filtered SVG content at
    /// the wrapper's smaller layout size. The coast halo stays out of tiles;
    /// land color and full band ladder remain identical to the world.
    #[test]
    fn a_tile_bakes_terrain_lighting_with_filter_bounds_for_its_own_height() {
        let context = PlaceMapContext::embedded().unwrap();
        // East Asian coast and a taller northern cell: both land and relief,
        // with different canvas heights that must not share a fixed filter.
        for (x, y) in [(29i16, 12i16), (29, 13)] {
            let svg = emit_tile_svg(&context, x, y);
            for required in ["feGaussianBlur", "feDiffuseLighting", "feDropShadow", "data-map-layer=\"lighting\""] {
                assert!(svg.contains(required), "tile ({x},{y}) lacks {required}");
            }
            assert!(!svg.contains("data-map-layer=\"coast\""), "tile ({x},{y}) should not carry the coast halo");
            assert!(!svg.contains("radialGradient id=\"moss-place-map-tile-"), "tile ({x},{y}) should omit the unused marker gradient");
            assert!(!svg.contains("-globe-clip\""), "tile ({x},{y}) should omit the unused globe clip");
            assert!(!svg.contains("-soft\" filterUnits=\"userSpaceOnUse\""), "tile ({x},{y}) should omit the unused coast halo filter");
            assert!(svg.contains("style=\"fill:") && svg.contains("filter=\"url(#moss-place-map-tile-"), "tile ({x},{y}) relief bands need their shadow");
            for layer in ["seafloor", "land", "relief", "lakes", "rivers"] {
                assert!(svg.contains(&format!("data-map-layer=\"{layer}\"")), "tile ({x},{y}) lost {layer}");
            }
            assert!(svg.contains("<use href=\"#moss-place-map-tile-"), "land fill is drawn");
            assert!(svg.contains("fill=\"var(--moss-place-land, "), "tile ({x},{y}) land is not --moss-place-land");
            let canvas_height = svg
                .split_once("<svg ").unwrap().1
                .split_once(" height=\"").unwrap().1
                .split_once('"').unwrap().0.parse::<f64>().unwrap();
            let expected_filter_height = format!("height=\"{}\"", ((canvas_height + 24.0) * 1000.0).round() / 1000.0);
            assert!(svg.matches(&expected_filter_height).count() >= 2, "tile ({x},{y}) shadow filters must cover its full canvas plus bleed");
        }
        assert!(emit_world_svg(&context).contains("feDiffuseLighting"), "the world layer keeps its filters");
    }

    /// A tile is drawn at a fractional size, and WebKit clips its stacked
    /// band polygons at the image edge one draw at a time, so the last
    /// partly covered row came out a few levels lighter than the tile (a
    /// light line along the bottom edge over open sea). Everything sits in
    /// one isolated group so the edge is clipped once.
    #[test]
    fn a_tile_draws_inside_one_isolated_group() {
        let context = PlaceMapContext::embedded().unwrap();
        let svg = emit_tile_svg(&context, 16, 13);
        let open = svg.find('>').unwrap() + 1;
        assert!(svg[open..].starts_with("<g style=\"isolation:isolate\">"), "{:.200}", &svg[open..]);
        assert!(svg.ends_with("</g></svg>"));
        assert!(!emit_world_svg(&context).contains("isolation:isolate"));
    }

    /// One integer coordinate in a tile is at most 0.01 degree (about 1 km),
    /// so a coast or lake is not rounded to the 0.1 degree staircase a
    /// 95-unit canvas gave. The serializer snaps to integers and simplifies
    /// at one unit, so this canvas width is the precision of the whole tile.
    /// The densest tiles stay well under the per-tile byte budget.
    #[test]
    fn a_tile_coordinate_unit_is_at_most_a_hundredth_of_a_degree_and_tiles_stay_small() {
        let (canvas_width, _) = Projection::new(&tile_frame(29, 12)).canvas_size();
        let degrees_per_unit = 10.0 / (canvas_width - 2.0 * TILE_BLEED * TILE_K);
        assert!(degrees_per_unit <= 0.01, "one unit is {degrees_per_unit} degrees");
        let context = PlaceMapContext::embedded().unwrap();
        for (x, y) in [(19i16, 13i16), (20, 12), (18, 13)] {
            let bytes = emit_tile_svg(&context, x, y).len();
            assert!(bytes < 150_000, "tile ({x},{y}) is {bytes} bytes");
        }
    }

    /// Two tiles overlap by their bleed, so one band must be one colour in
    /// every tile; tinted against only the bands a tile holds, the same
    /// 1000 band read differently in a lowland tile and a mountain one, and
    /// the strip two tiles share took whichever tile loaded last.
    #[test]
    fn a_band_has_one_tint_in_every_tile() {
        let context = PlaceMapContext::embedded().unwrap();
        // The Alps, the east Asian coast and the Atlantic off Iberia hold
        // bands up to different heights and depths.
        let mut tints: std::collections::HashMap<String, (String, usize)> = std::collections::HashMap::new();
        for (x, y) in [(18i16, 13i16), (29, 12), (16, 13), (17, 12)] {
            let svg = emit_tile_svg(&context, x, y);
            for part in svg.split("<g data-map-band=\"").skip(1) {
                let band = &part[..part.find('"').unwrap()];
                let tint_start = part.find("style=\"fill:").unwrap();
                let tint_end = part[tint_start..].find('"').unwrap();
                let tint = &part[tint_start..tint_start + tint_end];
                let entry = tints.entry(band.to_string()).or_insert((tint.to_string(), 0));
                assert_eq!(entry.0, tint, "band {band} is tinted differently in tile ({x},{y})");
                entry.1 += 1;
            }
        }
        let shared = |sign: i32| tints.iter().filter(|(band, (_, tiles))| *tiles > 1 && band.parse::<i32>().unwrap().signum() == sign).count();
        assert!(shared(1) > 0 && shared(-1) > 0, "the tiles share no relief or no sea-floor band, so this checks nothing");
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

    /// Where the runtime places a tile's local `(0, 0)`: the canvas origin
    /// `tiles.json` carries (`tile_origin_units`, whole canvas
    /// units), in world units.
    fn tile_anchor_world(x: i16, y: i16) -> (f64, f64) {
        let (left, top) = tile_origin_units(x, y);
        (left as f64 / TILE_K, top as f64 / TILE_K)
    }

    /// A tile's own viewBox is its cell's Patterson rectangle (in world
    /// units, from the full-extent projection), padded by `TILE_BLEED` on
    /// every edge and scaled by `TILE_K`, then grown outward to whole
    /// canvas units on the world's lattice: it starts at the origin
    /// `tiles.json` carries, covers the padded rectangle, and is less than
    /// two units larger than it in each direction. If `for_tile` stopped
    /// padding the box, this canvas would shrink back to the nominal cell
    /// and the assertions below would fail.
    ///
    /// The cell's width comes from `world_viewbox_width()` rather than from
    /// projecting its west/east edges: Patterson's x term is exactly linear
    /// in longitude, so every 10-degree cell is the same width, and a
    /// column at the antimeridian (`x == 35`, east edge at longitude 180)
    /// would otherwise collide with `ProjectedPoint::new`'s half-open
    /// normalisation (`longitude_normalisation_is_half_open`).
    #[test]
    fn tile_viewbox_covers_its_cell_padded_by_bleed_on_whole_units() {
        let full = Projection::new_world_full_extent();
        let padded_width = (world_viewbox_width() / 360.0 * 10.0 + 2.0 * TILE_BLEED) * TILE_K;
        for (x, y) in [(10i16, 9i16), (0, 0), (35, 17), (20, 14)] {
            let frame = tile_frame(x, y);
            let north = f64::from(y) * 10.0 - 90.0 + 10.0;
            let south = f64::from(y) * 10.0 - 90.0;
            let (_, min_y) = full.project(ProjectedPoint::new(0.0, north).unwrap()).unwrap();
            let (_, max_y) = full.project(ProjectedPoint::new(0.0, south).unwrap()).unwrap();
            let padded_height = (max_y - min_y + 2.0 * TILE_BLEED) * TILE_K;

            let (canvas_width, canvas_height) = Projection::new(&frame).canvas_size();
            assert_eq!(canvas_width.fract(), 0.0, "({x},{y}): canvas_width {canvas_width}");
            assert_eq!(canvas_height.fract(), 0.0, "({x},{y}): canvas_height {canvas_height}");
            assert!(
                canvas_width >= padded_width - 1e-6 && canvas_width < padded_width + 2.0,
                "({x},{y}): canvas_width {canvas_width}, padded {padded_width}"
            );
            assert!(
                canvas_height >= padded_height - 1e-6 && canvas_height < padded_height + 2.0,
                "({x},{y}): canvas_height {canvas_height}, padded {padded_height}"
            );
            let (_, top) = tile_origin_units(x, y);
            assert!(top as f64 <= (min_y - TILE_BLEED) * TILE_K + 1e-6, "({x},{y}): top {top}");
            assert!((top as f64) > (min_y - TILE_BLEED) * TILE_K - 1.0, "({x},{y}): top {top}");
        }
    }

    /// Two adjacent same-row tiles must place the SAME boundary-meridian
    /// point at the SAME world-space position once each one's own render is
    /// inverted by the runtime's own transform (`world = (cellOrigin -
    /// bleed) + tilePixel / K`, `translate(cellX - bleed, cellY - bleed)
    /// scale(1/K)` in `tiles.ts`'s `tileOverlayTransform`) — this is "meets
    /// at the shared edge" restated as the actual contract the runtime
    /// relies on, rather than a FlatProjection-specific pixel step that no
    /// longer applies now both tiles share the world's own Patterson
    /// projection. `TILE_BLEED` cancels out of this reconstruction by
    /// construction (padding the projection box symmetrically moves its own
    /// local origin by exactly `bleed`, and the runtime's anchor subtracts
    /// that same `bleed` back out) — it is the TILE's own rendered edge,
    /// not this reconstructed point, that moves; `tileOverlayTransform`'s
    /// own vitest suite covers that overlap directly.
    #[test]
    fn adjacent_tiles_meet_at_their_shared_edge_after_dividing_by_k() {
        for y in [0i16, 9, 16] {
            let (x_a, x_b) = (10i16, 11i16);
            let frame_a = tile_frame(x_a, y);
            let frame_b = tile_frame(x_b, y);
            let projection_a = Projection::new(&frame_a);
            let projection_b = Projection::new(&frame_b);
            let (origin_a_x, origin_a_y) = tile_anchor_world(x_a, y);
            let (origin_b_x, origin_b_y) = tile_anchor_world(x_b, y);

            let boundary_longitude = f64::from(x_b) * 10.0 - 180.0;
            let boundary_point = ProjectedPoint::new(boundary_longitude, frame_a.center_latitude).unwrap();
            let (px_a, py_a) = projection_a.project(boundary_point).unwrap();
            let (px_b, py_b) = projection_b.project(boundary_point).unwrap();
            let world_a = (origin_a_x + px_a / TILE_K, origin_a_y + py_a / TILE_K);
            let world_b = (origin_b_x + px_b / TILE_K, origin_b_y + py_b / TILE_K);

            assert!((world_a.0 - world_b.0).abs() < 0.5, "row {y}: x disagrees by {} world units", (world_a.0 - world_b.0).abs());
            assert!((world_a.1 - world_b.1).abs() < 0.5, "row {y}: y disagrees by {} world units", (world_a.1 - world_b.1).abs());
        }
    }

    /// A tile and the world itself must agree, to float precision, on where
    /// a known point lands once the tile's own render is inverted by the
    /// runtime's transform — not merely two neighbouring tiles agreeing
    /// with EACH OTHER (the test above), which alone could not catch both
    /// drifting the same way off the world's own projection.
    #[test]
    fn world_and_a_tile_agree_on_a_known_points_position_after_the_runtimes_transform() {
        let full = Projection::new_world_full_extent();
        for (x, y, longitude, latitude) in [(20i16, 12i16, 35.5, 33.89), (0, 0, -179.0, -89.0), (17, 9, 4.9, -0.1)] {
            let frame = tile_frame(x, y);
            let projection = Projection::new(&frame);
            let (origin_x, origin_y) = tile_anchor_world(x, y);
            let point = ProjectedPoint::new(longitude, latitude).unwrap();

            let (px, py) = projection.project(point).unwrap();
            let world = (origin_x + px / TILE_K, origin_y + py / TILE_K);
            let world_full = full.project(point).unwrap();
            assert!((world.0 - world_full.0).abs() < 1e-6, "({x},{y}) {longitude},{latitude}: x={}, expected {}", world.0, world_full.0);
            assert!((world.1 - world_full.1).abs() < 1e-6, "({x},{y}) {longitude},{latitude}: y={}, expected {}", world.1, world_full.1);
        }
    }

    /// Two tiles quantise their vertices to integers in their own canvas.
    /// A point in the overlap of two neighbours must land on the same
    /// integer once each tile's origin (a whole number of canvas units) is
    /// added back, or a shared coastline sits a pixel or two apart in the
    /// two tiles.
    #[test]
    fn a_point_in_the_overlap_of_two_tiles_quantises_to_one_global_coordinate() {
        let global = |x: i16, y: i16, longitude: f64, latitude: f64| {
            let frame = tile_frame(x, y);
            let projection = Projection::new(&frame);
            let (px, py) = projection.project(ProjectedPoint::new(longitude, latitude).unwrap()).unwrap();
            let (width, height) = projection.canvas_size();
            assert!((0.0..=width).contains(&px) && (0.0..=height).contains(&py), "({x},{y}) {longitude},{latitude}: ({px},{py}) is outside {width}x{height}");
            let (origin_x, origin_y) = tile_origin_units(x, y);
            (origin_x + px.round() as i64, origin_y + py.round() as i64)
        };
        // West and east neighbours: the shared meridian is -70, and a point
        // 0.05 degree to either side is inside both canvases' bleed.
        for latitude in [0.3f64, 25.3, 41.7, 66.1] {
            let y = ((latitude + 90.0) / 10.0).floor() as i16;
            for longitude in [-70.05, -69.97] {
                assert_eq!(global(10, y, longitude, latitude), global(11, y, longitude, latitude), "row {y}, {longitude},{latitude}");
            }
        }
        // North and south neighbours: the shared parallel is 10 degrees north.
        for longitude in [-100.3f64, -64.9, 12.2, 140.8] {
            let x = ((longitude + 180.0) / 10.0).floor() as i16;
            for latitude in [9.97, 10.05] {
                assert_eq!(global(x, 9, longitude, latitude), global(x, 10, longitude, latitude), "column {x}, {longitude},{latitude}");
            }
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
