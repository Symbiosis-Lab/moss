//! Core's `site_pages`, read from a folder's texts and bundle, must give every
//! page the facts the build gave it, on every snapshot fixture.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use moss_core::page::site_page::PageFacts;

/// What one build run knew once every page was read, keyed by site folder.
pub(crate) struct Captured {
    pub root_name: String,
    pub pages: BTreeMap<String, PageFacts>,
    pub folder_langs: BTreeMap<String, String>,
}

static CAPTURED: Mutex<Vec<(PathBuf, Captured)>> = Mutex::new(Vec::new());

pub(crate) fn record(
    root: &crate::vault::paths::VaultRoot,
    docs: &[crate::build::types::ParsedDocument],
    folder_langs: &HashMap<String, crate::i18n::Language>,
) {
    let pages = docs.iter().filter_map(|d| Some((d.source_path.clone()?, PageFacts::from_page(d)))).collect();
    let folder_langs = folder_langs.iter().map(|(k, v)| (k.clone(), v.code().to_string())).collect();
    let captured = Captured { root_name: root.name().to_string(), pages, folder_langs };
    if let Ok(mut all) = CAPTURED.lock() {
        all.push((root.path().to_path_buf(), captured));
    }
}

fn take(root: &Path) -> Option<Captured> {
    let mut all = CAPTURED.lock().ok()?;
    let i = all.iter().position(|(p, _)| p == root)?;
    Some(all.swap_remove(i).1)
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let dest = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &dest);
        } else {
            std::fs::copy(entry.path(), dest).unwrap();
        }
    }
}

fn site_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            site_files(root, &path, out);
        } else if let Ok(rel) = path.strip_prefix(root) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

fn build(folder: &Path) {
    use crate::build::{run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
    let path = folder.to_string_lossy().to_string();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _ = rt.block_on(run_pipeline(PipelineConfig {
        root: crate::vault_root::VaultRoot::resolve(folder),
        progress: crate::build::stdout_sink(),
        plugins: PluginMode::Skip,
        watch: false,
        start_server: false,
        host: crate::cli::host::cli_host_ports(&path),
        trigger: BuildTrigger::Full,
        reason: None,
        exits_after_build: true,
        site_url_override: None,
        server_port: None,
        admission_epoch: None,
        live_port: None,
        rename_pairs: Vec::new(),
    }));
}

/// Fixture × field mismatch counts, and the first few differing pages.
#[test]
fn core_site_pages_match_the_builds_pages_on_every_fixture() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/snapshot-sites");
    let mut names: Vec<String> = std::fs::read_dir(&fixtures).unwrap().flatten()
        .filter(|e| e.path().join("input").is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    let scratch = std::env::temp_dir().join(format!("moss_site_pages_{}", std::process::id()));
    let mut report = String::new();
    let mut failures = 0;
    let mut compared = 0;
    for name in &names {
        let folder = scratch.join(name).join("site");
        copy_dir(&fixtures.join(name).join("input"), &folder);
        build(&folder);
        let root = crate::vault_root::VaultRoot::resolve(&folder);
        let Some(captured) = take(root.path()) else {
            report.push_str(&format!("{name}: no capture\n"));
            failures += 1;
            continue;
        };
        let mut files = Vec::new();
        site_files(&folder, &folder, &mut files);
        let texts: HashMap<String, String> = files.iter()
            .filter(|f| moss_core::source_edit::policy::is_page_path(Path::new(f)))
            .filter_map(|f| Some((f.clone(), std::fs::read_to_string(folder.join(f)).ok()?)))
            .collect();
        let config_toml = std::fs::read_to_string(folder.join(".moss/config.toml")).ok();
        let (bundle, _) = moss_core::page::site_bundle(moss_core::page::SiteSources {
            files, texts: texts.clone(), config_toml, folder_langs: captured.folder_langs.clone(), ..Default::default()
        });
        let site = moss_core::page::site_page::site_pages(&texts, &bundle, &captured.root_name);
        let core: BTreeMap<String, PageFacts> = site.paths.into_iter().zip(site.pages).collect();
        assert!(!captured.pages.is_empty() || texts.is_empty(), "{name}: the build captured no pages");
        compared += captured.pages.len();
        let mut fields: BTreeMap<String, usize> = BTreeMap::new();
        let mut examples = Vec::new();
        for (path, built) in &captured.pages {
            let built_json = serde_json::to_value(built).unwrap();
            let core_json = core.get(path).map(|c| serde_json::to_value(c).unwrap());
            let Some(core_json) = core_json else {
                *fields.entry("<missing>".into()).or_default() += 1;
                examples.push(format!("  {path}: not in core's pages"));
                continue;
            };
            for (key, value) in built_json.as_object().unwrap() {
                if core_json.get(key) != Some(value) {
                    *fields.entry(key.clone()).or_default() += 1;
                    if examples.len() < 6 {
                        examples.push(format!("  {path} {key}: build {value} core {}", core_json.get(key).unwrap_or(&serde_json::Value::Null)));
                    }
                }
            }
        }
        for path in core.keys().filter(|p| !captured.pages.contains_key(*p)) {
            *fields.entry("<extra>".into()).or_default() += 1;
            if examples.len() < 6 {
                examples.push(format!("  {path}: only in core's pages"));
            }
        }
        if !fields.is_empty() {
            failures += 1;
            report.push_str(&format!("{name}: {fields:?}\n{}\n", examples.join("\n")));
        }
    }
    let _ = std::fs::remove_dir_all(&scratch);
    if let Ok(out) = std::env::var("SITE_PAGES_REPORT") {
        let _ = std::fs::write(out, &report);
    }
    assert!(compared > 0, "no page was compared");
    assert_eq!(failures, 0, "core's pages differ from the build's in {failures} of {} fixtures:\n{report}", names.len());
}
