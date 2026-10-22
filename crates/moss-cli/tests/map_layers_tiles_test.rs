//! A historical layer's folder of plate files becomes raster tiles, an index,
//! credits and notes; budgets, incremental builds and hand-written folders.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const PLATE: &str = "95abed283977b2cc";
const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../moss-build/tests/fixtures/layer-plate");

struct Site {
    dir: tempfile::TempDir,
    home: tempfile::TempDir,
}

struct Out {
    ok: bool,
    log: String,
}

impl Site {
    fn new(layer_config: &str, places: &str) -> Site {
        let site = Site { dir: tempfile::tempdir().unwrap(), home: tempfile::tempdir().unwrap() };
        let root = site.dir.path();
        std::fs::create_dir_all(root.join(".moss")).unwrap();
        std::fs::write(root.join(".moss/config.toml"), format!("schema_version = 6\n\n[terms.places]\ntype = \"place\"\nfields = [\"location\"]\n\n{layer_config}")).unwrap();
        std::fs::write(root.join(".moss/places.toml"), places).unwrap();
        std::fs::write(root.join("index.md"), "---\ntitle: Home\nhome: true\n---\nhi\n").unwrap();
        site
    }

    /// Copies the fixture plate's three files into `folder`, then lets `edit` change one of them.
    fn plate(&self, folder: &str, edit: impl Fn(&str, Vec<u8>) -> Vec<u8>) {
        let dest = self.dir.path().join(folder);
        std::fs::create_dir_all(&dest).unwrap();
        for name in [format!("{PLATE}.annotation.json"), format!("{PLATE}.jpg"), format!("{PLATE}.region.json")] {
            let bytes = std::fs::read(Path::new(FIXTURE).join(&name)).unwrap();
            std::fs::write(dest.join(&name), edit(&name, bytes)).unwrap();
        }
    }

    fn build(&self, extra: &[&str]) -> Out {
        let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
            .arg("build")
            .arg(std::fs::canonicalize(self.dir.path()).unwrap())
            .arg("--no-plugins")
            .args(extra)
            .env("MOSS_HOME", self.home.path())
            .stdin(Stdio::null())
            .output()
            .expect("run moss-cli build");
        Out { ok: out.status.success(), log: format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)) }
    }

    fn map_dir(&self) -> PathBuf {
        let current = self.dir.path().join(".moss/build.nosync/current/_moss");
        std::fs::read_dir(&current).unwrap().filter_map(Result::ok).map(|e| e.path()).find(|p| p.join("layers.json").is_file()).expect("a map directory with layers.json")
    }

    fn tiles(&self, layer: &str) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let mut stack = vec![self.map_dir().join("layers").join(layer)];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(dir).unwrap().filter_map(Result::ok) {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "jpg") {
                    found.push(path);
                }
            }
        }
        found.sort();
        found
    }
}

const PLACES: &str = "[\"Trinity Church\"]\nlat = 40.7081\nlng = -74.0119\nprecision = \"exact\"\n\n[\"Bowling Green\"]\nlat = 40.7048\nlng = -74.0140\nprecision = \"exact\"\n";
const LAYER: &str = "[map.layers.atlas]\nkind = \"historical\"\nsource = \"maps/atlas\"\nmax_zoom = 16\n";

#[test]
fn a_plate_folder_builds_tiles_an_index_and_a_credit() {
    let site = Site::new(LAYER, PLACES);
    site.plate("maps/atlas", |_, b| b);
    let out = site.build(&[]);
    assert!(out.ok, "{}", out.log);

    let dir = site.map_dir();
    let layers: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("layers.json")).unwrap()).unwrap();
    let layer = &layers["layers"][0];
    assert_eq!(layer["tiles"], "layers/atlas/index.json");
    assert_eq!(layer["credit"], "New York Public Library");
    let index: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("layers/atlas/index.json")).unwrap()).unwrap();
    assert_eq!(index["tile_size"], 256);

    // The index lists exactly the tiles written.
    let mut listed: Vec<String> = index["levels"]
        .as_object()
        .unwrap()
        .iter()
        .flat_map(|(level, tiles)| tiles.as_array().unwrap().iter().map(move |t| format!("{level}/{}/{}.jpg", t[0], t[1])))
        .collect();
    let mut written: Vec<String> = site.tiles("atlas").iter().map(|p| p.strip_prefix(dir.join("layers/atlas")).unwrap().to_string_lossy().into_owned()).collect();
    listed.sort();
    written.sort();
    assert!(!written.is_empty());
    assert_eq!(listed, written);
    assert_eq!(index["footprints"][0]["plate"], PLATE);
    let notice = std::fs::read_to_string(dir.join("NOTICE.txt")).unwrap();
    assert!(notice.contains("atlas") && notice.contains("https://iiif.nypl.org/iiif/3/5222333"), "{notice}");
}

