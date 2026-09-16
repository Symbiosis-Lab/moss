use super::*;
use crate::build::site_url::SiteUrl;
use crate::i18n::Language;

fn test_site_url() -> SiteUrl {
    SiteUrl::parse("https://example.com").unwrap()
}

/// A bare `<` is the less-than sign, not the start of a tag.
///
/// The tag stripper used to pair any `<` with the next `>` and delete
/// everything between, so an inequality swallowed the prose between two
/// comparisons. This is the page description, so the loss showed up in
/// <meta name="description">, og:/twitter: description, JSON-LD, and
/// hover/folder-embed excerpts at once.
#[test]
fn strip_markdown_inline_keeps_prose_between_two_comparisons() {
    assert_eq!(
        strip_markdown_inline("Given $a < b$ and $c > d$ we conclude."),
        "Given $a < b$ and $c > d$ we conclude.",
    );
    // A lone `>` with no preceding `<` was always safe; pin it anyway.
    assert_eq!(
        strip_markdown_inline("Let $S = \\{x : x > 0\\}$ be the set."),
        "Let $S = \\{x : x > 0\\}$ be the set.",
    );

    // UNSPACED is the case the single-byte lookahead got wrong, and it
    // is the ordinary way TeX is written: `<b` looked like a tag opener,
    // so everything up to the next `>` was deleted. These are the
    // regression vectors — without the tag-shape check the first became
    // "Given $a d$ we conclude." and the second "If $x0$ holds.".
    assert_eq!(
        strip_markdown_inline("Given $a <b$ and $c >d$ we conclude."),
        "Given $a <b$ and $c >d$ we conclude.",
    );
    assert_eq!(
        strip_markdown_inline("If $x<y$ then the map is monotone and $y>0$ holds."),
        "If $x<y$ then the map is monotone and $y>0$ holds.",
    );
    // No `>` downstream to pair with — safe on shape alone.
    assert_eq!(strip_markdown_inline("if a <b then"), "if a <b then");
    // Punctuation cannot follow a tag NAME, so these are prose too.
    // These are the vectors that witness the tag-shape check itself:
    // they contain no `$`, so the math guard does not cover them.
    assert_eq!(
        strip_markdown_inline("Suppose a <b, and c >d holds."),
        "Suppose a <b, and c >d holds.",
    );
    assert_eq!(
        strip_markdown_inline("for all a <b: c >d"),
        "for all a <b: c >d"
    );
    assert_eq!(
        strip_markdown_inline("x <y. Then z >w."),
        "x <y. Then z >w."
    );
    // A digit cannot start a tag name.
    assert_eq!(
        strip_markdown_inline("Compare 1 <2 and 3 >4 end."),
        "Compare 1 <2 and 3 >4 end.",
    );
}

/// The residual, stated honestly so nobody re-derives it as a bug.
///
/// `<b then c >` is syntactically a real tag — element `b` with two
/// bare attributes — so a text-level stripper cannot tell it from
/// prose, and this one strips it. What saves the math case is that TeX
/// puts a non-space character straight after the variable (`$a <b$`,
/// `$x<y$`), and punctuation cannot follow a tag name. Prose that
/// happens to write `<word ` with a space and a later `>` stays
/// ambiguous; resolving it would need a real HTML parser, which is not
/// warranted for excerpt extraction.
#[test]
fn strip_markdown_inline_residual_ambiguity_is_documented() {
    // Ambiguous, and we strip: indistinguishable from `<b attr attr>`.
    assert_eq!(strip_markdown_inline("if a <b then c >d"), "if a d");
    // The same shape with the TeX spacing is safe.
    assert_eq!(
        strip_markdown_inline("if $a <b$ then $c >d$"),
        "if $a <b$ then $c >d$",
    );
}

/// Narrowing the stripper must not stop it stripping actual HTML — that
/// is the job it exists for.
#[test]
fn strip_markdown_inline_still_strips_real_html_tags() {
    // (the function collapses runs of spaces, so the gap the removed tag
    // leaves behind reads as a single space)
    assert_eq!(
        strip_markdown_inline("Before <iframe src=\"x\"></iframe> after"),
        "Before after",
    );
    assert_eq!(strip_markdown_inline("<div class=\"a\">text</div>"), "text");
    assert_eq!(strip_markdown_inline("a <!-- note --> b"), "a b");
    // Mixed: the tag goes, the inequality stays.
    assert_eq!(
        strip_markdown_inline("<span>x</span> where 1 < 2 holds"),
        "x where 1 < 2 holds",
    );
}

