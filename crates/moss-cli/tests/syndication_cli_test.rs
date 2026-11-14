//! `moss deploy` posting copies through a channel plugin, end to end: a real
//! build and publish (through a deploy plugin that only reports an address),
//! then the copies, against a small HTTP server standing in for a network.
//! All names and addresses are invented.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};

const MOSS: &str = env!("CARGO_BIN_EXE_moss-cli");

/// What the stand-in network answers, and every request it received.
#[derive(Default)]
struct Network {
    /// `/publish` fails with a 500 while set, which the channel turns into a throw.
    failing: bool,
    /// The published pages answer 404 while set, as before an edge has them.
    pages_down: bool,
    /// `/publish` refuses the text while set.
    refusing: bool,
    /// `/find` fails with a 500 while set, so whether the copy exists stays unknown.
    find_failing: bool,
    /// `/find` reports this copy as present.
    holds: Option<(String, String)>,
    /// `/publish` hands the copy to the author to post while set.
    handing_off: bool,
    requests: Vec<(String, serde_json::Value)>,
}

fn serve(net: Arc<Mutex<Network>>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let mut words = line.split_whitespace();
            let method = words.next().unwrap_or("").to_string();
            let path = words.next().unwrap_or("/").to_string();
            let mut len = 0usize;
            loop {
                let mut h = String::new();
                reader.read_line(&mut h).unwrap();
                if h.trim().is_empty() {
                    break;
                }
                if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0; len];
            reader.read_exact(&mut body).unwrap();
            let job: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            if method == "GET" {
                // The published page, as a reader elsewhere fetches it.
                let status = if net.lock().unwrap().pages_down { "404 Not Found" } else { "200 OK" };
                let _ = write!(&stream, "HTTP/1.1 {status}\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok");
                continue;
            }
            let (status, reply) = {
                let mut n = net.lock().unwrap();
                n.requests.push((path.clone(), job));
                let i = n.requests.len();
                match path.as_str() {
                    "/publish" if n.failing => (500, serde_json::json!({})),
                    "/publish" if n.refusing => (422, serde_json::json!({"error": "text too long"})),
                    "/publish" if n.handing_off => (200, serde_json::json!({"handoff": true})),
                    "/find" if n.find_failing => (500, serde_json::json!({})),
                    "/find" => match &n.holds {
                        Some((id, url)) => (200, serde_json::json!({"found": true, "id": id, "url": url})),
                        None => (200, serde_json::json!({"found": false})),
                    },
                    _ => (200, serde_json::json!({"id": format!("copy-{i}"), "url": format!("https://example.social/@a/{i}")})),
                }
            };
            let reply = reply.to_string();
            let mut stream = stream;
            let _ = write!(stream, "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}", reply.len());
        }
    });
    port
}

/// The channel's code: one request per copy, the network's answer as the copy.
fn channel_js(port: u16) -> String {
    format!(
        r#"
async function call(path, job) {{
  var r = await fetch("http://127.0.0.1:{port}" + path, {{ method: "POST", headers: {{ "content-type": "application/json" }}, body: JSON.stringify(job) }});
  if (r.status === 422) return {{ refused: (await r.json()).error }};
  if (!r.ok) throw new Error("network error " + r.status);
  return await r.json();
}}
var FixturePlugin = {{
  publish_copy: async function (job) {{
    var c = await call("/publish", job);
    if (c.refused) return {{ success: false, copy: {{ refused: {{ cause: "content", message: c.refused }} }} }};
    if (c.handoff) return {{ success: true, copy: {{ handoff: {{ text: "paste me", open_url: "https://example.social/new" }} }} }};
    return {{ success: true, copy: {{ id: c.id, url: c.url }} }};
  }},
  update_copy: async function (job) {{ var c = await call("/update", job); return {{ success: true, copy: {{ id: job.previous.id, url: job.previous.url }} }}; }},
  find_copy: async function (job) {{ var c = await call("/find", job); return {{ success: true, copy: c.found ? {{ id: c.id, url: c.url }} : {{ absent: true }} }}; }}
}};
window.FixturePlugin = FixturePlugin;
"#
    )
}

struct Site {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    root: PathBuf,
}

