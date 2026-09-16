//! Turn the HTML entities that real text arrives wrapped in back into characters.
//!
//! Three callers in moss need the same small decoder and none of them is an
//! HTML parser:
//!
//! - the article scraper reads JSON-LD out of a `<script>` block, where `<` and
//!   `&` are HTML-special, so titles and bylines arrive as `China&#8217;s` —
//!   `serde_json` parses the JSON faithfully and leaves the entities alone;
//! - the build's HTML post-pass rewrites attribute values that the synthesizer
//!   already escaped, so it has to decode before it can split a URL on `?`/`#`
//!   (a `#` inside `&#39;` is not a fragment);
//! - the orphan sweep compares an author's media reference against the files on
//!   disk, and `&amp;` is not a filename.
//!
//! Deliberately not a full entity table. It covers what those three actually
//! meet: numeric references in both spellings (`&#8217;`, `&#x2019;`) and the
//! handful of named entities an escaper emits (`&amp;`, `&lt;`, `&gt;`,
//! `&quot;`, `&apos;`, `&nbsp;`). Anything else is left exactly as written,
//! which is the safe answer for text that was never entity-encoded to begin
//! with — `Cats & dogs` comes back unchanged.

/// Decode the entity subset above; return everything else byte-for-byte.
///
/// Total: an unrecognized token, a bare `&`, or an unterminated entity all
/// pass through literally rather than being dropped or erroring.
pub fn decode(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some((before, after_amp)) = rest.split_once('&') {
        out.push_str(before);
        match after_amp.split_once(';') {
            Some((token, after_semi)) => match decode_token(token) {
                Some(ch) => {
                    out.push(ch);
                    rest = after_semi;
                }
                // Not an entity we know: emit the `&` literally and rescan
                // from just past it (the `;` may end a later entity).
                None => {
                    out.push('&');
                    rest = after_amp;
                }
            },
            None => {
                out.push('&');
                out.push_str(after_amp);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// The text between `&` and `;`, or `None` when it names nothing we decode.
fn decode_token(token: &str) -> Option<char> {
    if let Some(rest) = token.strip_prefix('#') {
        let n: u32 = if let Some(hex) = rest.strip_prefix(['x', 'X']) {
            u32::from_str_radix(hex, 16).ok()?
        } else {
            rest.parse().ok()?
        };
        return char::from_u32(n);
    }
    match token {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{a0}'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_numeric_and_named_entities() {
        assert_eq!(decode("Finding China&#8217;s Voice"), "Finding China’s Voice");
        assert_eq!(decode("AT&amp;T"), "AT&T");
        assert_eq!(decode("a&#x2014;b"), "a—b");
        assert_eq!(decode("no entities"), "no entities");
    }

    /// The three callers all run this over text that was never encoded, so a
    /// bare ampersand has to survive intact — mangling it would corrupt a
    /// filename in the orphan sweep and a URL in the HTML post-pass.
    #[test]
    fn text_that_was_never_encoded_survives_unchanged() {
        assert_eq!(decode("Cats & dogs"), "Cats & dogs");
        assert_eq!(decode("a&notanentity;b"), "a&notanentity;b");
        assert_eq!(decode("50% & rising"), "50% & rising");
    }

    /// `&#39;` contains a `#`, which the HTML post-pass would otherwise read as
    /// the start of a URL fragment — decoding first is what makes that split
    /// correct, so the decode itself must not stop at the `#`.
    #[test]
    fn a_numeric_entity_is_decoded_whole() {
        assert_eq!(decode("Jo&#39;s photos/a.jpg"), "Jo's photos/a.jpg");
    }
}
