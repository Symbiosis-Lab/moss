use super::*;
use crate::types::content::FileInfo;

/// Tests for compute_home_file_winners
mod home_file_winner_tests {
    use super::*;

    fn make_file(path: &str) -> FileInfo {
        FileInfo {
            path: path.to_string(),
            file_type: "md".to_string(),
            size: 100,
            modified: None,
        }
    }

    #[test]
    fn test_root_index_md_wins_over_self_named() {
        // Root with index.md + 刘果.md, root_folder_name "刘果"
        // Winner should be index.md (index stems beat self-named)
        let files = vec![make_file("index.md"), make_file("刘果.md")];
        let winners = compute_home_file_winners(&files, "刘果", &std::collections::HashMap::new());
        assert!(
            winners.contains("index.md"),
            "index.md should be the winner"
        );
        assert!(
            !winners.contains("刘果.md"),
            "刘果.md should NOT be the winner"
        );
        assert_eq!(winners.len(), 1);
    }

    #[test]
    fn test_subfolder_index_wins_over_self_named() {
        // Subfolder recipes/ with index.md + recipes.md
        // Winner should be recipes/index.md
        let files = vec![
            make_file("recipes/index.md"),
            make_file("recipes/recipes.md"),
        ];
        let winners =
            compute_home_file_winners(&files, "mysite", &std::collections::HashMap::new());
        assert!(
            winners.contains("recipes/index.md"),
            "recipes/index.md should be the winner"
        );
        assert!(
            !winners.contains("recipes/recipes.md"),
            "recipes/recipes.md should NOT be the winner"
        );
    }

    #[test]
    fn test_self_named_wins_when_only_candidate() {
        // Root with ONLY 刘果.md, root_folder_name "刘果"
        // Winner should be 刘果.md (self-named, no index.md)
        let files = vec![make_file("刘果.md"), make_file("about.md")];
        let winners = compute_home_file_winners(&files, "刘果", &std::collections::HashMap::new());
        assert!(
            winners.contains("刘果.md"),
            "刘果.md should be the winner when no index.md exists"
        );
    }

    #[test]
    fn test_no_conflict_just_index() {
        // Root with just index.md → winner is index.md
        let files = vec![make_file("index.md"), make_file("about.md")];
        let winners =
            compute_home_file_winners(&files, "mysite", &std::collections::HashMap::new());
        assert!(
            winners.contains("index.md"),
            "index.md should be the winner"
        );
        assert_eq!(winners.len(), 1);
    }

    #[test]
    fn test_multiple_folders_each_get_winner() {
        // Root and subfolder each have their own winner
        let files = vec![
            make_file("index.md"),
            make_file("about.md"),
            make_file("recipes/index.md"),
            make_file("recipes/pasta.md"),
        ];
        let winners =
            compute_home_file_winners(&files, "mysite", &std::collections::HashMap::new());
        assert!(winners.contains("index.md"));
        assert!(winners.contains("recipes/index.md"));
        assert_eq!(winners.len(), 2);
    }
}

/// Tests for `build_page_map()` — the pre-scan that computes source_path → url_path mappings.
mod build_page_map_tests {
    use super::*;
    use crate::vault::paths::VaultRoot;
    use std::collections::HashSet;
    use std::fs;

    /// Test shorthand: adopt a fixture dir as the vault root. Production code
    /// gets its `VaultRoot` threaded down from the entry point.
    fn vroot(p: impl AsRef<Path>) -> VaultRoot {
        VaultRoot::resolve(p)
    }

    fn make_file(path: &str) -> FileInfo {
        FileInfo {
            path: path.to_string(),
            file_type: "md".to_string(),
            size: 0,
            modified: None,
        }
    }

    /// `blocking.rs` canonicalized `root_folder_name` while `compute_home_overrides`,
    /// eight lines away and reading the SAME `source_path`, used `.unwrap_or("")`.
    /// Their outputs are consumed TOGETHER (name-anchors vs marker/translation
    /// anchors), so for a dotted root the "a file is its own folder's home" rule fired
    /// in one map and not the other and rule 4's translation-group inheritance
    /// mis-elected. A half-canonicalized pair is worse than a uniformly-broken one: it
    /// produces a state neither function was tested for. Both now read the ONE name on
    /// `VaultRoot`, so a dot-path root cannot disagree with its absolute spelling.
    #[test]
    fn home_overrides_and_winners_agree_on_the_root_name_for_a_dot_path() {
        let tmp = setup_temp_dir(&[]);
        let site = tmp.path().join("潮汐");
        fs::create_dir_all(site.join("en")).unwrap();
        fs::write(
            site.join("潮汐.md"),
            "---\ntranslationKey: x\n---\n# 潮汐\n",
        )
        .unwrap();
        fs::write(
            site.join("en/潮汐.md"),
            "---\ntranslationKey: x\nlang: en\n---\n# Tide\n",
        )
        .unwrap();

        let dotted = VaultRoot::resolve_in(Path::new("."), &site);
        let absolute = VaultRoot::resolve(&site);
        assert_eq!(dotted.name(), "潮汐", "a `.` root must still know its name");
        assert_eq!(dotted.name(), absolute.name());

        let files = vec![make_file("潮汐.md"), make_file("en/潮汐.md")];
        let dotted_overrides = compute_home_overrides(&files, &dotted);
        assert_eq!(dotted_overrides, compute_home_overrides(&files, &absolute));

        // And the winners map — the other half of the pair — agrees too.
        assert_eq!(
            compute_home_file_winners(&files, dotted.name(), &dotted_overrides),
            compute_home_file_winners(&files, absolute.name(), &dotted_overrides),
        );
        // The self-named root file IS the root home: a `""` root name would have
        // demoted it off `/`.
        assert!(
            compute_home_file_winners(&files, dotted.name(), &dotted_overrides).contains("潮汐.md")
        );
    }

