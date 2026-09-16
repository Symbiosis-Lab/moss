use super::*;

// -- Fit ----------------------------------------------------------------

#[test]
fn test_fit_to_css_value() {
    assert_eq!(Fit::Cover.to_css_value(), "cover");
    assert_eq!(Fit::Contain.to_css_value(), "contain");
    assert_eq!(Fit::Fill.to_css_value(), "fill");
    assert_eq!(Fit::None.to_css_value(), "none");
    assert_eq!(Fit::ScaleDown.to_css_value(), "scale-down");
}

#[test]
fn test_fit_from_keyword() {
    assert_eq!(Fit::from_keyword("cover"), Some(Fit::Cover));
    assert_eq!(Fit::from_keyword("contain"), Some(Fit::Contain));
    assert_eq!(Fit::from_keyword("fill"), Some(Fit::Fill));
    assert_eq!(Fit::from_keyword("none"), Some(Fit::None));
    assert_eq!(Fit::from_keyword("scale-down"), Some(Fit::ScaleDown));
    assert_eq!(Fit::from_keyword("scaledown"), Some(Fit::ScaleDown));
}

#[test]
fn test_fit_from_keyword_case_insensitive() {
    assert_eq!(Fit::from_keyword("COVER"), Some(Fit::Cover));
    assert_eq!(Fit::from_keyword("Contain"), Some(Fit::Contain));
    assert_eq!(Fit::from_keyword("Scale-Down"), Some(Fit::ScaleDown));
    assert_eq!(Fit::from_keyword("SCALEDOWN"), Some(Fit::ScaleDown));
}

#[test]
fn test_fit_from_keyword_unknown() {
    assert_eq!(Fit::from_keyword("zoom"), None);
    assert_eq!(Fit::from_keyword(""), None);
    assert_eq!(Fit::from_keyword("cover "), None); // trailing space — not trimmed
}

// -- AlignSide ----------------------------------------------------------

#[test]
fn test_align_side_from_keyword() {
    assert_eq!(AlignSide::from_keyword("align-left"), Some(AlignSide::Left));
    assert_eq!(
        AlignSide::from_keyword("align-right"),
        Some(AlignSide::Right)
    );
    // WordPress-style unhyphenated alias.
    assert_eq!(AlignSide::from_keyword("alignleft"), Some(AlignSide::Left));
    assert_eq!(
        AlignSide::from_keyword("alignright"),
        Some(AlignSide::Right)
    );
    // Case-insensitive.
    assert_eq!(AlignSide::from_keyword("ALIGN-LEFT"), Some(AlignSide::Left));
    assert_eq!(
        AlignSide::from_keyword("AlignRight"),
        Some(AlignSide::Right)
    );
    // Empty input never matches.
    assert_eq!(AlignSide::from_keyword(""), None);
}

#[test]
fn test_align_side_from_keyword_bare_directional() {
    // Bare `left` / `right` are accepted because Stage 1 emits them as
    // the value of an explicit `align=` key (TitleParams), where the
    // key disambiguates from Position context. The existing pipe-
    // keyword space-separated parser (`parse_media_attrs`) still tries
    // Position::from_keyword first and never reaches AlignSide for
    // bare directionals — see test_parse_attrs_bare_left_is_position.
    assert_eq!(AlignSide::from_keyword("left"), Some(AlignSide::Left));
    assert_eq!(AlignSide::from_keyword("right"), Some(AlignSide::Right));
    assert_eq!(AlignSide::from_keyword("LEFT"), Some(AlignSide::Left));
    assert_eq!(AlignSide::from_keyword("Right"), Some(AlignSide::Right));
}

#[test]
fn test_parse_attrs_bare_left_is_position() {
    // In the pipe-keyword (`![[img|cover left]]`) parser, bare `left`
    // / `right` resolve as Position (object-position keyword), NOT as
    // AlignSide. Position::from_keyword is tried first in
    // `parse_media_attrs`; this test pins that ordering invariant so
    // a future refactor that re-orders the matchers will fail loudly.
    let attrs = parse_media_attrs("left");
    assert_eq!(attrs.position, Some(Position::Left));
    assert_eq!(attrs.align, None);

    let attrs = parse_media_attrs("right");
    assert_eq!(attrs.position, Some(Position::Right));
    assert_eq!(attrs.align, None);
}

