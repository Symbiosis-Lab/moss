//! A folder named in Chinese gives every page under it an English address
//! with `url:` in its own `index.md`, and the folder keeps its name.
use std::path::Path;
use std::process::{Command, Stdio};

#[test]
fn a_folder_sets_the_address_of_every_page_under_it() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, root) = (tmp.path().join("home"), tmp.path().join("site"));
    let pages = [
        ("index.md", "Home."),
        ("河流/index.md", "---\nurl: river\n---\n# 河流"),
        ("河流/flood.md", "# Flood"),
        ("河流/seasons/spring.md", "# Spring"),
    ];
    for (rel, text) in pages {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    std::fs::create_dir_all(&home).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .args(["build", root.to_str().unwrap(), "--no-plugins", "--json"])
        .env("MOSS_HOME", &home)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("one JSON object on stdout");
    let built = Path::new(report["output"].as_str().expect("an output path"));

    for page in ["river/index.html", "river/flood/index.html", "river/seasons/spring/index.html"] {
        assert!(built.join(page).is_file(), "{page} is not built");
    }
    assert!(!built.join("河流").exists(), "the folder's own name is still an address");
    assert!(root.join("河流/index.md").is_file(), "the folder was renamed");
}