fn site(port: u16, idempotent: bool, folder_names_channel: bool) -> Site {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let root = tmp.path().join("site");
    let w = |rel: &str, text: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    };
    std::fs::create_dir_all(&home).unwrap();
    w("index.md", "---\ntitle: Home\n---\nWelcome.\n");
    w("notes/notes.md", &notes_note(folder_names_channel));
    w("notes/first.md", "The first note, with [a link](https://example.org).\n");
    w(".moss/config.toml", "[hooks]\ndeploy = \"localdir\"\n\n[channels.fixture]\n");
    w(
        ".moss/plugins/localdir/manifest.json",
        r#"{"name":"localdir","version":"0.0.1","display_name":"Local","description":"","author":"","entry":"main.bundle.js","global_name":"LocaldirPlugin","contributes":{"deploy_target":{}}}"#,
    );
    w(
        ".moss/plugins/localdir/main.bundle.js",
        &format!(r#"window.LocaldirPlugin = {{ deploy: async function () {{ return {{ success: true, deployment: {{ method: "local", url: "http://127.0.0.1:{port}", deployed_at: new Date().toISOString(), metadata: {{}} }} }}; }} }};"#),
    );
    w(
        ".moss/plugins/fixture/manifest.json",
        &serde_json::json!({
            "name": "fixture", "version": "0.0.1", "display_name": "Fixture", "description": "", "author": "",
            "entry": "main.bundle.js", "global_name": "FixturePlugin", "domains": ["example.social"],
            "contributes": {"channel": {"syndication": {"forms": ["short", "thread", "teaser"], "max_chars": 300, "idempotent": idempotent}}}
        })
        .to_string(),
    );
    w(".moss/plugins/fixture/main.bundle.js", &channel_js(port));
    Site { _tmp: tmp, home, root }
}

fn notes_note(names_channel: bool) -> String {
    if names_channel {
        "---\ntitle: Notes\ncascade:\n  syndicate: [fixture]\n---\n".into()
    } else {
        "---\ntitle: Notes\n---\n".into()
    }
}

fn deploy(s: &Site, extra: &[&str]) -> Output {
    let mut args = vec!["deploy", s.root.to_str().unwrap(), "--allow-plugins"];
    args.extend_from_slice(extra);
    let o = Command::new(MOSS)
        .args(&args)
        .env("MOSS_HOME", &s.home)
        .env_remove("MOSS_ENV")
        .env_remove("MOSS_SETA_URL")
        .output()
        .expect("spawn moss-cli");
    assert!(o.status.success(), "deploy failed: {}", String::from_utf8_lossy(&o.stderr));
    o
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap()
}

fn paths(net: &Arc<Mutex<Network>>) -> Vec<String> {
    net.lock().unwrap().requests.iter().map(|(p, _)| p.clone()).collect()
}

#[test]
fn a_new_post_is_copied_once_its_address_is_written_back_and_an_edit_updates_the_copy() {
    let net = Arc::new(Mutex::new(Network::default()));
    let s = site(serve(net.clone()), true, true);

    let first = deploy(&s, &[]);
    assert!(stderr(&first).contains("posted to fixture: https://example.social/@a/1"), "{}", stderr(&first));
    assert_eq!(paths(&net), ["/publish"]);
    let job = net.lock().unwrap().requests[0].1.clone();
    assert!(job["post"]["url"].as_str().unwrap().ends_with("/notes/first/"), "{}", job["post"]["url"]);
    assert_eq!(job["op"], "publish");
    let canonical = job["post"]["url"].as_str().unwrap();
    let text = job["parts"][0]["text"].as_str().unwrap();
    assert!(text.contains(canonical), "the posted payload links to its page: {job}");
    assert!(text.chars().count() <= 300);
    assert!(job["parts"][0]["links"].as_array().unwrap().iter().any(|link| link == canonical));
    let page = read(&s.root, "notes/first.md");
    assert!(page.contains("syndicated:\n  - https://example.social/@a/1\n"), "{page}");
    assert!(page.ends_with("The first note, with [a link](https://example.org).\n"), "the body is untouched: {page}");

    // The write-back is an edit, but not one the copy shows: nothing is sent.
    deploy(&s, &[]);
    assert_eq!(paths(&net), ["/publish"], "a second publish posts nothing");

    std::fs::write(s.root.join("notes/first.md"), page.replace("The first note", "The first note, edited")).unwrap();
    deploy(&s, &[]);
    assert_eq!(paths(&net), ["/publish", "/update"]);
    let update = net.lock().unwrap().requests[1].1.clone();
    assert_eq!(update["previous"]["id"], "copy-1");
    assert_eq!(update["idempotency_key"], job["idempotency_key"], "the key follows the post and channel, not its content");
}

