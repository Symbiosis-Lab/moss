//! Empty files build for the preview only; ambiguous output addresses render with diagnostics.

#[path = "support/site.rs"]
mod site;

use moss_build::build::BuildTrigger;
use site::Site;

#[test]
fn an_empty_page_is_built_for_the_preview_and_noted() {
    let site = Site::new(&[
        ("index.md", "Welcome."),
        ("blank.md", " \n\t\n"),
        ("untitled.md", ""),
        ("yaml.md", "---\n---\n \n"),
        ("mapping.md", "---\n{}\n---\n"),
        ("metadata.md", "---\ntitle: Metadata\n---\n"),
    ]);
    for name in ["blank", "untitled", "yaml", "mapping"] {
        assert!(site.output(&format!("{name}/index.html")).exists(), "{name} has no page to preview");
        assert!(site.notes.iter().any(|line| line.contains(&format!("{name}.md")) && line.contains("preview only")), "{:?}", site.notes);
        assert!(!site.html("index.html").contains(&format!("/{name}/")), "the home lists or links {name}");
    }
    assert!(site.output("metadata/index.html").exists());
    assert!(!site.notes.iter().any(|line| line.contains("metadata.md")), "{:?}", site.notes);
}

#[test]
fn an_empty_page_becomes_an_ordinary_page_with_its_first_word() {
    let site = Site::new(&[("index.md", "Welcome."), ("notes/first.md", "A note."), ("notes/Second Note.md", "")]);
    assert!(site.output("notes/second-note/index.html").exists());
    assert!(!site.html("notes/index.html").contains("second-note"), "an empty page is listed");
    site.write("notes/Second Note.md", "Word");
    site::build_sync(&site.root.to_string_lossy(), BuildTrigger::ContentOnly(vec![site.root.join("notes/Second Note.md")])).unwrap();
    assert!(site.html("notes/second-note/index.html").contains("Word"));
    assert!(site.html("notes/index.html").contains("second-note"), "the page is not listed once it has text");
}

#[test]
fn an_empty_home_file_builds_no_page_and_its_folder_keeps_the_generated_index() {
    let site = Site::new(&[
        ("index.md", "Welcome."),
        ("stories/index.md", ""),
        ("stories/one.md", "One."),
        ("letters/Letters.md", "---\n---\n"),
        ("letters/two.md", "Two."),
        ("essays/index.fr.md", ""),
        ("essays/three.md", "Three."),
    ]);
    for (folder, page) in [("stories", "one"), ("letters", "two"), ("essays", "three")] {
        assert!(site.html(&format!("{folder}/index.html")).contains(&format!("/{folder}/{page}/")), "{folder} lost its generated index");
        assert!(site.notes.iter().any(|line| line.contains(&format!("{folder}/")) && line.contains("Skipping empty page")), "{:?}", site.notes);
    }
}

#[test]
fn an_empty_site_still_has_a_synthetic_home() {
    let site = Site::new(&[("blank.md", "")]);
    assert!(site.output("index.html").exists());
}

fn collision(files: &[(&str, &str)], first: &str, second: &str, url: &str) {
    let site = Site::new(&[("index.md", "Welcome.")]);
    for (path, content) in files {
        site.write(path, content);
    }
    let warnings = site::build_sync(&site.root.to_string_lossy(), BuildTrigger::Full)
        .expect("collisions must still render").join("\n");
    assert!(warnings.contains(first) && warnings.contains(second) && warnings.contains(url), "{warnings}");
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
fn a_content_edit_reports_a_url_collision() {
    let site = Site::new(&[("index.md", "Welcome."), ("one.md", "One."), ("two.md", "Two.")]);
    site.write("two.md", "---\nurl: one\n---\nTwo.");
    let warnings = site::build_sync(&site.root.to_string_lossy(), BuildTrigger::ContentOnly(vec![site.root.join("two.md")]))
        .expect("incremental collisions must still render").join("\n");
    assert!(warnings.contains("one.md") && warnings.contains("two.md") && warnings.contains("/one/"), "{warnings}");
}

#[test]
fn an_explicit_url_reports_a_collision_with_a_siblings_inferred_url() {
    collision(&[("awards/other.md", "---\nurl: comics\n---\nOne."), ("awards/comics.md", "Two.")],
        "awards/other.md", "awards/comics.md", "/awards/comics/");
}

#[test]
fn a_markdown_page_and_an_html_source_report_a_collision() {
    collision(&[("docs.md", "Documentation."), ("docs/index.html", "<p>Static documentation.</p>")],
        "docs.md", "docs/index.html", "/docs/");
}

#[test]
fn a_page_and_a_folder_index_report_a_collision() {
    collision(&[("docs.md", "Documentation."), ("docs/index.md", "Folder documentation.")],
        "docs.md", "docs/index.md", "/docs/");
}

#[test]
fn html_sources_in_equivalent_slug_directories_report_a_collision() {
    collision(&[("My Docs/index.html", "<p>First.</p>"), ("my-docs/index.html", "<p>Second.</p>")],
        "My Docs/index.html", "my-docs/index.html", "/my-docs/");
}
