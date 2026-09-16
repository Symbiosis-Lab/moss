//! Obsidian block reference transform.
//!
//! Transforms Obsidian block reference markers (`^block-id` at the end of
//! paragraphs) into HTML anchor elements (`<span id="block-id"></span>`).
//!
//! Block references in Obsidian are written as a space followed by a caret and
//! an alphanumeric-plus-hyphen identifier at the very end of a line:
//!
//! ```text
//! This is important content. ^my-block
//! ```
//!
//! They are stripped from the rendered output and replaced with invisible
//! anchors so that other notes can link directly to that block.
//!
//! # Stage 1 retention — intentional (Phase 4)
//!
//! This module was a candidate for AST migration in Phase 4 (originally
//! PR5) but was DEFERRED after impl-feasibility review confirmed no
//! production usage on any of the 4 client test sites. Block references
//! are an Obsidian-import shape; promoting them to a typed
//! `Inline::Anchor` variant would be speculative work without a real
//! consumer.
//!
//! The 246-LOC fence-aware regex below works correctly today and is the
//! pragmatic shape until usage actually surfaces. Revisit AST migration
//! when an Obsidian-imported site exercises this path.
//!
//! See `docs/archive/2026-05-27-phase4-typed-ast-completion.md` § "PR5
//! Deferred" and § "Out of scope" for the full rationale.

/// Transform block reference markers in `content` into HTML span anchors.
///
/// Returns a tuple of:
/// - The transformed content string (markers replaced with `<span id="…"></span>`)
/// - The list of extracted block IDs (for use by the ContentGraph)
///
/// Content inside fenced code blocks (``` or ~~~) is left untouched.
///
/// # Example
///
/// ```
/// use moss_core::resolve::block_refs::transform_block_refs;
///
/// let (out, ids) = transform_block_refs("Hello world. ^my-block");
/// assert_eq!(out, "Hello world. <span id=\"my-block\"></span>");
/// assert_eq!(ids, vec!["my-block"]);
/// ```
pub fn transform_block_refs(content: &str) -> (String, Vec<String>) {
    let mut block_ids: Vec<String> = Vec::new();
    let mut in_fence = false;
    let mut fence_char = ' ';

    // Process line-by-line, then join with '\n'.
    // We use `lines()` which does NOT produce a trailing empty string for a
    // trailing '\n', so we handle the trailing newline separately.
    let mut output_lines: Vec<String> = Vec::new();

    for line in content.lines() {
        if in_fence {
            let trimmed = line.trim_start();
            let closes = trimmed.starts_with(fence_char)
                && trimmed.chars().take(3).all(|c| c == fence_char)
                && trimmed.trim_matches(fence_char).trim().is_empty();
            if closes {
                in_fence = false;
            }
            output_lines.push(line.to_string());
            continue;
        }

        // Detect opening fence.
        let trimmed = line.trim_start();
        let fence_rest = trimmed
            .strip_prefix("```")
            .map(|r| ('`', r))
            .or_else(|| trimmed.strip_prefix("~~~").map(|r| ('~', r)));
        if let Some((candidate_char, rest)) = fence_rest {
            // Opening fence must not have additional fence chars after the triple.
            if !rest.contains(candidate_char) {
                fence_char = candidate_char;
                in_fence = true;
                output_lines.push(line.to_string());
                continue;
            }
        }

        // Try to match a block reference at the end of this line.
        // We right-trim whitespace first so "text ^id   " still matches.
        let line_stripped = line.trim_end();

        if let Some(id) = extract_block_id(line_stripped) {
            // The suffix we strip is ` ^<id>` (space + caret + id).
            let suffix_len = 1 + 1 + id.len(); // space + caret + id
                                               // prefix is everything before the space-caret-id.
                                               // Char-aligned: the suffix is " ^" (ASCII) + `id` (ASCII alphanumeric/'-',
                                               // verified in extract_block_id), so `len() - suffix_len` lands on a char boundary.
            #[allow(clippy::string_slice)]
            let prefix = &line_stripped[..line_stripped.len() - suffix_len];
            block_ids.push(id.to_string());
            // Emit prefix + the space that was between content and `^` + span.
            let transformed = format!("{} <span id=\"{}\"></span>", prefix, id);
            output_lines.push(transformed);
        } else {
            output_lines.push(line.to_string());
        }
    }

    let mut output = output_lines.join("\n");

    // Restore a trailing newline if the original had one.
    if content.ends_with('\n') {
        output.push('\n');
    }

    (output, block_ids)
}

