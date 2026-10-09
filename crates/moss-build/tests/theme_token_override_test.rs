//! A site theme must be able to override the colours moss derives from a cover
//! image. An inline `style="--moss-cover-color: …"` outranks every stylesheet
//! rule, so moss publishes the per-item value under a private name and its own
//! CSS reads `var(--moss-cover-color, var(--item-cover-color))`: a theme that
//! sets the public token wins, and with no theme the per-item colour still
//! paints.

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
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("moss_theme_token_override_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        Site(dir)
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


fn write_png(site: &Site, rel: &str, rgb: [u8; 3]) {
    let path = site.0.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbImage::from_pixel(8, 8, image::Rgb(rgb)).save(path).unwrap();
}

fn read(out: &Path, rel: &str) -> String {
    fs::read_to_string(out.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The moss stylesheet, minified, as the build wrote it.
fn moss_css(out: &Path) -> String {
    let dir = out.join("_moss");
    let name = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|n| n.starts_with("style.") && n.ends_with(".css"))
        .expect("the build writes _moss/style.<hash>.css");
    read(&dir, &name)
}

/// Every `style="…"` attribute value in `html`.
fn inline_styles(html: &str) -> Vec<&str> {
    html.split(r#"style=""#).skip(1).filter_map(|rest| rest.split('"').next()).collect()
}

#[test]
fn a_theme_token_is_not_shadowed_by_an_inline_declaration() {
    let site = Site::new();
    site.write(".moss/config.toml", b"[site]\ndomain = \"example.com\"\n");
    site.write(
        ".moss/theme/style.css",
        b":root { --moss-cover-color: rebeccapurple; --moss-hero-panel-bg: rebeccapurple; }\n",
    );
    write_png(&site, "poster.png", [48, 96, 160]);
    site.write(
        "index.md",
        b"---\ntitle: Home\n---\n\n:::hero\n![](/poster.png)\n\nOverlay words\n:::\n\n:::grid 1\n![](/poster.png)\n\nPart One\n\n### [Title](/works/one/)\n:::\n",
    );
    let out = site.build();
    let home = read(&out, "index.html");

    // The per-item colours are still published, under private names...
    assert!(home.contains("--item-cover-color:"), "no per-item cover colour:\n{home}");
    assert!(home.contains("--item-hero-panel-bg:"), "no per-item hero panel:\n{home}");

    // ...and nothing sets the public tokens inline, where no stylesheet could beat it.
    for style in inline_styles(&home) {
        assert!(!style.contains("--moss-cover-color"), "inline --moss-cover-color: {style}");
        assert!(!style.contains("--moss-hero-panel-bg"), "inline --moss-hero-panel-bg: {style}");
    }

    // moss's own rules prefer the public token and fall back to the per-item one.
    let css = moss_css(&out);
    assert!(
        css.contains("[data-cover-color] .moss-card-content{background:var(--moss-cover-color,var(--item-cover-color))"),
        "card band rule: {css}"
    );
    assert!(css.contains("var(--moss-hero-panel-bg,var(--item-hero-panel-bg"), "panel rule: {css}");
}
