//! Tokenizer for shortcode opening, closing, and divider lines.
//!
//! Pure Rust, zero I/O, zero async. Takes a single line and returns
//! a flat list of tokens with byte offsets.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShortcodeTokenType {
    Fence,
    Name,
    Number,
    Ratio,
    BraceOpen,
    BraceClose,
    ClassName,
    Divider,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShortcodeToken {
    #[serde(rename = "type")]
    pub token_type: ShortcodeTokenType,
    pub from: usize,
    pub to: usize,
}

// ---------------------------------------------------------------------------
// Tokenizers
// ---------------------------------------------------------------------------

/// Tokenize an opening shortcode line such as `:::grid 3 1:2 {.profiles}`.
pub fn tokenize_opening_line(line: &str) -> Vec<ShortcodeToken> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut pos = skip_whitespace(bytes, 0);
    let mut tokens = Vec::new();

    // 1. Expect ":::"
    if !bytes[pos..].starts_with(b":::") {
        return tokens;
    }
    tokens.push(ShortcodeToken {
        token_type: ShortcodeTokenType::Fence,
        from: pos,
        to: pos + 3,
    });
    pos += 3;

    // 2. Expect a name: [a-zA-Z_]\w*
    if pos < len && is_name_start(bytes[pos]) {
        let start = pos;
        pos += 1;
        while pos < len && is_word_char(bytes[pos]) {
            pos += 1;
        }
        tokens.push(ShortcodeToken {
            token_type: ShortcodeTokenType::Name,
            from: start,
            to: pos,
        });
    }

    // 3. Arguments loop
    while pos < len {
        // Skip whitespace between arguments
        let new_pos = skip_whitespace(bytes, pos);
        if new_pos >= len {
            break;
        }
        pos = new_pos;

        // Try ratio first (digit+:digit+) — must check before plain number
        if let Some(end) = try_ratio(bytes, pos) {
            tokens.push(ShortcodeToken {
                token_type: ShortcodeTokenType::Ratio,
                from: pos,
                to: end,
            });
            pos = end;
            continue;
        }

        // Try number
        if bytes[pos].is_ascii_digit() {
            let start = pos;
            while pos < len && bytes[pos].is_ascii_digit() {
                pos += 1;
            }
            tokens.push(ShortcodeToken {
                token_type: ShortcodeTokenType::Number,
                from: start,
                to: pos,
            });
            continue;
        }

        // Brace open
        if bytes[pos] == b'{' {
            tokens.push(ShortcodeToken {
                token_type: ShortcodeTokenType::BraceOpen,
                from: pos,
                to: pos + 1,
            });
            pos += 1;
            continue;
        }

        // Class name: . followed by [a-zA-Z_] then [\w-]*
        if bytes[pos] == b'.'
            && pos + 1 < len
            && is_name_start(bytes[pos + 1])
        {
            let start = pos;
            pos += 2; // skip '.' and first char
            while pos < len && is_class_char(bytes[pos]) {
                pos += 1;
            }
            tokens.push(ShortcodeToken {
                token_type: ShortcodeTokenType::ClassName,
                from: start,
                to: pos,
            });
            continue;
        }

        // Brace close
        if bytes[pos] == b'}' {
            tokens.push(ShortcodeToken {
                token_type: ShortcodeTokenType::BraceClose,
                from: pos,
                to: pos + 1,
            });
            pos += 1;
            continue;
        }

        // Unknown character — consume and skip
        pos += 1;
    }

    tokens
}

/// Tokenize a closing shortcode line (`:::`).
pub fn tokenize_closing_line(line: &str) -> Vec<ShortcodeToken> {
    let bytes = line.as_bytes();
    let pos = skip_whitespace(bytes, 0);
    let mut tokens = Vec::new();

    if bytes[pos..].starts_with(b":::") {
        tokens.push(ShortcodeToken {
            token_type: ShortcodeTokenType::Fence,
            from: pos,
            to: pos + 3,
        });
    }

    tokens
}

/// Tokenize a divider line (`+++` canonical, `---` deprecated).
pub fn tokenize_divider_line(line: &str) -> Vec<ShortcodeToken> {
    let bytes = line.as_bytes();
    let pos = skip_whitespace(bytes, 0);
    let mut tokens = Vec::new();

    if bytes[pos..].starts_with(b"+++") || bytes[pos..].starts_with(b"---") {
        tokens.push(ShortcodeToken {
            token_type: ShortcodeTokenType::Divider,
            from: pos,
            to: pos + 3,
        });
    }

    tokens
}

// ---------------------------------------------------------------------------
// HTML rendering
// ---------------------------------------------------------------------------

