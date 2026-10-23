use super::*;
use crate::config::ConfigFile;

fn layers(toml: &str) -> Result<Vec<MapLayer>, String> {
    ConfigFile::parse(toml).unwrap().map_layers()
}

const VALID: &str = "[map.layers.atlas-1930]\nkind = \"historical\"\nsource = \"maps/atlas-1930\"\nbbox = [-74.03, 40.68, -73.9, 40.88]\nmax_zoom = 17\nbudget = \"40 MB\"\ncredit = \"Atlas of the City, 1930\"\n";

#[test]
fn a_full_layer_parses() {
    let layers = layers(VALID).unwrap();
    assert_eq!(layers.len(), 1);
    let l = &layers[0];
    assert_eq!((l.name.as_str(), l.kind, l.source.as_str()), ("atlas-1930", LayerKind::Historical, "maps/atlas-1930"));
    assert_eq!(l.bbox, Some([-74.03, 40.68, -73.9, 40.88]));
    assert_eq!((l.max_zoom, l.budget), (17, Some(40_000_000)));
    assert_eq!(l.credit.as_deref(), Some("Atlas of the City, 1930"));
}

#[test]
fn a_minimal_layer_takes_the_defaults() {
    let l = &layers("[map.layers.a]\nkind = \"streets\"\nsource = \"s\"\n").unwrap()[0];
    assert_eq!((l.bbox, l.max_zoom, l.budget, l.credit.clone()), (None, 16, None, None));
}

#[test]
fn no_map_table_is_no_layers() {
    assert!(layers("[site]\nlang = \"en\"\n").unwrap().is_empty());
}

#[test]
fn layers_keep_the_order_the_file_writes() {
    let l = layers("[map.layers.zeta]\nkind = \"streets\"\nsource = \"z\"\n[map.layers.alpha]\nkind = \"streets\"\nsource = \"a\"\n").unwrap();
    assert_eq!(l.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["zeta", "alpha"]);
}

fn error_of(body: &str) -> String {
    layers(&format!("[map.layers.x]\n{body}")).unwrap_err()
}

#[test]
fn each_bad_key_is_an_error_naming_the_table_and_key() {
    for (body, needle) in [
        ("kind = \"satellite\"\nsource = \"s\"\n", "[map.layers.x] `kind`"),
        ("source = \"s\"\n", "[map.layers.x] needs a `kind`"),
        ("kind = \"streets\"\n", "[map.layers.x] needs a `source`"),
        ("kind = \"streets\"\nsource = \"s\"\nmax_zoom = 9\n", "[map.layers.x] `max_zoom`"),
        ("kind = \"streets\"\nsource = \"s\"\nmax_zoom = 20\n", "[map.layers.x] `max_zoom`"),
        ("kind = \"streets\"\nsource = \"s\"\nbbox = [1, 2, 3]\n", "[map.layers.x] `bbox`"),
        ("kind = \"streets\"\nsource = \"s\"\nbbox = [10, 0, 5, 1]\n", "[map.layers.x] `bbox`"),
        ("kind = \"streets\"\nsource = \"s\"\nbbox = [0, 0, 1, 91]\n", "[map.layers.x] `bbox`"),
        ("kind = \"streets\"\nsource = \"s\"\nbbox = [\"a\", 0, 1, 1]\n", "[map.layers.x] `bbox`"),
        ("kind = \"streets\"\nsource = \"s\"\nbudget = \"lots\"\n", "[map.layers.x] `budget`"),
        ("kind = \"streets\"\nsource = \"s\"\nmaxzoom = 12\n", "unknown key \"maxzoom\""),
    ] {
        let message = error_of(body);
        assert!(message.contains(needle), "{body:?} gave {message:?}");
    }
}

#[test]
fn byte_sizes_parse() {
    assert_eq!(parse_byte_size("40 MB"), Some(40_000_000));
    assert_eq!(parse_byte_size("500 KB"), Some(500_000));
    assert_eq!(parse_byte_size("1.5gb"), Some(1_500_000_000));
    assert_eq!(parse_byte_size("2 MiB"), Some(2_097_152));
    assert_eq!(parse_byte_size("2048"), Some(2048));
    assert_eq!(parse_byte_size("MB"), None);
    assert_eq!(parse_byte_size("4 parsecs"), None);
}

fn site_with(dirs: &[&str]) -> tempfile::TempDir {
    let site = tempfile::tempdir().unwrap();
    for d in dirs {
        std::fs::create_dir_all(site.path().join(d)).unwrap();
    }
    site
}

