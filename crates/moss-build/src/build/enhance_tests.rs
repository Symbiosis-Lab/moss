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

/// The two values the pass files a page's record under: the hash of the raw
/// bytes the render left in the stage, and the params a hit is compared on.
fn slot_cache_key(html: &str, page_path: &str, slots: &ResolvedSlots) -> (String, serde_json::Value) {
    let source_oid = format!("xxh3:{}", crate::build::assets::paths::compute_binary_hash(html.as_bytes()));
    let params = serde_json::json!({
        "page_path": page_path,
        "slots_hash": resolved_slots_hash(slots),
        "ship_rev": crate::build::ship::SHIP_TRANSFORM_REV,
    });
    (source_oid, params)
}

/// The record a pass left in the cache for one page, found and read back the way
/// the next pass finds it. `html` is the page as the render wrote it, before the
/// pass touched it.
fn cached_slot_record(
    objects: &crate::build::cache::ObjectStore,
    transforms: &crate::build::cache::TransformCache,
    html: &str,
    page_path: &str,
    slots: &ResolvedSlots,
) -> Option<SlotInjectRecord> {
    let (source_oid, params) = slot_cache_key(html, page_path, slots);
    transforms
        .find_cached_output(&source_oid, SLOT_INJECT_TRANSFORM, &params)
        .and_then(|record_oid| read_slot_inject_record(objects, &record_oid))
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
    assert!(changed_a[0].content_oid.is_some(), "an injected page reports the blob it minted");
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
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].page_path, "index.html");
    assert_eq!(changed[0].manifest_hash, expected);
    assert_ne!(
        expected,
        crate::build::assets::paths::compute_binary_hash(&staged),
        "a hash taken by reading the stage back would have been this one"
    );
}

/// The receipt of a page injection leaves alone: the blob holds its bytes, the
/// hash is that of the SHIPPED bytes, and a hit takes both from the record — no
/// hash is recomputed. Replaces a test that mutated the file between runs and so
/// changed the cache key, exercising a miss both times.
///
/// The record is overwritten with a sentinel hash between the runs: a warm run
/// that answered by rehashing would report the real one.
#[test]
fn a_no_op_page_reports_its_receipt_cold_and_from_the_cache_warm() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    let (objects, transforms) = slot_test_cache(cache_dir.path());
    // No markers at all, and an annotation the shipped bytes lose.
    let html = r#"<html><head></head><body><p data-source-line="2">no markers</p></body></html>"#;
    std::fs::write(dir.path().join("index.html"), html).unwrap();
    let slots = ResolvedSlots::empty();
    let run = || {
        inject_slots_into_directory_cached(dir.path(), dir.path(), &slots, &objects, &transforms).unwrap()
    };

    let cold = run();
    assert_eq!(cold.len(), 1);
    let oid = cold[0].content_oid.clone().expect("a no-op page still gets a blob");
    assert_eq!(
        std::fs::read(objects.get_path(&oid).unwrap()).unwrap(),
        html.as_bytes(),
        "the blob holds the page exactly as staged"
    );
    assert_eq!(
        cold[0].manifest_hash,
        crate::build::assets::paths::compute_binary_hash(&crate::build::ship::apply_transform(
            crate::build::ship::transform_for("index.html"),
            html.as_bytes(),
        )),
        "the hash is of what the site serves, annotations stripped"
    );
    assert_eq!(std::fs::read_to_string(dir.path().join("index.html")).unwrap(), html, "and the page is untouched");

    let (source_oid, params) = slot_cache_key(html, "index.html", &slots);
    write_slot_inject_record(
        &objects,
        &transforms,
        &source_oid,
        html.len() as u64,
        &params,
        &SlotInjectRecord {
            content_oid: oid.clone(),
            manifest_hash: "SENTINEL".to_string(),
            rewritten: false,
            residual: vec![],
        },
    );

    let warm = run();
    assert_eq!(warm.len(), 1);
    assert_eq!(warm[0].manifest_hash, "SENTINEL", "a hit must answer from the record, not rehash");
    assert_eq!(warm[0].content_oid.as_deref(), Some(oid.as_str()));
}

