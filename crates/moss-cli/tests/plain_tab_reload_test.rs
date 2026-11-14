//! Real-process proof that `build --serve --watch` tells a page with no token
//! that it was rebuilt: an edit puts one frame on `/__moss/reload`.

#[path = "served_child.rs"]
mod served_child;

use served_child::*;
use std::time::Duration;

#[test]
fn an_edit_under_watch_reaches_the_reload_stream() {
    let moss_home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let site = std::fs::canonicalize(root.path()).unwrap().join("site");
    std::fs::create_dir_all(&site).unwrap();
    std::fs::write(site.join("index.md"), "# Home\n\nFirst text.\n").unwrap();

    let watched = spawn_served_build(&site, moss_home.path(), &["--watch", "--no-plugins"]);
    let port = port_from_url_line(&watched.wait_for_access_line(Duration::from_secs(60)));
    wait_for_line(&watched.stderr, "seal+persist", Duration::from_secs(60))
        .unwrap_or_else(|| panic!("the first build never sealed; stderr:\n{}", joined(&watched.stderr)));

    let stream = ureq::get(&format!("http://localhost:{port}/__moss/reload"))
        .timeout(Duration::from_secs(60))
        .call()
        .expect("the reload stream opens with no token")
        .into_reader();
    std::fs::write(site.join("index.md"), "# Home\n\nSecond text.\n").unwrap();

    // Keep-alive comments (`:`) may come first; the first data line is the
    // frame. Four keep-alives, a minute, with no frame is a failure, not a wait.
    let frame = std::io::BufRead::lines(std::io::BufReader::new(stream))
        .take(8)
        .find_map(|line| line.ok().filter(|l| l.starts_with("data:")))
        .unwrap_or_else(|| panic!("no frame after the edit; stderr:\n{}", joined(&watched.stderr)));
    assert_eq!(frame, "data: reload");
}