#[test]
fn test_align_side_css_class() {
    assert_eq!(AlignSide::Left.css_class(), "moss-align-left");
    assert_eq!(AlignSide::Right.css_class(), "moss-align-right");
}

// -- Position -----------------------------------------------------------

#[test]
fn test_position_to_css_value() {
    assert_eq!(Position::Center.to_css_value(), "center");
    assert_eq!(Position::Left.to_css_value(), "left");
    assert_eq!(Position::Right.to_css_value(), "right");
    assert_eq!(Position::Top.to_css_value(), "top");
    assert_eq!(Position::Bottom.to_css_value(), "bottom");
    assert_eq!(Position::TopLeft.to_css_value(), "top left");
    assert_eq!(Position::TopRight.to_css_value(), "top right");
    assert_eq!(Position::BottomLeft.to_css_value(), "bottom left");
    assert_eq!(Position::BottomRight.to_css_value(), "bottom right");
}

#[test]
fn test_position_from_keyword_single() {
    assert_eq!(Position::from_keyword("center"), Some(Position::Center));
    assert_eq!(Position::from_keyword("left"), Some(Position::Left));
    assert_eq!(Position::from_keyword("right"), Some(Position::Right));
    assert_eq!(Position::from_keyword("top"), Some(Position::Top));
    assert_eq!(Position::from_keyword("bottom"), Some(Position::Bottom));
}

#[test]
fn test_position_from_keyword_compound() {
    // Hyphenated
    assert_eq!(Position::from_keyword("top-left"), Some(Position::TopLeft));
    assert_eq!(
        Position::from_keyword("top-right"),
        Some(Position::TopRight)
    );
    assert_eq!(
        Position::from_keyword("bottom-left"),
        Some(Position::BottomLeft)
    );
    assert_eq!(
        Position::from_keyword("bottom-right"),
        Some(Position::BottomRight)
    );

    // Concatenated
    assert_eq!(Position::from_keyword("topleft"), Some(Position::TopLeft));
    assert_eq!(
        Position::from_keyword("bottomright"),
        Some(Position::BottomRight)
    );

    // Space-separated (used when caller pre-joins tokens)
    assert_eq!(Position::from_keyword("top left"), Some(Position::TopLeft));
    assert_eq!(
        Position::from_keyword("bottom right"),
        Some(Position::BottomRight)
    );
}

#[test]
fn test_position_from_keyword_case_insensitive() {
    assert_eq!(Position::from_keyword("CENTER"), Some(Position::Center));
    assert_eq!(Position::from_keyword("Top-Left"), Some(Position::TopLeft));
    assert_eq!(
        Position::from_keyword("BOTTOMRIGHT"),
        Some(Position::BottomRight)
    );
}

#[test]
fn test_position_from_keyword_unknown() {
    assert_eq!(Position::from_keyword("middle"), None);
    assert_eq!(Position::from_keyword(""), None);
}

// -- MediaAttrs ---------------------------------------------------------

#[test]
fn test_media_attrs_is_empty() {
    let empty = MediaAttrs {
        fit: None,
        position: None,
        align: None,
        color: None,
        class_names: Vec::new(),
        extra_attrs: BTreeMap::new(),
    };
    assert!(empty.is_empty());

    let with_fit = MediaAttrs {
        fit: Some(Fit::Cover),
        position: None,
        align: None,
        color: None,
        class_names: Vec::new(),
        extra_attrs: BTreeMap::new(),
    };
    assert!(!with_fit.is_empty());

    let with_pos = MediaAttrs {
        fit: None,
        position: Some(Position::Center),
        align: None,
        color: None,
        class_names: Vec::new(),
        extra_attrs: BTreeMap::new(),
    };
    assert!(!with_pos.is_empty());
}