    /// Create a temp directory with markdown files and return the path.
    fn setup_temp_dir(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        for (path, content) in files {
            let full = dir.path().join(path);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent).expect("failed to create dirs");
            }
            fs::write(full, content).expect("failed to write file");
        }
        dir
    }

    #[test]
    fn basic_file_mapping() {
        let dir = setup_temp_dir(&[("about.md", "# About"), ("index.md", "# Home")]);
        let files = vec![make_file("about.md"), make_file("index.md")];
        let winners =
            compute_home_file_winners(&files, "mysite", &std::collections::HashMap::new());
        let (map, _overrides) = build_page_map(
            &files,
            dir.path(),
            "mysite",
            &winners,
            &std::collections::HashMap::new(),
        );

        assert_eq!(map.get("about.md").unwrap(), "about/index.html");
        assert_eq!(map.get("index.md").unwrap(), "index.html");
    }

    #[test]
    fn subfolder_index_file() {
        let dir = setup_temp_dir(&[("posts/index.md", "# Posts"), ("posts/hello.md", "# Hello")]);
        let files = vec![make_file("posts/index.md"), make_file("posts/hello.md")];
        let winners =
            compute_home_file_winners(&files, "mysite", &std::collections::HashMap::new());
        let (map, _overrides) = build_page_map(
            &files,
            dir.path(),
            "mysite",
            &winners,
            &std::collections::HashMap::new(),
        );

        assert_eq!(map.get("posts/index.md").unwrap(), "posts/index.html");
        assert_eq!(map.get("posts/hello.md").unwrap(), "posts/hello/index.html");
    }

    #[test]
    fn url_override_cascades_to_children() {
        let dir = setup_temp_dir(&[
            ("posts/index.md", "---\nurl: blog\n---\n# Posts"),
            ("posts/hello.md", "# Hello"),
        ]);
        let files = vec![make_file("posts/index.md"), make_file("posts/hello.md")];
        let winners =
            compute_home_file_winners(&files, "mysite", &std::collections::HashMap::new());
        let (map, _overrides) = build_page_map(
            &files,
            dir.path(),
            "mysite",
            &winners,
            &std::collections::HashMap::new(),
        );

        // Index gets the override directly
        assert_eq!(map.get("posts/index.md").unwrap(), "blog/index.html");
        // Child page should cascade: posts/ → blog/
        assert_eq!(map.get("posts/hello.md").unwrap(), "blog/hello/index.html");
    }

    #[test]
    fn title_case_folder_lowercases_url() {
        // Regression: Title-Case folder names must produce lowercase URLs.
        // The folder file "News/News.md" becomes /news/, and a child file
        // "News/2026-04-22.md" becomes /news/2026-04-22/.
        let dir = setup_temp_dir(&[
            ("News/News.md", "# News"),
            ("News/2026-04-22.md", "# Lecture"),
        ]);
        let files = vec![make_file("News/News.md"), make_file("News/2026-04-22.md")];
        let winners =
            compute_home_file_winners(&files, "mysite", &std::collections::HashMap::new());
        let (map, _overrides) = build_page_map(
            &files,
            dir.path(),
            "mysite",
            &winners,
            &std::collections::HashMap::new(),
        );

        assert_eq!(map.get("News/News.md").unwrap(), "news/index.html");
        assert_eq!(
            map.get("News/2026-04-22.md").unwrap(),
            "news/2026-04-22/index.html"
        );
    }

    #[test]
    fn title_case_sibling_unaffected_by_other_folder_override() {
        // Regression: when one folder has a `url:` override (so
        // `dir_overrides` is non-empty), Phase 2 of build_page_map runs
        // for every entry. A *sibling* Title-Case folder with no override
        // must produce clean lowercase URLs — no leakage of the original
        // case from the verbatim-last-segment heuristic in
        // resolve_path_with_overrides.
        let dir = setup_temp_dir(&[
            ("Posts/Posts.md", "---\nurl: blog\n---\n# Posts"),
            ("Posts/hello.md", "# Hello"),
            ("My Section/My Section.md", "# Section"),
            ("My Section/Sub.md", "# Sub"),
        ]);
        let files = vec![
            make_file("Posts/Posts.md"),
            make_file("Posts/hello.md"),
            make_file("My Section/My Section.md"),
            make_file("My Section/Sub.md"),
        ];
        let winners =
            compute_home_file_winners(&files, "mysite", &std::collections::HashMap::new());
        let (map, _overrides) = build_page_map(
            &files,
            dir.path(),
            "mysite",
            &winners,
            &std::collections::HashMap::new(),
        );

        // Posts/ folder uses the url: override → /blog/.
        assert_eq!(map.get("Posts/Posts.md").unwrap(), "blog/index.html");
        assert_eq!(map.get("Posts/hello.md").unwrap(), "blog/hello/index.html");

        // Sibling Title-Case folder with no override → straight slugify.
        // No leakage from Phase 2 firing on the override-empty path.
        assert_eq!(
            map.get("My Section/My Section.md").unwrap(),
            "my-section/index.html"
        );
        assert_eq!(
            map.get("My Section/Sub.md").unwrap(),
            "my-section/sub/index.html"
        );
    }

    #[test]
    fn title_case_folder_with_url_override_cascades() {
        // Regression for the Phase 2 cascade with mixed-case folders:
        // "News/" with `url: blog` on the index must (a) emit /blog/ for
        // the index and (b) cascade so children become /blog/<slug>/
        // — not /News/<slug>/ or a silently-broken passthrough.
        let dir = setup_temp_dir(&[
            ("News/News.md", "---\nurl: blog\n---\n# News"),
            ("News/hello.md", "# Hello"),
        ]);
        let files = vec![make_file("News/News.md"), make_file("News/hello.md")];
        let winners =
            compute_home_file_winners(&files, "mysite", &std::collections::HashMap::new());
        let (map, _overrides) = build_page_map(
            &files,
            dir.path(),
            "mysite",
            &winners,
            &std::collections::HashMap::new(),
        );

        assert_eq!(map.get("News/News.md").unwrap(), "blog/index.html");
        assert_eq!(map.get("News/hello.md").unwrap(), "blog/hello/index.html");
    }

    #[test]
    fn nested_url_overrides_cascade_to_leaf() {
        // a/index.md url: alpha → alpha/index.html (from compute_url_path, own segment)
        // a/b/index.md url: beta → alpha/beta/index.html (own segment from compute_url_path,
        //   ancestor "a" cascaded to "alpha" in Phase 2)
        // a/b/leaf.md → cascaded: a → alpha, b → beta → alpha/beta/leaf/index.html
        let dir = setup_temp_dir(&[
            ("a/index.md", "---\nurl: alpha\n---\n# A"),
            ("a/b/index.md", "---\nurl: beta\n---\n# B"),
            ("a/b/leaf.md", "# Leaf"),
        ]);
        let files = vec![
            make_file("a/index.md"),
            make_file("a/b/index.md"),
            make_file("a/b/leaf.md"),
        ];
        let winners =
            compute_home_file_winners(&files, "mysite", &std::collections::HashMap::new());
        let (map, _overrides) = build_page_map(
            &files,
            dir.path(),
            "mysite",
            &winners,
            &std::collections::HashMap::new(),
        );

        assert_eq!(map.get("a/index.md").unwrap(), "alpha/index.html");
        // Index file gets ancestor cascade: "a" → "alpha"
        assert_eq!(map.get("a/b/index.md").unwrap(), "alpha/beta/index.html");
        // Leaf gets both overrides cascaded
        assert_eq!(
            map.get("a/b/leaf.md").unwrap(),
            "alpha/beta/leaf/index.html"
        );
    }

    #[test]
    fn self_named_folder_note_as_index() {
        let dir = setup_temp_dir(&[
            ("recipes/recipes.md", "# Recipes"),
            ("recipes/pasta.md", "# Pasta"),
        ]);
        let files = vec![
            make_file("recipes/recipes.md"),
            make_file("recipes/pasta.md"),
        ];
        let winners =
            compute_home_file_winners(&files, "mysite", &std::collections::HashMap::new());
        let (map, _overrides) = build_page_map(
            &files,
            dir.path(),
            "mysite",
            &winners,
            &std::collections::HashMap::new(),
        );

        assert_eq!(map.get("recipes/recipes.md").unwrap(), "recipes/index.html");
        assert_eq!(
            map.get("recipes/pasta.md").unwrap(),
            "recipes/pasta/index.html"
        );
    }

    #[test]
    fn regular_file_with_url_override() {
        let dir = setup_temp_dir(&[("posts/hello.md", "---\nurl: greeting\n---\n# Hello")]);
        let files = vec![make_file("posts/hello.md")];
        let winners =
            compute_home_file_winners(&files, "mysite", &std::collections::HashMap::new());
        let (map, _overrides) = build_page_map(
            &files,
            dir.path(),
            "mysite",
            &winners,
            &std::collections::HashMap::new(),
        );

        assert_eq!(
            map.get("posts/hello.md").unwrap(),
            "posts/greeting/index.html"
        );
    }

    /// Pre-scan stem stripping is independent of ancestor folder lang.
    /// `en/post.zh-hans.md` → stem is `post`, regardless of the `en/`
    /// ancestor. The filename suffix wins for stem-stripping purposes;
    /// the ancestor lang only matters for the full-pipeline lang
    /// resolution downstream, not for URL-slug computation here.
    #[test]
    fn lang_suffix_strips_independently_of_ancestor_folder() {
        let dir = setup_temp_dir(&[("en/post.zh-hans.md", "# Some content")]);
        let files = vec![make_file("en/post.zh-hans.md")];
        let winners =
            compute_home_file_winners(&files, "mysite", &std::collections::HashMap::new());
        let (map, _overrides) = build_page_map(
            &files,
            dir.path(),
            "mysite",
            &winners,
            &std::collections::HashMap::new(),
        );

        // The `.zh-hans` suffix is stripped, leaving `post` as the slug.
        // The `en/` ancestor doesn't affect the URL here — the slug
        // resolver in `compute_url_path` keeps the original directory
        // structure; lang-prefix routing happens later in slug.rs.
        assert_eq!(
            map.get("en/post.zh-hans.md").unwrap(),
            "en/post/index.html",
            "lang-suffix should be stripped from the slug regardless of ancestor folder lang"
        );
    }

    /// The `home: true` marker promotes a non-INDEX_STEM, non-self-named
    /// file to be the folder home — issue #587. `en/Liu Guo.md` should
    /// land at `en/index.html`, not at the slug-based
    /// `en/liu-guo/index.html`.
    #[test]
    fn home_marker_promotes_to_folder_index() {
        let dir = setup_temp_dir(&[
            ("刘果.md", "---\nhome: true\n---\n# 刘果\n"),
            (
                "en/Liu Guo.md",
                "---\nhome: true\nlang: en\n---\n# Liu Guo\n",
            ),
            ("en/about.md", "# About\n"),
        ]);
        let files = vec![
            make_file("刘果.md"),
            make_file("en/Liu Guo.md"),
            make_file("en/about.md"),
        ];
        let overrides = compute_home_overrides(&files, &vroot(dir.path()));
        let winners = compute_home_file_winners(&files, "刘果", &overrides);
        let (map, _) = build_page_map(&files, dir.path(), "刘果", &winners, &overrides);

        // The promoted home gets the folder's index URL.
        assert_eq!(
            map.get("en/Liu Guo.md").unwrap(),
            "en/index.html",
            "home: true should promote en/Liu Guo.md to en/index.html"
        );
        // Other files in the folder keep their slug URLs.
        assert_eq!(map.get("en/about.md").unwrap(), "en/about/index.html");
        // The root home is still index.html.
        assert_eq!(map.get("刘果.md").unwrap(), "index.html");
    }

    /// Without the `home: true` marker, the previous (filename-only)
    /// behavior is preserved: a single file in `en/` falls through to
    /// `detect_home_file_in_folder`'s priority-5 alphabetical tiebreaker
    /// and *does* become the home — but with a slug URL since
    /// `is_home_file` (filename-based) returns false. Same as before.
    #[test]
    fn no_home_marker_falls_back_to_filename_detection() {
        let dir = setup_temp_dir(&[
            ("en/Liu Guo.md", "# Liu Guo\n"), // no frontmatter
        ]);
        let files = vec![make_file("en/Liu Guo.md")];
        let overrides = compute_home_overrides(&files, &vroot(dir.path()));
        let winners = compute_home_file_winners(&files, "site", &overrides);
        let (map, _) = build_page_map(&files, dir.path(), "site", &winners, &overrides);

        // Without the home marker, falls back to slug-based URL.
        // (This is the issue-#587 behavior; user opt-in via
        // home: true is required to promote.)
        assert_eq!(map.get("en/Liu Guo.md").unwrap(), "en/liu-guo/index.html");
    }

    /// Promotion is decoupled from the `translationKey` *value*: a keyed
    /// group with NO anchor (no `home: true`, no self-named, no index-stem)
    /// does NOT promote any member. `translationKey` value never means
    /// "home" — including the literal value `"home"`.
    #[test]
    fn translation_key_alone_does_not_promote() {
        let dir = setup_temp_dir(&[
            // translationKey: home — the OLD trigger — must NOT promote
            // because no member of the group is an anchor.
            (
                "en/Liu Guo.md",
                "---\ntranslationKey: home\nlang: en\n---\n# Liu Guo\n",
            ),
            // A different folder shares the key but is not an anchor either.
            (
                "fr/Liu Guo.md",
                "---\ntranslationKey: home\nlang: fr\n---\n# Liu Guo\n",
            ),
            // An arbitrary translationKey value with no anchor must NOT promote.
            ("en/post.md", "---\ntranslationKey: my-post\n---\n# Post\n"),
        ]);
        let files = vec![
            make_file("en/Liu Guo.md"),
            make_file("fr/Liu Guo.md"),
            make_file("en/post.md"),
        ];
        let overrides = compute_home_overrides(&files, &vroot(dir.path()));
        assert!(
                overrides.is_empty(),
                "a keyed group with NO anchor must not promote — the translationKey value (incl. `home`) never means home"
            );
    }

    /// A keyed group whose members live in two folders, neither of which
    /// has an anchor (no `home:true`, not self-named, not index-stem), gets
    /// NO promotion — they are just translations of each other.
    #[test]
    fn keyed_group_with_no_anchor_no_promotion() {
        let dir = setup_temp_dir(&[
            ("a/post.md", "---\ntranslationKey: y\n---\n# Post\n"),
            ("b/post.md", "---\ntranslationKey: y\n---\n# Post\n"),
        ]);
        let files = vec![make_file("a/post.md"), make_file("b/post.md")];
        let overrides = compute_home_overrides(&files, &vroot(dir.path()));
        assert!(
            overrides.is_empty(),
            "a keyed group with no anchor elects no home"
        );
    }

    /// Promotion-by-relationship (the 刘果 case, NO marker): a root file
    /// that is self-named (`刘果.md` with project root basename `刘果`) is an
    /// anchor by name alone. It shares a `translationKey` with `en/Liu Guo.md`
    /// which is NOT self-named and has NO `home` marker. The shared key
    /// propagates home-ness: `en/Liu Guo.md` becomes `en/`'s home.
    #[test]
    fn promotion_by_relationship_self_named_anchor() {
        let dir = setup_temp_dir(&[
            // Self-named root file → anchor (root basename derived from the
            // temp dir's own name, so we pass that as root_folder_name and
            // also exercise is_home_file against source_path.file_name()).
            ("刘果.md", "---\ntranslationKey: x\n---\n# 刘果\n"),
            (
                "en/Liu Guo.md",
                "---\ntranslationKey: x\nlang: en\n---\n# Liu Guo\n",
            ),
            ("en/about.md", "# About\n"),
        ]);
        // The temp dir's basename is random, so 刘果.md is NOT self-named
        // against it. Re-create with a controlled root basename by nesting:
        // build a child folder whose name is 刘果 and treat it as the root.
        let root = dir.path().join("刘果");
        std::fs::create_dir_all(root.join("en")).unwrap();
        std::fs::write(
            root.join("刘果.md"),
            "---\ntranslationKey: x\n---\n# 刘果\n",
        )
        .unwrap();
        std::fs::write(
            root.join("en/Liu Guo.md"),
            "---\ntranslationKey: x\nlang: en\n---\n# Liu Guo\n",
        )
        .unwrap();
        std::fs::write(root.join("en/about.md"), "# About\n").unwrap();

        let files = vec![
            make_file("刘果.md"),
            make_file("en/Liu Guo.md"),
            make_file("en/about.md"),
        ];
        let overrides = compute_home_overrides(&files, &vroot(&root));

        // The root self-named file is a NAME-based anchor — it is NOT in
        // the override map (name anchors are resolved from filenames by
        // compute_home_file_winners). The override map only carries the
        // inherited (and direct home:true) promotions.
        assert_eq!(
            overrides.get(""),
            None,
            "a name-based anchor is not added to the override map"
        );
        // en/Liu Guo.md inherits home-ness via the shared key.
        assert_eq!(
            overrides.get("en"),
            Some(&"en/Liu Guo.md".to_string()),
            "translation member of an anchor group becomes its own folder's home"
        );

        // End-to-end through the page map: en/Liu Guo.md → en/index.html.
        let winners = compute_home_file_winners(&files, "刘果", &overrides);
        let (map, _) = build_page_map(&files, &root, "刘果", &winners, &overrides);
        assert_eq!(map.get("en/Liu Guo.md").unwrap(), "en/index.html");
        assert_eq!(map.get("en/about.md").unwrap(), "en/about/index.html");
        assert_eq!(map.get("刘果.md").unwrap(), "index.html");
    }

    /// The translationKey VALUE is irrelevant — any value works the same.
    /// Same as above but with `home:true` as the anchor signal and a
    /// nonsense key value to prove value-irrelevance.
    #[test]
    fn promotion_by_relationship_value_irrelevant() {
        let dir = setup_temp_dir(&[
            (
                "home.md",
                "---\nhome: true\ntranslationKey: arbitrary-zzz\n---\n# Home\n",
            ),
            (
                "en/Welcome.md",
                "---\ntranslationKey: arbitrary-zzz\nlang: en\n---\n# Welcome\n",
            ),
        ]);
        let files = vec![make_file("home.md"), make_file("en/Welcome.md")];
        let overrides = compute_home_overrides(&files, &vroot(dir.path()));
        // Direct anchor at root.
        assert_eq!(overrides.get(""), Some(&"home.md".to_string()));
        // Inherited promotion in en/ via the (arbitrary-valued) key.
        assert_eq!(
            overrides.get("en"),
            Some(&"en/Welcome.md".to_string()),
            "the translationKey value is irrelevant; any shared value propagates home-ness"
        );
    }

    /// Direct `home: true` on a non-self-named file still promotes
    /// (regression for the shipped behavior).
    #[test]
    fn direct_home_marker_still_promotes() {
        let dir = setup_temp_dir(&[
            ("notes/notes-page.md", "---\nhome: true\n---\n# Notes\n"),
            ("notes/other.md", "# Other\n"),
        ]);
        let files = vec![
            make_file("notes/notes-page.md"),
            make_file("notes/other.md"),
        ];
        let overrides = compute_home_overrides(&files, &vroot(dir.path()));
        assert_eq!(
            overrides.get("notes"),
            Some(&"notes/notes-page.md".to_string()),
            "home: true on a non-self-named file still promotes it"
        );
    }

    /// A folder's OWN anchor (name-based here) always beats an inherited
    /// candidate. `en/` has `en/index.md` (name-based anchor) AND
    /// `en/foo.md` sharing a key with a root anchor — `en/`'s home stays
    /// `index.md` (no inherited override clobbers it).
    #[test]
    fn own_anchor_beats_inherited() {
        let dir = setup_temp_dir(&[]);
        let root = dir.path().join("刘果");
        std::fs::create_dir_all(root.join("en")).unwrap();
        // Root self-named anchor sharing key `k`.
        std::fs::write(
            root.join("刘果.md"),
            "---\ntranslationKey: k\n---\n# 刘果\n",
        )
        .unwrap();
        // en/ has its OWN name-based anchor (index.md).
        std::fs::write(root.join("en/index.md"), "# En index\n").unwrap();
        // en/foo.md shares the key with the root anchor → would inherit, but
        // en/ already has its own anchor, so this must NOT win.
        std::fs::write(
            root.join("en/foo.md"),
            "---\ntranslationKey: k\nlang: en\n---\n# Foo\n",
        )
        .unwrap();

        let files = vec![
            make_file("刘果.md"),
            make_file("en/index.md"),
            make_file("en/foo.md"),
        ];
        let overrides = compute_home_overrides(&files, &vroot(&root));

        // en/'s home is NOT promoted via override to foo.md — index.md is its
        // own anchor (recognized by compute_home_file_winners, not the
        // override map). The override map must NOT contain an en/ → foo entry.
        assert_ne!(
            overrides.get("en"),
            Some(&"en/foo.md".to_string()),
            "a folder's own name-based anchor beats an inherited candidate"
        );

        // End-to-end: en/ home stays index.md.
        let winners = compute_home_file_winners(&files, "刘果", &overrides);
        assert!(
            winners.contains("en/index.md"),
            "en/index.md remains the home"
        );
        assert!(!winners.contains("en/foo.md"), "en/foo.md is NOT the home");
    }

    /// Inheritance lands in the member's OWN (containing) folder, even deep
    /// in the tree: `en/sub/Deep.md` sharing the anchor's key is the home of
    /// `en/sub/`, not `en/`. (The override map keys on each member's parent dir.)
    #[test]
    fn inherited_home_lands_in_containing_folder() {
        let dir = setup_temp_dir(&[
            (
                "home.md",
                "---\nhome: true\ntranslationKey: k\n---\n# Home\n",
            ),
            (
                "en/sub/Deep.md",
                "---\ntranslationKey: k\nlang: en\n---\n# Deep\n",
            ),
        ]);
        let files = vec![make_file("home.md"), make_file("en/sub/Deep.md")];
        let overrides = compute_home_overrides(&files, &vroot(dir.path()));
        assert_eq!(
            overrides.get("en/sub"),
            Some(&"en/sub/Deep.md".to_string()),
            "a deep translation member is the home of its OWN folder (en/sub)"
        );
        assert_eq!(
            overrides.get("en"),
            None,
            "en/ has no member of the group, so it gets no inherited home"
        );
    }
}

