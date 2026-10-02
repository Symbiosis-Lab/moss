use super::*;

#[test]
fn renders_h1_with_class() {
    assert_eq!(
        render("文字", false, false),
        r#"<h1 class="moss-folder-title">文字</h1>"#
    );
}

#[test]
fn html_escapes_label() {
    assert_eq!(
        render("Code & Tips <em>", false, false),
        r#"<h1 class="moss-folder-title">Code &amp; Tips &lt;em&gt;</h1>"#,
    );
}

#[test]
fn empty_label_renders_empty_h1() {
    // Defensive: an empty h1 is semantically wrong but the CSS rule
    // `.moss-folder-title:empty { display: none; }` hides it. Verify the
    // helper itself doesn't choke on empty input.
    assert_eq!(render("", false, false), r#"<h1 class="moss-folder-title"></h1>"#);
}

#[test]
fn editor_preview_points_the_heading_at_the_title_field() {
    // Clicking a folder page's heading in the preview must find the same
    // `title` field an article heading does — same attribute, same value.
    assert_eq!(
        render("Notes", true, false),
        r#"<h1 class="moss-folder-title" data-source-fm="title">Notes</h1>"#,
    );
}

#[test]
fn visually_hidden_adds_the_utility_class_without_suppressing_the_heading() {
    // The places explorer root's own case: no VISIBLE heading, but still a
    // real <h1> for screen-reader navigation (design decision 7).
    assert_eq!(
        render("Places", false, true),
        r#"<h1 class="moss-folder-title visually-hidden">Places</h1>"#,
    );
}
