use super::*;

#[test]
fn empty_produces_no_content() {
    let slots = ResolvedSlots::empty();
    assert!(slots.get_html("head-end", "/index.html").is_none());
    assert!(slots.get_html("body-end", "/about.html").is_none());
    assert!(slots.get_html("nonexistent", "/").is_none());
}

#[test]
fn static_slot_injected_for_any_page() {
    let mut slots = ResolvedSlots::empty();
    let result = EnhanceResult {
        success: true,
        slots: HashMap::from([(
            "head-end".to_string(),
            EnhanceContent::Static {
                html: "<style>body{}</style>".to_string(),
            },
        )]),
    };
    slots.merge(&result, 10, "test-plugin");

    assert_eq!(
        slots.get_html("head-end", "/index.html").unwrap(),
        "<style>body{}</style>"
    );
    assert_eq!(
        slots.get_html("head-end", "/about.html").unwrap(),
        "<style>body{}</style>"
    );
}

#[test]
fn per_page_slot_matches_only_correct_page() {
    let mut slots = ResolvedSlots::empty();
    let result = EnhanceResult {
        success: true,
        slots: HashMap::from([(
            "after-title".to_string(),
            EnhanceContent::PerPage {
                pages: HashMap::from([(
                    "/post/hello.html".to_string(),
                    "<div>rating</div>".to_string(),
                )]),
            },
        )]),
    };
    slots.merge(&result, 10, "test-plugin");

    assert_eq!(
        slots.get_html("after-title", "/post/hello.html").unwrap(),
        "<div>rating</div>"
    );
    assert!(slots.get_html("after-title", "/other.html").is_none());
}

#[test]
fn merge_multiple_plugins_by_priority() {
    let mut slots = ResolvedSlots::empty();

    let result_low = EnhanceResult {
        success: true,
        slots: HashMap::from([(
            "head-end".to_string(),
            EnhanceContent::Static {
                html: "<meta name=\"a\">".to_string(),
            },
        )]),
    };
    let result_high = EnhanceResult {
        success: true,
        slots: HashMap::from([(
            "head-end".to_string(),
            EnhanceContent::Static {
                html: "<meta name=\"b\">".to_string(),
            },
        )]),
    };

    // Merge higher priority (20) first, then lower (5)
    slots.merge(&result_high, 20, "plugin-b");
    slots.merge(&result_low, 5, "plugin-a");

    let html = slots.get_html("head-end", "/").unwrap();
    // Lower priority number comes first
    assert_eq!(html, "<meta name=\"a\">\n<meta name=\"b\">");
}

#[test]
fn failed_result_not_merged() {
    let mut slots = ResolvedSlots::empty();
    let result = EnhanceResult {
        success: false,
        slots: HashMap::from([(
            "head-end".to_string(),
            EnhanceContent::Static {
                html: "should not appear".to_string(),
            },
        )]),
    };
    slots.merge(&result, 10, "test-plugin");
    assert!(slots.get_html("head-end", "/").is_none());
}

#[test]
fn inject_slots_replaces_markers() {
    let mut slots = ResolvedSlots::empty();
    let result = EnhanceResult {
        success: true,
        slots: HashMap::from([
            (
                "head-end".to_string(),
                EnhanceContent::Static {
                    html: "<style>h1{}</style>".to_string(),
                },
            ),
            (
                "body-end".to_string(),
                EnhanceContent::Static {
                    html: "<script>alert(1)</script>".to_string(),
                },
            ),
        ]),
    };
    slots.merge(&result, 10, "test-plugin");

    let html =
        "<html><head><!-- slot:head-end --></head><body><!-- slot:body-end --></body></html>";
    let out = inject_slots(html, &slots, "/");
    assert_eq!(
        out,
        "<html><head><style>h1{}</style></head><body><script>alert(1)</script></body></html>"
    );
}