/// What a cold pass caches is what the next pass may answer from, so it has to
/// say the same thing the pass just returned. Nothing else reads the record the
/// pass itself wrote: the sentinel tests overwrite it before their warm run, and
/// the byte-for-byte test gets the same output from a hit as from a miss. A pass
/// that never wrote a record, or wrote its fields into the wrong slots (so every
/// later hit is refused and the page silently re-injects), left them all green.
///
/// One page injection rewrites and one it leaves alone, since `rewritten` is the
/// field that tells the arms apart.
#[test]
fn a_cold_pass_caches_a_record_that_matches_the_receipt_it_returned() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    let (objects, transforms) = slot_test_cache(cache_dir.path());
    let marked = "<html><head><!-- slot:head-end --></head><body></body></html>";
    let plain = r#"<html><head></head><body><p data-source-line="2">no markers</p></body></html>"#;
    std::fs::write(dir.path().join("marked.html"), marked).unwrap();
    std::fs::write(dir.path().join("plain.html"), plain).unwrap();
    let slots = head_end_slots("<style>h1{}</style>");

    let receipts =
        inject_slots_into_directory_cached(dir.path(), dir.path(), &slots, &objects, &transforms).unwrap();

    assert_eq!(receipts.len(), 2);
    for (page, raw, injected) in [("marked.html", marked, true), ("plain.html", plain, false)] {
        let receipt = receipts.iter().find(|r| r.page_path == page).expect("a receipt per page");
        let record = cached_slot_record(&objects, &transforms, raw, page, &slots)
            .unwrap_or_else(|| panic!("{page}: the pass returned a receipt but cached nothing to answer from"));
        assert_eq!(
            Some(&record.content_oid),
            receipt.content_oid.as_ref(),
            "{page}: the record must name the blob the receipt names"
        );
        assert_eq!(record.manifest_hash, receipt.manifest_hash, "{page}: and carry the hash the receipt carries");
        assert_eq!(record.rewritten, injected, "{page}: `rewritten` says whether injection changed the bytes");
        assert!(record.residual.is_empty(), "{page}: nothing was left unresolved");
    }
}

/// The record the cold pass wrote is one the next pass actually answers from.
/// Reading it back (the test above) shows its fields are right; this shows the
/// next pass takes them. A record filed where the lookup does not look, or whose
/// blob and hash sit in each other's field, is refused as a miss, the page is
/// injected again, and the output is byte-identical to a hit's — so nothing about
/// the result can tell the two apart.
///
/// The blob the record names is overwritten with a marker, which a hit links into
/// the stage and a miss (re-injecting from the render) never would. The record
/// itself stays the pass's own, unlike the sentinel tests.
#[test]
fn the_record_a_cold_pass_wrote_is_what_the_next_pass_ships() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    let (objects, transforms) = slot_test_cache(cache_dir.path());
    let render = "<html><head><!-- slot:head-end --></head><body></body></html>";
    let page = dir.path().join("index.html");
    std::fs::write(&page, render).unwrap();
    let slots = head_end_slots("<style>h1{}</style>");
    let run = || {
        inject_slots_into_directory_cached(dir.path(), dir.path(), &slots, &objects, &transforms).unwrap()
    };

    let cold = run();
    let oid = cold[0].content_oid.clone().expect("an injected page gets a blob");
    // The next build renders the page afresh: markers back in the stage.
    std::fs::write(&page, render).unwrap();
    std::fs::write(objects.get_path(&oid).unwrap(), "PLANTED IN THE BLOB").unwrap();

    let warm = run();

    assert_eq!(
        std::fs::read_to_string(&page).unwrap(),
        "PLANTED IN THE BLOB",
        "a hit links the cached blob into the stage; a miss would have injected the render again"
    );
    assert_eq!(warm, cold, "and reports the receipt the cold pass returned");
}

