//! Editor-facing shortcode scanner.
//!
//! Companion to `extract_shortcodes` (which returns typed AST nodes for the
//! build pipeline). `editor_scan` returns source-position information needed
//! by the CodeMirror plugin: opening-fence ranges, closing-fence ranges,
//! cell-divider ranges (each marked canonical `+++` or deprecated `---`),
//! and a document-level flag saying whether any deprecated divider was used.
//!
//! Pure, no I/O. Safe to call from any Tauri thread.

use serde::{Deserialize, Serialize};

/// Half-open byte-offset range `[from, to)` into the original markdown
/// source. Bytes, not characters: positions are taken straight from
/// `&str` slicing math so they line up with what the editor receives over
/// the wire (the markdown is shipped as a `String`, and CodeMirror's own
/// position model is on the JS side; we never index the source as chars).
///
/// `u32` instead of `usize` so specta maps the fields to TS `number`
/// rather than `string` (JS Number can represent the full u32 range
/// precisely; a 64-bit `usize` cannot fit losslessly). 4 GiB markdown
/// is not a real moss editor scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct EditorRange {
    pub from: u32,
    pub to: u32,
}

/// One cell divider line, and whether the author used the old spelling.
///
/// `legacy_dash` on the whole result says "somewhere in this document";
/// this says *which line*, which is what the editor needs to put a hint
/// next to the divider the author actually typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct EditorDivider {
    /// Source range covering the `+++` or `---` characters only.
    pub range: EditorRange,
    /// True for the deprecated `---` form, false for canonical `+++`.
    pub legacy: bool,
}

/// One shortcode block as seen by the editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct EditorShortcodeBlock {
    /// Opening fence line (e.g. `:::grid 2`).
    pub open: EditorRange,
    /// Closing fence line (e.g. `:::`).
    pub close: EditorRange,
    /// Shortcode name (e.g. "grid", "buttons").
    pub name: String,
    /// Trailing args after the name (e.g. "2", "{.primary}").
    pub args: String,
    /// Top-level cell divider lines (only the dividers at this block's depth;
    /// nested-block dividers belong to their own block entry).
    pub dividers: Vec<EditorDivider>,
}

/// Result of editor-side shortcode scanning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct EditorScanResult {
    pub blocks: Vec<EditorShortcodeBlock>,
    /// True if any divider line in any grid block used the deprecated `---`
    /// form — including inside a block that never closed, whose dividers are
    /// dropped. Per-divider detail lives on [`EditorDivider::legacy`]; this
    /// stays as the cheap document-level answer.
    pub legacy_dash: bool,
}

/// Scan markdown for top-level shortcode blocks, returning source-position
/// information for the editor.
pub fn editor_scan(markdown: &str) -> EditorScanResult {
    let mut blocks = Vec::new();
    let mut legacy_dash = false;

    let mut current: Option<PartialBlock> = None;
    let mut depth: usize = 0;
    // Stack of arities for each nesting level. Entry 0 = outermost block's arity.
    // This prevents an inner ::: from accidentally closing an outer :::grid.
    let mut arity_stack: Vec<usize> = Vec::new();
    let mut current_is_grid: bool = false;

    let mut in_code_fence = false;
    let mut code_fence_marker = String::new();

    // `offset` is `u32` to match `EditorRange`'s field type. Documents larger
    // than 4 GiB aren't a real editor scenario; we'd truncate silently rather
    // than panic in that pathological case.
    let mut offset: u32 = 0;
    for line in markdown.split_inclusive('\n') {
        let line_content = line.strip_suffix('\n').unwrap_or(line);
        let line_len_without_newline = line_content.len();
        let line_start = offset;
        let line_end = offset + line_len_without_newline as u32;

        // Code-fence tracking: stable ``` or ~~~ fences (length >= 3).
        if let Some(fence) = match_code_fence(line_content) {
            if !in_code_fence {
                in_code_fence = true;
                code_fence_marker = fence.to_string();
            } else if code_fence_marker
                .chars()
                .next()
                .is_some_and(|marker_ch| fence.starts_with(marker_ch))
                && fence.len() >= code_fence_marker.len()
            {
                in_code_fence = false;
                code_fence_marker.clear();
            }
            offset += line.len() as u32;
            continue;
        }
        if in_code_fence {
            offset += line.len() as u32;
            continue;
        }

        if depth == 0 {
            if let Some((arity, name, args)) = match_open_fence(line_content) {
                current_is_grid = name == "grid";
                current = Some(PartialBlock {
                    open: EditorRange {
                        from: line_start,
                        to: line_end,
                    },
                    name: name.to_string(),
                    args: args.to_string(),
                    dividers: Vec::new(),
                });
                arity_stack.push(arity);
                depth = 1;
            }
        } else if let Some((inner_arity, _, _)) = match_open_fence(line_content) {
            // Nested opener — push its arity; depth increments.
            arity_stack.push(inner_arity);
            depth += 1;
        } else if let Some(current_arity) = arity_stack.last().copied() {
            // Checks whether this line closes the CURRENT depth level's block.
            // `arity_stack.len() == depth` and `depth > 0` here, so the pattern
            // always matches; written as one so a future invariant break skips
            // the line instead of panicking mid-build.
            if is_close_fence(line_content, current_arity) {
                arity_stack.pop();
                depth -= 1;
                if depth == 0 {
                    if let Some(partial) = current.take() {
                        blocks.push(EditorShortcodeBlock {
                            open: partial.open,
                            close: EditorRange {
                                from: line_start,
                                to: line_end,
                            },
                            name: partial.name,
                            args: partial.args,
                            dividers: partial.dividers,
                        });
                    }
                    current_is_grid = false;
                }
            } else if depth == 1 && current_is_grid {
                // Divider check only applies at depth 1 inside a grid block,
                // and only on lines that are not open/close fences.
                if let Some(divider) =
                    match_divider(line_content, line_start, &mut legacy_dash)
                {
                    if let Some(c) = current.as_mut() {
                        c.dividers.push(divider);
                    }
                }
            }
        }

        offset += line.len() as u32;
    }

    EditorScanResult {
        blocks,
        legacy_dash,
    }
}

