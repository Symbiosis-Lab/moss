//! `moss map bbox [<folder>] [--json]`: the frame a site's places need.
//!
//! Reads `.moss/config.toml`, `.moss/places.toml` and the pages' frontmatter,
//! and writes nothing. The places counted are the gazetteer entries that a
//! page names in a field of the site's place-typed kind.

use std::collections::BTreeSet;
use std::path::Path;

use crate::config::map_layers::{frame_for, Frame};

const USAGE: &str = "Usage: moss map bbox [<folder>] [--json]";

pub fn run(args: &[String]) -> i32 {
    let mut rest = args.iter();
    if rest.next().map(String::as_str) != Some("bbox") {
        eprintln!("{USAGE}");
        return 2;
    }
    let mut json = false;
    let mut folder = None;
    for arg in rest {
        if arg == "--json" && !json {
            json = true;
        } else if arg.starts_with('-') || folder.is_some() {
            eprintln!("{USAGE}");
            return 2;
        } else {
            folder = Some(arg);
        }
    }
    let start = folder.map_or_else(|| std::env::current_dir().unwrap_or_default(), crate::vault_root::resolve_input);
    let root = if folder.is_some() { Some(start.clone()) } else { crate::vault_root::VaultRoot::find_containing(&start).map(|r| r.path().to_path_buf()) };
    let Some(root) = root.filter(|r| r.is_dir()) else {
        eprintln!("Error: no moss site here; pass the site's folder");
        return 1;
    };
    match used_places(&root) {
        Ok(points) => match frame_for(&points) {
            Some(frame) => {
                println!("{}", render(&frame, points.len(), json));
                0
            }
            None => {
                eprintln!("Error: no page names a place that .moss/places.toml gives coordinates");
                1
            }
        },
        Err(message) => {
            eprintln!("Error: {message}");
            1
        }
    }
}

/// `west,south,east,north` and a TOML-ready `max_zoom` line, or the JSON object.
fn render(frame: &Frame, places: usize, json: bool) -> String {
    if json {
        return serde_json::json!({"bbox": frame.bbox, "max_zoom": frame.max_zoom, "places": places}).to_string();
    }
    let [w, s, e, n] = frame.bbox;
    format!("{w},{s},{e},{n}\nmax_zoom = {}", frame.max_zoom)
}

/// `(lat, lng)` of every gazetteer place with coordinates that a page names in
/// a field of the place-typed kind, one per place.
fn used_places(root: &Path) -> Result<Vec<(f64, f64)>, String> {
    let cfg = crate::build::site_config::read_project_config(&root.to_string_lossy())?;
    let fields: Vec<String> = crate::build::terms::term_kinds(&cfg, "en")
        .into_iter()
        .filter(|kind| kind.is_place)
        .flat_map(|kind| kind.fields)
        .collect();
    if fields.is_empty() {
        return Err("the site declares no place-typed kind; add [terms.places] with type = \"place\" and fields = [\"location\"] to .moss/config.toml".into());
    }
    let gazetteer = crate::vault::places::load_gazetteer(&crate::moss_paths::MossPaths::new(root).places());
    let mut used = BTreeSet::new();
    for page in super::doctor::markdown_files(root) {
        let Ok(text) = std::fs::read_to_string(&page) else { continue };
        let frontmatter = crate::build::scan::article_map::parse_frontmatter(&text);
        for field in &fields {
            let names = frontmatter
                .get(field)
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .and_then(|v| moss_core::frontmatter_union::normalize_name_list(&v).ok())
                .unwrap_or_default();
            used.extend(names.iter().filter_map(|name| gazetteer.find(name)).map(|(display, _)| display.clone()));
        }
    }
    Ok(used.iter().filter_map(|display| gazetteer.get(display)?.coords).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site() -> tempfile::TempDir {
        let site = tempfile::tempdir().unwrap();
        let w = |path: &str, text: &str| {
            let path = site.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        w(".moss/config.toml", "schema_version = 6\n\n[terms.places]\ntype = \"place\"\nfields = [\"location\"]\n");
        w(".moss/places.toml", "[\"Harbor\"]\nlat = 40.7\nlng = -74.0\n[\"Quay\"]\nlat = 40.8\nlng = -73.9\n[\"Unused Cape\"]\nlat = -33.0\nlng = 151.0\n");
        w("index.md", "---\ntitle: Home\n---\nhi\n");
        w("posts/a.md", "---\nlocation: Harbor\n---\na\n");
        w("posts/b.md", "---\nlocation:\n  - quay\n  - Nowhere\n---\nb\n");
        site
    }

    fn listing(root: &Path) -> Vec<(String, Vec<u8>)> {
        let mut all: Vec<_> = walkdir::WalkDir::new(root)
            .into_iter()
            .filter_map(Result::ok)
            .map(|e| (e.path().strip_prefix(root).unwrap().to_string_lossy().into_owned(), if e.file_type().is_file() { std::fs::read(e.path()).unwrap() } else { Vec::new() }))
            .collect();
        all.sort();
        all
    }

    #[test]
    fn only_the_places_pages_use_set_the_frame() {
        let site = site();
        let points = used_places(site.path()).unwrap();
        assert_eq!(points.len(), 2, "{points:?}");
        let frame = frame_for(&points).unwrap();
        assert_eq!(frame.bbox, [-74.01, 40.69, -73.89, 40.81]);
    }

    #[test]
    fn the_frame_prints_as_text_and_as_json() {
        let frame = Frame { bbox: [-74.01, 40.69, -73.89, 40.81], max_zoom: 16 };
        assert_eq!(render(&frame, 2, false), "-74.01,40.69,-73.89,40.81\nmax_zoom = 16");
        assert_eq!(render(&frame, 2, true), r#"{"bbox":[-74.01,40.69,-73.89,40.81],"max_zoom":16,"places":2}"#);
    }

    #[test]
    fn the_command_prints_the_frame_and_changes_nothing_in_the_folder() {
        let site = site();
        let before = listing(site.path());
        let folder = site.path().to_string_lossy().into_owned();
        for extra in [vec![], vec!["--json".to_string()]] {
            let mut args = vec!["bbox".to_string(), folder.clone()];
            args.extend(extra);
            assert_eq!(run(&args), 0);
        }
        assert_eq!(listing(site.path()), before);
    }

    #[test]
    fn a_site_with_no_used_place_or_no_place_kind_is_an_error() {
        let site = site();
        std::fs::write(site.path().join("posts/a.md"), "---\n---\na\n").unwrap();
        std::fs::write(site.path().join("posts/b.md"), "---\n---\nb\n").unwrap();
        assert_eq!(run(&["bbox".into(), site.path().to_string_lossy().into_owned()]), 1);
        std::fs::write(site.path().join(".moss/config.toml"), "schema_version = 6\n").unwrap();
        assert!(used_places(site.path()).unwrap_err().contains("no place-typed kind"));
    }

    #[test]
    fn bad_arguments_are_a_usage_error() {
        assert_eq!(run(&[]), 2);
        assert_eq!(run(&["frame".into()]), 2);
        assert_eq!(run(&["bbox".into(), "--wat".into()]), 2);
    }
}
