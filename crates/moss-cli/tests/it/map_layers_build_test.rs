//! A `[map.layers.<name>]` table the build cannot use fails the build and
//! names the table and key; a valid one builds and writes `layers.json`.

use std::process::{Command, Stdio};

fn build(config: &str, source_dir: bool) -> (bool, String, String, tempfile::TempDir) {
    let moss_home = tempfile::tempdir().unwrap();
    let site = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(site.path().join(".moss")).unwrap();
    if source_dir {
        std::fs::create_dir_all(site.path().join("maps/atlas")).unwrap();
    }
    std::fs::write(site.path().join(".moss/config.toml"), format!("schema_version = 6\n\n[terms.places]\ntype = \"place\"\nfields = [\"location\"]\n\n{config}")).unwrap();
    std::fs::write(site.path().join(".moss/places.toml"), "[\"Marlow Quay\"]\nlat = 51.5\nlng = -0.1\nprecision = \"exact\"\n").unwrap();
    std::fs::write(site.path().join("index.md"), "---\ntitle: Home\nhome: true\n---\nhi\n").unwrap();
    let dir = std::fs::canonicalize(site.path()).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .args(["build"])
        .arg(&dir)
        .arg("--no-plugins")
        .env("MOSS_HOME", moss_home.path())
        .stdin(Stdio::null())
        .output()
        .expect("run moss-cli build");
    (out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned(), site)
}

#[test]
fn a_bad_layer_fails_the_build_naming_the_table_and_key() {
    for (config, needle, source_dir) in [
        ("[map.layers.a]\nkind = \"satellite\"\nsource = \"maps/atlas\"\n", "[map.layers.a] `kind`", true),
        ("[map.layers.a]\nkind = \"streets\"\n", "[map.layers.a] needs a `source`", true),
        ("[map.layers.a]\nkind = \"streets\"\nsource = \"maps/gone\"\n", "[map.layers.a] `source`", false),
        ("[map.layers.a]\nkind = \"streets\"\nsource = \"../out\"\n", "[map.layers.a] `source`", true),
        ("[map.layers.a]\nkind = \"streets\"\nsource = \"maps/atlas\"\nmax_zoom = 25\n", "[map.layers.a] `max_zoom`", true),
        ("[map.layers.a]\nkind = \"streets\"\nsource = \"maps/atlas\"\nbbox = [1, 2]\n", "[map.layers.a] `bbox`", true),
    ] {
        let (ok, stdout, stderr, _site) = build(config, source_dir);
        assert!(!ok, "{config}\nstdout:\n{stdout}\nstderr:\n{stderr}");
        assert!(stderr.contains(needle), "{config}\nstderr:\n{stderr}");
    }
}

#[test]
fn a_valid_layer_builds_and_writes_layers_json() {
    let (ok, stdout, stderr, site) = build("[map.layers.atlas]\nkind = \"historical\"\nsource = \"maps/atlas\"\nbbox = [-0.2, 51.4, -0.0, 51.6]\n", true);
    assert!(ok, "stdout:\n{stdout}\nstderr:\n{stderr}");
    let current = site.path().join(".moss/build.nosync/current/_moss");
    let found = std::fs::read_dir(&current).unwrap().filter_map(Result::ok).any(|e| e.path().join("layers.json").is_file());
    assert!(found, "layers.json was not written under {}", current.display());
}