/// Match a grid divider line. Recognizes exactly `+++` (canonical) and
/// exactly `---` (deprecated, sets `legacy_dash` to true). Both allow
/// surrounding whitespace but the line must contain nothing else.
///
/// The returned range covers the `+++` or `---` characters only,
/// excluding leading/trailing whitespace.
fn match_divider(
    line: &str,
    line_start: u32,
    legacy_dash: &mut bool,
) -> Option<EditorDivider> {
    let legacy = match line.trim() {
        "+++" => false,
        "---" => true,
        _ => return None,
    };

    let leading_ws = (line.len() - line.trim_start().len()) as u32;
    if legacy {
        *legacy_dash = true;
    }
    Some(EditorDivider {
        range: EditorRange {
            from: line_start + leading_ws,
            to: line_start + leading_ws + 3,
        },
        legacy,
    })
}

struct PartialBlock {
    open: EditorRange,
    name: String,
    args: String,
    dividers: Vec<EditorDivider>,
}

/// Match `:::name args...` or `::::name args...` etc. (arity ≥ 3).
/// Returns `(arity, name, args)` if matched.
fn match_open_fence(line: &str) -> Option<(usize, &str, &str)> {
    let trimmed = line.trim_start();
    let arity = trimmed.bytes().take_while(|b| *b == b':').count();
    if arity < 3 {
        return None;
    }
    let rest = trimmed.get(arity..)?;
    // Match the name-char set used by `parse_shortcode_opener` in
    // `shortcode_extract.rs`: alphanumeric, underscore, hyphen. Plugins can
    // register names like `:::my-widget`, and the editor must recognize them
    // or its depth counter drifts from the build pipeline.
    let name_bytes = rest
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
        .count();
    if name_bytes == 0 {
        return None;
    }
    let name = rest.get(..name_bytes)?;
    let after_name = rest.get(name_bytes..)?;
    let args = after_name.trim();
    Some((arity, name, args))
}

/// Match a closing fence of the given arity (exactly `arity` colons, nothing else).
fn is_close_fence(line: &str, arity: usize) -> bool {
    let trimmed = line.trim();
    trimmed.len() == arity && trimmed.bytes().all(|b| b == b':')
}

