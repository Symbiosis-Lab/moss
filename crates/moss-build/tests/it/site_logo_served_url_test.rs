//! A `logo:` names a source file; the header must ask for the address the
//! build writes that file to. Folder segments are slugged on output
//! (`Pictures/` is served as `pictures/`), so a logo URL built from the
//! written path 404s on a case-sensitive host while a case-insensitive disk
//! serves it locally.

use moss_build::moss_paths::MossPaths;
use std::fs;
use std::path::{Path, PathBuf};

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Whether `url` names a file under `root`, comparing every segment's case
/// exactly, as a case-sensitive host would (`Path::exists` would not on macOS).
fn served_exactly(root: &Path, url: &str) -> bool {
    let mut dir = root.to_path_buf();
    for seg in url.trim_start_matches('/').split('/') {
        let found = fs::read_dir(&dir).ok().into_iter().flatten().flatten().any(|e| e.file_name() == seg);
        if !found {
            return false;
        }
        dir.push(seg);
    }
    dir.is_file()
}

fn logo_img(html: &str) -> String {
    let start = html.find(r#"<img class="site-logo""#).unwrap_or_else(|| panic!("no site logo in {html}"));
    html[start..].split_once('>').unwrap().0.to_string()
}

/// Builds a site whose home names `logo` and holds `Pictures/Mark.png`, and
/// returns the logo `<img>` of the home and of a page in another folder.
fn logo_imgs(logo: &str) -> (PathBuf, Cleanup, Vec<(String, String)>) {
    let tmp = std::env::temp_dir().join(format!("moss_logo_url_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let cleanup = Cleanup(tmp.clone());
    write(&tmp, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n");
    fs::create_dir_all(tmp.join("Pictures")).unwrap();
    image::RgbaImage::new(24, 8).save(tmp.join("Pictures/Mark.png")).unwrap();
    write(&tmp, "index.md", &format!("---\ntitle: Harbor Notes\nlogo: {logo}\n---\n\nWelcome.\n"));
    write(&tmp, "Notes/first.md", "---\ntitle: First Note\n---\n\nHello.\n");
    let result = crate::site::build_full(&tmp.to_string_lossy());
    assert!(result.is_ok(), "build failed: {result:?}");
    let out = MossPaths::new(&tmp).staging_dir();
    let imgs = ["index.html", "notes/first/index.html"]
        .iter()
        .map(|page| (page.to_string(), logo_img(&fs::read_to_string(out.join(page)).unwrap())))
        .collect();
    (out, cleanup, imgs)
}

fn src(img: &str) -> &str {
    img.split(r#"src=""#).nth(1).and_then(|s| s.split('"').next()).unwrap()
}

#[test]
fn a_logo_in_a_capitalised_folder_is_served_from_the_folders_slug_with_its_size() {
    for logo in ["Pictures/Mark.png", "./Pictures/Mark.png"] {
        let (out, _cleanup, imgs) = logo_imgs(logo);
        for (page, img) in imgs {
            assert!(served_exactly(&out, src(&img)), "{logo} on {page}: the logo asks for {}, which the build did not write: {img}", src(&img));
            assert!(img.contains(r#"width="24" height="8""#), "{logo} on {page}: the logo lost its size: {img}");
        }
    }
}

#[test]
fn an_external_logo_stays_external_when_a_local_file_shares_its_name() {
    let (_out, _cleanup, imgs) = logo_imgs("https://cdn.example.org/Mark.png");
    for (page, img) in imgs {
        assert_eq!(src(&img), "https://cdn.example.org/Mark.png", "{page}: {img}");
    }
}
