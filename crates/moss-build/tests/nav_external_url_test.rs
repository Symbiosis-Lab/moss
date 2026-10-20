//! A nav entry whose page declares `external_url:` links there: a `mailto:`
//! address or a section anchor, beside ordinary page links. Built through the
//! real pipeline, and one test only because `log::set_logger` is
//! process-global and one-shot (same reasoning as `root_identity_log_test.rs`).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

struct Capture(Mutex<Vec<String>>);

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        self.0.lock().unwrap().push(record.args().to_string());
    }
    fn flush(&self) {}
}

static CAPTURE: Capture = Capture(Mutex::new(Vec::new()));

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

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

#[test]
fn nav_entry_with_external_url_links_there_without_an_ignored_warning() {
    log::set_logger(&CAPTURE).unwrap();
    log::set_max_level(log::LevelFilter::Warn);

    let tmp = std::env::temp_dir().join(format!("moss_nav_ext_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(tmp.clone());

    write(&tmp, ".moss/config.toml", "[site]\ndomain = \"example.com\"\n");
    write(&tmp, "index.md", "---\ntitle: Home\n---\n\nWelcome.\n");
    write(&tmp, "about.md", "---\ntitle: About\nnav: true\nweight: 1\n---\n\nAbout us.\n");
    write(
        &tmp,
        "services.md",
        "---\ntitle: Services\nnav: true\nweight: 2\nexternal_url: \"/#services\"\n---\n\nSee the home page.\n",
    );
    write(
        &tmp,
        "contact.md",
        "---\ntitle: Contact\nnav_label: Write to us\nnav: true\nweight: 3\nexternal_url: \"mailto:hello@example.com\"\n---\n\nEmail us.\n",
    );

    let result = build_sync(&tmp.to_string_lossy());
    assert!(result.is_ok(), "build failed: {result:?}");
    let html = fs::read_to_string(tmp.join(".moss/build.nosync/staging/index.html")).unwrap();

    // Attributes after `href` (source markers) are the build's business, not this test's.
    let link = |href: &str, label: &str| {
        html.split("<a ").any(|a| a.starts_with(&format!(r#"href="{href}""#)) && a.contains(&format!(">{label}</a>")))
    };
    assert!(link("/about/", "About"), "{html}");
    assert!(link("/#services", "Services"), "{html}");
    assert!(link("mailto:hello@example.com", "Write to us"), "{html}");
    assert!(!html.contains(r#"href="/contact/""#) && !html.contains(r#"href="/services/""#), "{html}");

    let warnings = CAPTURE.0.lock().unwrap().join("\n");
    assert!(!warnings.contains("will be ignored"), "unexpected warning: {warnings}");
}
