use super::*;

/// Assert that the substring `needle` (which must appear exactly once) is
/// entirely inert / entirely live.
#[track_caller]
fn assert_inert(md: &str, needle: &str, expected: bool) {
    let at = md.find(needle).expect("needle present in fixture");
    assert_eq!(
        md.matches(needle).count(),
        1,
        "fixture must contain `{needle}` exactly once"
    );
    let regions = InertRegions::scan(md);
    let masked = regions.mask(md);
    let hidden = !masked.contains(needle);
    assert_eq!(
        regions.is_inert(at),
        expected,
        "is_inert({at}) for `{needle}` in:\n{md}\nranges: {:?}",
        regions.ranges()
    );
    assert_eq!(
        hidden, expected,
        "mask() and is_inert() disagree about `{needle}` in:\n{md}\nmasked:\n{masked}"
    );
}

#[test]
fn plain_prose_has_no_inert_regions() {
    let regions = InertRegions::scan("# Title\n\nA paragraph with a [link](/x/).\n");
    assert!(regions.is_empty());
    assert!(!regions.is_inert(0));
}

#[test]
fn mask_preserves_byte_length_and_newlines() {
    let md = "before\n```\n:::grid\n```\nafter `code` end\n";
    let masked = mask_inert(md);
    assert_eq!(masked.len(), md.len(), "mask must be length-preserving");
    assert_eq!(
        masked.lines().count(),
        md.lines().count(),
        "mask must be line-preserving"
    );
    assert!(masked.starts_with("before\n"));
    assert!(!masked.contains(":::grid"));
    assert!(masked.contains("after ") && !masked.contains("`code`"));
}

#[test]
fn mask_is_utf8_safe_across_cjk_prose() {
    // The #903 bug-1 shape: byte scanning that lands inside a multi-byte
    // char. Every ASCII byte we match is a full char, so this must not panic
    // and must not corrupt the CJK text.
    let md = "在場《紀念》— see `代碼` and <!-- 註解 --> 然後結束\n";
    let masked = mask_inert(md);
    assert_eq!(masked.len(), md.len());
    assert!(masked.starts_with("在場《紀念》— see "));
    assert!(masked.ends_with(" 然後結束\n"));
    assert!(!masked.contains("代碼"));
    assert!(!masked.contains("註解"));
}

// ── fenced code blocks ────────────────────────────────────────────────

#[test]
fn backtick_fence_body_is_inert() {
    assert_inert("text\n\n```\n:::grid\n```\n\nmore\n", ":::grid", true);
}

#[test]
fn tilde_fence_body_is_inert() {
    assert_inert("~~~md\n:::grid\n~~~\n", ":::grid", true);
}

#[test]
fn shorter_run_does_not_close_a_longer_fence() {
    // CommonMark: the closing run must be at least as long as the opener,
    // so the inner ``` is content and the `:::` stays inert.
    assert_inert("````\n```\n:::grid\n````\n", ":::grid", true);
}

#[test]
fn unclosed_fence_stays_inert_to_end_of_input() {
    assert_inert("```\nstill code\n:::grid\n", ":::grid", true);
}

#[test]
fn backtick_fence_info_string_may_not_contain_backticks() {
    // `` `a` and `b` `` is an inline-code line, not a fence opener — so the
    // following `:::grid` is live.
    let md = "``` `a` ```\n\n:::grid\n:::\n";
    assert_inert(md, ":::grid", false);
}

// ── indented code blocks ──────────────────────────────────────────────

#[test]
fn indented_code_block_is_inert() {
    assert_inert("intro\n\n    :::grid\n    +++\n\nafter\n", ":::grid", true);
}

#[test]
fn indented_code_block_survives_an_internal_blank_line() {
    let md = "intro\n\n    line one\n\n    :::grid\n\nafter\n";
    assert_inert(md, ":::grid", true);
}

#[test]
fn tab_indent_counts_as_four_columns() {
    assert_inert("intro\n\n\t:::grid\n\nafter\n", ":::grid", true);
}

#[test]
fn indented_code_cannot_interrupt_a_paragraph() {
    // No blank line before it: CommonMark reads this as a lazy paragraph
    // continuation, not code. The shortcode must stay live.
    assert_inert("intro line\n    :::grid\n", ":::grid", false);
}

#[test]
fn indentation_inside_a_list_is_list_content_not_code() {
    // The false-positive that would silently delete an author's shortcode:
    // indented content under a list item.
    let md = "- item one\n\n    :::grid\n    +++\n    [a](/b/)\n";
    assert_inert(md, ":::grid", false);
}

