//! Two pages with the same date keep the order their authors give them with
//! `weight:`; without a weight a listing keeps ordering them by url path.

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

/// A fresh site in a temp folder, built once. Returns the guard and the
/// build's staging directory.
fn build_site(name: &str, files: &[(&str, &str)]) -> (Cleanup, PathBuf) {
    let tmp =
        std::env::temp_dir().join(format!("moss_same_date_weight_{name}_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let cleanup = Cleanup(tmp.clone());
    for (rel, body) in files {
        write(&tmp, rel, body);
    }
    let result = build_sync(&tmp.to_string_lossy());
    assert!(result.is_ok(), "build failed: {result:?}");
    (cleanup, tmp.join(".moss/build.nosync/staging"))
}


const CONFIG: &str = "schema_version = 6\n\n[site]\nlang = \"en\"\n";

fn page(title: &str, extra: &str) -> String {
    format!("---\ntitle: {title}\ndate: 2026-05-01\n{extra}---\n\nBody.\n")
}

/// The titles in the order a listing page draws them.
fn order(out: &Path, folder: &str, titles: &[&str]) -> Vec<String> {
    let html = fs::read_to_string(out.join(folder).join("index.html")).unwrap();
    let mut at: Vec<(usize, String)> = titles
        .iter()
        .map(|t| (html.find(&format!(">{t}</h3>")).unwrap_or_else(|| panic!("{t} listed:\n{html}")), t.to_string()))
        .collect();
    at.sort();
    at.into_iter().map(|(_, t)| t).collect()
}

fn listing(sort: &str, weights: [Option<i32>; 3]) -> Vec<String> {
    let w = |n: Option<i32>| n.map(|n| format!("weight: {n}\n")).unwrap_or_default();
    let index = format!("---\ntitle: Essays\nsort: {sort}\nchildren_style: summary\nchildren_group: none\n---\n\nEssays.\n");
    let (a, b, c) = (page("Apple", &w(weights[0])), page("Berry", &w(weights[1])), page("Cherry", &w(weights[2])));
    let (_guard, out) = build_site(
        "tie",
        &[
            (".moss/config.toml", CONFIG),
            ("index.md", "---\ntitle: Home\n---\n\nHi.\n"),
            ("essays/index.md", &index),
            ("essays/a.md", &a),
            ("essays/b.md", &b),
            ("essays/c.md", &c),
        ],
    );
    order(&out, "essays", &["Apple", "Berry", "Cherry"])
}

#[test]
fn pages_with_one_date_follow_their_weight_newest_first() {
    assert_eq!(listing("date", [Some(3), Some(1), Some(2)]), ["Berry", "Cherry", "Apple"]);
}

#[test]
fn pages_with_one_date_follow_their_weight_oldest_first() {
    assert_eq!(listing("date-asc", [Some(3), Some(1), Some(2)]), ["Berry", "Cherry", "Apple"]);
}

#[test]
fn a_weighted_page_precedes_unweighted_ones_of_its_date() {
    assert_eq!(listing("date", [None, None, Some(1)]), ["Cherry", "Apple", "Berry"]);
}

#[test]
fn pages_without_weights_keep_their_order() {
    assert_eq!(listing("date", [None, None, None]), ["Apple", "Berry", "Cherry"]);
}
