//! A bare line of colons closes the innermost open block, whatever its colon
//! count, so a block nested inside another block of the same count is built
//! as nested. Built through the real CLI, because the closer rule is applied
//! in the markdown pipeline, not only in the parser.

use std::path::Path;
use std::process::{Command, Stdio};

/// A 1200x800 JPEG, wide enough that the build emits a webp rung beside the base.
const PORTRAIT: &[u8] = include_bytes!("fixtures/crew-portrait.jpg");

fn write(site: &Path, rel: &str, body: &str) {
    let p = site.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

#[test]
fn a_hero_nested_in_a_same_length_fence_stays_inside_its_block() {
    let moss_home = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let site = std::fs::canonicalize(tmp.path()).unwrap();
    write(
        &site,
        "index.md",
        "---\ntitle: Home\n---\n\n::: {#crew}\n::: {.note}\nCrew portrait\n\n![crew portrait](crew.jpg)\n:::\n\nStill in the crew block.\n:::\n\nAfter the block.\n",
    );
    std::fs::write(site.join("crew.jpg"), PORTRAIT).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .args(["build", site.to_str().unwrap(), "--no-plugins", "--strict"])
        .env("MOSS_HOME", moss_home.path())
        .env("MOSS_SITE_URL", "https://example.com")
        .stdin(Stdio::null())
        .output()
        .expect("run moss-cli");
    assert!(out.status.success(), "build must be clean; stderr:\n{}", String::from_utf8_lossy(&out.stderr));

    let html = std::fs::read_to_string(site.join(".moss/build.nosync/current/index.html")).expect("index.html is built");
    assert!(!html.contains(":::"), "no fence may survive as literal text:\n{html}");
    let body = &html[html.find("<main").expect("a page body")..];
    let block = body.find("id=\"crew\"").expect("the crew block is built");
    let hero = body.find("Crew portrait").expect("the nested block text is built");
    let still = body.find("Still in the crew block.").expect("the text after the nested block is built");
    let after = body.find("After the block.").expect("the text after the block is built");
    assert!(block < hero && hero < still && still < after, "{body}");

    let built = std::fs::canonicalize(site.join(".moss/build.nosync/current")).unwrap();
    let files: Vec<String> = std::fs::read_dir(&built).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(files.iter().any(|f| f == "crew.w800.webp"), "the nested hero's rung is built: {files:?}");
    assert!(files.iter().any(|f| f == "crew.jpg" || f == "crew.webp"), "the nested hero's base is built: {files:?}");
}