/// Produce syntax-highlighted HTML for a shortcode line.
///
/// Each token is wrapped in `<span class="hl-TYPE">`. Gaps between tokens
/// (including leading whitespace) are HTML-escaped and emitted as plain text.
pub fn tokens_to_html(line: &str, tokens: &[ShortcodeToken]) -> String {
    let mut out = String::with_capacity(line.len() * 2);
    let mut cursor = 0;

    for tok in tokens {
        // Emit gap before this token
        if tok.from > cursor {
            // Token offsets are produced by the byte-cursor tokenizer above,
            // which only advances on ASCII bytes (`:`, digits, `{`, `}`, `.`,
            // and ASCII name/word chars). Every recorded `from`/`to` therefore
            // lies on a UTF-8 char boundary.
            #[allow(clippy::string_slice)]
            html_escape_into(&line[cursor..tok.from], &mut out);
        }
        let class = css_class(tok.token_type);
        out.push_str("<span class=\"");
        out.push_str(class);
        out.push_str("\">");
        // Same invariant as the gap slice above: tokenizer only records ASCII
        // byte offsets, so `from`/`to` are char-boundary safe.
        #[allow(clippy::string_slice)]
        html_escape_into(&line[tok.from..tok.to], &mut out);
        out.push_str("</span>");
        cursor = tok.to;
    }

    // Trailing text after last token
    if cursor < line.len() {
        // `cursor` was last set to a token's `to` field — an ASCII byte offset
        // produced by the tokenizer, so it sits on a char boundary.
        #[allow(clippy::string_slice)]
        html_escape_into(&line[cursor..], &mut out);
    }

    out
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn skip_whitespace(bytes: &[u8], mut pos: usize) -> usize {
    while pos < bytes.len() && (bytes[pos] == b' ' || bytes[pos] == b'\t') {
        pos += 1;
    }
    pos
}

fn is_name_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

/// CSS class names can contain hyphens in addition to word chars.
fn is_class_char(b: u8) -> bool {
    is_word_char(b) || b == b'-'
}

/// Try to match `\d+:\d+` starting at `pos`.
/// Returns `Some(end)` if matched, `None` otherwise.
fn try_ratio(bytes: &[u8], pos: usize) -> Option<usize> {
    let len = bytes.len();
    if pos >= len || !bytes[pos].is_ascii_digit() {
        return None;
    }

    // Consume first digit run
    let mut i = pos;
    while i < len && bytes[i].is_ascii_digit() {
        i += 1;
    }

    // Must see ':'
    if i >= len || bytes[i] != b':' {
        return None;
    }
    i += 1;

    // Must see at least one digit after ':'
    if i >= len || !bytes[i].is_ascii_digit() {
        return None;
    }
    while i < len && bytes[i].is_ascii_digit() {
        i += 1;
    }

    Some(i)
}

fn css_class(tt: ShortcodeTokenType) -> &'static str {
    match tt {
        ShortcodeTokenType::Fence => "hl-punct",
        ShortcodeTokenType::Name => "hl-tag",
        ShortcodeTokenType::Number => "hl-attr",
        ShortcodeTokenType::Ratio => "hl-val",
        ShortcodeTokenType::BraceOpen => "hl-brace",
        ShortcodeTokenType::BraceClose => "hl-brace",
        ShortcodeTokenType::ClassName => "hl-val",
        ShortcodeTokenType::Divider => "hl-punct",
    }
}

fn html_escape_into(s: &str, out: &mut String) {
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Fixture format mirrors the JSON file.
    #[derive(Deserialize)]
    struct Fixture {
        description: String,
        input: String,
        kind: String,
        expected: Vec<ShortcodeToken>,
    }

    const FIXTURES: &str = include_str!("../../../tests/fixtures/shortcode-tokens.json");

    #[test]
    fn fixture_driven_tests() {
        let fixtures: Vec<Fixture> =
            serde_json::from_str(FIXTURES).expect("failed to parse fixtures JSON");

        for fixture in &fixtures {
            let tokens = match fixture.kind.as_str() {
                "opening" => tokenize_opening_line(&fixture.input),
                "closing" => tokenize_closing_line(&fixture.input),
                "divider" => tokenize_divider_line(&fixture.input),
                other => panic!("unknown kind {:?} in fixture {:?}", other, fixture.description),
            };

            assert_eq!(
                tokens, fixture.expected,
                "FAILED: {}\n  input: {:?}\n  got:      {:?}\n  expected: {:?}",
                fixture.description, fixture.input, tokens, fixture.expected,
            );
        }
    }

    #[test]
    fn tokens_to_html_basic() {
        let line = ":::grid 3";
        let tokens = tokenize_opening_line(line);
        let html = tokens_to_html(line, &tokens);
        assert_eq!(
            html,
            "<span class=\"hl-punct\">:::</span>\
             <span class=\"hl-tag\">grid</span> \
             <span class=\"hl-attr\">3</span>"
        );
    }

    #[test]
    fn tokens_to_html_escapes_special_chars() {
        // Fabricate a line with special HTML characters in a gap.
        let line = ":::tag <>&\"";
        let tokens = tokenize_opening_line(line);
        let html = tokens_to_html(line, &tokens);
        // The gap after the name token contains ` <>&"` — only the gap text
        // gets escaped (the name "tag" is clean).
        assert!(html.contains("&lt;&gt;&amp;&quot;"), "html was: {html}");
    }

    #[test]
    fn tokens_to_html_closing() {
        let line = "  :::";
        let tokens = tokenize_closing_line(line);
        let html = tokens_to_html(line, &tokens);
        assert_eq!(html, "  <span class=\"hl-punct\">:::</span>");
    }

    #[test]
    fn tokens_to_html_divider() {
        let line = "---";
        let tokens = tokenize_divider_line(line);
        let html = tokens_to_html(line, &tokens);
        assert_eq!(html, "<span class=\"hl-punct\">---</span>");
    }
}
