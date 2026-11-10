use super::*;
use std::cell::RefCell;

fn public(origin: &str, preview_port: Option<u16>) -> moss_build::ops::serve::ui::PublicUrl {
    let ui = moss_build::ops::serve::ui::PublicOrigin::parse(origin).unwrap();
    moss_build::ops::serve::ui::PublicUrl::new(ui, preview_port).unwrap()
}

fn parse(args: &[&str]) -> Result<EditArgs, String> {
    EditArgs::parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
}

#[test]
fn flags_and_default_folder() {
    assert_eq!(parse(&[]).unwrap().folder, ".");
    let parsed = parse(&[
        "--serve",
        "garden",
        "--no-browser",
        "--port",
        "9000",
        "--public-url",
        "https://edit.example.test",
    ])
    .unwrap();
    assert_eq!(
        parsed,
        EditArgs {
            folder: "garden".into(),
            serve: true,
            no_browser: true,
            port: Some(9000),
            public_url: Some(public("https://edit.example.test", None)),
        }
    );
    let funnel = parse(&["--port", "9000", "--public-url", "https://edit.example.test", "--public-preview-port", "8443"]).unwrap();
    assert_eq!(funnel.public_url, Some(public("https://edit.example.test", Some(8443))));
    assert_eq!(funnel.public_url.unwrap().preview.as_str(), "https://edit.example.test:8443");
    assert_eq!(parse(&["--", "-garden"]).unwrap().folder, "-garden");
    for args in [
        vec!["--port"],
        vec!["--port=0"],
        vec!["--port=65536"],
        vec!["--port=no"],
        vec!["--unknown=x"],
        vec!["a", "b"],
        vec!["--public-url=http://example.test"],
        vec!["--public-url=https://example.test/path"],
        // A proxy forwards to fixed ports.
        vec!["--public-url=https://example.test"],
        vec!["--port=9000", "--public-preview-port=8443"],
        vec!["--port=9000", "--public-url=https://example.test", "--public-preview-port=443"],
        vec!["--port=9000", "--public-url=https://example.test", "--public-preview-port=0"],
    ] {
        assert!(parse(&args).is_err(), "accepted {args:?}");
    }
}

#[test]
fn terminal_opens_loopback_and_print_modes_only_print_public_link_and_qr() {
    let local = "http://localhost:8081/__moss/sign-in#code=local-secret";
    let public = "https://edit.example.test/__moss/sign-in#code=local-secret";
    for (terminal, no_browser, opens) in [
        (true, false, true),
        (true, true, false),
        (false, false, false),
        (false, true, false),
    ] {
        let opened = RefCell::new(Vec::new());
        let mut output = Vec::new();
        present::present(
            terminal,
            no_browser,
            local,
            public,
            &|url| {
                opened.borrow_mut().push(url.to_string());
                Ok(())
            },
            &mut output,
        )
        .unwrap();
        if opens {
            assert_eq!(*opened.borrow(), vec![local]);
            assert!(output.is_empty());
        } else {
            assert!(opened.borrow().is_empty());
            let output = String::from_utf8(output).unwrap();
            assert!(output.starts_with(&format!("Edit at {public}\n")));
            assert!(output.contains('▀') && output.contains('▄') && output.contains('█'));
            assert!(!output.contains("localhost"));
        }
    }
}

#[test]
fn a_flag_for_the_served_editor_is_never_handed_to_the_app() {
    let installed = || true;
    assert!(hands_off(&parse(&["garden"]).unwrap(), true, installed));
    for flags in [&["--serve", "garden"][..], &["--no-browser", "garden"], &["--port", "9000", "garden"]] {
        assert!(!hands_off(&parse(flags).unwrap(), true, installed), "{flags:?} went to the app");
    }
    assert!(!hands_off(&parse(&["garden"]).unwrap(), false, installed), "no terminal still went to the app");
    assert!(!hands_off(&parse(&["garden"]).unwrap(), true, || false), "no app to hand to");
}
