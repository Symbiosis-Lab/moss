//! `moss comments` against a real build of a small site: a hidden comment
//! leaves the built page at the next build, and an unhidden one comes back.
//! All names and text are invented.

use std::path::Path;
use std::process::{Command, Output};

const MOSS: &str = env!("CARGO_BIN_EXE_moss-cli");

fn moss(home: &Path, args: &[&str]) -> Output {
    Command::new(MOSS)
        .args(args)
        .env("MOSS_HOME", home)
        .env_remove("MOSS_ENV")
        .env_remove("MOSS_SETA_URL")
        .output()
        .expect("spawn moss-cli")
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn built_page(site: &Path) -> String {
    std::fs::read_to_string(site.join(".moss/build.nosync/current/posts/hello/index.html")).expect("built page")
}

fn build(home: &Path, site: &Path) -> String {
    let o = moss(home, &["build", site.to_str().unwrap(), "--no-plugins"]);
    assert!(o.status.success(), "build failed: {}", String::from_utf8_lossy(&o.stderr));
    built_page(site)
}

#[test]
fn hide_and_unhide_change_the_built_page_and_the_json_shapes_hold() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let site = tmp.path().join("site");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(site.join(".moss/data/social")).unwrap();
    std::fs::create_dir_all(site.join("posts")).unwrap();
    std::fs::write(site.join(".moss/config.toml"), "[services.comments]\nprovider = \"artalk\"\n").unwrap();
    std::fs::write(site.join("index.md"), "---\ntitle: Home\n---\n\nWelcome.\n").unwrap();
    std::fs::write(site.join("posts/hello.md"), "---\ntitle: Hello Garden\n---\n\nA post about gardens.\n").unwrap();
    moss_build::identity::keypair::Identity::generate().unwrap().save(&site).unwrap();
    let site_str = site.to_str().unwrap();

    // The first build tells us the page's uid; comments are keyed by it.
    build(&home, &site);
    let map: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(site.join(".moss/build.nosync/article-map.json")).unwrap()).unwrap();
    let uid = map["articles"]["posts/hello/"]["uid"].as_str().unwrap().to_string();
    let comments = serde_json::json!({"articles": {uid.clone(): {"comments": [
        {"id": "1", "content": "<p>Cheap watches at example.com</p>", "createdAt": "2026-03-01T10:00:00Z", "author": {"displayName": "Spammy Sam"}},
        {"id": "2", "content": "<p>Reply from the spammer</p>", "createdAt": "2026-03-02T10:00:00Z", "author": {"displayName": "Spammy Sam"}, "replyToId": "1"},
        {"id": "3", "content": "<p>Lovely garden</p>", "createdAt": "2026-03-03T10:00:00Z", "author": {"displayName": "Ana Example"}}
    ]}}});
    std::fs::write(site.join(".moss/data/social/comment.json"), comments.to_string()).unwrap();

    let page = build(&home, &site);
    assert!(page.contains("Cheap watches") && page.contains("Reply from the spammer") && page.contains("Lovely garden"));

    // list works and names the page now that a build has run.
    let listed: serde_json::Value = serde_json::from_str(&out(&moss(&home, &["comments", "list", site_str, "--json"]))).unwrap();
    assert_eq!(listed[0]["id"], "3", "newest first");
    assert_eq!(listed[0]["pageUrl"], "/posts/hello/");
    assert_eq!(listed[0]["pageTitle"], "Hello Garden");
    assert_eq!(listed[0]["hidden"], false);

    let hid: serde_json::Value = serde_json::from_str(&out(&moss(&home, &["comments", "hide", site_str, "1", "--json"]))).unwrap();
    assert_eq!(hid["action"], "hide");
    assert_eq!(hid["changed"][0]["id"], "1");
    assert_eq!(hid["changed"][0]["seq"], 1);
    assert_eq!(hid["unchanged"].as_array().unwrap().len(), 0);

    let page = build(&home, &site);
    assert!(!page.contains("Cheap watches"), "hidden comment is gone");
    assert!(!page.contains("Reply from the spammer"), "hiding a parent removes its replies");
    assert!(page.contains("Lovely garden"));

    let listed: serde_json::Value = serde_json::from_str(&out(&moss(&home, &["comments", "list", site_str, "--json"]))).unwrap();
    let hidden: Vec<&str> = listed.as_array().unwrap().iter().filter(|r| r["hidden"] == true).map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(hidden.len(), 2);

    let again: serde_json::Value = serde_json::from_str(&out(&moss(&home, &["comments", "hide", site_str, "1", "--json"]))).unwrap();
    assert_eq!(again["changed"].as_array().unwrap().len(), 0);
    assert_eq!(again["unchanged"][0]["reason"], "already hidden");

    let bad = moss(&home, &["comments", "hide", site_str, "nope", "--json"]);
    assert!(!bad.status.success());
    let err: serde_json::Value = serde_json::from_str(&out(&bad)).unwrap();
    assert!(err["error"].as_str().unwrap().contains("nope"));

    assert!(moss(&home, &["comments", "unhide", site_str, "1"]).status.success());
    let page = build(&home, &site);
    assert!(page.contains("Cheap watches") && page.contains("Reply from the spammer"), "unhide brings them back");
}
