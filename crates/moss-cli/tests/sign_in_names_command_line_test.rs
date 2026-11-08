//! A sign-in link asked for from the command line names this binary and the
//! kind of computer on the page the link opens, so the person reading the mail
//! can tell it from the app. The hosting service is a local stand-in that refuses
//! the unverified key, records the sign-in request and expires the link; the
//! address is invented.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::{Arc, Mutex};

const MOSS: &str = env!("CARGO_BIN_EXE_moss-cli");

/// Serves until the test ends; returns its address and the sign-in bodies it saw.
fn stand_in_service() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let log = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            let _ = reader.read_line(&mut first);
            let path = first.split_whitespace().nth(1).unwrap_or("").to_string();
            let mut length = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; length];
            let _ = reader.read_exact(&mut body);
            let (status, json) = if path == "/auth/request" {
                log.lock().unwrap().push(String::from_utf8_lossy(&body).into_owned());
                ("200 OK", "{}")
            } else if path.starts_with("/auth/status/") {
                ("200 OK", r#"{"status":"expired","email":null}"#)
            } else if path.starts_with("/api/sites") {
                ("403 Forbidden", r#"{"error":"pubkey not verified for this email"}"#)
            } else {
                ("404 Not Found", "{}")
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}",
                json.len()
            );
        }
    });
    (url, seen)
}

#[test]
fn a_sign_in_from_the_command_line_names_it_and_the_computer() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, site) = (tmp.path().join("home"), tmp.path().join("site"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&site).unwrap();
    std::fs::write(site.join("index.md"), "# Hello\n").unwrap();
    let (url, seen) = stand_in_service();

    let moss = |args: &[&str]| {
        let mut cmd = Command::new(MOSS);
        cmd.args(args)
            .env("MOSS_HOME", &home)
            .env("HOME", &home)
            .env("MOSS_SETA_URL", &url)
            .env_remove("MOSS_ENV");
        for proxy in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"] {
            cmd.env_remove(proxy);
        }
        cmd.output().expect("spawn moss-cli")
    };
    let folder = site.to_str().unwrap();
    assert!(moss(&["env", "staging", folder]).status.success());
    let out = moss(&["deploy", folder, "--email", "reader@example.com"]);

    let bodies = seen.lock().unwrap().clone();
    assert_eq!(bodies.len(), 1, "one sign-in request; stderr: {}", String::from_utf8_lossy(&out.stderr));
    let body: serde_json::Value = serde_json::from_str(&bodies[0]).unwrap();
    let name = body["client"]["name"].as_str().unwrap();
    // The kind of computer, never its name.
    let expected = match moss_build::system::device_kind() {
        Some(kind) => format!("moss command line on {kind}"),
        None => "moss command line".to_string(),
    };
    assert_eq!(name, expected);
    assert_eq!(body["client"]["version"], env!("CARGO_PKG_VERSION"));
}
