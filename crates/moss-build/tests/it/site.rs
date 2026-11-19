//! A throwaway site built from inline files, and what its build wrote and logged.
#![allow(dead_code)]

use moss_build::moss_paths::MossPaths;
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

struct Capture(Mutex<Vec<String>>);

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        self.0.lock().unwrap().push(record.args().to_string());
        if record.target().ends_with("#note") {
            NOTES.lock().unwrap().push(record.args().to_string());
        }
        // What the CLI counts as a build problem: every warning that is not a note.
        if record.level() == log::Level::Warn && !record.target().ends_with("#note") {
            PROBLEMS.lock().unwrap().push(record.args().to_string());
        }
    }
    fn flush(&self) {}
}

static CAPTURE: Capture = Capture(Mutex::new(Vec::new()));
static NOTES: Mutex<Vec<String>> = Mutex::new(Vec::new());
static PROBLEMS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// What one build logged, read while it still held its turn: the global
/// capture belongs to another test's build the moment the turn ends.
pub struct Built {
    /// Every line at warn level and above.
    pub logged: Vec<String>,
    pub notes: Vec<String>,
    /// What the CLI counts as a build problem: every warning that is not a note.
    pub problems: Vec<String>,
    /// The process-wide count `--strict` fails on, as this build alone moved it.
    pub cli_problems: usize,
}

static TEST_HOME: std::sync::LazyLock<tempfile::TempDir> = std::sync::LazyLock::new(|| {
    let home = tempfile::tempdir().unwrap();
    if std::env::var_os("MOSS_HOME").is_none() {
        std::env::set_var("MOSS_HOME", home.path());
    }
    home
});

/// The logger is global to the test binary, so a build whose log a test reads
/// runs alone: it holds the write side from its first log line to its last.
/// Every other build takes the read side and runs beside the rest. Every build
/// in this binary goes through [`run`], which asks for a turn
/// (`run_pipeline_only_through_site_run` checks the files), or its warnings can
/// land in a reading build's capture.
static SEQUENTIAL: RwLock<()> = RwLock::new(());

