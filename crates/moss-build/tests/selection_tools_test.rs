//! `[site].selection_tools`: Share and Copy on selected text, and the QR codes
//! the shared picture carries. Off by default; off ships no QR code, no
//! share-card chunk and no attribute pointing at either.

#[path = "support/site.rs"]
mod site;

use site::Site;

const PAGES: &[(&str, &str)] = &[
    ("index.md", "---\ntitle: River Notes\n---\n\nThe water came up past the second step.\n"),
    ("about.md", "---\ntitle: About\ncover: cover.png\n---\n\nA small site about the river.\n"),
    ("notes/spring.md", "---\ntitle: Spring\n---\n\nFlood, day three.\n"),
];

const ON: &str = "domain = \"example.com\"\nselection_tools = true\n";
const OFF: &str = "domain = \"example.com\"\n";

fn scripts_named(site: &Site, name: &str) -> usize {
    std::fs::read_dir(site.output("_moss/js"))
        .unwrap()
        .filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().starts_with(&format!("{name}.")))
        .count()
}

fn share_card_chunks(site: &Site) -> usize {
    scripts_named(site, "share-card")
}

fn stylesheet(site: &Site) -> String {
    let css = std::fs::read_dir(site.output("_moss"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.file_name().unwrap().to_string_lossy().starts_with("style."))
        .unwrap();
    std::fs::read_to_string(css).unwrap()
}

fn has_selection_tag(html: &str) -> bool {
    html.contains(r#"<script src="/_moss/js/selection."#)
}

#[test]
fn a_site_that_never_asked_ships_no_share_tools_and_no_qr_codes() {
    let site = Site::with_config(OFF, PAGES);
    let about = site.html("about/index.html");
    for attr in ["data-share-qr", "data-share-cover", "data-share-card"] {
        assert!(!about.contains(attr), "{attr} on a site without selection tools:\n{about}");
    }
    assert!(!site.output("qr").exists(), "no qr/ folder at all");
    assert_eq!(share_card_chunks(&site), 0);
    assert!(!has_selection_tag(&about), "no selection script either:\n{about}");
    assert_eq!(scripts_named(&site, "selection"), 0);
    assert!(!stylesheet(&site).contains(".sel-popover"), "nor its stylesheet");
}

#[test]
fn comments_keep_the_selection_popover_without_share_or_copy() {
    let site = Site::with_config(&format!("{OFF}\n[services.comments]\nenabled = true\n"), PAGES);
    let about = site.html("about/index.html");
    assert!(has_selection_tag(&about), "Comment on selected text needs the script:\n{about}");
    assert!(!about.contains("data-share-card"), "{about}");
    assert!(!site.output("qr").exists());
    assert!(stylesheet(&site).contains(".sel-popover"));
}

#[test]
fn a_site_that_asks_gets_a_code_per_page_and_the_share_card() {
    let site = Site::with_config(ON, PAGES);
    let about = site.html("about/index.html");
    assert!(about.contains(r#"data-share-qr="/qr/about.svg""#), "{about}");
    assert!(about.contains("data-share-cover="), "{about}");
    assert!(about.contains("data-share-card="), "{about}");
    assert!(has_selection_tag(&about), "{about}");
    assert!(stylesheet(&site).contains(".sel-popover"));
    for qr in ["qr/index.svg", "qr/about.svg", "qr/notes/spring.svg", "qr/notes.svg"] {
        assert!(site.output(qr).is_file(), "{qr}");
    }
    assert_eq!(share_card_chunks(&site), 1);
}

#[test]
fn turning_the_tools_off_removes_every_code_on_the_next_build() {
    let site = Site::with_config(ON, PAGES);
    assert!(site.output("qr/about.svg").is_file());
    site.write(".moss/config.toml", &format!("schema_version = 6\n\n[site]\nlang = \"en\"\n{OFF}"));
    site.rebuild();
    assert!(!site.output("qr/about.svg").exists(), "a carried page must not carry its old code");
    assert!(!site.output("qr/notes.svg").exists(), "nor a generated folder page");
    assert!(!site.html("about/index.html").contains("data-share-qr"));
    assert_eq!(share_card_chunks(&site), 0);
}

#[test]
fn turning_them_back_on_restores_the_codes() {
    let site = Site::with_config(OFF, PAGES);
    site.write(".moss/config.toml", &format!("schema_version = 6\n\n[site]\nlang = \"en\"\n{ON}"));
    site.rebuild();
    assert!(site.output("qr/about.svg").is_file());
    assert!(site.html("about/index.html").contains(r#"data-share-qr="/qr/about.svg""#));
}

/// A page's own `comments: true` beats `[site].comments = false`, so the
/// service being on is enough to ship the popover's Comment.
#[test]
fn a_page_that_opts_into_comments_keeps_comment_on_selected_text() {
    let site = Site::with_config(
        &format!("{OFF}comments = false\n\n[services.comments]\nenabled = true\n"),
        &[
            ("index.md", "---\ntitle: River Notes\n---\n\nHome.\n"),
            ("talk.md", "---\ntitle: Talk\ncomments: true\n---\n\nSay something.\n"),
        ],
    );
    assert!(has_selection_tag(&site.html("talk/index.html")));
}
