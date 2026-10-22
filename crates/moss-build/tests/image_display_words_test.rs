//! Image display words (`focus:X%,Y%`, `contain`, ...) written after a picture's pipe
//! reach that picture's `<img>` wherever the picture sits: a gallery item, a collage
//! cell, or a lone embed.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Once};

fn build(folder: &Path) -> Result<String, String> {
    use moss_build::build::{run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
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
        trigger: BuildTrigger::Full,
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

fn write_picture(root: &Path, rel: &str, shade: u8) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbImage::from_fn(96, 64, |x, y| image::Rgb([shade, (x % 200) as u8, (y % 200) as u8]))
        .save(path)
        .unwrap();
}

struct Site(PathBuf);
impl Drop for Site {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A one-page site whose home page is `body`, with three pictures in `Photos/`.
fn home_html(body: &str) -> String {
    let root = std::env::temp_dir().join(format!("moss_display_words_{}", uuid::Uuid::new_v4()));
    let site = Site(root.clone());
    write(&root, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n");
    write(&root, "index.md", &format!("---\ntitle: Home\n---\n\n{body}\n"));
    for (i, name) in ["a", "b", "c"].iter().enumerate() {
        write_picture(&root, &format!("Photos/{name}.jpg"), 40 + i as u8 * 60);
    }
    image::RgbImage::from_fn(64, 96, |x, y| image::Rgb([90, (x % 200) as u8, (y % 200) as u8]))
        .save(root.join("Photos/tall.jpg"))
        .unwrap();
    std::fs::copy(root.join("Photos/tall.jpg"), root.join("Photos/海报 tall.jpg")).unwrap();
    let built = build(&root);
    assert!(built.is_ok(), "{built:?}");
    let html = fs::read_to_string(root.join(".moss/build.nosync/staging/index.html")).unwrap();
    drop(site);
    html
}

/// The `object-*` declarations on the `<img>` of each element matched by `selector`,
/// in document order. A picture's LQIP background is not a display word and is left out.
fn img_styles(html: &str, selector: &str) -> Vec<Option<String>> {
    let doc = scraper::Html::parse_document(html);
    let sel = scraper::Selector::parse(selector).unwrap();
    let img = scraper::Selector::parse("img").unwrap();
    doc.select(&sel)
        .map(|scope| {
            let style = scope.select(&img).next()?.value().attr("style")?;
            let object: Vec<&str> = style.split(';').filter(|d| d.starts_with("object-")).collect();
            (!object.is_empty()).then(|| object.join(";"))
        })
        .collect()
}

#[test]
fn collage_cells_keep_their_display_words() {
    let html = home_html(
        ":::grid {collage}\n![[Photos/a.jpg|focus:50%,20%]]\n+++\n![[Photos/b.jpg|contain]]\n+++\n![[Photos/c.jpg]]\n+++\n![[Photos/b.jpg|focus:30%,70% contain]]\n:::",
    );
    assert_eq!(
        img_styles(&html, ".moss-grid-card"),
        vec![
            Some("object-position:50% 20%".to_string()),
            Some("object-fit:contain".to_string()),
            None,
            Some("object-fit:contain;object-position:30% 70%".to_string()),
        ],
    );
}

#[test]
fn both_image_forms_style_a_captioned_picture_alike() {
    let html = home_html(
        "![[Photos/a.jpg|focus:20%,62%|Harbour at dawn]]\n\n![Harbour at dawn|focus:20%,62%](Photos/a.jpg)\n\n[![Harbour at dawn|contain](Photos/b.jpg)](https://example.org/harbour)\n*__Harbour__ 1890 · Public domain*",
    );
    assert_eq!(
        img_styles(&html, "figure.moss-image"),
        vec![
            Some("object-position:20% 62%".to_string()),
            Some("object-position:20% 62%".to_string()),
            Some("object-fit:contain".to_string()),
        ],
    );
    let doc = scraper::Html::parse_document(&html);
    let caption = scraper::Selector::parse("figure.moss-image figcaption").unwrap();
    let captions: Vec<String> = doc.select(&caption).map(|c| c.inner_html()).collect();
    assert_eq!(captions[..2], ["Harbour at dawn", "Harbour at dawn"]);
    assert_eq!(captions[2], "<strong>Harbour</strong> 1890 · Public domain");
    let linked = scraper::Selector::parse(r#"figure.moss-image > a[href="https://example.org/harbour"] img"#).unwrap();
    let img = doc.select(&linked).next().expect("the link wraps the image");
    let style = img.value().attr("style").unwrap_or_default();
    // The placeholder and the display words share the one attribute.
    assert!(style.contains("background") && style.ends_with("object-fit:contain"), "got: {style}");
}

#[test]
fn an_upright_lead_is_marked_and_a_landscape_lead_is_not() {
    let html = home_html(
        ":::grid {collage}\n![[Photos/tall.jpg|Tall]]\n+++\n![[Photos/a.jpg|A]]\n+++\n![[Photos/b.jpg|B]]\n:::\n\n:::grid {collage}\n![[Photos/a.jpg|A]]\n+++\n![[Photos/tall.jpg|Tall]]\n+++\n![[Photos/b.jpg|B]]\n:::\n\n:::grid {collage}\n![Poster](<Photos/海报 tall.jpg>)\n*Poster*\n+++\n![[Photos/a.jpg|A]]\n+++\n![[Photos/b.jpg|B]]\n:::",
    );
    let doc = scraper::Html::parse_document(&html);
    let leads = scraper::Selector::parse(".moss-grid[data-collage] > :first-child").unwrap();
    let marked: Vec<bool> = doc.select(&leads).map(|l| l.value().attr("data-upright").is_some()).collect();
    assert_eq!(marked, [true, false, true]);
    let upright = scraper::Selector::parse("[data-upright]").unwrap();
    assert_eq!(doc.select(&upright).count(), 2, "only a lead is ever marked");
}

#[test]
fn gallery_items_keep_their_display_words() {
    let html = home_html(
        ":::gallery\n![[Photos/a.jpg|focus:50%,20%]]\n![[Photos/b.jpg|contain]]\n![[Photos/c.jpg]]\n![[Photos/b.jpg|focus:30%,70% contain]]\n:::",
    );
    assert_eq!(
        img_styles(&html, ".moss-gallery-item"),
        vec![
            Some("object-position:50% 20%".to_string()),
            Some("object-fit:contain".to_string()),
            None,
            Some("object-fit:contain;object-position:30% 70%".to_string()),
        ],
    );
}

/// Every log line the build writes, for the warning test. The logger is process-wide.
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Capture;
impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        LOGGED.lock().unwrap().push(record.args().to_string());
    }
    fn flush(&self) {}
}
static CAPTURE: Capture = Capture;

fn capture_logs() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        log::set_logger(&CAPTURE).unwrap();
        log::set_max_level(log::LevelFilter::Warn);
    });
}

#[test]
fn a_mistyped_caption_warns_with_its_page_and_stays_the_caption() {
    capture_logs();
    let html = home_html("![[Photos/a.jpg|contian]]\n\n![[Photos/b.jpg|Lake at dusk]]\n\n![A lake|cvoer](Photos/c.jpg)");
    let logged = LOGGED.lock().unwrap().clone();
    assert!(logged.iter().any(|m| m.contains("'cvoer'") && m.contains("'cover'")), "a standard image warns too: {logged:?}");
    let warnings: Vec<&String> = logged.iter().filter(|m| m.contains("contian")).collect();
    assert_eq!(warnings.len(), 1, "expected exactly one warning line, got: {logged:?}");
    assert!(
        warnings[0].contains("index.md") && warnings[0].contains("'contain'") && warnings[0].contains("cover"),
        "expected a warning naming the page, the word and the known words, got: {}",
        warnings[0]
    );
    assert!(logged.iter().all(|m| !m.contains("Lake at dusk")), "an ordinary caption must not warn");
    assert!(html.contains("contian"), "the mistyped word is still the caption");
}