fn one(source: &str, bbox: bool) -> Vec<MapLayer> {
    let bbox = if bbox { "bbox = [0, 0, 1, 1]\n" } else { "" };
    layers(&format!("[map.layers.x]\nkind = \"historical\"\nsource = \"{source}\"\n{bbox}")).unwrap()
}

#[test]
fn a_source_folder_inside_the_site_resolves() {
    let site = site_with(&["maps/atlas"]);
    let resolved = resolve_layers(one("maps/atlas", true), site.path(), &Gazetteer::default()).unwrap();
    assert_eq!(resolved[0].bbox, Some([0.0, 0.0, 1.0, 1.0]));
}

#[test]
fn a_source_that_is_missing_outside_or_a_file_is_an_error() {
    let site = site_with(&["maps"]);
    std::fs::write(site.path().join("maps/file.txt"), "x").unwrap();
    for source in ["maps/gone", "../elsewhere", "/etc", "maps/file.txt"] {
        let message = resolve_layers(one(source, true), site.path(), &Gazetteer::default()).unwrap_err();
        assert!(message.starts_with("[map.layers.x] `source`"), "{source}: {message}");
    }
}

#[test]
fn a_source_symlinked_out_of_the_site_is_an_error() {
    let site = site_with(&["maps"]);
    let outside = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(outside.path(), site.path().join("maps/out")).unwrap();
        let message = resolve_layers(one("maps/out", true), site.path(), &Gazetteer::default()).unwrap_err();
        assert!(message.contains("inside the site"), "{message}");
    }
}

fn gazetteer(toml: &str) -> Gazetteer {
    crate::vault::places::parse_gazetteer(&toml::from_str(toml).unwrap())
}

#[test]
fn a_missing_bbox_comes_from_the_gazetteer_or_is_an_error() {
    let site = site_with(&["m"]);
    let places = gazetteer("[\"A\"]\nlat = 40.7\nlng = -74.0\n[\"B\"]\nlat = 40.8\nlng = -73.9\n");
    let resolved = resolve_layers(one("m", false), site.path(), &places).unwrap();
    assert_eq!(resolved[0].bbox, Some(frame_for(&[(40.7, -74.0), (40.8, -73.9)]).unwrap().bbox));
    let message = resolve_layers(one("m", false), site.path(), &Gazetteer::default()).unwrap_err();
    assert!(message.starts_with("[map.layers.x] has no `bbox`"), "{message}");
}

#[test]
fn the_frame_pads_ten_percent_and_never_less_than_a_hundredth_of_a_degree() {
    let f = frame_for(&[(40.0, -74.0), (41.0, -73.0)]).unwrap();
    assert_eq!(f.bbox, [-74.1, 39.9, -72.9, 41.1]);
    let one_place = frame_for(&[(40.7, -74.0)]).unwrap();
    assert_eq!(one_place.bbox, [-74.01, 40.69, -73.99, 40.71]);
    assert_eq!(one_place.max_zoom, 19);
}

#[test]
fn the_suggested_zoom_puts_the_longer_side_near_a_thousand_pixels_then_adds_three() {
    // Equator: 0.2 degrees across, 360 / (256 * 2^z) * 1024 = 0.2 at z = 12.8; +3 rounds to 16.
    let f = frame_for(&[(0.0, 0.0), (0.0, 0.1636)]).unwrap();
    assert_eq!(f.max_zoom, 16);
    let wide = frame_for(&[(0.0, 0.0), (0.0, 40.0)]).unwrap();
    assert_eq!(wide.max_zoom, 10);
    assert!(frame_for(&[]).is_none());
}

#[test]
fn the_frame_stays_on_the_globe() {
    let f = frame_for(&[(89.99, 179.99)]).unwrap();
    assert_eq!((f.bbox[2], f.bbox[3]), (180.0, 90.0));
}

#[test]
fn annotations_are_kept_in_order_and_must_be_a_list_of_text() {
    let l = &layers("[map.layers.a]\nkind = \"historical\"\nsource = \"s\"\nannotations = [\"https://example.org/maps/b\", \"https://example.org/maps/a\"]\n").unwrap()[0];
    assert_eq!(l.annotations, ["https://example.org/maps/b", "https://example.org/maps/a"]);
    assert!(error_of("kind = \"historical\"\nsource = \"s\"\nannotations = 3\n").contains("`annotations`"));
}

#[test]
fn a_layer_name_that_cannot_name_a_folder_is_refused() {
    let err = layers("[map.layers.\"a/b\"]\nkind = \"streets\"\nsource = \"s\"\n").unwrap_err();
    assert!(err.contains("[map.layers.a/b]") && err.contains("layer name"), "{err}");
}