#[test]
fn test_to_inline_style_empty() {
    let attrs = MediaAttrs {
        fit: None,
        position: None,
        align: None,
        color: None,
        class_names: Vec::new(),
        extra_attrs: BTreeMap::new(),
    };
    assert_eq!(attrs.to_inline_style(), None);
}

#[test]
fn test_to_inline_style_fit_only() {
    let attrs = MediaAttrs {
        fit: Some(Fit::Contain),
        position: None,
        align: None,
        color: None,
        class_names: Vec::new(),
        extra_attrs: BTreeMap::new(),
    };
    assert_eq!(attrs.to_inline_style(), Some("object-fit:contain".into()));
}

#[test]
fn test_to_inline_style_position_only() {
    let attrs = MediaAttrs {
        fit: None,
        position: Some(Position::Left),
        align: None,
        color: None,
        class_names: Vec::new(),
        extra_attrs: BTreeMap::new(),
    };
    assert_eq!(attrs.to_inline_style(), Some("object-position:left".into()));
}

#[test]
fn test_to_inline_style_both() {
    let attrs = MediaAttrs {
        fit: Some(Fit::Cover),
        position: Some(Position::TopLeft),
        align: None,
        color: None,
        class_names: Vec::new(),
        extra_attrs: BTreeMap::new(),
    };
    assert_eq!(
        attrs.to_inline_style(),
        Some("object-fit:cover;object-position:top left".into())
    );
}

// -- strip_wikilink -----------------------------------------------------

#[test]
fn test_strip_wikilink_with_brackets() {
    assert_eq!(strip_wikilink("[[photo.jpg]]"), "photo.jpg");
    assert_eq!(strip_wikilink("[[path/to/image.png]]"), "path/to/image.png");
}

#[test]
fn test_strip_wikilink_without_brackets() {
    assert_eq!(strip_wikilink("photo.jpg"), "photo.jpg");
    assert_eq!(strip_wikilink("path/to/image.png"), "path/to/image.png");
}

#[test]
fn test_strip_wikilink_with_pipe() {
    assert_eq!(strip_wikilink("[[photo.jpg|cover]]"), "photo.jpg|cover");
}

#[test]
fn test_strip_wikilink_with_whitespace() {
    assert_eq!(strip_wikilink("  [[photo.jpg]]  "), "photo.jpg");
}

#[test]
fn test_strip_wikilink_partial_brackets() {
    // Only opening bracket — no stripping.
    assert_eq!(strip_wikilink("[[photo.jpg"), "[[photo.jpg");
    // Only closing bracket — no stripping.
    assert_eq!(strip_wikilink("photo.jpg]]"), "photo.jpg]]");
}

#[test]
fn test_strip_wikilink_empty() {
    assert_eq!(strip_wikilink("[[]]"), "");
    assert_eq!(strip_wikilink(""), "");
}

// -- split_pipe ---------------------------------------------------------

#[test]
fn test_split_pipe_with_pipe() {
    assert_eq!(split_pipe("photo.jpg|cover"), ("photo.jpg", "cover"));
    assert_eq!(
        split_pipe("path/to/img.png|contain center"),
        ("path/to/img.png", "contain center")
    );
}

#[test]
fn test_split_pipe_no_pipe() {
    assert_eq!(split_pipe("photo.jpg"), ("photo.jpg", ""));
    assert_eq!(split_pipe(""), ("", ""));
}

#[test]
fn test_split_pipe_multiple_pipes() {
    // Only split on the first pipe.
    assert_eq!(split_pipe("a|b|c"), ("a", "b|c"));
}

#[test]
fn test_split_pipe_pipe_at_edges() {
    assert_eq!(split_pipe("|cover"), ("", "cover"));
    assert_eq!(split_pipe("photo.jpg|"), ("photo.jpg", ""));
}

// -- parse_media_attrs --------------------------------------------------

#[test]
fn test_parse_attrs_fit_only() {
    let attrs = parse_media_attrs("cover");
    assert_eq!(attrs.fit, Some(Fit::Cover));
    assert_eq!(attrs.position, None);
}

#[test]
fn test_parse_attrs_position_only() {
    let attrs = parse_media_attrs("center");
    assert_eq!(attrs.fit, None);
    assert_eq!(attrs.position, Some(Position::Center));
}

