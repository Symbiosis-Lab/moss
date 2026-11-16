//! A `page` field placed at `end` closes the page with a card for each page it
//! links to, through the real binary. All names and text are invented.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const MOSS: &str = env!("CARGO_BIN_EXE_moss-cli");

const CONFIG: &str = "[fields.next]\ntype = \"page\"\nplace = \"end\"\nlabel = \"next project\"\npictures = 2\n";

struct Site {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    root: PathBuf,
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn site(config: &str, harbour_next: &str) -> Site {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("site");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    write(&root, ".moss/config.toml", config);
    write(&root, "index.md", "---\ntitle: Home\n---\nWelcome\n");
    let harbour = format!("---\ntitle: Harbour Pavilion\n{harbour_next}---\n# Can a pier be a room?\n\n![](quay-one.jpg)\n\n![](quay-two.jpg)\n\n![](quay-three.jpg)\n");
    write(&root, "work/harbour-pavilion.md", &harbour);
    write(&root, "work/mill-house.md", "---\ntitle: Mill House\nnext: \"[[harbour-pavilion]]\"\n---\nThe mill opens onto the weir.\n\n## Method\n\nSlow.\n");
    for picture in ["quay-one", "quay-two", "quay-three"] {
        write(&root, &format!("work/{picture}.jpg"), "x");
    }
    Site { _tmp: tmp, home, root }
}

fn build(s: &Site, strict: bool) -> Output {
    let mut cmd = Command::new(MOSS);
    cmd.arg("build").arg(&s.root).env("MOSS_HOME", &s.home).env_remove("MOSS_ENV");
    if strict {
        cmd.arg("--strict");
    }
    cmd.output().expect("spawn moss-cli")
}

fn built(s: &Site, rel: &str) -> String {
    std::fs::read_to_string(s.root.join(".moss/build.nosync/current").join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

#[test]
fn a_linked_page_ends_the_page_as_a_card_with_its_opening_heading_and_pictures() {
    let s = site(CONFIG, "");
    let o = build(&s, true);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let html = built(&s, "work/mill-house/index.html");
    let start = html.find("<section class=\"moss-field-end\" data-field=\"next\">").unwrap_or_else(|| panic!("no section: {html}"));
    let section = &html[start..start + html[start..].find("</section>").unwrap()];
    assert!(section.contains("<h2 class=\"moss-field-end-label\">next project</h2>"), "{section}");
    assert_eq!(section.matches("class=\"moss-card\"").count(), 1, "{section}");
    assert!(section.contains("href=\"/work/harbour-pavilion/\""), "{section}");
    assert!(section.contains("<h3 class=\"moss-card-title\">Can a pier be a room?</h3>"), "the card headlines the target's opening heading: {section}");
    assert_eq!(section.matches("moss-card-cover").count(), 2, "the two pictures asked for, of three: {section}");
    assert!(html.find("Slow.").unwrap() < start && start < html.find("</article>").unwrap(), "after the body, inside the article: {html}");
}

#[test]
fn a_page_without_a_value_draws_no_section() {
    let s = site(CONFIG, "");
    assert!(build(&s, true).status.success());
    assert!(!built(&s, "work/harbour-pavilion/index.html").contains("moss-field-end"));
}

#[test]
fn a_link_that_names_no_page_warns_and_draws_nothing() {
    let s = site(CONFIG, "");
    write(&s.root, "work/mill-house.md", "---\ntitle: Mill House\nnext: \"[[nowhere-at-all]]\"\n---\nbody\n");
    let strict = build(&s, true);
    let err = String::from_utf8_lossy(&strict.stderr);
    assert!(!strict.status.success() && err.contains("nowhere-at-all"), "{err}");
    let lenient = build(&s, false);
    assert!(String::from_utf8_lossy(&lenient.stderr).contains("nowhere-at-all"));
    assert!(!built(&s, "work/mill-house/index.html").contains("moss-field-end"));
}

#[test]
fn a_target_without_a_different_opening_heading_is_headlined_by_its_title_and_the_label_defaults_to_the_name() {
    let s = site("[fields.next]\ntype = \"page\"\nplace = \"end\"\n", "");
    write(&s.root, "work/harbour-pavilion.md", "---\ntitle: Harbour Pavilion\n---\n# Harbour Pavilion\n\nWords.\n");
    assert!(build(&s, true).status.success());
    let html = built(&s, "work/mill-house/index.html");
    assert!(html.contains("<h2 class=\"moss-field-end-label\">Next</h2>"), "{html}");
    assert!(html.contains("<h3 class=\"moss-card-title\">Harbour Pavilion</h3>"), "{html}");
}

#[test]
fn end_on_a_text_field_warns_and_draws_nowhere() {
    let s = site("[fields.next]\ntype = \"text\"\nplace = \"end\"\n", "");
    let o = build(&s, false);
    assert!(String::from_utf8_lossy(&o.stderr).contains("needs type \"page\" or \"pages\""), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(!built(&s, "work/mill-house/index.html").contains("moss-field-end"));
}
