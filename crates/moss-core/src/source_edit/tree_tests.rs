use super::tree::*;
    // --- five-zone sort + folder recency ---

    fn node_md(name: &str, date: Option<&str>, source: DateSource, mtime: f64) -> TreeNode {
        TreeNode {
            name: name.to_string(),
            path: name.to_string(),
            is_dir: false,
            modified: Some(mtime),
            publish_date: date.map(|s| s.to_string()),
            date_source: source,
            is_home: false,
            children: None,
            hidden: 0,
            file_id: None,
            special: None,
        }
    }

    fn node_dir(name: &str, mtime: f64, publish_date: Option<&str>) -> TreeNode {
        TreeNode {
            name: name.to_string(),
            path: name.to_string(),
            is_dir: true,
            modified: Some(mtime),
            publish_date: publish_date.map(|s| s.to_string()),
            date_source: DateSource::None,
            is_home: false,
            children: Some(vec![]),
            hidden: 0,
            file_id: None,
            special: None,
        }
    }

    fn node_file(name: &str, mtime: f64) -> TreeNode {
        TreeNode {
            name: name.to_string(),
            path: name.to_string(),
            is_dir: false,
            modified: Some(mtime),
            publish_date: None,
            date_source: DateSource::None,
            is_home: false,
            children: None,
            hidden: 0,
            file_id: None,
            special: None,
        }
    }

    #[test]
    fn five_zone_sort_groups_in_order() {
        let mut nodes = vec![
            node_file("image.png", 100.0),
            node_dir("News", 50.0, Some("2025-11-15")),
            node_md("undated.md", Some("2024-01-01"), DateSource::Ctime, 90.0),
            node_md("dated.md", Some("2025-03-01"), DateSource::Frontmatter, 80.0),
            node_md("index.md", None, DateSource::None, 10.0),
        ];
        sort_tree_nodes(&mut nodes, "Project");
        let names: Vec<&str> = nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["index.md", "dated.md", "undated.md", "News", "image.png"]
        );
    }

    #[test]
    fn dated_md_sorts_newest_first_with_mtime_tiebreak() {
        let mut nodes = vec![
            node_md("apple.md", Some("2025-11-15"), DateSource::Frontmatter, 100.0),
            node_md("zebra.md", Some("2025-11-15"), DateSource::Frontmatter, 200.0),
        ];
        sort_tree_nodes(&mut nodes, "Project");
        let names: Vec<&str> = nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["zebra.md", "apple.md"]);
    }

    #[test]
    fn undated_md_sorts_by_mtime() {
        let mut nodes = vec![
            node_md("a.md", Some("2024-01-01"), DateSource::Ctime, 100.0),
            node_md("b.md", Some("2025-01-01"), DateSource::Ctime, 200.0),
        ];
        sort_tree_nodes(&mut nodes, "Project");
        let names: Vec<&str> = nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["b.md", "a.md"]);
    }

    #[test]
    fn folder_recency_prefers_md_descendant_dates() {
        let folder = TreeNode {
            name: "News".to_string(),
            path: "News".to_string(),
            is_dir: true,
            modified: Some(50.0),
            publish_date: None,
            date_source: DateSource::None,
            is_home: false,
            children: Some(vec![
                node_md("article.md", Some("2025-11-15"), DateSource::Frontmatter, 80.0),
                node_file("photo.png", 999_999_999.0), // very recent image, must NOT win
            ]),
            hidden: 0,
            file_id: None,
            special: None,
        };
        assert_eq!(
            compute_folder_recency(&folder),
            Some("2025-11-15".to_string())
        );
    }

    #[test]
    fn folder_recency_falls_back_to_md_mtime_when_no_dates() {
        let folder = TreeNode {
            name: "Pages".to_string(),
            path: "Pages".to_string(),
            is_dir: true,
            modified: Some(50.0),
            publish_date: None,
            date_source: DateSource::None,
            is_home: false,
            children: Some(vec![
                node_md("a.md", None, DateSource::None, 100.0),
                node_md("b.md", None, DateSource::None, 200.0),
            ]),
            hidden: 0,
            file_id: None,
            special: None,
        };
        assert!(compute_folder_recency(&folder).is_some());
    }

    #[test]
    fn folder_recency_falls_back_to_non_md_mtime_when_no_md() {
        let folder = TreeNode {
            name: "assets".to_string(),
            path: "assets".to_string(),
            is_dir: true,
            modified: Some(50.0),
            publish_date: None,
            date_source: DateSource::None,
            is_home: false,
            children: Some(vec![node_file("photo.png", 200.0)]),
            hidden: 0,
            file_id: None,
            special: None,
        };
        assert!(compute_folder_recency(&folder).is_some());
    }

    #[test]
    fn folder_recency_uses_folder_mtime_when_empty() {
        let folder = node_dir("Empty", 100.0, None);
        assert!(compute_folder_recency(&folder).is_some());
    }


