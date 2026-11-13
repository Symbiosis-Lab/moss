//! A folder embed, and the More link under it, find a folder whose home sets
//! a `url:` different from the folder's name.

#[path = "support/site.rs"]
mod site;

use site::Site;

fn home(embed: &str) -> String {
    format!("---\ntitle: Home\nchildren: false\n---\n\n{embed}\n")
}

fn dated(title: &str, day: u32) -> String {
    format!("---\ntitle: {title}\ndate: 2026-01-0{day}\n---\n\nText.\n")
}

fn embed_markup(html: &str) -> &str {
    let from = html.find("moss-cards-container").unwrap_or_else(|| panic!("no listing in {html}"));
    &html[from..html[from..].find("</article>").map_or(html.len(), |i| from + i)]
}

#[test]
fn an_embed_of_a_renamed_folder_lists_it_and_links_more_to_its_url() {
    let home = home("![[Notes-src/|limit:1]]");
    let (one, two) = (dated("Note One", 1), dated("Note Two", 2));
    let site = Site::new(&[
        ("index.md", &home),
        ("Notes-src/index.md", "---\ntitle: Notes\nurl: notes\n---\n\nNotes.\n"),
        ("Notes-src/One.md", &one),
        ("Notes-src/Two.md", &two),
    ]);
    let listing = embed_markup(&site.html("index.html")).to_string();
    assert!(listing.contains("Note Two") && !listing.contains("Note One"), "{listing}");
    assert!(listing.contains("<p class=\"moss-embed-more\"><a href=\"/notes/\">"), "{listing}");
}

#[test]
fn a_non_ascii_folder_with_an_ascii_url_still_draws_its_more_link() {
    let home = home("![[随记/|limit:1]]");
    let (one, two) = (dated("Note One", 1), dated("Note Two", 2));
    let site = Site::new(&[
        ("index.md", &home),
        ("随记/随记.md", "---\ntitle: Notes\nurl: notes\n---\n\nNotes.\n"),
        ("随记/One.md", &one),
        ("随记/Two.md", &two),
    ]);
    let listing = embed_markup(&site.html("index.html")).to_string();
    assert!(listing.contains("Note Two") && !listing.contains("Note One"), "{listing}");
    assert!(listing.contains("<p class=\"moss-embed-more\"><a href=\"/notes/\">"), "{listing}");
}

#[test]
fn more_naming_a_renamed_folder_points_at_its_url() {
    let home = home("![[Notes-src/|limit:1,more:Notes-src]]");
    let (one, two) = (dated("Note One", 1), dated("Note Two", 2));
    let site = Site::new(&[
        ("index.md", &home),
        ("Notes-src/index.md", "---\ntitle: Notes\nurl: notes\n---\n\nNotes.\n"),
        ("Notes-src/One.md", &one),
        ("Notes-src/Two.md", &two),
    ]);
    let listing = embed_markup(&site.html("index.html")).to_string();
    assert!(listing.contains("<p class=\"moss-embed-more\"><a href=\"/notes/\">Notes"), "{listing}");
}
