//! Cell-divider helper for the unified shortcode grammar.
//!
//! Shortcodes that take multiple cells (today: `grid`, `buttons`) split
//! their body on lines containing only `+++`. This module is the single
//! place that recognizes that divider.
//!
//! `+++` was chosen over `---` because `---` is a CommonMark thematic
//! break inside a markdown body; reusing it would force a per-shortcode
//! body-parsing rule and break the grammar's "body is always markdown"
//! invariant. See `docs/archive/2026-05-02-shortcode-grammar-design.md`.
//!
//! Backward-compatible by design: a body without any `+++` returns a
//! single-cell list, so existing buttons content (one link per line, no
//! divider) keeps producing the same shortcode.

/// Split `body` on lines containing only `+++` (after trim) into cells.
///
/// Returns at least one cell — a body with no divider produces a
/// single-element vector containing the full body. Surrounding newlines
/// inside each cell are preserved verbatim except for the divider line
/// itself.
///
/// The match is strict: a line must trim to exactly `"+++"`. Variants
/// like `++++` or `+ + +` or trailing comment text on the same line do
/// NOT count, mirroring how CommonMark handles `---` thematic breaks
/// vs. setext headings.
pub fn split_cells(body: &str) -> Vec<String> {
    if body.is_empty() {
        return vec![String::new()];
    }

    let mut cells = Vec::new();
    let mut current = String::new();
    let mut first_line_in_cell = true;

    for line in body.split_inclusive('\n') {
        // Determine if this line is a divider. We strip the trailing
        // newline (if any) before trimming to recognize the line content.
        let content_no_eol = line.strip_suffix('\n').unwrap_or(line);
        if content_no_eol.trim() == "+++" {
            // Push current cell (without the divider line) and reset.
            // Strip the trailing newline that the previous line added,
            // since the divider's leading newline conceptually belongs
            // between cells, not at the cell boundary.
            if let Some(stripped) = current.strip_suffix('\n') {
                current.truncate(stripped.len());
            }
            cells.push(std::mem::take(&mut current));
            first_line_in_cell = true;
            continue;
        }

        // Skip a single leading blank line at the start of a cell so
        // `+++\n\nlink`-style content yields the same cell as `+++\nlink`.
        if first_line_in_cell {
            first_line_in_cell = false;
            if content_no_eol.trim().is_empty() {
                continue;
            }
        }

        current.push_str(line);
    }

    // Drop a trailing newline so the final cell mirrors the others.
    if let Some(stripped) = current.strip_suffix('\n') {
        current.truncate(stripped.len());
    }
    cells.push(current);

    cells
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_divider_returns_single_cell() {
        let cells = split_cells("line one\nline two\n");
        assert_eq!(cells, vec!["line one\nline two"]);
    }

    #[test]
    fn empty_body_returns_one_empty_cell() {
        let cells = split_cells("");
        assert_eq!(cells, vec![""]);
    }

    #[test]
    fn single_divider_splits_into_two_cells() {
        let cells = split_cells("a\n+++\nb\n");
        assert_eq!(cells, vec!["a", "b"]);
    }

    #[test]
    fn two_dividers_three_cells() {
        let cells = split_cells("a\n+++\nb\n+++\nc\n");
        assert_eq!(cells, vec!["a", "b", "c"]);
    }

    #[test]
    fn multi_line_cells_preserved_verbatim() {
        let cells = split_cells("alpha\nbeta\n+++\ngamma\ndelta\n");
        assert_eq!(cells, vec!["alpha\nbeta", "gamma\ndelta"]);
    }

    #[test]
    fn divider_with_trailing_whitespace_recognized() {
        let cells = split_cells("a\n+++   \nb\n");
        assert_eq!(cells, vec!["a", "b"]);
    }

    #[test]
    fn divider_with_leading_whitespace_recognized() {
        let cells = split_cells("a\n  +++\nb\n");
        assert_eq!(cells, vec!["a", "b"]);
    }

    #[test]
    fn four_plus_signs_is_not_a_divider() {
        let cells = split_cells("a\n++++\nb\n");
        assert_eq!(cells, vec!["a\n++++\nb"]);
    }

    #[test]
    fn plus_with_trailing_text_is_not_a_divider() {
        let cells = split_cells("a\n+++ extra\nb\n");
        assert_eq!(cells, vec!["a\n+++ extra\nb"]);
    }

    #[test]
    fn empty_cells_preserved() {
        // Two consecutive dividers leave the middle cell empty.
        let cells = split_cells("a\n+++\n+++\nb\n");
        assert_eq!(cells, vec!["a", "", "b"]);
    }

    #[test]
    fn divider_at_start_creates_empty_first_cell() {
        let cells = split_cells("+++\nb\n");
        assert_eq!(cells, vec!["", "b"]);
    }

    #[test]
    fn divider_at_end_creates_empty_last_cell() {
        let cells = split_cells("a\n+++\n");
        assert_eq!(cells, vec!["a", ""]);
    }

    #[test]
    fn body_without_trailing_newline_works() {
        let cells = split_cells("a\n+++\nb");
        assert_eq!(cells, vec!["a", "b"]);
    }

    #[test]
    fn leading_blank_line_after_divider_is_dropped() {
        // Authors who write `+++\n\nnext cell` get the same content as
        // `+++\nnext cell`. The blank line was a visual separator, not data.
        let cells = split_cells("a\n+++\n\nb\n");
        assert_eq!(cells, vec!["a", "b"]);
    }

    #[test]
    fn buttons_style_one_link_per_line_no_divider() {
        // Backward-compatibility shape: existing :::buttons content.
        // Falls into a single cell — preserves identity for the legacy
        // line-by-line button parser.
        let body = "[Get started](/start)\n[Read the docs](/docs)\n";
        let cells = split_cells(body);
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0], "[Get started](/start)\n[Read the docs](/docs)");
    }

    #[test]
    fn spec_buttons_two_cells_with_divider() {
        // Spec example: each button in its own cell.
        let body = "[Get started](/start)\n+++\n[Read the docs](/docs)\n";
        let cells = split_cells(body);
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0], "[Get started](/start)");
        assert_eq!(cells[1], "[Read the docs](/docs)");
    }

    #[test]
    fn spec_grid_three_ratio_cells() {
        let body = "[Card one](/work/one)\n+++\n[Card two](/work/two)\n+++\n[Card three](/work/three)\n";
        let cells = split_cells(body);
        assert_eq!(cells.len(), 3);
        assert_eq!(cells[0], "[Card one](/work/one)");
        assert_eq!(cells[2], "[Card three](/work/three)");
    }
}
