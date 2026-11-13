//! `[services.booking]` belongs to the installed `booking` plugin once that
//! plugin declares `public_data`: moss's own reader then leaves the table alone.
//!
//! Its own test binary: `log::set_logger` is process-global and one-shot.
use moss_build::build::{run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
use serde_json::json;
use std::path::Path;
use std::sync::{LazyLock, Mutex};

static HOME: LazyLock<tempfile::TempDir> = LazyLock::new(|| {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("MOSS_HOME", home.path());
    home
});

/// Builds share one process-wide log capture, so they take turns.
static TURN: Mutex<()> = Mutex::new(());

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

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// Build `root` and return the log lines that mention `[services.booking]`.
fn build(root: &Path) -> Vec<String> {
    LazyLock::force(&HOME);
    let _ = log::set_logger(&CAPTURE);
    log::set_max_level(log::LevelFilter::Info);
    CAPTURE.0.lock().unwrap().clear();
    let result = tokio::runtime::Runtime::new().unwrap().block_on(run_pipeline(PipelineConfig {
        root: moss_build::vault_root::VaultRoot::resolve(root),
        progress: moss_build::build::stdout_sink(),
        plugins: PluginMode::SlotsOnly,
        watch: false,
        start_server: false,
        host: moss_build::cli::host::cli_host_ports(root.to_str().unwrap()),
        trigger: BuildTrigger::Full,
        exits_after_build: true,
        site_url_override: Some("https://example.com".into()),
        server_port: None,
        admission_epoch: None,
        live_port: None,
        rename_pairs: vec![],
    }));
    assert!(result.is_ok(), "{result:?}");
    let lines = CAPTURE.0.lock().unwrap().clone();
    lines.into_iter().filter(|l| l.contains("[services.booking]")).collect()
}

fn commerce(root: &Path) -> Option<String> {
    std::fs::read_to_string(root.join(".moss/build.nosync/current/_moss/commerce.json")).ok()
}

const OFFER: &str = r#"schema_version = 6
[services.booking]
timezone = "Europe/Berlin"
currency = "eur"
start_every = 30
notice_hours = 24
horizon_days = 60
[services.booking.services.walk]
name = "Walk"
minutes = 60
price = 4000
[services.booking.open]
mon = ["09:00-17:00"]
[services.booking.cancel]
full_before_hours = 48
late_percent = 50
[services.booking.pages]
book = "/book/"
done = "/book/done/"
manage = "/book/manage/"
"#;

fn site(config: &str) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), ".moss/config.toml", config);
    for page in ["index", "book", "book/done", "book/manage"] {
        write(tmp.path(), &format!("{page}.md"), "# Page\n");
    }
    tmp
}

fn booking_plugin(root: &Path) {
    let manifest = json!({"name":"booking","version":"1.0.0","contributes":{"public_data":{"keys":["currency","timezone","start_every","notice_hours","horizon_days","open.*","cancel.*","pages.*","services.*.name","services.*.minutes","services.*.price","services.*.add_ons.*"]}}});
    write(root, ".moss/plugins/booking/manifest.json", &manifest.to_string());
}

#[test]
fn a_site_without_the_plugin_keeps_core_reading_its_offer() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = site(OFFER);
    let lines = build(tmp.path());
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(commerce(tmp.path()).expect("commerce.json"), r#"{"booking":{"cancel":{"full_before_hours":48,"late_percent":50},"currency":"eur","horizon_days":60,"notice_hours":24,"open":{"mon":["09:00-17:00"]},"pages":{"book":"/book/","done":"/book/done/","manage":"/book/manage/"},"services":{"walk":{"minutes":60,"name":"Walk","price":4000}},"start_every":30,"timezone":"Europe/Berlin"},"forms":{}}"#);

    let extra = format!("{OFFER}[services.booking.services.walk.add_ons]\nmat = 500\n");
    let tmp = site(&extra);
    let lines = build(tmp.path());
    assert!(lines.iter().any(|l| l.contains("add_ons")), "an unknown key still warns without the plugin: {lines:?}");
    assert!(!commerce(tmp.path()).unwrap_or_default().contains("booking"));
}

#[test]
fn the_booking_plugin_owns_its_table_and_core_stays_out() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = site(&format!("{OFFER}[services.booking.services.walk.add_ons]\nmat = 500\n"));
    booking_plugin(tmp.path());
    let lines = build(tmp.path());
    assert_eq!(lines, ["[services.booking] is read by the booking plugin; moss's own booking reader is off"]);
    assert_eq!(commerce(tmp.path()), None, "with no forms and no booking object from core there is no commerce.json");
    let published = std::fs::read_to_string(tmp.path().join(".moss/build.nosync/current/_moss/extensions/booking.json")).unwrap();
    assert!(published.contains("Walk"), "{published}");
}

#[test]
fn a_plugin_that_publishes_nothing_does_not_take_the_table() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = site(OFFER);
    write(tmp.path(), ".moss/plugins/booking/manifest.json", &json!({"name":"booking","version":"1.0.0"}).to_string());
    assert!(build(tmp.path()).is_empty());
    assert!(commerce(tmp.path()).unwrap().contains("\"booking\""));
}

#[test]
fn a_plugin_of_another_name_does_not_take_the_booking_table() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = site(OFFER);
    let manifest = json!({"name":"calendar","version":"1.0.0","contributes":{"public_data":{"keys":["timezone"]}}});
    write(tmp.path(), ".moss/plugins/calendar/manifest.json", &manifest.to_string());
    assert!(build(tmp.path()).is_empty());
    assert!(commerce(tmp.path()).unwrap().contains("\"booking\""));
}
