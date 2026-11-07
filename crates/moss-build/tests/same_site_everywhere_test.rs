//! Two machines that build the same folder publish the same site. Here the
//! machines are two copies of one folder: the uid a build gives a new page,
//! which it never writes back, and what it publishes must match between them.

#[path = "support/site.rs"]
mod site;

use site::Site;

const FILES: [(&str, &str); 2] = [
    ("index.md", "---\ntitle: Home\n---\n\nWelcome.\n"),
    ("talk.md", "---\ntitle: Talk\nstart: 2099-03-01 19:30\n---\n\nA new page with no uid yet.\n"),
];

fn uid_of(site: &Site, page: &str) -> String {
    let map: serde_json::Value =
        serde_json::from_slice(&std::fs::read(site.root.join(".moss/build.nosync/article-map.json")).unwrap()).unwrap();
    let article = map["articles"].as_object().unwrap().values().find(|a| a["source_path"] == page);
    article.and_then(|a| a["uid"].as_str()).unwrap_or_else(|| panic!("no uid for {page}: {map}")).to_string()
}

#[test]
fn two_copies_of_a_new_page_get_the_same_uid_and_the_same_calendar_file() {
    let (one, two) = (Site::new(&FILES), Site::new(&FILES));
    assert_eq!(uid_of(&one, "talk.md"), uid_of(&two, "talk.md"));
    assert_eq!(std::fs::read_to_string(one.root.join("talk.md")).unwrap(), FILES[1].1, "the build writes no uid into the page");
    assert!(!one.root.join(".moss/build.nosync/provisional-uids.json").exists(), "no per-machine uid store");
    assert_eq!(one.html("talk/event.ics"), two.html("talk/event.ics"));
}
