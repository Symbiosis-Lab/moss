use super::super::{audit, render_text, to_json};
use std::path::Path;

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// A theme naming a moss class, one invented class a page fence uses, one
/// invented class no page uses, and an attribute selector on `.pdf`.
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        ".moss/theme/style.css",
        "/* .in-comment { } */\n.moss-card { color: red; }\n.price-tag { color: blue; }\na[href$=\".pdf\"] { x: 1; }\na[href$=.zip] { x: 1; }\n.unused-bit::before { content: \".not-a-class\"; background: url(a.b.png); }\n",
    );
    write(root, "Menu.md", "---\ntitle: Menu\n---\n\n::: {.price-tag}\nTea 3\n:::\n");
    dir
}

#[test]
fn an_invented_class_is_listed_with_where_it_is_used() {
    let dir = fixture();
    let report = audit(dir.path(), false);
    let own: Vec<(&str, usize)> = report.own_classes.iter().map(|c| (c.class.as_str(), c.pages)).collect();
    assert_eq!(own, vec![("price-tag", 1), ("unused-bit", 0)]);
    let tag = &report.own_classes[0];
    assert_eq!(tag.defined, (".moss/theme/style.css".to_string(), 3));
    assert_eq!(tag.example, Some(("Menu.md".to_string(), 5)));

    let text = render_text(&report);
    assert!(text.contains(".price-tag"), "{text}");
    assert!(text.contains("used in pages"), "{text}");
    assert!(text.contains("used nowhere"), "{text}");
    for not_a_class in ["moss-card", "pdf", "zip", "not-a-class", "in-comment", "png"] {
        assert!(!text.contains(&format!(".{not_a_class}")), "{not_a_class} listed:\n{text}");
    }
}

#[test]
fn the_json_carries_theme_own_classes() {
    let dir = fixture();
    let json = to_json(&audit(dir.path(), false));
    let list = json["theme_own_classes"].as_array().expect("array");
    assert_eq!(list.len(), 2);
    assert_eq!(list[0]["class"], "price-tag");
    assert_eq!(list[0]["pages"], 1);
    assert_eq!(list[0]["example"]["path"], "Menu.md");
    assert_eq!(list[0]["defined"]["line"], 3);
    assert_eq!(list[1]["pages"], 0);
    assert!(list[1]["example"].is_null());
}

#[test]
fn a_theme_with_only_moss_classes_names_none_of_its_own() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), ".moss/theme/style.css", ".moss-card, .moss-card:hover > .moss-card-title { x: 1 }\n");
    let report = audit(dir.path(), false);
    assert!(report.own_classes.is_empty());
    assert!(render_text(&report).contains("The theme names no class of its own."));
    assert_eq!(to_json(&report)["theme_own_classes"], serde_json::json!([]));
}

#[test]
fn html_class_attributes_count_and_code_does_not() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, ".moss/theme/style.css", ".aside-note, .coded { x: 1 }\n");
    write(root, "a.md", "<p class=\"wide aside-note\">x</p>\n\n```md\n::: {.coded}\n```\n\nand `{.coded}`\n");
    let report = audit(root, false);
    let by_name: Vec<(&str, usize)> = report.own_classes.iter().map(|c| (c.class.as_str(), c.pages)).collect();
    assert_eq!(by_name, vec![("aside-note", 1), ("coded", 0)]);
}

#[test]
fn a_class_in_an_imported_sheet_is_found() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, ".moss/theme/style.css", "@import \"parts/extra.css\";\n@import url('../shared.css');\n");
    write(root, ".moss/theme/parts/extra.css", ".from-import { x: 1 }\n");
    write(root, ".moss/shared.css", ".from-shared { x: 1 }\n");
    let report = audit(root, false);
    let names: Vec<&str> = report.own_classes.iter().map(|c| c.class.as_str()).collect();
    assert_eq!(names, vec!["from-import", "from-shared"]);
}

#[test]
fn an_escaped_class_is_read_whole() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), ".moss/theme/style.css", ".sm\\:flex, .w-1\\/2, .a\\31 0 { x: 1 }\n");
    let names: Vec<String> = audit(dir.path(), false).own_classes.into_iter().map(|c| c.class).collect();
    assert_eq!(names, vec!["a10", "sm:flex", "w-1/2"]);
}

#[test]
fn nested_rules_are_not_read() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), ".moss/theme/style.css", ".outer-one { &.inner-one { x: 1 } }\n");
    let names: Vec<String> = audit(dir.path(), false).own_classes.into_iter().map(|c| c.class).collect();
    assert_eq!(names, vec!["outer-one"]);
}

#[test]
fn a_comment_opener_inside_a_string_hides_no_import() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, ".moss/theme/style.css", "@import url(\"https://fonts.example/a/*b.css\");\n@import \"../shared.css\";\n");
    write(root, ".moss/shared.css", ".from-shared { x: 1 }\n");
    let names: Vec<String> = audit(root, false).own_classes.into_iter().map(|c| c.class).collect();
    assert_eq!(names, vec!["from-shared"]);
}