#[test]
fn a_top_level_paragraph_ends_the_list_context() {
    let md = "- item\n\nplain paragraph\n\n    :::grid\n";
    assert_inert(md, ":::grid", true);
}

#[test]
fn ordered_list_marker_also_opens_list_context() {
    assert_inert("1. item\n\n    :::grid\n", ":::grid", false);
}

// ── inline code spans ─────────────────────────────────────────────────

#[test]
fn inline_code_span_is_inert() {
    assert_inert("write `:::grid` to start a grid\n", ":::grid", true);
}

#[test]
fn double_backtick_span_is_inert() {
    assert_inert("write ``:::grid`` inline\n", ":::grid", true);
}

#[test]
fn unmatched_backtick_leaves_the_line_live() {
    assert_inert("a ` stray tick\n\n:::grid\n:::\n", ":::grid", false);
}

#[test]
fn single_tick_does_not_close_a_double_tick_span() {
    let md = "``a ` b`` then :::grid\n";
    assert_inert(md, ":::grid", false);
    assert_inert(md, "a ` b", true);
}

// ── HTML comments (moss#903 bug 2) ────────────────────────────────────

#[test]
fn single_line_html_comment_is_inert() {
    assert_inert("before <!-- :::grid --> after\n", ":::grid", true);
}

#[test]
fn multi_line_html_comment_is_inert() {
    // Verbatim shape from the #903 report.
    let md = "intro\n\n<!-- TODO owner assets:\n     :::gallery 8\n     some-image.jpg\n     ::: -->\n\nreal content\n";
    assert_inert(md, ":::gallery 8", true);
    let flags = inert_lines(md);
    // lines: 0 intro, 1 blank, 2 comment open, 3 gallery, 4 image, 5 close,
    // 6 blank, 7 real content
    assert_eq!(
        flags,
        vec![false, false, true, true, true, true, false, false],
        "only the comment's own lines are inert"
    );
}

#[test]
fn content_after_a_comment_closes_is_live_again() {
    let md = "<!-- note -->\n\n:::grid\n:::\n";
    assert_inert(md, ":::grid", false);
}

#[test]
fn a_second_comment_on_the_closing_line_is_still_inert() {
    let md = "<!-- one --> live <!-- :::grid -->\n";
    assert_inert(md, ":::grid", true);
    assert!(mask_inert(md).contains(" live "));
}

#[test]
fn unclosed_comment_is_inert_to_end_of_input() {
    // Matches CommonMark: an unterminated HTML comment block runs to EOF.
    assert_inert("<!-- oops\n\n:::grid\n:::\n", ":::grid", true);
}

#[test]
fn empty_comment_forms_do_not_swallow_the_document() {
    for md in ["<!-->\n\n:::grid\n:::\n", "<!--->\n\n:::grid\n:::\n"] {
        assert_inert(md, ":::grid", false);
    }
}

#[test]
fn comment_inside_a_fence_does_not_leak_state() {
    // The `<!--` is code, so it must not open a comment that swallows the
    // live shortcode below.
    assert_inert(
        "```\n<!-- unterminated\n```\n\n:::grid\n:::\n",
        ":::grid",
        false,
    );
}

#[test]
fn backtick_inside_a_comment_does_not_open_a_code_span() {
    let md = "<!-- a ` tick -->\n\n:::grid\n:::\n";
    assert_inert(md, ":::grid", false);
}

// ── query helpers ─────────────────────────────────────────────────────

#[test]
fn intersects_reports_partial_overlap() {
    let md = "a `code` b\n";
    let regions = InertRegions::scan(md);
    let start = md.find('`').expect("tick");
    assert!(regions.intersects(0..start + 1), "range ending inside code");
    assert!(regions.intersects(start..start + 2));
    assert!(!regions.intersects(0..start));
    assert!(!regions.intersects(md.len() - 2..md.len()));
}

#[test]
fn inert_lines_length_matches_str_lines() {
    for md in ["", "one", "one\n", "one\n\n", "a\r\nb\r\n", "```\nx\n```"] {
        assert_eq!(
            inert_lines(md).len(),
            md.lines().count(),
            "flag count must align with str::lines for {md:?}"
        );
    }
}

#[test]
fn crlf_input_is_handled() {
    let md = "intro\r\n\r\n```\r\n:::grid\r\n```\r\n";
    assert_inert(md, ":::grid", true);
    assert_eq!(mask_inert(md).len(), md.len());
}
