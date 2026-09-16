use super::*;

fn ok(s: &str) -> AttrBlock {
    parse_attrs(s).expect("parse_attrs should succeed")
}

fn err(s: &str) -> AttrError {
    parse_attrs(s).expect_err("parse_attrs should fail")
}

// ── empty / no-content cases ─────────────────────────────────────

#[test]
fn empty_block() {
    let b = ok("{}");
    assert!(b.is_empty());
}

#[test]
fn whitespace_only_block() {
    let b = ok("{   }");
    assert!(b.is_empty());
}

#[test]
fn missing_open_brace_errors() {
    assert_eq!(err("foo=bar"), AttrError::MissingOpenBrace);
}

#[test]
fn unclosed_brace_errors() {
    assert_eq!(err("{.foo"), AttrError::UnclosedBrace);
}

// ── classes ──────────────────────────────────────────────────────

#[test]
fn single_class() {
    let b = ok("{.primary}");
    assert_eq!(b.classes, vec!["primary"]);
    assert_eq!(b.class_string(), "primary");
}

#[test]
fn multiple_classes_space_separated() {
    let b = ok("{.primary .large}");
    assert_eq!(b.classes, vec!["primary", "large"]);
    assert_eq!(b.class_string(), "primary large");
}

#[test]
fn classes_preserve_source_order() {
    let b = ok("{.first .second .third}");
    assert_eq!(b.classes, vec!["first", "second", "third"]);
}

#[test]
fn class_with_dash_and_digit() {
    let b = ok("{.btn-primary .v2}");
    assert_eq!(b.classes, vec!["btn-primary", "v2"]);
}

#[test]
fn dot_with_no_name_skipped() {
    // Lonely `.` followed by whitespace produces an empty class — drop it
    // rather than insert "" into the list.
    let b = ok("{. .real}");
    assert_eq!(b.classes, vec!["real"]);
}

// ── id ───────────────────────────────────────────────────────────

#[test]
fn single_id() {
    let b = ok("{#hero}");
    assert_eq!(b.id.as_deref(), Some("hero"));
}

#[test]
fn multiple_ids_last_wins() {
    let b = ok("{#first #second}");
    assert_eq!(b.id.as_deref(), Some("second"));
}

#[test]
fn id_and_class_in_same_block() {
    let b = ok("{#main .container}");
    assert_eq!(b.id.as_deref(), Some("main"));
    assert_eq!(b.classes, vec!["container"]);
}

// ── key/value: bareword values ──────────────────────────────────

#[test]
fn key_with_bareword_integer() {
    let b = ok("{cols=3}");
    assert_eq!(b.get("cols"), Some("3"));
}

#[test]
fn key_with_ratio_value() {
    let b = ok("{cols=1:1:2}");
    assert_eq!(b.get("cols"), Some("1:1:2"));
}

#[test]
fn key_with_path_value() {
    let b = ok("{image=hero.jpg}");
    assert_eq!(b.get("image"), Some("hero.jpg"));
}

#[test]
fn key_with_dotted_path_value() {
    let b = ok("{image=path/to/file.jpg}");
    assert_eq!(b.get("image"), Some("path/to/file.jpg"));
}

#[test]
fn key_with_negative_int_value() {
    let b = ok("{offset=-5}");
    assert_eq!(b.get("offset"), Some("-5"));
}

// A non-ASCII filename is a bareword like any other. When `is_bareword` was
// ASCII-only this parsed as `EmptyValue`, and because every caller does
// `parse_attrs(...).unwrap_or_default()` the WHOLE block was discarded — so
// `:::hero {image=頭像.png .big}` lost its classes and width too, and the hero
// rendered with no image at all. The `.big` assertion is the load-bearing one:
// it fails if the block is being dropped wholesale rather than just the value.
#[test]
fn key_with_non_ascii_bareword_path_value() {
    let b = ok("{image=頭像.png .big}");
    assert_eq!(b.get("image"), Some("頭像.png"));
    assert_eq!(b.classes, vec!["big"]);
}

