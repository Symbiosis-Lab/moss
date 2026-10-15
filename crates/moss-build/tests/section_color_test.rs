//! A folder's `color:` is its section's accent: the pages under the folder
//! carry `--moss-section-color` and `data-section` on `<body>`, and a listing
//! card for such a page carries the property too. A site that sets no `color:`
//! builds as before.

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

fn build_sync(folder_path: &str, trigger: moss_build::build::BuildTrigger) -> Result<String, String> {
    use moss_build::build::{run_pipeline, PipelineConfig, PluginMode};
    use moss_build::cli::host::cli_host_ports;
    use moss_build::vault_root::VaultRoot;

    let _ = log::set_logger(&CAPTURE);
    log::set_max_level(log::LevelFilter::Warn);
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
    }))
}

struct Site(PathBuf);

impl Site {
    fn new(files: &[(&str, &str)]) -> Site {
        let root = std::env::temp_dir().join(format!("moss_section_color_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let site = Site(root);
        site.write(".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n");
        for (rel, body) in files {
            site.write(rel, body);
        }
        let result = build_sync(&site.0.to_string_lossy(), moss_build::build::BuildTrigger::Full);
        assert!(result.is_ok(), "build failed: {result:?}");
        site
    }

    fn write(&self, rel: &str, body: &str) {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn html(&self, rel: &str) -> String {
        fs::read_to_string(self.0.join(".moss/build.nosync/staging").join(rel)).unwrap()
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn body_tag(html: &str) -> &str {
    let start = html.find("<body").expect("a body tag");
    &html[start..start + html[start..].find('>').unwrap() + 1]
}

fn page(title: &str) -> String {
    format!("---\ntitle: {title}\n---\n\nText.\n")
}

fn folder_home(title: &str, color: Option<&str>) -> String {
    match color {
        Some(c) => format!("---\ntitle: {title}\ncolor: \"{c}\"\n---\n\nText.\n"),
        None => page(title),
    }
}

#[test]
fn a_folders_colour_reaches_its_home_and_its_pages() {
    let site = Site::new(&[
        ("index.md", &page("Home")),
        ("work/index.md", &folder_home("Work", Some("#a3312a"))),
        ("work/piece.md", &page("Piece")),
        ("notes/index.md", &folder_home("Notes", None)),
        ("notes/entry.md", &page("Entry")),
    ]);
    for rel in ["work/index.html", "work/piece/index.html"] {
        let body = body_tag(&site.html(rel)).to_string();
        assert!(body.contains(r#"data-section="work""#), "{rel}: {body}");
        assert!(body.contains("--moss-section-color: #a3312a"), "{rel}: {body}");
    }
    for rel in ["index.html", "notes/entry/index.html"] {
        let body = body_tag(&site.html(rel)).to_string();
        assert!(!body.contains("data-section") && !body.contains("--moss-section-color"), "{rel}: {body}");
    }
}

#[test]
fn a_nested_folders_own_colour_wins() {
    let site = Site::new(&[
        ("index.md", &page("Home")),
        ("work/index.md", &folder_home("Work", Some("#a3312a"))),
        ("work/film/index.md", &folder_home("Film", Some("#123"))),
        ("work/film/reel.md", &page("Reel")),
        ("work/sketch.md", &page("Sketch")),
    ]);
    let reel = body_tag(&site.html("work/film/reel/index.html")).to_string();
    assert!(reel.contains(r#"data-section="film""#) && reel.contains("--moss-section-color: #123"), "{reel}");
    let sketch = body_tag(&site.html("work/sketch/index.html")).to_string();
    assert!(sketch.contains(r#"data-section="work""#) && sketch.contains("#a3312a"), "{sketch}");
}

#[test]
fn a_listing_card_for_a_coloured_page_carries_the_property() {
    let site = Site::new(&[
        ("index.md", &page("Home")),
        ("work/index.md", &folder_home("Work", Some("#a3312a"))),
        ("work/piece.md", &page("Piece")),
        ("notes/index.md", &folder_home("Notes", None)),
        ("notes/entry.md", &page("Entry")),
    ]);
    let cards = |rel: &str, needle: &str| -> Vec<String> {
        let html = site.html(rel);
        html.split("<a ")
            .filter(|chunk| chunk.contains("class=\"moss-card\"") && chunk.contains(needle))
            .map(|chunk| chunk.split('>').next().unwrap().to_string())
            .collect()
    };
    let home_cards = cards("work/index.html", "/work/piece/");
    assert!(
        home_cards.iter().any(|c| c.contains("--moss-section-color: #a3312a")),
        "card for a coloured page: {home_cards:?}"
    );
    let plain = cards("notes/index.html", "/notes/entry/");
    assert!(plain.iter().all(|c| !c.contains("--moss-section-color")), "{plain:?}");
}

#[test]
fn an_invalid_colour_warns_once_and_adds_nothing() {
    let site = Site::new(&[
        ("index.md", &page("Home")),
        ("loud/index.md", &folder_home("Loud", Some("crimson"))),
        ("loud/piece.md", &page("Piece")),
    ]);
    let warned = CAPTURE
        .0
        .lock()
        .unwrap()
        .iter()
        .filter(|w| w.contains("loud/index.md") && w.contains("crimson"))
        .count();
    assert_eq!(warned, 1);
    for rel in ["loud/index.html", "loud/piece/index.html"] {
        let body = body_tag(&site.html(rel)).to_string();
        assert!(!body.contains("data-section") && !body.contains("style="), "{rel}: {body}");
    }
}

#[test]
fn a_site_without_colour_has_no_section_markup() {
    let site = Site::new(&[("index.md", &page("Home")), ("posts/first.md", &page("First"))]);
    for rel in ["index.html", "posts/first/index.html", "posts/index.html"] {
        let html = site.html(rel);
        assert!(!html.contains("data-section") && !html.contains("--moss-section-color"), "{rel}");
    }
}

#[test]
fn changing_only_a_folders_colour_restyles_its_pages_on_an_incremental_build() {
    let site = Site::new(&[
        ("index.md", &page("Home")),
        ("work/index.md", &folder_home("Work", Some("#a3312a"))),
        ("work/piece.md", &page("Piece")),
    ]);
    assert!(body_tag(&site.html("work/piece/index.html")).contains("#a3312a"));

    site.write("work/index.md", &folder_home("Work", Some("#00aa55")));
    let changed = site.0.join("work/index.md");
    let rebuilt = build_sync(&site.0.to_string_lossy(), moss_build::build::BuildTrigger::ContentOnly(vec![changed]));
    assert!(rebuilt.is_ok(), "{rebuilt:?}");

    let body = body_tag(&site.html("work/piece/index.html")).to_string();
    assert!(body.contains("--moss-section-color: #00aa55") && !body.contains("#a3312a"), "{body}");
}
