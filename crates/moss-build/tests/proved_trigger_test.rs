//! A build that watched nothing (`moss build`, a watch session's first build)
//! proves from the last sealed manifest what changed since, and carries the
//! rest. Each guard below is one input no page's markdown shows; moving it must
//! render every page again.

use moss_build::moss_paths::MossPaths;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use moss_build::build::BuildTrigger;

/// The environment is one per process, and these tests set it.
static SERIAL: Mutex<()> = Mutex::new(());

fn build(root: &Path, trigger: BuildTrigger) -> serde_json::Value {
    build_with(root, trigger, moss_build::build::PluginMode::Skip, None)
}

fn build_with(root: &Path, trigger: BuildTrigger, plugins: moss_build::build::PluginMode, site_url: Option<&str>) -> serde_json::Value {
    run(root, trigger, plugins, site_url, None)
}

/// A build for a folder a preview server shows: it leaves picture blurs to the
/// background media phase instead of decoding them before the render.
fn build_previewing(root: &Path, trigger: BuildTrigger) -> serde_json::Value {
    run(root, trigger, moss_build::build::PluginMode::Skip, None, Some(1))
}

fn run(root: &Path, trigger: BuildTrigger, plugins: moss_build::build::PluginMode, site_url: Option<&str>, server_port: Option<u16>) -> serde_json::Value {
    use moss_build::build::{run_pipeline, PipelineConfig};
    use moss_build::cli::host::cli_host_ports;
    use moss_build::vault_root::VaultRoot;
    let folder = root.to_string_lossy().into_owned();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(run_pipeline(PipelineConfig {
        root: VaultRoot::resolve(root),
        progress: moss_build::build::stdout_sink(),
        plugins,
        watch: false,
        start_server: false,
        host: cli_host_ports(&folder),
        trigger, reason: None,
        exits_after_build: true,
        site_url_override: site_url.map(str::to_string),
        server_port,
        admission_epoch: None,
        live_port: None,
        rename_pairs: Vec::new(),
    }))
    .unwrap_or_else(|e| panic!("build failed: {e}"));
    last_profile(root)
}

fn last_profile(root: &Path) -> serde_json::Value {
    let text = fs::read_to_string(MossPaths::new(root).profile()).unwrap();
    serde_json::from_str(text.lines().last().unwrap()).unwrap()
}

fn count(record: &serde_json::Value, key: &str) -> u64 {
    record["counts"][key].as_u64().unwrap_or_else(|| panic!("no count {key} in {record}"))
}

struct Site(tempfile::TempDir);

impl Site {
    fn new() -> Self {
        let site = Site(tempfile::tempdir().unwrap());
        site.write(".moss/config.toml", "[site]\ndomain = \"example.com\"\n");
        site.write("index.md", "---\ntitle: Home\n---\n\nWelcome.\n");
        site.write("notes/index.md", "---\ntitle: Notes\n---\n\nAll notes.\n");
        site.write("notes/alpha.md", "---\ntitle: Alpha\ndate: 2026-01-02\n---\n\nAlpha body.\n");
        site.write("notes/beta.md", "---\ntitle: Beta\ndate: 2026-01-03\n---\n\nBeta body.\n");
        site.write("about.md", "---\ntitle: About\n---\n\nAbout us.\n");
        site
    }

    fn root(&self) -> &Path {
        self.0.path()
    }

    /// `rel` may also be an absolute path, such as one `MossPaths` names.
    fn write(&self, rel: impl AsRef<Path>, text: &str) {
        let path = self.root().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// A settled seal for the build under test to prove against: two builds,
    /// since a first build's media (the share image) are new against no
    /// manifest, and no page of a seal whose media moved is carried.
    fn sealed(&self) -> &Self {
        build(self.root(), BuildTrigger::Unobserved);
        build(self.root(), BuildTrigger::Unobserved);
        self
    }
}

/// The build ended at its entry: nothing it would read had moved, so the
/// generation the last seal promoted is its output.
fn stopped(record: &serde_json::Value) -> bool {
    count(record, "unchanged_at_entry") == 1
}

/// Proved from the seal, or stopped before needing to.
fn unchanged(record: &serde_json::Value) -> bool {
    stopped(record) || (count(record, "proved_from_seal") == 1 && count(record, "pages_rendered") == 0)
}

fn rendered_everything(record: &serde_json::Value) -> bool {
    count(record, "pages_carried") == 0 && count(record, "pages_rendered") > 0
}

#[test]
fn a_build_that_watched_nothing_ends_unchanged_before_it_scans_when_nothing_changed() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let site = Site::new();
    site.sealed();
    let record = build(site.root(), BuildTrigger::Unobserved);
    assert!(stopped(&record), "{record}");
    assert!(record["phases"].as_array().unwrap().iter().all(|p| p["name"] != "scan"), "{record}");
    assert_publishes_a_full_render(&site);
}

/// Each thing the stop requires besides an unmoved folder, moved alone: the
/// build renders, and publishes what a full render does.
#[test]
fn a_stop_needs_the_sealed_generation_current_and_its_media_settled() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    type Move = fn(&Site);
    fn generation(s: &Site) -> PathBuf {
        MossPaths::new(s.root()).generations_dir().join(fs::read_to_string(MossPaths::new(s.root()).current_generation_marker()).unwrap().trim())
    }
    let moves: &[(&str, Move)] = &[
        ("current names another generation", |s| s.write(MossPaths::new(s.root()).current_generation_marker(), "0123456789abcdef")),
        ("the generation is gone", |s| fs::remove_dir_all(generation(s)).unwrap()),
        ("the seal left work to later passes", |s| rewrite_manifest(s, |m| m["build_inputs"]["complete"] = false.into())),
        ("media settled after the render", |s| rewrite_manifest(s, |m| m["build_inputs"]["media_settled"] = false.into())),
        ("no list of waiting cards", |s| fs::remove_file(MossPaths::new(s.root()).build_dir().join("link-cards-waiting.json")).unwrap()),
        ("an unreadable list of waiting cards", |s| s.write(MossPaths::new(s.root()).build_dir().join("link-cards-waiting.json"), "[\"cut off")),
    ];
    let control = Site::new();
    control.sealed();
    assert!(stopped(&build(control.root(), BuildTrigger::Unobserved)), "control");
    for (name, move_it) in moves {
        let site = Site::new();
        site.sealed();
        move_it(&site);
        let record = build(site.root(), BuildTrigger::Unobserved);
        assert!(!stopped(&record), "{name}: {record}");
        assert_publishes_a_full_render(&site);
    }
}

