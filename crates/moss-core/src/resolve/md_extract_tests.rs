use super::*;

/// The four unaliased wikilink shapes: bare stem / path, link / embed.
/// Four near-identical functions folded into one table (2026-08-03) — same
/// three assertions per row (text, syntax, exact source span).
#[test]
fn wikilink_shapes() {
    let cases: &[(&str, &str, &str, RefSyntax)] = &[
        ("See [[note]] for details.", "[[note]]", "note", RefSyntax::WikilinkStem),
        ("See [[a/b]] here.", "[[a/b]]", "a/b", RefSyntax::WikilinkPath),
        ("![[x]] is an embed.", "![[x]]", "x", RefSyntax::WikilinkStemEmbed),
        ("![[a/b]] embedded.", "![[a/b]]", "a/b", RefSyntax::WikilinkPathEmbed),
    ];
    for (src, token, text, syntax) in cases {
        let refs = extract_md_references(src);
        assert_eq!(refs.len(), 1, "{src}");
        assert_eq!(&refs[0].text, text, "{src}");
        assert_eq!(&refs[0].syntax, syntax, "{src}");
        assert_eq!(&src[refs[0].byte_from..refs[0].byte_to], *token, "{src}");
    }
}

#[test]
fn wikilink_aliased() {
    let src = "See [[stem|Display]] here.";
    let refs = extract_md_references(src);
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].text, "stem");
    assert!(
        matches!(&refs[0].syntax, RefSyntax::WikilinkAliased { display } if display == "Display")
    );
    assert_eq!(&src[refs[0].byte_from..refs[0].byte_to], "[[stem|Display]]");
}

#[test]
fn wikilink_aliased_embed() {
    let src = "![[stem|500]] wide embed.";
    let refs = extract_md_references(src);
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].text, "stem");
    assert!(
        matches!(&refs[0].syntax, RefSyntax::WikilinkAliasedEmbed { display } if display == "500")
    );
    assert_eq!(&src[refs[0].byte_from..refs[0].byte_to], "![[stem|500]]");
}

#[test]
fn markdown_link() {
    let src = "Click [here](page.md) now.";
    let refs = extract_md_references(src);
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].text, "page.md");
    assert!(matches!(&refs[0].syntax, RefSyntax::MarkdownLink { label } if label == "here"));
    assert_eq!(&src[refs[0].byte_from..refs[0].byte_to], "[here](page.md)");
}

#[test]
fn markdown_image() {
    let src = "![alt text](img.png) here.";
    let refs = extract_md_references(src);
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].text, "img.png");
    assert!(matches!(&refs[0].syntax, RefSyntax::MarkdownImage { alt } if alt == "alt text"));
    assert_eq!(
        &src[refs[0].byte_from..refs[0].byte_to],
        "![alt text](img.png)"
    );
}

#[test]
fn external_link_included() {
    let src = "[foo](https://example.com)";
    let refs = extract_md_references(src);
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].text, "https://example.com");
    assert!(matches!(&refs[0].syntax, RefSyntax::MarkdownLink { .. }));
}

#[test]
fn ref_after_fence_is_found() {
    // Regression: the closing-fence line must be advanced past, otherwise
    // the backtick handler swallows the rest of the file and drops every
    // reference after a fenced block.
    let src = "```\n[[skip]]\n```\n[[find]]";
    let refs = extract_md_references(src);
    assert_eq!(
        refs.len(),
        1,
        "exactly the ref after the fence is found, got: {:?}",
        refs
    );
    assert_eq!(refs[0].text, "find");
    assert_eq!(&src[refs[0].byte_from..refs[0].byte_to], "[[find]]");
}

#[test]
fn backslash_escaped_refs_are_skipped() {
    // `\[[note]]` and `\[t](p)` are escaped and must NOT be extracted.
    let src = "Escaped \\[[note]] and \\[t](p.md) but [[real]] counts.";
    let refs = extract_md_references(src);
    assert_eq!(
        refs.len(),
        1,
        "only the unescaped ref should be found, got: {:?}",
        refs
    );
    assert_eq!(refs[0].text, "real");
}

#[test]
fn skip_inline_code_span() {
    let src = "In `` [[note]] `` code.";
    let refs = extract_md_references(src);
    assert_eq!(
        refs.len(),
        0,
        "wikilink inside inline code should be skipped"
    );
}

#[test]
fn multiple_refs_byte_offsets() {
    let src = "[[a]] and [[b]]";
    let refs = extract_md_references(src);
    assert_eq!(refs.len(), 2);
    assert_eq!(&src[refs[0].byte_from..refs[0].byte_to], "[[a]]");
    assert_eq!(&src[refs[1].byte_from..refs[1].byte_to], "[[b]]");
}

#[test]
fn aliased_embed_preserves_alias() {
    // ![[image.png|600]] — pothole is "600"
    let src = "![[image.png|600]]";
    let refs = extract_md_references(src);
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].text, "image.png");
    assert!(
        matches!(&refs[0].syntax, RefSyntax::WikilinkAliasedEmbed { display } if display == "600")
    );
}

// ── Inert-mask migration (2026-08-03) ────────────────────────────────────

#[test]
fn refs_inside_html_comment_are_skipped() {
    // Behaviour CHANGE: the private fence tracker knew nothing about HTML
    // comments, so an authored `<!-- … -->` leaked its refs.
    let src = "<!-- draft: [[note]] and ![](x.png) -->\nLive [[real]].";
    let refs = extract_md_references(src);
    assert_eq!(refs.len(), 1, "only the live ref, got: {:?}", refs);
    assert_eq!(refs[0].text, "real");
}

#[test]
fn refs_inside_indented_code_are_skipped() {
    // Behaviour CHANGE: a 4-space-indented code block is inert.
    let src = "Intro.\n\n    [[in-code]]\n\nAfter [[live]].";
    let refs = extract_md_references(src);
    assert_eq!(refs.len(), 1, "only the live ref, got: {:?}", refs);
    assert_eq!(refs[0].text, "live");
}

#[test]
fn label_containing_code_span_is_preserved() {
    // Load-bearing for the mask migration: positions come from the mask
    // (which blanks the inline code span), but every STRING is sliced from
    // the original source. Slicing the label out of the mask would silently
    // return "a     c".
    let src = "[a `b` c](x.md)";
    let refs = extract_md_references(src);
    assert_eq!(refs.len(), 1, "got: {:?}", refs);
    assert_eq!(refs[0].text, "x.md");
    assert!(
        matches!(&refs[0].syntax, RefSyntax::MarkdownLink { label } if label == "a `b` c"),
        "label must come from the source, not the mask: {:?}",
        refs[0].syntax
    );
}
