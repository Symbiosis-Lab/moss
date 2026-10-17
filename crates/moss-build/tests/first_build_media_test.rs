//! A site's first build must publish the same pages its second build would.
//!
//! The render runs before the encode, so a page can only link a video's
//! adaptive-streaming ladder and a hero's full-size poster once the encode has
//! made them. One-shot builds (`moss build`, `moss deploy`) used to leave them
//! unlinked until a second run. These drive `run_pipeline` the way the CLI does,
//! with real ffmpeg, from an empty directory.
//!
//! Skipped when ffmpeg is absent rather than failing: the encoder is an
//! optional runtime dependency moss downloads, not a build requirement.

use std::fs;
use std::path::{Path, PathBuf};

fn ffmpeg_on_path() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn build_sync(folder_path: &str) -> Result<String, String> {
    use moss_build::build::{run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
    use moss_build::cli::host::cli_host_ports;
    use moss_build::vault_root::VaultRoot;

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(run_pipeline(PipelineConfig {
        root: VaultRoot::resolve(PathBuf::from(folder_path)),
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

/// A 1280x720 clip rich enough in bits (a ladder is only built for a source whose
/// own bitrate fills more than its bottom rung).
fn make_clip(dest: &Path) -> bool {
    std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner", "-loglevel", "error", "-y",
            "-f", "lavfi", "-i", "testsrc2=size=1280x720:rate=15,noise=alls=25:allf=t", "-t", "6",
            "-c:v", "libx264", "-preset", "veryfast", "-b:v", "3500k", "-maxrate", "3500k",
            "-bufsize", "7000k", "-pix_fmt", "yuv420p",
        ])
        .arg(dest)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

struct Site {
    root: PathBuf,
}

impl Site {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("moss_first_build_media_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        Site { root }
    }
    fn write(&self, rel: &str, body: &str) {
        fs::write(self.root.join(rel), body).unwrap();
    }
    fn staging(&self) -> PathBuf {
        self.root.join(".moss/build.nosync/staging")
    }
    fn page(&self, rel: &str) -> String {
        fs::read_to_string(self.staging().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
    }
    /// Just the `<video>` elements of a page, so a failure reads without the chrome.
    fn videos(&self, rel: &str) -> String {
        let html = self.page(rel);
        html.split("<video").skip(1).map(|v| format!("<video{}", v.split("</video>").next().unwrap_or(v))).collect::<Vec<_>>().join("\n")
    }
    fn build(&self) {
        build_sync(self.root.to_str().unwrap()).expect("build succeeds");
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// A home whose hero is a video, and an article embedding another.
fn site_with_two_videos() -> Option<Site> {
    let site = Site::new();
    site.write("index.md", "# Home\n\n:::hero\n![[hero.mp4]]\n:::\n");
    site.write("watch.md", "# Watch\n\n![[clip.mp4]]\n");
    (make_clip(&site.root.join("hero.mp4")) && make_clip(&site.root.join("clip.mp4"))).then_some(site)
}

#[test]
fn one_cold_build_links_the_ladder_and_the_poster_and_a_second_build_changes_nothing() {
    if !ffmpeg_on_path() {
        eprintln!("skipping: ffmpeg not on PATH");
        return;
    }
    let Some(site) = site_with_two_videos() else {
        eprintln!("skipping: could not synthesise the sources");
        return;
    };

    site.build();
    let (home, watch) = (site.page("index.html"), site.page("watch/index.html"));
    assert!(
        watch.contains(r#"src="/clip.hls/master.m3u8""#),
        "the first build's page must offer the ladder; got:\n{}", site.videos("watch/index.html")
    );
    assert!(
        home.contains(r#"poster="/hero.poster.jpg""#),
        "the first build's hero must show the full-size poster; got:\n{}", site.videos("index.html")
    );
    for named in ["clip.hls/master.m3u8", "hero.poster.jpg"] {
        assert!(site.staging().join(named).is_file(), "{named} is named by a page and must exist");
    }

    site.build();
    assert_eq!(watch, site.page("watch/index.html"), "a second build must not change the embed page");
    assert_eq!(home, site.page("index.html"), "a second build must not change the hero page");
}

#[test]
fn a_video_that_gets_no_ladder_is_not_given_one_by_name() {
    if !ffmpeg_on_path() {
        eprintln!("skipping: ffmpeg not on PATH");
        return;
    }
    let Some(site) = site_with_two_videos() else {
        eprintln!("skipping: could not synthesise the sources");
        return;
    };
    // Not a video at all: the encoder refuses it, so nothing is produced for it.
    fs::write(site.root.join("broken.mp4"), b"not a video").unwrap();
    site.write("broken.md", "# Broken\n\n![[broken.mp4]]\n");

    site.build();
    let broken = site.page("broken/index.html");
    assert!(
        !broken.contains("broken.hls") && !broken.contains("broken.poster.jpg"),
        "a page must not name a ladder or poster the build never made; got:\n{broken}"
    );
    assert!(
        site.page("watch/index.html").contains("clip.hls/master.m3u8"),
        "the healthy video beside it still gets its ladder on the first build"
    );
}

/// A site of pictures needs no second pass: its first build's pages are what a
/// second build would write, so a one-shot build does not owe one for an image.
#[test]
fn an_image_only_site_is_the_same_after_its_first_build() {
    let site = Site::new();
    site.write("index.md", "# Home\n\n:::hero\n![[hero.png]]\n:::\n\n![[photo.png]]\n");
    for name in ["hero.png", "photo.png"] {
        image::RgbImage::from_fn(1600, 900, |x, y| image::Rgb([(x % 251) as u8, (y % 241) as u8, 90]))
            .save(site.root.join(name))
            .unwrap();
    }

    site.build();
    let first = site.page("index.html");
    assert!(first.contains("hero.png") || first.contains("hero.webp"), "the hero image is on the page");
    site.build();
    assert_eq!(first, site.page("index.html"));
}
