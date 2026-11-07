//! A static place map drawn over a historical layer takes the shape of the part of the canvas the layer covers, so the layer fills the figure.

#[path = "support/site.rs"]
mod site;

use site::Site;
use std::fs;

const PLATE: &str = "tests/fixtures/snapshot-sites/map-layers-plate-site/input/maps/atlas-1930";
const PLACES: &str = "\n[terms.places]\ntype = \"place\"\nfields = [\"location\"]\ntitle = \"Places\"\n";
const LAYER: &str = "\n[map.layers.atlas-1930]\nkind = \"historical\"\nsource = \"maps/atlas-1930\"\nmax_zoom = 15\nbudget = \"2 MB\"\n";

fn site_over_the_plate(layer: bool) -> Site {
    let site = Site::with_config(
        PLACES,
        &[
            (".moss/places.toml", "[\"Churchyard Gate\"]\nlat = 40.7081\nlng = -74.0119\nprecision = \"exact\"\n"),
            ("index.md", "---\ntitle: Home\n---\n\nHi.\n"),
            ("posts/gate.md", "---\ndate: 2025-06-01\nuid: 2e90b4d1\nlocation: Churchyard Gate\n---\n\n# Gate\n\nA photograph.\n"),
        ],
    );
    if layer {
        let dir = site.root.join("maps/atlas-1930");
        fs::create_dir_all(&dir).unwrap();
        for entry in fs::read_dir(PLATE).unwrap() {
            let entry = entry.unwrap();
            fs::copy(entry.path(), dir.join(entry.file_name())).unwrap();
        }
        site.write(".moss/config.toml", &format!("schema_version = 6\n\n[site]\nlang = \"en\"\n{PLACES}{LAYER}"));
        site.rebuild();
    }
    site
}

/// The place map's figure, through the credit slot when it has one.
fn figure_of(page: &str) -> String {
    let start = page.find("<figure class=\"moss-place-map\"").expect("a place map");
    page[start..start + page[start..].find("</figure>").unwrap() + "</figure>".len()].to_string()
}

fn numbers(text: &str) -> Vec<f64> {
    text.split([' ', ',', '/']).filter_map(|n| n.trim().parse().ok()).collect()
}

fn attr<'a>(html: &'a str, tag: &str, name: &str) -> &'a str {
    let tag = &html[html.find(tag).unwrap_or_else(|| panic!("no {tag}"))..];
    let tag = &tag[..tag.find('>').unwrap()];
    let value = &tag[tag.find(&format!("{name}=\"")).unwrap_or_else(|| panic!("no {name} in {tag}")) + name.len() + 2..];
    &value[..value.find('"').unwrap()]
}

#[test]
fn a_map_over_a_layer_smaller_than_the_frame_takes_the_layers_shape() {
    let page = site_over_the_plate(true).html("places/churchyard-gate/index.html");
    let html = figure_of(&page);
    let view = numbers(attr(&html, "<svg", "viewBox"));
    let (x, y, width, height) = (view[0], view[1], view[2], view[3]);
    assert!(width < 720.0 * 0.9 || height < 480.0 * 0.9, "the figure should be cut to the layer, got {view:?}");

    assert!(html.contains(" data-map-cut"), "a cut figure is marked for the stylesheet");
    let ratio = attr(&html, "<figure", "style").trim_start_matches("--moss-place-map-ar:").parse::<f64>().unwrap();
    assert!((ratio - width / height).abs() < 0.01, "the figure's aspect {ratio} should follow its viewBox {view:?}");

    let clip = &html[html.find("<clipPath id=\"").unwrap()..];
    let points = numbers(attr(clip, "<polygon", "points"));
    let xs: Vec<f64> = points.iter().step_by(2).map(|x| x.clamp(0.0, 720.0)).collect();
    let ys: Vec<f64> = points.iter().skip(1).step_by(2).map(|y| y.clamp(0.0, 480.0)).collect();
    let (min_x, max_x) = (xs.iter().cloned().fold(f64::INFINITY, f64::min), xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
    let (min_y, max_y) = (ys.iter().cloned().fold(f64::INFINITY, f64::min), ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
    assert!((min_x - x).abs() < 1.0 && (max_x - (x + width)).abs() < 1.0, "the layer should span the figure's width: {min_x}..{max_x} in {view:?}");
    assert!((min_y - y).abs() < 1.0 && (max_y - (y + height)).abs() < 1.0, "the layer should span the figure's height: {min_y}..{max_y} in {view:?}");
}

#[test]
fn a_map_without_a_layer_keeps_the_fixed_frame() {
    let page = site_over_the_plate(false).html("places/churchyard-gate/index.html");
    let html = figure_of(&page);
    assert_eq!(attr(&html, "<svg", "viewBox"), "0 0 720 480");
    assert!(!html.contains("data-map-cut") && !html[..html.find("<svg").unwrap()].contains("style="), "no shape on a map without a layer");
}
