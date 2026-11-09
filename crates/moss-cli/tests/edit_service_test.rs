#[path = "served_child.rs"]
mod served_child;
use served_child::*;
use std::{path::Path, process::Command, time::Duration};

fn edit(site: &Path, home: &Path, flags: &[&str]) -> ServedChild {
    let mut command = Command::new(env!("CARGO_BIN_EXE_moss-cli"));
    command.arg("edit").arg(site).arg("--serve").args(flags);
    command
        .env("MOSS_DESKTOP_SEARCH_ROOT", home)
        .env("MOSS_EDIT_EFFECTS_FILE", home.join("effects"));
    spawn_piped(command, home)
}

fn link(child: &ServedChild) -> String {
    wait_for_line(&child.stdout, "Edit at ", Duration::from_secs(45))
        .unwrap_or_else(|| panic!("no sign-in link: {}", joined(&child.stderr)))
        .strip_prefix("Edit at ")
        .unwrap()
        .into()
}

/// A free port whose next port is free too, for the UI and the preview.
fn free_pair() -> u16 {
    loop {
        let first = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = first.local_addr().unwrap().port();
        if port < u16::MAX && std::net::TcpListener::bind(("127.0.0.1", port + 1)).is_ok() {
            return port;
        }
    }
}

#[test]
#[cfg_attr(moss_ui_bundle_missing, ignore = "needs the browser bundle built")]
fn second_edit_mints_fresh_link_without_storing_codes() {
    let site = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    std::fs::write(site.path().join("index.md"), "# A garden\n").unwrap();
    std::fs::create_dir(home.path().join("moss.app")).unwrap();
    let port = free_pair().to_string();
    let first = edit(
        site.path(),
        home.path(),
        &["--no-browser", "--port", &port, "--public-url", "https://edit.example.test", "--public-preview-port", "8443"],
    );
    let first_link = link(&first);
    assert!(first_link.starts_with("https://edit.example.test/__moss/sign-in#code="));
    // The proxy is told where both listeners are, the preview at the next public port.
    let printed = joined(&first.stdout);
    let preview = port.parse::<u16>().unwrap() + 1;
    assert!(printed.contains(&format!("Proxy https://edit.example.test to http://127.0.0.1:{port}")), "{printed}");
    assert!(printed.contains(&format!("Proxy https://edit.example.test:8443 to http://127.0.0.1:{preview}")), "{printed}");
    let mut second = edit(site.path(), home.path(), &[]);
    let second_link = link(&second);
    assert_eq!(
        wait_for_exit(&mut second.child, Duration::from_secs(10))
            .unwrap()
            .code(),
        Some(0)
    );
    second.join_drain_threads();
    assert_ne!(first_link, second_link);
    assert!(joined(&second.stdout).contains('▀'));
    assert!(!home.path().join("effects").exists());
    let record = std::fs::read_dir(home.path().join("servers"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let record = std::fs::read_to_string(record).unwrap();
    let owner: serde_json::Value = serde_json::from_str(&record).unwrap();
    let ui = owner["ui_url"].as_str().unwrap();
    assert!(ui.starts_with("http://127.0.0.1:"));
    assert_eq!(owner["pid"].as_u64().unwrap(), first.child.id() as u64);
    for url in [&first_link, &second_link] {
        let code = url.split_once("#code=").unwrap().1;
        assert!(!record.contains(code));
        assert!(!joined(&first.stderr).contains(code));
        assert!(!joined(&second.stderr).contains(code));
    }
    let sign_in = |url: &str| {
        ureq::post(&format!("{ui}/__moss/sign-in"))
            .set("Origin", ui)
            .set("Sec-Fetch-Site", "same-origin")
            .send_json(serde_json::json!({"code":url.split_once("#code=").unwrap().1}))
    };
    assert!(matches!(
        sign_in(&first_link),
        Err(ureq::Error::Status(401, _))
    ));
    assert_eq!(sign_in(&second_link).unwrap().status(), 204);
    assert!(matches!(
        sign_in(&second_link),
        Err(ureq::Error::Status(401, _))
    ));
    let preview = owner["url"].as_str().unwrap();
    assert!(matches!(
        ureq::post(&format!("{preview}/__moss/sign-in/new")).send_string(""),
        Err(ureq::Error::Status(401, _))
    ));
    let sessions = moss_build::ops::serve::ui::sessions::SessionStore::new(
        home.path().join("ui-sessions.json"),
    )
    .unwrap();
    assert_eq!(sessions.list().unwrap().len(), 1);

    #[cfg(target_os = "macos")]
    terminal_reuses_owner(site.path(), home.path(), ui);
}

#[cfg(target_os = "macos")]
fn terminal_reuses_owner(site: &Path, home: &Path, ui: &str) {
    for no_browser in [false, true] {
        let effects = home.join("effects");
        let _ = std::fs::remove_file(&effects);
        let mut command = Command::new("script");
        command
            .args([
                "-q",
                "/dev/null",
                env!("CARGO_BIN_EXE_moss-cli"),
                "edit",
                "--serve",
            ])
            .arg(site);
        if no_browser {
            command.arg("--no-browser");
        }
        command
            .env("MOSS_EDIT_EFFECTS_FILE", &effects)
            .env("MOSS_DESKTOP_SEARCH_ROOT", home);
        let mut child = spawn_piped(command, home);
        assert!(wait_for_exit(&mut child.child, Duration::from_secs(15))
            .unwrap()
            .success());
        child.join_drain_threads();
        if no_browser {
            assert!(!effects.exists());
            assert!(joined(&child.stdout).contains("Edit at https://edit.example.test/"));
        } else {
            let effect = std::fs::read_to_string(&effects).unwrap();
            assert!(
                effect.starts_with(&format!("browser {ui}/__moss/sign-in#code=")),
                "{effect}"
            );
            assert!(!joined(&child.stdout).contains("Edit at "));
        }
    }
}
