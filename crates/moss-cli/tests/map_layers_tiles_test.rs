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

    fn page(&self, name: &str, location: &str) {
        std::fs::write(self.dir.path().join(format!("{name}.md")), format!("---\ntitle: {name}\nlocation: {location}\n---\nbody\n")).unwrap();
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
fn the_notes() {
    // A used place outside the plate; an unused exact place outside the plate (a gazetteer entry no page names);
    // a plate whose control points disagree with their fit by over 30 m.
    let places = format!("{PLACES}\n[\"Far Place\"]\nlat = 40.9\nlng = -73.9\nprecision = \"exact\"\n\n[\"Unused Borough\"]\nlat = 40.95\nlng = -73.8\nprecision = \"exact\"\n");
    let site = Site::new(LAYER, &places);
    site.page("a", "Trinity Church");
    site.page("b", "Far Place");
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
    assert!(!out.log.contains("place 'Unused Borough'"), "a place no page names is not noted:\n{}", out.log);
    assert!(!out.log.contains("place 'Trinity Church' lies outside"), "{}", out.log);
    assert!(!out.log.contains("covers none of the site's places"), "the plate covers a used place:\n{}", out.log);
    assert!(out.log.contains(&format!("map plate {PLATE} fits its control points to within")), "{}", out.log);

    // Only a place outside the plate is used: the plate is kept, and the note says what its tiles cost.
    let idle = Site::new(LAYER, &places);
    idle.page("b", "Far Place");
    idle.plate("maps/atlas", |_, b| b);
    let out = idle.build(&["--strict"]);
    assert!(out.ok, "{}", out.log);
    assert!(out.log.contains(&format!("plate {PLATE} covers none of the site's places; it is kept, and its tiles cost ")), "{}", out.log);
    assert!(!idle.tiles("atlas").is_empty(), "the plate is still drawn");

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

const LOCATOR: &str = "[site]\nlocator = \"align-right\"\n";
const LOCATOR_PLACES: &str = "[\"Trinity Church\"]\nlat = 40.7081\nlng = -74.0119\nprecision = \"exact\"\n\n[\"Harbor Quarter\"]\nlat = 40.7060\nlng = -74.0125\nprecision = \"city\"\n\n[\"Far Place\"]\nlat = 40.9\nlng = -73.9\nprecision = \"exact\"\n";

impl Site {
    fn article(&self, name: &str) -> String {
        std::fs::read_to_string(self.dir.path().join(".moss/build.nosync/current").join(name).join("index.html")).unwrap()
    }

    fn locator_site(layers: &str) -> Site {
        let site = Site::new(&format!("{layers}{LOCATOR}"), LOCATOR_PLACES);
        site.page("exact", "Trinity Church");
        site.page("city", "Harbor Quarter");
        site.page("outside", "Far Place");
        site
    }
}

/// The `<figure>` of an article's locator.
fn locator(html: &str) -> &str {
    let start = html.find("moss-place-locator").expect("a locator");
    &html[start..start + html[start..].find("</figure>").expect("its end")]
}

/// `(width, height)` from the first start-of-frame marker of a baseline JPEG.
fn jpeg_size(bytes: &[u8]) -> (u32, u32) {
    let at = bytes.windows(2).position(|w| w == [0xFF, 0xC0]).expect("a start-of-frame marker");
    let field = |n: usize| u32::from(u16::from_be_bytes([bytes[at + n], bytes[at + n + 1]]));
    (field(7), field(5))
}

fn crop_hrefs(figure: &str) -> Vec<String> {
    figure.split("<image href=\"").skip(1).map(|rest| rest[..rest.find('"').unwrap()].to_string()).collect()
}

#[test]
fn an_exact_place_on_a_plate_gets_a_locator_framed_to_it_with_the_plate_clipped_in() {
    let site = Site::locator_site(LAYER);
    site.plate("maps/atlas", |_, b| b);
    let out = site.build(&[]);
    assert!(out.ok, "{}", out.log);

    let html = site.article("exact");
    let figure = locator(&html);
    let hrefs = crop_hrefs(figure);
    assert_eq!(hrefs.len(), 1, "{figure:.600}");
    assert!(hrefs[0].contains("/layers/atlas/crops/") && hrefs[0].ends_with(".jpg"), "{}", hrefs[0]);
    let written = site.dir.path().join(".moss/build.nosync/current").join(hrefs[0].trim_start_matches('/'));
    let bytes = std::fs::read(&written).unwrap_or_else(|_| panic!("{} was not written", written.display()));
    assert!(bytes.len() > 1000 && bytes.starts_with(&[0xFF, 0xD8]), "a JPEG");
    assert!(figure.contains("<clipPath") && figure.contains("<polygon points=\"") && figure.contains("clip-path=\"url(#"), "{figure:.800}");
    // The ground is the flat land tone and the world's own land and relief are not drawn.
    assert!(figure.contains("data-map-layer=\"ground\"") && figure.contains("data-map-layer=\"land\"/>") && figure.contains("data-map-layer=\"relief\"/>"), "{figure:.800}");
    assert!(figure.contains("aria-live=\"polite\">New York Public Library</p>") || html.contains("aria-live=\"polite\">New York Public Library</p>"), "the credit is in the static figure");
    // The picture is cut to the figure's size: 2x the locator's 352 px.
    assert_eq!(jpeg_size(&bytes), (704, 469));
}

#[test]
fn a_place_less_exact_than_that_keeps_its_regional_frame_and_a_place_off_the_plates_keeps_the_usual_one() {
    let site = Site::locator_site(LAYER);
    site.plate("maps/atlas", |_, b| b);
    assert!(site.build(&[]).ok);
    for name in ["city", "outside"] {
        let html = site.article(name);
        let figure = locator(&html);
        assert!(!figure.contains("<image") && !figure.contains("data-map-layer=\"ground\""), "{name}: {figure:.800}");
        assert!(!figure.contains("data-map-layer=\"land\"/>"), "{name} still draws the world's land");
        assert!(!html.contains("aria-live=\"polite\">New"), "{name}: no credit for a figure that draws no layer");
    }
}

#[test]
fn two_layers_are_drawn_in_declaration_order_each_clipped_to_its_own_plates() {
    let site = Site::locator_site(&format!("{LAYER}[map.layers.second]\nkind = \"historical\"\nsource = \"maps/second\"\nmax_zoom = 16\n"));
    site.plate("maps/atlas", |_, b| b);
    site.plate("maps/second", |_, b| b);
    let out = site.build(&[]);
    assert!(out.ok, "{}", out.log);
    let html = site.article("exact");
    let figure = locator(&html);
    let hrefs = crop_hrefs(figure);
    assert_eq!(hrefs.len(), 2, "{figure:.600}");
    assert!(hrefs[0].contains("/layers/atlas/") && hrefs[1].contains("/layers/second/"), "{hrefs:?}");
    assert_eq!(figure.matches("data-map-layer=\"detail\"").count(), 2);
}

#[test]
fn a_second_build_does_not_warp_a_crop_again() {
    let site = Site::locator_site(LAYER);
    site.plate("maps/atlas", |_, b| b);
    assert!(site.build(&[]).ok);
    let href = crop_hrefs(locator(&site.article("exact"))).remove(0);
    let marked = site.dir.path().join(".moss/build.nosync/current").join(href.trim_start_matches('/'));
    let staged = PathBuf::from(marked.to_string_lossy().replace("/current/", "/staging/"));
    assert!(staged.is_file(), "{}", staged.display());
    std::fs::write(&staged, b"previous build's crop").unwrap();
    assert!(site.build(&[]).ok);
    assert!(std::fs::read(&marked).unwrap() == b"previous build's crop", "the crop was warped again");
}

#[test]
fn crops_count_toward_the_layers_budget() {
    let build = |budget: Option<u64>| {
        let config = budget.map_or(LAYER.to_string(), |b| format!("{LAYER}budget = \"{b}\"\n"));
        let site = Site::locator_site(&config);
        site.plate("maps/atlas", |_, b| b);
        let out = site.build(&[]);
        assert!(out.ok, "{}", out.log);
        (site, out)
    };
    let (site, _) = build(None);
    let size = |paths: Vec<PathBuf>| paths.iter().map(|p| std::fs::metadata(p).unwrap().len()).sum::<u64>();
    let (crops, tiles): (Vec<_>, Vec<_>) = site.tiles("atlas").into_iter().partition(|p| p.to_string_lossy().contains("/crops/"));
    assert!(!crops.is_empty());
    let total = size(tiles) + size(crops);
    assert!(!build(Some(total)).1.log.contains("map layer atlas draws"), "exactly the tiles and the crops fits");
    assert!(build(Some(total - 1)).1.log.contains("map layer atlas draws"), "a byte less is over budget only if the crops count");
}

#[test]
fn a_place_on_a_plate_but_outside_its_fetched_image_keeps_the_usual_figure_and_warns() {
    let site = Site::locator_site(LAYER);
    // The fetched region is moved away from the places: the plate still lies under them, but no pixel of its image does.
    site.plate("maps/atlas", |name, b| if name.ends_with(".region.json") { String::from_utf8(b).unwrap().replace("3400", "0").into_bytes() } else { b });
    let out = site.build(&[]);
    assert!(out.ok, "{}", out.log);
    assert!(out.log.contains("outside the part of its image that was fetched"), "{}", out.log);
    let html = site.article("exact");
    let figure = locator(&html);
    assert!(!figure.contains("<image") && !figure.contains("data-map-layer=\"ground\""), "{figure:.800}");
    assert!(!figure.contains("data-map-layer=\"land\"/>"), "today's frame draws the world's land");
    assert!(!html.contains("aria-live=\"polite\">New"), "no credit");
}

fn published(site: &Site) -> Vec<String> {
    let current = site.dir.path().join(".moss/build.nosync/current");
    let mut found = Vec::new();
    let mut stack = vec![current.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(dir).unwrap().filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                found.push(path.strip_prefix(&current).unwrap().to_string_lossy().into_owned());
            }
        }
    }
    found.sort();
    found
}

