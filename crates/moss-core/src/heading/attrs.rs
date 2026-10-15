//! A heading written with an attribute block (`## Title {.class}`, `{#id}`)
//! is not given a class or an id: moss prints the braces as part of the
//! heading's text and of its anchor id. This finds those headings so the
//! build can say so.
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
            let block = attribute_tail(heading_text(line)?)?;
            Some(format!(
                "line {}: the heading's `{block}` is printed as text, in the heading and in its anchor id; moss has no heading attributes. To give the section a class or an id, wrap it in a fenced div (`:::{block}` … `:::`) and take the braces off the heading",
                line_offset + i + 1
            ))
        })
        .collect()
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
    let block = &text[open..];
    let inner = block[1..block.len() - 1].trim_start();
    let key = inner.split(['=', ' ']).next().unwrap_or("");
    let attribute = inner.starts_with(['.', '#'])
        || (!key.is_empty() && inner[key.len()..].starts_with('=') && key.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_'));
    attribute.then_some(block)
}
