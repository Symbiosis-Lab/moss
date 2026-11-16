//! `moss history --save` through the real binary: a save records the source
//! as the disk holds it and writes nothing but the version, under
//! `.moss/history`. It never builds, so a folder whose build cache never
//! arrived (a synced copy, a fresh clone) saves the same as any other. All
//! names and text are invented.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, Output};

const MOSS: &str = env!("CARGO_BIN_EXE_moss-cli");

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// Every file and directory under `dir`, relative to it.
fn listing(dir: &Path) -> BTreeSet<String> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            out.insert(path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                walk(root, &path, out);
            }
        }
    }
    let mut out = BTreeSet::new();
    walk(dir, dir, &mut out);
    out
}

fn save(tmp: &Path, site: &Path) -> Output {
    Command::new(MOSS).arg("history").arg(site).args(["--save", "Before delivery", "--json"])
        .env("MOSS_HOME", tmp.join("moss-home")).env("HOME", tmp.join("home"))
        .output().unwrap()
}

fn site(tmp: &Path) -> std::path::PathBuf {
    let site = tmp.join("site");
    for dir in ["moss-home", "home"] {
        std::fs::create_dir_all(tmp.join(dir)).unwrap();
    }
    write(&site, ".moss/config.toml", "[site]\nlang = \"en\"\n");
    write(&site, "index.md", "---\ntitle: Home\n---\nHello\n");
    write(&site, "notes/one.md", "# One\n");
    site
}

/// The version names every source, and the only new paths anywhere the
/// binary could write are the version's own.
#[test]
fn a_never_built_folder_saves_its_sources_and_writes_nothing_else() {
    let tmp = tempfile::tempdir().unwrap();
    let site = site(tmp.path());
    let before = listing(tmp.path());
    let o = save(tmp.path(), &site);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let saved: serde_json::Value = serde_json::from_slice(&o.stdout).expect("stdout is one JSON object");
    assert_eq!(saved["label"], "Before delivery");

    let added: Vec<String> = listing(tmp.path()).difference(&before).cloned().collect();
    assert!(!added.is_empty());
    let outside: Vec<&String> = added.iter().filter(|p| !p.starts_with("site/.moss/history")).collect();
    assert!(outside.is_empty(), "a save wrote outside the version store: {outside:?}");

    let id = saved["id"].as_str().unwrap();
    let record: serde_json::Value = serde_json::from_slice(&std::fs::read(site.join(format!(".moss/history/publishes/{id}.json"))).unwrap()).unwrap();
    let entries: BTreeSet<&str> = record["entries"].as_object().unwrap().keys().map(String::as_str).collect();
    for source in ["index.md", "notes/one.md", ".moss/config.toml"] {
        assert!(entries.contains(source), "{source} missing from {entries:?}");
    }
}

#[cfg(unix)]
#[test]
fn a_page_that_cannot_be_read_stops_the_save_and_names_it() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let site = site(tmp.path());
    let locked = site.join("notes/one.md");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let o = save(tmp.path(), &site);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();

    assert!(!o.status.success());
    let error: serde_json::Value = serde_json::from_slice(&o.stdout).expect("--json reports the error on stdout");
    let message = error["error"].as_str().unwrap();
    assert!(message.contains("notes/one.md") && message.contains("nothing was saved"), "{message}");
    assert!(!site.join(".moss/history/publishes").exists() || listing(&site.join(".moss/history/publishes")).is_empty());
}