/// Return the fence string (sequence of `` ` `` or `~`) if the line is a
/// fenced-code delimiter at the start of the line, else None.
fn match_code_fence(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let ch = trimmed.chars().next()?;
    if ch != '`' && ch != '~' {
        return None;
    }
    let len = trimmed.chars().take_while(|c| *c == ch).count();
    if len < 3 {
        return None;
    }
    trimmed.get(..len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_returns_empty_result() {
        let r = editor_scan("");
        assert!(r.blocks.is_empty());
        assert!(!r.legacy_dash);
    }

    #[test]
    fn finds_single_grid_block_no_dividers() {
        let md = ":::grid 2\nleft\nright\n:::\n";
        let r = editor_scan(md);

        assert_eq!(r.blocks.len(), 1);
        let b = &r.blocks[0];
        assert_eq!(b.name, "grid");
        assert_eq!(b.args, "2");
        assert_eq!(b.open, EditorRange { from: 0, to: 9 });   // ":::grid 2"
        assert_eq!(b.close, EditorRange { from: 21, to: 24 }); // ":::"
        assert!(b.dividers.is_empty());
        assert!(!r.legacy_dash);
    }

    #[test]
    fn nested_blocks_only_emit_outer() {
        // Outer :::grid contains a nested :::buttons. We only emit the outer
        // block; the inner one's open/close fences don't escape.
        let md = ":::grid 2\n:::buttons\n[a](#)\n:::\n:::\n";
        let r = editor_scan(md);

        assert_eq!(r.blocks.len(), 1);
        assert_eq!(r.blocks[0].name, "grid");
    }

    #[test]
    fn unclosed_block_is_dropped() {
        let md = ":::grid 2\nleft\nright\n";
        let r = editor_scan(md);
        assert!(r.blocks.is_empty());
    }

    #[test]
    fn two_sibling_blocks() {
        let md = ":::buttons\n[a](#)\n:::\n\n:::gallery\n[]()\n:::\n";
        let r = editor_scan(md);

        assert_eq!(r.blocks.len(), 2);
        assert_eq!(r.blocks[0].name, "buttons");
        assert_eq!(r.blocks[1].name, "gallery");
    }

    #[test]
    fn grid_with_canonical_plus_divider() {
        let md = ":::grid 2\nleft\n+++\nright\n:::\n";
        let r = editor_scan(md);

        assert_eq!(r.blocks.len(), 1);
        let b = &r.blocks[0];
        assert_eq!(b.dividers.len(), 1);
        // "+++" starts after ":::grid 2\nleft\n" (10 + 5 = 15) and is 3 chars long.
        assert_eq!(b.dividers[0].range, EditorRange { from: 15, to: 18 });
        assert!(!b.dividers[0].legacy);
        assert!(!r.legacy_dash);
    }

    #[test]
    fn grid_with_legacy_dash_divider_is_marked_per_divider_and_document_wide() {
        let md = ":::grid 2\nleft\n---\nright\n:::\n";
        let r = editor_scan(md);

        assert_eq!(r.blocks.len(), 1);
        assert_eq!(r.blocks[0].dividers.len(), 1);
        assert!(r.blocks[0].dividers[0].legacy);
        assert!(r.legacy_dash, "expected legacy_dash flag for --- divider");
    }

    #[test]
    fn mixed_dividers_are_flagged_individually() {
        // The editor decorates only the offending line, so a grid that mixes
        // both spellings must say which divider is which — not just that the
        // document contains one somewhere.
        let md = ":::grid 3\na\n+++\nb\n---\nc\n:::\n";
        let r = editor_scan(md);

        let dividers = &r.blocks[0].dividers;
        assert_eq!(dividers.iter().map(|d| d.legacy).collect::<Vec<_>>(), vec![false, true]);
        // The flagged range points at the `---` the author actually typed.
        let d = dividers[1].range;
        assert_eq!(&md[d.from as usize..d.to as usize], "---");
        assert!(r.legacy_dash);
    }

    #[test]
    fn dividers_only_at_top_depth() {
        // A nested :::buttons block contains a "---" line. That line is INSIDE
        // the nested block, so the outer grid should have zero dividers. (The
        // nested block isn't emitted at all per the nesting rule.)
        let md = ":::grid 2\n:::buttons\n---\n:::\n:::\n";
        let r = editor_scan(md);

        assert_eq!(r.blocks.len(), 1);
        assert!(r.blocks[0].dividers.is_empty());
        // legacy_dash is also false because the --- was inside a buttons block,
        // not a grid divider.
        assert!(!r.legacy_dash);
    }

    #[test]
    fn extra_plus_signs_are_not_divider() {
        // ++++ (four pluses) is NOT a divider — strict 3-char match.
        let md = ":::grid 2\nleft\n++++\nright\n:::\n";
        let r = editor_scan(md);

        assert_eq!(r.blocks.len(), 1);
        assert!(r.blocks[0].dividers.is_empty());
    }

    #[test]
    fn divider_with_leading_whitespace_is_recognized() {
        let md = ":::grid 2\nleft\n  +++\nright\n:::\n";
        let r = editor_scan(md);

        assert_eq!(r.blocks.len(), 1);
        assert_eq!(r.blocks[0].dividers.len(), 1);
        // Range covers the "+++" only, not the leading spaces.
        let div = r.blocks[0].dividers[0].range;
        let line_text = &md[div.from as usize..div.to as usize];
        assert_eq!(line_text, "+++");
    }

    #[test]
    fn hyphenated_names_are_recognized() {
        // Plugins can register shortcode names containing hyphens (e.g.
        // `:::my-widget`). `parse_shortcode_opener` in shortcode_extract.rs
        // accepts these; editor_scan must agree or its depth counter will
        // drift on documents that use them.
        let md = ":::my-widget\nbody\n:::\n";
        let r = editor_scan(md);

        assert_eq!(r.blocks.len(), 1);
        assert_eq!(r.blocks[0].name, "my-widget");
    }

    #[test]
    fn shortcode_open_inside_code_fence_is_inert() {
        let md = "```\n:::grid 2\n```\n";
        let r = editor_scan(md);
        assert!(r.blocks.is_empty());
    }

    #[test]
    fn shortcode_open_after_closing_code_fence_works() {
        let md = "```\nignored\n```\n:::buttons\n[a](#)\n:::\n";
        let r = editor_scan(md);
        assert_eq!(r.blocks.len(), 1);
        assert_eq!(r.blocks[0].name, "buttons");
    }

    #[test]
    fn close_fence_inside_code_fence_does_not_close_outer_shortcode() {
        // Without code-fence tracking, the ::: inside the ``` block would
        // incorrectly close the :::grid block, and then :::buttons after the
        // code fence would look like a second sibling block. With tracking,
        // only the outer ::: after the code fence closes the grid block, so
        // we see exactly 1 block (grid) and 0 extra sibling blocks.
        //
        // Structure:
        //   :::grid 2        <- open grid (depth 0→1)
        //   ```
        //   :::              <- should be inert (inside code fence)
        //   ```
        //   :::buttons       <- opens nested shortcode (depth 1→2), not a sibling
        //   :::              <- closes nested (depth 2→1)
        //   :::              <- closes grid (depth 1→0)
        let md = ":::grid 2\n```\n:::\n```\n:::buttons\n:::\n:::\n";
        let r = editor_scan(md);
        // With correct fence tracking: grid is the only top-level block.
        assert_eq!(r.blocks.len(), 1);
        assert_eq!(r.blocks[0].name, "grid");
    }

    #[test]
    fn nested_four_colon_block_closes_correctly() {
        // ::::buttons inside :::grid uses 4-colon arity.
        // The outer :::grid must close at the final :::, not be corrupted.
        // Byte offsets: ":::grid 2\n"=10, "::::buttons\n"=12, "[a](#)\n"=7, "::::\n"=5, "cell two\n"=9 → 43
        let md = ":::grid 2\n::::buttons\n[a](#)\n::::\ncell two\n:::\n";
        let r = editor_scan(md);

        assert_eq!(r.blocks.len(), 1, "only outer grid should be emitted");
        assert_eq!(r.blocks[0].name, "grid");
        assert_eq!(r.blocks[0].close.from, 43);
    }

    #[test]
    fn four_colon_top_level_block_is_recognized() {
        // A top-level ::::gallery block (4-colon arity) must be scanned correctly.
        let md = "::::gallery\nimg.jpg\n::::\n";
        let r = editor_scan(md);

        assert_eq!(r.blocks.len(), 1);
        assert_eq!(r.blocks[0].name, "gallery");
        assert_eq!(r.blocks[0].open.from, 0);
        assert_eq!(r.blocks[0].close.from, 20); // after "::::gallery\nimg.jpg\n"
    }

    #[test]
    fn mismatched_arity_close_drops_both_blocks() {
        // If ::::buttons (4-colon) is closed with ::: (3-colon), the inner
        // block never closes. The outer :::grid is also dropped as unclosed.
        // Correct: malformed markup produces no blocks.
        let md = ":::grid\n::::buttons\nbody\n:::\n:::\n";
        //                              ^^^ wrong arity — should be ::::
        let r = editor_scan(md);
        assert!(r.blocks.is_empty(),
            "mismatched inner close should leave outer block unclosed too");
    }
}