// =========================================================================
// external_url validation tests (moss#684)
//
// These tests cover `is_valid_external_url` (pure predicate) and the
// downstream `external_url()` accessor which must return None for anything
// that isn't an absolute http(s) URL.
// =========================================================================

mod external_url_validation_tests {
    use super::*;
    use std::collections::HashMap;

    // --- is_valid_external_url -------------------------------------------

    #[test]
    fn https_url_is_valid() {
        assert!(is_valid_external_url("https://example.com/post"));
    }

    #[test]
    fn http_url_is_valid() {
        assert!(is_valid_external_url("http://example.com/post"));
    }

    #[test]
    fn relative_path_is_invalid() {
        assert!(!is_valid_external_url("relative/path"));
    }

    #[test]
    fn relative_path_with_leading_slash_is_invalid() {
        assert!(!is_valid_external_url("/absolute/path"));
    }

    #[test]
    fn ftp_url_is_invalid() {
        assert!(!is_valid_external_url("ftp://files.example.com/data"));
    }

    #[test]
    fn javascript_url_is_invalid() {
        assert!(!is_valid_external_url("javascript:void(0)"));
    }

    #[test]
    fn data_url_is_invalid() {
        assert!(!is_valid_external_url("data:text/html,<b>x</b>"));
    }

    #[test]
    fn empty_string_is_invalid() {
        assert!(!is_valid_external_url(""));
    }

