use std::process::{Command, Stdio};

#[test]
fn piped_edit_never_hands_off_and_preview_always_does() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join("moss.app")).unwrap();
    let run = |verb: &str| {
        let effects = temp.path().join(format!("{verb}-effects"));
        let output = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
            .args([verb, temp.path().join("missing").to_str().unwrap()])
            .env("MOSS_DESKTOP_SEARCH_ROOT", temp.path())
            .env("MOSS_EDIT_EFFECTS_FILE", &effects)
            .env("MOSS_HOME", temp.path().join("home"))
            .stdin(Stdio::null())
            .output()
            .unwrap();
        (output, std::fs::read_to_string(&effects).ok())
    };
    let (output, effect) = run("edit");
    assert_eq!(effect, None, "piped edit handed off");
    assert!(!output.status.success(), "the missing served folder must be refused");
    let (_, effect) = run("preview");
    let effect = effect.expect("preview hands off to moss desktop without a terminal too");
    assert!(effect.starts_with("handoff preview "), "{effect}");
}

#[cfg(moss_ui_bundle_missing)]
#[test]
fn missing_bundle_refuses_with_build_command() {
    let temp = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .args(["edit", "--serve", temp.path().to_str().unwrap()])
        .env("MOSS_HOME", temp.path().join("home"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr).unwrap();
    assert_eq!(error.lines().count(), 1, "{error}");
    assert!(
        error.contains("bash scripts/build-moss-cli.sh"),
        "{error}"
    );
}

#[test]
fn sessions_list_and_revoke_persist() {
    use moss_build::ops::serve::ui::sessions::SessionStore;
    let temp = tempfile::tempdir().unwrap();
    let store = SessionStore::new(temp.path().join("ui-sessions.json")).unwrap();
    let first = store.open("browser one\ncontrol\x1b").unwrap();
    let second = store.open("browser two").unwrap();
    let sessions = store.list().unwrap();
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_moss-cli"))
            .arg("sessions")
            .args(args)
            .env("MOSS_HOME", temp.path())
            .output()
            .unwrap()
    };
    let output = run(&[]);
    assert!(output.status.success());
    let listing = String::from_utf8(output.stdout).unwrap();
    assert!(listing.contains("ID\tCREATED\tLAST SEEN\tUSER AGENT"));
    for session in &sessions {
        assert!(listing.contains(&session.id));
        assert!(listing.contains(&session.created.to_string()));
        assert!(listing.contains(&session.last_seen.to_string()));
    }
    assert!(listing.contains("browser one control "));
    assert!(listing.contains("browser two"));
    assert!(!listing.contains(&first) && !listing.contains(&second));
    assert!(run(&["revoke", &sessions[0].id]).status.success());
    assert!(!store.admits(&first, false).unwrap());
    assert!(store.admits(&second, false).unwrap());
    assert!(run(&["revoke", "--all"]).status.success());
    assert!(store.list().unwrap().is_empty());
    assert!(!store.admits(&second, false).unwrap());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(temp.path()).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
}

#[test]
fn help_and_describe_advertise_browser_edit_and_sessions() {
    for args in [
        vec!["--help"],
        vec!["edit", "--help"],
        vec!["describe", "--json"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
            .args(&args)
            .output()
            .unwrap();
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("--no-browser"), "{args:?}");
        assert!(text.contains("--public-url"));
        if args[0] != "edit" {
            assert!(text.contains("sessions"));
        }
    }
}
