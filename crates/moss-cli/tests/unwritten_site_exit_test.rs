//! A build whose site could not be written is a failed build: `moss build`
//! says so and exits non-zero, and `moss deploy` says so before it refuses.
//! A script that reads the exit code would otherwise go on to use the site
//! the build before it left. Run through the real CLI.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn moss(args: &[&str], site: &Path, home: &Path) -> (Output, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .args(args)
        .arg(site)
        .env("MOSS_HOME", home)
        .env("HOME", home)
        .env("MOSS_SITE_URL", "https://example.com")
        .env_remove("MOSS_ENV")
        .stdin(Stdio::null())
        .output()
        .expect("run moss-cli");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (out, stderr)
}

fn set_mode(dir: &Path, mode: u32) {
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// A built site whose next build renders and then has nowhere to put its
/// site. `None` where the folder cannot be made read-only: root writes through
/// the mode, so nothing would fail.
struct Unwritable {
    _tmp: tempfile::TempDir,
    home: tempfile::TempDir,
    site: PathBuf,
    generations: PathBuf,
}

impl Unwritable {
    fn new() -> Option<Self> {
        let home = tempfile::tempdir().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let site = std::fs::canonicalize(tmp.path()).unwrap();
        std::fs::write(site.join("index.md"), "---\ntitle: Home\n---\n\nFirst words.\n").unwrap();
        // `moss deploy` asks for one before it builds.
        assert!(moss(&["env", "local"], &site, home.path()).0.status.success());

        let (first, stderr) = moss(&["build", "--no-plugins"], &site, home.path());
        assert!(first.status.success(), "the first build must succeed:\n{stderr}");
        let generations = site.join(".moss/build.nosync/generations");
        let made = Self { _tmp: tmp, home, site, generations };
        assert!(made.served().contains("First words."));

        set_mode(&made.generations, 0o555);
        if std::fs::write(made.generations.join("probe"), "").is_ok() {
            return None;
        }
        std::fs::write(made.site.join("index.md"), "---\ntitle: Home\n---\n\nSecond words.\n").unwrap();
        Some(made)
    }

    fn served(&self) -> String {
        std::fs::read_to_string(self.site.join(".moss/build.nosync/current/index.html")).unwrap()
    }

    /// Run `args` on the folder; its build leaves the first build's site in place.
    fn run(&self, args: &[&str]) -> (Output, String) {
        let (out, stderr) = moss(args, &self.site, self.home.path());
        let page = self.served();
        assert!(
            page.contains("First words.") && !page.contains("Second words."),
            "premise: the second build must leave the first build's site in place:\n{stderr}"
        );
        (out, stderr)
    }
}

impl Drop for Unwritable {
    fn drop(&mut self) {
        set_mode(&self.generations, 0o755);
    }
}

#[test]
fn a_build_that_cannot_write_the_site_fails() {
    let Some(folder) = Unwritable::new() else { return };

    let (second, stderr) = folder.run(&["build", "--no-plugins"]);

    assert_eq!(second.status.code(), Some(1), "a build that wrote no site must fail:\n{stderr}");
    assert!(stderr.contains("Build failed: the site was not written."), "{stderr}");
    assert!(!stderr.contains("0 problems"), "a failed build must not report a clean one:\n{stderr}");
}

#[test]
fn a_deploy_whose_build_cannot_write_the_site_says_so_before_it_refuses() {
    let Some(folder) = Unwritable::new() else { return };

    let (dry, stderr) = folder.run(&["deploy", "--dry-run", "--json"]);

    assert_eq!(dry.status.code(), Some(1), "{stderr}");
    let report: serde_json::Value = serde_json::from_slice(&dry.stdout).unwrap_or_else(|e| panic!("{e}:\n{stderr}"));
    assert!(report["error"].as_str().is_some_and(|e| e.contains("not sealed yet")), "{report}\n{stderr}");
    assert!(stderr.contains("Build failed: the site was not written."), "{stderr}");
    assert!(!stderr.contains("0 problems"), "a failed build must not report a clean one:\n{stderr}");
}