#[test]
fn test_parse_attrs_fit_and_position() {
    let attrs = parse_media_attrs("contain left");
    assert_eq!(attrs.fit, Some(Fit::Contain));
    assert_eq!(attrs.position, Some(Position::Left));
}

#[test]
fn test_parse_attrs_two_word_position() {
    let attrs = parse_media_attrs("top left");
    assert_eq!(attrs.fit, None);
    assert_eq!(attrs.position, Some(Position::TopLeft));

    let attrs2 = parse_media_attrs("cover bottom right");
    assert_eq!(attrs2.fit, Some(Fit::Cover));
    assert_eq!(attrs2.position, Some(Position::BottomRight));
}

#[test]
fn test_parse_attrs_hyphenated_compound_position() {
    let attrs = parse_media_attrs("top-right");
    assert_eq!(attrs.fit, None);
    assert_eq!(attrs.position, Some(Position::TopRight));

    let attrs2 = parse_media_attrs("fill bottom-left");
    assert_eq!(attrs2.fit, Some(Fit::Fill));
    assert_eq!(attrs2.position, Some(Position::BottomLeft));
}

#[test]
fn test_parse_attrs_unknown_tokens_ignored() {
    let attrs = parse_media_attrs("cover unknown-token left");
    assert_eq!(attrs.fit, Some(Fit::Cover));
    assert_eq!(attrs.position, Some(Position::Left));
}

#[test]
fn test_parse_attrs_empty_string() {
    let attrs = parse_media_attrs("");
    assert!(attrs.is_empty());
}

#[test]
fn test_parse_attrs_only_whitespace() {
    let attrs = parse_media_attrs("   ");
    assert!(attrs.is_empty());
}

#[test]
fn test_parse_attrs_all_unknown() {
    let attrs = parse_media_attrs("foo bar baz");
    assert!(attrs.is_empty());
}

#[test]
fn test_parse_attrs_case_insensitive() {
    let attrs = parse_media_attrs("COVER CENTER");
    assert_eq!(attrs.fit, Some(Fit::Cover));
    assert_eq!(attrs.position, Some(Position::Center));
}

#[test]
fn test_parse_attrs_last_wins_for_duplicates() {
    // If multiple fit keywords appear, the last one wins.
    let attrs = parse_media_attrs("cover contain");
    assert_eq!(attrs.fit, Some(Fit::Contain));
}

#[test]
fn test_parse_attrs_scale_down() {
    let attrs = parse_media_attrs("scale-down");
    assert_eq!(attrs.fit, Some(Fit::ScaleDown));
}

// -- is_all_display_keywords -------------------------------------------

#[test]
fn test_is_all_display_keywords_positions() {
    assert!(is_all_display_keywords("left"));
    assert!(is_all_display_keywords("right"));
    assert!(is_all_display_keywords("center"));
    assert!(is_all_display_keywords("top"));
    assert!(is_all_display_keywords("bottom"));
    assert!(is_all_display_keywords("top left"));
    assert!(is_all_display_keywords("bottom right"));
}

#[test]
fn test_is_all_display_keywords_fits() {
    assert!(is_all_display_keywords("cover"));
    assert!(is_all_display_keywords("contain"));
    assert!(is_all_display_keywords("fill"));
    assert!(is_all_display_keywords("none"));
    assert!(is_all_display_keywords("scale-down"));
}

#[test]
fn test_is_all_display_keywords_combined() {
    assert!(is_all_display_keywords("contain left"));
    assert!(is_all_display_keywords("cover top left"));
    assert!(is_all_display_keywords("cover top-right"));
    assert!(is_all_display_keywords("scale-down bottom-left"));
}

#[test]
fn test_is_all_display_keywords_rejects_non_keywords() {
    assert!(!is_all_display_keywords("A beautiful sunset"));
    assert!(!is_all_display_keywords("left side"));
    assert!(!is_all_display_keywords(""));
    assert!(!is_all_display_keywords("   "));
}

// -- html_escape --------------------------------------------------

