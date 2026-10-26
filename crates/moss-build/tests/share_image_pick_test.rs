//! A page that declares no cover shares the first body picture a platform can
//! take as-is, not whatever picture happens to come first.

use std::fs;
use std::path::Path;

fn build(folder: &Path) -> Result<String, String> {
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
        trigger: moss_build::build::BuildTrigger::Full,
        exits_after_build: true,
        site_url_override: None,
        server_port: None,
        admission_epoch: None,
        live_port: None,
    }))
}

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

fn picture(root: &Path, rel: &str, w: u32, h: u32) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x % 251) as u8, (y % 241) as u8, 90]))
        .save(path)
        .unwrap();
}

fn page(front: &str, body: &str) -> String {
    format!("---\ntitle: Page\n{front}---\n\n{body}\n")
}

fn share_image(root: &Path, slug: &str) -> String {
    let html = fs::read_to_string(root.join(format!(".moss/build.nosync/staging/{slug}/index.html"))).unwrap();
    let doc = scraper::Html::parse_document(&html);
    doc.select(&scraper::Selector::parse(r#"meta[property="og:image"]"#).unwrap())
        .next()
        .and_then(|m| m.value().attr("content"))
        .unwrap_or_else(|| panic!("{slug} has no og:image"))
        .to_string()
}

#[test]
fn the_share_image_is_the_first_picture_a_platform_can_take_as_is() {
    let root = std::env::temp_dir().join(format!("moss_share_pick_{}", uuid::Uuid::new_v4()));
    write(&root, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n");
    write(&root, "index.md", &page("", "Welcome."));
    for n in 1..=3 {
        picture(&root, &format!("pics/slice{n}.png"), 420, 1048);
    }
    picture(&root, "pics/wide.png", 1600, 900);
    picture(&root, "pics/small.png", 300, 200);
    picture(&root, "pics/scroll.png", 600, 1800);
    picture(&root, "pics/declared.png", 1600, 900);

    let slices = "![a](pics/slice1.png)\n\n![b](pics/slice2.png)\n\n![c](pics/slice3.png)\n\n";
    write(&root, "slices.md", &page("", &format!("{slices}![w](pics/wide.png)")));
    write(&root, "scroll.md", &page("", "![s](pics/scroll.png)"));
    write(&root, "small.md", &page("", "![i](pics/small.png)\n\n![w](pics/wide.png)"));
    write(&root, "declared.md", &page("cover: pics/declared.png\n", &format!("{slices}![w](pics/wide.png)")));
    let built = build(&root);
    assert!(built.is_ok(), "{built:?}");

    let (slices, scroll, small, declared) = (
        share_image(&root, "slices"),
        share_image(&root, "scroll"),
        share_image(&root, "small"),
        share_image(&root, "declared"),
    );
    let _ = fs::remove_dir_all(&root);

    assert!(slices.ends_with("/pics/wide.png"), "tall slices are skipped: {slices}");
    assert!(small.ends_with("/pics/wide.png"), "a small picture is skipped: {small}");
    assert!(declared.ends_with("/pics/declared.png"), "an author's cover wins: {declared}");
    assert!(scroll.contains("/_moss/og/"), "a lone scroll becomes the plate card: {scroll}");
}