/// The bisect switch renders, whatever the seal proves.
#[test]
fn moss_no_incremental_keeps_a_no_change_build_from_stopping() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let site = Site::new();
    site.sealed();
    std::env::set_var("MOSS_NO_INCREMENTAL", "1");
    let record = build(site.root(), BuildTrigger::Unobserved);
    std::env::remove_var("MOSS_NO_INCREMENTAL");
    assert!(!stopped(&record) && rendered_everything(&record), "{record}");
}

/// A site with a picture whose last two builds ran beside a preview, sealed
/// as a preview leaves it before the background fills the placeholders in.
/// The second build reads colours the first one cached, so its own record
/// says complete; a session that opened on a cold cache does not.
fn served_site_with_a_picture() -> Site {
    let site = Site::new();
    site.write("notes/alpha.md", "---\ntitle: Alpha\ndate: 2026-01-02\n---\n\n![A field](field.png)\n");
    image::RgbImage::from_fn(640, 480, |x, y| image::Rgb([(x % 251) as u8, (y % 241) as u8, 160])).save(site.root().join("notes/field.png")).unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    for _ in 0..2 {
        run(site.root(), BuildTrigger::Unobserved, moss_build::build::PluginMode::Skip, None, Some(port));
    }
    rewrite_manifest(&site, |m| m["build_inputs"]["complete"] = serde_json::Value::Bool(false));
    site
}

/// A one-shot build after a serving seal renders and bakes the placeholders
/// rather than keeping that generation.
#[test]
fn a_build_after_a_serving_seal_renders_and_publishes_a_full_render() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let site = served_site_with_a_picture();
    let record = build(site.root(), BuildTrigger::Unobserved);
    assert!(!stopped(&record), "{record}");
    assert_publishes_a_full_render(&site);
}

/// A build that bakes but reuses pages from a serving seal keeps what they
/// lacked, so the build after it renders rather than stopping on its seal.
#[test]
fn a_build_that_reused_a_serving_seal_does_not_let_the_next_one_stop() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let site = served_site_with_a_picture();
    site.write("about.md", "---\ntitle: About\n---\n\nAbout us, again.\n");
    build(site.root(), BuildTrigger::ContentOnly(vec![PathBuf::from("about.md")]));
    let record = build(site.root(), BuildTrigger::Unobserved);
    assert!(!stopped(&record), "{record}");
    assert_publishes_a_full_render(&site);
}

/// A first build's sharing cards land at its seal, but no page reads one, so
/// the seal is settled and the build after it, with nothing changed, ends at
/// its entry.
#[test]
fn a_build_after_a_cold_build_with_nothing_changed_ends_at_its_entry() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let site = Site::new();
    build(site.root(), BuildTrigger::Unobserved);
    let record = build(site.root(), BuildTrigger::Unobserved);
    assert!(unchanged(&record), "{record}");
}

/// A video and a `.webp` source are published by their media lanes, not the
/// asset walk, but the proof walks them like any other source.
#[test]
fn a_site_with_a_video_and_a_webp_source_still_proves_from_the_seal() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let site = Site::new();
    site.write("media.md", "---\ntitle: Media\n---\n\n![Cover](cover.webp)\n\n![[clip.mp4]]\n");
    // Wider than the deploy edge, so the encoder plan owns it rather than calling it already small.
    image::RgbImage::from_fn(2600, 8, |x, _| image::Rgb([(x % 251) as u8, 90, 160])).save(site.root().join("cover.webp")).unwrap();
    fs::write(site.root().join("clip.mp4"), b"not a video").unwrap();
    site.sealed();
    let record = build(site.root(), BuildTrigger::Unobserved);
    assert!(unchanged(&record), "{record}");
}

#[test]
fn a_full_trigger_still_renders_everything() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let site = Site::new();
    site.sealed();
    let record = build(site.root(), BuildTrigger::Full);
    assert_eq!(count(&record, "proved_from_seal"), 0);
    assert!(rendered_everything(&record), "{record}");
}

#[test]
fn one_markdown_edit_renders_that_page_and_not_the_site() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let site = Site::new();
    site.sealed();
    site.write("about.md", "---\ntitle: About\n---\n\nAbout us, at length.\n");
    let record = build(site.root(), BuildTrigger::Unobserved);
    assert_eq!(count(&record, "proved_from_seal"), 1, "{record}");
    assert!(count(&record, "pages_rendered") >= 1, "{record}");
    assert!(count(&record, "pages_carried") > 0, "{record}");
}

/// A first build's sharing cards are new beside an empty last seal, but a page
/// names its card by address whether or not the card exists yet.
#[test]
fn the_first_edit_after_a_cold_build_renders_that_page_and_not_the_site() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let site = Site::new();
    build(site.root(), BuildTrigger::Unobserved);
    site.write("about.md", "---\ntitle: About\n---\n\nAbout us, at length.\n");
    let record = build(site.root(), BuildTrigger::Unobserved);
    assert_eq!(count(&record, "proved_from_seal"), 1, "{record}");
    assert!(count(&record, "pages_carried") > 0, "{record}");
}

