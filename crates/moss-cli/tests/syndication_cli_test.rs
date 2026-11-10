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
    return {{ success: true, copy: {{ id: c.id, url: c.url }} }};
  }},
  update_copy: async function (job) {{ var c = await call("/update", job); return {{ success: true, copy: {{ id: job.previous.id, url: job.previous.url }} }}; }},
  find_copy: async function (job) {{ var c = await call("/find", job); return {{ success: true, copy: c.found ? {{ id: c.id, url: c.url }} : null }}; }}
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
        .env("HOME", &s.home)
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
fn a_refused_copy_says_why_and_is_sent_again_next_run() {
    let net = Arc::new(Mutex::new(Network { refusing: true, ..Default::default() }));
    let s = site(serve(net.clone()), false, true);
    let refused = deploy(&s, &[]);
    assert!(stderr(&refused).contains("not posted to fixture (content): text too long"), "{}", stderr(&refused));
    net.lock().unwrap().refusing = false;
    let posted = deploy(&s, &[]);
    assert!(stderr(&posted).contains("posted to fixture"), "{}", stderr(&posted));
    assert_eq!(paths(&net), ["/publish", "/publish"], "a refusal is retried as a publish, with no lookup");
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
/// one paragraph a short post, parts a thread. The build gives it a label from
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
    assert_eq!(form("2026-11-06 1500.md"), "thread", "{report}");
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
