use super::*;
use crate::system::derive::test_support::{entry, script};
use tempfile::TempDir;

#[test]
fn a_missing_binary_says_it_is_not_installed() {
    let err = resolve_binary(&entry(std::path::Path::new("/nonexistent/unpack"), "1.0.0")).unwrap_err();
    assert!(err.message.contains("not installed"), "{err}");
}

#[test]
fn a_path_source_needs_no_download() {
    let dir = TempDir::new().unwrap();
    let bin = script(dir.path(), "d.sh", "true");
    assert_eq!(ensure_binary(&entry(&bin, "1.0.0"), None).unwrap(), bin);
}

fn download_entry(url: &str, sha256: &str) -> DerivationEntry {
    let mut e = entry(std::path::Path::new("/unused"), "1.0.0");
    e.contribution.run.artifact = serde_json::from_value(serde_json::json!([
        {"kind": "download", "url": url, "sha256": sha256, "archive_format": "tar.gz"}
    ]))
    .unwrap();
    e
}

#[test]
fn the_same_executable_keys_alike_downloaded_or_local() {
    let dir = TempDir::new().unwrap();
    let bin = script(dir.path(), "d.sh", "true");
    let local = entry(&bin, "1.0.0");
    let download = download_entry("https://example.test/a.tar.gz", &"a".repeat(64));
    assert_eq!(fingerprint(&local, Some(&bin)), fingerprint(&download, Some(&bin)));
}

#[test]
fn the_key_follows_bytes_and_fragments_not_locations() {
    let dir = TempDir::new().unwrap();
    let bin = script(dir.path(), "d.sh", "true");
    let other = script(dir.path(), "e.sh", "false");
    let e = entry(&bin, "1.0.0");
    let base = fingerprint(&e, Some(&bin));
    assert_ne!(fingerprint(&e, Some(&other)), base, "different bytes");
    let mut fragments = e.clone();
    fragments.contribution.fragments.clear();
    assert_ne!(fingerprint(&fragments, Some(&bin)), base, "different fragments");
    let elsewhere = TempDir::new().unwrap();
    let moved = script(elsewhere.path(), "d.sh", "true");
    assert_eq!(fingerprint(&entry(&moved, "1.0.0"), Some(&moved)), base, "same bytes elsewhere");

    let a = download_entry("https://one.test/a.tar.gz", &"a".repeat(64));
    let url = download_entry("https://two.test/b.tar.gz", &"a".repeat(64));
    let sha = download_entry("https://one.test/a.tar.gz", &"b".repeat(64));
    assert_eq!(fingerprint(&a, None), fingerprint(&url, None), "a changed URL alone");
    assert_ne!(fingerprint(&a, None), fingerprint(&sha, None), "a re-pinned archive");
}

#[test]
fn a_program_published_for_other_computers_says_so_in_plain_words_at_first_use() {
    let mut e = download_entry("https://example.test/a.tar.gz", &"a".repeat(64));
    e.contribution.run.artifact = serde_json::from_value(serde_json::json!([
        {"kind": "download", "platform": "plan9-riscv", "url": "https://example.test/a.tar.gz",
         "sha256": "a".repeat(64), "archive_format": "tar.gz"}
    ]))
    .unwrap();
    let err = ensure_binary(&e, None).unwrap_err();
    assert!(err.message.starts_with("Not available on this computer:") && err.message.contains("plan9-riscv"), "{err}");
}

/// Serve `body` for every request on a local port, pausing before each reply
/// so that simultaneous installers are all mid-download together.
fn serve_slowly(body: Vec<u8>) -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/pdf-pages.tar.gz", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let body = body.clone();
            std::thread::spawn(move || {
                let mut stream = stream;
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                std::thread::sleep(std::time::Duration::from_millis(300));
                let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            });
        }
    });
    url
}

fn tar_gz_with_one_program(name: &str) -> Vec<u8> {
    let mut tar = tar::Builder::new(Vec::new());
    let program = b"#!/bin/sh\ntrue\n";
    let mut header = tar::Header::new_gnu();
    header.set_size(program.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    tar.append_data(&mut header, name, &program[..]).unwrap();
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut gz, &tar.into_inner().unwrap()).unwrap();
    gz.finish().unwrap()
}

#[test]
fn readers_starting_together_on_a_fresh_machine_all_get_the_program() {
    let stacks = TempDir::new().unwrap();
    let archive = tar_gz_with_one_program("unpack");
    let sha256 = {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(&archive))
    };
    let url = serve_slowly(archive);
    let mut e = download_entry(&url, &sha256);
    e.contribution.run.artifact = serde_json::from_value(serde_json::json!([
        {"kind": "download", "url": url, "sha256": sha256, "archive_format": "tar.gz", "executable": "unpack"}
    ]))
    .unwrap();

    let results: Vec<_> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..6).map(|_| s.spawn(|| install_into(&e, stacks.path(), None))).collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for result in results {
        let path = result.expect("every concurrent reader gets the installed program");
        assert!(path.is_file(), "{}", path.display());
    }
}