/// One test per input the markdown does not show. Each must cost a full render.
#[test]
fn every_input_outside_the_page_tree_keeps_the_build_full() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    type Move = fn(&Site);
    let guards: &[(&str, Move)] = &[
        ("a config value", |s| s.write(".moss/config.toml", "[site]\ndomain = \"example.org\"\n")),
        ("a theme stylesheet", |s| s.write(".moss/theme/style.css", "body { color: red; }\n")),
        ("a created page", |s| s.write("notes/gamma.md", "---\ntitle: Gamma\n---\n\nNew.\n")),
        ("a deleted page", |s| fs::remove_file(s.root().join("notes/beta.md")).unwrap()),
        ("a redirect table", |s| s.write(".moss/redirects.json", "{\"/old/\": \"/about/\"}\n")),
        ("a data file", |s| s.write(".moss/data/people.json", "[\"Ada\"]\n")),
        ("deleted link cards", |s| fs::remove_dir_all(MossPaths::new(s.root()).cache_link_meta()).unwrap()),
        ("a new image", |s| s.write("notes/pic.svg", "<svg xmlns=\"http://www.w3.org/2000/svg\"/>")),
        ("another moss binary", |s| rewrite_manifest(s, |m| m["builder_fingerprint"] = "an older moss".into())),
        ("a manifest with no input record", |s| rewrite_manifest(s, |m| { m.as_object_mut().unwrap().remove("build_inputs"); })),
    ];
    let setup = || {
        let site = Site::new();
        site.write(".moss/data/people.json", "[]\n");
        site.write(MossPaths::new(site.root()).cache_link_meta().join(format!("{}.json", "a".repeat(64))), "{}\n");
        site.sealed();
        site
    };
    // The control: with nothing moved the same setup proves, so each guard
    // below is what keeps its build full.
    let control = build(setup().root(), BuildTrigger::Unobserved);
    assert!(unchanged(&control), "control: {control}");
    let mut carried = Vec::new();
    for (name, move_it) in guards {
        let site = setup();
        move_it(&site);
        let record = build(site.root(), BuildTrigger::Unobserved);
        if count(&record, "proved_from_seal") != 0 || !rendered_everything(&record) {
            carried.push(format!("{name}: {}", record["counts"]));
        }
    }
    assert!(carried.is_empty(), "proved despite a moved input:\n{}", carried.join("\n"));
}

#[test]
fn an_environment_variable_the_build_read_keeps_the_build_full_when_it_moves() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let site = Site::new();
    std::env::set_var("MOSS_SITE_URL", "https://one.example.com");
    site.sealed();
    std::env::set_var("MOSS_SITE_URL", "https://two.example.com");
    let record = build(site.root(), BuildTrigger::Unobserved);
    std::env::remove_var("MOSS_SITE_URL");
    assert_eq!(count(&record, "proved_from_seal"), 0, "{record}");
    assert!(rendered_everything(&record), "{record}");
}

/// A variable only a page's render reads stays in the record while that page
/// is carried, so changing it later still renders everything.
#[test]
fn a_variable_a_carried_page_read_still_keeps_the_build_full_when_it_moves() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    const VAR: &str = "MOSS_PROVED_TRIGGER_TEST_ONLY";
    let site = Site::new();
    site.sealed();
    // As if the sealing build had rendered a page that read VAR = "a".
    rewrite_manifest(&site, |m| { m["build_inputs"]["env"][VAR] = sha256_hex("a").into(); });
    std::env::set_var(VAR, "a");
    let carried = build(site.root(), BuildTrigger::Unobserved);
    assert!(unchanged(&carried), "{carried}");
    std::env::set_var(VAR, "b");
    let record = build(site.root(), BuildTrigger::Unobserved);
    std::env::remove_var(VAR);
    assert_eq!(count(&record, "proved_from_seal"), 0, "{record}");
    assert!(rendered_everything(&record), "{record}");
}

/// A watch build that carries pages from a manifest with no record cannot
/// know what those pages read, so it seals no record either.
#[test]
fn a_build_that_carries_from_no_record_leaves_the_next_one_unproved() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let site = Site::new();
    site.sealed();
    rewrite_manifest(&site, |m| { m.as_object_mut().unwrap().remove("build_inputs"); });
    site.write("about.md", "---\ntitle: About\n---\n\nAbout us, at length.\n");
    let carried = build(site.root(), BuildTrigger::ContentOnly(vec![site.root().join("about.md")]));
    assert!(count(&carried, "pages_carried") > 0, "{carried}");
    let record = build(site.root(), BuildTrigger::Unobserved);
    assert_eq!(count(&record, "proved_from_seal"), 0, "{record}");
}

/// A process hook that writes its own state under `.moss/plugins/<name>/` on
/// every build moves a measured `.moss/` input during each build: the build
/// measures again after the hook, finds its record broken and renders every
/// page, recording the state the hook left.
#[test]
fn a_process_hook_that_rewrites_its_state_every_build_keeps_the_build_full() {
    use moss_build::build::PluginMode;
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    const STATE: &str = ".moss/plugins/stamp-writer/state.json";
    let site = Site::new();
    for file in ["manifest.json", "main.bundle.js"] {
        let fixture = format!("{}/tests/fixtures/plugins/stamp-writer/{file}", env!("CARGO_MANIFEST_DIR"));
        site.write(&format!(".moss/plugins/stamp-writer/{file}"), &fs::read_to_string(fixture).unwrap());
    }
    build_with(site.root(), BuildTrigger::Unobserved, PluginMode::Blocking, None);
    let first = fs::read_to_string(site.root().join(STATE)).expect("the hook ran");
    assert!(recorded_moss_input(&site, STATE).is_some(), "the first build measured after its hook wrote");
    let record = build_with(site.root(), BuildTrigger::Unobserved, PluginMode::Blocking, None);
    let second = fs::read_to_string(site.root().join(STATE)).unwrap();
    assert_ne!(first, second, "the hook writes new bytes every build");
    let recorded = recorded_moss_input(&site, STATE);
    assert!(recorded.is_some(), "the state file is a recorded input");
    assert_eq!(count(&record, "proved_from_seal"), 0, "{record}");
    assert!(rendered_everything(&record), "{record}");
}

