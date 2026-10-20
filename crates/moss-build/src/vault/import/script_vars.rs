//! Read a dialect row's inline script variables into metadata overrides.
//!
//! Some platforms keep a page's date and account only in script
//! (`var ct = "1757394245";`). The row names the variables and a transform
//! from a closed set; this module finds each declaration and applies it.

use super::dialects::{ScriptTarget, ScriptTransform, ScriptVar};
use crate::build::markdown::html_post::decode_html_entities;
use crate::vault::import::scrape::converter::MetadataOverrides;

pub(crate) fn read(html: &str, vars: &[(&str, ScriptVar)]) -> MetadataOverrides {
    let mut out = MetadataOverrides::default();
    for (name, var) in vars {
        let Some(value) = declared_string(html, name).and_then(|raw| transform(&raw, var.transform)) else {
            continue;
        };
        match var.target {
            ScriptTarget::Date => out.date = Some(value),
            ScriptTarget::Publisher => out.publisher = Some(value),
            ScriptTarget::Author => out.author = Some(value),
        }
    }
    out
}

fn transform(raw: &str, t: ScriptTransform) -> Option<String> {
    let value = match t {
        ScriptTransform::Text => decode_html_entities(raw),
        ScriptTransform::EpochDate { utc_offset_secs } => {
            let offset = chrono::FixedOffset::east_opt(utc_offset_secs)?;
            let secs: i64 = raw.trim().parse().ok()?;
            chrono::DateTime::from_timestamp(secs, 0)?.with_timezone(&offset).to_rfc3339()
        }
    };
    let value = value.trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

/// The string literal of the first `var <name> = "…"` string declaration, with an
/// optional `htmlDecode(…)` wrapper. The name must stand alone, so `var
/// sub_ct` is not `ct`; a later reassignment is never reached.
fn declared_string(src: &str, name: &str) -> Option<String> {
    let bytes = src.as_bytes();
    let mut from = 0;
    while let Some(i) = src[from..].find("var") {
        let at = from + i;
        from = at + 3;
        if at > 0 && is_ident(bytes[at - 1]) {
            continue;
        }
        let rest = src[at + 3..].trim_start();
        if rest.len() == src.len() - at - 3 {
            continue; // `var` ran into the next token with no space
        }
        let Some(rest) = rest.strip_prefix(name) else { continue };
        if rest.bytes().next().is_some_and(is_ident) {
            continue;
        }
        let Some(rest) = rest.trim_start().strip_prefix('=') else { continue };
        let mut rest = rest.trim_start();
        if let Some(inner) = rest.strip_prefix("htmlDecode(") {
            rest = inner.trim_start();
        }
        // A minified bundle may declare the same name to a non-literal first.
        if let Some(value) = string_literal(rest) {
            return Some(value);
        }
    }
    None
}

/// The contents of a leading `"…"` or `'…'` literal, escapes resolved.
fn string_literal(s: &str) -> Option<String> {
    let mut chars = s.chars();
    let quote = chars.next().filter(|c| *c == '"' || *c == '\'')?;
    let mut out = String::new();
    loop {
        match chars.next()? {
            c if c == quote => return Some(out),
            '\\' => match chars.next()? {
                'x' => out.push(hex_char(&mut chars, 2)?),
                'u' => out.push(hex_char(&mut chars, 4)?),
                'n' => out.push('\n'),
                't' => out.push('\t'),
                c => out.push(c),
            },
            c => out.push(c),
        }
    }
}

fn hex_char(chars: &mut std::str::Chars, digits: usize) -> Option<char> {
    let hex: String = chars.take(digits).collect();
    char::from_u32(u32::from_str_radix(&hex, 16).ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::import::dialects::{ScriptTarget as T, ScriptTransform as X};

    const PAGE: &str = r#"<script>
        var sub_ct = "1";
        var ct = "1757394245";
        var nickname = htmlDecode("Invented Lanterns \x26amp; Co");
        var author = "";
        var ct = "2";
    </script>"#;

    const VARS: &[(&str, ScriptVar)] = &[
        ("ct", ScriptVar { target: T::Date, transform: X::EpochDate { utc_offset_secs: 8 * 3600 } }),
        ("nickname", ScriptVar { target: T::Publisher, transform: X::Text }),
        ("author", ScriptVar { target: T::Author, transform: X::Text }),
    ];

    #[test]
    fn reads_date_publisher_and_leaves_an_empty_byline_unset() {
        let o = read(PAGE, VARS);
        assert_eq!(o.date.as_deref(), Some("2025-09-09T13:04:05+08:00"));
        assert_eq!(o.publisher.as_deref(), Some("Invented Lanterns & Co"));
        assert_eq!(o.author, None);
    }

    #[test]
    fn a_timestamp_after_local_midnight_lands_on_the_local_date() {
        // 2026-01-31T16:00:30Z is 2026-02-01 00:00:30 at +08:00.
        let page = r#"var ct = '1769875230';"#;
        let o = read(page, &VARS[..1]);
        assert_eq!(o.date.as_deref(), Some("2026-02-01T00:00:30+08:00"));
    }

    #[test]
    fn a_minified_declaration_of_the_same_name_is_skipped() {
        let page = r#"<script>var ct=et(r,G);</script><script>var ct = "1757394245";</script>"#;
        let o = read(page, &VARS[..1]);
        assert_eq!(o.date.as_deref(), Some("2025-09-09T13:04:05+08:00"));
    }

    #[test]
    fn a_name_inside_a_longer_identifier_is_not_matched() {
        let page = r#"var sub_ct = "1757394245"; var myauthor = "x"; var author2 = "y";"#;
        let o = read(page, VARS);
        assert!(o.date.is_none() && o.author.is_none(), "matched a longer identifier");
    }
}
