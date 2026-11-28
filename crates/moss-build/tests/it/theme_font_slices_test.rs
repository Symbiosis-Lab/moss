//! A Han font a theme hosts whole is published as slices, each declared with
//! its own `unicode-range`, so a page fetches only the slices its text uses.

use moss_build::moss_paths::MossPaths;
use std::fs;
use std::path::PathBuf;

const HAN: &[u8] = include_bytes!("../fixtures/fonts/han-boxes.ttf");

struct Site(PathBuf);

impl Site {
    fn new(lang: &str) -> Self {
        let site = Site(std::env::temp_dir().join(format!("moss_theme_font_slices_{}", uuid::Uuid::new_v4())));
        site.write(".moss/config.toml", format!("[site]\ndomain = \"example.com\"\nlang = \"{lang}\"\n").as_bytes());
        site.write("index.md", "---\ntitle: 首頁\n---\n\n一丁七。\n".as_bytes());
        site.write(".moss/theme/fonts/Han.ttf", HAN);
        site.write(".moss/theme/style.css", b"@font-face{font-family:Han;font-weight:400;src:url(fonts/Han.ttf) format(\"truetype\")}\nbody{font-family:Han,serif}\n");
        site
    }

    fn write(&self, rel: &str, body: &[u8]) {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    /// The published theme stylesheet and the slice files it names, read from the output.
    fn build(&self) -> (String, Vec<(String, Vec<u8>)>) {
        let result = crate::site::build_full(&self.0.to_string_lossy());
        assert!(result.is_ok(), "build failed: {result:?}");
        let theme = MossPaths::new(&self.0).staging_dir().join("_moss/theme");
        let name = fs::read_dir(&theme).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).find(|n| n.starts_with("style.") && n.ends_with(".css")).expect("the theme stylesheet is published");
        let css = fs::read_to_string(theme.join(name)).unwrap();
        let slices = css.split("url(\"").skip(1).map(|rest| {
            let url = rest.split('"').next().unwrap().to_string();
            let bytes = fs::read(theme.join(&url)).unwrap_or_default();
            (url, bytes)
        }).collect();
        (css, slices)
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_whole_han_font_is_published_as_woff2_slices_each_with_its_range() {
    let (css, slices) = Site::new("zh-Hant").build();
    assert!(!css.contains("Han.ttf"), "the whole font is no longer loaded: {css}");
    assert!(slices.len() > 2, "{css}");
    assert_eq!(css.matches("unicode-range:").count(), slices.len());
    for (url, bytes) in &slices {
        assert!(url.starts_with("fonts/Han.") && url.ends_with(".woff2"), "{url}");
        assert_eq!(bytes.get(..4), Some(&b"wOF2"[..]), "{url} is written after paint as WOFF2");
    }
}

#[test]
fn the_site_language_picks_the_slice_table() {
    let (traditional, _) = Site::new("zh-Hant").build();
    let (japanese, _) = Site::new("ja").build();
    assert_ne!(traditional, japanese);
}