/// The address `--site-url` gives is in every page, so a build given another
/// one, or none, renders every page again.
#[test]
fn another_site_url_option_keeps_the_build_full() {
    use moss_build::build::PluginMode::Skip;
    const A: &str = "https://a.example.net";
    const B: &str = "https://b.example.net";
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let control = Site::new();
    for _ in 0..2 {
        build_with(control.root(), BuildTrigger::Unobserved, Skip, Some(A));
    }
    let record = build_with(control.root(), BuildTrigger::Unobserved, Skip, Some(A));
    assert!(unchanged(&record), "control: {record}");

    for next in [Some(B), None] {
        let site = Site::new();
        for _ in 0..2 {
            build_with(site.root(), BuildTrigger::Unobserved, Skip, Some(A));
        }
        let record = build_with(site.root(), BuildTrigger::Unobserved, Skip, next);
        assert_eq!(count(&record, "proved_from_seal"), 0, "{next:?}: {record}");
        assert!(rendered_everything(&record), "{next:?}: {record}");
        let pages = published(site.root());
        let stale: Vec<_> = pages.iter().filter(|(_, html)| String::from_utf8_lossy(html).contains(A)).map(|(path, _)| path).collect();
        assert!(stale.is_empty(), "{next:?}: pages still carry {A}: {stale:?}");
        if let Some(b) = next {
            assert!(pages.values().any(|html| String::from_utf8_lossy(html).contains(b)), "no page carries {b}");
        }
    }
}

/// Whether plugins run decides what their blocks, slots and derived embeds put
/// in a page, so a build after a `--no-plugins` one renders every page again.
#[test]
fn turning_plugins_on_keeps_the_build_full() {
    use moss_build::build::PluginMode::{Blocking, Skip};
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let control = Site::new();
    for _ in 0..2 {
        build_with(control.root(), BuildTrigger::Unobserved, Blocking, None);
    }
    let record = build_with(control.root(), BuildTrigger::Unobserved, Blocking, None);
    assert!(unchanged(&record), "control: {record}");

    let site = Site::new();
    for _ in 0..2 {
        build_with(site.root(), BuildTrigger::Unobserved, Skip, None);
    }
    let record = build_with(site.root(), BuildTrigger::Unobserved, Blocking, None);
    assert_eq!(count(&record, "proved_from_seal"), 0, "{record}");
    assert!(rendered_everything(&record), "{record}");
}

/// Every build of a site with a comment server records the refresh's outcome,
/// which no page reads. An older moss on another computer still rewrites it in
/// the folder on every attempt, so that file must not count as an input.
#[test]
fn a_comment_refresh_that_fails_every_build_still_lets_the_next_build_prove() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    for proxy in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"] {
        std::env::remove_var(proxy);
    }
    // A loopback port nothing listens on: every refresh fails at once.
    let server = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}", listener.local_addr().unwrap())
    };
    let site = Site::new();
    site.write(
        ".moss/config.toml",
        &format!("[site]\ndomain = \"example.com\"\n\n[services.comments]\nprovider = \"artalk\"\nserver_url = \"{server}\"\n"),
    );
    site.sealed();
    let state = ".moss/data/comment-sync-state.json";
    for n in 1..=2 {
        site.write(state, &format!(r#"{{"last_success_at": null, "last_attempt_at": {n}.0, "last_error": null}}"#));
        let record = build(site.root(), BuildTrigger::Unobserved);
        let recorded = moss_build::build::features::sync::load_comment_sync_state(site.root().to_str().unwrap());
        assert!(recorded.last_error.is_some(), "the refresh ran and recorded its outcome");
        assert!(unchanged(&record), "build {n}, recorded {state}: {:?}: {record}", recorded_moss_input(&site, state));
    }
}

fn recorded_moss_input(site: &Site, rel: &str) -> Option<serde_json::Value> {
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(MossPaths::new(site.root()).hashes()).unwrap()).unwrap();
    manifest["build_inputs"]["moss"].get(rel).cloned()
}

fn sha256_hex(text: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// The build day reaches past/upcoming listings and edition dates, so a new
/// day renders every page, whether the build watched a save or nothing.
#[test]
fn a_later_build_day_renders_every_page_again() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut carried = Vec::new();
    for trigger in [BuildTrigger::Unobserved, BuildTrigger::ContentOnly(vec![PathBuf::from("about.md")])] {
        let site = Site::new();
        site.write("events/index.md", "---\ntitle: Events\nchildren_group: upcoming\n---\n");
        site.write("events/talk.md", "---\ntitle: Talk\nevent:\n  start: 2026-03-14\n---\n\nA talk.\n");
        std::env::set_var("SOURCE_DATE_EPOCH", "1773360000"); // 2026-03-13 UTC
        site.sealed();
        std::env::set_var("SOURCE_DATE_EPOCH", "1773532800"); // 2026-03-15 UTC
        if let BuildTrigger::ContentOnly(_) = trigger {
            site.write("about.md", "---\ntitle: About\n---\n\nAbout us, again.\n");
        }
        let record = build(site.root(), trigger.clone());
        std::env::remove_var("SOURCE_DATE_EPOCH");
        if !rendered_everything(&record) {
            carried.push(format!("{trigger:?}: {}", record["counts"]));
        }
    }
    assert!(carried.is_empty(), "carried pages into a new day:\n{}", carried.join("\n"));
}

/// A carried page's bytes come from the last generation, not from staging, so
/// a build after staging is cleared still publishes what a full render would.
#[test]
fn a_proved_build_after_staging_is_cleared_publishes_what_a_full_render_does() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let site = Site::new();
    site.sealed();
    site.write("about.md", "---\ntitle: About\n---\n\nAbout us, at length.\n");
    let _ = fs::remove_dir_all(MossPaths::new(site.root()).staging_dir());
    let record = build(site.root(), BuildTrigger::Unobserved);
    assert_eq!(count(&record, "proved_from_seal"), 1, "{record}");
    let proved = published(site.root());

    let full = Site(tempfile::tempdir().unwrap());
    copy_source(site.root(), full.root());
    build(full.root(), BuildTrigger::Full);
    let reference = published(full.root());
    let differing: Vec<_> = reference.keys().chain(proved.keys())
        .filter(|k| reference.get(*k) != proved.get(*k))
        .collect();
    assert!(differing.is_empty(), "differs from a full render: {differing:?}");
}

