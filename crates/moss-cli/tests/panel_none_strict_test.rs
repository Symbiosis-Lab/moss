//! `panel=none` is the author's call: plain ink over a picture moss cannot
//! show the ink reads on is a note naming where, never a problem, so `--strict`
//! passes. Run through the real CLI.

use std::io::Write;
use std::process::{Command, Stdio};

fn chunk(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut crc = flate2::Crc::new();
    crc.update(kind);
    crc.update(body);
    let mut out = (body.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out.extend_from_slice(&crc.sum().to_be_bytes());
    out
}

/// A 400x200 pale picture with a dark line on every other row: hatching.
fn hatched_png() -> Vec<u8> {
    let (w, h) = (400u32, 200u32);
    let mut raw = Vec::new();
    for y in 0..h {
        raw.push(0);
        for _ in 0..w {
            raw.extend_from_slice(if y % 2 == 0 { &[10, 14, 24] } else { &[205, 218, 232] });
        }
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    z.write_all(&raw).unwrap();
    let mut ihdr = w.to_be_bytes().to_vec();
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend(chunk(b"IHDR", &ihdr));
    png.extend(chunk(b"IDAT", &z.finish().unwrap()));
    png.extend(chunk(b"IEND", &[]));
    png
}

fn build(hero: &str) -> (std::process::Output, String) {
    let moss_home = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let site = std::fs::canonicalize(tmp.path()).unwrap();
    std::fs::write(site.join("index.md"), format!("---\ntitle: Home\n---\n\n{hero}\n")).unwrap();
    std::fs::write(site.join("hatch.png"), hatched_png()).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .args(["build", site.to_str().unwrap(), "--no-plugins", "--strict"])
        .env("MOSS_HOME", moss_home.path())
        .env("MOSS_SITE_URL", "https://example.com")
        .stdin(Stdio::null())
        .output()
        .expect("run moss-cli");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (out, stderr)
}

#[test]
fn plain_ink_over_a_hatched_sky_is_a_note_and_passes_strict() {
    let (out, stderr) = build(":::hero {image=hatch.png align=top panel=none}\n## Open all hours today\n:::");
    assert_eq!(out.status.code(), Some(0), "plain ink is the author's call and must not fail --strict:\n{stderr}");
    assert!(stderr.contains("[NOTE]") && stderr.contains("panel=none") && stderr.contains("Open all hours today"), "{stderr}");
    assert!(stderr.contains("the words in the light theme") && stderr.contains(":1 at the worst spot,") && stderr.contains(":1 over the typical ground, needs"), "the note names where, with the ratio: {stderr}");
    assert!(!stderr.contains("[WARN]"), "{stderr}");
}

#[test]
fn without_panel_the_same_picture_says_nothing() {
    let (out, stderr) = build(":::hero {image=hatch.png align=top panel=auto}\n## Open all hours today\n:::");
    assert_eq!(out.status.code(), Some(0), "{stderr}");
    assert!(!stderr.contains("panel=none"), "{stderr}");
}
