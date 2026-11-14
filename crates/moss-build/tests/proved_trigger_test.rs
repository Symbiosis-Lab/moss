//! A build that watched nothing (`moss build`, a watch session's first build)
//! proves from the last sealed manifest what changed since, and carries the
//! rest. Each guard below is one input no page's markdown shows; moving it must
//! render every page again.

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
        trigger,
        exits_after_build: true,
        site_url_override: site_url.map(str::to_string),
        server_port: None,
        admission_epoch: None,
        live_port: None,
        rename_pairs: Vec::new(),
    }))
    .unwrap_or_else(|e| panic!("build failed: {e}"));
    last_profile(root)
}

fn last_profile(root: &Path) -> serde_json::Value {
    let text = fs::read_to_string(root.join(".moss/build.nosync/profile.jsonl")).unwrap();
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
        // What a build writes when no page was renamed. A first build would
        // create it after measuring `.moss/`, and the build after that renders
        // in full once.
        site.write(".moss/redirects.json", "{}");
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

    fn write(&self, rel: &str, text: &str) {
        let path = self.root().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// One sealed build for the build under test to prove against.
    fn sealed(&self) -> &Self {
        build(self.root(), BuildTrigger::Unobserved);
        self
    }
}

fn rendered_everything(record: &serde_json::Value) -> bool {
    count(record, "pages_carried") == 0 && count(record, "pages_rendered") > 0
}

#[test]
fn a_build_that_watched_nothing_carries_every_page_when_nothing_changed() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let site = Site::new();
    site.sealed();
    let record = build(site.root(), BuildTrigger::Unobserved);
    assert_eq!(count(&record, "proved_from_seal"), 1, "{record}");
    assert_eq!(count(&record, "pages_rendered"), 0, "{record}");
    assert!(count(&record, "pages_carried") > 0, "{record}");
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
        ("deleted link cards", |s| fs::remove_dir_all(s.root().join(".moss/build.nosync/cache/link-meta")).unwrap()),
        ("a new image", |s| s.write("notes/pic.svg", "<svg xmlns=\"http://www.w3.org/2000/svg\"/>")),
        ("another moss binary", |s| rewrite_manifest(s, |m| m["builder_fingerprint"] = "an older moss".into())),
        ("a manifest with no input record", |s| rewrite_manifest(s, |m| { m.as_object_mut().unwrap().remove("build_inputs"); })),
    ];
    let setup = || {
        let site = Site::new();
        site.write(".moss/data/people.json", "[]\n");
        site.write(&format!(".moss/build.nosync/cache/link-meta/{}.json", "a".repeat(64)), "{}\n");
        site.sealed();
        site
    };
    // The control: with nothing moved the same setup proves, so each guard
    // below is what keeps its build full.
    let control = build(setup().root(), BuildTrigger::Unobserved);
    assert_eq!(count(&control, "proved_from_seal"), 1, "control: {control}");
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
    assert_eq!(count(&carried, "proved_from_seal"), 1, "{carried}");
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
/// every build moves a measured `.moss/` input after the build measured it, so
/// the next build cannot prove anything and renders every page. The same holds
/// for anything a build fetches and renders in one go: its record is the
/// state before the write.
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
    assert!(recorded_moss_input(&site, STATE).is_none(), "the first build measured before its hook wrote");
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
    build_with(control.root(), BuildTrigger::Unobserved, Skip, Some(A));
    let record = build_with(control.root(), BuildTrigger::Unobserved, Skip, Some(A));
    assert_eq!(count(&record, "proved_from_seal"), 1, "control: {record}");

    for next in [Some(B), None] {
        let site = Site::new();
        build_with(site.root(), BuildTrigger::Unobserved, Skip, Some(A));
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
    build_with(control.root(), BuildTrigger::Unobserved, Blocking, None);
    let record = build_with(control.root(), BuildTrigger::Unobserved, Blocking, None);
    assert_eq!(count(&record, "proved_from_seal"), 1, "control: {record}");

    let site = Site::new();
    build_with(site.root(), BuildTrigger::Unobserved, Skip, None);
    let record = build_with(site.root(), BuildTrigger::Unobserved, Blocking, None);
    assert_eq!(count(&record, "proved_from_seal"), 0, "{record}");
    assert!(rendered_everything(&record), "{record}");
}

/// Every build of a site with a comment server records the refresh's outcome,
/// which no page reads, so it must not count as an input.
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
        let record = build(site.root(), BuildTrigger::Unobserved);
        let written = fs::read_to_string(site.root().join(state)).expect("the refresh ran and recorded its outcome");
        assert!(written.contains("last_error") && !written.contains("\"last_error\": null"), "{written}");
        assert_eq!(count(&record, "proved_from_seal"), 1, "build {n}, recorded {state}: {:?}: {record}", recorded_moss_input(&site, state));
        assert_eq!(count(&record, "pages_rendered"), 0, "build {n}: {record}");
    }
}

fn recorded_moss_input(site: &Site, rel: &str) -> Option<serde_json::Value> {
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(site.root().join(".moss/build.nosync/hashes.json")).unwrap()).unwrap();
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
    let _ = fs::remove_dir_all(site.root().join(".moss/build.nosync/staging"));
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

fn rewrite_manifest(site: &Site, edit: impl FnOnce(&mut serde_json::Value)) {
    let path = site.root().join(".moss/build.nosync/hashes.json");
    let mut manifest: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    edit(&mut manifest);
    fs::write(&path, serde_json::to_string(&manifest).unwrap()).unwrap();
}

/// Every HTML file the current generation serves, by path.
fn published(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let current = root.join(".moss/build.nosync/current");
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
        }
    }
}
