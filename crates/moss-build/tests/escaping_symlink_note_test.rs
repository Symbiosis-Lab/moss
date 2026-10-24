//! A symlink that points outside the site folder is skipped either way, so it
//! is a note, not a build problem: `--strict` can pass. A page that links to
//! it still reports the dead link.

#[path = "support/site.rs"]
mod site;

use std::fs;

fn site_with_escaping_link(page_body: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("moss_escape_site_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(root.join(".moss")).unwrap();
    fs::write(root.join(".moss/config.toml"), "schema_version = 6\n\n[site]\nlang = \"en\"\n").unwrap();
    fs::write(root.join("index.md"), format!("---\ntitle: Example Lab\n---\n\n{page_body}\n")).unwrap();
    std::os::unix::fs::symlink("..", root.join("outside-link")).unwrap();
    root
}

fn build(root: &std::path::Path) -> Vec<String> {
    site::build_sync(&root.to_string_lossy(), moss_build::build::BuildTrigger::Full).expect("builds")
}

#[cfg(unix)]
#[test]
fn an_escaping_symlink_is_a_note_unless_a_page_links_to_it() {
    let quiet = site_with_escaping_link("Welcome.");
    let logged = build(&quiet);
    assert!(logged.iter().any(|l| l.contains("outside-link") && l.contains("Remove it")), "the note names the file and what to do: {logged:?}");
    assert!(site::problems().is_empty(), "a skipped symlink must not fail --strict: {:?}", site::problems());
    let _ = fs::remove_dir_all(&quiet);

    let linked = site_with_escaping_link("See [the link](outside-link).");
    let logged = build(&linked);
    assert!(!site::problems().is_empty(), "a page linking to the skipped file still reports a problem: {logged:?}");
    let _ = fs::remove_dir_all(&linked);
}

/// A Finder alias made with osascript; the test is skipped where Finder is unavailable.
#[cfg(target_os = "macos")]
#[test]
fn an_escaping_finder_alias_is_a_note_too() {
    let outside = std::env::temp_dir().join(format!("moss_alias_target_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&outside).unwrap();
    let root = site_with_escaping_link("Welcome.");
    fs::remove_file(root.join("outside-link")).unwrap();
    let script = format!(
        r#"tell application "Finder" to make alias to (POSIX file "{}" as alias) at (POSIX file "{}" as alias)"#,
        outside.display(),
        root.display()
    );
    let made = std::process::Command::new("osascript").args(["-e", &script]).status().map(|s| s.success()).unwrap_or(false);
    if !made {
        eprintln!("skipping test: Finder alias could not be made");
    } else {
        let logged = build(&root);
        assert!(logged.iter().any(|l| l.contains("Skipping alias") && l.contains("Remove it")), "{logged:?}");
        assert!(site::problems().is_empty(), "{:?}", site::problems());
    }
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&outside);
}