    // --- external_url() accessor -----------------------------------------

    fn make_fm(key: &str, val: &str) -> std::collections::BTreeMap<String, serde_json::Value> {
        let mut m = std::collections::BTreeMap::new();
        m.insert(key.to_string(), serde_json::Value::String(val.to_string()));
        m
    }

    #[test]
    fn accessor_returns_https_url() {
        let fm = make_fm("external_url", "https://example.com/post");
        assert_eq!(
            external_url(&fm),
            Some("https://example.com/post".to_string())
        );
    }

    #[test]
    fn accessor_returns_none_for_relative_path() {
        let fm = make_fm("external_url", "relative/path");
        assert_eq!(external_url(&fm), None);
    }

    #[test]
    fn accessor_returns_none_for_ftp() {
        let fm = make_fm("external_url", "ftp://files.example.com/data");
        assert_eq!(external_url(&fm), None);
    }

    #[test]
    fn accessor_returns_none_when_field_absent() {
        let fm: std::collections::BTreeMap<String, serde_json::Value> = std::collections::BTreeMap::new();
        assert_eq!(external_url(&fm), None);
    }

    // --- build_external_url_map() ----------------------------------------

    fn make_file_info(path: &str) -> crate::types::content::FileInfo {
        crate::types::content::FileInfo {
            path: path.to_string(),
            file_type: "md".to_string(),
            size: 0,
            modified: None,
        }
    }