#[test]
fn test_html_escape_basic() {
    assert_eq!(html_escape("hello"), "hello");
    assert_eq!(html_escape("a&b"), "a&amp;b");
    assert_eq!(html_escape("a\"b"), "a&quot;b");
    assert_eq!(html_escape("a'b"), "a&#39;b");
    assert_eq!(html_escape("a<b>c"), "a&lt;b&gt;c");
    assert_eq!(
        html_escape("<div class=\"x\">&'</div>"),
        "&lt;div class=&quot;x&quot;&gt;&amp;&#39;&lt;/div&gt;"
    );
}

#[test]
fn test_parse_media_attrs_align_alone() {
    let attrs = parse_media_attrs("align-left");
    assert_eq!(attrs.align, Some(AlignSide::Left));
    assert_eq!(attrs.fit, None);
    assert_eq!(attrs.position, None);
}

#[test]
fn test_parse_media_attrs_align_with_cover() {
    // Order-free composition with Fit.
    let a = parse_media_attrs("cover align-right");
    assert_eq!(a.fit, Some(Fit::Cover));
    assert_eq!(a.align, Some(AlignSide::Right));

    let b = parse_media_attrs("align-right cover");
    assert_eq!(b, a);
}

#[test]
fn test_parse_media_attrs_align_last_wins() {
    // Contradictory align keywords resolve last-wins (no error, no warning).
    // Locked here so a future refactor can't silently flip to first-wins or
    // None-on-conflict.
    let attrs = parse_media_attrs("align-left align-right");
    assert_eq!(attrs.align, Some(AlignSide::Right));

    let attrs = parse_media_attrs("align-right align-left");
    assert_eq!(attrs.align, Some(AlignSide::Left));
}

#[test]
fn test_is_all_display_keywords_align() {
    assert!(is_all_display_keywords("align-left"));
    assert!(is_all_display_keywords("align-right"));
    assert!(is_all_display_keywords("cover align-left"));
    assert!(is_all_display_keywords("align-left cover"));
    // Composes with Position too.
    assert!(is_all_display_keywords("align-left top"));
}

// -- match_width_token / extract_width_from_alias ---------------------

#[test]
fn test_match_width_token_recognized() {
    assert_eq!(match_width_token("body"), Some("body"));
    assert_eq!(match_width_token("wide"), Some("wide"));
    assert_eq!(match_width_token("page"), Some("page"));
    assert_eq!(match_width_token("screen"), Some("screen"));
    // `full` is the author-facing alias for `screen` (canonical value).
    assert_eq!(match_width_token("full"), Some("screen"));
}

#[test]
fn test_match_width_token_rejects_non_width() {
    assert_eq!(match_width_token(""), None);
    assert_eq!(match_width_token("BODY"), None);
    assert_eq!(match_width_token("widely"), None);
    // Multi-token strings are exact-match only — no caption shadowing.
    assert_eq!(match_width_token("wide angle"), None);
    // Display keywords aren't width tokens.
    assert_eq!(match_width_token("contain"), None);
    assert_eq!(match_width_token("left"), None);
}

#[test]
fn test_extract_width_from_alias_single_segment_width() {
    let (w, rest) = extract_width_from_alias("full");
    assert_eq!(w, Some("screen"));
    assert_eq!(rest, "");
}

#[test]
fn test_extract_width_from_alias_caption_only() {
    // No width token — alias passes through unchanged.
    let (w, rest) = extract_width_from_alias("A beautiful sunset");
    assert_eq!(w, None);
    assert_eq!(rest, "A beautiful sunset");
}

#[test]
fn test_extract_width_from_alias_caption_then_width() {
    // Multi-pipe alias `caption|full` (the wikilink parser hands us
    // the post-first-`|` slice intact).
    let (w, rest) = extract_width_from_alias("A nice photo|full");
    assert_eq!(w, Some("screen"));
    assert_eq!(rest, "A nice photo");
}

#[test]
fn test_extract_width_from_alias_width_then_caption() {
    let (w, rest) = extract_width_from_alias("wide|A nice photo");
    assert_eq!(w, Some("wide"));
    assert_eq!(rest, "A nice photo");
}

