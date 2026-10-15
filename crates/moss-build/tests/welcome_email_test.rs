//! `welcome-email.md` at a language-tree root is parsed but never a page; the
//! build writes it to `.moss-email/welcome.<edition>.json` for the hosting
//! server to send to new subscribers.

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

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}


static WARNINGS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
struct Capture;
impl log::Log for Capture {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Warn
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            WARNINGS.lock().unwrap().push(r.args().to_string());
        }
    }
    fn flush(&self) {}
}

const HOSTED: &str = "[deployment]\nsite_id = \"harbor-notes\"\ndeploy_method = \"moss\"\n";

fn build(files: &[(&str, &str)]) -> (Cleanup, PathBuf) {
    build_with_state(files, Some(HOSTED))
}

fn build_with_state(files: &[(&str, &str)], state: Option<&str>) -> (Cleanup, PathBuf) {
    let _ = log::set_logger(&Capture).map(|()| log::set_max_level(log::LevelFilter::Warn));
    let tmp = std::env::temp_dir().join(format!("moss_welcome_email_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let cleanup = Cleanup(tmp.clone());
    write(&tmp, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n");
    write(&tmp, "index.md", "---\ntitle: Harbor Notes\n---\n\nHello.\n");
    if let Some(state) = state {
        write(&tmp, ".moss/state.toml", state);
    }
    for (rel, body) in files {
        write(&tmp, rel, body);
    }
    let result = build_sync(&tmp.to_string_lossy());
    assert!(result.is_ok(), "build failed: {result:?}");
    let stage = tmp.join(".moss/build.nosync/staging");
    (cleanup, stage)
}

fn json(stage: &Path, name: &str) -> serde_json::Value {
    let raw = fs::read_to_string(stage.join(".moss-email").join(name))
        .unwrap_or_else(|e| panic!("{name} missing: {e}"));
    serde_json::from_str(&raw).unwrap()
}

const ROOT_MAIL: &str = "---\nsubject: Welcome aboard\npreview: Glad you are here\nfrom: hello@example.com\n---\n\nHi {{email}},\n\nThanks for joining *{{site_name}}*.\n";
const ZH_MAIL: &str = "---\nsubject: 歡迎\nreply_to: me@example.com\n---\n\n你好\n";

#[test]
fn welcome_files_become_json_and_never_pages() {
    let (_c, stage) = build(&[
        ("welcome-email.md", ROOT_MAIL),
        ("zh-hans/welcome-email.md", ZH_MAIL),
        ("zh-hans/index.md", "---\ntitle: 首页\n---\n\n你好\n"),
        ("docs/welcome-email.md", "---\ntitle: Welcome Docs\n---\n\nAn ordinary page.\n"),
    ]);

    let root = json(&stage, "welcome.default.json");
    assert_eq!(root["subject"], "Welcome aboard");
    assert_eq!(root["preview"], "Glad you are here");
    assert_eq!(root["from"], "hello@example.com");
    assert!(root["reply_to"].is_null());
    assert_eq!(
        root["body"],
        "Hi {{email}},\n\nThanks for joining *{{site_name}}*.",
        "the body is the markdown after the frontmatter, unrendered"
    );

    let zh = json(&stage, "welcome.zh-hans.json");
    assert_eq!(zh["subject"], "歡迎");
    assert!(zh["preview"].is_null() && zh["from"].is_null());
    assert_eq!(zh["reply_to"], "me@example.com");

    assert!(!stage.join("welcome-email/index.html").exists());
    assert!(!stage.join("zh-hans/welcome-email/index.html").exists());
    assert!(stage.join("docs/welcome-email/index.html").exists(), "a nested file is an ordinary page");
    let home = fs::read_to_string(stage.join("index.html")).unwrap();
    for href in ["href=\"welcome-email", "href=\"/welcome-email", "zh-hans/welcome-email"] {
        assert!(!home.contains(href), "no nav item or link for the slot file ({href})");
    }
    let sitemap = fs::read_to_string(stage.join("sitemap.xml")).unwrap_or_default();
    assert!(!sitemap.contains("mosspub.com/welcome-email/") && !sitemap.contains("zh-hans/welcome-email/"), "{sitemap}");
}

#[test]
fn welcome_file_does_not_fill_the_footer() {
    let (_c, stage) = build(&[("welcome-email.md", ROOT_MAIL)]);
    let home = fs::read_to_string(stage.join("index.html")).unwrap();
    assert!(!home.contains("Thanks for joining"), "{home}");
}

#[test]
fn welcome_file_without_subject_emits_nothing_and_warns() {
    let (_c, stage) = build(&[("welcome-email.md", "---\npreview: x\n---\n\nBody\n")]);
    assert!(!stage.join(".moss-email/welcome.default.json").exists());
    let warnings = WARNINGS.lock().unwrap();
    assert!(warnings.iter().any(|w| w.contains("welcome-email.md") && w.contains("subject")), "{warnings:?}");
}

#[test]
fn a_site_not_on_the_moss_host_emits_nothing() {
    let (_c, stage) = build_with_state(&[("welcome-email.md", ROOT_MAIL)], None);
    assert!(!stage.join(".moss-email").exists(), "any other host would publish the template");
    assert!(!stage.join("welcome-email/index.html").exists());
}

#[test]
fn comments_in_the_body_do_not_reach_subscribers() {
    let (_c, stage) = build(&[(
        "welcome-email.md",
        "---\nsubject: Hi\n---\n\nHello %%private note%% there.\n",
    )]);
    let body = json(&stage, "welcome.default.json")["body"].as_str().unwrap().to_string();
    assert!(!body.contains("private note"), "{body}");
    assert!(body.contains("Hello"), "{body}");
}

#[test]
fn a_subject_that_is_not_text_gets_its_own_warning() {
    let (_c, stage) = build(&[("zh-hans/welcome-email.md", "---\nsubject: 2024\n---\n\nBody\n"), ("zh-hans/index.md", "---\ntitle: x\n---\n\nx\n")]);
    assert!(!stage.join(".moss-email/welcome.zh-hans.json").exists());
    let warnings = WARNINGS.lock().unwrap();
    assert!(warnings.iter().any(|w| w.contains("zh-hans/welcome-email.md") && w.contains("must be text")), "{warnings:?}");
}

#[test]
fn removing_the_file_removes_the_output_on_the_next_build() {
    let (c, stage) = build(&[("welcome-email.md", ROOT_MAIL)]);
    assert!(stage.join(".moss-email/welcome.default.json").exists());
    let root = c.0.clone();
    assert!(root.join("welcome-email.md").exists());
    fs::remove_file(root.join("welcome-email.md")).unwrap();
    let result = build_sync(&root.to_string_lossy());
    assert!(result.is_ok(), "{result:?}");
    assert!(
        !stage.join(".moss-email/welcome.default.json").exists(),
        "a deleted letter must not linger in the output"
    );
    drop(c);
}
