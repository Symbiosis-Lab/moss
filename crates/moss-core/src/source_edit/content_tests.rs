use super::*;
use super::save::coerce_union_fields;
use super::content::resolve_bootstrap_source;
    #[test]
    fn coerce_union_fields_quotes_numeric_uid() {
        // A uid YAML parsed as a number must be re-quoted to a
        // string on write so the on-disk file self-heals.
        let mut m = std::collections::HashMap::new();
        m.insert("uid".to_string(), serde_yaml::Value::Number(46160604u64.into()));
        coerce_union_fields(&mut m);
        assert_eq!(m.get("uid"), Some(&serde_yaml::Value::String("46160604".to_string())));
    }

    #[test]
    fn coerce_union_fields_leaves_string_uid_untouched() {
        let mut m = std::collections::HashMap::new();
        m.insert("uid".to_string(), serde_yaml::Value::String("54ddc5c0".to_string()));
        coerce_union_fields(&mut m);
        assert_eq!(m.get("uid"), Some(&serde_yaml::Value::String("54ddc5c0".to_string())));
    }

    use crate::heading::HeadingSource;

    #[test]
    fn heading_state_bridges_basic_article() {
        let s = compute_heading_state_inner(
            "posts/my-first-post.md",
            &serde_json::json!({}),
            "",
        );
        assert!(s.visible);
        assert_eq!(s.text, "my first post");
        assert!(matches!(s.source, HeadingSource::Filename));
    }

    #[test]
    fn heading_state_bridges_title_field() {
        let s = compute_heading_state_inner(
            "posts/article.md",
            &serde_json::json!({ "title": "Custom" }),
            "",
        );
        assert_eq!(s.text, "Custom");
        assert!(matches!(s.source, HeadingSource::Title));
        assert!(s.visible);
    }

    #[test]
    fn heading_state_bridges_empty_title_hides() {
        let s = compute_heading_state_inner(
            "posts/article.md",
            &serde_json::json!({ "title": "" }),
            "",
        );
        assert!(!s.visible);
        assert!(matches!(s.source, HeadingSource::Title));
    }

    #[test]
    fn heading_state_bridges_non_string_title_as_filename() {
        // Defensive: only string title:'s are honored. A boolean or number
        // shouldn't accidentally promote to Title source.
        let s = compute_heading_state_inner(
            "posts/article.md",
            &serde_json::json!({ "title": false }),
            "",
        );
        assert!(s.visible);
        assert!(matches!(s.source, HeadingSource::Filename));
    }

    #[test]
    fn heading_state_bridges_hero_in_body_hides() {
        let s = compute_heading_state_inner(
            "posts/article.md",
            &serde_json::json!({}),
            ":::hero\n# Overlay title\n:::\n\nBody.",
        );
        assert!(!s.visible);
    }

    #[test]
    fn heading_state_keeps_the_pinned_heading_for_an_image_only_hero() {
        // The editor reads the same rule the build does, so the 87-page title
        // loss showed up here too: a full-bleed cover made the pinned heading
        // element vanish from the editor, giving the author no way to see or
        // edit the title of the page they were writing.
        let s = compute_heading_state_inner(
            "posts/article.md",
            &serde_json::json!({}),
            ":::hero {image=assets/cover.jpg}\n:::\n\nBody.",
        );
        assert!(s.visible);
    }

    #[test]
    fn resolve_bootstrap_source_absolute_passes_through() {
        // Build an OS-native absolute path: a bare "/abs/..." literal is NOT
        // absolute on Windows (no drive prefix), so it would take the join
        // branch and fail there. Production callers always pass OS-native
        // absolute paths (baked by resolve_page_source).
        let abs = std::env::temp_dir().join("posts").join("a.md");
        let abs_str = abs.to_string_lossy().to_string();
        let resolved = resolve_bootstrap_source(Some("/proj"), &abs_str);
        assert_eq!(resolved.as_deref(), Some(abs_str.as_str()));
        // Absolute source needs no project path at all.
        let resolved = resolve_bootstrap_source(None, &abs_str);
        assert_eq!(resolved.as_deref(), Some(abs_str.as_str()));
    }

    #[test]
    fn resolve_bootstrap_source_relative_joins_project() {
        let resolved = resolve_bootstrap_source(Some("/proj"), "posts/a.md");
        let expected = std::path::Path::new("/proj")
            .join("posts/a.md")
            .to_string_lossy()
            .to_string();
        assert_eq!(resolved.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn resolve_bootstrap_source_relative_without_project_is_none() {
        assert_eq!(resolve_bootstrap_source(None, "posts/a.md"), None);
    }


#[test]
fn source_read_facts_preserve_pending_and_failure() {
    assert_eq!(classify_source_read("note.md", SourceRead::Text("body".into())).unwrap(), Some("body".into()));
    assert_eq!(classify_source_read("note.md", SourceRead::StillArriving).unwrap(), None);
    assert_eq!(classify_source_read("note.md", SourceRead::Failed("denied".into())).unwrap_err(), "Failed to read file 'note.md': denied");
    let pending = ParsedFrontmatter::still_arriving();
    assert!(pending.still_arriving);
    assert!(pending.hash.is_empty() && pending.body.is_empty());
}

#[test]
fn heading_accepts_criticmarkup_before_detecting_hero() {
    assert!(!compute_heading_state_inner("note.md", &serde_json::json!({}), "{>>comment<<}\n:::hero\n# Hero\n:::").visible);
}

    #[test]
    fn parse_frontmatter_preserves_block_and_reports_error() {
        let content =
            "---\nchildren_style: grid\nseries: true\nweight: 10\nuid: example-cover: \"006.jpg\"\n---\n\n\nreal body\n";

        let parsed =
            parse_frontmatter_of(content.to_string()).expect("parse_frontmatter");

        assert!(
            parsed.frontmatter_error.is_some(),
            "malformed YAML must surface an error"
        );
        assert!(
            parsed.has_frontmatter,
            "a block exists (even though invalid) → has_frontmatter is true"
        );
        assert!(
            parsed.body.contains("uid: example-cover"),
            "raw block must stay in body so CM6 can repair it, got: {}",
            parsed.body
        );
    }

    #[test]
    fn parse_frontmatter_valid_reports_no_error() {
        let content = "---\ntitle: Hi\n---\nBody.\n";

        let parsed =
            parse_frontmatter_of(content.to_string()).expect("parse_frontmatter");
        assert!(parsed.frontmatter_error.is_none());
        assert!(parsed.has_frontmatter);
    }