#[test]
fn flat_listing_preserves_visibility_election_dates_and_recency() {
    let entries: Vec<ListingEntry> = serde_json::from_value(serde_json::json!([
        {"path":".secret", "is_dir":false},
        {"path":".moss", "is_dir":true},
        {"path":".moss/cache", "is_dir":true},
        {"path":".moss/config.toml", "is_dir":false},
        {"path":".moss/theme", "is_dir":true},
        {"path":".moss/theme/site.css", "is_dir":false},
        {"path":"notes", "is_dir":true},
        {"path":"notes/.draft.md", "is_dir":false},
        {"path":"notes/Thumbs.db", "is_dir":false},
        {"path":"notes/dated.md", "is_dir":false, "frontmatter":"date: 2025-12-01\n---\nBody"},
        {"path":"photo.png", "is_dir":false, "modified":2000000000.0},
        {"path":"index.md", "is_dir":false},
        {"path":"chosen.md", "is_dir":false, "frontmatter":"---\nhome: true\n---\nBody"},
        {"path":"2025-01-01-post.md", "is_dir":false},
        {"path":"undated.md", "is_dir":false, "modified":100.0},
        {"path":"node_modules", "is_dir":true},
        {"path":"target", "is_dir":true}
    ])).unwrap();
    let options = TreeOptions { root_name: "Site".into(), ..Default::default() };
    let forest = build_tree(&entries, &options);
    assert_eq!(forest.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["chosen.md", "index.md", "2025-01-01-post.md", "undated.md", "notes", "photo.png"]);
    let notes = forest.iter().find(|n| n.name == "notes").unwrap();
    // Desktop historically hides root dotfiles, but retains nested dotfiles.
    assert!(notes.children.as_ref().unwrap().iter().any(|n| n.name == ".draft.md"));
    assert_eq!(notes.children.as_ref().unwrap().len(), 2);
    assert_eq!(notes.publish_date.as_deref(), Some("2025-12-01"));
    let with_internal = build_tree(&entries, &TreeOptions { show_internal: true, ..options });
    let moss = with_internal.iter().find(|n| n.name == ".moss").unwrap();
    assert_eq!(moss.hidden, 1);
    assert_eq!(moss.children.as_ref().unwrap().len(), 2);
}

#[test]
fn frontmatter_prefix_ignores_late_closers_and_split_utf8() {
    let header = "---\ndate: 2099-01-01\nhome: true\npadding: ";
    let long = format!("{header}{}\n---\nBody", "x".repeat(FRONTMATTER_PREFIX_BYTES));
    let split = format!("{header}{}河\n---\nBody", "x".repeat(FRONTMATTER_PREFIX_BYTES - header.len() - 1));
    assert!(!split.is_char_boundary(FRONTMATTER_PREFIX_BYTES));
    for text in [long, split] {
        let full = crate::frontmatter::frontmatter_map(&text);
        assert_eq!(full["date"].as_str(), Some("2099-01-01"));
        assert_eq!(full["home"].as_bool(), Some(true));
        let prefix = &text.as_bytes()[..FRONTMATTER_PREFIX_BYTES];
        assert!(frontmatter_from_prefix(text.as_bytes()).is_empty());
        assert_eq!(frontmatter_from_prefix(text.as_bytes()), frontmatter_from_prefix(prefix));
    }
    let short = b"---\ndate: 2099-01-01\nhome: true\n---\nBody";
    assert_eq!(frontmatter_from_prefix(short)["home"].as_bool(), Some(true));
}

#[test]
fn an_mtime_is_the_utc_day_it_falls_in_within_the_date_range() {
    assert_eq!(mtime_to_yyyy_mm_dd(0.0).as_deref(), Some("1970-01-01"));
    assert_eq!(mtime_to_yyyy_mm_dd(86_399.9).as_deref(), Some("1970-01-01"));
    assert_eq!(mtime_to_yyyy_mm_dd(86_400.0).as_deref(), Some("1970-01-02"));
    assert_eq!(mtime_to_yyyy_mm_dd(951_782_400.0).as_deref(), Some("2000-02-29"));
    // Negative and NaN inputs saturate to the epoch.
    assert_eq!(mtime_to_yyyy_mm_dd(-5.0).as_deref(), Some("1970-01-01"));
    assert_eq!(mtime_to_yyyy_mm_dd(f64::NAN).as_deref(), Some("1970-01-01"));
    assert_eq!(mtime_to_yyyy_mm_dd(253_402_300_800.0).as_deref(), Some("+10000-01-01"));
    assert_eq!(mtime_to_yyyy_mm_dd(8_210_266_876_799.0).as_deref(), Some("+262142-12-31"));
    assert_eq!(mtime_to_yyyy_mm_dd(8_210_266_876_800.0), None);
    assert_eq!(mtime_to_yyyy_mm_dd(f64::INFINITY), None);
}