#[test]
fn a_layer_source_folder_is_read_but_not_published() {
    let site = Site::new(LAYER, PLACES);
    site.plate("maps/atlas", |_, b| b);
    std::fs::write(site.dir.path().join("maps/atlas/notes.md"), "---\ntitle: Notes\n---\nplate notes\n").unwrap();
    let out = site.build(&[]);
    assert!(out.ok, "{}", out.log);
    let files = published(&site);
    assert!(files.iter().all(|f| !f.starts_with("maps/")), "source folder leaked: {files:?}");
    assert!(!files.iter().any(|f| f.contains(PLATE) && !f.starts_with("_moss/")), "{files:?}");
    assert!(!site.tiles("atlas").is_empty(), "the warper must still read the source");
    assert!(out.log.contains("notes.md") && out.log.contains("not published"), "{}", out.log);
}

#[test]
fn a_layer_source_that_is_the_whole_site_is_refused() {
    let site = Site::new("[map.layers.atlas]\nkind = \"historical\"\nsource = \".\"\nmax_zoom = 16\n", PLACES);
    let out = site.build(&[]);
    assert!(!out.ok && out.log.contains("is the whole site"), "{}", out.log);
}

#[test]
fn a_link_into_a_layer_source_is_reported_as_broken() {
    let site = Site::new(LAYER, PLACES);
    site.plate("maps/atlas", |_, b| b);
    std::fs::write(site.dir.path().join("about.md"), format!("---\ntitle: About\n---\n[the plate](maps/atlas/{PLATE}.region.json)\n")).unwrap();
    let out = site.build(&[]);
    assert!(out.log.contains(&format!("maps/atlas/{PLATE}.region.json")), "{}", out.log);
}

