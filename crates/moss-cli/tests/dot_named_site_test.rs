//! A site whose own folder name starts with a dot builds the same pages as the
//! same site in a plain-named folder. Dot-named entries *inside* a site are
//! excluded from the build by design; that rule applies to path components
//! below the site root, not to the root's own name.

use std::path::Path;
use std::process::{Command, Stdio};

/// An 8x8 red PNG.
const PIC: [u8; 75] = [137,80,78,71,13,10,26,10,0,0,0,13,73,72,68,82,0,0,0,8,0,0,0,8,8,2,0,0,0,75,109,41,220,0,0,0,18,73,68,65,84,120,156,99,248,207,192,128,21,97,23,29,180,18,0,40,255,63,193,110,236,223,97,0,0,0,0,73,69,78,68,174,66,96,130];

/// Every built file below the output root except the per-page share cards under
/// `_moss/`, whose names hash the page title. Sorted, `/`-separated.
fn built_files(current: &Path) -> Vec<String> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, root, out);
            } else {
                out.push(p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = Vec::new();
    walk(&std::fs::canonicalize(current).unwrap(), &std::fs::canonicalize(current).unwrap(), &mut out);
    out.retain(|f| !f.starts_with("_moss/"));
    out.sort();
    out
}

fn build_and_read_index(parent: &Path, folder: &str) -> (String, Vec<String>) {
    let site = parent.join(folder);
    std::fs::create_dir_all(site.join(".private")).unwrap();
    std::fs::write(
        site.join("index.md"),
        "---\ntitle: Home\n---\n\nVisible body words.\n\n![a red square](pic.png)\n",
    )
    .unwrap();
    std::fs::write(site.join("about.md"), "---\ntitle: About\n---\n\nAbout words.\n").unwrap();
    std::fs::write(site.join("pic.png"), PIC).unwrap();
    std::fs::write(
        site.join(".private/notes.md"),
        "---\ntitle: Notes\n---\n\nSecretive hidden words.\n",
    )
    .unwrap();
    let moss_home = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .arg("build")
        .arg(&site)
        .arg("--no-plugins")
        .env("MOSS_HOME", moss_home.path())
        .stdin(Stdio::null())
        .output()
        .expect("run moss-cli build");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let current = site.join(".moss/build.nosync/current");
    assert!(
        !current.join(".private").exists() && !current.join("private").exists(),
        "a dot-named folder inside the site stays excluded"
    );
    let index = std::fs::read_to_string(current.join("index.html")).expect("index.html was built");
    (index, built_files(&current))
}

#[test]
fn a_dot_named_site_root_builds_the_same_site_as_a_plain_one() {
    let parent = tempfile::tempdir().unwrap();
    let (plain, plain_files) = build_and_read_index(parent.path(), "plain");
    assert!(plain.contains("Visible body words."), "baseline: {plain}");
    assert!(plain_files.iter().any(|f| f.ends_with(".png") || f.ends_with(".webp")), "baseline has the image: {plain_files:?}");
    assert!(plain_files.iter().any(|f| f.starts_with("about")), "baseline has the second page: {plain_files:?}");

    let (dotted, dotted_files) = build_and_read_index(parent.path(), ".dotsite");
    assert!(
        dotted.contains("Visible body words."),
        "the page built from a dot-named root lost its content"
    );
    assert!(!dotted.contains("Secretive hidden words."));
    assert_eq!(dotted_files, plain_files, "the dot-named site must publish the same files, images and pages included");
}
