//! A folder's `cascade: { author: … }` gives every page beneath it the author a
//! page would get from its own `author:`: a byline naming them links to their
//! author page, they are listed there, and a page's own `author:` still wins.

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

/// A fresh site in a temp folder, built once. Returns the guard and the staging directory.
fn build_site(name: &str, files: &[(&str, &str)]) -> (Cleanup, PathBuf) {
    let tmp = std::env::temp_dir().join(format!("moss_cascade_author_{name}_{}", uuid::Uuid::new_v4()));
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

const CASCADED: &[(&str, &str)] = &[
    (".moss/config.toml", CONFIG),
    ("index.md", "---\ntitle: Home\n---\n\nHi.\n"),
    ("prints/index.md", "---\ntitle: Prints\ncascade:\n  author: \"Ada Example\"\n---\n\nA series.\n"),
    ("prints/one.md", "---\ntitle: Print One\nbyline: \"Photographs by Ada Example\"\n---\n\nFirst.\n"),
    ("prints/two.md", "---\ntitle: Print Two\nauthor: \"Ben Sample\"\nbyline: \"Photographs by Ben Sample\"\n---\n\nSecond.\n"),
];

#[test]
fn a_cascaded_author_links_the_byline_and_lists_the_page_on_its_author_page() {
    let (_guard, out) = build_site("cascaded", CASCADED);

    let page = fs::read_to_string(out.join("prints/one/index.html")).expect("the child page is built");
    assert!(page.contains("href=\"/authors/ada-example/\""), "the byline names the cascaded author as a link:\n{page}");

    let term = fs::read_to_string(out.join("authors/ada-example/index.html")).expect("the cascaded author has a term page");
    assert!(term.contains("Print One"), "the term page lists the child:\n{term}");
    assert!(!term.contains("Print Two"), "a page with its own author is not listed under the cascade:\n{term}");
}

#[test]
fn a_page_with_its_own_author_keeps_it() {
    let (_guard, out) = build_site("own", CASCADED);

    let page = fs::read_to_string(out.join("prints/two/index.html")).expect("the child page is built");
    assert!(page.contains("href=\"/authors/ben-sample/\""), "{page}");
    assert!(!page.contains("/authors/ada-example/"), "the page's own author replaces the cascaded one:\n{page}");

    let ben = fs::read_to_string(out.join("authors/ben-sample/index.html")).expect("the own author has a term page");
    assert!(ben.contains("Print Two"), "{ben}");
}

#[test]
fn a_cascaded_author_renders_as_a_written_one_does() {
    let written = [
        (".moss/config.toml", CONFIG),
        ("index.md", "---\ntitle: Home\n---\n\nHi.\n"),
        ("prints/index.md", "---\ntitle: Prints\n---\n\nA series.\n"),
        ("prints/one.md", "---\ntitle: Print One\nauthor: \"Ada Example\"\nbyline: \"Photographs by Ada Example\"\n---\n\nFirst.\n"),
        ("prints/two.md", "---\ntitle: Print Two\nauthor: \"Ben Sample\"\nbyline: \"Photographs by Ben Sample\"\n---\n\nSecond.\n"),
    ];
    let (_g1, cascaded_out) = build_site("equiv_cascaded", CASCADED);
    let (_g2, written_out) = build_site("equiv_written", &written);
    let a = fs::read_to_string(cascaded_out.join("prints/one/index.html")).unwrap();
    let b = fs::read_to_string(written_out.join("prints/one/index.html")).unwrap();
    assert_eq!(a, b, "a cascaded author must produce the page a written one does");
}

#[test]
fn a_page_that_writes_a_blank_author_keeps_no_cascaded_author() {
    let mut files = CASCADED.to_vec();
    files.push(("prints/three.md", "---\ntitle: Print Three\nauthor: \"\"\nbyline: \"Photographs by Ada Example\"\n---\n\nThird.\n"));
    let (_guard, out) = build_site("blank", &files);

    let page = fs::read_to_string(out.join("prints/three/index.html")).expect("the child page is built");
    assert!(!page.contains("/authors/ada-example/"), "a blank author: written on the page takes no cascaded author:\n{page}");

    let term = fs::read_to_string(out.join("authors/ada-example/index.html")).expect("the cascaded author has a term page");
    assert!(!term.contains("Print Three"), "{term}");
}
