//! A fenced div with the `screen` width word reaches the window's edges on a
//! phone as well as on a desktop, through the real binary. Names and text are
//! invented.

use std::path::Path;
use std::process::Command;

const MOSS: &str = env!("CARGO_BIN_EXE_moss-cli");

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// The built stylesheet of a one-page site whose page holds a `screen` band, whitespace removed.
fn built_css() -> (String, String) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("site");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    write(&root, ".moss/config.toml", "# mine\n");
    write(&root, "index.md", "---\ntitle: Example Hall\n---\nIntro\n\n:::{.band screen}\nSupport us.\n:::\n");
    let o = Command::new(MOSS).arg("build").arg(&root).env("MOSS_HOME", &home).env_remove("MOSS_ENV").output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let current = root.join(".moss/build.nosync/current");
    let html = std::fs::read_to_string(current.join("index.html")).unwrap();
    let rest = html.split("rel=\"stylesheet\" href=\"").nth(1).expect("a stylesheet link");
    let href = rest[..rest.find('"').unwrap()].trim_start_matches('/');
    let css = std::fs::read_to_string(current.join(href)).unwrap();
    (html, css.split_whitespace().collect())
}

#[test]
fn a_screen_band_is_not_folded_back_into_the_column_on_a_phone() {
    let (html, css) = built_css();
    assert!(html.contains(r#"data-width="screen""#), "{html}");
    // Every phone rule that collapses width escapes must leave `screen` alone.
    let phone = css.split("@media(max-width:48rem){").skip(1);
    for block in phone {
        for rule in block.split('}') {
            if let Some((sel, body)) = rule.split_once('{') {
                if sel.contains("article.container>[data-width") && body.contains("inline-size:auto") {
                    assert!(sel.contains(r#":not([data-width="screen"])"#), "phone rule collapses the screen band: {sel}");
                }
            }
        }
    }
    // The band keeps the page gutter around its content at every width.
    assert!(css.contains("padding-inline:var(--moss-container-padding)"), "no gutter on the band");
}
