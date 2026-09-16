use super::*;

/// The invariant the whole module rests on: the hash in the `<link href>` must
/// be computed over the bytes the file receives.
///
/// These are two call paths — `link_tag` and `emit` — over one `wrapped()`. If
/// a future change gives either its own copy of the wrapping, every page of
/// every site with comments links a stylesheet that 404s, and nothing else in
/// the suite notices because both halves are individually self-consistent.
#[test]
fn the_linked_url_names_the_bytes_that_get_written() {
    for style in FEATURE_STYLES {
        let bytes = wrapped(style);
        let hash = compute_content_hash(&bytes);
        let expected = ServedPath::for_feature_stylesheet_hashed(style.name, &hash)
            .unwrap()
            .to_relative_url();
        assert!(
            link_tag(style.name).contains(&expected),
            "{}: link_tag must name the hash of the emitted bytes",
            style.name
        );
    }
}

/// Every feature stylesheet on disk has a row. A file added without one is
/// simply never emitted — invisible until someone opens the feature.
#[test]
fn every_feature_css_file_has_a_row() {
    // The three feature sheets live directly in assets/css/; the always-shipped
    // `site.css` and `mark.css` (the latter also `@import`ed by the launcher)
    // and the gated `site/` partials are `emit::stylesheet`'s.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/assets/css");
    let known: Vec<&str> = FEATURE_STYLES.iter().map(|s| s.name).collect();
    let owned_elsewhere = ["site", "mark"];
    let mut unregistered = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("assets/css must exist") {
        let path = entry.expect("readable dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("css") {
            continue;
        }
        let stem = path.file_stem().unwrap().to_str().unwrap().to_string();
        if owned_elsewhere.contains(&stem.as_str()) || known.contains(&stem.as_str()) {
            continue;
        }
        unregistered.push(stem);
    }
    assert!(
        unregistered.is_empty(),
        "stylesheets with no FEATURE_STYLES row (they will never be emitted): {unregistered:?}"
    );
}

/// The emitted file carries its cascade layer. Without the wrapper these rules
/// land unlayered and outrank the entire layered sheet — including the user's
/// own `.moss/theme/style.css`, which is the thing `@layer plugins` exists to
/// stay below.
#[test]
fn every_feature_sheet_is_wrapped_in_layer_plugins() {
    for style in FEATURE_STYLES {
        let bytes = wrapped(style);
        assert!(
            bytes.starts_with("@layer plugins {"),
            "{} must open @layer plugins",
            style.name
        );
        assert!(bytes.trim_end().ends_with('}'), "{} must close it", style.name);
    }
}

/// Distinct sheets must not collide on a filename. They share the `_moss/css/`
/// mount, so the name is what separates them.
#[test]
fn feature_sheets_have_distinct_names_and_urls() {
    let mut urls = std::collections::BTreeSet::new();
    for style in FEATURE_STYLES {
        assert!(urls.insert(link_tag(style.name)), "{} collides", style.name);
    }
}

/// An unregistered name is a programming error, not a silent no-op: returning
/// an empty tag would ship a page with no styles and no failure.
#[test]
#[should_panic(expected = "no feature stylesheet named")]
fn linking_an_unregistered_sheet_panics() {
    let _ = link_tag("not-a-real-sheet");
}
