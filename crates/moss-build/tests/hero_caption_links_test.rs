//! A link written inside a `:::hero` caption resolves exactly as the same
//! link does in a body paragraph: same href, same `wikilink` class, and a
//! link to a missing page produces the same build problem.
//!
//! Captions were rendered from raw strings, so no URL walker saw them and the
//! graph never resolved their links. Its own test binary: the build problem
//! counter is per thread, and the build runs on this test's thread.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use moss_build::build::cli_output::take_cli_problems;
use moss_build::build::{run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
use moss_build::cli::host::cli_host_ports;
use moss_build::vault_root::VaultRoot;

/// Builds share the process-wide CLI problem bookkeeping, so run them one at a time.
static BUILD_LOCK: Mutex<()> = Mutex::new(());

struct Site {
    dir: PathBuf,
}

impl Drop for Site {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// Write `files` into a scratch folder and build it. Returns the output
/// directory and the number of build problems the build reported.
fn build(files: &[(&str, &str)]) -> (Site, PathBuf, usize) {
    let _guard = BUILD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("moss_caption_links_{}", uuid::Uuid::new_v4()));
    let site = Site { dir: dir.clone() };
    for (rel, body) in files {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    take_cli_problems();
    let source = dir.to_string_lossy().to_string();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(run_pipeline(PipelineConfig {
        root: VaultRoot::resolve(&dir),
        progress: moss_build::build::stdout_sink(),
        plugins: PluginMode::Skip,
        watch: false,
        start_server: false,
        host: cli_host_ports(&source),
        trigger: BuildTrigger::Full,
        exits_after_build: true,
        site_url_override: None,
        server_port: None,
        admission_epoch: None,
        live_port: None,
    }))
    .expect("build succeeds");
    let problems = take_cli_problems();
    (site, dir.join(".moss/build.nosync/staging"), problems)
}

/// Every opening `<a …>` tag in `html`, in order.
fn anchor_tags(html: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find("<a ") {
        let after = &rest[start..];
        let end = after.find('>').expect("anchor closes") + 1;
        tags.push(after[..end].to_string());
        rest = &after[end..];
    }
    tags
}

/// The slice of `html` that holds one paragraph beginning with `marker`.
fn paragraph_from<'a>(html: &'a str, marker: &str) -> &'a str {
    let start = html.find(marker).unwrap_or_else(|| panic!("no {marker:?} in page"));
    let end = html[start..].find("</p>").expect("paragraph closes") + start;
    &html[start..end]
}

const WORK_PIECE: &str = "---\ntitle: Piece One\n---\n\nThe piece.\n";
const WORK_INDEX: &str = "---\ntitle: Work\n---\n\nAll work.\n";
const SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"4\" height=\"3\"></svg>";

/// The home page carries the captions and the same two links in a body
/// paragraph, so the body is the reference for what the link should be.
fn home_with_captions(caption_a: &str, caption_b: &str, body: &str) -> String {
    format!(
        "---\ntitle: Home\n---\n\n:::hero\n![[assets/a.svg|{caption_a}]]\n\n![[assets/b.svg|{caption_b}]]\n\n[[work|Work →]]\n:::\n\n{body}\n"
    )
}

fn base_files<'a>(home: &'a str) -> Vec<(&'a str, &'a str)> {
    vec![
        (".moss/config.toml", "[site]\ndomain = \"example.com\"\n"),
        ("index.md", home),
        ("assets/a.svg", SVG),
        ("assets/b.svg", SVG),
        ("work/Piece One.md", WORK_PIECE),
        ("work/index.md", WORK_INDEX),
    ]
}

#[test]
fn hero_caption_links_resolve_like_body_links() {
    let body = "Body link: [piece](work/Piece%20One.md) and [[Piece One|*piece*]].";
    let home = home_with_captions(
        "Two dancers in [*Piece One*](work/Piece%20One.md) · Venue · 2018",
        "Two dancers in [[Piece One|*Piece One*]] · Venue · 2019",
        body,
    );
    let (_site, out, problems) = build(&base_files(&home));
    assert_eq!(problems, 0, "every link here resolves");

    let html = fs::read_to_string(out.join("index.html")).unwrap();
    let body_tags = anchor_tags(paragraph_from(&html, "Body link:"));
    let caption_tags: Vec<String> = [
        paragraph_from(&html, "moss-hero-caption\" data-slide=\"0\""),
        paragraph_from(&html, "moss-hero-caption\" data-slide=\"1\""),
    ]
    .iter()
    .flat_map(|p| anchor_tags(p))
    .collect();

    assert_eq!(body_tags.len(), 2, "body has both links: {body_tags:?}");
    assert!(
        body_tags[0].contains("href=\"work/piece-one/\""),
        "body link resolves to the page slug: {body_tags:?}"
    );
    assert_eq!(
        caption_tags, body_tags,
        "caption links must render the same anchor as the body does"
    );
}

#[test]
fn hero_caption_link_to_missing_page_warns_like_a_body_link() {
    let missing = "[gone](work/Nowhere.md) and [[Nowhere]]";
    let clean_body = "No links here.";

    let caption_home = home_with_captions(
        &format!("Caption {missing}"),
        "Plain caption",
        clean_body,
    );
    let (_a, _out, caption_problems) = build(&base_files(&caption_home));

    let body_home = home_with_captions("Plain caption", "Plain caption", &format!("Body {missing}"));
    let (_b, _out, body_problems) = build(&base_files(&body_home));

    assert!(body_problems > 0, "a body link to a missing page is a build problem");
    assert_eq!(
        caption_problems, body_problems,
        "a caption link to a missing page must report the same problems as a body link"
    );
}
