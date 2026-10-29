//! Keeping a number range in one piece when a heading wraps.
//!
//! A line may break after a hyphen or a dash, so a heading like "Maren Oduya,
//! 1626–1705" can end its line on "1626–" on a phone. Wrapping the run in a
//! `nowrap` span stops that without touching the heading's text, its id or its
//! contents-list label, which are all built from the source before this runs.

use crate::inert_regions::tag_end;

use std::borrow::Cow;

/// The byte ranges of digit–dash–digit runs in `text`: digits joined by one or
/// more hyphens, en dashes or em dashes.
fn ranges(text: &str) -> Vec<(usize, usize)> {
    let is_dash = |c: char| matches!(c, '-' | '\u{2013}' | '\u{2014}');
    let mut found = Vec::new();
    let mut start: Option<usize> = None;
    let mut joined = false;
    let mut prev_digit_end = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c.is_ascii_digit() {
            let run_start = start.unwrap_or(i);
            start = Some(run_start);
            prev_digit_end = i + 1;
            if chars.peek().is_none_or(|&(_, n)| !n.is_ascii_digit() && !is_dash(n)) {
                if joined {
                    found.push((run_start, prev_digit_end));
                }
                start = None;
                joined = false;
            }
        } else if is_dash(c) && start.is_some() && chars.peek().is_some_and(|&(_, n)| n.is_ascii_digit()) {
            joined = true;
        } else {
            if joined {
                found.push((start.unwrap_or(0), prev_digit_end));
            }
            start = None;
            joined = false;
        }
    }
    found
}

/// Wraps each digit–dash–digit run in a heading's inline HTML in
/// `<span class="moss-nobreak">`. Markup is left alone: only text between tags
/// is read.
pub fn keep_ranges_together(inline_html: &str) -> Cow<'_, str> {
    let mut out = String::new();
    let mut rest = inline_html;
    while !rest.is_empty() {
        let split = rest.find('<').and_then(|open| {
            let (text, from_tag) = rest.split_at_checked(open)?;
            let (tag, after) = from_tag.split_at_checked(tag_end(from_tag)?)?;
            Some((text, tag, after))
        });
        let (text, tag, after) = split.unwrap_or((rest, "", ""));
        let mut last = 0;
        for (from, to) in ranges(text) {
            out.push_str(text.get(last..from).unwrap_or_default());
            out.push_str(r#"<span class="moss-nobreak">"#);
            out.push_str(text.get(from..to).unwrap_or_default());
            out.push_str("</span>");
            last = to;
        }
        out.push_str(text.get(last..).unwrap_or_default());
        out.push_str(tag);
        rest = after;
    }
    if out.len() == inline_html.len() {
        Cow::Borrowed(inline_html)
    } else {
        Cow::Owned(out)
    }
}

#[cfg(test)]
mod tests {
    use super::keep_ranges_together as keep;

    #[test]
    fn wraps_each_kind_of_dash_between_digits() {
        for dash in ["-", "\u{2013}", "\u{2014}"] {
            assert_eq!(
                keep(&format!("Maren Oduya, 1626{dash}1705")),
                format!(r#"Maren Oduya, <span class="moss-nobreak">1626{dash}1705</span>"#)
            );
        }
    }

    #[test]
    fn a_chain_of_numbers_is_one_run() {
        assert_eq!(keep("2026-10-23"), r#"<span class="moss-nobreak">2026-10-23</span>"#);
    }

    #[test]
    fn a_dash_not_followed_by_a_digit_ends_the_run() {
        assert_eq!(keep("1626\u{2013} and 1-2-x"), "1626\u{2013} and <span class=\"moss-nobreak\">1-2</span>-x");
    }

    #[test]
    fn other_text_and_markup_are_untouched() {
        for s in ["Plain heading", "Well-known 1626", "1626 – 1705", "A-1", r#"<a href="/2020-2021/">Two</a>"#] {
            assert_eq!(keep(s), s);
        }
    }

    #[test]
    fn a_greater_than_in_a_quoted_attribute_does_not_end_the_tag() {
        let s = r#"<abbr title="a > 1-2">Gap</abbr> 3-4"#;
        assert_eq!(keep(s), r#"<abbr title="a > 1-2">Gap</abbr> <span class="moss-nobreak">3-4</span>"#);
    }

    #[test]
    fn only_text_between_tags_is_read() {
        assert_eq!(
            keep(r#"<em>Reign</em> 1558-1603 <a href="/a-1-2/">c. 1600-1610</a>"#),
            r#"<em>Reign</em> <span class="moss-nobreak">1558-1603</span> <a href="/a-1-2/">c. <span class="moss-nobreak">1600-1610</span></a>"#
        );
    }
}
