//! `.moss/theme/script.mjs` is an ES module; every `.js`/`.mjs` beside it is
//! served from a content-hashed URL named in an import map in `<head>`. A theme
//! without a `script.mjs` is untouched. Built through the real pipeline.

use std::fs;
use std::path::{Path, PathBuf};

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

struct Site(PathBuf);

impl Site {
    fn new(theme: &[(&str, &str)]) -> Self {
        let root = std::env::temp_dir().join(format!("moss_theme_modules_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        write(&root, ".moss/config.toml", "[site]\ndomain = \"example.com\"\n");
        write(&root, "index.md", "---\ntitle: Home\n---\n\nWelcome.\n");
        for (name, body) in theme {
            write(&root, &format!(".moss/theme/{name}"), body);
        }
        Site(root)
    }

    fn build(&self) -> String {
        build_sync(&self.0.to_string_lossy()).expect("build failed");
        fs::read_to_string(self.0.join(".moss/build.nosync/staging/index.html")).unwrap()
    }

    fn staged(&self, rel: &str) -> PathBuf {
        self.0.join(".moss/build.nosync/staging").join(rel)
    }

    fn hashed(&self, dir: &str, stem: &str) -> Vec<String> {
        let prefix = format!("{stem}.");
        let mut found: Vec<String> = fs::read_dir(self.staged(dir))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with(&prefix) && n.ends_with(".js") && n.len() > stem.len() + 4)
            .collect();
        found.sort();
        found
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_theme_with_no_sibling_script_gets_no_import_map() {
    let site = Site::new(&[("script.js", "var classic = 1;"), ("grain.png", "not js")]);
    let html = site.build();
    assert!(!html.contains("importmap"), "{html}");
    assert!(!html.contains(r#"type="module""#), "{html}");
}

#[test]
fn sibling_scripts_are_fingerprinted_even_without_script_mjs() {
    let site = Site::new(&[("script.js", "import('./vendor/kernel.js');"), ("vendor/kernel.js", "export const k = 1;")]);
    let html = site.build();
    assert!(html.contains(r#"<script type="importmap">"#), "{html}");
    assert!(html.contains("\"/_moss/theme/vendor/kernel.js\":\"/_moss/theme/vendor/kernel."), "{html}");
    assert!(!html.contains(r#"type="module""#), "no script.mjs, no module tag: {html}");
    assert!(site.staged("_moss/theme/vendor/kernel.js").is_file(), "the plain copy stays served");
}

#[test]
fn an_odd_file_name_and_a_non_utf8_sibling_do_not_break_the_page_or_the_build() {
    let site = Site::new(&[
        ("script.mjs", "import './lib dir/日本 k.js';"),
        ("lib dir/日本 k.js", "// </script> in a comment\nexport {};"),
    ]);
    fs::write(site.0.join(".moss/theme/latin.js"), [0x63, 0x61, 0x66, 0xe9]).unwrap();
    // A page that brings its own import map beside moss's.
    write(&site.0, "index.md", "---\ntitle: Home\n---\n\n<script type=\"importmap\">{\"imports\":{}}</script>\n\nWelcome.\n");
    let html = site.build();
    let head = html.split("</head>").next().unwrap();
    assert!(head.contains("日本 k.js\":\"/_moss/theme/lib dir/日本 k."), "{head}");
    assert!(!html.contains("latin.js\":"), "a non-UTF-8 file is left out of the map");
    assert_eq!(html.matches(r#"<script type="importmap">"#).count(), 2, "moss's map and the page's own");
    assert!(html.find(r#"<script type="importmap">"#).unwrap() < html.find(r#"type="module""#).unwrap());
}

#[test]
fn script_mjs_runs_as_a_module_and_its_imports_are_hashed_through_an_import_map() {
    let site = Site::new(&[
        ("script.js", "var classic = 1;"),
        ("script.mjs", "import { k } from './kernel.js'; import './lib/figures.mjs'; window.k = k;"),
        ("kernel.js", "export const k = 1;"),
        ("lib/figures.mjs", "import { k } from '../kernel.js'; export const f = k;"),
    ]);
    write(&site.0, "notes/a.md", "---\ntitle: A\n---\n\nFirst.\n");
    write(&site.0, "notes/b.md", "---\ntitle: B\n---\n\nSecond.\n");
    let html = site.build();

    // Every page shape carries the map before its module tag: an article page
    // and the folder index moss generates for `notes/` (no `notes/index.md`).
    for page in ["notes/a/index.html", "notes/index.html"] {
        let page_html = fs::read_to_string(site.staged(page)).unwrap_or_else(|e| panic!("{page}: {e}"));
        let map = page_html.find(r#"<script type="importmap">"#).unwrap_or_else(|| panic!("{page} has no import map"));
        assert!(map < page_html.find(r#"type="module""#).unwrap_or_else(|| panic!("{page} has no module tag")), "{page}");
    }

    let head = html.split("</head>").next().unwrap();
    let map_at = head.find(r#"<script type="importmap">"#).expect("import map in <head>");
    assert!(map_at < html.find(r#"type="module""#).unwrap(), "map must precede every module script");
    assert!(html.contains(r#"<script src="/_moss/theme/script."#), "classic script.js tag stays: {html}");

    let kernel = site.hashed("_moss/theme", "kernel");
    assert_eq!(kernel.len(), 1, "{kernel:?}");
    assert!(html.contains(&format!("\"/_moss/theme/kernel.js\":\"/_moss/theme/{}\"", kernel[0])), "{head}");
    assert_eq!(site.hashed("_moss/theme/lib", "figures").len(), 1);
    assert!(head.contains("\"/_moss/theme/lib/figures.mjs\":\"/_moss/theme/lib/figures."), "{head}");
    assert!(site.staged("_moss/theme/kernel.js").is_file(), "the plain copy stays served");

    // Editing kernel.js moves only kernel's address, not the entry's.
    let entry_before = html.split(r#"type="module" src=""#).nth(1).unwrap().split('"').next().unwrap().to_string();
    write(&site.0, ".moss/theme/kernel.js", "export const k = 2;");
    let html = site.build();
    let entry_after = html.split(r#"type="module" src=""#).nth(1).unwrap().split('"').next().unwrap().to_string();
    assert_eq!(entry_before, entry_after);
    let mapped = |html: &str| -> String {
        let after = html.split("\"/_moss/theme/kernel.js\":\"").nth(1).expect("kernel in map");
        after.split('"').next().unwrap().to_string()
    };
    let kernel_now = mapped(&html);
    assert_ne!(kernel_now, format!("/_moss/theme/{}", kernel[0]));
    assert!(site.staged(kernel_now.trim_start_matches('/')).is_file(), "{kernel_now}");
}
