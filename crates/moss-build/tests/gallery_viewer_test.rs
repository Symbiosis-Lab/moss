//! A `:::gallery` opens its images in the full-screen viewer that the media
//! collection pages use: the page carries the viewer's markup and script, and
//! every image sits in a link the script binds to.

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
        let dir = std::env::temp_dir().join(format!("moss_gallery_viewer_{}", uuid::Uuid::new_v4()));
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

// 1x1 transparent PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xff, 0xff, 0x3f,
    0x00, 0x05, 0xfe, 0x02, 0xfe, 0xa7, 0x35, 0x81, 0x84, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];

fn read_page(out: &Path, url: &str) -> String {
    fs::read_to_string(out.join(url)).unwrap_or_else(|e| panic!("{url}: {e}"))
}

#[test]
fn gallery_images_open_in_the_shared_viewer() {
    let site = Site::new();
    site.write(".moss/config.toml", b"[site]\ndomain = \"example.com\"\n");
    site.write("index.md", b"---\ntitle: Home\n---\n\nHello.\n");
    site.write(
        "photos.md",
        b"---\ntitle: Photos\n---\n\nPhotos.\n\n:::gallery\nred.png\nblue.png\n:::\n",
    );
    site.write("red.png", PNG);
    site.write("blue.png", PNG);
    let out = site.build();

    let page = read_page(&out, "photos/index.html");
    assert!(page.contains(r#"id="lightbox""#), "the viewer's markup is missing:\n{page}");
    assert_eq!(
        page.matches(r#"class="moss-gallery-link""#).count(),
        2,
        "each image must sit in a link the viewer binds to:\n{page}"
    );
    assert!(
        page.contains(r#"class="moss-gallery-link" href="/red.png""#),
        "the link must point at the full-size image:\n{page}"
    );
    assert!(page.contains("<img"), "the image stays a real <img>:\n{page}");

    let script = page
        .split("/_moss/js/fullscreen.")
        .nth(1)
        .unwrap_or_else(|| panic!("the viewer's script tag is missing:\n{page}"));
    let hash = script.split(".js").next().unwrap();
    assert!(
        out.join(format!("_moss/js/fullscreen.{hash}.js")).exists(),
        "the tag must name a file the build wrote"
    );

    // A page without a gallery carries no viewer markup.
    let home = read_page(&out, "index.html");
    assert!(!home.contains(r#"id="lightbox""#), "{home}");
}

#[test]
fn a_gallery_link_without_alt_text_still_has_a_name() {
    let site = Site::new();
    site.write(".moss/config.toml", b"[site]\ndomain = \"example.com\"\n");
    site.write("index.md", b"---\ntitle: Home\n---\n\nHello.\n");
    site.write(
        "photos.md",
        b"---\ntitle: Photos\n---\n\nPhotos.\n\n:::gallery\nred.png\ngreen.png\n:::\n\n:::gallery\n![A lake](lake.png)\n:::\n",
    );
    site.write("red.png", PNG);
    site.write("green.png", PNG);
    site.write("lake.png", PNG);
    let out = site.build();

    let page = read_page(&out, "photos/index.html");
    assert!(
        page.contains(r#"href="/red.png" data-title="" aria-label="Open image 1 of 2">"#),
        "the first image of two must be named by its position:\n{page}"
    );
    assert!(
        page.contains(r#"href="/green.png" data-title="" aria-label="Open image 2 of 2">"#),
        "the second image of two must be named by its position:\n{page}"
    );
    assert!(
        page.contains(r#"href="/lake.png" data-title="A lake">"#),
        "an image with alt text needs no aria-label:\n{page}"
    );
    assert_eq!(
        page.matches("aria-label=\"Open image").count(),
        2,
        "only the two unnamed images get a label:\n{page}"
    );
}

#[test]
fn a_gallery_link_label_follows_the_page_language() {
    let site = Site::new();
    site.write(".moss/config.toml", b"[site]\ndomain = \"example.com\"\n");
    site.write("index.md", b"---\ntitle: Home\n---\n\nHello.\n");
    site.write(
        "zh-photos.md",
        b"---\ntitle: \xe7\x85\xa7\xe7\x89\x87\nlang: zh-hans\n---\n\n:::gallery\nred.png\ngreen.png\n:::\n",
    );
    site.write("red.png", PNG);
    site.write("green.png", PNG);
    let out = site.build();

    let page = read_page(&out, "zh-photos/index.html");
    assert!(
        page.contains(r#"href="/red.png" data-title="" aria-label="打开第 1 张图片，共 2 张">"#),
        "a zh-hans page must name its images in Chinese:\n{page}"
    );
}

