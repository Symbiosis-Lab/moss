//! `chrome: none` draws one page without the site's frame: no header, nav
//! island, footer or skip link. It keeps its title, the controls and the
//! colophon, and every other page keeps the frame.

#[path = "support/site.rs"]
mod site;

use site::Site;

const PAGES: [(&str, &str); 6] = [
    ("index.md", "---\ntitle: Harbour\n---\n\nHome text.\n"),
    ("events/landing.md", "---\ntitle: Open day\nchrome: none\n---\n\nCome along.\n"),
    ("events/about.md", "---\ntitle: About\n---\n\nWho we are.\n"),
    ("events/index.md", "---\ntitle: Events\nchrome: none\n---\n\nWhat is on.\n"),
    ("events/plates.md", "---\ntitle: Plates\nchrome: none\ncontents: pictures\n---\n\n## One\n\nText.\n"),
    ("footer.md", "## Contact\n\nWrite to us.\n"),
];

fn has_frame(html: &str) -> [bool; 4] {
    [html.contains("<header"), html.contains("moss-nav-island"), html.contains("Write to us."), html.contains("moss-skip-link")]
}

#[test]
fn a_bare_page_drops_the_frame_and_keeps_its_own_parts() {
    let site = Site::with_config("floating_nav = true\n", &PAGES);
    let bare = site.html("events/landing/index.html");
    assert_eq!(has_frame(&bare), [false; 4], "{bare}");
    assert!(bare.contains(r#"data-chrome="none""#), "{bare}");
    assert!(bare.contains(r#"<main id="main-content""#), "{bare}");
    assert_eq!(bare.matches("<h1").count(), 1, "{bare}");
    assert!(bare.contains(">Open day</h1>"), "{bare}");
    assert!(bare.contains(r#"<div class="nav-icons""#), "the controls stay: {bare}");
    assert!(bare.contains(r#"class="moss-colophon""#), "{bare}");

    let sibling = site.html("events/about/index.html");
    assert_eq!(has_frame(&sibling), [true; 4], "{sibling}");
    assert!(!sibling.contains("data-chrome"), "{sibling}");
}

/// The key is honoured wherever a page is: a folder's home and the site's.
#[test]
fn a_folder_home_and_the_home_page_can_be_bare() {
    let site = Site::with_config("floating_nav = true\n", &PAGES);
    let folder = site.html("events/index.html");
    assert_eq!(has_frame(&folder), [false; 4], "{folder}");
    assert!(folder.contains(r#"data-chrome="none""#), "{folder}");

    let home = Site::new(&[("index.md", "---\ntitle: Harbour\nchrome: none\n---\n\nHome text.\n"), ("about.md", "---\ntitle: About\n---\n\nWho we are.\n")]).html("index.html");
    assert_eq!(has_frame(&home), [false; 4], "{home}");
    // With no header to name the site, the home page's title is shown.
    let h1 = home.split("<h1").nth(1).and_then(|rest| rest.split("</h1>").next()).expect("an h1");
    assert!(h1.contains("Harbour") && !h1.contains("visually-hidden"), "{h1}");
}

/// The contents lane's list lives in the island, so a bare page keeps the
/// whole width.
#[test]
fn a_bare_page_reserves_no_contents_lane() {
    let site = Site::with_config("floating_nav = true\n", &PAGES);
    let plates = site.html("events/plates/index.html");
    assert!(plates.contains(r#"data-chrome="none""#) && !plates.contains("data-contents"), "{plates}");
}

#[test]
fn the_site_colophon_switch_holds_on_a_bare_page() {
    let site = Site::with_config("colophon = false\n", &PAGES);
    assert!(!site.html("events/landing/index.html").contains("moss-colophon"));
}

#[test]
fn an_unknown_value_warns_and_keeps_the_frame() {
    let site = Site::new(&[("index.md", "---\ntitle: Harbour\nchrome: bare\n---\n\nHome text.\n")]);
    let html = site.html("index.html");
    assert!(html.contains("<header") && html.contains("moss-skip-link") && !html.contains("data-chrome"), "{html}");
    assert!(site.logged.iter().any(|p| p.contains("chrome") && p.contains("bare")), "{:?}", site.logged);
}
