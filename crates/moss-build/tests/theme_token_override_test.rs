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

/// The overlay block of a hero is as wide as its longest word unless the
/// author gives it a width; the property must reach both the width and the
/// cap, or the cap (`max(600px, 35%)`) silently clamps a wider request.
#[test]
fn hero_content_width_property_sets_the_overlay_width() {
    let site = Site::new();
    site.write(".moss/config.toml", b"[site]\ndomain = \"example.com\"\n");
    site.write(".moss/theme/style.css", b":root { --moss-hero-content-width: 29%; }\n");
    write_png(&site, "poster.png", [48, 96, 160]);
    site.write(
        "index.md",
        b"---\ntitle: Home\n---\n\n:::hero\n![](/poster.png)\n\nExample Show\n:::\n",
    );
    let out = site.build();
    let css = moss_css(&out);
    assert!(
        css.contains("inline-size:var(--moss-hero-content-width,auto)"),
        "overlay width rule: {css}"
    );
    assert!(
        css.contains("max-inline-size:var(--moss-hero-content-width,max(600px,35%))"),
        "overlay cap rule: {css}"
    );
    assert!(read(&out, "index.html").contains("Example Show"));
}

/// A theme states a header band and its link colour with tokens, not with
/// selectors that have to outrank moss's own rules. These are the rules that
/// read the tokens; the render gate `theme-floor-hooks` checks what they paint.
#[test]
fn header_tokens_are_read_by_moss_rules() {
    let site = Site::new();
    site.write(".moss/config.toml", b"[site]\ndomain = \"example.com\"\n");
    site.write("index.md", b"---\ntitle: Home\n---\n\nBody.\n");
    let out = site.build();
    let css = moss_css(&out).replace(' ', "");

    assert!(css.contains("body>header{background:var(--moss-header-bg,transparent)}"), "header band: {css}");
    assert!(css.contains(".main-nava{color:var(--moss-header-ink,"), "nav ink: {css}");
    assert!(css.contains("body>header.site-name{color:var(--moss-header-ink,"), "site name ink: {css}");
}

/// The hairline under the navigation takes its colour from a token, so a theme
/// that gives the header a band removes the rule with one declaration on `:root`.
#[test]
fn header_rule_token_colours_the_navigation_hairline() {
    let site = Site::new();
    site.write(".moss/config.toml", b"[site]\ndomain = \"example.com\"\n");
    site.write("index.md", b"---\ntitle: Home\n---\n\nBody.\n");
    let out = site.build();
    let css = moss_css(&out).replace(' ', "");

    assert!(
        css.contains("border-block-end:1pxsolidvar(--moss-header-rule,var(--moss-border-light))"),
        "navigation hairline: {css}"
    );
}

/// A theme that clears the cover tint states its card text colour with one
/// token; the muted lines derive from it, so one token recolours all four.
#[test]
fn cover_ink_token_is_read_by_moss_rules() {
    let site = Site::new();
    site.write(".moss/config.toml", b"[site]\ndomain = \"example.com\"\n");
    site.write("index.md", b"---\ntitle: Home\n---\n\nBody.\n");
    let out = site.build();
    let css = moss_css(&out).replace(' ', "");

    // Every line in the full ink, not a fade of it: the band is darkened only until full ink reaches 4.5:1.
    assert!(
        css.contains("[data-cover-color].moss-card-content:is(.moss-card-title,.moss-card-meta,.moss-card-kicker,.moss-card-description){color:var(--moss-cover-ink,#fff)"),
        "cover lines: {css}"
    );
}

/// A page whose body is only its opening hero keeps `<main id="main-content">`
/// (the skip link targets it) and its hidden `<h1>`, but no empty
/// `<article class="container">`, whose margin left blank page above the footer.
#[test]
fn a_hero_only_home_renders_no_empty_article() {
    let site = Site::new();
    site.write(".moss/config.toml", b"[site]\ndomain = \"example.com\"\n");
    write_png(&site, "poster.png", [48, 96, 160]);
    site.write("index.md", b"---\ntitle: Home\n---\n\n:::hero\n![](/poster.png)\n\nOverlay words\n:::\n");
    site.write("about.md", b"---\ntitle: About\nlisted: false\n---\n\nSome body text.\n");
    let out = site.build();

    let home = read(&out, "index.html");
    assert!(home.contains(r#"<main id="main-content""#), "main landmark kept:\n{home}");
    assert!(home.contains(r#"<h1 class="moss-folder-title visually-hidden""#) && home.contains(">Home</h1>"), "heading kept:\n{home}");
    assert!(!home.contains("<article"), "empty article:\n{home}");

    let about = read(&out, "about/index.html");
    assert!(about.contains(r#"<article class="container""#), "a page with text keeps its article:\n{about}");
}