#[test]
fn an_interrupted_send_on_a_network_that_cannot_retry_is_looked_up_never_posted_twice() {
    let net = Arc::new(Mutex::new(Network { failing: true, ..Default::default() }));
    let s = site(serve(net.clone()), false, true);

    let failed = deploy(&s, &[]);
    assert!(stderr(&failed).contains("unconfirmed on fixture"), "{}", stderr(&failed));
    assert_eq!(paths(&net), ["/publish"]);

    // The copy did reach the network: the lookup finds it and nothing is posted again.
    {
        let mut n = net.lock().unwrap();
        n.failing = false;
        n.holds = Some(("copy-x".into(), "https://example.social/@a/x".into()));
    }
    let found = deploy(&s, &[]);
    assert!(stderr(&found).contains("posted to fixture: https://example.social/@a/x"), "{}", stderr(&found));
    assert_eq!(paths(&net), ["/publish", "/find"]);
    assert!(read(&s.root, "notes/first.md").contains("https://example.social/@a/x"));
}

#[test]
fn a_lookup_that_finds_nothing_lets_the_copy_be_sent() {
    let net = Arc::new(Mutex::new(Network { failing: true, ..Default::default() }));
    let s = site(serve(net.clone()), false, true);
    deploy(&s, &[]);
    net.lock().unwrap().failing = false;
    let o = deploy(&s, &[]);
    assert_eq!(paths(&net), ["/publish", "/find", "/publish"]);
    assert!(stderr(&o).contains("posted to fixture"), "{}", stderr(&o));
}

#[test]
fn an_unconfirmed_copy_waits_for_the_author_and_retry_copies_sends_it() {
    let net = Arc::new(Mutex::new(Network { failing: true, ..Default::default() }));
    let s = site(serve(net.clone()), false, true);
    deploy(&s, &[]);
    {
        let mut n = net.lock().unwrap();
        n.failing = false;
        n.find_failing = true;
    }
    let waiting = deploy(&s, &[]);
    assert!(stderr(&waiting).contains("--retry-copies=fixture"), "{}", stderr(&waiting));
    assert_eq!(paths(&net), ["/publish", "/find"], "nothing is sent while the lookup cannot answer");

    let retried = deploy(&s, &["--retry-copies=fixture"]);
    assert!(stderr(&retried).contains("posted to fixture"), "{}", stderr(&retried));
    assert_eq!(paths(&net), ["/publish", "/find", "/find", "/publish"]);
}

/// A copy handed to the author rests there: the network finding nothing does
/// not hand it over again, nor does retrying the channel; retrying it for the
/// one page does.
#[test]
fn a_handed_off_copy_is_handed_over_again_only_for_the_page_named() {
    let net = Arc::new(Mutex::new(Network { handing_off: true, ..Default::default() }));
    let s = site(serve(net.clone()), false, true);
    assert!(stderr(&deploy(&s, &[])).contains("ready to paste into fixture"));
    let quiet = [deploy(&s, &[]), deploy(&s, &["--retry-copies=fixture"])];
    assert!(quiet.iter().all(|o| !stderr(o).contains("ready to paste")), "{}", stderr(&quiet[1]));
    assert_eq!(paths(&net), ["/publish", "/find", "/find"]);
    let misspelt = deploy(&s, &["--retry-copies=fixture", "--copies-of=notes/frist.md"]);
    assert!(stderr(&misspelt).contains("--copies-of=notes/frist.md names no page of this site; no copies sent"), "{}", stderr(&misspelt));
    let again = deploy(&s, &["--retry-copies=fixture", "--copies-of=notes/first.md"]);
    assert!(!stderr(&again).contains("names no page"), "{}", stderr(&again));
    assert!(stderr(&again).contains("ready to paste into fixture"), "{}", stderr(&again));
    assert_eq!(paths(&net), ["/publish", "/find", "/find", "/find", "/publish"]);
}