/// The re-render owed once pictures settle renders the pages showing them,
/// carries the rest, and serves what a full render would. The reference is the
/// folder opened on another machine: its encodes arrive with it, its picture
/// facts stay behind, so its first render has no blur and its own settle must
/// bring it.
#[test]
fn a_settled_picture_renders_the_pages_showing_it_and_matches_a_full_render() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let site = Site::new();
    site.write("notes/alpha.md", "---\ntitle: Alpha\ndate: 2026-01-02\n---\n\n![A field](field.png)\n");
    image::RgbImage::from_fn(1600, 900, |x, y| image::Rgb([(x % 251) as u8, (y % 241) as u8, 160]))
        .save(site.root().join("notes/field.png"))
        .unwrap();
    let blur = "background-image:url(data:image";
    build_previewing(site.root(), BuildTrigger::Unobserved);
    let page = || fs::read_to_string(MossPaths::new(site.root()).staging_dir().join("notes/alpha/index.html")).unwrap();
    assert!(!page().contains(blur), "the first preview render left the blur to the media phase");

    let record = build_previewing(site.root(), BuildTrigger::ImagesSettled { edited: false });
    assert_eq!(count(&record, "proved_from_seal"), 1, "{record}");
    assert!(count(&record, "pages_rendered") >= 1, "{record}");
    assert!(count(&record, "pages_carried") > 0, "{record}");
    assert!(page().contains(blur), "the settled blur is baked into the page showing the picture");
    let settled = published(site.root());

    let full = Site(tempfile::tempdir().unwrap());
    copy_source(site.root(), full.root());
    build_previewing(full.root(), BuildTrigger::Full);
    build_previewing(full.root(), BuildTrigger::ImagesSettled { edited: false });
    let reference = published(full.root());
    let differing: Vec<_> = reference.keys().chain(settled.keys())
        .filter(|k| reference.get(*k) != settled.get(*k))
        .collect();
    assert!(differing.is_empty(), "differs from a full render: {differing:?}");
}

/// A card the entry check's retry brings in is on the page this build
/// publishes, though nothing in the folder moved.
#[test]
fn a_card_that_arrives_at_the_entry_check_is_on_the_page_that_build_publishes() {
    use std::io::{Read, Write};
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    for proxy in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"] {
        std::env::remove_var(proxy);
    }
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{port}/page");
    let site = Site::new();
    site.write("notes/alpha.md", &format!("---\ntitle: Alpha\ndate: 2026-01-02\n---\n\n:::grid 1\n[External]({url})\n:::\n"));
    site.sealed();
    assert!(stopped(&build(site.root(), BuildTrigger::Unobserved)), "the card is still missing, so nothing moved");

    let listener = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
    std::thread::spawn(move || {
        let html = "<html><head><title>Card Title</title></head><body></body></html>";
        for mut stream in listener.incoming().flatten() {
            let _ = stream.read(&mut [0u8; 2048]);
            let _ = stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\n\r\n{html}", html.len()).as_bytes());
        }
    });
    let record = build(site.root(), BuildTrigger::Unobserved);
    assert!(!stopped(&record), "{record}");
    let page = String::from_utf8(published(site.root())["notes/alpha/index.html"].clone()).unwrap();
    assert!(page.contains(&format!("http://127.0.0.1:{port}/favicon.ico")), "the card's favicon is not on the page: {record}");
    assert_publishes_a_full_render(&site);
}

/// A card fetched outside any build (a publish settles the waiting cards
/// before its rebuild) leaves that build nothing to fetch, so only the card
/// files themselves keep it from ending at its entry.
#[test]
fn a_card_settled_before_the_build_is_on_the_page_that_build_publishes() {
    use std::io::{Read, Write};
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    for proxy in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"] {
        std::env::remove_var(proxy);
    }
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{port}/page");
    let site = Site::new();
    site.write("notes/alpha.md", &format!("---\ntitle: Alpha\ndate: 2026-01-02\n---\n\n:::grid 1\n[External]({url})\n:::\n"));
    site.sealed();

    let listener = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
    std::thread::spawn(move || {
        let html = "<html><head><title>Card Title</title></head><body></body></html>";
        for mut stream in listener.incoming().flatten() {
            let _ = stream.read(&mut [0u8; 2048]);
            let _ = stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\n\r\n{html}", html.len()).as_bytes());
        }
    });
    let settled = moss_build::build::page::link_meta::settle(&moss_build::moss_paths::MossPaths::new(site.root()));
    assert_eq!(settled.map(|s| (s.fetched, s.waiting)), Some((1, 0)), "the card came in before the build");

    let record = build(site.root(), BuildTrigger::Unobserved);
    assert!(!stopped(&record), "{record}");
    let page = String::from_utf8(published(site.root())["notes/alpha/index.html"].clone()).unwrap();
    assert!(page.contains(&format!("http://127.0.0.1:{port}/favicon.ico")), "the card's favicon is not on the page: {record}");
    assert_publishes_a_full_render(&site);
}

/// Inputs a build moves itself after measuring `.moss/` (an older moss's link
/// card taken into the site folder, the redirect history rewritten) are
/// recorded as it left them, so the re-render owed once pictures settle still
/// proves from its seal.
#[test]
fn inputs_a_build_moves_itself_still_let_the_next_build_prove() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let site = Site::new();
    let url = "https://example.com";
    site.write("notes/alpha.md", &format!("---\ntitle: Alpha\ndate: 2026-01-02\n---\n\n:::grid 1\n[External]({url})\n:::\n"));
    let old_card = MossPaths::new(site.root()).cache_link_meta().join(format!("{}.json", sha256_hex(url)));
    site.write(&old_card, "{\"title\": \"Example\"}\n");
    site.write(".moss/redirects.json", "{\"qr/notes/alpha.svg\": \"notes/alpha/\"}\n");
    build_previewing(site.root(), BuildTrigger::Unobserved);
    assert!(site.root().join(format!(".moss/data/social/links/{}.json", sha256_hex(url))).exists(), "the card moved into the site folder");
    assert!(!site.root().join(&old_card).exists());
    assert!(!fs::read_to_string(site.root().join(".moss/redirects.json")).unwrap().contains("qr/"), "the saved QR stub was dropped");

    let record = build_previewing(site.root(), BuildTrigger::ImagesSettled { edited: false });
    assert_eq!(count(&record, "proved_from_seal"), 1, "{record}");
}

