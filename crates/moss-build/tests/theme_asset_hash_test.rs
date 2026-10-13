//! The files a theme stylesheet points at are published under content-hashed
//! names, and the stylesheet's own name follows what it points at.

use moss_build::build::assets::paths::compute_binary_hash;
use std::fs;
use std::path::{Path, PathBuf};

fn build_sync(folder_path: &str) -> Result<String, String> {
    use moss_build::build::{run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
    use moss_build::cli::host::cli_host_ports;
    use moss_build::vault_root::VaultRoot;

    let source = PathBuf::from(folder_path);
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(run_pipeline(PipelineConfig {
        root: VaultRoot::resolve(&source),
        progress: moss_build::build::stdout_sink(),
        plugins: PluginMode::Skip,
        watch: false,
        start_server: false,
        host: cli_host_ports(folder_path),
        trigger: BuildTrigger::Full,
        exits_after_build: true,
        site_url_override: None,
        server_port: None,
        admission_epoch: None,
        live_port: None,
    }))
}

struct Site(PathBuf);

impl Site {
    fn new(font: &[u8], css: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("moss_theme_asset_hash_{}", uuid::Uuid::new_v4()));
        let site = Site(dir);
        site.write(".moss/config.toml", b"[site]\ndomain = \"example.com\"\n");
        site.write("index.md", b"---\ntitle: Home\n---\n\nBody.\n");
        site.write(".moss/theme/fonts/a.woff2", font);
        site.write(".moss/theme/img/grain.png", b"GRAIN");
        site.write(".moss/theme/style.css", css.as_bytes());
        site
    }

    fn write(&self, rel: &str, body: &[u8]) {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn build(&self) -> PathBuf {
        let result = build_sync(&self.0.to_string_lossy());
        assert!(result.is_ok(), "build failed: {result:?}");
        self.0.join(".moss/build.nosync/staging")
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const CSS: &str = "@font-face{font-family:T;src:url(\"fonts/a.woff2\") format(\"woff2\")}\nbody{background:url(img/grain.png)}\n";

/// The name of `_moss/theme/style.<hash>.css` in `out`.
fn theme_css_name(out: &Path) -> String {
    fs::read_dir(out.join("_moss/theme"))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|n| n.starts_with("style.") && n.ends_with(".css"))
        .expect("the build writes _moss/theme/style.<hash>.css")
}

#[test]
fn a_theme_stylesheet_points_at_hashed_copies_of_its_fonts_and_images() {
    let site = Site::new(b"FONT-ONE", CSS);
    let out = site.build();
    let name = theme_css_name(&out);
    let css = fs::read_to_string(out.join("_moss/theme").join(&name)).unwrap();

    let font = format!("fonts/a.{}.woff2", compute_binary_hash(b"FONT-ONE"));
    let img = format!("img/grain.{}.png", compute_binary_hash(b"GRAIN"));
    assert!(css.contains(&format!("url(\"{font}\")")), "{css}");
    assert!(css.contains(&format!("url({img})")), "{css}");
    assert_eq!(fs::read(out.join("_moss/theme").join(&font)).unwrap(), b"FONT-ONE");
    assert_eq!(fs::read(out.join("_moss/theme").join(&img)).unwrap(), b"GRAIN");

    // The plain addresses still work for pages and authored HTML.
    assert_eq!(fs::read(out.join("_moss/theme/fonts/a.woff2")).unwrap(), b"FONT-ONE");
    assert_eq!(fs::read(out.join("_moss/theme/img/grain.png")).unwrap(), b"GRAIN");

    // The stylesheet is named for the bytes it now contains.
    assert_eq!(name, format!("style.{}.css", compute_binary_hash(css.as_bytes())));
    let home = fs::read_to_string(out.join("index.html")).unwrap();
    assert!(home.contains(&format!("/_moss/theme/{name}")), "home links the stylesheet:\n{home}");
}

#[test]
fn changing_a_font_changes_the_stylesheets_address() {
    let one = theme_css_name(&Site::new(b"FONT-ONE", CSS).build());
    let two = theme_css_name(&Site::new(b"FONT-TWO", CSS).build());
    assert_ne!(one, two);
}

/// A theme with nothing to rewrite keeps the address it had before this existed.
#[test]
fn a_stylesheet_without_theme_urls_keeps_its_old_address() {
    let css = "body{color:red;background:url(data:image/gif;base64,R0lGOD)}\n@font-face{src:url(/abs/x.woff2)}\n";
    let site = Site::new(b"F", css);
    let out = site.build();
    let name = theme_css_name(&out);
    assert_eq!(name, format!("style.{}.css", compute_binary_hash(css.as_bytes())));
    assert_eq!(fs::read_to_string(out.join("_moss/theme").join(&name)).unwrap(), css);
}

/// The warm build: an edited font renames the stylesheet and its hashed copy,
/// the old hashed files do not pile up, and an unchanged rebuild keeps what it
/// emitted (the hashed copies are written outside the asset walk).
#[test]
fn an_edited_font_replaces_the_hashed_files_and_an_unchanged_rebuild_keeps_them() {
    let site = Site::new(b"FONT-ONE", CSS);
    let out = site.build();
    let old_sheet = theme_css_name(&out);
    let old_font = format!("fonts/a.{}.woff2", compute_binary_hash(b"FONT-ONE"));
    assert!(out.join("_moss/theme").join(&old_font).exists());

    site.write(".moss/theme/fonts/a.woff2", b"FONT-TWO");
    let out = site.build();
    let new_sheet = theme_css_name(&out);
    let new_font = format!("fonts/a.{}.woff2", compute_binary_hash(b"FONT-TWO"));
    assert_ne!(new_sheet, old_sheet);
    let home = fs::read_to_string(out.join("index.html")).unwrap();
    assert!(home.contains(&format!("/_moss/theme/{new_sheet}")) && !home.contains(&old_sheet), "{home}");
    assert_eq!(fs::read(out.join("_moss/theme").join(&new_font)).unwrap(), b"FONT-TWO");
    assert!(fs::read_to_string(out.join("_moss/theme").join(&new_sheet)).unwrap().contains(&new_font));
    assert_eq!(fs::read(out.join("_moss/theme/fonts/a.woff2")).unwrap(), b"FONT-TWO");
    assert!(!out.join("_moss/theme").join(&old_sheet).exists(), "old stylesheet left behind");
    assert!(!out.join("_moss/theme").join(&old_font).exists(), "old hashed font left behind");

    let out = site.build();
    assert_eq!(theme_css_name(&out), new_sheet);
    assert_eq!(fs::read(out.join("_moss/theme").join(&new_font)).unwrap(), b"FONT-TWO");
}