    fn setup_temp_dir(files: &[(&str, &str)]) -> tempfile::TempDir {
        use std::fs;
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        for (path, content) in files {
            let full = dir.path().join(path);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent).expect("failed to create dirs");
            }
            fs::write(full, content).expect("failed to write file");
        }
        dir
    }

    #[test]
    fn map_includes_https_url() {
        let dir = setup_temp_dir(&[(
            "post.md",
            "---\nexternal_url: \"https://example.com/post\"\n---\n# Post\n",
        )]);
        let files = vec![make_file_info("post.md")];
        let map = build_external_url_map(&files, dir.path());
        assert_eq!(
            map.get("post.md"),
            Some(&"https://example.com/post".to_string())
        );
    }

    #[test]
    fn map_excludes_relative_path_and_does_not_panic() {
        let dir = setup_temp_dir(&[(
            "post.md",
            "---\nexternal_url: \"relative/path\"\n---\n# Post\n",
        )]);
        let files = vec![make_file_info("post.md")];
        let map = build_external_url_map(&files, dir.path());
        // Invalid URL must be silently dropped from the map (warning is
        // logged but not assertable in unit tests without log capture).
        assert!(
            map.get("post.md").is_none(),
            "relative external_url must not appear in the map"
        );
    }

    #[test]
    fn map_excludes_ftp_url() {
        let dir = setup_temp_dir(&[(
            "post.md",
            "---\nexternal_url: \"ftp://files.example.com/data\"\n---\n# Post\n",
        )]);
        let files = vec![make_file_info("post.md")];
        let map = build_external_url_map(&files, dir.path());
        assert!(
            map.get("post.md").is_none(),
            "ftp:// external_url must not appear in the map"
        );
    }
}

