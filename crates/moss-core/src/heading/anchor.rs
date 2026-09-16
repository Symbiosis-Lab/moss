//! Obsidian-compatible heading anchor generation.
//!
//! Obsidian's anchor algorithm differs from standard slug generation:
//! - Keeps parentheses, colons, commas, periods, ampersand, single quotes,
//!   exclamation marks, and question marks
//! - Strips `#`, `^`, `|`, `[`, `]`, `\`
//! - Replaces spaces and tabs with hyphens
//! - Lowercases all characters
//! - Collapses consecutive hyphens into one
//! - Trims leading/trailing hyphens
//! - Preserves Unicode characters (CJK, accented, etc.)

/// Characters that Obsidian strips from heading anchors.
const STRIPPED: &[char] = &['#', '^', '|', '[', ']', '\\'];

/// Generate a heading anchor matching Obsidian's behavior.
///
/// ```
/// use moss_core::heading::anchor::obsidian_heading_anchor;
/// assert_eq!(obsidian_heading_anchor("Getting Started"), "getting-started");
/// assert_eq!(obsidian_heading_anchor("Step 1: Install"), "step-1:-install");
/// assert_eq!(obsidian_heading_anchor("中文标题"), "中文标题");
/// ```
pub fn obsidian_heading_anchor(heading: &str) -> String {
    let mut result = String::with_capacity(heading.len());

    for ch in heading.chars() {
        if STRIPPED.contains(&ch) {
            // Strip these characters entirely
            continue;
        } else if ch == ' ' || ch == '\t' {
            result.push('-');
        } else {
            // Lowercase and keep everything else (including Unicode, punctuation, etc.)
            for lower in ch.to_lowercase() {
                result.push(lower);
            }
        }
    }

    // Collapse consecutive hyphens into one
    let collapsed = collapse_hyphens(&result);

    // Trim leading/trailing hyphens
    collapsed.trim_matches('-').to_string()
}

/// Replace runs of consecutive hyphens with a single hyphen.
fn collapse_hyphens(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut prev_hyphen = false;

    for ch in s.chars() {
        if ch == '-' {
            if !prev_hyphen {
                result.push('-');
            }
            prev_hyphen = true;
        } else {
            result.push(ch);
            prev_hyphen = false;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_heading() {
        assert_eq!(obsidian_heading_anchor("Getting Started"), "getting-started");
    }

    #[test]
    fn test_preserves_parentheses() {
        assert_eq!(obsidian_heading_anchor("fn(x, y)"), "fn(x,-y)");
    }

    #[test]
    fn test_strips_leading_trailing_whitespace() {
        assert_eq!(obsidian_heading_anchor("  Hello World  "), "hello-world");
    }

    #[test]
    fn test_unicode_preserved() {
        assert_eq!(obsidian_heading_anchor("中文标题"), "中文标题");
    }

    #[test]
    fn test_mixed_content() {
        assert_eq!(obsidian_heading_anchor("Step 1: Install"), "step-1:-install");
    }

    #[test]
    fn test_consecutive_spaces() {
        assert_eq!(obsidian_heading_anchor("hello   world"), "hello-world");
    }

    #[test]
    fn test_strips_brackets() {
        assert_eq!(obsidian_heading_anchor("Array[0]"), "array0");
    }

    #[test]
    fn test_strips_hash() {
        assert_eq!(obsidian_heading_anchor("Section #1"), "section-1");
    }

    #[test]
    fn test_strips_pipe() {
        assert_eq!(obsidian_heading_anchor("A | B"), "a-b");
    }

    #[test]
    fn test_strips_caret() {
        assert_eq!(obsidian_heading_anchor("Note ^ref"), "note-ref");
    }

    #[test]
    fn test_empty_string() {
        assert_eq!(obsidian_heading_anchor(""), "");
    }

    #[test]
    fn test_only_special_chars() {
        assert_eq!(obsidian_heading_anchor("###"), "");
    }

    #[test]
    fn test_accented_characters() {
        assert_eq!(obsidian_heading_anchor("café résumé"), "café-résumé");
    }
}
