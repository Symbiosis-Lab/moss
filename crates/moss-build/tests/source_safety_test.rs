//! Empty source files and ambiguous output addresses must not become pages.

#[path = "support/site.rs"]
mod site;

use moss_build::build::BuildTrigger;
use site::Site;

#[test]
fn empty_sources_are_skipped_but_metadata_only_pages_remain() {
    let site = Site::new(&[
        ("index.md", "Welcome."),
        ("blank.md", " \n\t\n"),
        ("untitled.md", ""),
        ("yaml.md", "---\n---\n \n"),
        ("mapping.md", "---\n{}\n---\n"),
        ("metadata.md", "---\ntitle: Metadata\n---\n"),
    ]);
    for name in ["blank", "untitled", "yaml", "mapping"] {
        assert!(!site.output(&format!("{name}/index.html")).exists(), "{name} emitted a page");
        assert!(site.notes.iter().any(|line| line.contains(&format!("{name}.md")) && line.contains("empty")), "{:?}", site.logged);
    }
    assert!(site.output("metadata/index.html").exists());
}

#[test]
fn a_page_emptied_on_rebuild_loses_its_output() {
    let site = Site::new(&[("index.md", "Welcome."), ("note.md", "A useful note.")]);
    assert!(site.output("note/index.html").exists());
    site.write("note.md", " \n");
    site::build_sync(&site.root.to_string_lossy(), BuildTrigger::ContentOnly(vec![site.root.join("note.md")])).unwrap();
    assert!(!site.output("note/index.html").exists());
}

#[test]
fn an_empty_site_still_has_a_synthetic_home() {
    let site = Site::new(&[("blank.md", "")]);
    assert!(site.output("index.html").exists());
    assert!(!site.output("blank/index.html").exists());
}

fn collision(files: &[(&str, &str)], first: &str, second: &str, url: &str) {
    let site = Site::new(&[("index.md", "Welcome.")]);
    for (path, content) in files {
        site.write(path, content);
    }
    let error = site::build_sync(&site.root.to_string_lossy(), BuildTrigger::Full)
        .expect_err("duplicate output URLs must stop the build");
    assert!(error.contains(first) && error.contains(second) && error.contains(url), "{error}");
}

#[test]
fn explicit_and_inferred_url_collisions_name_both_sources() {
    collision(&[("one.md", "---\nurl: shared\n---\nOne."), ("two.md", "---\nurl: shared\n---\nTwo.")], "one.md", "two.md", "/shared/");
    collision(&[("Hello World.md", "One."), ("hello-world.md", "Two.")], "Hello World.md", "hello-world.md", "/hello-world/");
}

#[test]
fn copied_folder_urls_report_the_parent_collision() {
    collision(&[
        ("first/index.md", "---\nurl: shared\n---\nFirst."),
        ("copy/index.md", "---\nurl: shared\n---\nCopy."),
        ("first/note.md", "First note."),
        ("copy/note.md", "Copied note."),
    ], "first/index.md", "copy/index.md", "/shared/");
}

#[test]
fn language_prefixes_and_non_page_slots_are_not_collisions() {
    let site = Site::new(&[
        ("index.md", "Welcome."),
        ("about.md", "---\nlang: en\n---\nAbout."),
        ("about.de.md", "---\nlang: de\n---\nÜber."),
        ("part.md", "---\npartial: true\nurl: about\n---\nA partial."),
    ]);
    assert!(site.output("about/index.html").exists());
    assert!(site.output("de/about/index.html").exists());
}

#[test]
fn a_content_edit_cannot_introduce_a_url_collision() {
    let site = Site::new(&[("index.md", "Welcome."), ("one.md", "One."), ("two.md", "Two.")]);
    site.write("two.md", "---\nurl: one\n---\nTwo.");
    let error = site::build_sync(&site.root.to_string_lossy(), BuildTrigger::ContentOnly(vec![site.root.join("two.md")]))
        .expect_err("an incremental URL collision must stop the build");
    assert!(error.contains("one.md") && error.contains("two.md") && error.contains("/one/"), "{error}");
}

#[test]
fn an_explicit_url_cannot_take_a_siblings_inferred_url() {
    collision(&[("awards/other.md", "---\nurl: comics\n---\nOne."), ("awards/comics.md", "Two.")],
        "awards/other.md", "awards/comics.md", "/awards/comics/");
}

#[test]
fn a_markdown_page_and_an_html_source_cannot_share_an_output() {
    collision(&[("docs.md", "Documentation."), ("docs/index.html", "<p>Static documentation.</p>")],
        "docs.md", "docs/index.html", "/docs/");
}

#[test]
fn a_page_and_a_folder_index_cannot_share_an_output() {
    collision(&[("docs.md", "Documentation."), ("docs/index.md", "Folder documentation.")],
        "docs.md", "docs/index.md", "/docs/");
}

#[test]
fn html_sources_in_equivalent_slug_directories_cannot_share_an_output() {
    collision(&[("My Docs/index.html", "<p>First.</p>"), ("my-docs/index.html", "<p>Second.</p>")],
        "My Docs/index.html", "my-docs/index.html", "/my-docs/");
}
