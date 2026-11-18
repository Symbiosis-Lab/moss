//! `[services.booking]` through the real CLI: the offer reaches the site's public
//! description, `/_moss/commerce.json`, and a broken one is a build problem.

use std::path::Path;
use std::process::{Command, Stdio};

const OFFER: &str = r#"[services.booking]
endpoint = "https://booking.example/sites/demo/booking"
timezone = "America/Los_Angeles"
currency = "usd"
start_every = 60
notice_hours = 12
horizon_days = 60

[services.booking.open]
mon = ["09:00-23:00"]

[services.booking.cancel]
full_before_hours = 24
late_percent = 50

[services.booking.pages]
book = "/book/"
done = "/book/done/"
manage = "/book/manage/"

[services.booking.services.tour]
name = "Guided walk"
per_hour = 4000
hours = { min = 2, max = 6 }
fields = { meeting_point = "required" }
"#;

const FORM: &str = "[services.forms]\nendpoint = \"https://forms.example/f\"\n";
const ASK: &str = ":::form {#ask next=/}\n- Your name *\n:::\n";

fn site(config: &str, home: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = std::fs::canonicalize(tmp.path()).unwrap();
    let write = |rel: &str, body: &str| {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    };
    write("index.md", &format!("---\ntitle: Home\n---\n\nWelcome.\n\n{home}"));
    for (page, title) in [("book/index.md", "Book"), ("book/done.md", "Booked"), ("book/manage.md", "Your booking")] {
        write(page, &format!("---\ntitle: {title}\n---\n\n{title}.\n"));
    }
    write(".moss/config.toml", &format!("schema_version = 6\n\n{config}"));
    (tmp, dir)
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

fn commerce(dir: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(dir.join(".moss/build.nosync/current/_moss/commerce.json")).ok()?;
    Some(serde_json::from_str(&text).unwrap())
}

#[test]
fn a_booking_offer_is_published_for_the_booking_service() {
    let (_t, dir) = site(OFFER, "");
    let (ok, stderr) = run(&dir, true);
    assert!(ok, "{stderr}");
    let json = commerce(&dir).expect("the offer alone writes commerce.json");
    let booking = &json["booking"];
    assert_eq!(booking["endpoint"], "https://booking.example/sites/demo/booking");
    assert_eq!(booking["services"]["tour"]["per_hour"], 4000);
    assert_eq!(booking["services"]["tour"]["fields"]["meeting_point"], "required");
    assert_eq!(booking["pages"]["done"], "/book/done/");
    assert_eq!(booking["cancel"]["full_before_hours"], 24);
}

#[test]
fn a_broken_offer_is_a_problem_and_is_not_published() {
    let (_t, dir) = site(&OFFER.replace("currency = \"usd\"", "currency = \"US dollars\""), "");
    let (ok, stderr) = run(&dir, false);
    assert!(ok, "{stderr}");
    assert!(stderr.contains("[services.booking] currency"), "{stderr}");
    assert!(commerce(&dir).is_none_or(|j| j.get("booking").is_none()), "a broken offer must not reach the service");
    let (strict_ok, _) = run(&dir, true);
    assert!(!strict_ok, "--strict fails on a broken offer");
}

#[test]
fn a_misspelt_booking_key_costs_the_site_nothing_else() {
    let (_t, dir) = site(&format!("{FORM}\n{}", OFFER.replace("notice_hours", "notice_hourz")), ASK);
    let (ok, stderr) = run(&dir, false);
    assert!(ok, "{stderr}");
    assert!(stderr.contains("notice_hourz"), "the problem names the key: {stderr}");
    let json = commerce(&dir).expect("the forms still write commerce.json");
    assert!(json["forms"]["ask"].is_object(), "the form is still published");
    assert!(json.get("booking").is_none(), "the broken offer is not");
    let html = std::fs::read_to_string(dir.join(".moss/build.nosync/current/index.html")).unwrap();
    assert!(html.contains(r#"action="https://forms.example/f/ask""#), "the form still posts to its endpoint");
}

#[test]
fn a_booking_page_the_build_did_not_write_is_a_problem() {
    let (_t, dir) = site(&OFFER.replace("manage = \"/book/manage/\"", "manage = \"/book/change/\""), "");
    let (ok, stderr) = run(&dir, false);
    assert!(ok, "{stderr}");
    assert!(stderr.contains("pages.manage is `/book/change/`"), "{stderr}");
}