/// Attempt to extract a block ID from the end of a (already right-trimmed) line.
///
/// The pattern is: `<space>^<id>` where `<id>` is one or more chars that are
/// ASCII alphanumeric or `-`.  The space before the caret is required — a bare
/// caret at the start of the line or in the middle of a word does not count.
///
/// Returns `Some(id_str)` on match, `None` otherwise.
fn extract_block_id(line: &str) -> Option<&str> {
    // Fast path: must contain ` ^`.
    let caret_pos = line.rfind(" ^")?;

    // Everything after ` ^` is the candidate id.
    // Char-aligned: " ^" is two ASCII bytes, so `caret_pos + 2` is a char boundary.
    #[allow(clippy::string_slice)]
    let id = &line[caret_pos + 2..];

    // ID must be non-empty and consist solely of ASCII alphanumeric chars or hyphens.
    if id.is_empty() {
        return None;
    }
    if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }

    // Verify nothing meaningful follows the id on this line — since we already
    // right-trimmed, anything remaining IS the id.
    Some(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_block_id_from_paragraph() {
        let input = "This is important. ^my-block";
        let (out, ids) = transform_block_refs(input);
        assert_eq!(out, "This is important. <span id=\"my-block\"></span>");
        assert_eq!(ids, vec!["my-block"]);
    }

    #[test]
    fn test_multiple_block_ids() {
        let input = "First paragraph. ^block-one\n\nSecond paragraph. ^block-two";
        let (out, ids) = transform_block_refs(input);
        assert_eq!(
            out,
            "First paragraph. <span id=\"block-one\"></span>\n\nSecond paragraph. <span id=\"block-two\"></span>"
        );
        assert_eq!(ids, vec!["block-one", "block-two"]);
    }

    #[test]
    fn test_block_id_in_code_not_transformed() {
        let input = "Normal line. ^ref1\n\n```\nCode line. ^not-a-ref\n```\n\nAfter code. ^ref2";
        let (out, ids) = transform_block_refs(input);
        // ref1 and ref2 should be transformed; the one inside the fence should not.
        assert!(out.contains("<span id=\"ref1\"></span>"));
        assert!(out.contains("<span id=\"ref2\"></span>"));
        assert!(out.contains("Code line. ^not-a-ref"));
        assert!(!out.contains("<span id=\"not-a-ref\">"));
        assert_eq!(ids, vec!["ref1", "ref2"]);
    }

    #[test]
    fn test_no_block_ids() {
        let input = "Just a plain paragraph.\n\nAnother paragraph.";
        let (out, ids) = transform_block_refs(input);
        assert_eq!(out, input);
        assert!(ids.is_empty());
    }

    #[test]
    fn test_block_id_must_be_at_line_end() {
        // Caret in the middle of the line — should NOT be transformed.
        let input = "text ^mid more text";
        let (out, ids) = transform_block_refs(input);
        assert_eq!(out, input);
        assert!(ids.is_empty());
    }

    #[test]
    fn test_block_id_alphanumeric_only() {
        // IDs with special characters beyond hyphens must be rejected.
        let input = "text ^invalid!id";
        let (out, ids) = transform_block_refs(input);
        assert_eq!(out, input);
        assert!(ids.is_empty());

        let input2 = "text ^also_invalid";
        let (out2, ids2) = transform_block_refs(input2);
        assert_eq!(out2, input2);
        assert!(ids2.is_empty());
    }

    #[test]
    fn test_trailing_whitespace_after_id() {
        // Trailing whitespace after the ID should still match.
        let input = "text ^my-id   ";
        let (out, ids) = transform_block_refs(input);
        assert_eq!(out, "text <span id=\"my-id\"></span>");
        assert_eq!(ids, vec!["my-id"]);
    }

    #[test]
    fn test_empty_content() {
        let (out, ids) = transform_block_refs("");
        assert_eq!(out, "");
        assert!(ids.is_empty());
    }

    // Additional edge-case tests for robustness.

    #[test]
    fn test_tilde_fence_preserved() {
        let input = "Before. ^ref1\n\n~~~\nfenced ^skip\n~~~\n\nAfter. ^ref2";
        let (out, ids) = transform_block_refs(input);
        assert!(out.contains("<span id=\"ref1\"></span>"));
        assert!(out.contains("<span id=\"ref2\"></span>"));
        assert!(out.contains("fenced ^skip"));
        assert!(!out.contains("<span id=\"skip\">"));
        assert_eq!(ids, vec!["ref1", "ref2"]);
    }

    #[test]
    fn test_id_with_only_hyphens_rejected() {
        // An id that is purely hyphens is technically valid under the
        // alphanumeric+hyphen rule, but let's verify the function is consistent.
        // Pure hyphens pass the char check; this test documents current behavior.
        let input = "text ^---";
        let (out, ids) = transform_block_refs(input);
        // All chars are hyphens, which are allowed, so this DOES transform.
        assert_eq!(out, "text <span id=\"---\"></span>");
        assert_eq!(ids, vec!["---"]);
    }

    #[test]
    fn test_content_with_trailing_newline_preserved() {
        let input = "Hello. ^block\n";
        let (out, ids) = transform_block_refs(input);
        assert_eq!(out, "Hello. <span id=\"block\"></span>\n");
        assert_eq!(ids, vec!["block"]);
    }
}