/// What a serving build told its host: the change set it published, and
/// whether it said a servable site was ready, and with which port it completed.
#[derive(Default)]
struct Heard {
    change_sets: Mutex<Vec<Option<moss_build::build::manifest::change_set::ChangeSet>>>,
    stage_ready: std::sync::atomic::AtomicUsize,
    completed_on: Mutex<Vec<Option<u16>>>,
    page_ticks: std::sync::atomic::AtomicUsize,
}

struct Ear(std::sync::Arc<Heard>);

impl moss_build::build::ports::reporter::BuildReporter for Ear {
    fn report(&self, event: &moss_build::build::progress::PipelineEvent) {
        if let moss_build::build::progress::PipelineEvent::BackgroundProgress { task, .. } = event {
            if task == "markdown" || task == "html_pages" {
                self.0.page_ticks.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
        if let moss_build::build::progress::PipelineEvent::Progress { step, completed: true, port, .. } = event {
            if step == "build" {
                self.0.completed_on.lock().unwrap().push(*port);
            }
        }
    }

    fn stage_ready(&self, _site_path: &Path) {
        self.0.stage_ready.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

#[async_trait::async_trait]
impl moss_build::build::ports::announcer::SealAnnouncer for Ear {
    fn promoted(&self, _generation_id: &str) {}
    async fn adopt_sealed(&self, _sealed: moss_build::build::manifest::SealedManifest) {}
    async fn publish_change_set(&self, _project_root: &Path, change_set: Option<moss_build::build::manifest::change_set::ChangeSet>) {
        self.0.change_sets.lock().unwrap().push(change_set);
    }
}

const PREVIEW_PORT: u16 = 47_311;

/// A build that opens a preview, as the app's folder open does, on `cell`: the
/// preview cell the server reads. Its server is a stand-in that answers with a
/// port.
fn serve(root: &Path, cell: &moss_build::moss_paths::ServedCell) -> (serde_json::Value, std::sync::Arc<Heard>) {
    serve_as(root, cell, BuildTrigger::Unobserved)
}

fn serve_as(root: &Path, cell: &moss_build::moss_paths::ServedCell, trigger: BuildTrigger) -> (serde_json::Value, std::sync::Arc<Heard>) {
    use moss_build::build::{run_pipeline, PipelineConfig};
    let heard = std::sync::Arc::new(Heard::default());
    let folder = root.to_string_lossy().into_owned();
    let mut host = moss_build::cli::host::cli_host_ports(&folder);
    host.site_dir = Some(cell.clone());
    host.launch_server = Some(std::sync::Arc::new(|_, _| Box::pin(async { Ok(PREVIEW_PORT) })));
    host.services.reporter = std::sync::Arc::new(Ear(heard.clone()));
    host.announcer = std::sync::Arc::new(Ear(heard.clone()));
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(run_pipeline(PipelineConfig {
        root: moss_build::vault_root::VaultRoot::resolve(root),
        progress: std::sync::Arc::new(Ear(heard.clone())),
        plugins: moss_build::build::PluginMode::Skip,
        watch: false,
        start_server: true,
        host,
        trigger, reason: None,
        exits_after_build: true,
        site_url_override: None,
        server_port: None,
        admission_epoch: None,
        live_port: None,
        rename_pairs: Vec::new(),
    }))
    .unwrap_or_else(|e| panic!("build failed: {e}"));
    (last_profile(root), heard)
}

/// A preview cell still on another folder, as a server left by an earlier folder is.
fn new_cell() -> moss_build::moss_paths::ServedCell {
    moss_build::moss_paths::Served {
        folder: moss_build::vault_root::VaultRoot::resolve("/moss-test/no-folder"),
        dir: PathBuf::new(),
    }
    .into_cell()
}

fn served(cell: &moss_build::moss_paths::ServedCell) -> PathBuf {
    cell.read().unwrap().dir.clone()
}

/// Every file under `dir`, by path.
fn tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    walkdir::WalkDir::new(dir).follow_links(true).into_iter().filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| (e.path().strip_prefix(dir).unwrap().to_string_lossy().into_owned(), fs::read(e.path()).unwrap()))
        .collect()
}

fn staging(root: &Path) -> PathBuf {
    MossPaths::new(&root.canonicalize().unwrap()).staging_dir()
}

/// Two serving opens: the second proves from the first's seal, so the next one
/// has a settled seal to stop on.
fn served_site() -> (Site, moss_build::moss_paths::ServedCell) {
    let site = Site::new();
    let cell = new_cell();
    serve(site.root(), &cell);
    serve(site.root(), &cell);
    (site, cell)
}

/// An open that changed nothing shows the staging tree the seal left, without
/// scanning or rendering, and tells the host what a build would have.
#[test]
fn a_serving_open_with_nothing_changed_shows_the_sealed_staging_without_scanning() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (site, cell) = served_site();
    let before = tree(&staging(site.root()));
    let folder = moss_build::vault_root::VaultRoot::resolve(site.root()).as_str().to_string();
    let records = moss_build::system::build_records::records();
    records.record_content_hashes(&folder, Default::default());
    let (record, heard) = serve(site.root(), &cell);
    assert!(stopped(&record), "{record}");
    let sealed: serde_json::Value = serde_json::from_str(&fs::read_to_string(MossPaths::new(site.root()).hashes()).unwrap()).unwrap();
    let baseline = records.content_hashes(&folder).expect("the watcher has a baseline");
    assert_eq!(serde_json::to_value(&baseline.files).unwrap(), sealed["files"], "the watcher diffs the next edit against the seal");
    assert!(record["phases"].as_array().unwrap().iter().all(|p| p["name"] != "scan"), "{record}");
    assert_eq!(served(&cell), staging(site.root()));
    assert_eq!(tree(&staging(site.root())), before);
    assert_eq!(heard.stage_ready.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(*heard.completed_on.lock().unwrap(), vec![Some(PREVIEW_PORT)]);
    assert_publishes_a_full_render(&site);
}

/// The preview's progress line reads a page count as a build starting, so a
/// re-render nobody asked for must not send one to the host.
#[test]
fn a_settle_render_counts_no_pages_to_the_host_where_a_full_render_does() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (site, cell) = served_site();
    let ticks = |heard: &Heard| heard.page_ticks.load(std::sync::atomic::Ordering::SeqCst);
    let (_, full) = serve_as(site.root(), &cell, BuildTrigger::Full);
    assert!(ticks(&full) > 0, "premise: a full render counts its pages to the host");
    let (record, settle) = serve_as(site.root(), &cell, BuildTrigger::VideosSettled);
    assert!(count(&record, "pages_rendered") > 0, "premise: the settle rendered pages: {record}");
    assert_eq!(ticks(&settle), 0, "a settle render announced itself");
    assert_eq!(settle.stage_ready.load(std::sync::atomic::Ordering::SeqCst), full.stage_ready.load(std::sync::atomic::Ordering::SeqCst), "the settle still hands its stage to the server");
}

/// The stopped open publishes the change set a proved open would: the same
/// verdict, from the persisted manifest rather than a fresh seal.
#[test]
fn a_stopped_open_publishes_the_change_set_a_proved_open_does() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (site, cell) = served_site();
    // A proved open: the stop refuses on a staging entry gone, so this build
    // renders that page and seals.
    fs::remove_file(staging(site.root()).join("about/index.html")).unwrap();
    let (record, proved) = serve(site.root(), &cell);
    assert!(!stopped(&record), "{record}");
    let (record, stopped_heard) = serve(site.root(), &cell);
    assert!(stopped(&record), "{record}");
    let proved = proved.change_sets.lock().unwrap().last().cloned().flatten();
    let stopped_set = stopped_heard.change_sets.lock().unwrap().last().cloned().flatten();
    assert!(proved.is_some());
    assert_eq!(stopped_set, proved);
}

