//! `moss plugin install` against a local registry. One test: the app-data
//! directory comes from the environment and the verdicts are process-wide, so
//! scenarios that share them run in order rather than racing.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::{Arc, Mutex};

use moss_build::plugins::install::install_cli::install;
use sha2::{Digest, Sha256};

type Routes = Arc<Mutex<HashMap<&'static str, Vec<u8>>>>;

fn serve(routes: Routes) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut line = String::new();
            if BufReader::new(&stream).read_line(&mut line).is_err() {
                continue;
            }
            let body = routes.lock().unwrap().iter().find(|(path, _)| line.contains(*path)).map(|(_, b)| b.clone());
            let head = match &body {
                Some(b) => format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", b.len()),
                None => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
            };
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body.unwrap_or_default());
        }
    });
    format!("http://127.0.0.1:{port}")
}

fn zip(id: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file("manifest.json", opts).unwrap();
        write!(w, r#"{{"name":"{id}","version":"1.2.0","entry":"main.js"}}"#).unwrap();
        w.start_file("main.js", opts).unwrap();
        w.write_all(b"module.exports = {};").unwrap();
        w.finish().unwrap();
    }
    buf
}

fn entry(origin: &str, id: &str) -> String {
    let sha = hex::encode(Sha256::digest(zip(id)));
    format!(
        r#"{{"type":"plugin","id":"{id}","display_name":"{id}","version":"1.2.0","download_url":"{origin}/{id}.zip","sha256":"{sha}"}}"#
    )
}

fn index(origin: &str, serial: u64, ids: &[&str]) -> Vec<u8> {
    let entries: Vec<String> = ids.iter().map(|id| entry(origin, id)).collect();
    format!(r#"{{"schema_version":1,"serial":{serial},"entries":[{}]}}"#, entries.join(",")).into_bytes()
}

fn site(root: &Path) -> std::path::PathBuf {
    let site = root.join("site");
    std::fs::create_dir_all(site.join(".moss")).unwrap();
    site
}

#[test]
fn installing_checks_the_catalog_every_time_and_approves_in_app_data() {
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("HOME", tmp.path());
    std::env::set_var("XDG_DATA_HOME", tmp.path().join("data"));
    moss_build::system::seed_app_version("0.0.0");
    let app_data = moss_build::infra::app_data::app_data_dir_early().unwrap();
    assert!(app_data.starts_with(tmp.path()), "{}", app_data.display());
    let site = site(tmp.path());

    let routes: Routes = Arc::default();
    let origin = serve(routes.clone());
    {
        let mut r = routes.lock().unwrap();
        r.insert("/index.json", index(&origin, 1, &["alpha"]));
        r.insert("/revoked.json", br#"{"schema_version":1,"serial":1,"revocations":[]}"#.to_vec());
        r.insert("/alpha.zip", zip("alpha"));
        r.insert("/beta.zip", zip("beta"));
        r.insert("/gamma.zip", zip("gamma"));
    }

    // Online: installed, one line said, consent kept in app data and not in the site.
    let done = install("alpha", &site, &origin).unwrap();
    assert_eq!(done.line, "Installed alpha 1.2.0 and approved it for this user on this machine");
    assert!(done.notice.is_none());
    assert!(site.join(".moss/plugins/alpha/main.js").exists());
    let approvals = std::fs::read_to_string(app_data.join("plugin-approvals.json")).unwrap();
    assert!(approvals.contains("alpha"), "{approvals}");
    let in_site: Vec<_> = walk(&site).into_iter().filter(|p| p.contains("approval")).collect();
    assert!(in_site.is_empty(), "no approval is written into the site: {in_site:?}");

    // A plugin already in the folder, with no approval on this machine, is allowed by the command too.
    std::fs::remove_file(app_data.join("plugin-approvals.json")).unwrap();
    install("alpha", &site, &origin).unwrap();
    assert!(std::fs::read_to_string(app_data.join("plugin-approvals.json")).unwrap().contains("alpha"));

    // The catalog moved since the last look: a plugin listed only now installs,
    // so the index was fetched again rather than read from the cache.
    routes.lock().unwrap().insert("/index.json", index(&origin, 2, &["alpha", "gamma"]));
    let done = install("gamma", &site, &origin).unwrap();
    assert!(done.notice.is_none() && site.join(".moss/plugins/gamma/main.js").exists());

    // Unreachable: the cached catalog serves, and the notice says so.
    let dead = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://127.0.0.1:{}", l.local_addr().unwrap().port())
    };
    std::fs::remove_dir_all(site.join(".moss/plugins/gamma")).unwrap();
    let done = install("gamma", &site, &dead).unwrap();
    let notice = done.notice.expect("a stale catalog is named");
    assert!(notice.contains("cached") && notice.contains("within the last hour"), "{notice}");

    // Unreachable with nothing cached: fails, naming the network error.
    std::fs::remove_dir_all(app_data.join("registry")).unwrap();
    let err = install("beta", &site, &dead).err().expect("no catalog, no install");
    assert!(err.contains("none is cached") && err.contains("127.0.0.1"), "{err}");
    assert!(!site.join(".moss/plugins/beta").exists());
}

fn walk(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        out.push(p.display().to_string());
        if p.is_dir() {
            out.extend(walk(&p));
        }
    }
    out
}
