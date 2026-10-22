use std::path::Path;

use serde_json::Value;

use super::fit::{self, mercator};
use super::plate::load_plates;
use super::tiles::{self, Loaded};
use crate::build::place_map::world_coords;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/layer-plate");

fn fixture_points() -> Vec<((f64, f64), (f64, f64))> {
    let annotation: Value = serde_json::from_slice(&std::fs::read(format!("{FIXTURE}/95abed283977b2cc.annotation.json")).unwrap()).unwrap();
    annotation["body"]["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            let px = f["properties"]["resourceCoords"].as_array().unwrap();
            let g = f["geometry"]["coordinates"].as_array().unwrap();
            ((px[0].as_f64().unwrap(), px[1].as_f64().unwrap()), (g[0].as_f64().unwrap(), g[1].as_f64().unwrap()))
        })
        .collect()
}

#[test]
fn the_real_plate_fits_and_round_trips_within_a_pixel() {
    let plates = load_plates(Path::new(FIXTURE));
    assert_eq!(plates.len(), 1);
    let plate = &plates[0];
    assert!(plate.rms_metres < 30.0, "rms {} m", plate.rms_metres);
    println!("real plate: residual rms {:.2} m", plate.rms_metres);

    let points = fixture_points();
    let pixels: Vec<_> = points.iter().map(|p| p.0).collect();
    let merc: Vec<_> = points.iter().map(|&(_, (lng, lat))| mercator(lat, lng)).collect();
    let forward = fit::fit(1, &pixels, &merc).unwrap();
    let mut worst = 0f64;
    for &((u, v), _) in &points {
        let (mx, my) = forward.eval(u, v);
        let (u2, v2) = plate.to_pixel.eval(mx, my);
        worst = worst.max((u2 - u).hypot(v2 - v));
    }
    // Measured in pixels of the image as fetched at a quarter of full size.
    let worst = worst * 0.25;
    println!("real plate: worst forward-then-inverse round trip {worst:.3} px at scale 0.25");
    assert!(worst < 1.0, "{worst}");
}

#[test]
fn the_footprint_contains_the_control_points() {
    let plate = load_plates(Path::new(FIXTURE)).remove(0);
    for ((_, (lng, lat)), n) in fixture_points().into_iter().zip(0..) {
        let (x, y) = world_coords::project(lat, lng);
        assert!(tiles::inside(&plate.footprint, x, y), "control point {n} at {lat},{lng} is outside the footprint");
    }
    assert_eq!(plate.footprint.first(), plate.footprint.last(), "the footprint ring is closed");
}

/// A 100x100 plate: blue, with a red block at pixels 20..30 and 20..30, whose
/// control points lie on an exact affine map of the ground. Only the left half
/// is inside the selector.
fn synthetic(dir: &Path) {
    let mut img = image::RgbImage::from_pixel(100, 100, image::Rgb([0, 0, 255]));
    for y in 20..30 {
        for x in 20..30 {
            img.put_pixel(x, y, image::Rgb([255, 0, 0]));
        }
    }
    img.save(dir.join("syn.jpg")).unwrap();
    let at = |u: f64, v: f64| (-74.010 + u * 0.00004, 40.708 - v * 0.00004);
    let features: Vec<Value> = [(0.0, 0.0), (100.0, 0.0), (0.0, 100.0), (100.0, 100.0), (30.0, 60.0)]
        .iter()
        .map(|&(u, v)| {
            let (lng, lat) = at(u, v);
            serde_json::json!({"type":"Feature","properties":{"resourceCoords":[u, v]},"geometry":{"type":"Point","coordinates":[lng, lat]}})
        })
        .collect();
    let annotation = serde_json::json!({
        "target": {"source": {"id": "https://example.org/iiif/syn", "width": 100, "height": 100},
                   "selector": {"type": "SvgSelector", "value": "<svg><polygon points=\"0,0 50,0 50,100 0,100\" /></svg>"}},
        "body": {"type": "FeatureCollection", "transformation": {"type": "polynomial", "options": {"order": 1}}, "features": features}
    });
    std::fs::write(dir.join("syn.annotation.json"), annotation.to_string()).unwrap();
    std::fs::write(
        dir.join("syn.region.json"),
        r#"{"version":1,"annotation":"syn.annotation.json","image":"syn.jpg","source":"https://example.org/iiif/syn","region":[0,0,100,100],"scale":1,"credit":"Example Library","rights":"","fetched":"2026-01-01"}"#,
    )
    .unwrap();
}

