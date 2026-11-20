use super::*;

fn folder_with_theme(css: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".moss/theme")).unwrap();
    std::fs::write(dir.path().join(THEME_CSS), css).unwrap();
    dir
}

#[test]
fn a_theme_lists_each_retired_name_it_uses_with_its_count_and_migration() {
    let dir = folder_with_theme(
        ".moss-cards-grid { gap: 1rem; }\n.moss-cards-grid > a { color: red; }\n.moss-visual { display: none; }\n\
         body { font-size: var(--moss-font-size); }\n/* .moss-card-list is only in a comment */\n.moss-card-grid-overlay { }\n",
    );
    let json = to_json(&found(dir.path()).unwrap());
    let rows: Vec<(String, String, usize)> = json["retired"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["id"].as_str().unwrap().to_string(), r["migrate"].as_str().unwrap().to_string(), r["occurrences"].as_u64().unwrap() as usize))
        .collect();
    assert_eq!(
        rows,
        [
            ("class:moss-cards-grid".to_string(), "rename".to_string(), 2),
            ("class:moss-visual".to_string(), "gone".to_string(), 1),
            ("property:--moss-font-size".to_string(), "rename".to_string(), 1),
        ]
    );
    let grid = &json["retired"][0];
    assert_eq!(grid["replaced_by"], ".moss-cards[data-layout=grid]");
    assert_eq!(grid["keeps_look"], false);
    assert_eq!(grid["files"], serde_json::json!([THEME_CSS]));
    assert_eq!(json["retired"][1]["replaced_by"], serde_json::Value::Null);
}

#[test]
fn a_folder_without_a_theme_has_nothing_to_migrate() {
    let dir = tempfile::tempdir().unwrap();
    let found = found(dir.path()).unwrap();
    assert!(found.is_empty());
    assert_eq!(to_text(&found), "Nothing to migrate.\n");
}

#[test]
fn the_text_names_the_id_the_release_and_what_replaces_it() {
    let dir = folder_with_theme(".moss-visual { } .moss-subscribe-btn-slot { }");
    assert_eq!(
        to_text(&found(dir.path()).unwrap()),
        "class:moss-visual  .moss/theme/style.css (once), retired in moss 0.7.5: the Visual page layout was removed; nothing replaces it\n\
         class:moss-subscribe-btn-slot  .moss/theme/style.css (once), retired in moss 0.7.11: every button slot shares one class; now .moss-btn-slot\n"
    );
}