#[test]
fn naming_a_channel_for_a_folder_does_not_post_its_earlier_posts() {
    let net = Arc::new(Mutex::new(Network::default()));
    let s = site(serve(net.clone()), true, false);
    deploy(&s, &[]);
    assert!(paths(&net).is_empty());

    std::fs::write(s.root.join("notes/notes.md"), notes_note(true)).unwrap();
    std::fs::write(s.root.join("notes/second.md"), "A second note.\n").unwrap();
    deploy(&s, &[]);
    let sent = net.lock().unwrap().requests.clone();
    assert_eq!(sent.len(), 1, "only the post that went live with the channel named is posted");
    assert_eq!(sent[0].1["post"]["source_path"], "notes/second.md");
}

#[test]
fn a_refused_copy_uses_the_edited_page_and_can_choose_a_different_form() {
    let net = Arc::new(Mutex::new(Network { refusing: true, ..Default::default() }));
    let s = site(serve(net.clone()), false, true);
    let manifest_path = s.root.join(".moss/plugins/fixture/manifest.json");
    let mut manifest: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["contributes"]["channel"]["syndication"]["forms"] = serde_json::json!(["short", "long"]);
    std::fs::write(manifest_path, manifest.to_string()).unwrap();
    let refused = deploy(&s, &[]);
    assert!(stderr(&refused).contains("not posted to fixture (content): text too long"), "{}", stderr(&refused));
    assert_eq!(net.lock().unwrap().requests[0].1["form"], "short");
    let records = moss_build::syndication::record::Records::in_vault(&moss_build::moss_paths::MossPaths::new(&s.root));
    let before = records.all().pop().unwrap();
    assert!(before.form.is_none() && before.part_count.is_none(), "a refused attempt has no posted form: {before:?}");
    let stamped = read(&s.root, "notes/first.md");
    assert!(stamped.contains(&before.uid), "{stamped}");
    let edited = "The edited page is now much longer. ".repeat(25);
    std::fs::write(s.root.join("notes/first.md"), format!("{stamped}\n{edited}\n")).unwrap();
    net.lock().unwrap().refusing = false;
    let posted = deploy(&s, &[]);
    assert!(stderr(&posted).contains("posted to fixture"), "{}", stderr(&posted));
    let sent = net.lock().unwrap().requests.clone();
    assert_eq!(paths(&net), ["/publish", "/publish"], "a refusal is retried as a publish, with no lookup");
    assert_eq!(sent[1].1["form"], "long");
    assert!(sent[1].1["article"]["html"].as_str().unwrap().contains(edited.trim_end()), "{}", sent[1].1);
    assert_eq!(sent[0].1["idempotency_key"], sent[1].1["idempotency_key"]);
    assert_eq!(records.all()[0].uid, before.uid);
}

#[test]
fn a_dry_run_shows_each_copy_shaped_and_sends_nothing() {
    let net = Arc::new(Mutex::new(Network::default()));
    let s = site(serve(net.clone()), true, true);
    let o = deploy(&s, &["--dry-run", "--json"]);
    let report: serde_json::Value = serde_json::from_slice(&o.stdout).expect("the dry run prints JSON");
    let copies = report["copies"].as_array().expect("copies listed");
    assert_eq!(copies.len(), 1, "{report}");
    assert_eq!(copies[0]["channel"], "fixture");
    assert_eq!(copies[0]["action"], "due");
    assert!(copies[0]["parts"][0]["chars"].as_u64().unwrap() > 0);
    assert_eq!(copies[0]["parts"][0]["max_chars"], 300);
    assert!(paths(&net).is_empty(), "a dry run sends nothing");
    assert!(!s.root.join(".moss/deploy/copies").exists(), "and records nothing");
}

/// A note named only by its date has no title, so its copy is its own text:
/// one paragraph or several fitting parts as a short post. The build gives it a label from
/// its opening words, which must not make it a titled article sent as a teaser.
#[test]
fn a_note_named_by_its_date_goes_out_as_its_own_text() {
    let net = Arc::new(Mutex::new(Network::default()));
    let s = site(serve(net.clone()), true, true);
    std::fs::write(s.root.join("notes/202611061432.md"), "Hello from the river.\n").unwrap();
    std::fs::write(s.root.join("notes/2026-11-06 1500.md"), "A thread, in three parts.\n\n---\n\nThe second part.\n\n---\n\nAnd the third.\n").unwrap();
    let o = deploy(&s, &["--dry-run", "--json"]);
    let report: serde_json::Value = serde_json::from_slice(&o.stdout).expect("the dry run prints JSON");
    let form = |name: &str| {
        let copy = report["copies"].as_array().unwrap().iter().find(|c| c["source_path"].as_str().is_some_and(|p| p.ends_with(name)));
        copy.unwrap_or_else(|| panic!("no copy of {name}: {report}"))["form"].clone()
    };
    assert_eq!(form("202611061432.md"), "short", "{report}");
    assert_eq!(form("2026-11-06 1500.md"), "short", "{report}");
}