/// A page injection leaves alone stays alone: the warm pass answers from the
/// cache without writing the stage file. A watch rebuild carries most pages
/// through this arm, so a hit that relinked every one of them from its blob would
/// hand the file the same bytes under a new mtime on every build, and everything
/// downstream that watches the stage would see a change that is not one.
///
/// The file's mtime is pinned to the distant past before the warm pass, so a
/// rewrite of any kind moves it and no timing has to be waited out.
#[test]
fn a_warm_no_op_page_is_left_untouched_in_the_stage() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    let (objects, transforms) = slot_test_cache(cache_dir.path());
    let page = dir.path().join("index.html");
    std::fs::write(&page, "<html><head></head><body>no markers</body></html>").unwrap();
    let slots = ResolvedSlots::empty();
    let run = || {
        inject_slots_into_directory_cached(dir.path(), dir.path(), &slots, &objects, &transforms).unwrap()
    };

    let cold = run();
    let long_ago = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
    std::fs::OpenOptions::new().write(true).open(&page).unwrap().set_modified(long_ago).unwrap();

    let warm = run();

    assert_eq!(warm, cold, "the warm pass reports the same receipt");
    assert_eq!(
        std::fs::metadata(&page).unwrap().modified().unwrap(),
        long_ago,
        "the warm pass rewrote a page that injection had not changed"
    );
}

/// Slot content with 8 keys in every map, nested ones included. `ResolvedSlots`
/// is rebuilt from scratch each build and its `HashMap`s iterate in a per-instance
/// random order; with 8 keys (40320 orderings) two constructions differ in order
/// almost every time, so a hash that leaks the order fails on the first pair
/// rather than on an unlucky run. `edit` is appended to one slot's HTML.
fn slots_with_eight_keys_per_map(edit: &str) -> ResolvedSlots {
    let mut content: HashMap<String, EnhanceContent> = (0..8)
        .map(|i| (format!("slot-{i}"), EnhanceContent::Static { html: format!("<p>{i}{edit}</p>") }))
        .collect();
    content.insert(
        "per-page".to_string(),
        EnhanceContent::PerPage { pages: (0..8).map(|i| (format!("/p{i}/"), format!("<b>{i}</b>"))).collect() },
    );
    content.insert(
        "per-language".to_string(),
        EnhanceContent::PerLanguage {
            default: Some("<i>default</i>".to_string()),
            by_lang: (0..8).map(|i| (format!("lang-{i}"), format!("<i>{i}</i>"))).collect(),
        },
    );
    let mut slots = ResolvedSlots::empty();
    slots.merge(&EnhanceResult { success: true, slots: content }, 10, "test");
    slots
}

/// The hash keys the slot-injection cache, so identical slot content must hash
/// identically however its maps happen to iterate. Fifty independent
/// constructions: a hash that leaks iteration order gives ~50 distinct values.
/// The second assertion keeps a degenerate "canonical" hash honest — a constant
/// would also pass the first.
#[test]
fn slots_hash_does_not_depend_on_map_iteration_order() {
    let distinct: std::collections::HashSet<String> =
        (0..50).map(|_| resolved_slots_hash(&slots_with_eight_keys_per_map(""))).collect();
    assert_eq!(
        distinct.len(),
        1,
        "the same slot content hashed to {} different values across 50 constructions",
        distinct.len()
    );
    assert_ne!(
        resolved_slots_hash(&slots_with_eight_keys_per_map("")),
        resolved_slots_hash(&slots_with_eight_keys_per_map("-edited")),
        "the hash must still tell different slot content apart"
    );
}

/// What that buys: a build that reconstructs identical slots must hit the
/// injection cache, not miss on every page. Pins the call site, which the test
/// above cannot see — a pass that stopped keying on `resolved_slots_hash` would
/// leave that one green.
///
/// The record is overwritten with a sentinel before each run, as in the no-op
/// receipt test above, because a miss writes the real hash back over it.
#[test]
fn rebuilt_identical_slots_hit_the_injection_cache() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    let (objects, transforms) = slot_test_cache(cache_dir.path());
    let html = "<html><head></head><body>no markers</body></html>";
    std::fs::write(dir.path().join("index.html"), html).unwrap();

    let first = slots_with_eight_keys_per_map("");
    let cold =
        inject_slots_into_directory_cached(dir.path(), dir.path(), &first, &objects, &transforms).unwrap();
    let oid = cold[0].content_oid.clone().expect("a no-op page still gets a blob");
    let (source_oid, params) = slot_cache_key(html, "index.html", &first);

    for build in 1..=10 {
        write_slot_inject_record(
            &objects,
            &transforms,
            &source_oid,
            html.len() as u64,
            &params,
            &SlotInjectRecord {
                content_oid: oid.clone(),
                manifest_hash: "SENTINEL".to_string(),
                rewritten: false,
                residual: vec![],
            },
        );
        let rebuilt = slots_with_eight_keys_per_map("");
        let warm =
            inject_slots_into_directory_cached(dir.path(), dir.path(), &rebuilt, &objects, &transforms).unwrap();
        assert_eq!(
            warm[0].manifest_hash, "SENTINEL",
            "build {build}: identical slot content, rebuilt from scratch, missed the cache"
        );
    }
}

