//! Real-process proof that `build --serve --watch` watches while the first
//! build's pictures still encode: a save made during the batch reaches the
//! preview without waiting for the whole batch.

use crate::served_child::*;
use std::time::Duration;

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

/// A save while the pictures encode reads their size from the scan's own
/// record and leaves colour and blur to the background, as the app's watcher
/// does: decoding every picture again in front of the save is the cost a
/// served watch exists to avoid.
#[test]
fn a_save_during_the_picture_batch_decodes_no_picture() {
    let moss_home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let site = std::fs::canonicalize(root.path()).unwrap().join("site");
    std::fs::create_dir_all(site.join("media")).unwrap();
    let mut body = String::from("# Home\n\n");
    for i in 0..4 {
        noise_png(&site.join(format!("media/n{i}.png")), 2000, 0x85EB_CA6B + i);
        body.push_str(&format!("![n{i}](media/n{i}.png)\n\n"));
    }
    std::fs::write(site.join("index.md"), &body).unwrap();

    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_moss-cli"));
    command.arg("build").arg(&site).args(["--serve", "--watch", "--no-plugins"]).env("MOSS_LOG_LEVEL", "debug");
    let watched = spawn_piped(command, moss_home.path());
    wait_for_line(&watched.stderr, "File watcher active", Duration::from_secs(300))
        .unwrap_or_else(|| panic!("the watcher never started; stderr:\n{}", joined(&watched.stderr)));
    let opened = watched.stderr_len();
    std::fs::write(site.join("index.md"), format!("{body}Saved.\n")).unwrap();
    wait_for_line(&watched.stderr, "trigger=content", Duration::from_secs(120))
        .unwrap_or_else(|| panic!("the save never rebuilt; stderr:\n{}", joined(&watched.stderr)));

    let lines = watched.stderr.lock().unwrap()[opened..].to_vec();
    let save = lines.iter().position(|line| line.contains("trigger=content")).unwrap();
    assert!(
        !lines[..save].iter().any(|line| line.contains("Image conversion complete")),
        "the batch ended before the save; nothing was tested"
    );
    let decoded: Vec<_> = lines[..save].iter().filter(|line| line.contains("Media meta cache miss for")).collect();
    assert!(decoded.is_empty(), "the save decoded pictures the background is encoding: {decoded:?}");
}