// =========================================================================
// Video Path Mapping Tests (file-tree → page-tree)
//
// These tests verify that placeholder SVGs and AssetRegistry keys use
// mapped (page-tree) paths via resolve_path_with_overrides(), not raw
// filesystem paths. This ensures SVGs land at page-tree locations
// matching the HTML references generated during the blocking phase.
// =========================================================================

mod video_path_mapping_tests {
    use super::*;
    use std::collections::HashMap;

    // test_placeholder_svg_uses_mapped_path was removed in #615
    // (generate_svg_placeholder deleted — Pattern E removed).

    /// AssetRegistry keys must match HTML <video src> paths (page-tree).
    ///
    /// HTML references are generated using PathResolver (which applies
    /// dir_overrides), so AssetRegistry keys must also use mapped paths
    /// for the preview server to correctly serve placeholders.
    #[test]
    fn test_asset_registry_key_uses_mapped_path() {
        use moss_core::asset_paths;

        let mut dir_overrides = HashMap::new();
        dir_overrides.insert("视频".to_string(), "video".to_string());

        let source_path = "视频/aimeili.mov";

        // The correct key derivation: map FIRST, then derive mp4 path
        let mapped = resolve_path_with_overrides(source_path, &dir_overrides);
        let mp4_key = asset_paths::to_mp4(&mapped);

        assert_eq!(
            mp4_key, "video/aimeili.mp4",
            "AssetRegistry key should be page-tree path 'video/aimeili.mp4'"
        );

        // The WRONG derivation (current bug): derive mp4 without mapping
        let wrong_key = asset_paths::to_mp4(source_path);
        assert_eq!(wrong_key, "视频/aimeili.mp4");

        // These should NOT match — the bug is that they currently do
        // (because the code doesn't map before deriving)
        assert_ne!(
            mp4_key, wrong_key,
            "Mapped and unmapped keys should differ when overrides exist"
        );
    }
}

/// Tests for the `_with_evicted` injectable-predicate seam added by
/// docs/archive/2026-07-31-cloud-download-waiting-mode.md Stage 3 — proves
/// the guarded read sites skip a "cloud-dataless" file exactly like a read
/// error, without needing real `SF_DATALESS` state to exercise the path.
mod with_evicted_seam_tests {
    use super::*;
    use crate::vault::paths::VaultRoot;
    use std::fs;

    fn vroot(p: impl AsRef<Path>) -> VaultRoot {
        VaultRoot::resolve(p)
    }

    fn make_file(path: &str) -> FileInfo {
        FileInfo {
            path: path.to_string(),
            file_type: "md".to_string(),
            size: 0,
            modified: None,
        }
    }