#[test]
fn a_copy_waits_until_its_page_answers() {
    let net = Arc::new(Mutex::new(Network { pages_down: true, ..Default::default() }));
    let s = site(serve(net.clone()), true, true);
    let held = deploy(&s, &[]);
    assert!(stderr(&held).contains("held for fixture: its page does not answer yet"), "{}", stderr(&held));
    assert!(paths(&net).is_empty());
    net.lock().unwrap().pages_down = false;
    deploy(&s, &[]);
    assert_eq!(paths(&net), ["/publish"]);
}

#[test]
fn dry_run_chooses_short_then_long_and_honors_footer_settings() {
    let net = Arc::new(Mutex::new(Network::default()));
    let s = site(serve(net.clone()), true, true);
    let manifest_path = s.root.join(".moss/plugins/fixture/manifest.json");
    let mut manifest: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["contributes"]["channel"]["syndication"]["forms"] = serde_json::json!(["short", "long"]);
    std::fs::write(&manifest_path, manifest.to_string()).unwrap();
    std::fs::write(s.root.join("notes/first.md"), "---\ntitle: Titled\n---\nFits.").unwrap();
    let preview = || -> serde_json::Value { serde_json::from_slice(&deploy(&s, &["--dry-run", "--json"]).stdout).unwrap() };
    let first = preview();
    assert_eq!(first["copies"][0]["form"], "short", "{first}");
    assert!(first["copies"][0]["parts"][0]["text"].as_str().unwrap().starts_with("Titled\nFits."));
    std::fs::write(s.root.join("notes/first.md"), format!("---\ntitle: Titled\n---\n{}", "Long sentence. ".repeat(40))).unwrap();
    let long = preview();
    assert_eq!(long["copies"][0]["form"], "long", "{long}");
    assert!(long["copies"][0]["article"]["html"].as_str().unwrap().contains("Originally published on"));
    manifest["contributes"]["channel"]["syndication"]["forms"] = serde_json::json!(["long"]);
    std::fs::write(&manifest_path, manifest.to_string()).unwrap();
    std::fs::write(s.root.join("notes/first.md"), "---\ntitle: Titled\n---\nFits.").unwrap();
    let config = read(&s.root, ".moss/config.toml");
    std::fs::write(s.root.join(".moss/config.toml"), format!("{config}\n[syndication]\nlink_back = false\n")).unwrap();
    let long = preview();
    assert_eq!(long["copies"][0]["form"], "long", "{long}");
    assert!(!long["copies"][0]["article"]["html"].as_str().unwrap().contains("Originally published on"));
    assert!(paths(&net).is_empty());
}

#[test]
fn a_posted_short_copy_that_outgrows_its_limit_is_never_reposted_as_long() {
    let net = Arc::new(Mutex::new(Network::default()));
    let s = site(serve(net.clone()), true, true);
    deploy(&s, &[]);
    let path = s.root.join(".moss/plugins/fixture/manifest.json");
    let mut manifest: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    manifest["contributes"]["channel"]["syndication"]["forms"] = serde_json::json!(["short", "long"]);
    std::fs::write(path, manifest.to_string()).unwrap();
    let page = read(&s.root, "notes/first.md");
    std::fs::write(s.root.join("notes/first.md"), format!("{page}\n{}", "More words. ".repeat(50))).unwrap();
    let result = deploy(&s, &[]);
    assert!(stderr(&result).contains("retaining the prior version"), "{}", stderr(&result));
    assert_eq!(paths(&net), ["/publish"]);
    deploy(&s, &[]);
    assert_eq!(paths(&net), ["/publish"]);
}

