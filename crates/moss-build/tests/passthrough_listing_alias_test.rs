//! A passthrough folder gets no generated listing, but the address its listing
//! had keeps forwarding to the nearest page still served.

#[path = "support/site.rs"]
mod site;

use site::Site;

#[test]
fn a_passthrough_folders_old_listing_addresses_forward_up() {
    let site = Site::with_config(
        "domain = \"example.org\"\n\n[build]\npassthrough = [\"scene\"]\n",
        &[
            ("index.md", "---\ntitle: Home\n---\n\nText.\n"),
            ("scene/app.js", "console.log(1)"),
            ("scene/credits.md", "---\ntitle: Credits\n---\n\nCredit text.\n"),
            ("scene/sketch/README.md", "---\ntitle: Sketch\n---\n\nSketch text.\n"),
            ("scene/sketch/a.js", "console.log(2)"),
            ("notes/First.md", "---\ntitle: First\n---\n\nFirst text.\n"),
        ],
    );

    // The pages inside still render; the ordinary folder keeps its listing.
    assert!(site.html("scene/credits/index.html").contains("Credit text."));
    assert!(site.html("scene/sketch/index.html").contains("Sketch text."));
    assert!(!site.html("notes/index.html").contains("http-equiv=\"refresh\""));
    assert!(site.html("notes/index.html").contains("/notes/first/"));

    let scene = site.html("scene/index.html");
    assert!(scene.contains("http-equiv=\"refresh\"") && scene.contains("url=/"), "{scene}");
    let table = site.html("_moss/redirects.json");
    assert!(table.contains(r#"{"from":"/scene/","to":"/","status":301}"#), "{table}");
    assert!(!table.contains("/scene/sketch/"), "an authored page keeps its address: {table}");
}

#[test]
fn a_nested_folder_without_a_page_of_its_own_forwards_to_the_nearest_served_ancestor() {
    let site = Site::with_config(
        "domain = \"example.org\"\n\n[build]\npassthrough = [\"scene\"]\n",
        &[
            ("index.md", "---\ntitle: Home\n---\n\nText.\n"),
            ("scene/sketch/deep/Note.md", "---\ntitle: Note\n---\n\nNote text.\n"),
        ],
    );
    assert!(site.html("scene/sketch/deep/note/index.html").contains("Note text."));
    let table = site.html("_moss/redirects.json");
    for from in ["/scene/", "/scene/sketch/", "/scene/sketch/deep/"] {
        assert!(table.contains(&format!(r#"{{"from":"{from}","to":"/","status":301}}"#)), "{from}: {table}");
    }
}

#[test]
fn a_passthrough_folder_inside_a_listed_folder_forwards_to_that_listing() {
    let site = Site::with_config(
        "domain = \"example.org\"\n\n[build]\npassthrough = [\"work/scene\"]\n",
        &[
            ("index.md", "---\ntitle: Home\n---\n\nText.\n"),
            ("work/Piece.md", "---\ntitle: Piece\n---\n\nPiece text.\n"),
            ("work/scene/Credits.md", "---\ntitle: Credits\n---\n\nCredit text.\n"),
        ],
    );
    assert!(site.html("work/index.html").contains("/work/piece/"));
    let table = site.html("_moss/redirects.json");
    assert!(table.contains(r#"{"from":"/work/scene/","to":"/work/","status":301}"#), "{table}");
}

#[test]
fn a_page_inside_a_passthrough_folder_has_no_trail_link_to_a_listing_that_is_not_there() {
    let site = Site::with_config(
        "domain = \"example.org\"\n\n[build]\npassthrough = [\"scene\"]\n",
        &[
            ("index.md", "---\ntitle: Home\nbreadcrumb: true\n---\n\nText.\n"),
            ("scene/Credits.md", "---\ntitle: Credits\n---\n\nCredit text.\n"),
            ("scene/sketch/README.md", "---\ntitle: Sketch\n---\n\nSketch text.\n"),
            ("notes/First.md", "---\ntitle: First\n---\n\nFirst text.\n"),
        ],
    );
    let credits = site.html("scene/credits/index.html");
    assert!(!site::hrefs_in(&credits).contains(&"/scene/"), "{credits}");
    assert!(site::hrefs_in(&site.html("notes/first/index.html")).contains(&"/notes/"), "an ordinary folder still links");
    assert!(!site.logged.iter().any(|m| m.contains("Dead links")), "{:?}", site.logged);
}