#[test]
fn inject_slots_removes_markers_when_no_content() {
    let slots = ResolvedSlots::empty();
    let html = "<head><!-- slot:head-end --></head><body><!-- slot:body-end --></body>";
    let out = inject_slots(html, &slots, "/");
    assert_eq!(out, "<head></head><body></body>");
}

#[test]
fn inject_slots_leaves_unknown_markers() {
    let slots = ResolvedSlots::empty();
    let html = "<div><!-- slot:custom-thing --></div>";
    let out = inject_slots(html, &slots, "/");
    assert_eq!(out, "<div><!-- slot:custom-thing --></div>");
}

#[test]
fn per_language_slot_resolves_by_page_path_language() {
    // A PerLanguage footer injects the page's language footer, keyed by the
    // output path's language-tree prefix, and falls back to `default` for
    // root pages and languages without their own footer.
    let mut slots = ResolvedSlots::empty();
    slots.merge(
        &EnhanceResult {
            success: true,
            slots: HashMap::from([(
                "footer-left".to_string(),
                EnhanceContent::PerLanguage {
                    default: Some("EN".to_string()),
                    by_lang: HashMap::from([("zh-hans".to_string(), "ZH".to_string())]),
                },
            )]),
        },
        6,
        "native",
    );
    let html = r#"<footer><!-- slot:footer-left --></footer>"#;

    // Root page → default.
    assert!(inject_slots(html, &slots, "index.html").contains("<footer>EN</footer>"));
    // zh-hans page → its language footer.
    assert!(inject_slots(html, &slots, "zh-hans/index.html").contains("<footer>ZH</footer>"));
    // Deeper zh-hans page → still its language footer.
    assert!(inject_slots(html, &slots, "zh-hans/docs/index.html").contains("<footer>ZH</footer>"));
    // A language without its own footer → falls back to default.
    assert!(inject_slots(html, &slots, "zh-hant/index.html").contains("<footer>EN</footer>"));
}

#[test]
fn per_language_slot_without_default_strips_marker_for_other_languages() {
    // No default footer: only the matching language gets content; other
    // pages have the marker stripped (no content leaks, no stray marker).
    let mut slots = ResolvedSlots::empty();
    slots.merge(
        &EnhanceResult {
            success: true,
            slots: HashMap::from([(
                "footer-left".to_string(),
                EnhanceContent::PerLanguage {
                    default: None,
                    by_lang: HashMap::from([("zh-hans".to_string(), "ZH".to_string())]),
                },
            )]),
        },
        6,
        "native",
    );
    let html = r#"<footer><!-- slot:footer-left --></footer>"#;
    assert!(inject_slots(html, &slots, "zh-hans/index.html").contains("<footer>ZH</footer>"));
    // Root page: no default → marker stripped, empty footer.
    assert_eq!(
        inject_slots(html, &slots, "index.html"),
        "<footer></footer>"
    );
}

#[test]
fn residual_known_slot_markers_flags_unresolved_known_markers() {
    // An unresolved known marker in shipped HTML = slot injection didn't
    // cover this file. Must be detectable so it can be WARN'd.
    let html = r#"<footer class="container"><!-- slot:footer-end --></footer>"#;
    assert_eq!(residual_known_slot_markers(html), vec!["footer-end"]);
}

#[test]
fn residual_known_slot_markers_reports_multiple() {
    let html = "<head><!-- slot:head-end --></head><footer><!-- slot:footer-left --></footer>";
    let mut found = residual_known_slot_markers(html);
    found.sort_unstable();
    assert_eq!(found, vec!["footer-left", "head-end"]);
}