#[test]
fn normal_build_emits_the_formula_pictures_advertised_by_copies() {
    let net = Arc::new(Mutex::new(Network::default()));
    let s = site(serve(net.clone()), true, true);
    let path = s.root.join(".moss/plugins/fixture/manifest.json");
    let mut manifest: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    manifest["contributes"]["channel"]["syndication"]["images"] = serde_json::json!({"max": 4, "alt": true});
    std::fs::write(&path, manifest.to_string()).unwrap();
    std::fs::write(s.root.join("notes/first.md"), "A formula: $x^2$.\n").unwrap();
    let config = read(&s.root, ".moss/config.toml");
    std::fs::write(s.root.join(".moss/config.toml"), format!("{config}\n[site]\nurl = 'https://example.org'\n")).unwrap();
    let report: serde_json::Value = serde_json::from_slice(&deploy(&s, &["--dry-run", "--json"]).stdout).unwrap();
    let pictures = report["copies"][0]["pictures"].as_array().unwrap();
    assert_eq!(pictures.len(), 1, "{report}");
    assert_eq!(pictures[0]["alt"], "$x^2$");
    let url = pictures[0]["url"].as_str().unwrap();
    let relative = url.strip_prefix("https://example.org/").unwrap();
    let mp = moss_build::moss_paths::MossPaths::new(&s.root);
    assert!(mp.initial_serve_dir().join(relative).is_file(), "{url}");
    manifest["contributes"]["channel"]["syndication"]["forms"] = serde_json::json!(["long"]);
    std::fs::write(&path, manifest.to_string()).unwrap();
    let long: serde_json::Value = serde_json::from_slice(&deploy(&s, &["--dry-run", "--json"]).stdout).unwrap();
    let html = long["copies"][0]["article"]["html"].as_str().unwrap();
    assert!(html.contains(url), "{html}");
    assert!(!html.contains("<svg"));
    std::fs::write(s.root.join(".moss/config.toml"), format!("{config}\n[site]\nurl = 'https://example.org'\nmath = false\n")).unwrap();
    let disabled: serde_json::Value = serde_json::from_slice(&deploy(&s, &["--dry-run", "--json"]).stdout).unwrap();
    assert_eq!(disabled["copies"][0]["pictures"], serde_json::json!([]));
    assert!(!disabled["copies"][0]["article"]["html"].as_str().unwrap().contains("/_moss/math/"));
    assert!(paths(&net).is_empty());
}

#[test]
fn article_footers_use_the_rendered_home_name_and_page_language() {
    let net = Arc::new(Mutex::new(Network::default()));
    let s = site(serve(net.clone()), true, true);
    let path = s.root.join(".moss/plugins/fixture/manifest.json");
    let mut manifest: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    manifest["contributes"]["channel"]["syndication"]["forms"] = serde_json::json!(["long"]);
    std::fs::write(path, manifest.to_string()).unwrap();
    std::fs::rename(s.root.join("notes/first.md"), s.root.join("notes/first.zh-hans.md")).unwrap();
    let config = read(&s.root, ".moss/config.toml");
    std::fs::write(s.root.join(".moss/config.toml"), format!("{config}\n[site]\nlang = 'en'\ntitle = 'A & B'\n")).unwrap();
    for (home, localized, expected) in [("---\ntitle: A & B\n---\nHome.", None, "A &amp; B"), ("---\ntitle: A & B\n---\nHome.", Some("---\ntitle: 山居 & 文\n---\n首页。"), "山居 &amp; 文"), ("Home without an authored title.", None, "site")] {
        std::fs::write(s.root.join("index.md"), home).unwrap();
        let translated = s.root.join("zh-hans/index.md");
        if let Some(text) = localized {
            std::fs::create_dir_all(translated.parent().unwrap()).unwrap();
            std::fs::write(&translated, text).unwrap();
        } else if translated.exists() { std::fs::remove_file(translated).unwrap(); }
        let result = deploy(&s, &["--dry-run", "--json"]);
        assert!(stderr(&result).contains("title is not a setting"));
        let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        let html = report["copies"][0]["article"]["html"].as_str().unwrap();
        assert!(html.contains("本文首发于 <a href="), "{html}");
        assert!(html.contains(&format!("{expected}</a>。")), "{html}");
    }
}

