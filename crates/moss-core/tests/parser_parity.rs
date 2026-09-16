//! Differential tests: assert that editor_scan() and extract_shortcodes()
//! agree on top-level block counts and names.
//!
//! `editor_scan` returns source byte positions for the CM6 editor. It
//! recognises ALL `:::name` openers including unknown/plugin names and
//! tracks nesting depth, but only emits top-level blocks.
//!
//! `extract_shortcodes` returns typed AST nodes for the build pipeline.
//! It only populates `ExtractionResult::extracted` for the six built-in
//! typed-known names (subscribe, buttons, gallery, hero, grid, recent).
//! Unknown names are rendered as `moss-unknown-shortcode` divs and do NOT
//! appear in `extracted`.
//!
//! Parity check: for input that uses only typed-known names, both parsers
//! must agree on the number of top-level blocks and their names.
//!
//! Note on the `hyphenated-names` fixture: the original spec used
//! `:::my-widget` (a hyphenated unknown name). `editor_scan` correctly
//! returns 1 block while `extract_shortcodes` returns 0 in `extracted`
//! — that is intentional, documented asymmetry. The fixture here uses
//! `:::gallery` instead to exercise the scanner on a standard block.
//! The `editor_scan` unit tests already cover hyphenated unknown names
//! (see `editor_scan::tests::shortcode_with_hyphenated_name`).

use moss_core::ast::editor_scan::editor_scan;
use moss_core::ast::shortcode::ShortcodeKind;
use moss_core::ast::shortcode_extract::extract_shortcodes;

/// Map a [`ShortcodeKind`] to the lowercase name string used in `:::name` syntax.
fn kind_name(kind: ShortcodeKind) -> &'static str {
    match kind {
        ShortcodeKind::Subscribe => "subscribe",
        ShortcodeKind::Buttons => "buttons",
        ShortcodeKind::Gallery => "gallery",
        ShortcodeKind::Hero => "hero",
        ShortcodeKind::Grid => "grid",
        ShortcodeKind::Recent => "recent",
        ShortcodeKind::Apply => "apply",
    }
}

fn check_parity(markdown: &str, fixture_name: &str) {
    let scan_result = editor_scan(markdown);
    let extract_result = extract_shortcodes(markdown);

    let scan_names: Vec<&str> = scan_result.blocks.iter().map(|b| b.name.as_str()).collect();
    let extract_names: Vec<&str> = extract_result
        .extracted
        .iter()
        .map(|e| kind_name(e.shortcode.kind()))
        .collect();

    assert_eq!(
        scan_names.len(),
        extract_names.len(),
        "fixture '{fixture_name}': block count mismatch\n  scan:    {scan_names:?}\n  extract: {extract_names:?}"
    );

    for (i, (sn, en)) in scan_names.iter().zip(extract_names.iter()).enumerate() {
        assert_eq!(
            sn, en,
            "fixture '{fixture_name}' block {i}: name mismatch — scan='{sn}' extract='{en}'"
        );
    }
}

#[test]
fn nested_arity_parity() {
    // :::grid wrapping ::::buttons — editor tracks depth correctly and emits
    // only the outer grid as a top-level block; extractor also extracts only
    // the outer grid (inner buttons body is stored as raw cell text).
    let md = include_str!("../../../tests/fixtures/parser-parity/nested-arity.md");
    check_parity(md, "nested-arity");
}

#[test]
fn gallery_single_block_parity() {
    // Uses :::gallery (a known typed name) to verify single-block agreement.
    // (Fixture file is still named hyphenated-names.md but contains :::gallery;
    //  see module comment for why the original :::my-widget was swapped out.)
    let md = include_str!("../../../tests/fixtures/parser-parity/hyphenated-names.md");
    check_parity(md, "hyphenated-names");
}

#[test]
fn hyphenated_name_editor_scan_recognises_unknown() {
    // Documents the INTENTIONAL asymmetry: editor_scan recognises :::my-widget
    // as a top-level block (name parsing allows hyphens), but extract_shortcodes
    // returns 0 in .extracted because unknown shortcode names are rendered as
    // fallback divs, not typed AST nodes. This is not a parity bug — it is
    // by design. This test locks in the contract so a future refactor doesn't
    // accidentally change one side without the other.
    use moss_core::ast::editor_scan::editor_scan;
    use moss_core::ast::shortcode_extract::extract_shortcodes;
    let md = ":::my-widget\nbody\n:::\n";
    let scan = editor_scan(md);
    let extract = extract_shortcodes(md);
    assert_eq!(scan.blocks.len(), 1, "editor_scan must recognise :::my-widget");
    assert_eq!(scan.blocks[0].name, "my-widget");
    assert_eq!(extract.extracted.len(), 0,
        "extract_shortcodes must return 0 typed nodes for unknown names (intentional asymmetry)");
}

#[test]
fn positional_args_parity() {
    // :::grid with a positional column-count arg and ratio string.
    let md = include_str!("../../../tests/fixtures/parser-parity/positional-args.md");
    check_parity(md, "positional-args");
}

#[test]
fn multiline_attrs_parity() {
    // :::subscribe with a multi-line attribute block. extract_shortcodes
    // joins the attribute lines; editor_scan treats them as body lines. Both
    // must still agree: 1 top-level block named "subscribe".
    let md = include_str!("../../../tests/fixtures/parser-parity/multiline-attrs.md");
    check_parity(md, "multiline-attrs");
}
