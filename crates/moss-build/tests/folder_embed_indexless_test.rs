//! Embedding a folder that has pages but no home file (`![[orchard/]]` with no
//! `orchard/index.md` or `orchard/orchard.md`) must list those pages. The
//! build writes a synthetic `/orchard/` index for such a folder, so the embed
//! cannot call the same folder missing.

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

fn built(home_body: &str) -> (String, PathBuf, Cleanup) {
    built_with(home_body, &[])
}

fn built_with(home_body: &str, extra: &[(&str, &str)]) -> (String, PathBuf, Cleanup) {
    let tmp = std::env::temp_dir().join(format!("moss_indexless_embed_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let cleanup = Cleanup(tmp.clone());
    write(&tmp, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n");
    write(&tmp, "index.md", &format!("---\ntitle: Home\nchildren: false\n---\n\n{home_body}\n"));
    write(&tmp, "orchard/apples.md", "---\ntitle: Apples Of Autumn\n---\n\nCrisp.\n");
    write(&tmp, "orchard/pears.md", "---\ntitle: Pears In Winter\n---\n\nSoft.\n");
    for (rel, body) in extra {
        write(&tmp, rel, body);
    }
    let result = build_sync(&tmp.to_string_lossy());
    assert!(result.is_ok(), "build failed: {result:?}");
    let home = fs::read_to_string(tmp.join(".moss/build.nosync/staging/index.html")).unwrap();
    (home, tmp, cleanup)
}

#[test]
fn wiki_embed_of_indexless_folder_lists_its_pages() {
    let (home, tmp, _c) = built("![[orchard/]]");
    assert!(tmp.join(".moss/build.nosync/staging/orchard/index.html").exists());
    assert!(!home.contains("moss-embed-missing") && !home.contains("Folder not found"), "{home}");
    assert!(home.contains("Apples Of Autumn") && home.contains("Pears In Winter"), "{home}");
}

#[test]
fn standard_embed_of_indexless_folder_lists_its_pages() {
    let (home, _tmp, _c) = built("![](orchard/)");
    assert!(!home.contains("moss-embed-missing") && !home.contains("Folder not found"), "{home}");
    assert!(home.contains("Apples Of Autumn") && home.contains("Pears In Winter"), "{home}");
}

#[test]
fn embed_of_folder_with_no_pages_says_so() {
    let (home, _tmp, _c) = built("![[nowhere/]]");
    assert!(home.contains("No pages under nowhere/"), "{home}");
    assert!(!home.contains("Folder not found"), "{home}");
}

#[test]
fn a_folder_with_a_longer_name_is_not_the_embedded_folder() {
    let (home, _tmp, _c) = built_with("![[orch/]]", &[]);
    assert!(home.contains("No pages under orch/"), "{home}");
    assert!(!home.contains("Apples Of Autumn"), "{home}");
}
