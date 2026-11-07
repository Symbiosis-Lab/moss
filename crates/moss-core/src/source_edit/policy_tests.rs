use super::policy::*;
use std::collections::HashSet;

#[test]
fn free_names_keep_the_extension_and_use_the_first_gap() {
    use std::path::Path;
    for (dir, name, taken, expected) in [
        ("", "a.md", vec![], "a.md"),
        ("posts", "a.md", vec!["elsewhere/a.md"], "a.md"),
        ("posts", "a.md", vec!["posts/a.md", "posts/a-2.md", "posts/a-4.md"], "a-3.md"),
        ("", "notes", vec!["notes", "notes-2"], "notes-3"),
        ("", "archive.tar.gz", vec!["archive.tar.gz"], "archive.tar-2.gz"),
        ("", ".hidden", vec![".hidden"], ".hidden-2"),
        ("", "a-2.md", vec!["a-2.md"], "a-2-2.md"),
        ("", "a.", vec!["a."], "a-2."),
        ("照片", "河灣.jpg", vec!["照片/河灣.jpg"], "河灣-2.jpg"),
    ] {
        let actual = free_name(Path::new(dir), name.as_ref(), |p| taken.iter().any(|t| Path::new(t) == p));
        assert_eq!(actual, expected, "{dir}/{name}");
    }
}
    #[test]
    fn os_junk_hides_at_every_level_regardless_of_the_toggle() {
        for name in [".DS_Store", FINDER_ICON_MARKER, "Thumbs.db"] {
            for parent in ["", "posts"] {
                assert!(is_hidden(name, parent, false), "{name:?} in {parent:?}");
                assert!(is_hidden(name, parent, true), "{name:?} in {parent:?} with internals shown");
            }
        }
    }

    // --- is_hidden: root agent-instruction files ---

    #[test]
    fn root_agent_instruction_files_are_listed_in_both_modes() {
        for name in ["AGENTS.md", "CLAUDE.md", "GEMINI.md"] {
            assert!(!is_hidden(name, "", false), "{name} must be reachable in the default tree");
            assert!(!is_hidden(name, "", true));
        }
    }

    #[test]
    fn moss_lists_only_what_the_user_writes() {
        for name in ["config.toml", "theme", "places.toml"] {
            assert!(!is_hidden(name, ".moss", true), "{name} is the user's to edit");
        }
        for name in ["state.toml", "build.nosync", "cache", "identity", "plugins", "hashes.json", "data", "assets"] {
            assert!(is_hidden(name, ".moss", true), "{name} is moss's own, not the author's");
        }
        // `data/` is gone as a row, so nothing below it can be reached either.
        assert!(is_hidden("social", ".moss/data", true));
        assert!(is_hidden("matters.json", ".moss/data/social", true));
        // Below a listed path, everything shows.
        assert!(!is_hidden("grain.png", ".moss/theme", true));
        assert!(!is_hidden("Inter.woff2", ".moss/theme/fonts", true));
    }

    #[test]
    fn a_nested_agents_md_is_an_ordinary_file() {
        // `posts/AGENTS.md` is an article about agents. The scan publishes it
        // (`left_out_of_site` is root-only for these) and so the tree
        // shows it, in both modes.
        assert!(!is_hidden("AGENTS.md", "posts", false));
        assert!(!is_hidden("AGENTS.md", "posts", true));
    }

    #[test]
    fn a_lowercase_agents_md_at_the_root_stays_visible() {
        // Exact case, matching `is_agent_config_name`: an essay named
        // `Agents.md` is prose, and hiding it would be the worse failure.
        assert!(!is_hidden("Agents.md", "", false));
        assert!(!is_hidden("agents.md", "", false));
    }

    /// `.mdx` reads as markdown in the editor (`file-types.ts` maps it to the
    /// markdown language) but the build has no mdx arm, and `.markdown` is the
    /// mirror image. That asymmetry is why the editor asks for this verdict
    /// instead of testing extensions itself.
    #[test]
    fn page_sources_are_the_scan_arm_not_the_editor_language_set() {
        assert!(is_page_source("md"));
        assert!(is_page_source("markdown"));
        assert!(is_page_source("mdown"));
        assert!(is_page_source("mkd"));
        assert!(!is_page_source("mdx"));
        assert!(!is_page_source("html"));
        assert!(!is_page_source("txt"));
    }

    // --- special_file_kind ---

    fn kind(rel: &str) -> Option<SpecialFileKind> {
        special_file_kind(rel, &HashSet::new())
    }

    #[test]
    fn agent_instructions_are_special_only_at_the_root() {
        for name in ["AGENTS.md", "CLAUDE.md", "GEMINI.md"] {
            assert_eq!(kind(name), Some(SpecialFileKind::AgentInstructions), "{name}");
        }
        assert_eq!(kind("posts/AGENTS.md"), None);
        assert_eq!(kind("Agents.md"), None);
    }

    #[test]
    fn welcome_email_and_footer_sit_at_a_tree_root() {
        assert_eq!(kind("welcome-email.md"), Some(SpecialFileKind::WelcomeEmail));
        assert_eq!(kind("fr/welcome-email.md"), Some(SpecialFileKind::WelcomeEmail));
        assert_eq!(kind("footer.md"), Some(SpecialFileKind::Footer));
        assert_eq!(kind("fr/footer.md"), Some(SpecialFileKind::Footer));
        assert_eq!(kind("docs/footer.md"), None, "a non-language folder's footer.md is an ordinary page");
        assert_eq!(kind("docs/welcome-email.md"), None);
        assert_eq!(kind("footer-blog.md"), None);
    }

    #[test]
    fn a_page_that_is_a_slot_only_by_frontmatter_is_not_the_footer_kind() {
        // Decided by name alone, so a `slot:` declaration cannot make a file this kind.
        assert_eq!(kind("closing.md"), None);
    }

    #[test]
    fn not_found_is_the_root_404_page() {
        assert_eq!(kind("404.md"), Some(SpecialFileKind::NotFound));
        assert_eq!(kind("fr/404.md"), None);
        assert_eq!(kind("docs/404.md"), None);
        assert_eq!(kind("404.png"), None);
        assert_eq!(kind("4040.md"), None);
    }

    #[test]
    fn nothing_inside_moss_is_special() {
        assert_eq!(kind(".moss/footer.md"), None);
        assert_eq!(kind(".moss/theme/404.md"), None);
        let dirs: HashSet<String> = [String::new(), ".moss".to_string()].into();
        assert_eq!(special_file_kind(".moss/index.md", &dirs), None);
    }

    #[test]
    fn page_settings_follow_the_declared_index_html_dirs() {
        let dirs: HashSet<String> = [String::new(), "fr".to_string()].into();
        assert_eq!(special_file_kind("index.md", &dirs), Some(SpecialFileKind::PageSettings));
        assert_eq!(special_file_kind("fr/index.md", &dirs), Some(SpecialFileKind::PageSettings));
        assert_eq!(special_file_kind("about/index.md", &dirs), None);
        assert_eq!(special_file_kind("about.md", &dirs), None);
        assert_eq!(special_file_kind("index.png", &dirs), None);
        assert_eq!(kind("index.md"), None);
    }


    #[test]
    fn test_slot_from_filename_root_footer() {
        assert_eq!(slot_from_filename("footer.md"), Some("footer-left"));
    }

    #[test]
    fn test_slot_from_filename_ignores_subdir_paths() {
        assert_eq!(slot_from_filename("zh-hans/footer.md"), None);
        assert_eq!(slot_from_filename("docs/footer.md"), None);
    }

    #[test]
    fn test_slot_from_filename_ignores_unrelated_files() {
        assert_eq!(slot_from_filename("index.md"), None);
        assert_eq!(slot_from_filename("about.md"), None);
        assert_eq!(slot_from_filename("footer-left.md"), None);
        assert_eq!(slot_from_filename("footer-right.md"), None);
    }