#[test]
fn residual_known_slot_markers_ignores_resolved_and_unknown() {
    // Fully-resolved chrome: nothing flagged.
    assert!(residual_known_slot_markers(r#"<footer class="container"></footer>"#).is_empty());
    // Unknown (non-moss) slot comment is not a moss-owned marker.
    assert!(residual_known_slot_markers("<div><!-- slot:custom-thing --></div>").is_empty());
}

#[test]
fn priority_tie_broken_by_plugin_name() {
    let mut slots = ResolvedSlots::empty();
    let result_z = EnhanceResult {
        success: true,
        slots: HashMap::from([(
            "head-end".to_string(),
            EnhanceContent::Static {
                html: "Z".to_string(),
            },
        )]),
    };
    let result_a = EnhanceResult {
        success: true,
        slots: HashMap::from([(
            "head-end".to_string(),
            EnhanceContent::Static {
                html: "A".to_string(),
            },
        )]),
    };
    // Same priority, different names — alphabetical wins
    slots.merge(&result_z, 10, "zebra");
    slots.merge(&result_a, 10, "alpha");
    let html = slots.get_html("head-end", "/").unwrap();
    assert_eq!(html, "A\nZ");
}

#[test]
fn serde_roundtrip_static() {
    let content = EnhanceContent::Static {
        html: "<p>hi</p>".to_string(),
    };
    let json = serde_json::to_string(&content).unwrap();
    let parsed: EnhanceContent = serde_json::from_str(&json).unwrap();
    match parsed {
        EnhanceContent::Static { html } => assert_eq!(html, "<p>hi</p>"),
        _ => panic!("expected Static"),
    }
}

#[test]
fn serde_roundtrip_per_page() {
    let content = EnhanceContent::PerPage {
        pages: HashMap::from([("/a.html".to_string(), "<b>a</b>".to_string())]),
    };
    let json = serde_json::to_string(&content).unwrap();
    let parsed: EnhanceContent = serde_json::from_str(&json).unwrap();
    match parsed {
        EnhanceContent::PerPage { pages } => assert_eq!(pages.get("/a.html").unwrap(), "<b>a</b>"),
        _ => panic!("expected PerPage"),
    }
}

// --- inject_slots_into_directory_cached tests (moss#919 item 2) ---

fn slot_test_cache(
    tmp: &std::path::Path,
) -> (
    crate::build::cache::ObjectStore,
    crate::build::cache::TransformCache,
) {
    let objects = crate::build::cache::ObjectStore::new(tmp.join("objects"));
    let transforms = crate::build::cache::TransformCache::new(
        tmp.join("transforms"),
        crate::build::cache::ObjectStore::new(tmp.join("objects")),
    );
    (objects, transforms)
}

fn head_end_slots(html: &str) -> ResolvedSlots {
    let mut slots = ResolvedSlots::empty();
    let result = EnhanceResult {
        success: true,
        slots: HashMap::from([(
            "head-end".to_string(),
            EnhanceContent::Static {
                html: html.to_string(),
            },
        )]),
    };
    slots.merge(&result, 10, "test");
    slots
}

#[test]
fn inject_slots_into_directory_cached_hit_matches_miss_output_byte_for_byte() {
    let src_dir_a = tempfile::tempdir().unwrap();
    let src_dir_b = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    let (objects, transforms) = slot_test_cache(cache_dir.path());
    let html = "<html><head><!-- slot:head-end --></head><body></body></html>";
    std::fs::write(src_dir_a.path().join("index.html"), html).unwrap();
    std::fs::write(src_dir_b.path().join("index.html"), html).unwrap();
    let slots = head_end_slots("<style>h1{}</style>");

    // Miss: dir A, empty cache.
    let changed_a =
        inject_slots_into_directory_cached(src_dir_a.path(), src_dir_a.path(), &slots, &objects, &transforms)
            .unwrap();
    // Hit: dir B, same slots, same relative page path, warm cache.
    let changed_b =
        inject_slots_into_directory_cached(src_dir_b.path(), src_dir_b.path(), &slots, &objects, &transforms)
            .unwrap();

    assert_eq!(changed_a, changed_b);
    let out_a = std::fs::read_to_string(src_dir_a.path().join("index.html")).unwrap();
    let out_b = std::fs::read_to_string(src_dir_b.path().join("index.html")).unwrap();
    assert_eq!(out_a, out_b);
    assert!(out_b.contains("<style>h1{}</style>"));
    assert!(!out_b.contains("<!-- slot:head-end -->"));
}

/// The receipt's hash describes the bytes the SITE will serve, not the bytes
/// left in the stage. The stage keeps the preview annotations on purpose; the
/// ship transform strips them. Nothing downstream can recover that hash — the
/// stage holds the wrong bytes and the cache blob is keyed by SHA-256 of the
/// unstripped ones — so the injector computes it while it still holds them, and
/// the registration site never opens the file it is registering.
#[test]
fn inject_slots_receipt_hashes_the_stripped_bytes_not_the_staged_ones() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    let (objects, transforms) = slot_test_cache(cache_dir.path());
    let html = r#"<html><head><!-- slot:head-end --></head><body><p data-source-line="3">x</p></body></html>"#;
    std::fs::write(dir.path().join("index.html"), html).unwrap();
    let slots = head_end_slots("<style>h1{}</style>");

    let changed =
        inject_slots_into_directory_cached(dir.path(), dir.path(), &slots, &objects, &transforms)
            .unwrap();

    let staged = std::fs::read(dir.path().join("index.html")).unwrap();
    assert!(
        String::from_utf8_lossy(&staged).contains("data-source-line"),
        "the stage keeps its annotations — that is what makes this test meaningful"
    );
    let expected = crate::build::assets::paths::compute_binary_hash(
        &crate::build::ship::apply_transform(
            crate::build::ship::transform_for("index.html"),
            &staged,
        ),
    );
    assert_eq!(changed, vec![("index.html".to_string(), expected.clone())]);
    assert_ne!(
        expected,
        crate::build::assets::paths::compute_binary_hash(&staged),
        "a hash taken by reading the stage back would have been this one"
    );
}

#[test]
fn inject_slots_into_directory_cached_no_op_result_is_cached_too() {
    let src_dir_a = tempfile::tempdir().unwrap();
    let src_dir_b = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    let (objects, transforms) = slot_test_cache(cache_dir.path());
    // No markers at all — injection is a no-op (content_oid: None cache path).
    let html = "<html><head></head><body>no markers</body></html>";
    std::fs::write(src_dir_a.path().join("index.html"), html).unwrap();
    std::fs::write(src_dir_b.path().join("index.html"), html).unwrap();
    let slots = ResolvedSlots::empty();

    inject_slots_into_directory_cached(src_dir_a.path(), src_dir_a.path(), &slots, &objects, &transforms).unwrap();
    // Prove the cache-hit no-op path does not overwrite the file: mutate
    // it between the miss and the hit, then confirm the hit leaves the
    // mutation untouched (a real rewrite would clobber it back to `html`).
    std::fs::write(src_dir_b.path().join("index.html"), "MUTATED").unwrap();
    let changed_b =
        inject_slots_into_directory_cached(src_dir_b.path(), src_dir_b.path(), &slots, &objects, &transforms)
            .unwrap();

    assert!(changed_b.is_empty());
    let out_b = std::fs::read_to_string(src_dir_b.path().join("index.html")).unwrap();
    assert_eq!(out_b, "MUTATED");
}

#[test]
fn slot_inject_record_residual_round_trips_through_the_cache() {
    // `inject_slots` always resolves (replaces or strips) every known
    // marker, so a residual marker can never survive a REAL injection
    // call — `residual_known_slot_markers` guards a coverage bug (some
    // file skipping injection entirely), not ordinary no-content
    // markers. That means the only way to exercise the cache-hit
    // residual path is to write a record with a non-empty `residual`
    // directly (as a miss legitimately would if that bug occurred) and
    // confirm it reads back intact — which is exactly the plumbing
    // `inject_slots_into_directory_cached`'s hit branch depends on to
    // still call `log_residual_slot_markers` on a hit (moss#919 item 2,
    // prior-art requirement from `4dca6d3fc`).
    let cache_dir = tempfile::tempdir().unwrap();
    let (objects, transforms) = slot_test_cache(cache_dir.path());
    let params = serde_json::json!({ "page_path": "index.html", "slots_hash": "test" });
    let record = SlotInjectRecord {
        injected: None,
        residual: vec!["footer-end".to_string()],
    };
    write_slot_inject_record(&objects, &transforms, "xxh3:deadbeef", 4, &params, &record);

    let record_oid = transforms
        .find_cached_output("xxh3:deadbeef", SLOT_INJECT_TRANSFORM, &params)
        .expect("cache hit");
    let read_back = read_slot_inject_record(&objects, &record_oid).expect("record readable");
    assert_eq!(read_back.residual, vec!["footer-end".to_string()]);
}

// --- merge_all tests ---

#[test]
fn merge_all_combines_slots_from_two_sources() {
    let mut native = ResolvedSlots::empty();
    native.merge(
        &EnhanceResult {
            success: true,
            slots: HashMap::from([(
                "head-end".to_string(),
                EnhanceContent::Static {
                    html: "<script>beacon</script>".to_string(),
                },
            )]),
        },
        0,
        "__native_beacon",
    );

    let mut plugin = ResolvedSlots::empty();
    plugin.merge(
        &EnhanceResult {
            success: true,
            slots: HashMap::from([(
                "body-end".to_string(),
                EnhanceContent::Static {
                    html: "<script>plugin</script>".to_string(),
                },
            )]),
        },
        10,
        "my-plugin",
    );

    native.merge_all(plugin);

    assert!(native.get_html("head-end", "/").unwrap().contains("beacon"));
    assert!(native.get_html("body-end", "/").unwrap().contains("plugin"));
}

#[test]
fn merge_all_preserves_priority_ordering() {
    let mut native = ResolvedSlots::empty();
    native.merge(
        &EnhanceResult {
            success: true,
            slots: HashMap::from([(
                "head-end".to_string(),
                EnhanceContent::Static {
                    html: "NATIVE".to_string(),
                },
            )]),
        },
        0, // priority 0 = highest
        "__native",
    );

    let mut plugin = ResolvedSlots::empty();
    plugin.merge(
        &EnhanceResult {
            success: true,
            slots: HashMap::from([(
                "head-end".to_string(),
                EnhanceContent::Static {
                    html: "PLUGIN".to_string(),
                },
            )]),
        },
        10, // priority 10 = lower
        "plugin",
    );

    native.merge_all(plugin);

    let html = native.get_html("head-end", "/").unwrap();
    // Native (priority 0) should come before plugin (priority 10)
    assert!(
        html.starts_with("NATIVE"),
        "Expected NATIVE first, got: {}",
        html
    );
    assert!(
        html.ends_with("PLUGIN"),
        "Expected PLUGIN last, got: {}",
        html
    );
}

#[test]
fn merge_all_empty_into_empty() {
    let mut a = ResolvedSlots::empty();
    let b = ResolvedSlots::empty();
    a.merge_all(b);
    assert!(a.is_empty());
}

#[test]
fn merge_all_empty_into_populated() {
    let mut native = ResolvedSlots::empty();
    native.merge(
        &EnhanceResult {
            success: true,
            slots: HashMap::from([(
                "head-end".to_string(),
                EnhanceContent::Static {
                    html: "NATIVE".to_string(),
                },
            )]),
        },
        0,
        "__native",
    );

    let empty = ResolvedSlots::empty();
    native.merge_all(empty);

    assert_eq!(native.get_html("head-end", "/").unwrap(), "NATIVE");
}

// --- wrap_style_blocks_in_layer tests ---

#[test]
fn wrap_style_blocks_no_style_tag_unchanged() {
    let html = "<script>console.log('hi')</script>";
    assert_eq!(wrap_style_blocks_in_layer(html), html);
}

#[test]
fn wrap_style_blocks_wraps_single_style() {
    let html = "<style>body{color:red}</style>";
    let out = wrap_style_blocks_in_layer(html);
    assert_eq!(out, "<style>@layer plugins{body{color:red}}</style>");
}

#[test]
fn wrap_style_blocks_preserves_style_attributes() {
    let html = r#"<style class="x">a{}</style>"#;
    let out = wrap_style_blocks_in_layer(html);
    assert_eq!(out, r#"<style class="x">@layer plugins{a{}}</style>"#);
}

#[test]
fn wrap_style_blocks_leaves_non_style_html_intact() {
    let html = "<meta name=\"foo\"><style>h1{}</style><script>x</script>";
    let out = wrap_style_blocks_in_layer(html);
    assert_eq!(
        out,
        "<meta name=\"foo\"><style>@layer plugins{h1{}}</style><script>x</script>"
    );
}

#[test]
fn wrap_style_blocks_wraps_multiple_style_blocks() {
    let html = "<style>a{}</style><style>b{}</style>";
    let out = wrap_style_blocks_in_layer(html);
    assert_eq!(
        out,
        "<style>@layer plugins{a{}}</style><style>@layer plugins{b{}}</style>"
    );
}

// --- wrap_plugin_head_end_css_in_layer tests ---

#[test]
fn wrap_plugin_head_end_wraps_static_style() {
    let mut slots = ResolvedSlots::empty();
    let result = EnhanceResult {
        success: true,
        slots: HashMap::from([(
            "head-end".to_string(),
            EnhanceContent::Static {
                html: "<style>body{color:blue}</style>".to_string(),
            },
        )]),
    };
    slots.merge(&result, 10, "some-plugin");
    let wrapped = wrap_plugin_head_end_css_in_layer(slots);
    let html = wrapped.get_html("head-end", "/").unwrap();
    assert_eq!(html, "<style>@layer plugins{body{color:blue}}</style>");
}

#[test]
fn wrap_plugin_head_end_leaves_non_css_intact() {
    let mut slots = ResolvedSlots::empty();
    let result = EnhanceResult {
        success: true,
        slots: HashMap::from([(
            "head-end".to_string(),
            EnhanceContent::Static {
                html: "<script>window.foo=1</script>".to_string(),
            },
        )]),
    };
    slots.merge(&result, 10, "some-plugin");
    let wrapped = wrap_plugin_head_end_css_in_layer(slots);
    let html = wrapped.get_html("head-end", "/").unwrap();
    assert_eq!(html, "<script>window.foo=1</script>");
}

#[test]
fn wrap_plugin_head_end_does_not_touch_other_slots() {
    let mut slots = ResolvedSlots::empty();
    let result = EnhanceResult {
        success: true,
        slots: HashMap::from([(
            "body-end".to_string(),
            EnhanceContent::Static {
                html: "<style>footer{}</style>".to_string(),
            },
        )]),
    };
    slots.merge(&result, 10, "some-plugin");
    let wrapped = wrap_plugin_head_end_css_in_layer(slots);
    // body-end is NOT wrapped
    let html = wrapped.get_html("body-end", "/").unwrap();
    assert_eq!(html, "<style>footer{}</style>");
}

#[test]
fn wrap_plugin_head_end_per_page_css_wrapped() {
    let mut slots = ResolvedSlots::empty();
    let mut pages = HashMap::new();
    pages.insert("/post/".to_string(), "<style>.post{}</style>".to_string());
    pages.insert("/".to_string(), "<style>.home{}</style>".to_string());
    let result = EnhanceResult {
        success: true,
        slots: HashMap::from([("head-end".to_string(), EnhanceContent::PerPage { pages })]),
    };
    slots.merge(&result, 10, "some-plugin");
    let wrapped = wrap_plugin_head_end_css_in_layer(slots);
    assert_eq!(
        wrapped.get_html("head-end", "/").unwrap(),
        "<style>@layer plugins{.home{}}</style>"
    );
    assert_eq!(
        wrapped.get_html("head-end", "/post/").unwrap(),
        "<style>@layer plugins{.post{}}</style>"
    );
}