#[test]
fn multiple_kvs() {
    let b = ok("{cols=3 image=hero.jpg gap=2}");
    assert_eq!(b.get("cols"), Some("3"));
    assert_eq!(b.get("image"), Some("hero.jpg"));
    assert_eq!(b.get("gap"), Some("2"));
}

#[test]
fn duplicate_key_last_wins() {
    let b = ok("{cols=2 cols=4}");
    assert_eq!(b.get("cols"), Some("4"));
}

#[test]
fn key_with_dash_and_underscore() {
    let b = ok("{button-text=foo my_field=bar}");
    assert_eq!(b.get("button-text"), Some("foo"));
    assert_eq!(b.get("my_field"), Some("bar"));
}

// ── key/value: quoted values ────────────────────────────────────

#[test]
fn key_with_quoted_simple() {
    let b = ok(r#"{button="Request access"}"#);
    assert_eq!(b.get("button"), Some("Request access"));
}

#[test]
fn key_with_quoted_punctuation() {
    let b = ok(r#"{description="One email. No newsletter."}"#);
    assert_eq!(b.get("description"), Some("One email. No newsletter."));
}

#[test]
fn quoted_value_with_escaped_quote() {
    let b = ok(r#"{label="say \"hi\""}"#);
    assert_eq!(b.get("label"), Some(r#"say "hi""#));
}

#[test]
fn quoted_value_with_escaped_backslash() {
    let b = ok(r#"{path="C:\\Users\\me"}"#);
    assert_eq!(b.get("path"), Some(r"C:\Users\me"));
}

#[test]
fn quoted_value_with_brace_inside() {
    // The closing `}` of the block must NOT be confused with a `}`
    // inside a quoted value.
    let b = ok(r#"{template="{name}"}"#);
    assert_eq!(b.get("template"), Some("{name}"));
}

#[test]
fn quoted_value_can_be_empty() {
    let b = ok(r#"{label=""}"#);
    assert_eq!(b.get("label"), Some(""));
}

#[test]
fn unterminated_quote_errors() {
    assert_eq!(err(r#"{button="oops}"#), AttrError::UnterminatedQuote);
}

// ── multi-line ───────────────────────────────────────────────────

#[test]
fn multi_line_attrs() {
    let b = ok("{\n  placeholder=\"you@domain.com\"\n  button=\"Request access\"\n}");
    assert_eq!(b.get("placeholder"), Some("you@domain.com"));
    assert_eq!(b.get("button"), Some("Request access"));
}

#[test]
fn multi_line_with_classes_and_kvs() {
    let b = ok("{\n  .primary\n  .large\n  cols=3\n  image=hero.jpg\n}");
    assert_eq!(b.classes, vec!["primary", "large"]);
    assert_eq!(b.get("cols"), Some("3"));
    assert_eq!(b.get("image"), Some("hero.jpg"));
}

#[test]
fn tab_separator_works() {
    let b = ok("{.foo\tcols=3}");
    assert_eq!(b.classes, vec!["foo"]);
    assert_eq!(b.get("cols"), Some("3"));
}

// ── error paths ──────────────────────────────────────────────────

#[test]
fn key_with_no_value_errors() {
    let e = err("{cols=}");
    assert!(matches!(e, AttrError::EmptyValue { ref key } if key == "cols"));
}

#[test]
fn key_without_equals_errors() {
    // A bare keyword without `=` is invalid (spec says only `.foo`,
    // `#foo`, `key=value` are recognized) — EXCEPT for the spec § P9
    // width tokens (body | wide | page | screen | full), which are
    // tested separately below.
    assert!(matches!(err("{cols}"), AttrError::InvalidKey { .. }));
    assert!(matches!(err("{flush}"), AttrError::InvalidKey { .. }));
}

// ── width tokens (spec § P9) ─────────────────────────────────────

#[test]
fn width_token_body() {
    let b = ok("{body}");
    assert_eq!(b.width, Some("body"));
    assert!(b.classes.is_empty());
    assert!(b.id.is_none());
    assert!(b.kvs.is_empty());
}

#[test]
fn width_token_wide() {
    let b = ok("{wide}");
    assert_eq!(b.width, Some("wide"));
}

#[test]
fn width_token_page() {
    let b = ok("{page}");
    assert_eq!(b.width, Some("page"));
}

#[test]
fn width_token_screen() {
    let b = ok("{screen}");
    assert_eq!(b.width, Some("screen"));
}

#[test]
fn width_token_full_aliases_to_screen() {
    // Per spec § P9 authoring grammar: `full` is the author-facing
    // alias for `screen`. The emitted value is always the value-space
    // term `screen`.
    let b = ok("{full}");
    assert_eq!(b.width, Some("screen"));
}

#[test]
fn width_token_with_class() {
    let b = ok("{wide .showcase}");
    assert_eq!(b.width, Some("wide"));
    assert_eq!(b.classes, vec!["showcase"]);
}

#[test]
fn width_token_with_kv() {
    let b = ok("{cols=3 wide}");
    assert_eq!(b.width, Some("wide"));
    assert_eq!(b.get("cols"), Some("3"));
}

#[test]
fn width_token_repeated_last_wins() {
    // Authors are unlikely to do this, but stay deterministic.
    let b = ok("{wide page}");
    assert_eq!(b.width, Some("page"));
}

#[test]
fn width_token_does_not_become_class() {
    let b = ok("{wide}");
    assert!(b.classes.is_empty());
}

#[test]
fn width_token_with_explicit_dot_is_a_class_not_width() {
    // `.wide` is still a class — width tokens are recognized only as
    // bare keywords, not as `.class` shortcuts.
    let b = ok("{.wide}");
    assert_eq!(b.classes, vec!["wide"]);
    assert!(b.width.is_none());
}

#[test]
fn key_starting_with_digit_is_invalid_token() {
    let e = err("{3cols=2}");
    assert!(matches!(e, AttrError::InvalidKey { .. }));
}

#[test]
fn lonely_equals_is_invalid_token() {
    let e = err("{=foo}");
    assert!(matches!(e, AttrError::InvalidKey { .. }));
}

// ── round-trip via class_string / get ────────────────────────────

#[test]
fn class_string_joins_with_spaces() {
    let b = ok("{.alpha .beta .gamma}");
    assert_eq!(b.class_string(), "alpha beta gamma");
}

#[test]
fn class_string_empty_when_no_classes() {
    let b = ok("{cols=3}");
    assert_eq!(b.class_string(), "");
}

#[test]
fn get_returns_none_for_missing_key() {
    let b = ok("{cols=3}");
    assert!(b.get("rows").is_none());
}

// ── realistic spec examples ──────────────────────────────────────

#[test]
fn spec_subscribe_form_multi_line() {
    let b = ok("{\n  placeholder=\"you@domain.com\"\n  button=\"Request access\"\n}");
    assert_eq!(b.get("placeholder"), Some("you@domain.com"));
    assert_eq!(b.get("button"), Some("Request access"));
    assert!(b.classes.is_empty());
    assert!(b.id.is_none());
}

#[test]
fn spec_buttons_classes_only() {
    let b = ok("{.primary .large}");
    assert_eq!(b.classes, vec!["primary", "large"]);
    assert!(b.kvs.is_empty());
}

#[test]
fn spec_gallery_cols_and_class() {
    let b = ok("{cols=3 .showcase}");
    assert_eq!(b.get("cols"), Some("3"));
    assert_eq!(b.classes, vec!["showcase"]);
}

#[test]
fn spec_grid_ratio_cols() {
    let b = ok("{cols=1:1:2}");
    assert_eq!(b.get("cols"), Some("1:1:2"));
}

#[test]
fn spec_hero_image_path() {
    let b = ok("{image=hero.jpg}");
    assert_eq!(b.get("image"), Some("hero.jpg"));
}

#[test]
fn spec_pure_css_region_class_and_id() {
    let b = ok("{.tagline #intro}");
    assert_eq!(b.classes, vec!["tagline"]);
    assert_eq!(b.id.as_deref(), Some("intro"));
}

// ── leading whitespace before brace ──────────────────────────────

#[test]
fn leading_whitespace_before_brace_ok() {
    // The opener-scanner upstream may pass " {.foo}" if it strips name
    // first. Tolerate leading whitespace.
    let b = ok("  {.foo}");
    assert_eq!(b.classes, vec!["foo"]);
}

// ── brace-depth tracker ──────────────────────────────────────────

#[test]
fn brace_depth_tracks_simple_open_close() {
    assert_eq!(brace_depth("{}", 0), 0);
    assert_eq!(brace_depth("{", 0), 1);
    assert_eq!(brace_depth("}", 1), 0);
}

#[test]
fn brace_depth_ignores_braces_in_quoted_strings() {
    assert_eq!(brace_depth(r#"{key="{name}"}"#, 0), 0);
    assert_eq!(brace_depth(r#"{key="{"}"#, 0), 0);
}

#[test]
fn brace_depth_handles_escaped_quote() {
    assert_eq!(brace_depth(r#"{label="say \"hi\""}"#, 0), 0);
}

#[test]
fn brace_depth_clamps_at_zero_for_orphan_close() {
    // A stray `}` with no matching `{` shouldn't go negative —
    // depth-0 input followed by `}` stays 0.
    assert_eq!(brace_depth("}", 0), 0);
}

// ── multi-line attr gather ───────────────────────────────────────

#[test]
fn gather_no_brace_returns_none_zero_consumed() {
    let (out, consumed) = gather_multi_line_attrs("plain text", &["following"]);
    assert!(out.is_none());
    assert_eq!(consumed, 0);
}

#[test]
fn gather_balanced_single_line_returns_none() {
    let (out, consumed) = gather_multi_line_attrs("{.foo}", &["body"]);
    assert!(out.is_none());
    assert_eq!(consumed, 0);
}

#[test]
fn gather_two_line_block_returns_combined() {
    let (out, consumed) = gather_multi_line_attrs("{", &[".foo", "}", "body"]);
    assert_eq!(consumed, 2);
    let combined = out.expect("multi-line should return a combined string");
    let parsed = parse_attrs(&combined).expect("combined attrs should parse");
    assert_eq!(parsed.classes, vec!["foo"]);
}

#[test]
fn gather_three_line_block_with_kvs() {
    let (out, consumed) = gather_multi_line_attrs(
        "{",
        &[
            "  placeholder=\"you@domain.com\"",
            "  button=\"Go\"",
            "}",
            "body",
        ],
    );
    assert_eq!(consumed, 3);
    let parsed = parse_attrs(&out.unwrap()).unwrap();
    assert_eq!(parsed.get("placeholder"), Some("you@domain.com"));
    assert_eq!(parsed.get("button"), Some("Go"));
}

#[test]
fn gather_unclosed_returns_what_it_has() {
    let (out, consumed) = gather_multi_line_attrs("{", &[".foo", ".bar"]);
    // Both lines were consumed, brace never closed.
    assert_eq!(consumed, 2);
    // The combined string is returned even though it's unparseable.
    assert!(out.is_some());
}

#[test]
fn gather_quoted_brace_does_not_count() {
    // A `}` inside a quoted value must not close the block prematurely.
    let (out, consumed) = gather_multi_line_attrs("{", &[r#"  template="say }"#, r#"  end"#, "}"]);
    // Three lines absorbed (the closing brace is on the third).
    assert_eq!(consumed, 3);
    assert!(out.is_some());
}
