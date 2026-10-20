//! A throwaway site built from inline files, and what its build wrote and logged.
#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;
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

/// The logger is global to the test binary, so builds take turns: each one
/// owns the capture from its first log line to its last.
static SEQUENTIAL: Mutex<()> = Mutex::new(());

/// Build `folder_path` and return what it logged at warn level and above.
pub fn build_sync(folder_path: &str, trigger: moss_build::build::BuildTrigger) -> Result<Vec<String>, String> {
    use moss_build::build::{run_pipeline, PipelineConfig, PluginMode};
    use moss_build::cli::host::cli_host_ports;
    use moss_build::vault_root::VaultRoot;

    let _turn = SEQUENTIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _ = log::set_logger(&CAPTURE);
    log::set_max_level(log::LevelFilter::Warn);
    CAPTURE.0.lock().unwrap().clear();
    let source = PathBuf::from(folder_path);
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(run_pipeline(PipelineConfig {
        root: VaultRoot::resolve(&source),
        progress: moss_build::build::stdout_sink(),
        plugins: PluginMode::Skip,
        watch: false,
        start_server: false,
        host: cli_host_ports(folder_path),
        trigger,
        exits_after_build: true,
        site_url_override: None,
        server_port: None,
        admission_epoch: None,
        live_port: None,
    }))?;
    Ok(std::mem::take(&mut *CAPTURE.0.lock().unwrap()))
}

pub struct Site {
    pub root: PathBuf,
    /// What the first build logged at warn level and above.
    pub logged: Vec<String>,
}

impl Site {
    pub fn new(files: &[(&str, &str)]) -> Site {
        Site::with_config("", files)
    }

    /// A site with `lang = "en"` and `extra_config` after its `[site]` table.
    pub fn with_config(extra_config: &str, files: &[(&str, &str)]) -> Site {
        Site::in_language("en", extra_config, files)
    }

    pub fn in_language(lang: &str, extra_config: &str, files: &[(&str, &str)]) -> Site {
        let root = std::env::temp_dir().join(format!("moss_inline_site_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let mut site = Site { root, logged: Vec::new() };
        site.write(".moss/config.toml", &format!("schema_version = 6\n\n[site]\nlang = \"{lang}\"\n{extra_config}"));
        for (rel, body) in files {
            site.write(rel, body);
        }
        site.logged = site.rebuild();
        site
    }

    pub fn rebuild(&self) -> Vec<String> {
        build_sync(&self.root.to_string_lossy(), moss_build::build::BuildTrigger::Full)
            .unwrap_or_else(|e| panic!("build failed: {e}"))
    }

    pub fn write(&self, rel: &str, body: &str) {
        let path = self.root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    /// The path a build wrote `rel` at.
    pub fn output(&self, rel: &str) -> PathBuf {
        self.root.join(".moss/build.nosync/staging").join(rel)
    }

    pub fn html(&self, rel: &str) -> String {
        fs::read_to_string(self.output(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
    }

    /// The links of the page's `<nav>`, and of its `<main>`.
    pub fn links(&self, rel: &str) -> (Vec<String>, Vec<String>) {
        let html = self.html(rel);
        let between = |open: &str, close: &str| {
            let from = html.find(open).unwrap_or_else(|| panic!("no {open} in {rel}"));
            let to = html[from..].find(close).unwrap() + from;
            hrefs_in(&html[from..to]).into_iter().map(str::to_string).collect()
        };
        (between("<nav", "</nav>"), between("<main", "</main>"))
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub fn hrefs_in(html: &str) -> Vec<&str> {
    html.split("href=\"").skip(1).filter_map(|p| p.split('"').next()).collect()
}
