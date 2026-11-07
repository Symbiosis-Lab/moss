//! A collision is a build problem and an unconditional publish refusal.
use std::process::{Command, Output, Stdio};

struct Site {
    root: tempfile::TempDir,
    home: tempfile::TempDir,
    messages: Vec<&'static [&'static str]>,
}

impl Site {
    fn new(verbatim: bool) -> Self {
        let site = Self {
            root: tempfile::tempdir().unwrap(),
            home: tempfile::tempdir().unwrap(),
            messages: if verbatim {
                vec![&["docs.md", "docs/index.html", "/docs/"]]
            } else {
                vec![&["one.md", "two.md", "/shared/"], &["three.md", "four.md", "/another/"]]
            },
        };
        let files: &[(&str, &str)] = if verbatim {
            &[("docs.md", "Documentation."), ("docs/index.html", "<p>Static documentation.</p>")]
        } else {
            &[
                ("one.md", "---\nurl: shared\n---\nOne."),
                ("two.md", "---\nurl: shared\n---\nTwo."),
                ("three.md", "---\nurl: another\n---\nThree."),
                ("four.md", "---\nurl: another\n---\nFour."),
            ]
        };
        for (name, body) in files.iter().chain([("index.md", "Home.")].iter()) {
            let path = site.root.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        site
    }

    fn run(&self, command: &str, flags: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_moss-cli"))
            .arg(command).arg(self.root.path()).args(flags)
            .env("MOSS_HOME", self.home.path()).env("MOSS_SITE_URL", "https://example.com")
            .stdin(Stdio::null()).output().unwrap()
    }

    fn warnings(&self, output: &Output) -> Vec<String> {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let warnings: Vec<_> = stderr.lines().filter(|l| l.contains("both publish to")).collect();
        assert_eq!(warnings.len(), self.messages.len(), "{stderr}");
        for message in &self.messages {
            assert!(warnings.iter().any(|line| line.contains("[WARN]") && message.iter().all(|part| line.contains(part))), "{stderr}");
        }
        warnings.iter().map(|line| line.split_once(": ").unwrap().1.to_string()).collect()
    }
}

#[test]
fn output_collisions_warn_and_render_in_plain_builds() {
    for verbatim in [false, true] {
        let site = Site::new(verbatim);
        let plain = site.run("build", &["--no-plugins"]);
        assert!(plain.status.success(), "{}", String::from_utf8_lossy(&plain.stderr));
        site.warnings(&plain);
        let output = site.root.path().join(".moss/build.nosync/current");
        for path in if verbatim { vec!["index.html", "docs/index.html"] } else {
            vec!["index.html", "shared/index.html", "shared-2/index.html", "another/index.html", "another-2/index.html"]
        } {
            assert!(output.join(path).exists(), "{path} was not rendered");
        }
    }
}

#[test]
fn output_collisions_fail_strict_builds() {
    for verbatim in [false, true] {
        let site = Site::new(verbatim);
        let strict = site.run("build", &["--no-plugins", "--strict"]);
        assert_eq!(strict.status.code(), Some(1), "{}", String::from_utf8_lossy(&strict.stderr));
        site.warnings(&strict);
        assert!(site.root.path().join(".moss/build.nosync/current/index.html").exists());
    }
}

#[test]
fn output_collisions_refuse_dry_run_with_the_build_messages() {
    for verbatim in [false, true] {
        let site = Site::new(verbatim);
        let dry = site.run("deploy", &["--dry-run", "--json"]);
        assert_eq!(dry.status.code(), Some(1), "{}", String::from_utf8_lossy(&dry.stderr));
        let report: serde_json::Value = serde_json::from_slice(&dry.stdout)
            .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&dry.stderr)));
        assert_eq!(report["refusal"]["kind"], "output_collision", "{report}");
        assert_eq!(report["refusal"]["text"], site.warnings(&dry).join("\n"), "{report}");
    }
}
