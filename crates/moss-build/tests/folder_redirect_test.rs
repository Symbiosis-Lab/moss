//! A folder redirect: one `[redirects]` line whose two sides end in `/` keeps
//! every address under the old folder answering, from what the new folder
//! serves.

#[path = "support/site.rs"]
mod site;

use site::Site;

const CONFIG: &str = "domain = \"example.org\"\n\n[redirects]\n\"/review/old/\" = \"/review/client/old/\"\n";

const ROUND: &str = "<!doctype html><html><head><meta name=\"robots\" content=\"noindex\"><title>Round</title></head><body><img src=\"photo.png\"></body></html>";

/// What a build logged about redirects; each test reads its own build's log,
/// since the shared problem list belongs to whichever build ran last.
fn redirect_lines(logged: &[String]) -> Vec<&String> {
    logged.iter().filter(|l| l.contains("redirect '")).collect()
}

fn moved_folder() -> (Site, Vec<String>) {
    let site = Site::with_config(
        CONFIG,
        &[
            ("index.md", "---\ntitle: Home\n---\n\nText.\n"),
            ("review/client/old/index.html", ROUND),
            ("review/client/old/notes/detail.html", "<!doctype html><html><head><title>Detail</title></head><body>Detail</body></html>"),
            ("review/client/old/plan.pdf", "%PDF-1.4 plan"),
        ],
    );
    image::RgbImage::from_fn(96, 64, |x, y| image::Rgb([200, (x % 200) as u8, (y % 200) as u8]))
        .save(site.root.join("review/client/old/photo.png"))
        .unwrap();
    let logged = site.rebuild();
    (site, logged)
}

fn forwards(site: &Site, old: &str, to: &str) -> bool {
    let html = site.html(old);
    html.contains("http-equiv=\"refresh\"") && html.contains(&format!("url={to}\""))
}

#[test]
fn every_address_under_the_old_folder_answers_from_the_new_one() {
    let (site, logged) = moved_folder();

    assert!(forwards(&site, "review/old/index.html", "/review/client/old/"));
    assert!(forwards(&site, "review/old/notes/detail.html", "/review/client/old/notes/detail.html"));
    for file in ["plan.pdf", "photo.png"] {
        let served = std::fs::read(site.output(&format!("review/client/old/{file}"))).unwrap();
        let old = std::fs::read(site.output(&format!("review/old/{file}"))).unwrap();
        assert!(old == served, "review/old/{file} must be the served file's bytes");
    }
    assert!(redirect_lines(&logged).is_empty(), "{logged:?}");
}

/// A folder of files alone gets the listing moss writes for it, and both move.
#[test]
fn a_folder_of_files_alone_moves_with_its_listing() {
    let site = Site::with_config(
        CONFIG,
        &[("index.md", "---\ntitle: Home\n---\n\nText.\n"), ("review/client/old/plan.pdf", "%PDF-1.4 plan")],
    );
    assert_eq!(std::fs::read(site.output("review/old/plan.pdf")).unwrap(), b"%PDF-1.4 plan");
    assert!(forwards(&site, "review/old/index.html", "/review/client/old/"));
    assert!(redirect_lines(&site.logged).is_empty(), "{:?}", site.logged);
}

/// What the old folder still serves keeps its own content, its listing
/// included, without a word: the line still moves everything else.
#[test]
fn a_file_the_old_folder_still_serves_is_left_alone() {
    let site = Site::with_config(
        CONFIG,
        &[
            ("index.md", "---\ntitle: Home\n---\n\nText.\n"),
            ("review/client/old/plan.pdf", "%PDF-1.4 new"),
            ("review/client/old/notes.pdf", "%PDF-1.4 notes"),
            ("review/old/plan.pdf", "%PDF-1.4 kept"),
        ],
    );
    assert_eq!(std::fs::read(site.output("review/old/plan.pdf")).unwrap(), b"%PDF-1.4 kept");
    assert_eq!(std::fs::read(site.output("review/old/notes.pdf")).unwrap(), b"%PDF-1.4 notes");
    assert!(redirect_lines(&site.logged).is_empty(), "{:?}", site.logged);
}

#[test]
fn a_folder_redirect_to_a_folder_that_serves_nothing_is_reported() {
    let site = Site::with_config(CONFIG, &[("index.md", "---\ntitle: Home\n---\n\nText.\n")]);
    assert!(site.logged.iter().any(|l| l.contains("no file is served under it")), "{:?}", site.logged);
    assert!(!site.output("review/old/index.html").exists());
}

/// A folder line whose old address is a real page and that keeps nothing else
/// is reported, as any line the site's own pages override.
#[test]
fn a_folder_line_that_keeps_nothing_is_reported() {
    let site = Site::with_config(
        "domain = \"example.org\"\n\n[redirects]\n\"/about/\" = \"/contact/\"\n",
        &[("index.md", "---\ntitle: Home\n---\n\nText.\n"), ("about.md", "# About\n"), ("contact.md", "# Contact\n")],
    );
    assert!(!site.html("about/index.html").contains("http-equiv=\"refresh\""));
    assert!(site.logged.iter().any(|l| l.contains("already serves that address and every one under it")), "{:?}", site.logged);
}

/// An ordinary page line has `/` on both sides too. Its folder half keeps
/// nothing when the new page has nothing under it, and that is not a problem.
#[test]
fn a_page_line_with_nothing_under_the_new_page_reports_nothing() {
    let site = Site::with_config(
        "domain = \"example.org\"\n\n[redirects]\n\"/old-post/\" = \"/about/\"\n",
        &[("index.md", "---\ntitle: Home\n---\n\nText.\n"), ("about.md", "# About\n")],
    );
    assert!(forwards(&site, "old-post/index.html", "/about/"));
    assert!(redirect_lines(&site.logged).is_empty(), "{:?}", site.logged);
}
