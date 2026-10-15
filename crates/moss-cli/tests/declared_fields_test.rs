//! A site declares its own frontmatter fields in `.moss/config.toml`, and a
//! page whose value does not fit the declared type fails a `--strict` build
//! through the real binary. All names and text are invented.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const MOSS: &str = env!("CARGO_BIN_EXE_moss-cli");

struct Site {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    root: PathBuf,
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn site(config: &str, pages: &[(&str, &str)]) -> Site {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("site");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    write(&root, ".moss/config.toml", config);
    write(&root, "index.md", "---\ntitle: Home\n---\nWelcome\n");
    for (rel, text) in pages {
        write(&root, rel, text);
    }
    Site { _tmp: tmp, home, root }
}

fn build_strict(s: &Site) -> Output {
    Command::new(MOSS)
        .arg("build")
        .arg(&s.root)
        .arg("--strict")
        .env("MOSS_HOME", &s.home)
        .env_remove("MOSS_ENV")
        .output()
        .expect("spawn moss-cli")
}

const KIND: &str = "[fields.kind]\ntype = \"choice\"\nof = [\"Building\", \"Service\"]\n";

#[test]
fn a_value_outside_its_declared_choice_fails_a_strict_build_and_names_the_page() {
    let s = site(KIND, &[("Housing/court.md", "---\ntitle: Court\nkind: Buidling\n---\nbody\n")]);
    let o = build_strict(&s);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success(), "a strict build must fail: {err}");
    assert!(
        err.contains("[Housing/court.md] frontmatter: field 'kind'") && err.contains("\"Buidling\" is not one of: Building, Service"),
        "{err}"
    );
}

#[test]
fn a_declared_value_that_fits_builds_clean_under_strict() {
    let s = site(KIND, &[("Housing/court.md", "---\ntitle: Court\nkind: Building\n---\nbody\n")]);
    let o = build_strict(&s);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

#[test]
fn a_page_outside_the_declared_folders_is_not_checked() {
    let config = format!("{KIND}in = [\"Housing\"]\n");
    let s = site(&config, &[("Services/desk.md", "---\ntitle: Desk\nkind: Buidling\n---\nbody\n")]);
    let o = build_strict(&s);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

#[test]
fn the_warning_still_fails_a_second_strict_build_of_an_unchanged_site() {
    let s = site(KIND, &[("Housing/court.md", "---\ntitle: Court\nkind: Buidling\n---\nbody\n")]);
    let first = build_strict(&s);
    assert!(!first.status.success(), "{}", String::from_utf8_lossy(&first.stderr));
    let second = build_strict(&s);
    let err = String::from_utf8_lossy(&second.stderr);
    assert!(!second.status.success(), "a second strict build must still fail: {err}");
    assert!(err.contains("field 'kind'"), "{err}");
}

#[test]
fn a_wikilink_in_a_pages_field_is_read_as_written_and_builds_clean_under_strict() {
    let config = "[fields.highlights]\ntype = \"pages\"\n";
    let s = site(
        config,
        &[
            ("Housing/Riverside Court.md", "---\ntitle: Riverside Court\n---\nbody\n"),
            ("Housing/court.md", "---\ntitle: Court\nhighlights: [\"[[Riverside Court]]\"]\n---\nbody\n"),
            ("Housing/block.md", "---\ntitle: Block\nhighlights:\n  - \"[[Housing/Riverside Court]]\"\n---\nbody\n"),
        ],
    );
    let o = build_strict(&s);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

#[test]
fn a_value_in_a_language_tree_is_checked_under_its_folder_scope() {
    let config = format!("{KIND}in = [\"Housing\"]\n");
    let s = site(&config, &[("de/Housing/x.md", "---\ntitle: X\nkind: Buidling\n---\nbody\n")]);
    let o = build_strict(&s);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success(), "a strict build must fail: {err}");
    assert!(err.contains("[de/Housing/x.md] frontmatter: field 'kind'"), "{err}");
}
