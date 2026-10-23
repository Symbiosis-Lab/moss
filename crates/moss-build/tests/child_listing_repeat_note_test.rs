//! A page whose body embeds a listing and whose own child listing shows the same
//! pages gets one note naming the count, unless `children: false` turns the child
//! listing off. Its own binary and one test, because the logger is process-wide.

use std::fs;
use std::path::Path;
use std::sync::Mutex;

struct Capture(Mutex<Vec<(String, String)>>);

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        self.0.lock().unwrap().push((record.target().to_string(), record.args().to_string()));
    }
    fn flush(&self) {}
}

static CAPTURE: Capture = Capture(Mutex::new(Vec::new()));

fn repeat_notes() -> Vec<String> {
    CAPTURE
        .0
        .lock()
        .unwrap()
        .drain(..)
        .filter(|(target, msg)| target.ends_with("#note") && msg.contains("listed both"))
        .map(|(_, msg)| msg)
        .collect()
}

fn build_sync(folder_path: &str) -> Result<String, String> {
    use moss_build::build::{run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
    use moss_build::cli::host::cli_host_ports;
    use moss_build::vault_root::VaultRoot;

    let source = std::path::PathBuf::from(folder_path);
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

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

/// Builds a site whose home page has `home_extra` frontmatter and `home_body`,
/// and returns the repeat notes the build printed.
fn repeat_notes_for(home_extra: &str, home_body: &str, pages: &[(&str, &str)]) -> Vec<String> {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(root, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n");
    write(root, "index.md", &format!("---\ntitle: Home\n{home_extra}---\n\n{home_body}\n"));
    for (rel, body) in pages {
        write(root, rel, body);
    }
    let result = build_sync(&root.to_string_lossy());
    assert!(result.is_ok(), "build failed: {result:?}");
    repeat_notes()
}

#[test]
fn a_home_whose_embed_repeats_its_child_listing_gets_one_note() {
    log::set_logger(&CAPTURE).unwrap();
    log::set_max_level(log::LevelFilter::Info);

    let apples = ("apples.md", "---\ntitle: Apples\ntags: [fruit]\n---\n\nCrisp.\n");
    let pears = ("pears.md", "---\ntitle: Pears\ntags: [fruit]\n---\n\nSoft.\n");
    let embed = "![[/tags/fruit/|style:grid]]";

    let notes = repeat_notes_for("", embed, &[apples, pears]);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0].contains("2 pages are listed both by this page's embeds and by its automatic child listing below them; set `children: false`"),
        "{notes:?}"
    );

    let notes = repeat_notes_for("children: false\n", embed, &[apples, pears]);
    assert!(notes.is_empty(), "{notes:?}");

    let old = ("archive/old.md", "---\ntitle: Old\n---\n\nOld.\n");
    let notes = repeat_notes_for("children_depth: direct\n", "![[/archive/]]", &[apples, old]);
    assert!(notes.is_empty(), "{notes:?}");

    // A folder home takes the same path, through its own child listing.
    let folder_a = ("folder/a.md", "---\ntitle: A\ntags: [fruit]\n---\n\nA.\n");
    let folder_b = ("folder/b.md", "---\ntitle: B\ntags: [fruit]\n---\n\nB.\n");
    let folder_home = ("folder/index.md", "---\ntitle: Folder\n---\n\n![[/tags/fruit/|style:grid]]\n");
    let notes = repeat_notes_for("children: false\n", "", &[folder_home, folder_a, folder_b]);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("[folder/index.md] 2 pages are listed both"), "{notes:?}");

    // A capped embed repeats only the pages it shows.
    let notes = repeat_notes_for("", "![[/tags/fruit/|style:grid,limit:1]]", &[apples, pears]);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("1 page is listed both"), "{notes:?}");
}