/// A record can outlive its blob (GC). A receipt naming a missing blob would
/// hand `ship_phase` nothing to read, so the hit is refused and the page takes
/// the miss path, which stores the bytes again.
///
/// The record is the one the pass wrote, and the test checks it is there and
/// still readable once the blob is gone. Without a record the next pass misses
/// and re-stores the blob whether or not the hit is refused, so the final
/// assertions would hold for a pass that never cached anything.
#[test]
fn a_record_naming_a_collected_blob_is_a_miss_that_stores_it_again() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    let (objects, transforms) = slot_test_cache(cache_dir.path());
    let html = "<html><head></head><body>no markers</body></html>";
    std::fs::write(dir.path().join("index.html"), html).unwrap();
    let slots = ResolvedSlots::empty();

    let cold =
        inject_slots_into_directory_cached(dir.path(), dir.path(), &slots, &objects, &transforms).unwrap();
    let oid = cold[0].content_oid.clone().unwrap();
    let cached = cached_slot_record(&objects, &transforms, html, "index.html", &slots)
        .expect("fixture: the cold pass cached its page");
    assert_eq!(cached.content_oid, oid, "fixture: the record names the blob about to be collected");
    std::fs::remove_file(objects.get_path(&oid).unwrap()).unwrap();
    assert!(objects.get_path(&oid).is_none(), "fixture: the blob is gone");
    assert!(
        cached_slot_record(&objects, &transforms, html, "index.html", &slots).is_some(),
        "fixture: the record outlived it, so only the blob's absence can make the next pass miss"
    );

    let warm =
        inject_slots_into_directory_cached(dir.path(), dir.path(), &slots, &objects, &transforms).unwrap();

    assert_eq!(warm[0].content_oid.as_deref(), Some(oid.as_str()));
    assert!(objects.get_path(&oid).is_some(), "the miss path must put the blob back");
}

/// A full or unwritable object store must not fail the pass: it costs each page
/// its `content_oid`, and the page — injected or not — still gets its hash and its
/// place in the stage. Both arms, because they store through the same call.
#[test]
fn an_unwritable_object_store_costs_a_page_its_oid_not_the_pass() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    // A file where the store's directory should be: every write beneath it fails.
    let blocker = cache_dir.path().join("not-a-dir");
    std::fs::write(&blocker, "x").unwrap();
    let objects = crate::build::cache::ObjectStore::new(blocker.join("objects"));
    let transforms = crate::build::cache::TransformCache::new(
        cache_dir.path().join("transforms"),
        crate::build::cache::ObjectStore::new(blocker.join("objects")),
    );
    std::fs::write(
        dir.path().join("marked.html"),
        "<html><head><!-- slot:head-end --></head><body></body></html>",
    )
    .unwrap();
    std::fs::write(dir.path().join("plain.html"), "<html><body>no markers</body></html>").unwrap();

    let mut receipts = inject_slots_into_directory_cached(
        dir.path(),
        dir.path(),
        &head_end_slots("<style>h1{}</style>"),
        &objects,
        &transforms,
    )
    .expect("a store that cannot take blobs must not fail the pass");
    receipts.sort_by(|a, b| a.page_path.cmp(&b.page_path));

    assert_eq!(receipts.len(), 2);
    for receipt in &receipts {
        assert_eq!(receipt.content_oid, None, "{}: nothing to name", receipt.page_path);
        let staged = std::fs::read(dir.path().join(&receipt.page_path)).unwrap();
        assert_eq!(
            receipt.manifest_hash,
            crate::build::assets::paths::compute_binary_hash(&crate::build::ship::apply_transform(
                crate::build::ship::transform_for(&receipt.page_path),
                &staged,
            )),
            "{}: the hash still describes the staged bytes",
            receipt.page_path
        );
    }
    assert!(std::fs::read_to_string(dir.path().join("marked.html")).unwrap().contains("<style>h1{}</style>"));
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
        content_oid: "0".repeat(64),
        manifest_hash: "0".repeat(16),
        rewritten: false,
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