#[test]
fn a_french_article_in_a_chinese_edition_keeps_its_rendered_site_name() {
    let net = Arc::new(Mutex::new(Network::default()));
    let s = site(serve(net), true, true);
    let manifest_path = s.root.join(".moss/plugins/fixture/manifest.json");
    let mut manifest: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["contributes"]["channel"]["syndication"]["forms"] = serde_json::json!(["long"]);
    std::fs::write(manifest_path, manifest.to_string()).unwrap();
    std::fs::create_dir_all(s.root.join("zh-hans")).unwrap();
    std::fs::write(s.root.join("zh-hans/index.md"), "---\ntitle: 山居\n---\n首页。\n").unwrap();
    std::fs::remove_file(s.root.join("notes/first.md")).unwrap();
    std::fs::write(s.root.join("notes/first.zh-hans.md"), "---\nlang: fr\ntitle: Bonjour\n---\nUn article.\n").unwrap();
    let report: serde_json::Value = serde_json::from_slice(&deploy(&s, &["--dry-run", "--json"]).stdout).unwrap();
    let html = report["copies"][0]["article"]["html"].as_str().unwrap();
    let mp = moss_build::moss_paths::MossPaths::new(&s.root);
    let map: moss_build::build::scan::article_map::ArticleMap = serde_json::from_slice(&std::fs::read(mp.article_map()).unwrap()).unwrap();
    let article = map.articles.values().find(|p| p.source_path == "notes/first.zh-hans.md").unwrap();
    assert_eq!(map.page_languages[&article.url_path], "fr");
    assert_eq!(map.page_site_names[&article.url_path], "山居");
    let shell = std::fs::read_to_string(mp.initial_serve_dir().join(&article.url_path).join("index.html")).unwrap();
    let shell_name = shell.split("class=\"site-name\"").nth(1).unwrap().split("</a>").next().unwrap().rsplit('>').next().unwrap();
    assert_eq!(shell_name, "山居", "the rendered shell uses the Chinese edition's name");
    assert!(html.contains("Originally published on <a href="), "content language still controls the footer: {html}");
    assert!(html.contains(&format!("{shell_name}</a>.")), "the copy must match the rendered shell: {html}");
}

#[test]
fn human_dry_run_counts_omitted_pictures_like_json_without_sending() {
    let net = Arc::new(Mutex::new(Network::default()));
    let s = site(serve(net.clone()), true, true);
    let path = s.root.join(".moss/plugins/fixture/manifest.json");
    let mut manifest: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    manifest["contributes"]["channel"]["syndication"]["images"] = serde_json::json!({"max": 1, "alt": true});
    std::fs::write(path, manifest.to_string()).unwrap();
    for name in ["one", "two", "three"] {
        std::fs::write(s.root.join(format!("notes/{name}.svg")), "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\"><rect width=\"1\" height=\"1\"/></svg>").unwrap();
    }
    std::fs::write(s.root.join("notes/first.md"), "Pictures.\n\n![One](one.svg)\n\n![Two](two.svg)\n\n![Three](three.svg)\n").unwrap();
    for form in ["short", "long"] {
        manifest["contributes"]["channel"]["syndication"]["forms"] = serde_json::json!([form]);
        std::fs::write(s.root.join(".moss/plugins/fixture/manifest.json"), manifest.to_string()).unwrap();
        let report: serde_json::Value = serde_json::from_slice(&deploy(&s, &["--dry-run", "--json"]).stdout).unwrap();
        let copy = &report["copies"][0];
        assert_eq!(copy["included_pictures"], 1, "{report}");
        assert_eq!(copy["omitted_pictures"], 2, "{report}");
        let attachments = copy["pictures"].as_array().unwrap().len();
        if form == "long" {
            assert_eq!(attachments, 0);
            assert_eq!(copy["article"]["html"].as_str().unwrap().matches("<img").count(), 1);
        } else { assert_eq!(attachments, 1); }
        let human = deploy(&s, &["--dry-run"]);
        let text = format!("{}{}", String::from_utf8_lossy(&human.stdout), stderr(&human));
        assert!(text.contains("pictures: 1 included, 2 omitted"), "{text}");
    }
    assert!(paths(&net).is_empty());
    assert!(!s.root.join(".moss/deploy/copies").exists());
}

