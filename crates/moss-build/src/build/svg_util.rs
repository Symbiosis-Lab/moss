//! Small SVG-related utilities shared by the OG card renderer and the
//! favicon rasterizer. Both build SVG strings by interpolating site-config
//! values (theme colors, page titles) and feed the result to usvg/resvg.
//! These helpers guard the interpolation surface — colors must be valid
//! hex strings, otherwise we'd risk attribute-quote-escape attacks if a
//! site-config value ever leaked through unchecked.

/// Returns true iff `s` is a valid CSS-style hex color: `#rgb`, `#rgba`,
/// `#rrggbb`, or `#rrggbbaa`. Used to guard the SVG attribute interpolation
/// path. Anything else (named colors, `var(--token)`, malformed input) is
/// rejected, callers map the failure to their own error type.
pub fn is_valid_hex_color(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'#') {
        return false;
    }
    let rest = &bytes[1..];
    if !matches!(rest.len(), 3 | 4 | 6 | 8) {
        return false;
    }
    rest.iter().all(|b| b.is_ascii_hexdigit())
}

/// Drop every `@media` at-rule from an SVG's inline CSS.
///
/// usvg cannot honour one — it logs `simplecss: The @media rule is not
/// supported. Skipped.` and carries on — and a rasterization has no colour
/// scheme for a query to answer: the PNG is composited by iOS or by a browser
/// long after any query would have been evaluated. moss's own mark carries a
/// `prefers-color-scheme: dark` rule so it stays visible on a dark browser tab
/// strip, which is a live question for the SVG that ships and a settled one
/// for the PNGs cut from it. Stripping before parsing keeps a per-build
/// warning nobody can act on out of CLI output that scripts and agents read.
///
/// An unterminated rule returns the input untouched. A malformed stylesheet is
/// usvg's to complain about, not this function's to silently truncate.
///
/// One pass over `chars`, no slicing: `moss` denies `clippy::string_slice`, and
/// the offset arithmetic a scanning version needs is exactly what that lint is
/// for.
pub fn strip_media_queries(svg: &str) -> String {
    const TOKEN: &str = "@media";
    let mut out = String::with_capacity(svg.len());
    let mut awaiting_brace = false;
    let mut depth = 0usize;

    for c in svg.chars() {
        if depth > 0 {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
        } else if awaiting_brace {
            // the rule's prelude — `screen and (…)` — goes with its body
            if c == '{' {
                awaiting_brace = false;
                depth = 1;
            }
        } else {
            out.push(c);
            if out.ends_with(TOKEN) {
                out.truncate(out.len() - TOKEN.len());
                awaiting_brace = true;
            }
        }
    }

    if awaiting_brace || depth > 0 {
        return svg.to_string();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_a_media_block_and_keeps_the_rest() {
        assert_eq!(
            strip_media_queries(
                "<style>path{fill:#000}@media (prefers-color-scheme:dark){path{fill:#fff}}</style>"
            ),
            "<style>path{fill:#000}</style>"
        );
    }

    #[test]
    fn keeps_svgs_that_have_none() {
        let svg = r##"<svg><path fill="#000" d="M0 0"/></svg>"##;
        assert_eq!(strip_media_queries(svg), svg);
    }

    #[test]
    fn leaves_an_unbalanced_rule_alone_rather_than_truncating() {
        let svg = "<style>@media screen{path{fill:#fff}</style>";
        assert_eq!(strip_media_queries(svg), svg);
    }

    #[test]
    fn accepts_short_and_long_hex() {
        assert!(is_valid_hex_color("#fff"));
        assert!(is_valid_hex_color("#FFF"));
        assert!(is_valid_hex_color("#FfFf"));
        assert!(is_valid_hex_color("#abcdef"));
        assert!(is_valid_hex_color("#ABCDEF"));
        assert!(is_valid_hex_color("#faf8f5ff"));
    }

    #[test]
    fn rejects_invalid() {
        assert!(!is_valid_hex_color(""));
        assert!(!is_valid_hex_color("fff"));         // missing #
        assert!(!is_valid_hex_color("#"));            // empty body
        assert!(!is_valid_hex_color("#ff"));          // wrong length
        assert!(!is_valid_hex_color("#fffff"));       // wrong length
        assert!(!is_valid_hex_color("#gggggg"));      // non-hex
        assert!(!is_valid_hex_color("red"));          // named color
        assert!(!is_valid_hex_color(r#"#fff" onload="x"#)); // injection
        assert!(!is_valid_hex_color("var(--accent)")); // CSS var
    }
}