#[test]
fn test_extract_width_from_alias_caption_with_width_word_not_shadowed() {
    // The phrase "caption that says wide" must NOT trigger width
    // recognition — a width token only fires when an entire alias
    // segment is exactly the token.
    let (w, rest) = extract_width_from_alias("caption that says wide");
    assert_eq!(w, None);
    assert_eq!(rest, "caption that says wide");
}

#[test]
fn test_extract_width_from_alias_only_first_width_extracted() {
    // If two width tokens appear, only the first one is canonical-ised;
    // the second stays in the caption text. Authors writing two width
    // tokens is malformed input, and rather than silently merging we
    // preserve the surplus for diagnostic visibility downstream.
    let (w, rest) = extract_width_from_alias("full|wide");
    assert_eq!(w, Some("screen"));
    assert_eq!(rest, "wide");
}

#[test]
fn test_extract_width_from_alias_segment_whitespace_trimmed() {
    // Authors who write `caption | full` should still get width
    // recognition — leading/trailing whitespace on a segment is
    // ignored for the token check but preserved in the rejoined rest.
    let (w, rest) = extract_width_from_alias("caption | full");
    assert_eq!(w, Some("screen"));
    assert_eq!(rest, "caption ");
}

// -- MediaAttrs passthroughs: class_names + extra_attrs ----------------

#[test]
fn test_media_attrs_class_names_preserved() {
    // Author-provided class names (not in moss vocabulary) survive on
    // MediaAttrs; the wikilink Stage 1 translator and downstream Stage 2
    // dispatcher consume `class_attr()` to compose the final class list.
    let attrs = MediaAttrs {
        fit: None,
        position: None,
        align: None,
        color: None,
        class_names: vec!["theme-rounded".to_string(), "shadow-lg".to_string()],
        extra_attrs: BTreeMap::new(),
    };
    assert!(!attrs.is_empty());
    assert_eq!(
        attrs.class_attr(),
        Some("theme-rounded shadow-lg".to_string())
    );
}

#[test]
fn test_media_attrs_class_names_compose_with_align() {
    // align (typed) and class_names (passthrough) compose into the same
    // class list. Stage 2 dispatcher recomposes them into the final
    // `class="moss-image moss-align-left theme-rounded"`.
    let attrs = MediaAttrs {
        fit: None,
        position: None,
        align: Some(AlignSide::Left),
        color: None,
        class_names: vec!["theme-rounded".to_string()],
        extra_attrs: BTreeMap::new(),
    };
    assert_eq!(
        attrs.class_attr(),
        Some("moss-align-left theme-rounded".to_string())
    );
}

#[test]
fn test_media_attrs_extra_attrs_non_empty() {
    // extra_attrs make MediaAttrs non-empty so callers know to round-trip
    // them through the wikilink title-params channel.
    let mut extras = BTreeMap::new();
    extras.insert("data-zoom".to_string(), "true".to_string());
    extras.insert("data-id".to_string(), "42".to_string());
    let attrs = MediaAttrs {
        fit: None,
        position: None,
        align: None,
        color: None,
        class_names: vec![],
        extra_attrs: extras,
    };
    assert!(!attrs.is_empty());
    // BTreeMap iteration is deterministic alphabetical (data-id < data-zoom).
    let keys: Vec<&str> = attrs.extra_attrs.keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["data-id", "data-zoom"]);
}

// -- classify_image_alias (Phase 1: lifted from ImageRenderer) ----------

#[test]
fn test_classify_image_alias_none() {
    let c = classify_image_alias(None);
    assert_eq!(c.display_keywords, None);
    assert_eq!(c.caption, None);
}

#[test]
fn test_classify_image_alias_empty_is_none_never_some_empty() {
    // THE invariant: an empty alias yields caption=None, never Some(""),
    // so no caller emits an empty <figcaption>.
    let c = classify_image_alias(Some(""));
    assert_eq!(c.display_keywords, None);
    assert_eq!(c.caption, None);
}

