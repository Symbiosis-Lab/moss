//! Real-process proof that `build --serve --watch` watches while the first
//! build's pictures still encode: a save made during the batch reaches the
//! preview without waiting for the whole batch.

use crate::served_child::*;
use std::io::Write;
use std::time::Duration;

/// A PNG of `side`×`side` noise, stored uncompressed: slow to decode and encode,
/// quick to write.
fn noise_png(path: &std::path::Path, side: u32, seed: u32) {
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend(kind);
        out.extend(data);
        let crc = out[start..].iter().fold(!0u32, |mut c, &b| {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 == 1 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            c
        });
        out.extend((!crc).to_be_bytes());
    }
    let mut state = seed | 1;
    let mut raw = Vec::with_capacity((side * (side * 3 + 1)) as usize);
    for _ in 0..side {
        raw.push(0);
        for _ in 0..side * 3 {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            raw.push(state as u8);
        }
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::none());
    z.write_all(&raw).unwrap();
    let mut header = Vec::new();
    header.extend(side.to_be_bytes());
    header.extend(side.to_be_bytes());
    header.extend([8, 2, 0, 0, 0]);
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut png, b"IHDR", &header);
    chunk(&mut png, b"IDAT", &z.finish().unwrap());
    chunk(&mut png, b"IEND", &[]);
    std::fs::write(path, png).unwrap();
}

#[test]
fn the_watcher_starts_while_the_first_builds_pictures_still_encode() {
    let moss_home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let site = std::fs::canonicalize(root.path()).unwrap().join("site");
    std::fs::create_dir_all(site.join("media")).unwrap();
    let mut body = String::from("# Home\n\n");
    for i in 0..4 {
        noise_png(&site.join(format!("media/n{i}.png")), 2000, 0x9E37_79B9 + i);
        body.push_str(&format!("![n{i}](media/n{i}.png)\n\n"));
    }
    std::fs::write(site.join("index.md"), body).unwrap();

    let watched = spawn_served_build(&site, moss_home.path(), &["--watch", "--no-plugins"]);
    wait_for_line(&watched.stderr, "File watcher active", Duration::from_secs(300))
        .unwrap_or_else(|| panic!("the watcher never started; stderr:\n{}", joined(&watched.stderr)));
    assert!(
        !joined(&watched.stderr).contains("Image conversion complete"),
        "the watcher started only after the picture batch; stderr:\n{}",
        joined(&watched.stderr)
    );
}
