use super::*;

#[test]
fn parses_percent_size() {
    let p = parse_params("80%");
    assert_eq!(p.size, Some("80%".to_string()));
    assert_eq!(p.limit, None);
}

#[test]
fn parses_box_size() {
    let p = parse_params("800x600");
    assert_eq!(p.size, Some("800x600".to_string()));
    assert_eq!(p.limit, None);
}

#[test]
fn parses_vh_size() {
    assert_eq!(parse_params("80vh").size, Some("80vh".to_string()));
}

#[test]
fn bare_px_token_is_not_recognized() {
    // `400px` is NOT a recognized size here because the shared
    // `Sizing::parse` splits on the literal `x` — `400px` → `("400p","")`
    // — and rejects both halves. This is a quirk of the shared parser
    // (the wikilink iframe dispatcher inherits the same gap), so a folder
    // embed `|400px` stays a no-op bare flag (unsized iframe), consistent
    // with `![[file.html|400px]]`. Plain `400` or `400%`/`80vh` work.
    assert_eq!(parse_params("400px").size, None);
}

#[test]
fn bare_integer_is_not_size_and_not_limit() {
    // Collision guard: a bare integer must NOT become a size (it stays a
    // no-op bare flag), and it never set limit (only `limit:N` does).
    let p = parse_params("5");
    assert_eq!(p.size, None, "bare int must not be a size");
    assert_eq!(p.limit, None, "bare int must not set limit");
}

#[test]
fn size_coexists_with_limit_key() {
    let p = parse_params("limit:3,80%");
    assert_eq!(p.limit, Some(3));
    assert_eq!(p.size, Some("80%".to_string()));
}

#[test]
fn marker_roundtrips() {
    let p = FolderEmbedParams {
        limit: Some(3),
        sort: Some(SortAxis::Date),
        ..Default::default()
    };
    let m = emit_marker("/journal/", "index.md", &p);
    assert!(m.starts_with(MARKER_FOLDER_LIST));
    assert!(m.contains("path=/journal/"));
    assert!(m.contains("from=index.md"));
    assert!(m.contains("limit=3"));
    assert!(!m.contains("more"));
    assert!(m.contains("sort=date"));
    assert!(m.ends_with(MARKER_END));
}

#[test]
fn marker_roundtrips_new_fields() {
    let p = FolderEmbedParams {
        style: Some("grid".to_string()),
        depth: Some("all".to_string()),
        group: Some("year".to_string()),
        limit: Some(5),
        size: Some("80%".to_string()),
        ..Default::default()
    };
    let m = emit_marker("/p/", "index.md", &p);
    assert!(m.contains("style=grid"));
    assert!(m.contains("depth=all"));
    assert!(m.contains("group=year"));
    assert!(m.contains("limit=5"));
    assert!(m.contains("size=80%"));
    assert!(!m.contains("more"));
}

#[test]
fn marker_omits_size_when_absent() {
    let p = FolderEmbedParams::default();
    let m = emit_marker("/p/", "index.md", &p);
    assert!(!m.contains("size="));
}

#[test]
fn parses_covers_only() {
    let p = parse_params("depth:all,covers:only,limit:6");
    assert_eq!(p.covers, Some("only".to_string()));
    assert_eq!(p.depth, Some("all".to_string()));
    assert_eq!(p.limit, Some(6));
}

#[test]
fn marker_roundtrips_covers() {
    let p = FolderEmbedParams {
        covers: Some("only".to_string()),
        ..Default::default()
    };
    let m = emit_marker("/p/", "index.md", &p);
    assert!(m.contains("covers=only"));
}

#[test]
fn parses_more_target() {
    let p = parse_params("limit:2,more:Archive");
    assert_eq!(p.more, Some("Archive".to_string()));
    assert_eq!(p.limit, Some(2));
}

#[test]
fn bare_more_flag_with_no_target_is_ignored() {
    // The legacy bare `more` (no `:target`) stays a no-op — only the keyed
    // `more:target` form sets the field.
    let p = parse_params("more");
    assert_eq!(p.more, None);
}

#[test]
fn keyed_more_with_empty_target_is_ignored() {
    // `more:` (colon present, nothing after it) must stay a no-op the same
    // way the bare flag is — an empty target resolves to nothing and would
    // otherwise reach folder_embed's render_one as `Some("")`, which fails
    // resolve_more_link_target and fires its unresolved-target warning on
    // an empty string. synthesize_children_marker already filters an empty
    // `children_more` the same way; the embed grammar needs the same guard.
    let p = parse_params("limit:2,more:");
    assert_eq!(p.more, None);
}

#[test]
fn marker_roundtrips_more() {
    let p = FolderEmbedParams {
        more: Some("Archive".to_string()),
        ..Default::default()
    };
    let m = emit_marker("/p/", "index.md", &p);
    assert!(m.contains("more=Archive"));
}