#[test]
fn test_classify_image_alias_structural_single_keyword() {
    let c = classify_image_alias(Some("cover"));
    assert_eq!(c.display_keywords.as_deref(), Some("cover"));
    assert_eq!(c.caption, None);
}

#[test]
fn test_classify_image_alias_structural_compound() {
    // `wide cover` = width token + fit keyword — fully structural.
    let c = classify_image_alias(Some("wide cover"));
    assert_eq!(c.display_keywords.as_deref(), Some("wide cover"));
    assert_eq!(c.caption, None);
}

#[test]
fn test_classify_image_alias_pure_width_token_is_structural() {
    // A bare width token alone is structural, not a caption.
    let c = classify_image_alias(Some("wide"));
    assert_eq!(c.display_keywords.as_deref(), Some("wide"));
    assert_eq!(c.caption, None);
}

#[test]
fn test_classify_image_alias_caption_text() {
    let c = classify_image_alias(Some("My nice photo"));
    assert_eq!(c.display_keywords, None);
    assert_eq!(c.caption.as_deref(), Some("My nice photo"));
}

#[test]
fn test_is_structural_alias_matches_classifier() {
    // Sanity: the lifted helper agrees with the classifier's branch.
    assert!(is_structural_alias("cover"));
    assert!(is_structural_alias("wide cover"));
    assert!(is_structural_alias("top left"));
    assert!(!is_structural_alias("My nice photo"));
    assert!(!is_structural_alias(""));
}

// -- parse_image_width -------------------------------------------------

#[test]
fn parse_image_width_named_tokens() {
    assert_eq!(parse_image_width("wide").as_deref(), Some("wide"));
    assert_eq!(parse_image_width("full").as_deref(), Some("screen")); // alias
    assert_eq!(parse_image_width("body").as_deref(), Some("body"));
}

#[test]
fn parse_image_width_percent() {
    assert_eq!(parse_image_width("55%").as_deref(), Some("55%"));
    assert_eq!(parse_image_width("100%").as_deref(), Some("100%"));
    assert_eq!(parse_image_width(" 40% ").as_deref(), Some("40%")); // trimmed
}

#[test]
fn parse_image_width_clamps_and_rejects() {
    assert_eq!(parse_image_width("150%").as_deref(), Some("100%")); // clamp > 100
    assert_eq!(parse_image_width("0%"), None); // reject <= 0
    assert_eq!(parse_image_width("-5%"), None); // reject negative
    assert_eq!(parse_image_width("50.5%").as_deref(), Some("50.5%")); // f32 ok
}

#[test]
fn parse_image_width_rejects_non_width() {
    assert_eq!(parse_image_width("wide angle photo"), None); // multi-word caption
    assert_eq!(parse_image_width("200x150"), None); // box sizing not a figure width
    assert_eq!(parse_image_width("hello"), None);
    assert_eq!(parse_image_width(""), None);
}

// -- split_alt_width ---------------------------------------------------

#[test]
fn split_alt_width_extracts_and_preserves() {
    // (remaining_alt, width)
    assert_eq!(
        split_alt_width("My caption|55%"),
        ("My caption".to_string(), Some("55%".to_string()))
    );
    assert_eq!(
        split_alt_width("55%"),
        (String::new(), Some("55%".to_string()))
    );
    assert_eq!(
        split_alt_width("wide"),
        (String::new(), Some("wide".to_string()))
    );
    // No width → alt unchanged, no pipe collapse
    assert_eq!(
        split_alt_width("just a caption"),
        ("just a caption".to_string(), None)
    );
    // Width is any one segment; other segments preserved joined by '|'
    assert_eq!(
        split_alt_width("cap|55%|extra"),
        ("cap|extra".to_string(), Some("55%".to_string()))
    );
    // Only the FIRST width-looking segment is consumed
    assert_eq!(
        split_alt_width("40%|60%"),
        ("60%".to_string(), Some("40%".to_string()))
    );
}

// -- set_image_width ---------------------------------------------------

