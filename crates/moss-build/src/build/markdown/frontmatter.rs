//! Frontmatter structs, YAML deserializers, and URL-path computation.
//!
//! Covers: `AnalyticsConfig`, `FrontMatter`, `deserialize_bool_lenient`,
//! `frontmatter_ref_to_stem`, `is_simplified_frontmatter`,
//! `parse_simplified_frontmatter`, `compute_url_path`.

// Re-export slug functions needed by callers who historically imported them
// from the markdown module. generate_slug and slugify_path_segments are now
// in moss-core (ADR-018) and forwarded via scan::slug.
pub use crate::build::scan::slug::{
    generate_slug, generate_uid, insert_uid_into_frontmatter, replace_uid_in_frontmatter,
    resolve_duplicate_slugs_with_lang,
};

// AnalyticsConfig, FrontMatter, compute_url_path, and their helpers moved to
// moss-core (ADR-018). Re-exported for backward compat.
pub use moss_core::frontmatter_typed::{
    AnalyticsConfig,
    FrontMatter,
    apply_sidebar_alias,
    compute_url_path,
    deserialize_bool_lenient,
    deserialize_children_lenient,
    frontmatter_ref_to_stem,
    is_simplified_frontmatter,
    parse_simplified_frontmatter,
};

/// Parse a document's traditional-YAML frontmatter into the typed
/// [`FrontMatter`] using the ONE parser the editor/chips use (ADR-020):
/// `moss_core::frontmatter::parse` + `project_typed`. gray_matter is gone.
///
/// This is the YAML-branch counterpart to `parse_simplified_frontmatter`.
/// Build-pipeline pre-scans (`page_map`, `external_url` map) that previously
/// did `Matter::<YAML>::parse(...).data.deserialize::<FrontMatter>()` call this
/// instead, so every frontmatter read in the build goes through a single
/// resilient projection. Per-field projection warnings are discarded here —
/// these pre-scans read a single field and never blank the struct; the
/// authoritative warning surface is `process_markdown_file`.
pub fn parse_typed_frontmatter(content: &str) -> FrontMatter {
    let parsed = moss_core::frontmatter::parse(content);
    let mut mapping = serde_yaml::Mapping::new();
    for (k, v) in &parsed.frontmatter {
        mapping.insert(serde_yaml::Value::String(k.clone()), v.clone());
    }
    moss_core::frontmatter_typed::project_typed(&mapping).0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frontmatter_analytics_parsing() {
        let markdown = r#"---
title: My Site
analytics:
  url: "https://analytics.example.com/script.js"
  site_id: "abc123"
---

# Hello

Some content.
"#;
        let frontmatter: FrontMatter = parse_typed_frontmatter(markdown);

        assert!(frontmatter.analytics.is_some());
        let analytics = frontmatter.analytics.unwrap();
        assert_eq!(analytics.url, "https://analytics.example.com/script.js");
        assert_eq!(analytics.site_id, Some("abc123".to_string()));
        assert_eq!(analytics.provider, None);
    }

    #[test]
    fn test_frontmatter_page_tree_fields() {
        let markdown = r#"---
title: My Page
nav: true
draft: false
children: true
sidebar: "[[News]]"
children_style: card
children_depth: all
breadcrumb: true
description: "A test page"
tags:
  - rust
  - moss
also_in:
  - weekly-highlights
  - best-of-2024
---

# Hello

Some content.
"#;
        let frontmatter: FrontMatter = parse_typed_frontmatter(markdown);

        assert_eq!(frontmatter.nav, Some(true));
        assert_eq!(frontmatter.draft, Some(false));
        assert_eq!(frontmatter.children, Some(true));
        assert_eq!(frontmatter.sidebar, Some("[[News]]".to_string()));
        assert_eq!(frontmatter.children_style, Some("card".to_string()));
        assert_eq!(frontmatter.children_depth, Some("all".to_string()));
        assert_eq!(frontmatter.breadcrumb, Some(true));
        assert_eq!(frontmatter.description, Some("A test page".to_string()));
        assert_eq!(frontmatter.tags, Some(vec!["rust".to_string(), "moss".to_string()]));
        assert_eq!(
            frontmatter.also_in,
            Some(vec!["weekly-highlights".to_string(), "best-of-2024".to_string()])
        );
    }

    #[test]
    fn test_frontmatter_breadcrumb_string_true() {
        let markdown = r#"---
title: 刘果
breadcrumb: "true"
---
看星星，食烟火。
"#;
        let frontmatter: FrontMatter = parse_typed_frontmatter(markdown);

        assert_eq!(frontmatter.title, Some("刘果".to_string()));
        assert_eq!(frontmatter.breadcrumb, Some(true));
    }

    #[test]
    fn test_frontmatter_breadcrumb_string_false() {
        let markdown = "---\nbreadcrumb: \"false\"\n---\n";
        let frontmatter: FrontMatter = parse_typed_frontmatter(markdown);

        assert_eq!(frontmatter.breadcrumb, Some(false));
    }

    #[test]
    fn test_frontmatter_page_tree_fields_defaults() {
        let markdown = r#"---
title: Simple Page
---

# Hello
"#;
        let frontmatter: FrontMatter = parse_typed_frontmatter(markdown);

        assert_eq!(frontmatter.nav, None);
        assert_eq!(frontmatter.draft, None);
        assert_eq!(frontmatter.children, None);
        assert_eq!(frontmatter.children_style, None);
        assert_eq!(frontmatter.children_depth, None);
        assert_eq!(frontmatter.description, None);
        assert_eq!(frontmatter.tags, None);
        assert_eq!(frontmatter.breadcrumb, None);
        assert_eq!(frontmatter.also_in, None);
    }

    #[test]
    fn test_frontmatter_series_field_list() {
        let markdown = r#"---
title: "无用之旅"
series:
  - "[[1-from-yosemite]]"
  - "[[2-cost-of-civilization]]"
  - "[[3-a-great-river]]"
---

Content here.
"#;
        let frontmatter: FrontMatter = parse_typed_frontmatter(markdown);

        match frontmatter.series {
            Some(crate::build::types::SeriesField::Ordered(list)) => {
                assert_eq!(list.len(), 3);
                assert_eq!(list[0], "[[1-from-yosemite]]");
                assert_eq!(list[1], "[[2-cost-of-civilization]]");
                assert_eq!(list[2], "[[3-a-great-river]]");
            }
            other => panic!("Expected SeriesField::Ordered, got {:?}", other),
        }
    }

    #[test]
    fn test_frontmatter_order_alias_for_sort() {
        // Task 4: `order:` alias migrated from `series:` to `sort:`. The
        // legacy YAML form keeps parsing — it now lands on the `sort` field
        // as `SortField::List` instead of `SeriesField::Ordered`.
        let markdown = r#"---
title: "Test"
order:
  - "[[chapter-1]]"
  - "[[chapter-2]]"
---

Content here.
"#;
        let frontmatter: FrontMatter = parse_typed_frontmatter(markdown);

        match frontmatter.sort {
            Some(moss_core::sort::SortField::List(list)) => {
                assert_eq!(list.len(), 2);
                assert_eq!(list[0], "[[chapter-1]]");
                assert_eq!(list[1], "[[chapter-2]]");
            }
            other => panic!("Expected SortField::List, got {:?}", other),
        }
        // series is untouched by the alias migration
        assert!(frontmatter.series.is_none());
    }

    #[test]
    fn test_frontmatter_series_field_bool() {
        let markdown = r#"---
title: "Test"
series: true
---

Content.
"#;
        let frontmatter: FrontMatter = parse_typed_frontmatter(markdown);

        match frontmatter.series {
            Some(crate::build::types::SeriesField::Flag(true)) => {}
            other => panic!("Expected SeriesField::Flag(true), got {:?}", other),
        }
    }

    #[test]
    fn test_frontmatter_series_field_default() {
        let markdown = r#"---
title: Simple Page
---

Content.
"#;
        let frontmatter: FrontMatter = parse_typed_frontmatter(markdown);

        assert!(frontmatter.series.is_none());
    }

    #[test]
    fn test_frontmatter_without_analytics() {
        let markdown = r#"---
title: My Site
---

# Hello

Some content.
"#;
        let frontmatter: FrontMatter = parse_typed_frontmatter(markdown);

        assert!(frontmatter.analytics.is_none());
    }

    #[test]
    fn test_frontmatter_goatcounter_analytics_parsing() {
        let markdown = r#"---
title: My Site
analytics:
  provider: goatcounter
  url: "https://mysite.goatcounter.com/count"
---

# Hello

Some content.
"#;
        let frontmatter: FrontMatter = parse_typed_frontmatter(markdown);

        assert!(frontmatter.analytics.is_some());
        let analytics = frontmatter.analytics.unwrap();
        assert_eq!(analytics.url, "https://mysite.goatcounter.com/count");
        assert_eq!(analytics.provider, Some("goatcounter".to_string()));
        assert_eq!(analytics.site_id, None);
    }

    #[test]
    fn test_analytics_goatcounter_script_tag() {
        let analytics = AnalyticsConfig {
            provider: Some("goatcounter".to_string()),
            url: "https://mysite.goatcounter.com/count".to_string(),
            site_id: None,
        };
        let script = analytics.to_script_tag();
        assert!(script.contains("data-goatcounter"));
        assert!(script.contains("gc.zgo.at/count.js"));
    }

    #[test]
    fn test_analytics_umami_script_tag() {
        let analytics = AnalyticsConfig {
            provider: None,
            url: "https://analytics.example.com/script.js".to_string(),
            site_id: Some("abc123".to_string()),
        };
        let script = analytics.to_script_tag();
        assert!(script.contains("defer"));
        assert!(script.contains("abc123"));
    }

    #[test]
    fn test_analytics_explicit_umami_provider_script_tag() {
        let analytics = AnalyticsConfig {
            provider: Some("umami".to_string()),
            url: "https://analytics.example.com/script.js".to_string(),
            site_id: Some("site-id-123".to_string()),
        };
        let script = analytics.to_script_tag();
        assert!(script.contains("defer"));
        assert!(script.contains("site-id-123"));
        assert!(!script.contains("goatcounter"));
    }

    #[test]
    fn test_frontmatter_analytics_string_goatcounter() {
        let markdown = r#"---
title: My Site
analytics: "https://mysite.goatcounter.com/count"
---

# Hello
"#;
        let frontmatter: FrontMatter = parse_typed_frontmatter(markdown);

        assert!(frontmatter.analytics.is_some());
        let analytics = frontmatter.analytics.unwrap();
        assert_eq!(analytics.url, "https://mysite.goatcounter.com/count");
        assert_eq!(analytics.provider, None); // auto-detected at render time

        // Verify auto-detection works for GoatCounter
        let script = analytics.to_script_tag();
        assert!(script.contains("data-goatcounter"));
    }

    #[test]
    fn test_frontmatter_analytics_string_umami() {
        let markdown = r#"---
title: My Site
analytics: "https://analytics.mysite.com/script.js"
---

# Hello
"#;
        let frontmatter: FrontMatter = parse_typed_frontmatter(markdown);

        assert!(frontmatter.analytics.is_some());
        let analytics = frontmatter.analytics.unwrap();
        assert_eq!(analytics.url, "https://analytics.mysite.com/script.js");
        assert_eq!(analytics.provider, None); // auto-detected at render time

        // Verify auto-detection works for Umami (default)
        let script = analytics.to_script_tag();
        assert!(script.contains("defer"));
        assert!(!script.contains("goatcounter"));
    }

    #[test]
    fn test_analytics_auto_detect_goatcounter_from_url() {
        let analytics = AnalyticsConfig {
            provider: None,
            url: "https://mysite.goatcounter.com/count".to_string(),
            site_id: None,
        };
        let script = analytics.to_script_tag();
        assert!(script.contains("data-goatcounter"));
        assert!(script.contains("gc.zgo.at/count.js"));
    }

    #[test]
    fn test_goatcounter_pixel_url() {
        let analytics = AnalyticsConfig {
            provider: Some("goatcounter".to_string()),
            url: "https://mysite.goatcounter.com/count".to_string(),
            site_id: None,
        };
        let url = analytics.to_pixel_url("/posts/hello/");
        assert_eq!(
            url,
            Some("https://mysite.goatcounter.com/count?p=/posts/hello/".to_string())
        );
    }

    #[test]
    fn test_goatcounter_pixel_url_auto_detect() {
        let analytics = AnalyticsConfig {
            provider: None,
            url: "https://mysite.goatcounter.com/count".to_string(),
            site_id: None,
        };
        let url = analytics.to_pixel_url("/posts/hello/");
        assert!(url.is_some());
        assert!(url.unwrap().contains("?p="));
    }

    #[test]
    fn test_goatcounter_pixel_url_without_leading_slash() {
        let analytics = AnalyticsConfig {
            provider: Some("goatcounter".to_string()),
            url: "https://mysite.goatcounter.com/count".to_string(),
            site_id: None,
        };
        let url = analytics.to_pixel_url("posts/hello/");
        assert_eq!(
            url,
            Some("https://mysite.goatcounter.com/count?p=/posts/hello/".to_string())
        );
    }

    #[test]
    fn test_umami_pixel_url_returns_none() {
        let analytics = AnalyticsConfig {
            provider: Some("umami".to_string()),
            url: "https://analytics.example.com/script.js".to_string(),
            site_id: Some("abc123".to_string()),
        };
        let url = analytics.to_pixel_url("/posts/hello/");
        assert_eq!(url, None);
    }

    #[test]
    fn test_frontmatter_ref_to_stem_wikilink() {
        assert_eq!(frontmatter_ref_to_stem("[[Departure]]"), "Departure");
    }

    #[test]
    fn test_frontmatter_ref_to_stem_path() {
        assert_eq!(frontmatter_ref_to_stem("travel/departure.md"), "departure");
    }

    #[test]
    fn test_frontmatter_ref_to_stem_folder_note() {
        assert_eq!(frontmatter_ref_to_stem("blog/index.md"), "blog");
    }

    #[test]
    fn test_frontmatter_ref_to_stem_root_file() {
        assert_eq!(frontmatter_ref_to_stem("news.md"), "news");
    }

    #[test]
    fn test_frontmatter_ref_to_stem_plain_name() {
        assert_eq!(frontmatter_ref_to_stem("Departure"), "Departure");
    }

    #[test]
    fn test_simplified_frontmatter_boolean_flag() {
        let content = "nav\n---\n# Hello\n";
        assert!(is_simplified_frontmatter(content));
        let (fm, body) = parse_simplified_frontmatter(content);
        assert_eq!(fm.nav, Some(true));
        assert!(body.contains("# Hello"));
    }

    #[test]
    fn test_simplified_frontmatter_key_value() {
        let content = "title: My Page\n---\n# Content\n";
        assert!(is_simplified_frontmatter(content));
        let (fm, body) = parse_simplified_frontmatter(content);
        assert_eq!(fm.title, Some("My Page".to_string()));
        assert!(body.contains("# Content"));
    }

    #[test]
    fn test_simplified_frontmatter_comma_list() {
        let content = "tags: rust, moss, web\n---\n# Content\n";
        let (fm, _) = parse_simplified_frontmatter(content);
        assert_eq!(
            fm.tags,
            Some(vec!["rust".to_string(), "moss".to_string(), "web".to_string()])
        );
    }

    #[test]
    fn test_simplified_frontmatter_multiple_booleans() {
        let content = "nav\ndraft\n---\n# Content\n";
        let (fm, _) = parse_simplified_frontmatter(content);
        assert_eq!(fm.nav, Some(true));
        assert_eq!(fm.draft, Some(true));
    }

    #[test]
    fn test_simplified_frontmatter_mixed() {
        let content = "title: Test\nnav\ndraft: false\n---\n# Content\n";
        let (fm, body) = parse_simplified_frontmatter(content);
        assert_eq!(fm.title, Some("Test".to_string()));
        assert_eq!(fm.nav, Some(true));
        assert_eq!(fm.draft, Some(false));
        assert!(body.contains("# Content"));
    }

    #[test]
    fn test_yaml_frontmatter_still_works() {
        let content = "---\ntitle: YAML Page\nnav: true\n---\n# Content\n";
        assert!(!is_simplified_frontmatter(content));
    }

    #[test]
    fn test_simplified_frontmatter_sidebar_wikilink() {
        let content = "sidebar: [[News]]\n---\n# Content\n";
        let (fm, _) = parse_simplified_frontmatter(content);
        assert_eq!(fm.sidebar, Some("[[News]]".to_string()));
    }

    #[test]
    fn test_frontmatter_uid_parsing() {
        let markdown = "---\nuid: abcd1234\n---\n# Page\n";
        let fm: FrontMatter = parse_typed_frontmatter(markdown);
        assert_eq!(fm.uid, Some("abcd1234".to_string()));
    }

    #[test]
    fn test_frontmatter_uid_missing() {
        let markdown = "---\ntitle: No UID\n---\n# Page\n";
        let fm: FrontMatter = parse_typed_frontmatter(markdown);
        assert_eq!(fm.uid, None);
    }

    #[test]
    fn test_frontmatter_url_field_parsed() {
        let markdown = "---\ntitle: Test\nurl: custom-path\n---\n# Content\n";
        let fm: FrontMatter = parse_typed_frontmatter(markdown);
        assert_eq!(fm.url, Some("custom-path".to_string()));
    }

    #[test]
    fn test_parse_slot_frontmatter() {
        // Mirrors the canonical `test_simplified_frontmatter_*` pattern:
        // exercise `parse_simplified_frontmatter` and read back the populated
        // FrontMatter field. The simplified format (no leading `---`) routes
        // through the `match key` arms updated in this commit.
        let content = "title: Attribution\nslot: footer-left\n---\n\nbody";
        let (fm, _) = parse_simplified_frontmatter(content);
        assert_eq!(fm.slot.as_deref(), Some("footer-left"));
    }

    #[test]
    fn test_parse_slot_absent_is_none() {
        let content = "title: Just a page\n---\n\nbody";
        let (fm, _) = parse_simplified_frontmatter(content);
        assert_eq!(fm.slot, None);
    }

    // ---- sidebar→children alias translation ----

    fn fm_with_sidebar(s: &str) -> FrontMatter {
        let mut fm = FrontMatter::default();
        fm.sidebar = Some(s.to_string());
        fm
    }

    #[test]
    fn children_string_form_deserializes_without_choking() {
        // B2: the old pre-parse rewrite is gone; the lenient deserializer must
        // accept a wikilink/path string directly (resolving to children: true),
        // without failing the whole struct deserialize (which unwrap_or_default
        // would silently swallow).
        let yaml = "title: T\nchildren: \"[[News]]\"\n";
        let fm: FrontMatter = serde_yaml::from_str(yaml).expect("string children must deserialize");
        assert_eq!(fm.children, Some(true), "wikilink string resolves to children: true");

        let yaml_path = "title: T\nchildren: news/index.md\n";
        let fm2: FrontMatter = serde_yaml::from_str(yaml_path).expect("bare path must deserialize");
        assert_eq!(fm2.children, Some(true), "bare path resolves to children: true");

        let yaml_bool = "title: T\nchildren: false\n";
        let fm3: FrontMatter = serde_yaml::from_str(yaml_bool).unwrap();
        assert_eq!(fm3.children, Some(false));
    }

    #[test]
    fn alias_translates_when_only_sidebar_set() {
        let mut fm = fm_with_sidebar("[[News]]");
        let warnings = apply_sidebar_alias(&mut fm);

        assert_eq!(fm.children, Some(true));
        assert_eq!(fm.children_source.as_deref(), Some("[[News]]"));
        assert_eq!(fm.children_in.as_deref(), Some("sidebar"));
        assert_eq!(fm._from_sidebar_alias, Some(true));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("deprecated"));
    }

    #[test]
    fn alias_translates_yi_website_pattern_sidebar_plus_children_false() {
        // Yi-website's literal frontmatter: sidebar: '[[news]]' + children: false.
        // children: false is redundant when sidebar is set (today: !has_sidebar gate);
        // alias must treat both unset and false as "no real children intent".
        let mut fm = fm_with_sidebar("[[news]]");
        fm.children = Some(false);

        let _warnings = apply_sidebar_alias(&mut fm);

        assert_eq!(fm.children, Some(true), "alias should override children: false");
        assert_eq!(fm.children_source.as_deref(), Some("[[news]]"));
        assert_eq!(fm.children_in.as_deref(), Some("sidebar"));
        assert_eq!(fm._from_sidebar_alias, Some(true));
    }

    #[test]
    fn alias_does_not_fire_when_children_wikilink_present() {
        // Conflict: user has explicit children intent. children: wins, sidebar: ignored, warn.
        let mut fm = fm_with_sidebar("[[A]]");
        fm.children = Some(true);
        fm.children_source = Some("[[B]]".to_string());

        let warnings = apply_sidebar_alias(&mut fm);

        assert_eq!(fm.children_source.as_deref(), Some("[[B]]"), "children_source untouched");
        assert_eq!(fm.children_in, None, "children_in not set on conflict");
        assert_eq!(fm._from_sidebar_alias, None);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("ignored"));
    }

    #[test]
    fn alias_does_not_fire_when_children_explicit_true_only() {
        // Edge: children: true with no source. Still treated as positive intent.
        let mut fm = fm_with_sidebar("[[News]]");
        fm.children = Some(true);

        let warnings = apply_sidebar_alias(&mut fm);

        assert_eq!(fm.children_in, None);
        assert_eq!(fm._from_sidebar_alias, None);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("ignored"));
    }

    #[test]
    fn alias_no_op_when_sidebar_absent() {
        let mut fm = FrontMatter::default();
        fm.children = Some(true);
        fm.children_source = Some("[[X]]".to_string());

        let warnings = apply_sidebar_alias(&mut fm);

        assert_eq!(warnings.len(), 0);
        assert_eq!(fm.children_in, None);
        assert_eq!(fm._from_sidebar_alias, None);
    }

    #[test]
    fn alias_preserves_unresolvable_target_silently() {
        // sidebar: "[[Typo]]" — alias still translates literally; unresolved
        // target is handled at the rendering callsite (silent empty render).
        let mut fm = fm_with_sidebar("[[Typo]]");

        let _warnings = apply_sidebar_alias(&mut fm);

        assert_eq!(fm.children_source.as_deref(), Some("[[Typo]]"));
        assert_eq!(fm.children_in.as_deref(), Some("sidebar"));
    }

    // =========================================================================
    // Tests for sort: + order alias migration (Task 4)
    // =========================================================================

    #[test]
    fn frontmatter_parses_sort_axis() {
        let fm: FrontMatter = serde_yaml::from_str("title: T\nsort: date\n").unwrap();
        assert!(matches!(fm.sort, Some(moss_core::sort::SortField::Axis(moss_core::sort::SortAxis::Date))));
    }

    #[test]
    fn frontmatter_parses_sort_list() {
        let fm: FrontMatter = serde_yaml::from_str("title: T\nsort: [intro, setup]\n").unwrap();
        match fm.sort {
            Some(moss_core::sort::SortField::List(items)) => assert_eq!(items, vec!["intro", "setup"]),
            _ => panic!("expected List"),
        }
    }

    #[test]
    fn order_alias_parses_into_sort_list() {
        let fm: FrontMatter = serde_yaml::from_str("title: T\norder: [a, b]\n").unwrap();
        match fm.sort {
            Some(moss_core::sort::SortField::List(items)) => assert_eq!(items, vec!["a", "b"]),
            other => panic!("order: should alias to sort, got {:?}", other),
        }
    }

    #[test]
    fn order_with_wikilinks_parses_into_sort_list() {
        // Real-world 刘果 form: order: ["[[Ch 1]]", "[[Ch 2]]"]
        let fm: FrontMatter = serde_yaml::from_str("title: T\norder:\n  - \"[[Ch 1]]\"\n  - \"[[Ch 2]]\"\n").unwrap();
        match fm.sort {
            Some(moss_core::sort::SortField::List(items)) => {
                assert_eq!(items, vec!["[[Ch 1]]", "[[Ch 2]]"]);
            }
            other => panic!("expected wikilink list, got {:?}", other),
        }
    }

    #[test]
    fn series_true_still_parses() {
        let fm: FrontMatter = serde_yaml::from_str("title: T\nseries: true\n").unwrap();
        assert!(matches!(fm.series, Some(crate::build::types::SeriesField::Flag(true))));
    }

    // =========================================================================
    // Tests for normalize() — Task 5
    // =========================================================================

    #[test]
    fn normalize_converts_legacy_series_list_into_sort_list() {
        let mut fm: FrontMatter = serde_yaml::from_str("title: T\nseries:\n  - \"[[Ch 1]]\"\n  - \"[[Ch 2]]\"\n").unwrap();
        fm.normalize();
        match &fm.sort {
            Some(moss_core::sort::SortField::List(items)) => assert_eq!(items.len(), 2),
            other => panic!("series:[list] should be normalized into sort:list, got {:?}", other),
        }
        assert!(matches!(fm.series, Some(crate::build::types::SeriesField::Flag(true))));
    }

    #[test]
    fn normalize_no_op_when_series_is_flag() {
        let mut fm: FrontMatter = serde_yaml::from_str("title: T\nseries: true\n").unwrap();
        fm.normalize();
        assert!(fm.sort.is_none());
        assert!(matches!(fm.series, Some(crate::build::types::SeriesField::Flag(true))));
    }

    #[test]
    fn normalize_preserves_existing_sort_over_legacy_series_list() {
        let mut fm: FrontMatter = serde_yaml::from_str(
            "title: T\nseries:\n  - \"[[Ch 1]]\"\nsort: title\n"
        ).unwrap();
        fm.normalize();
        // sort: title was explicitly declared, should NOT be overwritten by legacy series-list
        match &fm.sort {
            Some(moss_core::sort::SortField::Axis(moss_core::sort::SortAxis::Title)) => {}
            other => panic!("explicit sort should win over legacy series-list, got {:?}", other),
        }
        assert!(matches!(fm.series, Some(crate::build::types::SeriesField::Flag(true))),
            "series should still flip to Flag(true) since chrome was implied");
    }
}
