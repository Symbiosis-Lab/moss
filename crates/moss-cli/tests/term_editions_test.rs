//! A plugin listing on an edition's home receives that edition's term pages in
//! `ctx.terms`, through the real binary. All names are invented.

use std::path::Path;
use std::process::Command;

const MOSS: &str = env!("CARGO_BIN_EXE_moss-cli");

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn the_plugin_on_an_edition_home_gets_that_editions_term_pages() {
    let tmp = tempfile::tempdir().unwrap();
    let (root, home) = (tmp.path().join("site"), tmp.path().join("home"));
    std::fs::create_dir_all(&home).unwrap();
    write(&root, ".moss/config.toml", "# mine\n");
    write(&root, "index.md", "---\ntitle: Home\n---\nWelcome\n");
    write(&root, "Concerts/Concert.md", "---\ntitle: Concert\ndate: 2026-01-01\nauthor: Ada Example\n---\nbody\n");
    write(&root, "de/index.md", "---\ntitle: Start\nchildren_style: board\n---\nWillkommen\n");
    write(&root, "de/Konzerte/Konzert.md", "---\ntitle: Konzert\ndate: 2026-01-02\nauthor: Ada Example\n---\nKörper\n");
    write(
        &root,
        ".moss/plugins/boardview/manifest.json",
        r#"{"name":"boardview","version":"1.0.0","entry":"main.js","contributes":{"listing_styles":{"board":{"label":"Board","description":"Cards"}}}}"#,
    );
    write(
        &root,
        ".moss/plugins/boardview/main.js",
        r#"var BoardviewPlugin = (() => ({ async listing(ctx) { return { html: '<script type="application/json" id="ctx">' + JSON.stringify(ctx) + '</script>' }; } }))();"#,
    );

    let out = Command::new(MOSS)
        .arg("build")
        .arg(&root)
        .args(["--allow-plugins", "--strict"])
        .env("MOSS_HOME", &home)
        .env_remove("MOSS_ENV")
        .output()
        .expect("spawn moss-cli");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let page = std::fs::read_to_string(root.join(".moss/build.nosync/current/de/index.html")).unwrap();
    let json = page.split(r#"id="ctx">"#).nth(1).and_then(|rest| rest.split("</script>").next()).expect("no context in the page");
    let ctx: serde_json::Value = serde_json::from_str(json).unwrap();
    let pages = ctx["pages"].as_array().unwrap();
    assert_eq!(pages.len(), 1, "{pages:#?}");
    // Relative to the listing page at /de/: its own edition's page, not ../authors/.
    assert_eq!(ctx["terms"]["authors"]["Ada Example"], "authors/ada-example/", "{}", ctx["terms"]);
}