#[test]
fn set_image_width_standard_markdown() {
    // add to a bare image
    assert_eq!(
        set_image_width("![alt](pic.jpg)", Some("55%")),
        "![alt|55%](pic.jpg)"
    );
    // replace an existing percent
    assert_eq!(
        set_image_width("![alt|30%](pic.jpg)", Some("55%")),
        "![alt|55%](pic.jpg)"
    );
    // replace an existing named token
    assert_eq!(
        set_image_width("![alt|wide](pic.jpg)", Some("55%")),
        "![alt|55%](pic.jpg)"
    );
    // remove (double-click reset)
    assert_eq!(
        set_image_width("![alt|55%](pic.jpg)", None),
        "![alt](pic.jpg)"
    );
    // add to empty-alt image
    assert_eq!(
        set_image_width("![](pic.jpg)", Some("55%")),
        "![|55%](pic.jpg)"
    );
    // preserve a caption segment
    assert_eq!(
        set_image_width("![My cap|30%](pic.jpg)", Some("55%")),
        "![My cap|55%](pic.jpg)"
    );
}

#[test]
fn set_image_width_wikilink() {
    assert_eq!(
        set_image_width("![[pic.jpg]]", Some("55%")),
        "![[pic.jpg|55%]]"
    );
    assert_eq!(
        set_image_width("![[pic.jpg|30%]]", Some("55%")),
        "![[pic.jpg|55%]]"
    );
    assert_eq!(
        set_image_width("![[pic.jpg|wide]]", Some("55%")),
        "![[pic.jpg|55%]]"
    );
    assert_eq!(set_image_width("![[pic.jpg|55%]]", None), "![[pic.jpg]]");
    // preserve a caption pothole segment
    assert_eq!(
        set_image_width("![[pic.jpg|My cap|30%]]", Some("55%")),
        "![[pic.jpg|My cap|55%]]"
    );
    assert_eq!(
        set_image_width("![[pic.jpg|My cap]]", Some("55%")),
        "![[pic.jpg|My cap|55%]]"
    );
}

#[test]
fn set_image_width_validates() {
    // out-of-range width is clamped/rejected by parse_image_width
    assert_eq!(
        set_image_width("![a](p.jpg)", Some("150%")),
        "![a|100%](p.jpg)"
    );
    // an unrecognized width string is ignored → treated as removal-or-noop
    assert_eq!(
        set_image_width("![a|30%](p.jpg)", Some("garbage")),
        "![a](p.jpg)"
    );
}

#[test]
fn set_image_width_passthrough_non_image() {
    // Not an image syntax → returned unchanged (defensive).
    assert_eq!(set_image_width("plain text", Some("55%")), "plain text");
}

// -- color= pipe attr ---------------------------------------------------

#[test]
fn parse_color_attr() {
    let attrs = parse_media_attrs("color=black");
    assert_eq!(attrs.color.as_deref(), Some("black"));

    let attrs = parse_media_attrs("color=#0a0a0a");
    assert_eq!(attrs.color.as_deref(), Some("#0a0a0a"));
}

#[test]
fn parse_color_attr_alongside_keywords() {
    let attrs = parse_media_attrs("contain color=rgb(10,10,10) top left");
    assert_eq!(attrs.color.as_deref(), Some("rgb(10,10,10)"));
    assert!(attrs.fit.is_some(), "fit keyword must still parse");
    assert!(
        attrs.position.is_some(),
        "position keywords must still parse"
    );
}

#[test]
fn parse_empty_color_attr_is_none() {
    let attrs = parse_media_attrs("color=");
    assert_eq!(attrs.color, None);
    assert!(attrs.is_empty());
}

#[test]
fn color_attr_alone_is_not_empty() {
    let attrs = parse_media_attrs("color=black");
    assert!(!attrs.is_empty());
}

#[test]
fn color_attr_does_not_leak_into_style_or_class() {
    let attrs = parse_media_attrs("color=black");
    assert_eq!(attrs.to_inline_style(), None);
    assert_eq!(attrs.class_attr(), None);
}

#[test]
fn repeated_color_attr_last_wins() {
    // Consistent with fit/position: later tokens overwrite earlier ones.
    let attrs = parse_media_attrs("color=red color=blue");
    assert_eq!(attrs.color.as_deref(), Some("blue"));
}