#[test]
fn page_settings_use_existing_exact_files_and_ordered_config() {
    let files = ["index.html", "fr/index.html", "app/index.html"];
    let settings = |config: &[&str]| page_settings_dirs(&config.iter().map(|s| s.to_string()).collect::<Vec<_>>(), &files);
    assert_eq!(settings(&["/index.html/", "fr/index.html", "app/index.html"]), HashSet::from([String::new(), "fr".into()]));
    assert!(settings(&["fr", "app", "missing/index.html"]).is_empty());
    assert!(settings(&["index.html", "!index.html"]).is_empty());
    assert_eq!(settings(&["index.html", "!index.html", "index.html"]), HashSet::from([String::new()]));
}

#[test]
fn passthrough_roots_are_index_folders_then_config_entries_in_order() {
    // (scanned HTML files, config entries, expected roots)
    let cases: &[(&[&str], &[&str], &[&str])] = &[
        // A folder holding index.html is a root; a root-level page is not.
        (&["app/index.html", "about.html"], &[], &["app/"]),
        (&["index.html"], &[], &[]),
        (&[], &["raw-data"], &["raw-data/"]),
        // A listed HTML file named in config is an exact root, without `/`.
        (&["index.html"], &["index.html"], &["index.html"]),
        (&["index.html"], &["index.html", "!index.html"], &[]),
        // A dotted name not among the files stays a folder.
        (&[], &["app.v2"], &["app.v2/"]),
        (&["app/index.html"], &["!app"], &[]),
        // A leading slash in config still names the root-relative folder.
        (&[], &["/app"], &["app/"]),
        // Empty, bare-slash and lone-`!` entries make no root.
        (&["real/index.html"], &["", "/", "!"], &["real/"]),
    ];
    for (html, config, expected) in cases {
        let config: Vec<String> = config.iter().map(|e| e.to_string()).collect();
        let roots = compute_passthrough_roots(html, &config);
        let expected: HashSet<String> = expected.iter().map(|r| r.to_string()).collect();
        assert_eq!(roots, expected, "{html:?} with {config:?}");
    }
}

#[test]
fn in_passthrough_matches_whole_segments_or_the_exact_file() {
    let folders: HashSet<String> = ["app/".to_string(), "app.v2/".to_string()].into();
    assert!(is_in_passthrough("app/image.png", &folders));
    assert!(is_in_passthrough("app/sub/deep.js", &folders));
    assert!(is_in_passthrough("app.v2/index.html", &folders));
    assert!(!is_in_passthrough("other/image.png", &folders));
    assert!(!is_in_passthrough("appended/file.png", &folders), "must not prefix-match across separator");
    let exact: HashSet<String> = ["index.html".to_string()].into();
    assert!(is_in_passthrough("index.html", &exact));
    assert!(!is_in_passthrough("index.html/child", &exact));
    assert!(!is_in_passthrough("index.html.bak", &exact));
}
