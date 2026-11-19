//! The link-preview card moss draws follows the theme's card tokens on every
//! page that gets one, the home page included, and a rebuild after
//! the theme's card picture changes draws the new picture.

use std::fs;
use std::path::Path;

fn build(folder: &Path, trigger: moss_build::build::BuildTrigger) -> Result<String, String> {
    use moss_build::build::{run_pipeline, PipelineConfig, PluginMode};
    use moss_build::cli::host::cli_host_ports;
    use moss_build::vault_root::VaultRoot;

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(run_pipeline(PipelineConfig {
        root: VaultRoot::resolve(folder),
        progress: moss_build::build::stdout_sink(),
        plugins: PluginMode::Skip,
        watch: false,
        start_server: false,
        host: cli_host_ports(&folder.to_string_lossy()),
        trigger,
        exits_after_build: true,
        site_url_override: None,
        server_port: None,
        admission_epoch: None,
        live_port: None,
        rename_pairs: Vec::new(),
    }))
}

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

fn strip(root: &Path, rgb: [u8; 3]) {
    let path = root.join(".moss/theme/strip.png");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbImage::from_pixel(300, 630, image::Rgb(rgb)).save(path).unwrap();
}

/// The card a page's `og:image` names, decoded.
fn card(root: &Path, page: &str) -> image::RgbImage {
    let html = fs::read_to_string(root.join(".moss/build.nosync/staging").join(page)).unwrap();
    let doc = scraper::Html::parse_document(&html);
    let url = doc
        .select(&scraper::Selector::parse(r#"meta[property="og:image"]"#).unwrap())
        .next()
        .and_then(|m| m.value().attr("content"))
        .unwrap_or_else(|| panic!("{page} has no og:image"))
        .to_string();
    let at = url.find("/_moss/og/").unwrap_or_else(|| panic!("{page} shares no drawn card: {url}"));
    let file = root.join(".moss/build.nosync/staging").join(&url[at + 1..]);
    image::open(&file).unwrap_or_else(|e| panic!("{page}'s card {}: {e}", file.display())).to_rgb8()
}

fn near(px: &image::Rgb<u8>, want: [u8; 3]) -> bool {
    px.0.iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 6)
}

#[test]
fn the_theme_colours_and_pictures_every_drawn_card_and_a_new_picture_is_drawn() {
    let root = std::env::temp_dir().join(format!("moss_share_card_theme_{}", uuid::Uuid::new_v4()));
    write(&root, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\ntitle = \"Garden\"\n");
    write(
        &root,
        ".moss/theme/style.css",
        ":root { --moss-share-card-bg: #101830; --moss-share-card-ink: #f0eee8; --moss-share-card-picture: url(\"strip.png\"); }\n",
    );
    strip(&root, [200, 40, 30]);
    write(&root, "index.md", "---\ntitle: Garden\n---\n\nA small garden by the river.\n");
    write(&root, "notes.md", "---\ntitle: Notes on the garden\n---\n\nThe beds were turned in March.\n");
    let built = build(&root, moss_build::build::BuildTrigger::Full);
    assert!(built.is_ok(), "{built:?}");

    for page in ["notes/index.html", "index.html"] {
        let card = card(&root, page);
        assert!(near(card.get_pixel(40, 600), [16, 24, 48]), "{page}: ground {:?}", card.get_pixel(40, 600));
        assert!(near(card.get_pixel(1100, 300), [200, 40, 30]), "{page}: picture {:?}", card.get_pixel(1100, 300));
    }

    // A build that may carry pages from the last one, as a one-shot build does.
    strip(&root, [30, 160, 60]);
    let rebuilt = build(&root, moss_build::build::BuildTrigger::Unobserved);
    assert!(rebuilt.is_ok(), "{rebuilt:?}");
    let card = card(&root, "notes/index.html");
    let _ = fs::remove_dir_all(&root);
    assert!(near(card.get_pixel(1100, 300), [30, 160, 60]), "the old picture was kept: {:?}", card.get_pixel(1100, 300));
}