/// The seal names a page staging no longer holds: the open builds, which puts
/// it back. A file the seal does not name is swept, and the open still stops.
#[test]
fn a_staging_entry_gone_builds_and_a_stray_file_is_swept_by_the_stop() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (site, cell) = served_site();
    fs::remove_file(staging(site.root()).join("about/index.html")).unwrap();
    let (record, _) = serve(site.root(), &cell);
    assert!(!stopped(&record), "{record}");
    assert!(staging(site.root()).join("about/index.html").is_file());

    fs::write(staging(site.root()).join("stray.html"), "left over").unwrap();
    let (record, _) = serve(site.root(), &cell);
    assert!(stopped(&record), "{record}");
    assert!(!staging(site.root()).join("stray.html").exists());
    assert_eq!(served(&cell), staging(site.root()));
}

/// Opening again in the same process after a stopped open, with the preview
/// already on staging: the stop's record says `current` holds what is shown,
/// so the next open may sweep and stops again.
#[test]
fn a_second_stopped_open_in_the_same_process_stops_again() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (site, cell) = served_site();
    assert!(stopped(&serve(site.root(), &cell).0));
    assert_eq!(served(&cell), staging(site.root()));
    assert!(stopped(&serve(site.root(), &cell).0));
    assert_publishes_a_full_render(&site);
}

/// The publish rebuild opens no preview and outlives its build: it never stops.
#[test]
fn a_build_that_neither_serves_nor_exits_does_not_stop() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (site, _cell) = served_site();
    use moss_build::build::{run_pipeline, PipelineConfig};
    let folder = site.root().to_string_lossy().into_owned();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(run_pipeline(PipelineConfig {
        root: moss_build::vault_root::VaultRoot::resolve(site.root()),
        progress: moss_build::build::null_sink(),
        plugins: moss_build::build::PluginMode::SlotsOnly,
        watch: false,
        start_server: false,
        host: moss_build::cli::host::cli_host_ports(&folder),
        trigger: BuildTrigger::Unobserved, reason: None,
        exits_after_build: false,
        site_url_override: None,
        server_port: None,
        admission_epoch: None,
        live_port: None,
        rename_pairs: Vec::new(),
    }))
    .unwrap();
    let record = last_profile(site.root());
    assert!(!stopped(&record), "{record}");
}

/// Staging holds bytes newer than the seal for a page whose source is as
/// sealed: a render that reached staging but never sealed, its edit then
/// undone. The stop serves what a proved open would, which carries the page
/// by its presence.
#[test]
fn staging_newer_than_the_seal_is_served_as_a_proved_open_would() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (site, cell) = served_site();
    let page = staging(site.root()).join("about/index.html");
    let newer = fs::read_to_string(&page).unwrap().replace("About us.", "About us, unsaved.");
    fs::write(&page, &newer).unwrap();
    let (record, _) = serve(site.root(), &cell);
    assert!(stopped(&record), "{record}");
    let stopped_bytes = fs::read_to_string(&page).unwrap();

    // The same staging under a proved open: another page moved, so the
    // stop refuses, and about.md is carried.
    fs::write(&page, &newer).unwrap();
    site.write("notes/beta.md", "---\ntitle: Beta\ndate: 2026-01-03\n---\n\nBeta body, again.\n");
    let (record, _) = serve(site.root(), &cell);
    assert!(!stopped(&record), "{record}");
    assert_eq!(fs::read_to_string(&page).unwrap(), stopped_bytes);
}

/// The site's current generation equals a full render of a copy of its source.
fn assert_publishes_a_full_render(site: &Site) {
    let built = published(site.root());
    let full = Site(tempfile::tempdir().unwrap());
    copy_source(site.root(), full.root());
    build(full.root(), BuildTrigger::Full);
    let reference = published(full.root());
    let differing: Vec<_> = reference.keys().chain(built.keys()).filter(|k| reference.get(*k) != built.get(*k)).collect();
    assert!(differing.is_empty(), "differs from a full render: {differing:?}");
}

fn rewrite_manifest(site: &Site, edit: impl FnOnce(&mut serde_json::Value)) {
    let path = MossPaths::new(site.root()).hashes();
    let mut manifest: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    edit(&mut manifest);
    fs::write(&path, serde_json::to_string(&manifest).unwrap()).unwrap();
}

