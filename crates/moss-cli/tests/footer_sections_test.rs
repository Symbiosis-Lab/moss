//! In `footer.md`, each heading starts a column: the built footer wraps every
//! heading and what follows it in a `<section>`, and its headings carry
//! neither the id nor the permalink a page heading gets, so a footer `##
//! Contact` cannot collide with a page's own. Built through the real CLI.

use std::path::Path;
use std::process::{Command, Stdio};

fn write(site: &Path, rel: &str, body: &str) {
    let p = site.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

fn footer_of_home(footer_md: &str) -> (String, String) {
    footer_of_home_with(footer_md, None)
}

fn footer_of_home_with(footer_md: &str, config: Option<&str>) -> (String, String) {
    let moss_home = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let site = std::fs::canonicalize(tmp.path()).unwrap();
    write(&site, "index.md", "---\ntitle: Home\n---\n\n## Contact\n\nWrite to us.\n");
    write(&site, "footer.md", footer_md);
    if let Some(config) = config {
        write(&site, ".moss/config.toml", config);
    }
    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .args(["build", site.to_str().unwrap(), "--no-plugins", "--strict"])
        .env("MOSS_HOME", moss_home.path())
        .stdin(Stdio::null())
        .output()
        .expect("run moss-cli");
    assert!(out.status.success(), "stderr:\n{}", String::from_utf8_lossy(&out.stderr));
    let html = std::fs::read_to_string(site.join(".moss/build.nosync/current/index.html")).unwrap();
    let start = html.find("<footer").expect("footer");
    let end = html.find("</footer>").expect("footer end");
    (html[start..end].to_string(), html)
}

#[test]
fn each_footer_heading_starts_a_column_without_a_page_id_or_permalink() {
    let (footer, page) = footer_of_home(
        "Lead line.\n\n## Site\n\n- [Home](/)\n\n## Contact\n\nMail us.\n\n### Hours\n\nDaily.\n",
    );
    assert_eq!(footer.matches("<section>").count(), 3, "lead + two columns:\n{footer}");
    assert!(footer.contains("<section>\n<h2>Contact</h2>\n<p>Mail us.</p>\n<h3>Hours</h3>\n<p>Daily.</p>\n</section>"), "{footer}");
    assert!(!footer.contains("moss-heading-anchor") && !footer.contains(" id="), "footer headings: {footer}");
    assert_eq!(page.matches("id=\"contact\"").count(), 1, "only the page's own heading keeps the id");
}

#[test]
fn a_footer_without_headings_has_no_sections() {
    let (footer, _) = footer_of_home("One line of footer text.\n\nAnother.\n");
    assert!(!footer.contains("<section"), "{footer}");
    assert!(footer.contains("<p>One line of footer text.</p>\n<p>Another.</p>"), "{footer}");
}

#[test]
fn footer_headings_carry_no_id_when_heading_anchors_are_off() {
    let (footer, page) = footer_of_home_with("## Contact\n\nMail us.\n", Some("[site]\nheading_anchors = false\n"));
    assert!(!footer.contains(" id="), "footer headings: {footer}");
    assert_eq!(page.matches("id=\"contact\"").count(), 1, "only the page's own heading keeps the id");
}

#[test]
fn raw_html_in_the_footer_does_not_change_how_it_splits() {
    let (footer, _) = footer_of_home(
        "## One\n\n<script>for(var i=0;i<3;i++){}</script>\n\n## Two\n\n<ul><li>a<li>b</ul>\n\n## Three\n\nEnd.\n",
    );
    assert_eq!(footer.matches("<section>").count(), 3, "one section per heading:\n{footer}");
}

#[test]
fn an_id_written_in_raw_html_survives() {
    let (footer, _) = footer_of_home("<h2 id=\"mine\">Own</h2>\n\nText.\n");
    assert!(footer.contains("<h2 id=\"mine\">Own</h2>"), "{footer}");
}

#[test]
fn a_thematic_break_starts_the_bottom_row_after_the_columns() {
    let (footer, _) = footer_of_home("## Site\n\nLinks.\n\n## Contact\n\nMail us.\n\n---\n\nCopyright\n\n[Terms](#terms)\n");
    assert_eq!(footer.matches("<section>").count(), 2, "{footer}");
    assert!(!footer.contains("<hr"), "the break itself is not drawn:\n{footer}");
    let last_section = footer.rfind("</section>").expect("sections");
    let bottom = footer.find("<div class=\"footer-bottom\">").expect("bottom row");
    assert!(bottom > last_section, "the bottom row follows the columns:\n{footer}");
    assert!(footer[bottom..].contains("<p>Copyright</p>") && footer[bottom..].contains("Terms"), "{footer}");
}

#[test]
fn a_break_without_headings_gives_one_column_and_a_bottom_row() {
    let (footer, _) = footer_of_home("About line.\n\n---\n\nCopyright\n");
    assert_eq!(footer.matches("<section>").count(), 1, "{footer}");
    assert!(footer.contains("<div class=\"footer-bottom\">\n<p>Copyright</p>"), "{footer}");
}