#[test]
fn an_archival_source_copy_still_holds_the_layer_source_data() {
    let site = Site::new(&format!("{LAYER}\n[site]\narchival = true\n"), PLACES);
    site.plate("maps/atlas", |_, b| b);
    let out = site.build(&[]);
    assert!(out.ok, "{}", out.log);
    let files = published(&site);
    for name in [format!("{PLATE}.annotation.json"), format!("{PLATE}.jpg"), format!("{PLATE}.region.json")] {
        let copy = format!("_moss/source/maps/atlas/{name}");
        assert!(files.contains(&copy), "{copy} missing from {files:?}");
    }
    assert!(!files.iter().any(|f| f.starts_with("maps/")), "{files:?}");
    let readme = std::fs::read_to_string(site.dir.path().join(".moss/build.nosync/current/_moss/source/README.md")).unwrap();
    assert!(readme.contains("## Map layer `maps/atlas`"), "{readme}");
    assert!(readme.contains(&format!("- `{PLATE}`: from ")), "{readme}");
}

fn notice_of(site: &Site) -> String {
    std::fs::read_to_string(site.map_dir().join("NOTICE.txt")).unwrap()
}

#[test]
fn the_notice_gives_each_plates_fit_and_who_georeferenced_it_when_stated() {
    let site = Site::new(LAYER, PLACES);
    site.plate("maps/atlas", |name, b| {
        if name.ends_with(".region.json") {
            String::from_utf8(b).unwrap().replacen("\"version\": 1,", "\"version\": 1,\n  \"georeferenced_by\": \"Example Mapping Project\",", 1).into_bytes()
        } else {
            b
        }
    });
    let out = site.build(&[]);
    assert!(out.ok, "{}", out.log);
    let notice = notice_of(&site);
    assert!(notice.contains("Fit: 3 m root mean square"), "{notice}");
    assert!(notice.contains("Georeferenced by: Example Mapping Project"), "{notice}");
}

#[test]
fn the_notice_names_no_georeferencer_when_none_is_given() {
    let site = Site::new(LAYER, PLACES);
    site.plate("maps/atlas", |_, b| b);
    let out = site.build(&[]);
    assert!(out.ok, "{}", out.log);
    let notice = notice_of(&site);
    assert!(notice.contains("Fit: 3 m root mean square") && !notice.contains("Georeferenced by"), "{notice}");
}