    fn setup_temp_dir(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        for (path, content) in files {
            let full = dir.path().join(path);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent).expect("failed to create dirs");
            }
            fs::write(full, content).expect("failed to write file");
        }
        dir
    }

    #[test]
    fn compute_home_overrides_with_evicted_skips_evicted_home_marker() {
        // Content says `home: true`, but the predicate reports it evicted —
        // the promotion must not happen (the frontmatter is never read).
        let dir = setup_temp_dir(&[("liu-guo.md", "---\nhome: true\n---\n# Liu Guo\n")]);
        let root = vroot(dir.path());
        let files = vec![make_file("liu-guo.md")];

        let is_evicted = |p: &Path| p.ends_with("liu-guo.md");
        let overrides = compute_home_overrides_with_evicted(&files, &root, &is_evicted);
        assert!(
            overrides.is_empty(),
            "an evicted home-marker file must not be promoted — its frontmatter was never read"
        );

        // Sanity: with the real (non-evicting) predicate, the same fixture
        // DOES promote — proving the test fixture is valid and the skip
        // above is caused by eviction, not a fixture mistake.
        let not_evicted = |_: &Path| false;
        let real_overrides = compute_home_overrides_with_evicted(&files, &root, &not_evicted);
        assert!(!real_overrides.is_empty(), "fixture sanity check: non-evicted read should promote");
    }

    #[test]
    fn build_page_map_with_evicted_skips_url_override_on_evicted_file() {
        let dir = setup_temp_dir(&[("about.md", "---\nurl: custom-slug\n---\n# About\n")]);
        let files = vec![make_file("about.md")];
        let winners = compute_home_file_winners(&files, "mysite", &std::collections::HashMap::new());

        let is_evicted = |p: &Path| p.ends_with("about.md");
        let (map, _overrides) = build_page_map_with_evicted(
            &files,
            dir.path(),
            "mysite",
            &winners,
            &std::collections::HashMap::new(),
            &is_evicted,
        );
        // The evicted file is deferred entirely (Stage 3: skip, don't wait,
        // don't fabricate a fallback) — it never gets a `url_path` entry at
        // all, and critically its `url: custom-slug` frontmatter override
        // was never read.
        assert!(
            map.get("about.md").is_none(),
            "an evicted file must be deferred out of the page map, not slugged from a name it was never read to check"
        );

        // Sanity: with the real (non-evicting) predicate, the file DOES
        // appear, proving the assertion above is caused by eviction.
        let not_evicted = |_: &Path| false;
        let (real_map, _) = build_page_map_with_evicted(
            &files,
            dir.path(),
            "mysite",
            &winners,
            &std::collections::HashMap::new(),
            &not_evicted,
        );
        assert!(real_map.get("about.md").is_some(), "fixture sanity check: non-evicted read should populate the map");
    }

    #[test]
    fn build_external_url_map_with_evicted_skips_evicted_file() {
        let dir = setup_temp_dir(&[(
            "linkpost.md",
            "---\nexternal_url: https://example.com/post\n---\n# Linkpost\n",
        )]);
        let files = vec![make_file("linkpost.md")];

        let is_evicted = |p: &Path| p.ends_with("linkpost.md");
        let map = build_external_url_map_with_evicted(&files, dir.path(), &is_evicted);
        assert!(
            map.is_empty(),
            "an evicted file's external_url frontmatter must never be read"
        );

        let not_evicted = |_: &Path| false;
        let real_map = build_external_url_map_with_evicted(&files, dir.path(), &not_evicted);
        assert!(!real_map.is_empty(), "fixture sanity check: non-evicted read should populate the map");
    }
}

/// Pins the corpus-scale fix: `FrontmatterScanCache` must reuse an unchanged
/// file's extraction without re-reading it, but never serve a changed file
/// from a stale entry — and the merged cached scan must return exactly what
/// the two separate uncached scans it replaced would have.
/// See docs/archive/2026-08-20-rebuild-loop-incrementality.md.
mod frontmatter_scan_cache_tests {
    use super::*;
    use std::fs;

    fn make_file(path: &str) -> FileInfo {
        FileInfo {
            path: path.to_string(),
            file_type: "md".to_string(),
            size: 0,
            modified: None,
        }
    }

