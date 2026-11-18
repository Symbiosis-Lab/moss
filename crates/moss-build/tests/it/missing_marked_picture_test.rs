//! A picture marked for a gallery grid that is not in the folder: its page is
//! built and names it in a box, and the grid holds only the pictures that exist.

use crate::site;

use site::Site;

#[test]
fn a_missing_marked_picture_keeps_its_page_and_stays_out_of_the_grid() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Home\n---\n\nHello.\n"),
        (
            "gallery.md",
            "---\ntitle: Gallery\n---\n\n![Harbour](here.svg)\n<!-- photography -->\n\n![Lighthouse](gone.svg)\n<!-- photography -->\n",
        ),
        ("here.svg", r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 4 3"/>"#),
    ]);

    let page = site.html("gallery/index.html");
    assert!(page.contains(r#"<div class="moss-embed-missing">Picture not found: gone.svg</div>"#), "{page}");
    assert!(page.contains(r#"src="/here.svg""#), "{page}");

    let grid = site.html("photography/index.html");
    assert_eq!(grid.matches(r#"class="media-item""#).count(), 1, "{grid}");
    assert!(grid.contains("here.svg"), "{grid}");
    assert!(!grid.contains("gone.svg") && !grid.contains("Lighthouse"), "{grid}");
}
