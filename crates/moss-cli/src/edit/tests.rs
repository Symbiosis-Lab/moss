use super::*;
use std::cell::RefCell;

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
            public_url: Some(moss_build::ops::serve::ui::PublicOrigin::parse("https://edit.example.test").unwrap())
        }
    );
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