#[test]
fn a_folder_made_the_way_the_recipe_makes_it_builds() {
    // What the docs have an author do by hand: the annotation as fetched, the image
    // as served, and a region.json written from them.
    let site = Site::new(
        "[map.layers.full]\nkind = \"historical\"\nsource = \"maps/full\"\nmax_zoom = 15\n[map.layers.bare]\nkind = \"historical\"\nsource = \"maps/bare\"\nmax_zoom = 15\n",
        PLACES,
    );
    site.plate("maps/full", |name, b| {
        if name.ends_with(".region.json") {
            br#"{
  "version": 1,
  "annotation": "95abed283977b2cc.annotation.json",
  "image": "95abed283977b2cc.jpg",
  "source": "https://iiif.nypl.org/iiif/3/5222333",
  "region": [3400, 1700, 4000, 4200],
  "scale": 0.125,
  "credit": "Example Public Library",
  "rights": "No known copyright restrictions",
  "fetched": "2026-10-22"
}"#
            .to_vec()
        } else {
            b
        }
    });
    site.plate("maps/bare", |name, b| {
        if name.ends_with(".region.json") {
            br#"{"region": [3400, 1700, 4000, 4200], "scale": 0.125}"#.to_vec()
        } else {
            b
        }
    });
    let out = site.build(&["--strict"]);
    assert!(out.ok, "{}", out.log);
    let layers: serde_json::Value = serde_json::from_slice(&std::fs::read(site.map_dir().join("layers.json")).unwrap()).unwrap();
    assert_eq!(layers["layers"][0]["credit"], "Example Public Library");
    assert_eq!(layers["layers"][1]["credit"], "New York Public Library", "no credit in region.json: the annotation's provider");
    assert!(!site.tiles("full").is_empty() && !site.tiles("bare").is_empty());
    let notice = std::fs::read_to_string(site.map_dir().join("NOTICE.txt")).unwrap();
    assert!(notice.contains("No known copyright restrictions"), "{notice}");
}

#[test]
fn over_budget_is_a_problem_that_fails_strict_and_names_the_layer_size_and_budget() {
    let site = Site::new(&format!("{LAYER}budget = \"1 KB\"\n"), PLACES);
    site.plate("maps/atlas", |_, b| b);
    let lax = site.build(&[]);
    assert!(lax.ok, "{}", lax.log);
    assert!(lax.log.contains("map layer atlas draws") && lax.log.contains("budget of 1 KB"), "{}", lax.log);
    let strict = site.build(&["--strict"]);
    assert!(!strict.ok, "{}", strict.log);
}

#[test]
fn within_budget_is_quiet() {
    let site = Site::new(&format!("{LAYER}budget = \"40 MB\"\n"), PLACES);
    site.plate("maps/atlas", |_, b| b);
    let out = site.build(&["--strict"]);
    assert!(out.ok && !out.log.contains("over its budget"), "{}", out.log);
}

#[test]
fn a_second_build_does_not_draw_a_tile_again() {
    let site = Site::new(LAYER, PLACES);
    site.plate("maps/atlas", |_, b| b);
    assert!(site.build(&[]).ok);
    let before = site.tiles("atlas");
    assert!(!before.is_empty());
    // Mark one tile where the previous build left it for the next: a build that draws it again overwrites the mark.
    let marked = before[0].clone();
    let honest = std::fs::read(&marked).unwrap();
    let staged = PathBuf::from(marked.to_string_lossy().replace("/current/", "/staging/"));
    assert!(staged.is_file(), "{}", staged.display());
    std::fs::write(&staged, b"previous build's tile").unwrap();
    assert!(site.build(&[]).ok);
    assert!(std::fs::read(&marked).unwrap() == b"previous build's tile", "the tile was drawn again");
    assert_eq!(site.tiles("atlas"), before);

    // Changing a plate changes what the tiles depend on: everything is drawn again.
    std::fs::write(site.dir.path().join("maps/atlas").join(format!("{PLATE}.region.json")), std::fs::read_to_string(Path::new(FIXTURE).join(format!("{PLATE}.region.json"))).unwrap().replace("\"scale\": 0.125", "\"scale\": 0.125, \"rights\": \"x\"")).unwrap();
    assert!(site.build(&[]).ok);
    let after = site.tiles("atlas");
    assert!(!after.is_empty());
    assert!(std::fs::read(&after[0]).unwrap().len() > 1000 && honest.len() > 1000);
}

#[test]
fn the_three_notes() {
    // A place outside the plate; a plate whose control points disagree with their fit by over 30 m;
    // a plate outside the layer's bbox.
    let places = format!("{PLACES}\n[\"Far Place\"]\nlat = 40.9\nlng = -73.9\nprecision = \"exact\"\n");
    let site = Site::new(LAYER, &places);
    site.plate("maps/atlas", |name, b| {
        if name.ends_with(".annotation.json") {
            String::from_utf8(b).unwrap().replace("40.705560678246314", "40.707560678246314").into_bytes()
        } else {
            b
        }
    });
    let out = site.build(&["--strict"]);
    assert!(out.ok, "notes never fail --strict:\n{}", out.log);
    assert!(out.log.contains("place 'Far Place' lies outside every historical map plate"), "{}", out.log);
    assert!(!out.log.contains("place 'Trinity Church' lies outside"), "{}", out.log);
    assert!(out.log.contains(&format!("map plate {PLATE} fits its control points to within")), "{}", out.log);

    let apart = Site::new("[map.layers.atlas]\nkind = \"historical\"\nsource = \"maps/atlas\"\nbbox = [10.0, 10.0, 10.5, 10.5]\n", PLACES);
    apart.plate("maps/atlas", |_, b| b);
    let out = apart.build(&["--strict"]);
    assert!(out.ok, "{}", out.log);
    assert!(out.log.contains(&format!("map plate {PLATE} lies outside the bbox of layer atlas")), "{}", out.log);
}

#[test]
fn a_broken_plate_is_a_warning_naming_it_and_the_good_plate_still_draws() {
    let site = Site::new(LAYER, PLACES);
    site.plate("maps/atlas", |_, b| b);
    std::fs::write(site.dir.path().join("maps/atlas/broken.annotation.json"), "{ not json").unwrap();
    let out = site.build(&[]);
    assert!(out.ok, "{}", out.log);
    assert!(out.log.contains("map plate broken skipped"), "{}", out.log);
    assert!(!site.tiles("atlas").is_empty());
}
