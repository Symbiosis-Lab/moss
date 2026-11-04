//! Heading anchors: the one rule that turns a heading's words into an id.
//!
//! Every producer and consumer of a heading id goes through
//! [`obsidian_heading_anchor`]: the id the parser gives a heading, the
//! fragment of a `[[Page#Heading]]` link, the section an embed selects, the
//! autocomplete list. The rule matches GitHub's:
//!
//! - Lowercases
//! - Keeps letters and digits of every script (CJK, accented, Cyrillic, ...),
//!   their combining marks, `-` and `_`
//! - Turns each run of spaces, tabs and hyphens into one hyphen
//! - Drops everything else (punctuation, symbols, emoji)
//! - Trims hyphens from both ends
//! - Names a heading with no letters or digits `section`
//!
//! Before this rule punctuation stayed in the id (`klein,-hell.`), which made
//! a link to a heading unusable. [`legacy_heading_anchor`] is that earlier
//! rule, kept so the page can still answer the id a stored link carries.

/// Name given to a heading whose text holds no letter or digit.
const PUNCTUATION_ONLY_ID: &str = "section";

/// Generate the anchor id for a heading's words.
///
/// ```
/// use moss_core::heading::anchor::obsidian_heading_anchor;
/// assert_eq!(obsidian_heading_anchor("Getting Started"), "getting-started");
/// assert_eq!(obsidian_heading_anchor("Klein, hell, unter Backsteingewölbe."), "klein-hell-unter-backsteingewölbe");
/// assert_eq!(obsidian_heading_anchor("中文标题"), "中文标题");
/// ```
pub fn obsidian_heading_anchor(heading: &str) -> String {
    let mut result = String::with_capacity(heading.len());
    let mut pending_hyphen = false;
    for ch in heading.chars() {
        if ch == ' ' || ch == '\t' || ch == '-' {
            pending_hyphen = !result.is_empty();
        } else if ch.is_alphanumeric() || ch == '_' || unicode_normalization::char::is_combining_mark(ch) {
            if pending_hyphen {
                result.push('-');
                pending_hyphen = false;
            }
            result.extend(ch.to_lowercase());
        }
    }
    if result.is_empty() && !heading.trim().is_empty() {
        return PUNCTUATION_ONLY_ID.to_string();
    }
    result
}

/// Characters the earlier rule stripped from heading anchors.
const LEGACY_STRIPPED: &[char] = &['#', '^', '|', '[', ']', '\\'];

/// The earlier anchor rule, which kept punctuation (`klein,-hell.`). Only the
/// alias that keeps a stored link answering uses it.
pub(crate) fn legacy_heading_anchor(heading: &str) -> String {
    let mut result = String::with_capacity(heading.len());

    for ch in heading.chars() {
        if LEGACY_STRIPPED.contains(&ch) {
            continue;
        } else if ch == ' ' || ch == '\t' {
            result.push('-');
        } else {
            for lower in ch.to_lowercase() {
                result.push(lower);
            }
        }
    }

    collapse_hyphens(&result).trim_matches('-').to_string()
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
    fn punctuation_is_dropped_not_kept() {
        assert_eq!(obsidian_heading_anchor("Klein, hell, unter Backsteingewölbe."), "klein-hell-unter-backsteingewölbe");
        assert_eq!(obsidian_heading_anchor("fn(x, y)"), "fnx-y");
        assert_eq!(obsidian_heading_anchor("Step 1: Install"), "step-1-install");
        assert_eq!(obsidian_heading_anchor("It's a \"test\"?!"), "its-a-test");
        assert_eq!(obsidian_heading_anchor("TCP/IP v1.2"), "tcpip-v12");
    }

    #[test]
    fn separators_collapse_and_trim() {
        assert_eq!(obsidian_heading_anchor("  Hello World  "), "hello-world");
        assert_eq!(obsidian_heading_anchor("hello   world"), "hello-world");
        assert_eq!(obsidian_heading_anchor("a - b -- c"), "a-b-c");
        assert_eq!(obsidian_heading_anchor("--x--"), "x");
        assert_eq!(obsidian_heading_anchor("Section #1"), "section-1");
        assert_eq!(obsidian_heading_anchor("A | B"), "a-b");
        assert_eq!(obsidian_heading_anchor("Array[0]"), "array0");
    }

    #[test]
    fn letters_and_digits_of_every_script_are_kept() {
        assert_eq!(obsidian_heading_anchor("中文标题"), "中文标题");
        assert_eq!(obsidian_heading_anchor("中文，标题。"), "中文标题");
        assert_eq!(obsidian_heading_anchor("café résumé"), "café-résumé");
        assert_eq!(obsidian_heading_anchor("Als nächstes"), "als-nächstes");
        assert_eq!(obsidian_heading_anchor("Привет, мир"), "привет-мир");
        assert_eq!(obsidian_heading_anchor("snake_case"), "snake_case");
    }

    #[test]
    fn combining_marks_stay_with_their_letter() {
        assert_eq!(obsidian_heading_anchor("cafe\u{0301} au lait"), "cafe\u{0301}-au-lait");
        assert_eq!(obsidian_heading_anchor("हिन्दी भाषा"), "हिन्दी-भाषा");
    }

    #[test]
    fn emoji_are_dropped() {
        assert_eq!(obsidian_heading_anchor("Launch 🚀 day"), "launch-day");
    }

    #[test]
    fn a_heading_without_letters_or_digits_is_named_section() {
        assert_eq!(obsidian_heading_anchor("###"), "section");
        assert_eq!(obsidian_heading_anchor("?!"), "section");
        assert_eq!(obsidian_heading_anchor("🚀"), "section");
    }

    #[test]
    fn test_empty_string() {
        assert_eq!(obsidian_heading_anchor(""), "");
        assert_eq!(obsidian_heading_anchor("   "), "");
    }

    #[test]
    fn the_rule_is_idempotent() {
        for t in ["Klein, hell.", "a - b", "中文，标题", "cafe\u{0301}", "🚀", "x_y-z"] {
            let once = obsidian_heading_anchor(t);
            assert_eq!(obsidian_heading_anchor(&once), once, "{t:?}");
        }
    }

    #[test]
    fn legacy_rule_keeps_punctuation() {
        assert_eq!(legacy_heading_anchor("Klein, hell, unter Backsteingewölbe."), "klein,-hell,-unter-backsteingewölbe.");
        assert_eq!(legacy_heading_anchor("Step 1: Install"), "step-1:-install");
        assert_eq!(legacy_heading_anchor("Array[0]"), "array0");
    }
}
