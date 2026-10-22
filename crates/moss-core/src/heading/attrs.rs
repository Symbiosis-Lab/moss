//! A heading may end with `{contents=end}`, `{contents-label="Short name"}` or
//! a lone `{#id}`, in any combination, which [`heading_attrs`] reads. Any other attribute block (`## Title {.class}`) is
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
                "line {}: the heading's `{block}` is printed as text, in the heading and in its anchor id; moss reads only `{{contents=end}}`, `{{contents-label=\"Short name\"}}` and a lone `{{#id}}` on a heading, where an id is letters, digits and `-_.:`. To give the section a class or other attributes, wrap it in a fenced div (`:::{block}` … `:::`) and take the braces off the heading",
                line_offset + i + 1
            ))
        })
        .collect()
}

/// What a heading's trailing attribute block sets. `title` is the text without
/// the block. `{contents=end}` marks where a page's main sections stop;
/// `{contents-label="…"}` is the section's name in the contents list, the
/// floating bar and its panel; `{#id}` names the heading's anchor. Each may
/// appear once, alone or with the others, in any order.
pub(crate) struct HeadingAttrs<'a> {
    pub title: &'a str,
    pub id: Option<&'a str>,
    pub contents_end: bool,
    pub contents_label: Option<&'a str>,
}

/// The attributes a heading's trailing `{...}` block sets, or `None` when the
/// text does not end with a block made only of them. Any class, key=value or
/// other token leaves the braces as text. A label is quoted when it holds
/// spaces (`contents-label="Opening hours"`), bare when it is one word.
pub(crate) fn heading_attrs(text: &str) -> Option<HeadingAttrs<'_>> {
    let trimmed = text.trim_end();
    let (head, block) = trimmed.split_at(trimmed.strip_suffix('}')?.rfind('{')?);
    let inner = block.strip_prefix('{')?.strip_suffix('}')?;
    let mut attrs = HeadingAttrs { title: head.trim_end(), id: None, contents_end: false, contents_label: None };
    let mut rest = inner.trim_start();
    if rest.is_empty() {
        return None;
    }
    while !rest.is_empty() {
        const LABEL: &str = "contents-label=";
        let end = match rest.strip_prefix(LABEL).and_then(|v| v.strip_prefix('"')) {
            Some(quoted) => LABEL.len() + 2 + quoted.find('"')?,
            None => rest.find(char::is_whitespace).unwrap_or(rest.len()),
        };
        let (token, after) = rest.split_at(end);
        if !(after.is_empty() || after.starts_with(char::is_whitespace)) {
            return None;
        }
        if let Some(value) = token.strip_prefix(LABEL) {
            let value = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')).unwrap_or(value).trim();
            if attrs.contents_label.is_some() || value.is_empty() || value.contains('"') {
                return None;
            }
            attrs.contents_label = Some(value);
        } else {
            match token.strip_prefix('#') {
                Some(id) if attrs.id.is_none() && is_anchor(id) => attrs.id = Some(id),
                _ if token == "contents=end" && !attrs.contents_end => attrs.contents_end = true,
                _ => return None,
            }
        }
        rest = after.trim_start();
    }
    Some(attrs)
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
    fn heading_attrs_accepts_only_its_three_keys() {
        let accepted = [
            ("Title {#a}", "Title", Some("a"), false),
            ("Title {#a contents=end}", "Title", Some("a"), true),
            ("Title {contents=end #a}", "Title", Some("a"), true),
            ("Title {contents=end}", "Title", None, true),
            ("Title { #a }", "Title", Some("a"), false),
        ];
        for (text, title, id, contents_end) in accepted {
            let a = heading_attrs(text).unwrap_or_else(|| panic!("{text:?} should be accepted"));
            assert_eq!((a.title, a.id, a.contents_end, a.contents_label), (title, id, contents_end, None), "{text:?}");
        }
        for text in [
            "Title {.w}",
            "Title {data-x=1}",
            "Title {#a #b}",
            "Title {#}",
            "Title {#a/b}",
            "Title {#a b}",
            "Title {contents=end contents=end}",
            "Title {contents-label=}",
            "Title {contents-label=\"\"}",
            "Title {contents-label=\"Open}",
            "Title {contents-label=\"A\"B}",
            "Title {contents-label=a contents-label=b}",
            "Title {contents-label=\"A\" .w}",
            "Title {}",
            "Title",
        ] {
            assert!(heading_attrs(text).is_none(), "{text:?} should be left as text");
        }
    }

    #[test]
    fn a_contents_label_combines_with_the_other_keys_in_any_order() {
        for (text, id, end) in [
            (r#"Opening hours and the long way round {#hours contents-label="Opening hours"}"#, Some("hours"), false),
            (r#"Opening hours and the long way round {contents-label="Opening hours" #hours}"#, Some("hours"), false),
            (r#"Opening hours and the long way round {contents-label="Opening hours" contents=end}"#, None, true),
            (r#"Opening hours and the long way round {contents-label="Opening hours"}"#, None, false),
        ] {
            let a = heading_attrs(text).unwrap_or_else(|| panic!("{text:?} should be accepted"));
            assert_eq!((a.title, a.id, a.contents_end, a.contents_label), ("Opening hours and the long way round", id, end, Some("Opening hours")), "{text:?}");
        }
        let bare = heading_attrs("The ferry ledgers, 1861 to 1862 {contents-label=Ledgers}").unwrap();
        assert_eq!(bare.contents_label, Some("Ledgers"));
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
