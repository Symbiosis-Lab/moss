//! `moss build` refreshes the comment cache before its pages read it, so a publish
//! from the command line ships the comments the server has now, and says so
//! when it could not. The comment server is a local stand-in; all names and
//! text are invented.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

const MOSS: &str = env!("CARGO_BIN_EXE_moss-cli");

fn build(home: &Path, site: &Path, env: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(MOSS);
    cmd.args(["build", site.to_str().unwrap(), "--no-plugins"])
        .env("MOSS_HOME", home)
        .env_remove("MOSS_ENV")
        .env_remove("MOSS_SETA_URL");
    for proxy in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"] {
        cmd.env_remove(proxy);
    }
    cmd.envs(env.iter().copied());
    let o = cmd.output().expect("spawn moss-cli");
    assert!(o.status.success(), "build failed: {}", String::from_utf8_lossy(&o.stderr));
    o
}

fn page(site: &Path) -> String {
    std::fs::read_to_string(site.join(".moss/build.nosync/current/posts/hello/index.html")).expect("built page")
}

/// A site whose comment server is `server`, built once so the article map
/// names its page, with a cached comment on that page that the server no
/// longer matches. The page has been published, so its file carries its uid.
/// Returns the site.
fn site_with_stale_cache(tmp: &Path, home: &Path, server: &str) -> std::path::PathBuf {
    let site = tmp.join("site");
    std::fs::create_dir_all(site.join(".moss/data/social")).unwrap();
    std::fs::create_dir_all(site.join("posts")).unwrap();
    std::fs::write(
        site.join(".moss/config.toml"),
        format!("[services.comments]\nprovider = \"artalk\"\nserver_url = \"{server}\"\n"),
    )
    .unwrap();
    std::fs::write(site.join("index.md"), "---\ntitle: Home\n---\n\nWelcome.\n").unwrap();
    std::fs::write(site.join("posts/hello.md"), "---\ntitle: Hello Garden\nuid: \"4e110a11\"\n---\n\nA post about gardens.\n").unwrap();
    build(home, &site, &[]);
    let map: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(site.join(".moss/build.nosync/article-map.json")).unwrap()).unwrap();
    let uid = map["articles"]["posts/hello/"]["uid"].as_str().unwrap().to_string();
    std::fs::write(tmp.join("uid.txt"), &uid).unwrap();
    let cached = serde_json::json!({"schemaVersion": "1.1.0", "articles": {uid: {"comments": [
        {"id": "1", "content": "<p>An old cached note</p>", "createdAt": "2026-03-01T10:00:00Z", "author": {"displayName": "Ana Example"}}
    ]}}});
    std::fs::write(site.join(".moss/data/social/comment.json"), cached.to_string()).unwrap();
    // 2026-10-14 12:00 UTC: the cache's last refresh.
    std::fs::write(
        site.join(".moss/data/comment-sync-state.json"),
        r#"{"last_success_at": 1791979200.0, "last_attempt_at": 1791979200.0, "last_error": null}"#,
    )
    .unwrap();
    site
}

/// A comment server that answers every request after `delay` with one new
/// comment on the page whose uid it reads from `uid_file` at that moment.
fn serve_new_comment(delay: Duration, uid_file: std::path::PathBuf) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            std::thread::sleep(delay);
            let uid = std::fs::read_to_string(&uid_file).unwrap_or_default();
            let body = format!(
                r#"{{"data":[{{"id":42,"page_key":"{uid}","content":"A comment left yesterday","date":"2026-10-31 09:00:00","nick":"Ben Example"}}]}}"#
            );
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    url
}

/// The server answers more slowly than the build would take without waiting,
/// so only a build that waits for the refresh can carry the new comment.
#[test]
fn a_build_ships_the_comment_the_server_has_now() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let server = serve_new_comment(Duration::from_secs(3), tmp.path().join("uid.txt"));
    let site = site_with_stale_cache(tmp.path(), &home, &server);

    let o = build(&home, &site, &[]);
    let html = page(&site);
    assert!(
        html.contains("A comment left yesterday"),
        "the build must ship the refreshed comment; stderr: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(!String::from_utf8_lossy(&o.stderr).contains("comments may be stale"));
}

/// A folder copied without its build cache has no article map, which is what
/// the refresh asks the server about; the first build must still ship the
/// server's comment.
#[test]
fn the_first_build_of_a_fresh_folder_ships_the_comment_the_server_has_now() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let server = serve_new_comment(Duration::from_millis(200), tmp.path().join("uid.txt"));
    let site = site_with_stale_cache(tmp.path(), &home, &server);
    std::fs::remove_dir_all(site.join(".moss/build.nosync")).unwrap();

    let o = build(&home, &site, &[]);
    assert!(
        page(&site).contains("A comment left yesterday"),
        "the first build must ship the refreshed comment; stderr: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(!String::from_utf8_lossy(&o.stderr).contains("comments may be stale"));
}

/// A server that accepts and never answers holds the build for the limit only,
/// and the build says its comments may be stale and how old they are.
#[test]
fn a_server_that_never_answers_costs_the_limit_and_says_how_old_the_comments_are() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let server = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        let held: Vec<_> = listener.incoming().flatten().collect();
        drop(held);
    });
    let site = site_with_stale_cache(tmp.path(), &home, &server);

    let started = Instant::now();
    let o = build(&home, &site, &[("MOSS_COMMENT_WAIT_SECS", "1")]);
    let elapsed = started.elapsed();
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert!(
        stderr.contains("comments may be stale: the comment server did not answer within 1s"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("this build shows comments as of 2026-10-14"), "stderr: {stderr}");
    assert!(page(&site).contains("An old cached note"), "the cached comments still ship");
    // Below the request's own 15 s timeout: the limit, not the request, ended the wait.
    assert!(elapsed < Duration::from_secs(12), "build took {elapsed:?}");
}
