//! Real-process proof that a `title:` edit under `build --serve --watch` costs
//! only the rebuild the edit asked for, and leaves the same output a clean
//! build of the edited folder would.
//!
//! A title edit changes the page's share card, and the card is
//! content-addressed, so it lands under a path the previous manifest never
//! held. The media-settle trigger used to read that as a brand-new image and
//! enqueue a full, uncached re-render of the whole site after every title
//! edit. The card is an output of the page render that just ran, so nothing
//! else can be waiting on it.

#[path = "served_child.rs"]
mod served_child;

use served_child::*;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, SystemTime};

fn write_page(site: &Path, rel: &str, title: &str) {
    let path = site.join(rel);
    std::fs::write(&path, format!("---\ntitle: {title}\n---\n\nSome body text.\n")).unwrap();
}

fn count(child: &ServedChild, needle: &str, from: usize) -> usize {
    child.stderr.lock().unwrap_or_else(std::sync::PoisonError::into_inner)[from..]
        .iter()
        .filter(|l| l.contains(needle))
        .count()
}

/// Wait until stderr has gone `quiet` without a new line — long enough for a
/// follow-up rebuild enqueued at seal time to have started and finished.
fn settle(child: &ServedChild, quiet: Duration) {
    let mut last = child.stderr_len();
    let mut since = std::time::Instant::now();
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
        let now = child.stderr_len();
        if now != last {
            last = now;
            since = std::time::Instant::now();
        } else if since.elapsed() >= quiet {
            return;
        }
    }
    panic!("child never went quiet; stderr:\n{}", joined(&child.stderr));
}

fn sealed_files(site: &Path) -> BTreeMap<String, String> {
    let path = site.join(".moss/build.nosync/hashes.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    json["files"]
        .as_object()
        .expect("hashes.json has a files map")
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
        .collect()
}

fn og_image(site: &Path, page: &str) -> String {
    let html = std::fs::read_to_string(site.join(".moss/build.nosync/staging").join(page)).unwrap();
    let marker = "og:image\" content=\"";
    let start = html.find(marker).unwrap_or_else(|| panic!("{page} has no og:image:\n{html}")) + marker.len();
    let end = start + html[start..].find('"').unwrap();
    html[start..end].trim_start_matches('/').to_string()
}

/// Wait for one complete build: a sealed generation, then quiet.
fn spawn_and_settle(site: &Path, moss_home: &Path) -> ServedChild {
    let child = spawn_served_build(site, moss_home, &["--watch", "--no-plugins"]);
    child.wait_for_access_line(Duration::from_secs(60));
    wait_for_line(&child.stderr, "seal+persist", Duration::from_secs(60))
        .unwrap_or_else(|| panic!("the first build never sealed; stderr:\n{}", joined(&child.stderr)));
    settle(&child, Duration::from_secs(2));
    child
}

#[test]
fn a_title_edit_rebuilds_once_and_matches_a_clean_build() {
    let moss_home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let site = std::fs::canonicalize(root.path()).unwrap().join("site");
    std::fs::create_dir_all(&site).unwrap();
    write_page(&site, "index.md", "Home");
    write_page(&site, "post.md", "First Title");

    let watched = spawn_and_settle(&site, moss_home.path());
    let card_before = og_image(&site, "post/index.html");

    let from = watched.stderr_len();
    write_page(&site, "post.md", "Second Title");
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while count(&watched, "seal+persist", from) == 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "the title edit never rebuilt; stderr:\n{}",
            joined(&watched.stderr)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    settle(&watched, Duration::from_secs(3));

    // Every rebuild after the edit must answer a file event. Not a bare `== 1`:
    // FSEvents keeps reporting a file created seconds ago as `Create`, and this
    // fixture's files are that young, so one save can arrive as two
    // unconditional events. The settle trigger's rebuild answers no event.
    let rebuilds = count(&watched, "build.summary", from);
    let asked = count(&watched, "Rebuild triggered by", from) - count(&watched, "Gated at admission", from);
    assert!(
        rebuilds >= 1 && rebuilds <= asked && count(&watched, "media settle", from) == 0,
        "a title edit must not enqueue a rebuild of its own ({rebuilds} rebuilds for {asked} file events); stderr:\n{}",
        joined(&watched.stderr)
    );

    let card_after = og_image(&site, "post/index.html");
    assert_ne!(card_after, card_before, "the edited page must point at a new share card");
    let watched_files = sealed_files(&site);
    assert!(
        watched_files.contains_key(&card_after),
        "og:image {card_after} must be a sealed output: {watched_files:?}"
    );
    assert!(site.join(".moss/build.nosync/staging").join(&card_after).is_file());
    drop(watched);

    // A clean build of the edited vault, same mode, same source mtimes.
    let clean = std::fs::canonicalize(root.path()).unwrap().join("clean");
    std::fs::create_dir_all(&clean).unwrap();
    for name in ["index.md", "post.md"] {
        std::fs::copy(site.join(name), clean.join(name)).unwrap();
        let mtime: SystemTime = std::fs::metadata(site.join(name)).unwrap().modified().unwrap();
        std::fs::File::options().write(true).open(clean.join(name)).unwrap().set_modified(mtime).unwrap();
    }
    let fresh = spawn_and_settle(&clean, moss_home.path());
    let clean_files = sealed_files(&clean);
    drop(fresh);

    assert_eq!(watched_files, clean_files, "the incremental output must equal a clean build's");
}