enum Guard {
    Quiet(RwLockReadGuard<'static, ()>),
    Reading(RwLockWriteGuard<'static, ()>),
}

/// A turn at the capture. A test whose build leaves work running (a watch, a
/// derive it does not await) holds its turn until that work is done.
pub struct Turn {
    _guard: Guard,
}

/// A turn whose builds no other build overlaps, so the capture is theirs.
struct Reading(Turn);

fn ready_logger() {
    std::sync::LazyLock::force(&TEST_HOME);
    let _ = log::set_logger(&CAPTURE);
    log::set_max_level(log::LevelFilter::Warn);
}

/// A turn for builds whose log nobody reads; they run in parallel.
pub fn turn() -> Turn {
    let guard = SEQUENTIAL.read().unwrap_or_else(PoisonError::into_inner);
    ready_logger();
    Turn { _guard: Guard::Quiet(guard) }
}

fn reading_turn() -> Reading {
    let guard = SEQUENTIAL.write().unwrap_or_else(PoisonError::into_inner);
    ready_logger();
    CAPTURE.0.lock().unwrap().clear();
    PROBLEMS.lock().unwrap().clear();
    NOTES.lock().unwrap().clear();
    let _ = moss_build::build::cli_output::take_cli_problems();
    Reading(Turn { _guard: Guard::Reading(guard) })
}

impl Reading {
    fn take(&self) -> Built {
        Built {
            logged: std::mem::take(&mut *CAPTURE.0.lock().unwrap()),
            notes: std::mem::take(&mut *NOTES.lock().unwrap()),
            problems: std::mem::take(&mut *PROBLEMS.lock().unwrap()),
            cli_problems: moss_build::build::cli_output::take_cli_problems(),
        }
    }
}

fn full_config(folder_path: &str, trigger: moss_build::build::BuildTrigger) -> moss_build::build::PipelineConfig {
    use moss_build::build::{PipelineConfig, PluginMode};
    use moss_build::cli::host::cli_host_ports;
    use moss_build::vault_root::VaultRoot;

    PipelineConfig {
        root: VaultRoot::resolve(&PathBuf::from(folder_path)),
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
        rename_pairs: Vec::new(),
    }
}

fn build_in(turn: &Turn, folder_path: &str, trigger: moss_build::build::BuildTrigger) -> Result<String, String> {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(run(turn, full_config(folder_path, trigger)))
}

/// Build `folder_path`, beside any other build whose log nobody reads.
pub fn build_sync(folder_path: &str, trigger: moss_build::build::BuildTrigger) -> Result<(), String> {
    build_in(&turn(), folder_path, trigger).map(drop)
}

/// A full build of `folder_path`, the way the CLI runs one.
pub fn build_full(folder_path: &str) -> Result<(), String> {
    build_sync(folder_path, moss_build::build::BuildTrigger::Full)
}

/// Build `folder_path` alone, and return what it logged.
pub fn build_logged(folder_path: &str, trigger: moss_build::build::BuildTrigger) -> Result<Built, String> {
    let reading = reading_turn();
    build_in(&reading.0, folder_path, trigger)?;
    Ok(reading.take())
}

/// The one way a test in this binary runs the pipeline: only while it holds
/// a turn.
pub async fn run(_turn: &Turn, config: moss_build::build::PipelineConfig) -> Result<String, String> {
    moss_build::build::run_pipeline(config).await
}

pub struct Site {
    pub root: PathBuf,
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
        let site = Site::unbuilt(lang, extra_config, files);
        site.rebuild();
        site
    }

    fn unbuilt(lang: &str, extra_config: &str, files: &[(&str, &str)]) -> Site {
        let root = std::env::temp_dir().join(format!("moss_inline_site_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let site = Site { root };
        site.write(".moss/config.toml", &format!("schema_version = 6\n\n[site]\nlang = \"{lang}\"\n{extra_config}"));
        for (rel, body) in files {
            site.write(rel, body);
        }
        site
    }

    pub fn rebuild(&self) {
        build_full(&self.root.to_string_lossy()).unwrap_or_else(|e| panic!("build failed: {e}"));
    }

    pub fn rebuild_logged(&self) -> Built {
        build_logged(&self.root.to_string_lossy(), moss_build::build::BuildTrigger::Full)
            .unwrap_or_else(|e| panic!("build failed: {e}"))
    }

    pub fn write(&self, rel: &str, body: &str) {
        let path = self.root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    /// The path a build wrote `rel` at.
    pub fn output(&self, rel: &str) -> PathBuf {
        MossPaths::new(&self.root).staging_dir().join(rel)
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

/// A site built alone, with what its first build logged.
pub struct LoggedSite {
    pub site: Site,
    /// Every line at warn level and above.
    pub logged: Vec<String>,
    pub notes: Vec<String>,
    /// What the CLI counts as a build problem: every warning that is not a note.
    pub problems: Vec<String>,
    pub cli_problems: usize,
}

impl LoggedSite {
    pub fn new(files: &[(&str, &str)]) -> LoggedSite {
        LoggedSite::with_config("", files)
    }

    pub fn with_config(extra_config: &str, files: &[(&str, &str)]) -> LoggedSite {
        LoggedSite::in_language("en", extra_config, files)
    }

    pub fn in_language(lang: &str, extra_config: &str, files: &[(&str, &str)]) -> LoggedSite {
        let site = Site::unbuilt(lang, extra_config, files);
        let Built { logged, notes, problems, cli_problems } = site.rebuild_logged();
        LoggedSite { site, logged, notes, problems, cli_problems }
    }
}

impl std::ops::Deref for LoggedSite {
    type Target = Site;
    fn deref(&self) -> &Site {
        &self.site
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

/// The page's opening hero section up to its first `</section>`, or "" when it has none.
pub fn hero_of(html: &str) -> String {
    let Some(from) = html.find("<section class=\"moss-hero") else { return String::new() };
    let to = html[from..].find("</section>").map_or(html.len(), |i| from + i);
    html[from..to].to_string()
}

#[test]
fn run_pipeline_only_through_site_run() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/it");
    let mut bypass: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n != "site.rs"))
        .filter(|p| fs::read_to_string(p).is_ok_and(|text| text.contains("run_pipeline(")))
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    bypass.sort();
    assert!(bypass.is_empty(), "these call run_pipeline outside a turn, so their warnings land in another test's capture: {bypass:?} (use site::build_full, or site::run with a held site::turn())");
}