    fn setup_temp_dir(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        for (path, content) in files {
            let full = dir.path().join(path);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent).expect("failed to create dirs");
            }
            fs::write(full, content).expect("failed to write file");
        }
        dir
    }

    #[test]
    fn cached_combined_scan_matches_the_two_uncached_scans() {
        let dir = setup_temp_dir(&[
            (
                "posts/hello.md",
                "---\nurl: greeting\nexternal_url: https://example.com/hello\n---\n# Hello",
            ),
            ("about.md", "# About, no frontmatter at all"),
        ]);
        let files = vec![make_file("posts/hello.md"), make_file("about.md")];
        let winners = compute_home_file_winners(&files, "mysite", &std::collections::HashMap::new());
        let overrides = std::collections::HashMap::new();

        let (expected_map, expected_dir_overrides) =
            build_page_map(&files, dir.path(), "mysite", &winners, &overrides);
        let expected_external = build_external_url_map(&files, dir.path());

        let mut cache = FrontmatterScanCache::default();
        let (map, dir_overrides, external_map) = build_page_map_and_external_urls_cached(
            &files, dir.path(), "mysite", &winners, &overrides, &mut cache,
        );

        assert_eq!(map, expected_map, "page_map should equal the uncached build_page_map result");
        assert_eq!(
            dir_overrides, expected_dir_overrides,
            "dir_overrides should equal the uncached build_page_map result"
        );
        assert_eq!(
            external_map, expected_external,
            "external_url_map should equal the uncached build_external_url_map result"
        );
    }

    #[test]
    fn unchanged_file_is_served_from_the_cache_not_reread() {
        let dir = setup_temp_dir(&[("about.md", "---\nurl: custom-slug\n---\n# About\n")]);
        let files = vec![make_file("about.md")];
        let winners = compute_home_file_winners(&files, "mysite", &std::collections::HashMap::new());
        let overrides = std::collections::HashMap::new();
        let not_evicted = |_: &Path| false;
        let mut cache = FrontmatterScanCache::default();

        // Cold pass: reads the real file and populates the cache.
        let (map1, ..) = build_page_map_and_external_urls_cached_with_evicted(
            &files, dir.path(), "mysite", &winners, &overrides, &mut cache, &not_evicted,
        );
        assert_eq!(map1.get("about.md").unwrap(), "custom-slug/index.html");
        assert!(cache.entries.contains_key("about.md"));

        // Poison the cached VALUE while leaving its stat identity untouched.
        // The file on disk still says "custom-slug" — a hit reuses this
        // poisoned entry; a miss would re-read the real file and recover
        // "custom-slug". This is how a hit is told apart from a miss
        // without instrumenting the read itself.
        cache.entries.get_mut("about.md").unwrap().url_override = Some("poisoned".to_string());

        // Warm pass: the file is untouched since the cold pass, so its stat
        // identity still matches — this MUST be served from the (poisoned)
        // cache rather than re-read.
        let (map2, ..) = build_page_map_and_external_urls_cached_with_evicted(
            &files, dir.path(), "mysite", &winners, &overrides, &mut cache, &not_evicted,
        );
        assert_eq!(
            map2.get("about.md").unwrap(),
            "poisoned/index.html",
            "an unchanged file must be served from the cache, not re-read"
        );
    }

    #[test]
    fn changed_file_bypasses_a_stale_cache_entry() {
        let dir = setup_temp_dir(&[("about.md", "---\nurl: first-slug\n---\n# About\n")]);
        let files = vec![make_file("about.md")];
        let winners = compute_home_file_winners(&files, "mysite", &std::collections::HashMap::new());
        let overrides = std::collections::HashMap::new();
        let not_evicted = |_: &Path| false;
        let mut cache = FrontmatterScanCache::default();

        let (map1, ..) = build_page_map_and_external_urls_cached_with_evicted(
            &files, dir.path(), "mysite", &winners, &overrides, &mut cache, &not_evicted,
        );
        assert_eq!(map1.get("about.md").unwrap(), "first-slug/index.html");

        // A real edit: different length, so size alone already fails the
        // identity check regardless of filesystem mtime granularity.
        fs::write(
            dir.path().join("about.md"),
            "---\nurl: second-slug\n---\n# About, now with a longer body so the file size moves too\n",
        )
        .unwrap();

        let (map2, ..) = build_page_map_and_external_urls_cached_with_evicted(
            &files, dir.path(), "mysite", &winners, &overrides, &mut cache, &not_evicted,
        );
        assert_eq!(
            map2.get("about.md").unwrap(),
            "second-slug/index.html",
            "a changed file must never be served from a stale cache entry"
        );
    }

    #[test]
    fn cache_round_trips_through_save_and_load() {
        let dir = setup_temp_dir(&[("about.md", "---\nurl: custom-slug\n---\n# About\n")]);
        let files = vec![make_file("about.md")];
        let winners = compute_home_file_winners(&files, "mysite", &std::collections::HashMap::new());
        let overrides = std::collections::HashMap::new();
        let not_evicted = |_: &Path| false;

        let mut cache = FrontmatterScanCache::default();
        build_page_map_and_external_urls_cached_with_evicted(
            &files, dir.path(), "mysite", &winners, &overrides, &mut cache, &not_evicted,
        );

        let cache_path = dir.path().join("frontmatter-scan.json");
        cache.save(&cache_path).expect("save must succeed");
        let reloaded = FrontmatterScanCache::load(&cache_path);
        assert_eq!(
            reloaded.entries.get("about.md").map(|e| &e.url_override),
            Some(&Some("custom-slug".to_string())),
            "a saved cache must reload with the same extracted values"
        );
    }

    /// The declaration rung reads `lang` out of THIS cache, so it has to
    /// survive persistence and it has to be served on a hit — the two paths a
    /// cold-scan test never touches.
    #[test]
    fn a_declared_lang_survives_the_cache_and_is_served_without_re_reading() {
        let dir = setup_temp_dir(&[("index.md", "---\nlang: \"zh-hant\"\n---\n# 首頁\n")]);
        let files = vec![make_file("index.md")];
        let winners = compute_home_file_winners(&files, "mysite", &std::collections::HashMap::new());
        let overrides = std::collections::HashMap::new();
        let not_evicted = |_: &Path| false;

        let mut cache = FrontmatterScanCache::default();
        build_page_map_and_external_urls_cached_with_evicted(
            &files, dir.path(), "mysite", &winners, &overrides, &mut cache, &not_evicted,
        );
        // Quoted in the simplified dialect: the reader this replaced went
        // through serde_yaml and unquoted, so a raw value would have failed
        // the allowlist and silently dropped the declaration.
        assert_eq!(cache.declared_lang("index.md"), Some("zh-hant"));

        let cache_path = dir.path().join("frontmatter-scan.json");
        cache.save(&cache_path).expect("save must succeed");
        let mut reloaded = FrontmatterScanCache::load(&cache_path);
        assert_eq!(reloaded.declared_lang("index.md"), Some("zh-hant"), "lost in save/load");

        // Second build, file untouched: served from the entry, and the file
        // itself is now unreadable to prove nothing re-read it.
        std::fs::remove_file(dir.path().join("index.md")).unwrap();
        build_page_map_and_external_urls_cached_with_evicted(
            &files, dir.path(), "mysite", &winners, &overrides, &mut reloaded, &not_evicted,
        );
        assert_eq!(reloaded.declared_lang("index.md"), Some("zh-hant"), "lost on a cache hit");
    }

    /// An entry written before `lang` existed is not incomplete, it is WRONG:
    /// absent reads identically to "declares nothing". The schema stamp is
    /// what stops a whole build of folders silently falling to inference.
    #[test]
    fn a_cache_from_before_the_lang_field_is_discarded_not_trusted() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let path = dir.path().join("frontmatter-scan.json");
        std::fs::write(
            &path,
            r#"{"entries":{"index.md":{"size":10,"mtime":1,"mtime_nanos":0,
               "ctime":null,"inode":null,"url_override":null,"external_url":null}}}"#,
        )
        .unwrap();
        assert!(
            FrontmatterScanCache::load(&path).entries.is_empty(),
            "a schema-less cache must be discarded, not read as 'declares nothing'"
        );
    }

    #[test]
    fn load_of_a_missing_file_is_a_cold_empty_cache_not_an_error() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let cache = FrontmatterScanCache::load(&dir.path().join("does-not-exist.json"));
        assert!(cache.entries.is_empty());
    }
}
