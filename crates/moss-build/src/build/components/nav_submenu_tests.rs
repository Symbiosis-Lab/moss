use super::*;

fn doc(url_path: &str, title: &str, weight: Option<i32>, kind: moss_core::PageKind) -> ParsedDocument {
    ParsedDocument {
        url_path: url_path.to_string(),
        title: title.to_string(),
        label: title.to_string(),
        weight,
        nav: Some(true),
        kind,
        ..Default::default()
    }
}

fn folder(url_path: &str, title: &str, weight: Option<i32>) -> ParsedDocument {
    doc(url_path, title, weight, moss_core::PageKind::Folder)
}

fn page(url_path: &str, title: &str, weight: Option<i32>) -> ParsedDocument {
    doc(url_path, title, weight, moss_core::PageKind::Article)
}

fn render(documents: &[ParsedDocument], current: Option<&str>) -> String {
    NavigationBuilder::new(documents, "Site", current, crate::i18n::Language::En, false).generate_navigation()
}

/// The `<ul class="nav-sub">` markup, or empty when there is none.
fn submenu(html: &str) -> &str {
    html.split_once(r#"<ul class="nav-sub""#)
        .and_then(|(_, rest)| rest.split_once("</ul>"))
        .map_or("", |(inside, _)| inside)
}

#[test]
fn nav_child_of_a_nav_folder_is_listed_in_its_submenu() {
    let documents = vec![
        folder("services/index.html", "Services", Some(1)),
        page("services/patrol/index.html", "Patrol", Some(1)),
        page("about/index.html", "About", Some(2)),
    ];
    let html = render(&documents, None);

    assert!(html.contains(r#"<span class="nav-group"><a href="/services/""#), "got: {html}");
    assert!(
        html.contains(r#"aria-controls="nav-sub-1""#) && html.contains(r#"<ul class="nav-sub" id="nav-sub-1" hidden>"#),
        "got: {html}"
    );
    assert!(submenu(&html).contains(r#"<li><a href="/services/patrol/""#), "got: {html}");
    assert_eq!(html.matches(r#"href="/services/patrol/""#).count(), 1, "listed once, not also top-level: {html}");
    assert!(html.contains(r#"href="/about""#) || html.contains(r#"href="/about/""#), "got: {html}");
}

#[test]
fn nav_child_of_a_folder_outside_the_header_stays_a_top_level_link() {
    let mut index = folder("services/index.html", "Services", None);
    index.nav = Some(false);
    let documents = vec![index, page("services/patrol/index.html", "Patrol", Some(1))];
    let html = render(&documents, None);

    assert!(html.contains(r#"<a href="/services/patrol/""#), "got: {html}");
    assert!(!html.contains("nav-group") && !html.contains("nav-sub"), "got: {html}");
}

#[test]
fn nav_folder_whose_children_are_not_in_the_header_gets_no_group() {
    let mut child = page("services/patrol/index.html", "Patrol", None);
    child.nav = None;
    let documents = vec![folder("services/index.html", "Services", Some(1)), child];
    let html = render(&documents, None);

    assert!(html.contains(r#"<a href="/services/""#), "got: {html}");
    assert!(!html.contains("nav-group") && !html.contains("nav-sub") && !html.contains("Patrol"), "got: {html}");
}

#[test]
fn deeper_nav_pages_join_the_outermost_folder_submenu() {
    let documents = vec![
        folder("services/index.html", "Services", None),
        folder("services/water/index.html", "Water", Some(2)),
        page("services/water/tests/index.html", "Tests", Some(1)),
    ];
    let html = render(&documents, None);
    let list = submenu(&html);

    assert_eq!(html.matches("nav-group").count(), 1, "one group only: {html}");
    assert!(list.contains(">Water<") && list.contains(">Tests<"), "got: {html}");
    assert!(list.find(">Tests<") < list.find(">Water<"), "weight order: {html}");
}

#[test]
fn current_child_marks_its_parent_as_the_current_section() {
    let documents = vec![
        folder("services/index.html", "Services", None),
        page("services/patrol/index.html", "Patrol", None),
    ];
    let html = render(&documents, Some("services/patrol/index.html"));

    assert!(html.contains(r#"<a href="/services/" class="active" aria-current="true""#), "got: {html}");
    assert!(submenu(&html).contains(r#"class="active" aria-current="page""#), "got: {html}");
}

#[test]
fn submenu_labels_are_escaped_and_ordered_by_weight_then_label() {
    let mut zed = page("services/zed/index.html", "Zed", Some(1));
    zed.nav_label = Some("Q&A <b>".to_string());
    let documents = vec![
        folder("services/index.html", "A & B", None),
        page("services/beta/index.html", "Beta", None),
        page("services/alpha/index.html", "Alpha", None),
        zed,
    ];
    let html = render(&documents, None);
    let list = submenu(&html);

    assert!(html.contains(r#"aria-label="A &amp; B: more""#), "got: {html}");
    assert!(list.contains("Q&amp;A &lt;b&gt;") && !list.contains("<b>"), "got: {html}");
    let at = |needle: &str| list.find(needle).unwrap();
    assert!(at("Q&amp;A") < at(">Alpha<") && at(">Alpha<") < at(">Beta<"), "got: {html}");
}

#[test]
fn link_only_partials_in_a_folder_without_a_home_page_stay_top_level() {
    let link = |name: &str, url: &str| {
        let mut d = page(&format!("services/{name}/index.html"), name, None);
        d.slot_only = true;
        d.raw_frontmatter.insert("partial".into(), serde_json::json!(true));
        d.raw_frontmatter.insert("external_url".into(), serde_json::json!(url));
        d
    };
    let documents = vec![link("book", "https://example.org/book"), link("call", "mailto:hello@example.org")];
    let html = render(&documents, None);

    assert!(html.contains(r#"href="https://example.org/book""#) && html.contains(r#"href="mailto:hello@example.org""#), "got: {html}");
    assert!(!html.contains("nav-group") && !html.contains("nav-sub"), "got: {html}");
}

/// A folder's home page leaves `nav` unset: it is not in the bar by position
/// (only a top-level file is), so its nav child stays a plain link.
#[test]
fn folder_home_without_nav_is_not_in_the_bar_by_position() {
    let mut index = folder("services/index.html", "Services", None);
    index.nav = None;
    index.is_root_level = false;
    let documents = vec![index, page("services/patrol/index.html", "Patrol", None)];
    let html = NavigationBuilder::new(&documents, "Site", None, crate::i18n::Language::En, true).generate_navigation();

    assert!(html.contains(">Patrol<") && !html.contains(">Services<"), "got: {html}");
    assert!(!html.contains("nav-group"), "got: {html}");
}
