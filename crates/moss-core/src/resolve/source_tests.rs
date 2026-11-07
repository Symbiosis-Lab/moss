use super::*;

    // ── prepare_markdown_source ───────────────────────────────────────

    #[test]
    fn prepare_applies_criticmarkup_then_percent_stripping() {
        // Order is the reason this is one function: the CriticMarkup deletion
        // carries the `%%` delimiters, so accepting it first leaves nothing
        // for the `%%` pass to mis-pair.
        assert_eq!(
            prepare_markdown_source("keep {--%%--} end", 0).0,
            "keep  end"
        );
    }

    #[test]
    fn prepare_leaves_clean_source_byte_identical() {
        let body = "# Title\n\nJust prose, 50% done.\n";
        assert_eq!(prepare_markdown_source(body, 0).0, body);
    }

    #[test]
    fn unterminated_comment_warning_names_the_line_in_the_authors_file() {
        // Body line 3, and the page has a 4-line frontmatter block, so the
        // author's file line is 7. Reporting the body-relative line would send
        // them to the wrong place in their editor.
        let body = "intro\n\n<!-- TODO owner assets\n\nlost content\n";
        let warning = unterminated_comment_warning(body, 4)
            .unwrap_or_else(|| panic!("expected a warning for {body:?}"));
        assert!(
            warning.contains("line 7"),
            "must point at the file line, got: {warning}"
        );
        assert!(warning.contains("will not render"), "{warning}");
    }

    #[test]
    fn unterminated_comment_warning_is_silent_on_closed_comments() {
        assert!(unterminated_comment_warning("<!-- fine -->\n\nbody\n", 0).is_none());
        assert!(unterminated_comment_warning("no comments at all\n", 0).is_none());
        // `<!--` inside a code fence is not a comment.
        assert!(unterminated_comment_warning("```\n<!-- sample\n```\n", 0).is_none());
    }

    #[test]
    fn prepare_is_a_passthrough_for_an_unterminated_comment() {
        // The warning is a side effect on the diagnostics channel; the body
        // itself must survive untouched (the tail is inert, not deleted).
        let body = "intro\n\n<!-- TODO owner assets\n\nlost content\n";
        assert_eq!(prepare_markdown_source(body, 4).0, body);
    }

    // ── strip_percent_comments tests ──────────────────────────────────

    #[test]
    fn pct_inline_stripped() {
        assert_eq!(strip_percent_comments("before %%note%% after"), "before  after");
    }

    #[test]
    fn pct_inline_multiple_on_one_line() {
        assert_eq!(
            strip_percent_comments("a %%one%% b %%two%% c"),
            "a  b  c"
        );
    }

    #[test]
    fn pct_block_stripped() {
        let input = "before\n%%\nhidden text\n%%\nafter";
        assert_eq!(strip_percent_comments(input), "before\nafter");
    }

    #[test]
    fn pct_block_multiline_stripped() {
        let input = "before\n%%\nline one\nline two\n%%\nafter";
        assert_eq!(strip_percent_comments(input), "before\nafter");
    }

    #[test]
    fn pct_pair_opening_mid_line_closes_on_a_later_line_without_eating_the_heading() {
        let input = "Intro %% a note\nthat runs on %% tail\n\n## Next heading\n\nBody";
        assert_eq!(
            strip_percent_comments(input),
            "Intro  tail\n\n## Next heading\n\nBody"
        );
    }

    #[test]
    fn pct_unbalanced_marker_stays_visible_and_is_reported() {
        let input = "Intro\n\n%% never closed\n\n## Heading";
        assert_eq!(strip_percent_comments(input), input);
        let warning = unbalanced_percent_warning(input, 4).expect("a warning");
        assert!(warning.contains("line 7"), "{warning}");
        assert!(unbalanced_percent_warning("a %%b%% c", 0).is_none());
        assert!(unbalanced_percent_warning("`%%`", 0).is_none());
    }

    #[test]
    fn pct_inside_fenced_code_preserved() {
        let input = "```\nbefore %%note%% after\n```";
        assert_eq!(strip_percent_comments(input), input);
    }

    #[test]
    fn pct_inside_inline_code_preserved() {
        let input = "Use `%%comment%%` syntax in Obsidian.";
        assert_eq!(strip_percent_comments(input), input);
    }

    #[test]
    fn pct_prose_fifty_percent_not_stripped() {
        // A lone `%` repeated twice but with space between is NOT a `%%` token.
        let input = "50% off and 80% done";
        assert_eq!(strip_percent_comments(input), input);
    }

    #[test]
    fn pct_single_percent_percent_prose_balanced_but_unintended() {
        // Two `%%` tokens on one line ARE stripped — this is the inline form.
        // Authors who want a literal `%%` in prose must escape one `%` or use code.
        let input = "profit: 50%%, cost: 30%%";
        // No space between the `%%` tokens — the first `%%` at offset 9 and the
        // second at offset 18 are a balanced pair and will be stripped.
        // Result: "profit: 50 cost: 30"
        let result = strip_percent_comments(input);
        assert!(!result.contains("%%"), "should have stripped the pair: {result}");
    }

    #[test]
    fn pct_no_double_percent_is_fast_path() {
        use std::borrow::Cow;
        let input = "plain prose with 50% discount";
        match strip_percent_comments(input) {
            Cow::Borrowed(s) => assert_eq!(s, input),
            Cow::Owned(_) => panic!("expected Cow::Borrowed (fast path) for input with no %%"),
        }
    }

    #[test]
    fn pct_block_delimiter_alone_on_line_with_whitespace() {
        // Whitespace around `%%` on its own line is allowed.
        let input = "text\n  %%  \nhidden\n  %%  \nend";
        assert_eq!(strip_percent_comments(input), "text\nend");
    }

    #[test]
    fn pct_unclosed_block_left_alone() {
        // An opening `%%` with no matching closing line is left untouched.
        let input = "text\n%%\nhidden\nend";
        let result = strip_percent_comments(input);
        // Should not have removed anything because there's no closing `%%`.
        assert!(result.contains("%%"), "unclosed block delimiter should remain: {result}");
    }

    // ── CriticMarkup accept-mode tests ────────────────────────────────

    #[test]
    fn cm_accept_addition() {
        assert_eq!(accept_criticmarkup("x {++added++} y"), "x added y");
    }

    #[test]
    fn cm_accept_deletion() {
        assert_eq!(accept_criticmarkup("x {--gone--} y"), "x  y");
    }

    #[test]
    fn cm_accept_substitution() {
        assert_eq!(accept_criticmarkup("say {~~hi~>hello~~} there"), "say hello there");
    }

    #[test]
    fn cm_accept_highlight_keeps_content() {
        assert_eq!(accept_criticmarkup("note {==key==} end"), "note key end");
    }

    #[test]
    fn cm_accept_comment_strips_entirely() {
        assert_eq!(accept_criticmarkup("text {>>TODO<<} here"), "text  here");
    }

    #[test]
    fn cm_accept_nested_wikilink_preserves_inner_syntax() {
        // The outer highlight is peeled, leaving the wiki-link intact for
        // downstream resolution.
        assert_eq!(
            accept_criticmarkup("{==[[media|media files]]==}"),
            "[[media|media files]]"
        );
    }

    #[test]
    fn cm_accept_adjacent_highlight_plus_comment() {
        // The idiomatic annotation pattern from moss-releases docs.
        assert_eq!(
            accept_criticmarkup("{==[[media]]==}{>> why <<}"),
            "[[media]]"
        );
    }

    #[test]
    fn cm_accept_skips_fenced_code_backticks() {
        let input = "```\n{==fake==}\n```";
        assert_eq!(accept_criticmarkup(input), input);
    }

    #[test]
    fn cm_accept_skips_fenced_code_tildes() {
        let input = "~~~\n{==fake==}\n~~~";
        assert_eq!(accept_criticmarkup(input), input);
    }

    #[test]
    fn cm_accept_skips_inline_code() {
        let input = "Use `{==text==}` for highlights.";
        assert_eq!(accept_criticmarkup(input), input);
    }

    #[test]
    fn cm_accept_skips_indented_code() {
        // New coverage from the shared inert scanner: the private mask here
        // knew fences and inline spans only, so a CriticMarkup example inside
        // an indented code block was accepted and unwrapped in the very page
        // documenting it.
        let input = "How to annotate:\n\n    {==fake==}\n";
        assert_eq!(accept_criticmarkup(input), input);
    }

    #[test]
    fn cm_accept_skips_html_comments() {
        // Same fix as the inert-scanner regression, one layer over: an
        // annotation parked in an authored comment stays parked instead of
        // being unwrapped inside it.
        let input = "<!-- draft: {++new text++} -->\n\nlive";
        assert_eq!(accept_criticmarkup(input), input);
    }

    #[test]
    fn pct_skips_html_comments() {
        let input = "<!-- note: %%hidden%% -->\n\nlive";
        assert_eq!(strip_percent_comments(input), input);
    }

    #[test]
    fn pct_skips_indented_code() {
        let input = "Obsidian comments:\n\n    %%hidden%%\n";
        assert_eq!(strip_percent_comments(input), input);
    }

    #[test]
    fn cm_accept_real_mark_outside_following_code_block() {
        let input = "```\ncode\n```\n\n{==real==}";
        assert_eq!(accept_criticmarkup(input), "```\ncode\n```\n\nreal");
    }

    #[test]
    fn cm_accept_unclosed_addition_is_passthrough() {
        assert_eq!(accept_criticmarkup("start {++ oops"), "start {++ oops");
    }

    #[test]
    fn cm_accept_substitution_without_arrow_is_passthrough() {
        let input = "{~~ no arrow ~~}";
        assert_eq!(accept_criticmarkup(input), input);
    }

    #[test]
    fn cm_accept_empty_input_returns_borrowed() {
        use std::borrow::Cow;
        match accept_criticmarkup("") {
            Cow::Borrowed(s) => assert_eq!(s, ""),
            Cow::Owned(_) => panic!("expected Cow::Borrowed for empty input"),
        }
    }

    #[test]
    fn cm_accept_plain_markdown_returns_borrowed() {
        use std::borrow::Cow;
        let input = "# Heading\n\nJust prose. No markup.";
        match accept_criticmarkup(input) {
            Cow::Borrowed(s) => assert_eq!(s, input),
            Cow::Owned(_) => panic!("expected Cow::Borrowed when no marks present"),
        }
    }

    #[test]
    fn cm_accept_multiline_mark_within_paragraph() {
        assert_eq!(
            accept_criticmarkup("{==first line\nsecond line==}"),
            "first line\nsecond line"
        );
    }

    #[test]
    fn span_comment_publishes_as_plain_text() {
        // Highlight keeps its content (markers stripped); adjacent comment is
        // stripped entirely — the idiomatic "annotated span" pattern must never
        // leak the comment text into published HTML.
        assert_eq!(
            accept_criticmarkup("a {==brown fox==}{>>too vague<<} z").as_ref(),
            "a brown fox z"
        );
    }

    #[test]
    fn point_comment_at_line_start_is_stripped() {
        // Distinct from cm_accept_comment_strips_entirely (mid-sentence).
        // A comment at the very start of input with following text must not
        // prepend any marker residue.
        assert_eq!(
            accept_criticmarkup("{>>note<<}lead text").as_ref(),
            "lead text"
        );
    }