/// Every HTML file the current generation serves, by path.
fn published(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let current = MossPaths::new(root).current_ptr();
    walkdir::WalkDir::new(&current).follow_links(true).into_iter().filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "html"))
        .map(|e| (e.path().strip_prefix(&current).unwrap().to_string_lossy().into_owned(), fs::read(e.path()).unwrap()))
        .collect()
}

fn copy_source(from: &Path, to: &Path) {
    for entry in walkdir::WalkDir::new(from).into_iter().filter_entry(|e| e.file_name() != "build.nosync").filter_map(Result::ok) {
        let rel = entry.path().strip_prefix(from).unwrap();
        let dest = to.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&dest).unwrap();
        } else if entry.file_type().is_file() {
            fs::copy(entry.path(), &dest).unwrap();
            // A sync client keeps each file's mtime; `fs::copy` does on macOS
            // but not on Linux.
            let mtime = filetime::FileTime::from_last_modification_time(&entry.metadata().unwrap());
            filetime::set_file_mtime(&dest, mtime).unwrap();
        }
    }
}

/// Two sections of 80 words each: enough for the floating island to list.
fn long_page(title: &str) -> String {
    let words = "The harbour keeps its own hours and the boats come in with the tide. ".repeat(12);
    format!("---\ntitle: {title}\n---\n\n## Morning\n\n{words}\n\n## Evening\n\n{words}\n")
}

/// `[site].floating_nav` changes only the pages that can show the island, so a
/// watch save of `config.toml` that flips it renders those and carries the
/// rest; any other change to the file still renders everything.
#[test]
fn a_floating_nav_toggle_renders_only_the_pages_it_changes() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let site = Site::new();
    site.write("notes/long.md", &long_page("Long"));
    site.sealed();
    let config = site.root().join(".moss/config.toml");
    let save = |text: &str| {
        site.write(".moss/config.toml", text);
        build(site.root(), BuildTrigger::ContentOnly(vec![config.clone()]))
    };

    for flag in ["true", "false"] {
        let record = save(&format!("[site]\ndomain = \"example.com\"\nfloating_nav = {flag}\n"));
        assert_eq!(count(&record, "pages_rendered"), 1, "floating_nav = {flag}: {record}");
        assert!(count(&record, "pages_carried") > 0, "floating_nav = {flag}: {record}");
        assert_publishes_a_full_render(&site);
    }

    let record = save("[site]\ndomain = \"example.org\"\nfloating_nav = false\n");
    assert!(rendered_everything(&record), "{record}");
    assert_publishes_a_full_render(&site);
}

fn deployment_state(at: &str, url: &str, site_url: &str) -> String {
    format!(
        "[deployment]\nsite_id = \"site-1\"\n\n[deployment.observed]\nchecked_at = \"{at}\"\n\n\
         [deployment.observed.dns_target]\nrecords = [{{ record_type = \"A\", name = \"@\", value = \"{url}\" }}]\n\n\
         [deployment.targets.\"moss:site-1\"]\nlast_deployment_at = \"{at}\"\nlast_deployment_url = \"https://{url}\"\nsite_url = \"https://{site_url}\"\n"
    )
}

/// A publish rewrites its own bookkeeping in `.moss/state.toml` (when and where
/// it published, the DNS it saw). No page reads it, so the build after it stops
/// at entry and serves what a full render does. The address the site is
/// published under still renders every page.
#[test]
fn a_publish_rewriting_its_bookkeeping_leaves_the_last_seal_standing() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let site = Site::new();
    site.write(".moss/state.toml", &deployment_state("2026-11-01T00:00:00Z", "a.example.net", "site-1.example.org"));
    site.sealed();

    site.write(".moss/state.toml", &deployment_state("2026-11-02T00:00:00Z", "b.example.net", "site-1.example.org"));
    let record = build(site.root(), BuildTrigger::Unobserved);
    assert!(stopped(&record), "{record}");
    assert_publishes_a_full_render(&site);

    site.write(".moss/state.toml", &deployment_state("2026-11-02T00:00:00Z", "b.example.net", "site-2.example.org"));
    let record = build(site.root(), BuildTrigger::Unobserved);
    assert!(rendered_everything(&record), "{record}");
    assert_publishes_a_full_render(&site);
}

/// A publish of the sealed generation writes its record and names it in
/// `.moss/state.toml`. The build after a publish renders the generation it
/// published, so the next build stops at entry and serves what a full render
/// does. A publish of any other generation renders every page.
#[test]
fn a_publish_of_the_sealed_generation_leaves_the_last_seal_standing() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let site = Site::new();
    site.write(".moss/state.toml", &deployment_state("2026-11-01T00:00:00Z", "a.example.net", "site-1.example.org"));
    site.sealed();
    let paths = MossPaths::new(site.root());
    let manifest: serde_json::Value = serde_json::from_str(&fs::read_to_string(paths.hashes()).unwrap()).unwrap();
    let files: std::collections::HashMap<String, String> = serde_json::from_value(manifest["files"].clone()).unwrap();
    let publish = |generation: &str| {
        let record = serde_json::json!({
            "generation_id": generation, "target": "moss:site-1", "published_at": "2026-11-02T00:00:00Z",
            "sources": {}, "source_to_output": {}, "files": files,
        });
        site.write(moss_build::build::manifest::published_record::path_for(&paths, "moss:site-1"), &record.to_string());
        site.write(".moss/state.toml", &format!("{}last_deployed_generation_id = \"{generation}\"\n", deployment_state("2026-11-02T00:00:00Z", "a.example.net", "site-1.example.org")));
    };

    publish(&moss_build::build::assets::paths::compute_manifest_generation_id(&files));
    let record = build(site.root(), BuildTrigger::Unobserved);
    assert!(stopped(&record), "{record}");
    assert_publishes_a_full_render(&site);

    publish("another-generation");
    let record = build(site.root(), BuildTrigger::Unobserved);
    assert!(rendered_everything(&record), "{record}");
    assert_publishes_a_full_render(&site);
}
