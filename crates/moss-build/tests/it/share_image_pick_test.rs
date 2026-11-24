//! A page that declares no cover shares the first body picture a platform can
//! take as-is, not whatever picture happens to come first.

use moss_build::moss_paths::MossPaths;
use std::fs;
use std::path::Path;

fn build(folder: &Path) -> Result<String, String> {
    let turn = crate::site::turn();
    use moss_build::build::{PipelineConfig, PluginMode};
    use moss_build::cli::host::cli_host_ports;
    use moss_build::vault_root::VaultRoot;

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(crate::site::run(&turn, PipelineConfig {
        root: VaultRoot::resolve(folder),
        progress: moss_build::build::stdout_sink(),
        plugins: PluginMode::Skip,
        watch: false,
        start_server: false,
        host: cli_host_ports(&folder.to_string_lossy()),
        trigger: moss_build::build::BuildTrigger::Full, reason: None,
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
    let html = fs::read_to_string(MossPaths::new(root).staging_dir().join(format!("{slug}/index.html"))).unwrap();
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

fn meta_content(root: &Path, slug: &str, selector: &str) -> Option<String> {
    let html = fs::read_to_string(MossPaths::new(root).staging_dir().join(format!("{slug}/index.html"))).unwrap();
    let doc = scraper::Html::parse_document(&html);
    let found = doc.select(&scraper::Selector::parse(selector).unwrap()).next().and_then(|m| m.value().attr("content")).map(String::from);
    found
}

/// A shared picture is described by the alt text the author wrote for it,
/// so a platform that shows alt text (Mastodon's link card) reads what the
/// picture shows rather than the page title again.
#[test]
fn the_share_image_carries_the_pictures_own_alt_text() {
    let root = std::env::temp_dir().join(format!("moss_share_alt_{}", uuid::Uuid::new_v4()));
    write(&root, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n");
    write(&root, "index.md", &page("", "Welcome."));
    picture(&root, "pics/heron.png", 1600, 900);
    picture(&root, "pics/plain.png", 1600, 900);
    write(&root, "heron.md", &page("", "Text.\n\n![A grey heron wading at dusk](pics/heron.png)"));
    write(&root, "declared.md", &page("cover: pics/heron.png\n", "![A grey heron wading at dusk](pics/heron.png)"));
    write(&root, "plain.md", &page("", "![](pics/plain.png)"));
    let built = build(&root);
    assert!(built.is_ok(), "{built:?}");

    for slug in ["heron", "declared"] {
        assert_eq!(meta_content(&root, slug, r#"meta[property="og:image:alt"]"#).as_deref(), Some("A grey heron wading at dusk"), "{slug}");
        assert_eq!(meta_content(&root, slug, r#"meta[name="twitter:image:alt"]"#).as_deref(), Some("A grey heron wading at dusk"), "{slug}");
    }
    assert_eq!(meta_content(&root, "plain", r#"meta[property="og:image:alt"]"#).as_deref(), Some("Page"), "a picture with no alt keeps the title");
    let _ = fs::remove_dir_all(&root);
}

fn poster_site(share_cover: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("moss_share_cover_{}", uuid::Uuid::new_v4()));
    write(&root, ".moss/config.toml", &format!("schema_version = 6\n\n[site]\nlang = \"en\"\n{share_cover}"));
    picture(&root, "pics/wide.png", 1600, 900);
    picture(&root, "pics/poster.png", 1080, 1350);
    // The home page has its own landscape picture so that no page needs a generated card.
    write(&root, "index.md", &page("", "![h](pics/wide.png)"));
    write(&root, "event.md", &page("cover: pics/poster.png\n", "Doors at eight."));
    root
}

#[test]
fn share_cover_as_is_shares_a_portrait_poster_itself() {
    let root = poster_site("share_cover = \"as-is\"\n");
    let built = build(&root);
    assert!(built.is_ok(), "{built:?}");
    let shared = share_image(&root, "event");
    let cards = MossPaths::new(&root).staging_dir().join("_moss/og");
    let drawn = cards.exists();
    let _ = fs::remove_dir_all(&root);

    assert!(shared.ends_with("/pics/poster.png"), "the poster is the share image: {shared}");
    assert!(!drawn, "no generated card is written");
}

#[test]
fn share_cover_auto_draws_a_portrait_poster_into_the_card() {
    let root = poster_site("");
    let built = build(&root);
    assert!(built.is_ok(), "{built:?}");
    let shared = share_image(&root, "event");
    let drawn = MossPaths::new(&root).staging_dir().join("_moss/og").exists();
    let _ = fs::remove_dir_all(&root);

    assert!(shared.contains("/_moss/og/"), "the poster is drawn into the card: {shared}");
    assert!(drawn, "the card is written where the as-is test looks for it");
}

#[test]
fn share_cover_with_an_unknown_value_stops_the_build_naming_the_key() {
    let root = poster_site("share_cover = \"original\"\n");
    let built = build(&root);
    let _ = fs::remove_dir_all(&root);

    let error = built.expect_err("an unknown value is a build error");
    assert!(error.contains("share_cover") && error.contains("original"), "{error}");
}