#[test]
fn test_build_og_tags_basic() {
    let site_url = test_site_url();
    let result = build_og_tags(
        "Test Article",
        "A test description",
        "https://example.com/posts/test/",
        "My Site",
        Some("2024-01-15"),
        None,
        None,
        &[],
        "en_US",
        &site_url,
    );

    assert!(result.contains(r#"og:type" content="article"#));
    assert!(result.contains(r#"og:title" content="Test Article"#));
    assert!(result.contains(r#"og:url" content="https://example.com/posts/test/"#));
    assert!(result.contains(r#"og:site_name" content="My Site"#));
    assert!(result.contains(r#"og:description" content="A test description"#));
    assert!(result.contains(r#"article:published_time" content="2024-01-15"#));
}

#[test]
fn test_build_og_tags_with_image_and_tags() {
    let site_url = test_site_url();
    let cover = CoverRef::External("https://example.com/image.jpg".to_string());
    let result = build_og_tags(
        "Test",
        "Desc",
        "https://example.com/",
        "Site",
        None,
        Some(&cover),
        None,
        &["rust".to_string(), "programming".to_string()],
        "en_US",
        &site_url,
    );

    assert!(result.contains(r#"og:image" content="https://example.com/image.jpg"#));
    assert!(result.contains(r#"article:tag" content="rust"#));
    assert!(result.contains(r#"article:tag" content="programming"#));
}

#[test]
fn test_build_og_tags_escapes_special_chars() {
    let site_url = test_site_url();
    let result = build_og_tags(
        "Title with \"quotes\" & <tags>",
        "Description",
        "https://example.com/",
        "Site",
        None,
        None,
        None,
        &[],
        "en_US",
        &site_url,
    );

    assert!(result.contains("&quot;quotes&quot;"));
    assert!(result.contains("&amp;"));
    assert!(result.contains("&lt;tags&gt;"));
}

#[test]
fn test_build_og_tags_emits_locale() {
    let site_url = test_site_url();
    let result = build_og_tags(
        "Title",
        "Desc",
        "https://example.com/",
        "Site",
        None,
        None,
        None,
        &[],
        "en_US",
        &site_url,
    );
    assert!(
        result.contains(r#"<meta property="og:locale" content="en_US">"#),
        "expected og:locale tag, got: {result}"
    );
}

#[test]
fn test_build_og_tags_skips_locale_when_empty() {
    let site_url = test_site_url();
    let result = build_og_tags(
        "Title",
        "Desc",
        "https://example.com/",
        "Site",
        None,
        None,
        None,
        &[],
        "",
        &site_url,
    );
    assert!(
        !result.contains("og:locale"),
        "should not emit og:locale when locale is empty"
    );
}

#[test]
fn test_build_og_tags_website_skips_locale_when_empty() {
    let site_url = test_site_url();
    let result = build_og_tags_website(
        "Title",
        "Test Site",
        "Desc",
        "https://example.com/",
        None,
        None,
        "",
        &site_url,
    );
    assert!(
        !result.contains("og:locale"),
        "should not emit og:locale when locale is empty"
    );
}

#[test]
fn test_build_og_tags_emits_image_dimensions_when_image_present() {
    let site_url = test_site_url();
    let cover = CoverRef::External("https://example.com/card.png".to_string());
    let result = build_og_tags(
        "Title",
        "Desc",
        "https://example.com/",
        "Site",
        None,
        Some(&cover),
        Some((1200, 630)),
        &[],
        "en_US",
        &site_url,
    );
    assert!(result.contains(r#"og:image" content="https://example.com/card.png"#));
    assert!(result.contains(r#"<meta property="og:image:width" content="1200">"#));
    assert!(result.contains(r#"<meta property="og:image:height" content="630">"#));
    assert!(result.contains(r#"<meta property="og:image:alt" content="Title">"#));
}

#[test]
fn test_build_twitter_tags_uses_summary_large_image() {
    let site_url = test_site_url();
    let cover = CoverRef::External("https://example.com/card.png".to_string());
    let result = build_twitter_tags("Title", "Desc", Some(&cover), Some("alt text"), &site_url);
    assert!(result.contains(r#"<meta name="twitter:card" content="summary_large_image">"#));
    assert!(result.contains(r#"<meta name="twitter:title" content="Title">"#));
    assert!(result.contains(r#"<meta name="twitter:description" content="Desc">"#));
    assert!(
        result.contains(r#"<meta name="twitter:image" content="https://example.com/card.png">"#)
    );
    assert!(result.contains(r#"<meta name="twitter:image:alt" content="alt text">"#));
}

#[test]
fn test_build_twitter_tags_uses_summary_when_no_image() {
    let site_url = test_site_url();
    let result = build_twitter_tags("Title", "Desc", None, None, &site_url);
    assert!(result.contains(r#"<meta name="twitter:card" content="summary">"#));
    assert!(result.contains(r#"<meta name="twitter:title" content="Title">"#));
    assert!(result.contains(r#"<meta name="twitter:description" content="Desc">"#));
    assert!(!result.contains("twitter:image"));
}

#[test]
fn test_build_schema_json_ld_basic() {
    let site_url = test_site_url();
    let result = build_schema_json_ld(
        "Test Article",
        "A test description",
        "https://example.com/posts/test/",
        "My Site",
        Some("2024-01-15"),
        None,
        &[],
        &site_url,
    );

    assert!(result.contains(r#""@context": "https://schema.org""#));
    assert!(result.contains(r#""@type": "Article""#));
    assert!(result.contains(r#""headline": "Test Article""#));
    assert!(result.contains(r#""url": "https://example.com/posts/test/""#));
    assert!(result.contains(r#""datePublished": "2024-01-15""#));
    assert!(result.contains(r#""publisher":"#));
    assert!(result.contains(r#""name": "My Site""#));
}

#[test]
fn test_build_schema_json_ld_with_keywords() {
    let site_url = test_site_url();
    let result = build_schema_json_ld(
        "Test",
        "Desc",
        "https://example.com/",
        "Site",
        None,
        None,
        &["rust".to_string(), "wasm".to_string()],
        &site_url,
    );

    assert!(result.contains(r#""keywords": ["rust", "wasm"]"#));
}

#[test]
fn test_build_schema_website_basic() {
    let result = build_schema_website(
        "My Site",
        "A site description",
        "https://example.com/",
        "en",
    );

    // Wrapped in the ld+json script tag
    assert!(result.starts_with(r#"<script type="application/ld+json">"#));
    assert!(result.ends_with("</script>"));

    assert!(result.contains(r#""@context": "https://schema.org""#));
    assert!(result.contains(r#""@type": "WebSite""#));
    assert!(result.contains(r#""name": "My Site""#));
    assert!(result.contains(r#""url": "https://example.com/""#));
    // description + inLanguage present when provided
    assert!(result.contains(r#""description": "A site description""#));
    assert!(result.contains(r#""inLanguage": "en""#));
    // publisher block: Organization with the site name
    assert!(result.contains(r#""publisher":"#));
    assert!(result.contains(r#""@type": "Organization""#));
}

#[test]
fn test_build_schema_website_omits_empty_description_and_lang() {
    let result = build_schema_website("My Site", "", "https://example.com/", "");

    assert!(result.contains(r#""@type": "WebSite""#));
    assert!(result.contains(r#""name": "My Site""#));
    assert!(result.contains(r#""url": "https://example.com/""#));
    // Empty description / lang lines are skipped entirely
    assert!(!result.contains("description"));
    assert!(!result.contains("inLanguage"));
    // publisher Organization always present
    assert!(result.contains(r#""@type": "Organization""#));
}

#[test]
fn test_extract_description_strips_markdown() {
    let content = r#"# Heading

This is **bold** and *italic* text.

Here's a [link](https://example.com) and `code`.

```rust
fn main() {}
```

More content here."#;

    let desc = extract_description(content, true);

    assert!(!desc.contains('#'));
    assert!(!desc.contains("**"));
    assert!(!desc.contains('['));
    assert!(!desc.contains("```"));
    assert!(desc.contains("This is bold and italic text"));
}

#[test]
fn test_extract_description_truncates() {
    // Content longer than 160 chars to ensure truncation
    let content = "This is a very long description that should be truncated at a word boundary to ensure it fits within the maximum length limit for meta descriptions. Adding more text here to exceed the limit.";

    let desc = extract_description(content, true);

    assert!(
        desc.len() <= 163,
        "Description too long: {} chars",
        desc.len()
    ); // 160 + "..."
    assert!(desc.ends_with("..."), "Should end with ellipsis: {}", desc);
}

#[test]
fn test_extract_description_short_content() {
    let content = "Short content.";
    let desc = extract_description(content, true);
    assert_eq!(desc, "Short content.");
}

#[test]
fn a_salutation_lead_reads_on_into_the_letter() {
    // A letter's opening line is a paragraph by the blank-line rule, so the
    // description used to be the salutation alone — true to the source and
    // useless as a summary in a folder listing.
    let content = "Dear Friends & Family,\n\n\
         I am writing to you from the TCU, where Michael has been since Tuesday.";

    assert_eq!(
        extract_description(content, true),
        "Dear Friends & Family, I am writing to you from the TCU, where Michael has \
         been since Tuesday.",
        "salutation should be joined to the sentence after it"
    );
}

#[test]
fn a_complete_short_lead_is_left_alone() {
    // The widening applies only where the narrow excerpt said nothing. A
    // short opening that ends like a sentence is a summary already.
    let content = "He died at home.\n\nThe rest of the letter follows here.";

    assert_eq!(extract_description(content, true), "He died at home.");
}

#[test]
fn a_lead_in_colon_reads_on_into_what_it_introduces() {
    let content = "Yesterday I found this on his workbench:\n\n\
         A list of everything he still meant to fix.";

    assert_eq!(
        extract_description(content, true),
        "Yesterday I found this on his workbench: A list of everything he still meant to fix.",
        "a colon lead-in introduces the paragraph after it"
    );
}

#[test]
fn a_long_unpunctuated_lead_does_not_swallow_the_next_paragraph() {
    // Length bounds the reach: past FRAGMENT_LEAD_CHARS there is enough text
    // to summarise with, whatever the punctuation.
    let content = "An opening line long enough to carry its own meaning even with no full stop\n\n\
         A second paragraph that must stay out of the description.";

    assert_eq!(
        extract_description(content, true),
        "An opening line long enough to carry its own meaning even with no full stop",
        "a long lead should stand alone"
    );
}

#[test]
fn a_fragment_reads_on_into_one_paragraph_and_no_further() {
    // A stanza-per-paragraph poem is all fragments; without a hard stop the
    // walk would space-join the whole thing into one run-on line.
    let content = "Roses are red\n\nViolets are blue\n\nSugar is sweet\n\nAnd so are you";

    assert_eq!(
        extract_description(content, true),
        "Roses are red Violets are blue",
        "the read-on reaches exactly one paragraph"
    );
}

#[test]
fn a_fragment_does_not_read_on_across_a_heading() {
    // A recipe's ingredient list is a fragment, but the paragraph under the
    // next heading does not continue it — gluing them makes a run-on that
    // belongs to neither section.
    let content = "## Ingredients\n\n- 400g spaghetti\n- 2 cloves garlic\n\n\
         ## Method\n\nBoil water first.";

    assert_eq!(
        extract_description(content, true),
        "- 400g spaghetti - 2 cloves garlic",
        "a heading between two paragraphs ends the read-on"
    );
}

#[test]
fn a_bare_lead_in_does_not_read_on_into_its_list() {
    // `Contents:` carries no sentence, so it IS a fragment — the list rule is
    // the only thing keeping a page's navigation out of its description.
    let content = "Contents:\n\n- Getting Started\n- FAQ\n- Contact";

    assert_eq!(
        extract_description(content, true),
        "Contents:",
        "a list is introduced by its lead-in, not a continuation of it"
    );
}

#[test]
fn a_chinese_salutation_reads_on_and_keeps_the_letter() {
    // Chinese prose carries no spaces, so the join is the only one in the
    // string: truncating back to the "last word boundary" would drop
    // everything the read-on just added and leave the salutation plus an
    // ellipsis that promises a sentence never shown.
    let body = "在场的各位朋友，这一年里我们经历了太多难以言说的变化，".repeat(8);
    let content = format!("亲爱的朋友们，\n\n{body}");

    let desc = extract_description(&content, true);

    assert!(
        desc.starts_with("亲爱的朋友们， 在场的各位朋友"),
        "the letter itself must survive truncation, got: {desc}"
    );
    assert!(desc.chars().count() > 100, "got a stub: {desc}");
}

#[test]
fn an_abbreviation_in_the_lead_reads_as_a_sentence() {
    // A known and deliberate limit: the full stop in `Dr.` looks like a
    // finished sentence, so this salutation gets no widening. Under-detecting
    // leaves the old, stubby description rather than a wrong one.
    let content = "Dear Dr. Smith,\n\nThank you for seeing Michael on Tuesday.";

    assert_eq!(extract_description(content, true), "Dear Dr. Smith,");
}

#[test]
fn the_preview_path_widens_past_a_stubby_lead_too() {
    // Both public excerpts share `first_paragraph_excerpt`, but only
    // `extract_description` strips callout markers — pin the other path.
    let content = "> Dear Friends,\n\n\
         The service will be held on Saturday at eleven.";

    assert_eq!(
        extract_preview(content, 300, true),
        "Dear Friends, The service will be held on Saturday at eleven."
    );
}

#[test]
fn a_callout_marker_reads_as_a_sentence_in_the_preview_path() {
    // `extract_preview` deliberately leaves `[!type]` markers in place, and
    // the `!` inside one counts as a finished sentence — so a callout
    // salutation keeps the old, narrow behaviour on this path. Same
    // under-detecting trade as `an_abbreviation_in_the_lead_reads_as_a_sentence`;
    // pinned so the coupling is visible if the marker rule ever moves.
    let content = "> [!note] Dear Friends,\n\n\
         The service will be held on Saturday at eleven.";

    assert_eq!(extract_preview(content, 300, true), "[!note] Dear Friends,");
}

#[test]
fn a_lead_closing_on_a_quote_mark_counts_as_a_sentence() {
    // The sentence ended inside the quotes; the closing mark is not proof of
    // a fragment.
    let content = "He wrote, \"Come home.\"\n\nShe read it twice.";

    assert_eq!(
        extract_description(content, true),
        "He wrote, \"Come home.\"",
        "a quoted sentence should stand alone"
    );
}

#[test]
fn a_lead_that_has_said_something_does_not_read_on_past_its_colon() {
    // Trailing punctuation alone does not make a fragment: this opening has
    // already delivered a sentence, and reading on would append the whole
    // list it introduces.
    let content = "Welcome to my vault! Check out:\n\n- Getting Started\n- Pasta Recipe";

    assert_eq!(
        extract_description(content, true),
        "Welcome to my vault! Check out:",
        "an opening carrying a finished sentence should stand alone"
    );
}

#[test]
fn test_escape_html_attr() {
    assert_eq!(
        escape_html_attr(r#"a "b" <c>"#),
        r#"a &quot;b&quot; &lt;c&gt;"#
    );
}

#[test]
fn test_escape_json_string() {
    assert_eq!(escape_json_string("line1\nline2"), r#"line1\nline2"#);
    assert_eq!(escape_json_string(r#"say "hello""#), r#"say \"hello\""#);
}

#[test]
fn test_extract_description_chinese_text() {
    // Chinese text longer than 160 characters - tests UTF-8 boundary handling
    // This text has ~80 Chinese characters, ensuring truncation occurs
    let content = "上个月，美国联邦上诉法院推翻了之前的判决，认定美国财政部对 Tornado Cash 的制裁不合法。至此，Tornado Cash 案件算是告了一个段落。虽然围绕 Tornado Cash 的争议远未结束，现在是时候回顾这桩案件的来龙去脉了。";

    // Should not panic - the bug causes panic on multi-byte UTF-8 boundaries
    let desc = extract_description(content, true);

    // Verify result is valid UTF-8 and truncation works
    assert!(!desc.is_empty(), "Description should not be empty");
    // Character count should be <= 163 (160 + "...")
    let char_count: usize = desc.chars().count();
    assert!(
        char_count <= 163,
        "Should truncate to ~160 chars, got {}",
        char_count
    );
}

#[test]
fn test_extract_description_strips_image_syntax() {
    let content = "![image alt](photo.jpg) Some text after.";
    let desc = extract_description(content, true);
    assert!(
        !desc.starts_with('!'),
        "Description should not start with '!', got: {}",
        desc
    );
    assert_eq!(desc, "Some text after.");
}

#[test]
fn test_extract_description_stops_at_paragraph_break() {
    let content = "First paragraph is short.\n\nSecond paragraph continues here.";
    let desc = extract_description(content, true);
    assert_eq!(desc, "First paragraph is short.");
}

#[test]
fn test_extract_description_truncates_long_first_paragraph() {
    let long_para = "This is a very long first paragraph that goes on and on with many words to ensure it exceeds the one hundred and sixty character limit that we impose for meta descriptions in summary cards.";
    let content = format!("{}\n\nSecond paragraph.", long_para);
    let desc = extract_description(&content, true);
    assert!(desc.ends_with("..."), "Should end with ellipsis: {}", desc);
    assert!(
        desc.chars().count() <= 163,
        "Too long: {} chars",
        desc.chars().count()
    );
    assert!(!desc.contains("Second paragraph"));
}

#[test]
fn test_extract_description_skips_heading_paragraph() {
    let content = "# Just a Heading\n\nThe actual content starts here.";
    let desc = extract_description(content, true);
    assert_eq!(desc, "The actual content starts here.");
}

#[test]
fn test_extract_description_multiple_blank_lines() {
    let content = "First paragraph.\n\n\n\nSecond paragraph.";
    let desc = extract_description(content, true);
    assert_eq!(desc, "First paragraph.");
}

#[test]
fn test_extract_description_single_paragraph_no_break() {
    let content = "Line one.\nLine two.\nLine three.";
    let desc = extract_description(content, true);
    assert_eq!(desc, "Line one. Line two. Line three.");
}

#[test]
fn test_extract_description_skips_code_block_paragraph() {
    // Code block separated from description by blank lines
    let content = "```rust\nfn main() {}\n```\n\nActual description here.";
    let desc = extract_description(content, true);
    // The ``` lines are filtered, but interior lines (fn main) form their own paragraph.
    // The description text is in the second paragraph after the code block interior.
    // This is a known limitation — code block interiors aren't tracked.
    // With a blank line after the code block, the description is a separate paragraph.
    assert!(!desc.contains("```"));

    // When code block is self-contained and followed by real content:
    let content2 = "Some intro.\n\n```rust\nfn main() {}\n```\n\nMore text.";
    let desc2 = extract_description(content2, true);
    assert_eq!(desc2, "Some intro.");
}

#[test]
fn test_build_og_tags_website_basic() {
    let site_url = test_site_url();
    let result = build_og_tags_website(
        "Research",
        "My Site",
        "A personal blog about stars and life.",
        "/",
        None,
        None,
        "en_US",
        &site_url,
    );

    assert!(result.contains(r#"og:type" content="website"#));
    // og:title is the page label, og:site_name is the (distinct) site name.
    assert!(result.contains(r#"og:title" content="Research"#));
    assert!(result.contains(r#"og:url" content="/"#));
    assert!(result.contains(r#"og:site_name" content="My Site"#));
    assert!(result.contains(r#"og:description" content="A personal blog about stars and life."#));
    // Should NOT contain article-specific tags
    assert!(!result.contains("article:published_time"));
    assert!(!result.contains("article:tag"));
}

#[test]
fn test_build_og_tags_website_with_cover() {
    let site_url = test_site_url();
    let cover = CoverRef::Local(ServedPath::from_source("images/cover.jpg").unwrap());
    let result = build_og_tags_website(
        "My Site",
        "My Site",
        "Description",
        "/",
        Some(&cover),
        None,
        "en_US",
        &site_url,
    );

    // Local cover resolved via site_url
    assert!(result.contains(r#"og:image" content="https://example.com/images/cover.jpg"#));
}

#[test]
fn test_extract_description_strips_blockquote_prefix() {
    let content = "> 译自 Galway Kinnell 1980年诗集 *Mortal Acts, Mortal Words*。";
    let desc = extract_description(content, true);
    assert!(!desc.contains('>'));
    assert!(desc.starts_with("译自"));

    // Multi-line blockquote
    let content2 = "> First line of quote.\n> Second line of quote.\n\nNormal paragraph.";
    let desc2 = extract_description(content2, true);
    assert!(!desc2.contains('>'));
    assert_eq!(desc2, "First line of quote. Second line of quote.");

    // No space after >
    let content3 = ">Tight blockquote.";
    let desc3 = extract_description(content3, true);
    assert_eq!(desc3, "Tight blockquote.");
}

// ── Shortcode-fence exclusion (issue: grid/callout syntax leaking
// into auto-extracted child-summary descriptions) ──────────────────
//
// Real repro: a folder note whose ONLY body content is a `:::grid
// N ... :::` shortcode block (wikilinks to sibling pages, `+++`
// cell dividers) had no `description:` frontmatter. The summary/list
// children listing fell back to `extract_description(&doc.content, true)`,
// which filtered out the `:::` fence-marker LINES but not the body
// BETWEEN them — so wikilink display text, `+++` dividers, and the
// closing `:::` all leaked into `.moss-card-description` verbatim.

#[test]
fn test_extract_description_skips_grid_shortcode_body_entirely() {
    // Mirrors the real "Songs of Experience.md" repro: a grid block
    // with wikilink cells, no other prose in the file at all.
    let content = "\
:::grid 2
[[The Tyger]]
+++
[[London]]
+++
[[The Sick Rose]]
+++
[[A Poison Tree]]
+++
[[The Garden of Love]]
+++
[[The Clod & the Pebble]]
:::";
    let desc = extract_description(content, true);
    assert!(
        !desc.contains("+++"),
        "cell divider must not leak, got: {:?}",
        desc
    );
    assert!(
        !desc.contains(":::"),
        "fence markers must not leak, got: {:?}",
        desc
    );
    assert!(
        !desc.contains("Tyger"),
        "cell content must not leak, got: {:?}",
        desc
    );
    // No prose anywhere else in the file — per the "no description,
    // no summary" contract, the excerpt is empty rather than a
    // mangled fallback.
    assert_eq!(
        desc, "",
        "no prose outside the shortcode => empty excerpt, got: {:?}",
        desc
    );
}

#[test]
fn test_extract_description_falls_through_to_prose_after_shortcode() {
    let content = "\
:::grid 2
[[A]]
+++
[[B]]
:::

Real prose paragraph here.";
    let desc = extract_description(content, true);
    assert_eq!(desc, "Real prose paragraph here.");
}

#[test]
fn test_extract_description_prose_before_shortcode_unaffected() {
    // Mirrors "Illuminated Books.md": real prose lede, THEN a grid
    // block. The plain-prose fallback must still work — this is the
    // already-relied-upon behavior the fix must not regress.
    let content = "\
I rest not from my great task!

:::grid 2
[[Songs of Innocence]]
+++
[[Songs of Experience]]
:::";
    let desc = extract_description(content, true);
    assert_eq!(desc, "I rest not from my great task!");
}

#[test]
fn test_extract_description_skips_non_grid_shortcode_types_too() {
    // The typed-shortcode-body exclusion is generic over shortcode NAME,
    // not grid-specific — any TYPED-KNOWN `:::name ... :::` fence (buttons
    // here, and likewise gallery / hero / recent / subscribe / apply) has
    // its body excluded, because a typed shortcode's body is internal
    // grammar (cell content, `+++` dividers), not excerpt-safe prose. The
    // renderer turns these into a sentinel, so the excerpt never sees them.
    //
    // NOTE on the two `:::` shapes whose body is NOT excluded — because the
    // renderer emits their body as genuine visible prose, so the excerpt
    // matches the page:
    //   * a pure-CSS region (`:::{.class}` — EMPTY name, a styling wrapper);
    //   * an UNKNOWN name (`:::typo` — a `moss-unknown-shortcode` fallback
    //     div, with a build warning).
    // Those are pinned by `test_extract_description_keeps_css_region_prose_*`
    // and `test_extract_description_keeps_unknown_shortcode_body`. An earlier
    // revision of this test asserted a `:::{.tagline}` region returned ""
    // (and its comment claimed unknown names were excluded too) — that
    // encoded the CSS-region-prose-drop regression and was corrected once
    // the excerpt walk was consolidated onto the real parser.
    let content = ":::buttons\n[Get started](/start)\n+++\n[Read the docs](/docs)\n:::";
    let desc = extract_description(content, true);
    assert_eq!(desc, "", "buttons body must not leak, got: {:?}", desc);
}

#[test]
fn test_extract_description_keeps_unknown_shortcode_body() {
    // Consolidating onto the real parser (moss-core `extract_shortcodes`)
    // means the excerpt sees exactly what the renderer emits. An UNKNOWN
    // shortcode name renders as a `<div class="moss-unknown-shortcode">`
    // fallback wrapper whose body IS visible on the page (the author also
    // gets a build warning to fix the typo). So the body is excerpt-eligible
    // prose here — the excerpt honestly reflects the rendered page rather
    // than hiding content the visitor can see. (Before the consolidation the
    // old blanket fence-skip dropped this body; that divergence from the
    // renderer is intentionally gone.)
    let content = ":::mysterybox\nReal prose inside an unknown shortcode.\n:::";
    let desc = extract_description(content, true);
    assert_eq!(desc, "Real prose inside an unknown shortcode.");
}

// ── Pure-CSS region prose is REAL visible content ───────────────────
//
// A pure-CSS region (`:::{.class}` / `::::{.class}` — an EMPTY shortcode
// name, just a class wrapper) is NOT internal shortcode grammar the way
// grid/buttons cells are. The real parser (moss-core
// `shortcode_extract.rs`, the `name.is_empty()` branch) renders it as a
// plain `<div class="...">` wrapper and RECURSES into its body, so the
// surrounding prose stays visible on the page while any NESTED typed
// shortcode is still extracted. The excerpt walk must see that same view:
// keep the region's prose, drop only nested typed-shortcode cells.

#[test]
fn test_extract_description_keeps_css_region_prose_reviewer_repro() {
    // Real prose lives inside a `::::{.callout}` styling wrapper, with a
    // typed `:::buttons` shortcode nested in the middle. The buttons body
    // must be dropped, but BOTH surrounding real paragraphs must survive
    // as excerpt-eligible prose. The first one is what we return.
    let content = "\
::::{.callout}
Some real prose users should see.

:::buttons
[Go](/go)
:::

More real prose after the buttons.
::::

Trailing paragraph outside everything.";
    let desc = extract_description(content, true);
    assert_eq!(
        desc, "Some real prose users should see.",
        "CSS-region prose must survive; got: {:?}",
        desc
    );
    assert!(
        !desc.contains("Go"),
        "nested buttons cell must not leak, got: {:?}",
        desc
    );
}

#[test]
fn test_extract_description_keeps_css_region_prose_no_nested_shortcode() {
    // Simplest case: a pure-CSS region wrapping only prose, nothing
    // nested. The prose is genuinely visible content on the page.
    let content = ":::{.tagline}\nA new way to publish.\n:::";
    let desc = extract_description(content, true);
    assert_eq!(desc, "A new way to publish.");
}

#[test]
fn test_extract_description_css_region_wrapping_only_typed_shortcode_is_empty() {
    // A CSS region whose ONLY content is a nested typed shortcode (the
    // common SoCiviC `:::{.support-band}` around `::::buttons` pattern)
    // has no surrounding prose — the excerpt is empty, not the leaked
    // button text.
    let content = "\
:::{.support-band}
::::buttons
[Apply now](/apply)
::::
:::";
    let desc = extract_description(content, true);
    assert_eq!(
        desc, "",
        "no surrounding prose => empty excerpt, got: {:?}",
        desc
    );
}

#[test]
fn test_extract_description_nested_css_regions_keep_prose() {
    // CSS regions nested inside CSS regions: every level is a styling
    // wrapper, so the innermost prose is still real visible content.
    let content = "\
::::{.outer}
:::{.inner}
Deeply wrapped but real prose.
:::
::::";
    let desc = extract_description(content, true);
    assert_eq!(desc, "Deeply wrapped but real prose.");
}

#[test]
fn test_extract_preview_keeps_css_region_prose() {
    // extract_preview shares first_paragraph_excerpt, so it gets the
    // same CSS-region-prose-preservation as extract_description.
    let content = "\
::::{.callout}
Some real prose users should see.

:::buttons
[Go](/go)
:::
::::";
    let preview = extract_preview(content, 300, true);
    assert_eq!(preview, "Some real prose users should see.");
    assert!(
        !preview.contains("Go"),
        "nested buttons cell must not leak, got: {:?}",
        preview
    );
}

#[test]
fn test_extract_description_one_line_empty_body_shortcode_does_not_swallow_rest() {
    // `:::subscribe {button="Go"} :::` opens and closes on the SAME
    // line (spec § "One-line, empty body" shape). If the fence
    // tracker mistook this for an opener with no closer, it would
    // swallow every subsequent line in the file as "still inside a
    // fence" — a much worse regression than the original leak.
    let content = ":::subscribe {button=\"Go\"} :::\n\nReal content after.";
    let desc = extract_description(content, true);
    assert_eq!(desc, "Real content after.");
}

#[test]
fn test_extract_description_nested_shortcode_arity_does_not_leak_or_truncate() {
    // Higher-arity outer fence wrapping a lower-arity inner fence
    // (docs/reference/shortcode-grammar.md "What's reserved").
    // Nothing between the outer opener and closer should leak, and
    // prose after the outer closer must still be reachable.
    let content = "\
::::grid 2
:::buttons
[A](/a)
:::
more cell text
::::

Prose after everything.";
    let desc = extract_description(content, true);
    assert_eq!(desc, "Prose after everything.");
}

#[test]
fn test_extract_description_unclosed_shortcode_fence_does_not_swallow_rest_of_document() {
    // Safety property: moss auto-saves 2s after a keystroke (CM6
    // debounce), so a build can run while the author has typed only
    // an opening `:::name` and hasn't typed its closer yet. An
    // unclosed fence must NOT be treated as "open forever" — that
    // would blank the description for the whole rest of the file
    // every time a build fires mid-edit. Matches
    // shortcode_extract.rs's real-parser contract for unclosed
    // blocks (falls through to verbatim, not swallowed).
    let content = "# Title\n\n:::hero\n\nActual paragraph content here.";
    let desc = extract_description(content, true);
    assert_eq!(desc, "Actual paragraph content here.");
}

#[test]
fn test_extract_description_colons_inside_code_fence_are_inert() {
    // A `:::` shown as a code EXAMPLE (not real shortcode syntax)
    // must not start fence-tracking state that then swallows
    // whatever prose follows.
    let content = "\
```markdown
:::grid 2
:::
```

Real description after the code sample.";
    let desc = extract_description(content, true);
    assert_eq!(desc, "Real description after the code sample.");
}

#[test]
fn test_extract_preview_longer_than_description() {
    let content = "A paragraph that is long enough to test truncation behavior with the preview function which allows more characters than the description.";
    let desc = extract_description(content, true);
    let preview = extract_preview(content, 300, true);
    // Preview should be at least as long as description (same content, higher limit)
    assert!(preview.len() >= desc.len());
}

#[test]
fn test_extract_preview_cjk_content() {
    let content = "文件夹里的每个 `.md` 文件都会变成一个页面，每个子文件夹变成一个栏目。不需要任何配置——在 moss 中打开文件夹，网站结构已经成型。";
    let preview = extract_preview(content, 300, true);
    assert!(preview.contains("文件夹"));
    assert!(!preview.is_empty());
}

#[test]
fn test_extract_preview_skips_headings_and_shortcodes() {
    let content = "# Title\n\n:::hero\n\nActual paragraph content here.";
    let preview = extract_preview(content, 300, true);
    assert!(preview.contains("Actual paragraph"));
}

#[test]
fn test_extract_preview_skips_grid_shortcode_body_entirely() {
    // extract_preview shares first_paragraph_excerpt with
    // extract_description, so it must get the same fix: an article
    // whose body is entirely a `:::grid:::` block (linked from
    // elsewhere, previewed in `.moss-preview-popup`) must not leak
    // `+++`/`:::`/cell text into the preview excerpt.
    let content = ":::grid 2\n[[The Tyger]]\n+++\n[[London]]\n:::";
    let preview = extract_preview(content, 300, true);
    assert!(!preview.contains("+++"), "got: {:?}", preview);
    assert!(!preview.contains(":::"), "got: {:?}", preview);
    assert!(!preview.contains("Tyger"), "got: {:?}", preview);
    assert_eq!(preview, "");
}

#[test]
fn test_extract_preview_empty_content() {
    let content = "";
    let preview = extract_preview(content, 300, true);
    assert!(preview.is_empty());
}

fn make_translation_link(lang: Language, url_path: &str) -> TranslationLink {
    TranslationLink {
        lang_tag: lang.as_bcp47_attr().to_string(),
        url_path: url_path.to_string(),
        display_name: lang.display_name(),
    }
}

#[test]
fn an_unshipped_language_advertises_its_own_tag_not_the_nearest_shipped_one() {
    // `Language` has three variants, so a `fr` page resolves to `En` for the
    // interface. Its `<html lang>` says `fr` (#977) and its hreflang used to
    // say `en` — the two contradicting each other on the same page — while a
    // second `fr` page deduped away against the first as "already seen en".
    let fr = |url: &str| TranslationLink {
        lang_tag: "fr".to_string(),
        url_path: url.to_string(),
        display_name: "EN",
    };
    let html = build_hreflang_link_tags(
        "de",
        "de/about/index.html",
        &[fr("fr/about/index.html"), fr("fr-ca/about/index.html")],
        "example.com",
        "de",
    )
    .expect("expected hreflang block");

    assert!(html.contains(r#"hreflang="de" href="https://example.com/de/about/""#), "{html}");
    assert!(html.contains(r#"hreflang="fr" href="https://example.com/fr/about/""#), "{html}");
    // Same declared tag on both alternates: still deduped, first one wins.
    assert!(!html.contains("fr-ca"), "{html}");
    // x-default resolves against the tag too, not against a `Language`.
    assert!(html.contains(r#"hreflang="x-default" href="https://example.com/de/about/""#), "{html}");
}

#[test]
fn x_default_follows_the_edition_not_the_exact_tag() {
    // Retyping this function from `Language` to declared tags nearly dropped
    // x-default for a site whose declaration carries a region subtag.
    // See `i18n::same_language_edition`.
    let alt = |tag: &str, url: &str| TranslationLink {
        lang_tag: tag.to_string(),
        url_path: url.to_string(),
        display_name: "EN",
    };
    let x_default_of = |site: &str, alt_tag: &str| {
        build_hreflang_link_tags("fr", "fr/about/index.html", &[alt(alt_tag, "about/index.html")], "example.com", site)
            .expect("expected hreflang block")
    };

    // Region subtag on the site. `en-gb` is an allowed declaration but
    // `Language::from_code` rejects region subtags, so it arrives here as
    // `en-GB` and only prefix matching pairs it with an `en` page.
    assert!(
        x_default_of("en-GB", "en").contains(r#"hreflang="x-default" href="https://example.com/about/""#),
        "an `en` page is the x-default of an `en-GB` site"
    );

    // Same primary subtag, different script: two editions, so no x-default at
    // all rather than the wrong one.
    assert!(!x_default_of("zh-Hant", "zh-Hans").contains("x-default"));
}

#[test]
fn hreflang_emits_self_plus_alternates_plus_x_default() {
    let translations = vec![make_translation_link(
        Language::ZhHans,
        "zh-hans/about/index.html",
    )];
    let out = build_hreflang_link_tags(
        Language::En.as_bcp47_attr(),
        "about/index.html",
        &translations,
        "example.com",
        Language::En.as_bcp47_attr(),
    );
    let html = out.expect("expected hreflang block");
    assert!(
        html.contains(r#"<link rel="alternate" hreflang="en" href="https://example.com/about/">"#)
    );
    assert!(html.contains(
        r#"<link rel="alternate" hreflang="zh-Hans" href="https://example.com/zh-hans/about/">"#
    ));
    assert!(html.contains(
        r#"<link rel="alternate" hreflang="x-default" href="https://example.com/about/">"#
    ));
}

#[test]
fn hreflang_stays_per_article_only_no_homepage_root_fallback() {
    // Locks the intentional divergence from the nav switcher: the switcher
    // gains homepage-root fallbacks for translation-less pages, but hreflang
    // must NOT — a homepage root is not a same-content translation, and
    // emitting it as an hreflang alternate would be an invalid-hreflang SEO
    // bug. An EN page with NO authored translations must produce no hreflang
    // block at all (and certainly no fabricated zh-Hans alternate).
    let out = build_hreflang_link_tags(
        Language::En.as_bcp47_attr(),
        "en/writing/post/index.html",
        &[], // no per-article translations
        "example.com",
        Language::ZhHans.as_bcp47_attr(), // site default is ZH
    );
    assert!(
        out.is_none(),
        "no per-article translations => no hreflang block"
    );
    if let Some(s) = out {
        assert!(
            !s.contains("zh-Hans"),
            "hreflang must not fabricate a ZH alternate"
        );
    }
}

#[test]
fn hreflang_x_default_points_to_site_default_lang() {
    let translations = vec![make_translation_link(Language::En, "about/index.html")];
    let out = build_hreflang_link_tags(
        Language::ZhHans.as_bcp47_attr(),
        "zh-hans/about/index.html",
        &translations,
        "example.com",
        Language::En.as_bcp47_attr(),
    );
    let html = out.expect("expected hreflang block");
    assert!(html.contains(
        r#"<link rel="alternate" hreflang="zh-Hans" href="https://example.com/zh-hans/about/">"#
    ));
    assert!(
        html.contains(r#"<link rel="alternate" hreflang="en" href="https://example.com/about/">"#)
    );
    assert!(html.contains(
        r#"<link rel="alternate" hreflang="x-default" href="https://example.com/about/">"#
    ));
}

#[test]
fn hreflang_skips_x_default_when_no_site_default_translation() {
    let translations = vec![make_translation_link(
        Language::ZhHant,
        "zh-hant/about/index.html",
    )];
    let out = build_hreflang_link_tags(
        Language::ZhHans.as_bcp47_attr(),
        "zh-hans/about/index.html",
        &translations,
        "example.com",
        Language::En.as_bcp47_attr(),
    );
    let html = out.expect("expected hreflang block");
    assert!(html.contains(r#"hreflang="zh-Hans""#));
    assert!(html.contains(r#"hreflang="zh-Hant""#));
    assert!(!html.contains(r#"hreflang="x-default""#));
}

#[test]
fn hreflang_returns_none_when_no_translations() {
    let out = build_hreflang_link_tags(
        Language::En.as_bcp47_attr(),
        "about/index.html",
        &[],
        "example.com",
        Language::En.as_bcp47_attr(),
    );
    assert!(
        out.is_none(),
        "single-language pages should emit no hreflang"
    );
}

#[test]
fn hreflang_strips_index_html_suffix() {
    let translations = vec![make_translation_link(
        Language::ZhHans,
        "zh-hans/about/index.html",
    )];
    let out = build_hreflang_link_tags(
        Language::En.as_bcp47_attr(),
        "about/index.html",
        &translations,
        "example.com",
        Language::En.as_bcp47_attr(),
    )
    .unwrap();
    assert!(out.contains(r#"href="https://example.com/about/""#));
    assert!(out.contains(r#"href="https://example.com/zh-hans/about/""#));
    assert!(!out.contains("index.html"));
}

#[test]
fn hreflang_handles_filename_suffix_style() {
    let translations = vec![make_translation_link(
        Language::ZhHans,
        "about.zh-hans.html",
    )];
    let out = build_hreflang_link_tags(
        Language::En.as_bcp47_attr(),
        "about.html",
        &translations,
        "example.com",
        Language::En.as_bcp47_attr(),
    )
    .unwrap();
    assert!(out.contains(r#"href="https://example.com/about.html""#));
    assert!(out.contains(r#"href="https://example.com/about.zh-hans.html""#));
}

#[test]
fn hreflang_three_languages() {
    let translations = vec![
        make_translation_link(Language::ZhHans, "zh-hans/about/index.html"),
        make_translation_link(Language::ZhHant, "zh-hant/about/index.html"),
    ];
    let out = build_hreflang_link_tags(
        Language::En.as_bcp47_attr(),
        "about/index.html",
        &translations,
        "example.com",
        Language::En.as_bcp47_attr(),
    )
    .unwrap();
    assert!(out.contains(r#"hreflang="en""#));
    assert!(out.contains(r#"hreflang="zh-Hans""#));
    assert!(out.contains(r#"hreflang="zh-Hant""#));
    assert!(out.contains(r#"hreflang="x-default""#));
}

#[test]
fn hreflang_strips_trailing_slash_and_scheme_from_host() {
    let translations = vec![make_translation_link(
        Language::ZhHans,
        "zh-hans/about/index.html",
    )];
    let out_with_scheme = build_hreflang_link_tags(
        Language::En.as_bcp47_attr(),
        "about/index.html",
        &translations,
        "https://example.com/",
        Language::En.as_bcp47_attr(),
    )
    .unwrap();
    assert!(out_with_scheme.contains(r#"href="https://example.com/about/""#));
    assert!(!out_with_scheme.contains("https://https://"));
}

#[test]
fn hreflang_strips_leading_slash_from_url_path() {
    let translations = vec![make_translation_link(
        Language::ZhHans,
        "/zh-hans/about/index.html",
    )];
    let out = build_hreflang_link_tags(
        Language::En.as_bcp47_attr(),
        "/about/index.html",
        &translations,
        "example.com",
        Language::En.as_bcp47_attr(),
    )
    .unwrap();
    assert!(
        !out.contains("https://example.com//"),
        "no double-slash after host: {}",
        out
    );
    assert!(out.contains(r#"href="https://example.com/about/""#));
    assert!(out.contains(r#"href="https://example.com/zh-hans/about/""#));
}

#[test]
fn hreflang_escapes_html_unsafe_chars_in_url_path() {
    let translations = vec![make_translation_link(
        Language::ZhHans,
        "zh-hans/posts/q&a.html",
    )];
    let out = build_hreflang_link_tags(
        Language::En.as_bcp47_attr(),
        "posts/q&a.html",
        &translations,
        "example.com",
        Language::En.as_bcp47_attr(),
    )
    .unwrap();
    assert!(
        out.contains("q&amp;a.html"),
        "ampersand should be escaped: {}",
        out
    );
    assert!(
        !out.contains("q&a.html"),
        "raw & must not appear in href: {}",
        out
    );
}

#[test]
fn test_build_og_tags_skips_description_when_empty() {
    let site_url = test_site_url();
    let result = build_og_tags(
        "Title",
        "",
        "https://example.com/",
        "Site",
        None,
        None,
        None,
        &[],
        "",
        &site_url,
    );
    assert!(
        !result.contains("og:description"),
        "should skip og:description when empty, got: {result}"
    );
}

#[test]
fn test_build_og_tags_website_skips_description_when_empty() {
    let site_url = test_site_url();
    let result = build_og_tags_website(
        "Title",
        "Test Site",
        "",
        "https://example.com/",
        None,
        None,
        "",
        &site_url,
    );
    assert!(
        !result.contains("og:description"),
        "should skip og:description when empty, got: {result}"
    );
}

#[test]
fn test_build_twitter_tags_skips_description_when_empty() {
    let site_url = test_site_url();
    let result = build_twitter_tags("Title", "", None, None, &site_url);
    assert!(
        !result.contains("twitter:description"),
        "should skip twitter:description when empty, got: {result}"
    );
}

// --- New tests for absolute og:image / twitter:image ---

#[test]
fn og_image_becomes_absolute_when_cover_is_local() {
    let site_url = SiteUrl::parse("https://chps.mosspub.com").unwrap();
    let cover = CoverRef::Local(ServedPath::for_og_card("abc1234567890def").unwrap());
    let tags = build_og_tags(
        "Page Title",
        "Description",
        "https://chps.mosspub.com/page/",
        "Site",
        None,
        Some(&cover),
        Some((1200, 630)),
        &[],
        "",
        &site_url,
    );
    assert!(
        tags.contains(
            r#"og:image" content="https://chps.mosspub.com/_moss/og/abc1234567890def.png"#
        ),
        "og:image must be absolute; got: {}",
        tags
    );
}

#[test]
fn og_image_passes_through_already_absolute_cover() {
    let site_url = SiteUrl::parse("https://chps.mosspub.com").unwrap();
    let cover = CoverRef::External("https://cdn.example.com/photo.jpg".to_string());
    let tags = build_og_tags(
        "Page Title",
        "Description",
        "https://chps.mosspub.com/page/",
        "Site",
        None,
        Some(&cover),
        None,
        &[],
        "",
        &site_url,
    );
    assert!(
        tags.contains(r#"og:image" content="https://cdn.example.com/photo.jpg"#),
        "external absolute URL must pass through unchanged; got: {}",
        tags
    );
}

#[test]
fn twitter_image_becomes_absolute_when_cover_is_local() {
    let site_url = SiteUrl::parse("https://chps.mosspub.com").unwrap();
    let cover = CoverRef::Local(ServedPath::for_og_card("abc1234567890def").unwrap());
    let tags = build_twitter_tags("Page Title", "Description", Some(&cover), None, &site_url);
    assert!(
        tags.contains(
            r#"twitter:image" content="https://chps.mosspub.com/_moss/og/abc1234567890def.png"#
        ),
        "twitter:image must be absolute; got: {}",
        tags
    );
}

#[test]
fn og_image_relative_for_localhost_site_url() {
    // Unconfigured / preview builds use http://localhost(:port) — og:image
    // emits a root-relative path so the Vite dev port doesn't bake into
    // static HTML that may later be served by other means. The previous
    // policy (absolute-even-for-localhost) prioritized crawler resolution,
    // but in practice no crawler ingests localhost URLs anyway, and the
    // dev-port leak (e.g. http://localhost:1420/...) was the active bug.
    // Production builds (https SiteUrl) still emit absolute URLs — see
    // og_image_becomes_absolute_when_cover_is_local.
    let site_url = SiteUrl::parse("http://localhost").unwrap();
    let cover = CoverRef::Local(ServedPath::for_og_card("abc1234567890def").unwrap());
    let tags = build_og_tags(
        "Page Title",
        "Description",
        "/page/",
        "Site",
        None,
        Some(&cover),
        Some((1200, 630)),
        &[],
        "",
        &site_url,
    );
    assert!(
        tags.contains(r#"og:image" content="/_moss/og/abc1234567890def.png"#),
        "og:image must be relative for localhost site URL; got: {}",
        tags
    );
    assert!(
        !tags.contains("http://localhost"),
        "no localhost URL should leak into static og:image; got: {}",
        tags
    );
}

#[test]
fn og_image_is_relative_when_site_url_is_not_deployed() {
    // Regression: preview/dev builds baked http://localhost:1420 into
    // OG/Twitter URLs because resolve_site_url falls back to the Vite
    // dev port and CoverRef::to_absolute_url always absolutizes.
    //
    // NOTE: plan called for ServedPath::from_source("_moss/og/abc123.png")
    // but `_moss/` is a reserved prefix; from_source rejects it. The
    // proper constructor for OG card paths is for_og_card(hex16). Hex
    // value below is the literal "abc123" padded to 16 hex chars.
    let preview = SiteUrl::parse("http://localhost:1420").unwrap();
    let cover_hash = "abc1230000000000";
    let cover =
        CoverRef::Local(crate::build::served_path::ServedPath::for_og_card(cover_hash).unwrap());
    let og_result = build_og_tags(
        "Test Article",
        "A test description",
        "/posts/test/",
        "My Site",
        Some("2024-01-15"),
        Some(&cover),
        Some((1200, 630)),
        &[],
        "en_US",
        &preview,
    );
    assert!(
        og_result.contains(&format!(
            r#"og:image" content="/_moss/og/{}.png"#,
            cover_hash
        )),
        "non-deployed must emit relative og:image. Got: {}",
        og_result
    );
    assert!(
        !og_result.contains("http://localhost"),
        "no localhost URL should leak into static og:image. Got: {}",
        og_result
    );

    // Same gate applies to twitter:image (emitted by build_twitter_tags).
    let twitter_result = build_twitter_tags(
        "Test Article",
        "A test description",
        Some(&cover),
        None,
        &preview,
    );
    assert!(
        twitter_result.contains(&format!(
            r#"twitter:image" content="/_moss/og/{}.png"#,
            cover_hash
        )),
        "twitter:image must also be relative. Got: {}",
        twitter_result
    );
    assert!(
        !twitter_result.contains("http://localhost"),
        "no localhost URL should leak into static twitter:image. Got: {}",
        twitter_result
    );
}

#[test]
fn og_image_is_absolute_when_site_url_is_deployed() {
    // NOTE: plan called for ServedPath::from_source("_moss/og/abc123.png")
    // but `_moss/` is reserved; use for_og_card(hex16) instead.
    let real = SiteUrl::parse("https://liu-guo.com").unwrap();
    let cover_hash = "abc1230000000000";
    let cover =
        CoverRef::Local(crate::build::served_path::ServedPath::for_og_card(cover_hash).unwrap());
    let result = build_og_tags(
        "Test",
        "desc",
        "https://liu-guo.com/",
        "Liu Guo",
        None,
        Some(&cover),
        Some((1200, 630)),
        &[],
        "zh_CN",
        &real,
    );
    assert!(
        result.contains(&format!(
            r#"og:image" content="https://liu-guo.com/_moss/og/{}.png"#,
            cover_hash
        )),
        "deployed must emit absolute og:image. Got: {}",
        result
    );
}

#[test]
fn cover_ref_local_resolves_via_site_url() {
    let sp = ServedPath::for_og_card("87ba30f2b3c09ca9").unwrap();
    let cr = CoverRef::Local(sp);
    let site = SiteUrl::parse("https://example.com").unwrap();
    assert_eq!(
        cr.to_absolute_url(&site),
        "https://example.com/_moss/og/87ba30f2b3c09ca9.png"
    );
}

#[test]
fn cover_ref_external_passes_through() {
    let cr = CoverRef::External("https://cdn.example.com/photo.jpg".to_string());
    let site = SiteUrl::parse("https://moss.example.com").unwrap();
    assert_eq!(
        cr.to_absolute_url(&site),
        "https://cdn.example.com/photo.jpg"
    );
}

// ---- description chain tests (homepage-hero OG fallback) ----

fn desc_inputs<'a>(
    page_description: Option<&'a str>,
    page_hero_overlay_text: Option<&'a str>,
    page_content: &'a str,
    homepage_description: Option<&'a str>,
    homepage_hero_overlay_text: Option<&'a str>,
    homepage_content: Option<&'a str>,
) -> DescriptionChainInputs<'a> {
    DescriptionChainInputs {
        page_description,
        page_hero_overlay_text,
        page_content,
        homepage_description,
        homepage_hero_overlay_text,
        homepage_content,
        math: true,
    }
}

#[test]
fn page_description_wins_over_all_other_rungs() {
    let inputs = desc_inputs(
        Some("Explicit page description."),
        Some("Page hero overlay"),
        "Page body lede.",
        Some("Homepage description"),
        Some("Homepage hero overlay"),
        Some("Homepage body lede."),
    );
    assert_eq!(
        resolve_page_description_with_fallbacks(&inputs).as_deref(),
        Some("Explicit page description."),
    );
}

#[test]
fn page_hero_overlay_beats_page_body() {
    let inputs = desc_inputs(
        None,
        Some("Hero overlay rules."),
        "Body paragraph that loses.",
        None,
        None,
        None,
    );
    assert_eq!(
        resolve_page_description_with_fallbacks(&inputs).as_deref(),
        Some("Hero overlay rules."),
    );
}

#[test]
fn page_body_used_when_no_page_hero() {
    let inputs = desc_inputs(None, None, "Body paragraph used as lede.", None, None, None);
    assert_eq!(
        resolve_page_description_with_fallbacks(&inputs).as_deref(),
        Some("Body paragraph used as lede."),
    );
}

#[test]
fn homepage_description_when_page_empty() {
    let inputs = desc_inputs(
        None,
        None,
        "",
        Some("The site tagline."),
        Some("Home hero"),
        Some("Home body"),
    );
    assert_eq!(
        resolve_page_description_with_fallbacks(&inputs).as_deref(),
        Some("The site tagline."),
    );
}

#[test]
fn homepage_hero_overlay_when_no_homepage_description() {
    let inputs = desc_inputs(
        None,
        None,
        "",
        None,
        Some("Home hero overlay lede."),
        Some("Home body content."),
    );
    assert_eq!(
        resolve_page_description_with_fallbacks(&inputs).as_deref(),
        Some("Home hero overlay lede."),
    );
}

#[test]
fn homepage_body_when_no_homepage_hero() {
    // Mirrors the Yi-website case: home page has no `description:`, hero
    // overlay is empty, but the body has a usable first paragraph.
    let inputs = desc_inputs(
        None,
        None,
        "",
        None,
        None,
        Some("We study interactions between terrestrial ecosystems and the climate."),
    );
    assert_eq!(
        resolve_page_description_with_fallbacks(&inputs).as_deref(),
        Some("We study interactions between terrestrial ecosystems and the climate."),
    );
}

#[test]
fn all_empty_returns_none() {
    let inputs = desc_inputs(None, None, "", None, None, None);
    assert_eq!(resolve_page_description_with_fallbacks(&inputs), None);
}

#[test]
fn page_body_beats_homepage_description() {
    // The asymmetry from covers: for descriptions, the page's own lede
    // outranks the site tagline because body text is meaningful per-page.
    let inputs = desc_inputs(
        None,
        None,
        "Specific lede for this article.",
        Some("Generic site tagline."),
        None,
        None,
    );
    assert_eq!(
        resolve_page_description_with_fallbacks(&inputs).as_deref(),
        Some("Specific lede for this article."),
    );
}

#[test]
fn whitespace_only_rungs_fall_through() {
    let inputs = desc_inputs(
        Some("   \n   "), // whitespace-only frontmatter
        Some(""),         // empty hero overlay
        "",               // empty body
        None,
        None,
        Some("Real homepage body lede."),
    );
    assert_eq!(
        resolve_page_description_with_fallbacks(&inputs).as_deref(),
        Some("Real homepage body lede."),
    );
}

#[test]
fn frontmatter_description_strips_inline_markdown() {
    let inputs = desc_inputs(
        Some("A **bold** claim about [stuff](https://example.com)"),
        None,
        "",
        None,
        None,
        None,
    );
    assert_eq!(
        resolve_page_description_with_fallbacks(&inputs).as_deref(),
        Some("A bold claim about stuff"),
    );
}

/// A footnote marker is a pointer, not prose — it must not leak into a
/// description (the same invariant the typed render paths declare in
/// `gather_text_inline`/`inlines_to_text`).
///
/// Two shapes, in escalating order of damage:
/// - alone, `[^1]` has no `](` pair, so the link loop ignored it and the
///   marker survived verbatim into <meta name="description">, og:/twitter:
///   description, and JSON-LD;
/// - with a real link later in the paragraph, the marker's `[` paired with
///   the LINK's `](` and the stripper deleted the wrong span, mangling the
///   description ("The claim^1] holds; see [the docs for details.").
/// Ordering is therefore load-bearing: markers must be removed BEFORE the
/// link loop runs.
#[test]
fn footnote_markers_are_stripped_from_descriptions() {
    assert_eq!(
        extract_description("The claim[^1] holds under load.\n\n[^1]: the aside\n", true),
        "The claim holds under load.",
    );
    // CASE 2: the marker must not corrupt link parsing.
    assert_eq!(
        extract_description(
            "The claim[^1] holds; see [the docs](https://example.com) for details.\n\n[^1]: aside\n",
            true,
        ),
        "The claim holds; see the docs for details.",
    );
    // Non-marker lookalikes stay: a whitespace label is not a footnote ref.
    assert_eq!(
        strip_markdown_inline("A regex class [^a b] stays prose."),
        "A regex class [^a b] stays prose.",
    );
    // An empty label is not a footnote ref either.
    assert_eq!(strip_markdown_inline("Empty [^] stays."), "Empty [^] stays.");
}

/// The description strips exactly what the body drops, no more. A `[^…]`
/// token is only a reference when the document DEFINES that label — that is
/// the parser's rule (`ctx.index.number(label)`), so it has to be the
/// stripper's rule too. Otherwise a page reads `Use [^0-9] to strip digits.`
/// while its own <meta name="description"> reads `Use to strip digits.`
#[test]
fn undefined_bracket_caret_tokens_stay_in_the_description() {
    // Regex character classes are the common shape, and none of them is a
    // footnote: the page renders every one of these verbatim.
    assert_eq!(
        extract_description("Use [^0-9] to strip non-digits.", true),
        "Use [^0-9] to strip non-digits.",
    );
    assert_eq!(
        extract_description("The path regex [^/]+/[^/]+ has two segments.", true),
        "The path regex [^/]+/[^/]+ has two segments.",
    );
    assert_eq!(
        extract_description("sed 's/[^[:digit:]]//g' cleans it.", true),
        "sed 's/[^[:digit:]]//g' cleans it.",
    );
    // Left in place, an undefined marker must still not pair its `[` with a
    // later link's `](` — the mangling the marker loop was ordered ahead of
    // the link loop to prevent.
    assert_eq!(
        extract_description("Match [^/]+ first; see [the docs](https://example.com) after.", true),
        "Match [^/]+ first; see the docs after.",
    );
    // Frontmatter can never carry a footnote definition, so an explicit
    // `description:` keeps every bracket it was written with.
    assert_eq!(
        strip_markdown_inline("Match [^/]+ in the path."),
        "Match [^/]+ in the path.",
    );
    assert_eq!(
        strip_markdown_inline("The claim[^1] holds under load."),
        "The claim[^1] holds under load.",
    );
}

/// A `[` opens a link only when its MATCHING `]` is followed by `(`. Prose
/// brackets are common (regex classes, kaomoji, footnote markers the document
/// never defines), and pairing one with the next `](` downstream deleted the
/// span between them.
#[test]
fn a_bracket_in_prose_does_not_swallow_the_link_after_it() {
    assert_eq!(
        strip_markdown_inline("A class [a-z] then [the docs](https://example.com) after."),
        "A class [a-z] then the docs after.",
    );
    // Bracket depth, not the first `]`, so link text may hold brackets.
    assert_eq!(
        strip_markdown_inline("See [note [1]](https://example.com) there."),
        "See note [1] there.",
    );
    // An unmatched `[` is prose and must not stop the scan either.
    assert_eq!(
        strip_markdown_inline("Open [ bracket, then [docs](https://example.com)."),
        "Open [ bracket, then docs.",
    );
    // Images still drop whole, and a following link still resolves.
    assert_eq!(
        strip_markdown_inline("![alt](img.png) then [docs](https://example.com)."),
        "then docs.",
    );
}

/// Inside a code span the renderer emits the token verbatim (`Inline::Code`),
/// and no markdown parser looks for a reference there — so a defined label in
/// backticks is documentation ABOUT the marker, not a marker.
#[test]
fn bracket_caret_inside_a_code_span_is_never_a_marker() {
    assert_eq!(
        extract_description("Write `[^1]` where the note belongs.\n\n[^1]: the aside\n", true),
        "Write [^1] where the note belongs.",
    );
    // A defined marker outside the span still goes, in the same paragraph.
    assert_eq!(
        extract_description("Write `[^1]` here[^1] though.\n\n[^1]: the aside\n", true),
        "Write [^1] here though.",
    );
    assert_eq!(
        extract_description("Use `[^0-9]` to strip non-digits.", true),
        "Use [^0-9] to strip non-digits.",
    );
}

/// A `[^label]:` definition line is endnote matter, not lead prose — the
/// renderer hoists it out of the body, so the excerpt must skip it too.
#[test]
fn footnote_definition_lines_are_skipped_by_the_excerpt() {
    // Definition first: the excerpt is the first REAL paragraph.
    assert_eq!(
        extract_description("[^note]: the aside text\n\nThe real lead paragraph.", true),
        "The real lead paragraph.",
    );
    // Marker in the lead + definition below — both must vanish.
    assert_eq!(
        extract_description("The claim[^1] holds.\n\n[^1]: the aside\n", true),
        "The claim holds.",
    );
}

/// The stripper's defined-label set comes from the PARSER, not a line walk.
///
/// A line walk cannot see block context, and every place it guessed
/// differently from the renderer was a bug in one direction or the other:
/// it counted definitions the renderer ignores (inside code), and missed
/// definitions the renderer hoists (inside containers). Both produced a
/// `<meta name="description">` that disagreed with the rendered page about
/// the same characters.
///
/// Mutation check: re-derive `defined_footnotes` from
/// `footnote_definition_label` over `processed.lines()` and the fenced and
/// blockquote cases go red in opposite directions.
#[test]
fn description_footnote_gate_matches_the_renderer_not_the_line_walk() {
    // A definition inside a fenced code block is NOT a definition. The page
    // renders `[^1]` in the prose verbatim, so the description must keep it.
    let fenced = extract_description(
        "Use [^1] to negate a class.\n\n```\n[^1]: not a definition\n```\n",
        true,
    );
    assert!(
        fenced.contains("[^1]"),
        "a fenced `[^1]:` is not a definition — the token must survive: {fenced:?}"
    );

    // Same for an indented (4-space) code block.
    let indented = extract_description("Use [^1] to negate.\n\n    [^1]: not a definition\n", true);
    assert!(
        indented.contains("[^1]"),
        "an indented `[^1]:` is not a definition: {indented:?}"
    );

    // A definition nested in a blockquote IS a definition — the renderer
    // hoists it — so its marker must be stripped from the description.
    let quoted = extract_description("Prose with a note[^a] inside.\n\n> [^a]: the note\n", true);
    assert!(
        !quoted.contains("[^a]"),
        "a blockquote-nested definition is real; its marker must go: {quoted:?}"
    );
    assert!(
        quoted.contains("Prose with a note"),
        "surrounding prose lost: {quoted:?}"
    );

    // And nested in a list item.
    let listed = extract_description("Prose with a note[^b] inside.\n\n- [^b]: the note\n", true);
    assert!(
        !listed.contains("[^b]"),
        "a list-nested definition is real; its marker must go: {listed:?}"
    );
}

/// A code span closes on a run of the SAME length it opened with
/// (CommonMark). Searching for a single backtick closed ``[^1]`` at its own
/// second tick, putting the marker outside the span so a defined label was
/// stripped from a description that renders it as literal code.
#[test]
fn a_double_backtick_span_protects_a_defined_marker() {
    let d = extract_description("Write ``[^1]`` to show the marker.\n\n[^1]: the note\n", true);
    assert!(
        d.contains("[^1]"),
        "a marker inside a double-backtick span renders as code and must survive: {d:?}"
    );
}

/// A footnote definition is a BLOCK, not a line. The excerpt used to skip only
/// the `[^a]:` line, so a note that wrapped onto a second source line handed
/// that line to the description — and because the renderer hoists the whole
/// note into the endnotes, the description quoted text the reader never meets
/// at the top of the page.
///
/// Mutation check: revert to the original single-line `continue` skip and
/// `wrapped` / `multi_para` go red with citation text in the description
/// (`ends` and `below` pass under any of the three implementations — they
/// pin the boundary where a definition's block STOPS). The false-positive /
/// false-negative cases for the parser-derived line set live in
/// `the_excerpt_asks_the_parser_which_lines_are_a_footnote_definition`.
#[test]
fn the_excerpt_skips_a_whole_footnote_definition_not_just_its_first_line() {
    // Lazy continuation: the second line is flush left, still part of the note.
    let wrapped = extract_description(
        "[^a]: The full citation continues\nhere on the next line.\n\nThe article's real opening.\n",
        true,
    );
    assert_eq!(
        wrapped, "The article's real opening.",
        "the note's second line became the description"
    );

    // Indented second paragraph after a blank line — still the note.
    let multi_para = extract_description(
        "[^a]: First paragraph of the note.\n\n    Second paragraph of the note.\n\nThe real opening.\n",
        true,
    );
    assert_eq!(
        multi_para, "The real opening.",
        "the note's second paragraph became the description"
    );

    // The block genuinely ends: an unindented paragraph after the blank line is
    // body prose and must still be reachable.
    let ends = extract_description("[^a]: The note.\n\nThe real opening.\n", true);
    assert_eq!(ends, "The real opening.");

    // A note BELOW the lead paragraph must not swallow the lead.
    let below = extract_description("The real opening.\n\n[^a]: The note\ncontinues here.\n", true);
    assert_eq!(below, "The real opening.");
}

/// The excerpt asks the parser which lines are a footnote definition, because a
/// line test is wrong in BOTH directions and the two earlier versions of this
/// skip each shipped one of them.
///
/// FALSE POSITIVE: a `[^x]:`-shaped line inside a code block is not a
/// definition. Treating it as one latched a skip that ate real prose to end of
/// file, so the page's only paragraph vanished and the description fell through
/// to the site-wide rung — the page then shipped another page's tagline as its
/// social card.
///
/// FALSE NEGATIVE: `> [^a]: note` IS a definition, and the renderer hoists it.
/// A line test that misses it collected the note as prose; because the marker
/// was correctly stripped, the description shipped the citation body with a
/// stray `:` where the marker had been.
#[test]
fn the_excerpt_asks_the_parser_which_lines_are_a_footnote_definition() {
    // False positives — the fenced/indented `[^1]:` is a code sample.
    let fenced = extract_description(
        "```\n[^1]: the definition goes here\n```\nEvery footnote needs a definition like the one above.\n",
        true,
    );
    assert_eq!(
        fenced, "Every footnote needs a definition like the one above.",
        "a fenced code sample of footnote syntax swallowed the article"
    );

    let indented = extract_description(
        "    [^1]: this is a code sample\n    second code line\nReal opening paragraph of the article.\n",
        true,
    );
    assert!(
        indented.contains("Real opening paragraph of the article."),
        "an indented code block swallowed the article: {indented}"
    );

    // A real definition followed by a SIBLING block — not a continuation of it.
    for (name, markdown, want) in [
        (
            "blockquote",
            "[^a]: The citation.\n> A pull quote that opens the article.\n",
            "A pull quote that opens the article.",
        ),
        (
            "heading",
            "[^a]: The citation.\n# Article Title\nOpening paragraph of the article.\n",
            "Opening paragraph of the article.",
        ),
    ] {
        let got = extract_description(markdown, true);
        assert_eq!(got, want, "{name} after a definition was swallowed");
    }

    // False negatives — a definition nested in a container is still a
    // definition, and its body must not become the description.
    for (name, markdown) in [
        ("blockquote", "> [^a]: the note\n\nThe real lead paragraph.\n"),
        ("list item", "- [^b]: the note\n\nThe real lead paragraph.\n"),
    ] {
        let got = extract_description(markdown, true);
        assert_eq!(
            got, "The real lead paragraph.",
            "a {name}-nested definition supplied the description"
        );
    }
}

/// Only the doc-order-FIRST definition of a label is hoisted; a repeat
/// renders its body in place (ADR-035, `footnotes::is_hoisted`), so the
/// repeat's text is the page's visible prose and must stay in the
/// description — marker-less, exactly as the page shows it. Skipping every
/// definition deleted the page's real lead; when the repeat was the ONLY
/// prose, the description fell to the site-wide rung and the page shipped
/// someone else's tagline as its social-card text.
#[test]
fn a_repeated_definitions_body_stays_in_the_description() {
    let lead = extract_description(
        "[^a]: first note\n\n[^a]: The visible opening line of the page.\n\nSecond paragraph.\n",
        true,
    );
    assert_eq!(
        lead, "The visible opening line of the page.",
        "the repeat renders in place, so its body is the page's lead"
    );

    // Same rule inside a container: the repeat's body keeps its quote text.
    let quoted = extract_description(
        "[^a]: first note\n\n> [^a]: A quoted repeat that opens the page.\n\nAfter.\n",
        true,
    );
    assert_eq!(
        quoted, "A quoted repeat that opens the page.",
        "a container-nested repeat still renders in place"
    );
}

/// DELIBERATE deviation from excerpt-mirrors-page, pinned so a reviewer
/// reads intent instead of an accident: a footnote marker inside an image's
/// alt text breaks the image at parse time, so the page renders the whole
/// token as literal prose (`![alt <sup>1</sup> text](x.png)`). The
/// description still skips it — after marker redaction the token is
/// image-shaped again and `strip_markdown_inline`'s shape-based image drop
/// removes it, falling through to the next real paragraph. Quoting garbled
/// syntax in a social card serves the reader worse than the next paragraph
/// does, so the shape-based drop is the chosen behavior, not a gap.
#[test]
fn a_broken_image_paragraph_is_deliberately_not_the_description() {
    let d = extract_description("![alt [^a] text](x.png)\n\nProse para.\n\n[^a]: note\n", true);
    assert_eq!(d, "Prose para.");
}

/// The excerpt parses with the SITE's math setting — the same value the page
/// renders with. On a math-ON site (the default) `$[^a]$` is an equation:
/// pulldown emits no reference event and the page displays the characters,
/// so the description must keep them. A math-blind parse read the token as a
/// marker and deleted characters the published page shows.
///
/// Mutation check: hardcode the excerpt's parses to `math: false` and the
/// first assertion goes red (`$$` where the page shows `$[^a]$`).
#[test]
fn a_marker_inside_math_survives_on_a_math_on_site() {
    let input = "Compare $[^a]$ with the note[^a].\n\n[^a]: the note\n";

    let math_on = extract_description(input, true);
    assert_eq!(
        math_on, "Compare $[^a]$ with the note.",
        "math-ON: `$[^a]$` is an equation the page displays; only the real marker leaves"
    );

    // With math OFF both tokens are markers on the page, and both leave.
    let math_off = extract_description(input, false);
    assert_eq!(
        math_off, "Compare $$ with the note.",
        "math-OFF: both tokens are markers and are dropped"
    );
}

/// The callout-marker heuristic used to fire on ANY line starting with
/// `[!`, not just an actual blockquote line. A paragraph-leading linked
/// image (`[![alt](src)](url)` — the classic README-badge shape, and a
/// real, CommonMark-legal, non-callout construct) also starts with those
/// two bytes: the alt's closing `]` was misread as the callout marker's
/// close, and the mangled remainder (raw image path + link URL syntax)
/// shipped as the published description, blocking fallback to the real
/// next paragraph. Obsidian callout syntax is ALWAYS a blockquote
/// (`> [!note]`), so gating on `is_blockquote_line` is the correct fix,
/// not a special case for images specifically.
#[test]
fn linked_image_opening_paragraph_is_not_mistaken_for_a_callout_marker() {
    let d = extract_description(
        "[![build status](badge.svg)](https://ci.example.com)\n\nThe real first paragraph.\n",
        true,
    );
    assert_eq!(d, "The real first paragraph.");

    let d2 = extract_description(
        "[![alt](img.png)](https://x.dev) rest of para.\n",
        true,
    );
    assert_eq!(d2, "rest of para.");
}

/// A real callout still strips its marker — the gate gained above
/// (`is_blockquote_line`) must not break the feature it protects.
#[test]
fn real_callout_marker_still_strips_inside_a_blockquote() {
    let d = extract_description("> [!note] Custom Title\n> Body text here.\n", true);
    assert_eq!(d, "Custom Title Body text here.");
}

/// `strip_markdown_inline`'s link/image destination scan used to take the
/// FIRST `)` after `](` with no depth counter, while the bracket side
/// already depth-tracked nested `[` (both are `matching_delim` now).
/// CommonMark allows
/// a bare destination to contain balanced parentheses — the canonical case
/// being a Wikipedia disambiguation URL — so an unescaped nested `(...)`
/// truncated the destination early and left the URL tail as residue in the
/// published description / hover excerpt instead of the link cleanly
/// reducing to its link text (or the image cleanly disappearing).
#[test]
fn balanced_parens_in_a_link_or_image_destination_do_not_leak_residue() {
    assert_eq!(
        strip_markdown_inline(
            "Read [the article](https://en.wikipedia.org/wiki/Rust_(programming_language)) for context."
        ),
        "Read the article for context.",
    );
    assert_eq!(
        strip_markdown_inline("See ![diagram](assets/fig_(1).png) here."),
        "See here.",
    );
    assert_eq!(
        strip_markdown_inline("A [link](https://x.dev/a_(b)) mid."),
        "A link mid.",
    );
}

/// `matching_delim` returns `None` when a destination opens a `(` that
/// never closes (the nesting count never returns to zero before the text
/// ends) — e.g. a URL with a stray unescaped `(` and no matching `)`. The
/// stripper used to treat that as "not a link after all" and fall through,
/// leaving the raw `[text](url` syntax in the published description. Same
/// contract as the balanced case above: garbled markdown is not description
/// material even when the page displays it, so the garbled span (which by
/// construction runs to the end of the text — nothing after an unclosed
/// `(` can be a well-formed continuation) is dropped instead of kept.
#[test]
fn an_unbalanced_paren_in_a_link_destination_is_dropped_not_left_raw() {
    assert_eq!(
        strip_markdown_inline("See [doc](https://x.dev/f(x) for details."),
        "See",
    );
    assert_eq!(
        strip_markdown_inline("Before text. ![alt](assets/f(x).png"),
        "Before text.",
    );
}

/// Obsidian callouts are typed blockquotes, and both extra whitespace after
/// the `>` and indentation/nesting of the blockquote itself are legal
/// CommonMark shapes the AST parser promotes to `Block::Callout` all the
/// same. The line-walk callout-marker gate used to only recognize a single
/// `> ` or `>` at column 0, so any of these variants leaked raw `[!type]`
/// marker text (or a stray `>`) into the published description instead of
/// stripping it like the plain `"> [!note]"` case already covered by
/// `real_callout_marker_still_strips_inside_a_blockquote`.
#[test]
fn callout_marker_strips_across_whitespace_indentation_and_nesting_variants() {
    assert_eq!(
        extract_description(">  [!note] Extra space after the marker.\n", true),
        "Extra space after the marker.",
        "two spaces between `>` and `[!note]` must still strip"
    );
    assert_eq!(
        extract_description("  > [!note] Indented blockquote.\n", true),
        "Indented blockquote.",
        "a blockquote indented by leading spaces must still strip"
    );
    assert_eq!(
        extract_description("> > [!note] Nested blockquote.\n", true),
        "Nested blockquote.",
        "a nested blockquote (`> >`) must still strip"
    );
}

/// A local cover's `og:image` must keep the source's own raster extension —
/// never be "optimised" to the webp variant. Facebook's scraper has never
/// ingested webp for `og:image`, and moss emits no `og:image:type`, so a
/// scraper that does not sniff has only the extension to go on. This is the
/// one reason the raster fallback cannot be dropped even once every browser
/// takes the `<source>`.
#[test]
fn og_image_keeps_the_raster_extension_and_never_becomes_webp() {
    let site_url = test_site_url();
    for source in ["images/cover.jpg", "images/cover.png", "images/cover.jpeg"] {
        let cover = CoverRef::Local(ServedPath::from_source(source).unwrap());
        let url = cover.to_meta_url(&site_url);
        assert!(
            !url.ends_with(".webp"),
            "og:image for {source} resolved to {url}; social scrapers need the raster"
        );
        assert!(
            url.ends_with(&source[source.rfind('.').unwrap()..]),
            "og:image for {source} resolved to {url}; extension must survive"
        );
    }
}
