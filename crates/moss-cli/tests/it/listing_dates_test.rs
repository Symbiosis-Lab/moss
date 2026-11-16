//! How a folder listing prints and orders dates, through the real binary. All
//! names and text are invented.

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

/// A site whose `shows/` folder holds `pages`, each `(file, frontmatter lines)`,
/// under a folder home page that carries `home_extra` frontmatter.
fn site(home_extra: &str, pages: &[(&str, &str)]) -> Site {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("site");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    write(&root, ".moss/config.toml", "# mine\n");
    write(&root, "index.md", "---\ntitle: Home\n---\nWelcome\n");
    write(&root, "shows/index.md", &format!("---\ntitle: Shows\n{home_extra}---\nIntro\n"));
    for (file, fm) in pages {
        write(&root, &format!("shows/{file}"), &format!("---\n{fm}---\nbody\n"));
    }
    Site { _tmp: tmp, home, root }
}

impl Site {
    fn build(&self, flags: &[&str]) -> Output {
        Command::new(MOSS)
            .arg("build")
            .arg(&self.root)
            .args(flags)
            .env("MOSS_HOME", &self.home)
            .env_remove("MOSS_ENV")
            .output()
            .expect("spawn moss-cli")
    }

    fn listing(&self) -> String {
        std::fs::read_to_string(self.root.join(".moss/build.nosync/current/shows/index.html")).expect("shows listing")
    }
}

fn log(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

#[test]
fn a_month_precision_date_prints_as_a_month_in_the_page_language() {
    let en = site("", &[("a.md", "title: A\ndate: 2026-02\n"), ("b.md", "title: B\ndate: 2026-03-04\n")]);
    assert!(en.build(&[]).status.success());
    let html = en.listing();
    assert!(html.contains("February 2026"), "{html}");
    assert!(!html.contains("2026 · 02"), "{html}");
    assert!(html.contains("March 2026") && !html.contains("2026 · 03"), "a full date names its month too:\n{html}");

    let zh = site("lang: zh-hans\n", &[("a.md", "title: A\ndate: 2026-02\nlang: zh-hans\n")]);
    assert!(zh.build(&[]).status.success());
    assert!(zh.listing().contains("2026年2月"), "{}", zh.listing());
}

#[test]
fn a_folder_that_falls_back_to_title_order_names_the_undated_pages() {
    let s = site("", &[("a.md", "title: A\ndate: 2026-02-01\n"), ("b.md", "title: B\n"), ("c.md", "title: C\n")]);
    let out = s.build(&["--strict"]);
    let text = log(&out);
    assert!(!out.status.success(), "--strict must fail:\n{text}");
    assert!(text.contains("shows") && text.contains("b.md") && text.contains("c.md"), "{text}");
    assert!(!text.contains("a.md"), "only undated pages are named:\n{text}");
}

#[test]
fn no_warning_when_the_order_is_declared_the_dates_suffice_or_none_exist() {
    let declared = site("sort: title\n", &[("a.md", "title: A\ndate: 2026-02-01\n"), ("b.md", "title: B\n")]);
    let enough = site("", &[("a.md", "title: A\ndate: 2026-02-01\n"), ("b.md", "title: B\ndate: 2026-02-02\n")]);
    let hidden = site("children: false\n", &[("a.md", "title: A\ndate: 2026-02-01\n"), ("b.md", "title: B\n")]);
    let none = site("", &[("a.md", "title: A\n"), ("b.md", "title: B\n")]);
    for s in [declared, enough, hidden, none] {
        let out = s.build(&["--strict"]);
        assert!(out.status.success(), "{}", log(&out));
    }
}
