//! An inline `:::subscribe` form must carry the same scope the footer form
//! would on that page: the page's language section, or "" at the root.

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

fn read_page(out: &Path, url: &str) -> String {
    fs::read_to_string(out.join(url)).unwrap_or_else(|e| panic!("{url}: {e}"))
}

/// The hidden `scope` value of the page's inline subscribe form.
fn inline_scope(html: &str) -> String {
    let form = html
        .find(r#"data-position="inline""#)
        .unwrap_or_else(|| panic!("no inline subscribe form in: {html}"));
    let marker = r#"<input type="hidden" name="scope" value=""#;
    let start = form + html[form..].find(marker).expect("inline form has no scope input") + marker.len();
    let end = start + html[start..].find('"').unwrap();
    html[start..end].to_string()
}

#[test]
fn inline_subscribe_scope_matches_the_page_language_section() {
    let tmp = std::env::temp_dir().join(format!("moss_inline_scope_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(tmp.clone());

    write(&tmp, ".moss/config.toml", "[site]\ndomain = \"example.com\"\n");
    write(&tmp, "index.md", "---\ntitle: Home\nlang: en\ntranslationKey: home\n---\n\nHi.\n\n:::subscribe\n:::\n");
    write(&tmp, "zh-hans/index.md", "---\ntitle: 首页\ntranslationKey: home\n---\n\n你好。\n");
    write(&tmp, "zh-hans/join.md", "---\ntitle: 订阅\n---\n\n订阅我们。\n\n:::subscribe {button=\"订阅\"}\n:::\n");
    // A language section declared only by translationKey: the judge's second signal.
    write(&tmp, "essais/index.md", "---\ntitle: Accueil\nlang: fr\ntranslationKey: home\n---\n\nBonjour.\n");
    write(&tmp, "essais/rejoindre.md", "---\ntitle: Rejoindre\nlang: fr\n---\n\nAbonnez-vous.\n\n:::subscribe\n:::\n");

    let result = build_sync(&tmp.to_string_lossy());
    assert!(result.is_ok(), "build failed: {result:?}");
    let out = tmp.join(".moss/build.nosync/staging");

    assert_eq!(inline_scope(&read_page(&out, "index.html")), "");
    assert_eq!(inline_scope(&read_page(&out, "zh-hans/join/index.html")), "zh-hans");
    assert_eq!(inline_scope(&read_page(&out, "essais/rejoindre/index.html")), "essais");

    // Parity with the footer's renderer: the same inputs produce the same bytes.
    let expected = moss_build::build::features::email::render_hosted_subscribe_form(
        None,
        moss_build::i18n::Language::ZhHans,
        "zh-hans",
        None,
        Some("订阅"),
        "https://api.mosspub.com",
    );
    assert!(
        read_page(&out, "zh-hans/join/index.html").contains(&expected),
        "stamped form diverged from the footer renderer's output:\n{expected}"
    );
}
