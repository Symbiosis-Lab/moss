//! A heading may end with `{contents=end}` or a lone `{#id}`, which
//! [`heading_attrs`] reads. Any other attribute block (`## Title {.class}`) is
//! not given a class or an id: moss prints the braces as part of the heading's
//! text and of its anchor id. [`warnings`] finds those headings so the build
//! can say so.
//!
//! It reads the author's lines rather than the parsed events, so a heading
//! inside a fenced div, a grid cell or a hero is reported at its own line of
//! the file, not at a line counted from the start of its block.

use crate::inert_regions::{html_and_math_lines, inert_lines};

/// One warning per ATX heading whose text ends in an attribute block.
/// `line_offset` is the number of file lines above `markdown`.
pub(crate) fn warnings(markdown: &str, line_offset: usize) -> Vec<String> {
    let inert = inert_lines(markdown);
    let quiet = html_and_math_lines(markdown, &inert);
    let silent = |i: usize| inert.get(i).copied().unwrap_or(false) || quiet.get(i).copied().unwrap_or(false);
    markdown
        .lines()
        .enumerate()
        .filter(|(i, _)| !silent(*i))
        .filter_map(|(i, line)| {
            let text = heading_text(line)?;
            let block = attribute_tail(text).filter(|_| heading_attrs(text).is_none())?;
            Some(format!(
                "line {}: the heading's `{block}` is printed as text, in the heading and in its anchor id; moss reads only `{{contents=end}}` and a lone `{{#id}}` on a heading, where an id is letters, digits and `-_.:`. To give the section a class or other attributes, wrap it in a fenced div (`:::{block}` … `:::`) and take the braces off the heading",
                line_offset + i + 1
            ))
        })
        .collect()
}

/// What a heading's trailing attribute block sets. `title` is the text without
/// the block. `{contents=end}` marks where a page's main sections stop; `{#id}`
/// names the heading's anchor. Either may appear alone or together, once each.
pub(crate) struct HeadingAttrs<'a> {
    pub title: &'a str,
    pub id: Option<&'a str>,
    pub contents_end: bool,
}

/// The attributes a heading's trailing `{...}` block sets, or `None` when the
/// text does not end with a block made only of them. Any class, key=value or
/// other token leaves the braces as text.
pub(crate) fn heading_attrs(text: &str) -> Option<HeadingAttrs<'_>> {
    let trimmed = text.trim_end();
    let (head, block) = trimmed.split_at(trimmed.strip_suffix('}')?.rfind('{')?);
    let inner = block.strip_prefix('{')?.strip_suffix('}')?;
    let mut attrs = HeadingAttrs { title: head.trim_end(), id: None, contents_end: false };
    let mut tokens = 0;
    for token in inner.split_whitespace() {
        tokens += 1;
        match token.strip_prefix('#') {
            Some(id) if attrs.id.is_none() && is_anchor(id) => attrs.id = Some(id),
            _ if token == "contents=end" && !attrs.contents_end => attrs.contents_end = true,
            _ => return None,
        }
    }
    (tokens > 0).then_some(attrs)
}

/// An id a heading may name: letters, digits and `-_.:`, so it needs no escaping
/// in the element and can never hold a brace.
fn is_anchor(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
}

#[cfg(test)]
mod tests {
    use super::heading_attrs;

    #[test]
    fn heading_attrs_accepts_only_its_two_forms() {
        let accepted = [
            ("Title {#a}", "Title", Some("a"), false),
            ("Title {#a contents=end}", "Title", Some("a"), true),
            ("Title {contents=end #a}", "Title", Some("a"), true),
            ("Title {contents=end}", "Title", None, true),
            ("Title { #a }", "Title", Some("a"), false),
        ];
        for (text, title, id, contents_end) in accepted {
            let a = heading_attrs(text).unwrap_or_else(|| panic!("{text:?} should be accepted"));
            assert_eq!((a.title, a.id, a.contents_end), (title, id, contents_end), "{text:?}");
        }
        for text in [
            "Title {.w}",
            "Title {data-x=1}",
            "Title {#a #b}",
            "Title {#}",
            "Title {#a/b}",
            "Title {#a b}",
            "Title {contents=end contents=end}",
            "Title {}",
            "Title",
        ] {
            assert!(heading_attrs(text).is_none(), "{text:?} should be left as text");
        }
    }
}

/// The text of an ATX heading line, without its `#`s or closing `#`s.
fn heading_text(line: &str) -> Option<&str> {
    let rest = line.trim_start_matches(' ');
    if line.len() - rest.len() > 3 {
        return None;
    }
    let after = rest.trim_start_matches('#');
    if !(1..=6).contains(&(rest.len() - after.len())) || !(after.is_empty() || after.starts_with([' ', '\t'])) {
        return None;
    }
    let text = after.trim();
    let open = text.trim_end_matches('#');
    Some(if open.is_empty() || open.ends_with([' ', '\t']) { open.trim_end() } else { text })
}

/// The `{...}` that ends `text`, when it reads as attributes: it opens with a
/// class (`.x`), an id (`#x`) or a `key=value` pair.
fn attribute_tail(text: &str) -> Option<&str> {
    let text = text.trim_end();
    let open = text.strip_suffix('}')?.rfind('{')?;
    let block = text.split_at(open).1;
    let inner = block.strip_prefix('{')?.strip_suffix('}')?.trim_start();
    let key = inner.split(['=', ' ']).next().unwrap_or("");
    let attribute = inner.starts_with(['.', '#'])
        || (!key.is_empty() && inner.strip_prefix(key).is_some_and(|r| r.starts_with('=')) && key.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_'));
    attribute.then_some(block)
}