#[test]
fn impossible_new_copies_are_unavailable_before_sending() {
    let net = Arc::new(Mutex::new(Network::default()));
    let s = site(serve(net.clone()), true, true);
    let path = s.root.join(".moss/plugins/fixture/manifest.json");
    let mut manifest: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    manifest["contributes"]["channel"]["syndication"]["forms"] = serde_json::json!(["short"]);
    manifest["contributes"]["channel"]["syndication"]["max_chars"] = serde_json::json!(10);
    std::fs::write(path, manifest.to_string()).unwrap();
    let report: serde_json::Value = serde_json::from_slice(&deploy(&s, &["--dry-run", "--json"]).stdout).unwrap();
    assert_eq!(report["copies"][0]["action"], "unavailable", "{report}");
    assert!(report["copies"][0]["reason"].as_str().unwrap().contains("fits none"));
    let choices = moss_build::syndication::choose::read_post(&s.root, "notes/first.md").unwrap();
    assert!(!choices[0].on);
    assert!(choices[0].reason.as_deref().unwrap().contains("fits none"));
    let preview = moss_build::syndication::preview::for_post(&s.root, "notes/first.md", "https://example.org");
    assert!(matches!(&preview[0].action, moss_build::syndication::plan::Action::Unavailable { reason } if reason.contains("fits none")));
    let human = deploy(&s, &["--dry-run"]);
    assert!(String::from_utf8_lossy(&human.stdout).contains("fits none"));
    deploy(&s, &[]);
    assert!(paths(&net).is_empty());
    let records = moss_build::syndication::record::Records::in_vault(&moss_build::moss_paths::MossPaths::new(&s.root));
    assert!(records.all().is_empty());
}

fn unavailable_edit_cancels_stale_work(posted: bool) {
    let net = Arc::new(Mutex::new(Network { refusing: !posted, ..Default::default() }));
    let s = site(serve(net.clone()), false, true);
    let path = s.root.join(".moss/plugins/fixture/manifest.json");
    let mut manifest: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    manifest["contributes"]["channel"]["syndication"]["forms"] = serde_json::json!(["short"]);
    std::fs::write(path, manifest.to_string()).unwrap();
    // Dry runs compare the configured target with the plugin's reported method.
    let plugin = s.root.join(".moss/plugins/localdir/main.bundle.js");
    let js = std::fs::read_to_string(&plugin).unwrap().replace("method: \"local\"", "method: \"localdir\"");
    std::fs::write(plugin, js).unwrap();
    deploy(&s, &[]);
    let records = moss_build::syndication::record::Records::in_vault(&moss_build::moss_paths::MossPaths::new(&s.root));
    let mut before = records.all().pop().unwrap();
    if posted {
        let map = moss_build::build::scan::article_map::ArticleMap::load(&s.root.join(".moss")).unwrap();
        let mut queued = map.articles[&before.url_path].clone();
        queued.content = "Queued stale update".into();
        before.post = Some(Box::new(moss_build::syndication::record::PublishedPost::new(&queued, &map)));
        before.update_due = true;
        records.save(&before).unwrap();
    }
    let stamped = read(&s.root, "notes/first.md");
    std::fs::write(s.root.join("notes/first.md"), format!("{stamped}\n{}", "Impossible edit. ".repeat(80))).unwrap();
    net.lock().unwrap().refusing = false;
    let preview: serde_json::Value = serde_json::from_slice(&deploy(&s, &["--dry-run", "--json"]).stdout).unwrap();
    assert_eq!(preview["copies"][0]["action"], "unavailable", "{preview}");
    let human = deploy(&s, &["--dry-run"]);
    assert!(String::from_utf8_lossy(&human.stdout).contains("unavailable:"));
    deploy(&s, &[]);
    deploy(&s, &[]);
    assert_eq!(paths(&net), ["/publish"], "the old snapshot must not send after the impossible edit");
    let blocked = records.all().pop().unwrap();
    assert_eq!(blocked.next_op(false), None);
    assert!(blocked.post.is_none());
    assert_eq!((&blocked.remote, &blocked.hash, &blocked.form, &blocked.parts), (&before.remote, &before.hash, &before.form, &before.parts));
    std::fs::write(s.root.join("notes/first.md"), format!("{stamped}\nFits again.\n")).unwrap();
    deploy(&s, &[]);
    assert_eq!(paths(&net), ["/publish", if posted { "/update" } else { "/publish" }]);
    let sent = net.lock().unwrap().requests.clone();
    assert_eq!(sent[0].1["idempotency_key"], sent[1].1["idempotency_key"]);
    assert!(sent[1].1["parts"][0]["text"].as_str().unwrap().contains("Fits again."));
    assert_eq!(records.all()[0].uid, before.uid);
}

#[test]
fn impossible_edit_cancels_a_refused_first_copy_until_it_fits_again() {
    unavailable_edit_cancels_stale_work(false);
}

#[test]
fn impossible_edit_cancels_a_queued_posted_update_until_it_fits_again() {
    unavailable_edit_cancels_stale_work(true);
}
