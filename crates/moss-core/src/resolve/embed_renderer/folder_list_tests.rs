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
