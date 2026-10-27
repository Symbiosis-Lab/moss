//! A site that needs a newer moss than this one: the build still builds and
//! says why the pages may look wrong, `--strict` fails on it, and a deploy is
//! refused before anything is sent. The floor comes from the site's
//! `min_moss_version` or a plugin manifest's.

use std::path::Path;
use std::process::{Command, Output, Stdio};

fn site(config: &str, plugin_floor: Option<&str>) -> tempfile::TempDir {
    let site = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(site.path().join(".moss")).unwrap();
    std::fs::write(site.path().join(".moss/config.toml"), config).unwrap();
    std::fs::write(site.path().join("index.md"), "---\ntitle: Home\nhome: true\n---\n\nHello.\n").unwrap();
    if let Some(floor) = plugin_floor {
        let plugin = site.path().join(".moss/plugins/research");
        std::fs::create_dir_all(&plugin).unwrap();
        let manifest = format!(r#"{{"name":"research","version":"1.0.0","entry":"main.js","min_moss_version":"{floor}"}}"#);
        std::fs::write(plugin.join("manifest.json"), manifest).unwrap();
    }
    site
}

fn moss(site: &Path, args: &[&str]) -> Output {
    let moss_home = tempfile::tempdir().unwrap();
    Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .args(args)
        .arg(std::fs::canonicalize(site).unwrap())
        .env("MOSS_HOME", moss_home.path())
        .stdin(Stdio::null())
        .output()
        .expect("run moss-cli")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn a_site_floor_above_this_moss_builds_with_a_notice_and_refuses_to_deploy() {
    let site = site("min_moss_version = \"99.0.0\"\n", None);

    let build = moss(site.path(), &["build", "--no-plugins"]);
    assert!(build.status.success(), "a plain build still builds:\n{}", stderr(&build));
    assert_eq!(stderr(&build).matches("needs moss 99.0.0").count(), 1, "said once:\n{}", stderr(&build));

    let strict = moss(site.path(), &["build", "--no-plugins", "--strict"]);
    assert!(!strict.status.success(), "--strict must fail on it:\n{}", stderr(&strict));

    let deploy = moss(site.path(), &["deploy", "--dry-run"]);
    assert!(!deploy.status.success(), "a deploy must be refused:\n{}", stderr(&deploy));
    assert!(stderr(&deploy).contains("needs moss 99.0.0"), "stderr:\n{}", stderr(&deploy));
}

#[test]
fn a_plugin_floor_above_this_moss_is_named_in_the_refusal() {
    let site = site("", Some("98.0.0"));
    let strict = moss(site.path(), &["build", "--no-plugins", "--strict"]);
    assert!(!strict.status.success(), "stderr:\n{}", stderr(&strict));
    assert!(stderr(&strict).contains("research plugin in this site needs moss 98.0.0"), "stderr:\n{}", stderr(&strict));
}

/// A dev build carries the number of the release it becomes, so a designer
/// running it can write that number as the floor and keep publishing.
#[test]
fn a_floor_equal_to_this_builds_release_number_is_met() {
    let core = env!("CARGO_PKG_VERSION").split('-').next().unwrap();
    let site = site(&format!("min_moss_version = \"{core}\"\n"), None);
    let strict = moss(site.path(), &["build", "--no-plugins", "--strict"]);
    assert!(strict.status.success(), "stderr:\n{}", stderr(&strict));
    assert!(!stderr(&strict).contains("needs moss"), "stderr:\n{}", stderr(&strict));
}

/// A site an earlier development build already stamped 7 keeps building and
/// publishing here.
#[test]
fn a_site_stamped_seven_still_builds_and_deploys() {
    let site = site("schema_version = 7\n", None);
    let strict = moss(site.path(), &["build", "--no-plugins", "--strict"]);
    assert!(strict.status.success(), "stderr:\n{}", stderr(&strict));
    let deploy = moss(site.path(), &["deploy", "--dry-run"]);
    assert!(!stderr(&deploy).contains("newer than"), "stderr:\n{}", stderr(&deploy));
}