#[test]
fn an_affine_plate_is_recovered_and_a_known_colour_lands_at_the_expected_tile_pixel() {
    let dir = tempfile::tempdir().unwrap();
    synthetic(dir.path());
    let plates = load_plates(dir.path());
    assert_eq!(plates.len(), 1);
    let plate = &plates[0];
    assert!(plate.rms_metres < 0.5, "an affine plate fits to the control points: {}", plate.rms_metres);

    // The centre of the red block, pixel (25, 25), on the ground.
    let (lng, lat) = (-74.010 + 25.0 * 0.00004, 40.708 - 25.0 * 0.00004);
    let (u, v) = plate.to_pixel.eval(mercator(lat, lng).0, mercator(lat, lng).1);
    assert!((u - 25.0).abs() < 0.05 && (v - 25.0).abs() < 0.05, "({u}, {v})");

    let level = 19;
    let upp = tiles::units_per_pixel(level);
    let (x, y) = world_coords::project(lat, lng);
    let side = 256.0 * upp;
    let (i, j) = ((x / side) as u32, (y / side) as u32);
    let (px, py) = (((x - f64::from(i) * side) / upp) as u32, ((y - f64::from(j) * side) / upp) as u32);

    let loaded = [Loaded::open(plate).unwrap()];
    let tile = tiles::render_tile(&loaded, level, i, j).expect("the tile is drawn");
    assert_eq!(tile.get_pixel(px, py).0.map(|c| c / 32), [7, 0, 0], "red block at tile pixel ({px}, {py})");

    // Across the selector's right edge (u = 50) in one tile: blue inside, white outside.
    let level = 18;
    let (upp, side) = (tiles::units_per_pixel(level), 256.0 * tiles::units_per_pixel(level));
    let at = |u: f64, v: f64| world_coords::project(40.708 - v * 0.00004, -74.010 + u * 0.00004);
    let (ex, ey) = at(50.0, 70.0);
    let (ti, tj) = ((ex / side) as u32, (ey / side) as u32);
    let edge = tiles::render_tile(&loaded, level, ti, tj).unwrap();
    let pixel = |u: f64, v: f64| {
        let (x, y) = at(u, v);
        edge.get_pixel(((x - f64::from(ti) * side) / upp) as u32, ((y - f64::from(tj) * side) / upp) as u32).0
    };
    assert_eq!(pixel(44.0, 70.0).map(|c| c / 32), [0, 0, 7]);
    assert_eq!(pixel(56.0, 70.0), [255, 255, 255], "outside the selector is white");
    // A mirrored or rotated plate would put the red block elsewhere: just above or below it (plate rows 19 and 31) is blue.
    let (upp19, side19) = (tiles::units_per_pixel(19), 256.0 * tiles::units_per_pixel(19));
    let probes: Vec<[u8; 3]> = [19.0, 31.0]
        .iter()
        .map(|&v| world_coords::project(40.708 - v * 0.00004, -74.010 + 25.0 * 0.00004))
        .filter(|&(x, y)| (x / side19) as u32 == i && (y / side19) as u32 == j)
        .map(|(x, y)| tile.get_pixel(((x - f64::from(i) * side19) / upp19) as u32, ((y - f64::from(j) * side19) / upp19) as u32).0.map(|c| c / 32))
        .collect();
    assert!(!probes.is_empty() && probes.iter().all(|p| *p == [0, 0, 7]), "{probes:?}");
}

#[test]
fn levels_run_from_the_overview_down_to_the_zoom_asked_for() {
    let (min, max) = tiles::level_range([-74.018, 40.7046, -74.0089, 40.7094], 17);
    assert_eq!(max, 17);
    assert!((14..=17).contains(&min), "{min}");
}

#[test]
fn the_deepest_levels_keep_only_tiles_near_a_place() {
    let extent = [-74.018, 40.7046, -74.0089, 40.7094];
    let place = tiles::NearPlace::new(40.7081, -74.0119);
    let all = tiles::candidates(17, extent, false, &[]);
    let near = tiles::candidates(17, extent, true, &[place]);
    assert!(!near.is_empty() && near.len() < all.len(), "{} of {}", near.len(), all.len());
    assert!(tiles::candidates(17, extent, true, &[]).is_empty(), "no places, no deep tiles");
}
