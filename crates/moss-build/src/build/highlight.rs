//! Build-time syntax highlighting for code blocks.
//!
//! Replaces the client-side highlight.js bundle. Code blocks are pre-highlighted
//! during `moss build` using syntect (for general languages) and the moss-core
//! shortcode tokenizer (for `moss` language blocks).

use std::sync::LazyLock;
use syntect::parsing::SyntaxSet;
use syntect::html::{ClassedHTMLGenerator, ClassStyle};

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);

/// Languages that should not be highlighted (pass-through as plain text).
const SKIP_LANGUAGES: &[&str] = &["text", "plaintext", "nohighlight", "tree", "ascii"];

/// Highlight a code block, returning HTML with `<span class="...">` wrappers.
/// The returned HTML does NOT include the outer `<pre><code>` tags.
pub fn highlight_code(code: &str, language: &str) -> String {
    if language.is_empty() || SKIP_LANGUAGES.contains(&language) {
        return html_escape(code);
    }

    if language == "moss" {
        return highlight_moss(code);
    }

    highlight_with_syntect(code, language)
}

fn highlight_moss(code: &str) -> String {
    use moss_core::shortcode_tokens::{
        tokenize_opening_line, tokenize_closing_line, tokenize_divider_line,
        tokens_to_html,
    };

    let mut result = String::new();
    for (i, line) in code.lines().enumerate() {
        if i > 0 {
            result.push('\n');
        }

        let trimmed = line.trim();

        // Try closing line first (just ":::")
        if trimmed == ":::" {
            let tokens = tokenize_closing_line(line);
            result.push_str(&tokens_to_html(line, &tokens));
            continue;
        }

        // Try opening line (":::name ...")
        if trimmed.starts_with(":::") && trimmed.len() > 3 {
            let tokens = tokenize_opening_line(line);
            result.push_str(&tokens_to_html(line, &tokens));
            continue;
        }

        // Try divider line ("---")
        if trimmed == "---" {
            let tokens = tokenize_divider_line(line);
            result.push_str(&tokens_to_html(line, &tokens));
            continue;
        }

        // Regular content line -- HTML escape only
        result.push_str(&html_escape(line));
    }
    result
}

fn highlight_with_syntect(code: &str, language: &str) -> String {
    let syntax_set = &*SYNTAX_SET;

    // Try to find the syntax by name, then by extension
    let syntax = syntax_set
        .find_syntax_by_token(language)
        .or_else(|| syntax_set.find_syntax_by_extension(language));

    let syntax = match syntax {
        Some(s) => s,
        None => return html_escape(code), // Unknown language -> plain text
    };

    let mut generator = ClassedHTMLGenerator::new_with_class_style(
        syntax,
        syntax_set,
        ClassStyle::Spaced,
    );

    for line in syntect::util::LinesWithEndings::from(code) {
        // parse_html_for_line_which_includes_newline can fail on malformed input,
        // but in practice it doesn't for valid syntax definitions.
        let _ = generator.parse_html_for_line_which_includes_newline(line);
    }

    generator.finalize()
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_rust_produces_spans() {
        let result = highlight_code("let x = 1;", "rust");
        assert!(result.contains("<span"), "Expected span elements in: {}", result);
        // syntect emits scope-based classes
        assert!(result.contains("class="), "Expected class attributes in: {}", result);
    }

    #[test]
    fn highlight_moss_produces_hl_classes() {
        let result = highlight_code(":::grid 3\ncontent\n:::", "moss");
        assert!(result.contains("hl-tag"), "Expected hl-tag class in: {}", result);
        assert!(result.contains("hl-punct"), "Expected hl-punct class in: {}", result);
        assert!(result.contains("hl-attr"), "Expected hl-attr class in: {}", result);
    }

    #[test]
    fn highlight_text_returns_escaped() {
        let result = highlight_code("<script>alert('xss')</script>", "text");
        assert!(result.contains("&lt;script&gt;"), "Expected HTML escaping in: {}", result);
        assert!(!result.contains("<span"), "Should not have span elements");
    }

    #[test]
    fn highlight_unknown_language_returns_escaped() {
        let result = highlight_code("hello world", "nonexistent_lang_xyz");
        assert_eq!(result, "hello world");
        assert!(!result.contains("<span"), "Should not have span elements for unknown language");
    }

    #[test]
    fn highlight_empty_language_returns_escaped() {
        let result = highlight_code("hello", "");
        assert_eq!(result, "hello");
    }

    #[test]
    fn highlight_moss_divider() {
        let result = highlight_code("---", "moss");
        assert!(result.contains("hl-punct"), "Expected hl-punct for divider: {}", result);
    }

    #[test]
    fn html_escape_single_quotes() {
        assert_eq!(html_escape("it's"), "it&#39;s");
        assert_eq!(html_escape("&<>\"'"), "&amp;&lt;&gt;&quot;&#39;");
    }
}
