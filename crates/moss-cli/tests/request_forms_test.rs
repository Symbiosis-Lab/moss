//! `:::form` through the real CLI: the markup, the site's public description
//! of its forms, and the build problems (a warning, a failure under `--strict`).

use std::path::Path;
use std::process::{Command, Stdio};

const ENDPOINT: &str = "[services.forms]\nendpoint = \"https://forms.example/f\"\n";
const BOOKING: &str = ":::form {#booking next=/booking/received/ button=\"Ask for a date\"}\n- Your name * {autocomplete=name}\n- Email * {type=email}\n- Session * {type=radio}\n  - Portrait\n  - Family\n:::\n";

fn write(site: &Path, rel: &str, body: &str) {
    let p = site.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

fn run(site: &Path, strict: bool) -> (bool, String) {
    let home = tempfile::tempdir().unwrap();
    let mut args = vec!["build", site.to_str().unwrap(), "--no-plugins"];
    if strict {
        args.push("--strict");
    }
    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .args(args)
        .env("MOSS_HOME", home.path())
        .env("MOSS_SITE_URL", "https://example.com")
        .stdin(Stdio::null())
        .output()
        .expect("run moss-cli");
    (out.status.success(), String::from_utf8_lossy(&out.stderr).into_owned())
}

/// A site with a booking page, its thank-you page and `config` appended to the config.
fn site(config: &str, booking_md: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = std::fs::canonicalize(tmp.path()).unwrap();
    write(&dir, "index.md", "---\ntitle: Home\n---\n\nWelcome.\n");
    write(&dir, "booking/index.md", &format!("---\ntitle: Booking\n---\n\n{booking_md}"));
    write(&dir, "booking/received.md", "---\ntitle: Received\n---\n\nThanks.\n");
    if !config.is_empty() {
        write(&dir, ".moss/config.toml", &format!("schema_version = 6\n\n{config}"));
    }
    (tmp, dir)
}

fn built(dir: &Path, rel: &str) -> String {
    std::fs::read_to_string(dir.join(".moss/build.nosync/current").join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

#[test]
fn a_form_with_an_endpoint_builds_clean_and_posts_there() {
    let (_t, dir) = site(ENDPOINT, BOOKING);
    let (ok, stderr) = run(&dir, true);
    assert!(ok, "{stderr}");
    let html = built(&dir, "booking/index.html");
    assert!(html.contains(r#"action="https://forms.example/f/booking""#), "{html}");
    assert!(html.contains(r#"<label class="moss-form__label" for="booking-your_name">Your name</label>"#));
    assert!(html.contains(">Ask for a date<"));
    assert!(html.contains("/_moss/js/forms."), "the page loads the form script");
    let sheet = html.split("href=\"/_moss/style.").nth(1).and_then(|r| r.split('"').next()).expect("a stylesheet link");
    assert!(built(&dir, &format!("_moss/style.{sheet}")).contains(".moss-form__field"), "the stylesheet styles the form");
    assert!(!built(&dir, "index.html").contains("moss-form__field"));
}

#[test]
fn the_commerce_description_lists_each_form() {
    let (_t, dir) = site(ENDPOINT, BOOKING);
    assert!(run(&dir, false).0);
    let json: serde_json::Value = serde_json::from_str(&built(&dir, "_moss/commerce.json")).unwrap();
    let f = &json["forms"]["booking"];
    assert_eq!(f["next"], "/booking/received/");
    assert_eq!(f["turnstile"], false);
    assert_eq!(f["fields"][1], serde_json::json!({"name": "email", "type": "email", "required": true}));
    assert_eq!(f["fields"][2]["options"], serde_json::json!(["Portrait", "Family"]));
}

#[test]
fn the_description_exists_for_a_setting_alone_and_not_for_a_site_with_neither() {
    let (_t, with_setting) = site(ENDPOINT, "No form here.\n");
    assert!(run(&with_setting, true).0);
    assert_eq!(built(&with_setting, "_moss/commerce.json"), r#"{"forms":{}}"#);

    let (_t2, neither) = site("", "No form here.\n");
    assert!(run(&neither, true).0);
    assert!(!neither.join(".moss/build.nosync/current/_moss/commerce.json").exists());
}

#[test]
fn a_site_on_the_moss_host_tells_it_the_subscribe_form_passes_its_bot_check() {
    let (_t, dir) = site("", "No form here.\n");
    write(&dir, ".moss/state.toml", "[deployment]\nsite_id = \"harbor-notes\"\ndeploy_method = \"moss\"\n");
    assert!(run(&dir, true).0);
    let json: serde_json::Value = serde_json::from_str(&built(&dir, "_moss/commerce.json")).unwrap();
    assert_eq!(json["subscribe"], serde_json::json!({"turnstile": true}));
}

#[test]
fn no_endpoint_warns_naming_the_setting_fails_strict_and_leaves_a_pending_form_for_the_preview() {
    let (_t, dir) = site("", BOOKING);
    let (ok, stderr) = run(&dir, false);
    assert!(ok, "{stderr}");
    assert!(stderr.contains("[services.forms] endpoint"), "{stderr}");
    assert!(built(&dir, "booking/index.html").contains(r#"data-moss-pending="true""#));
    let (strict_ok, strict_err) = run(&dir, true);
    assert!(!strict_ok, "{strict_err}");
}

#[test]
fn each_build_problem_is_a_warning_and_fails_under_strict() {
    let cases: &[(&str, &str)] = &[
        ("unknown type", ":::form {#booking next=/booking/received/}\n- A {type=banana}\n:::\n"),
        ("two fields, one name", ":::form {#booking next=/booking/received/}\n- A\n- A\n:::\n"),
        ("too few choices", ":::form {#booking next=/booking/received/}\n- A {type=select}\n  - Only\n:::\n"),
        ("choices on text", ":::form {#booking next=/booking/received/}\n- A\n  - x\n  - y\n:::\n"),
        ("no Latin letters", ":::form {#booking next=/booking/received/}\n- 姓名\n:::\n"),
        ("reserved name", ":::form {#booking next=/booking/received/}\n- A {name=_hp}\n:::\n"),
        ("bad form name", ":::form {#Booking_1 next=/booking/received/}\n- A\n:::\n"),
        ("next is not a path", ":::form {#booking next=https://elsewhere.test/}\n- A\n:::\n"),
        ("next names no page", ":::form {#booking next=/nowhere/}\n- A\n:::\n"),
    ];
    for (what, md) in cases {
        let (_t, dir) = site(ENDPOINT, md);
        let (ok, stderr) = run(&dir, false);
        assert!(ok && stderr.contains(":::form"), "{what}: {stderr}");
        assert!(!run(&dir, true).0, "{what} must fail under --strict");
    }
}

#[test]
fn next_may_be_a_generated_listing_page_but_not_a_missing_one() {
    let md = |next: &str| format!(":::form {{#booking next={next}}}\n- A\n:::\n");
    let (_t, dir) = site(ENDPOINT, &md("/notes/"));
    write(&dir, "notes/first.md", "---\ntitle: First\n---\n\nHello.\n");
    let (ok, stderr) = run(&dir, true);
    assert!(ok, "a folder listing is a page the build writes: {stderr}");

    let (_t2, missing) = site(ENDPOINT, &md("/nowhere/"));
    let (ok, stderr) = run(&missing, true);
    assert!(!ok && stderr.contains("next=/nowhere/") && stderr.contains("#booking"), "{stderr}");
}

#[test]
fn two_forms_with_one_name_are_a_problem() {
    let (_t, dir) = site(ENDPOINT, BOOKING);
    write(&dir, "second.md", &format!("---\ntitle: Second\n---\n\n{BOOKING}"));
    let (ok, stderr) = run(&dir, false);
    assert!(ok && stderr.contains("more than one form"), "{stderr}");
    assert!(!run(&dir, true).0);
}
