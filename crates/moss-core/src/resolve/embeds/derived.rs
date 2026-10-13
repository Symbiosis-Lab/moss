//! Embeds of a file's derived text: `![[report.pdf#text]]`.
//!
//! A plugin can declare that it derives readable markdown from files of some
//! extension, under named fragments. This module is the pure half: it knows
//! only a table of `(extension, fragment)` pairs and a reader that answers
//! what a derivation currently holds. It never names a format and does no I/O.
//!
//! Resolution lowers a claimed embed to a `<!-- moss-derived:path#fragment -->`
//! marker line, and a later pass replaces each marker with the derived
//! markdown or with a placeholder block. Derived text is spliced exactly once
//! and is not scanned again for embeds, so a derived text that happens to
//! contain embed syntax cannot recurse.
//!
//! The `markdown` a deriver returns is text read out of a file someone else
//! wrote, so it carries no raw HTML and none of moss's own authoring syntax:
//! [`escape_derived_markdown`] neutralizes both at the splice, and a tag, a
//! dollar amount or a `[[link]]` in the source file renders as text.

use std::collections::{BTreeSet, HashMap};

use crate::resolve::embed_renderer::{html_escape_attr, MARKER_DERIVED};

#[cfg(test)]
mod tests;

/// What the reader knows about one `(file, fragment)` right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DerivedFragment {
    /// The derivation ran: its markdown, and the `title` fact when it gave one.
    Ready { markdown: String, title: Option<String> },
    /// The derivation has not finished yet.
    Pending,
    /// The deriver declined this file, with a reason fit to show a reader.
    Skipped { message: String },
    /// The deriver could not read the file.
    Failed { message: String },
}

/// The `(extension, fragment)` pairs derivations declare.
#[derive(Debug, Clone, Default)]
pub struct DerivedTable {
    by_ext: HashMap<String, BTreeSet<String>>,
}

impl DerivedTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Claim `#fragment` on files with `extension` (no dot, case-insensitive).
    pub fn declare(&mut self, extension: &str, fragment: &str) {
        self.by_ext
            .entry(extension.trim_start_matches('.').to_ascii_lowercase())
            .or_default()
            .insert(fragment.to_string());
    }

    pub fn claims(&self, extension: &str, fragment: &str) -> bool {
        self.by_ext
            .get(&extension.to_ascii_lowercase())
            .is_some_and(|fragments| fragments.contains(fragment))
    }
}

/// Reads a derivation: `(site-relative path, fragment)` to its current state.
pub type DerivedReader<'a> = dyn Fn(&str, &str) -> DerivedFragment + 'a;

/// The table plus the reader, handed to resolution together.
pub struct Derivations<'a> {
    pub table: &'a DerivedTable,
    pub reader: &'a DerivedReader<'a>,
}

pub fn extension_of(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((_, ext)) => ext.to_ascii_lowercase(),
        None => String::new(),
    }
}

/// The marker a claimed embed lowers to, or `None` when the pair is not claimed.
pub(in crate::resolve) fn claim_marker(
    target_path: &str,
    anchor: Option<&str>,
    derivations: Option<&Derivations<'_>>,
) -> Option<String> {
    let (derivations, fragment) = (derivations?, anchor?);
    derivations
        .table
        .claims(&extension_of(target_path), fragment)
        .then(|| format!("<!-- {MARKER_DERIVED}:{target_path}#{fragment} -->"))
}

/// The marker line's `(path, fragment)`, when `line` is one.
pub(in crate::resolve) fn parse_marker(line: &str) -> Option<(&str, &str)> {
    let body = line
        .trim()
        .strip_prefix("<!-- ")?
        .strip_prefix(MARKER_DERIVED)?
        .strip_prefix(':')?
        .strip_suffix(" -->")?;
    body.rsplit_once('#')
}

/// The placeholder block for a state that has no text to show.
pub fn placeholder_html(state: &DerivedFragment) -> Option<String> {
    match state {
        DerivedFragment::Ready { .. } => None,
        DerivedFragment::Pending => Some(
            "<div class=\"moss-derived moss-derived--pending\" role=\"status\"><span class=\"visually-hidden\">Reading\u{2026}</span></div>"
                .to_string(),
        ),
        DerivedFragment::Skipped { message } => Some(skipped_html(message)),
        DerivedFragment::Failed { .. } => {
            Some(skipped_html("The text of this file couldn't be read."))
        }
    }
}

/// `content` without the lines [`placeholder_html`] wrote. Each is a status note
/// for a reader of the page, not prose, so a description or a plain-text feed
/// of the page leaves it out.
pub fn without_status_blocks(content: &str) -> String {
    content.split_inclusive('\n').filter(|line| !line.trim_start().starts_with(STATUS_BLOCK_OPEN)).collect()
}

/// How every line from [`placeholder_html`] begins.
const STATUS_BLOCK_OPEN: &str = "<div class=\"moss-derived ";

fn skipped_html(message: &str) -> String {
    format!(
        "<div class=\"moss-derived moss-derived--skipped\" role=\"note\">{}</div>",
        html_escape_attr(message)
    )
}

/// `markdown` reduced to plain CommonMark structure, so text read out of
/// someone else's file cannot act as raw HTML or as moss's own authoring
/// syntax.
///
/// Outside code spans and code blocks (which stay literal; the renderer
/// escapes those itself, so escaping here too would show `&amp;`):
/// `<` and `&` become entities (an HTML comment is not code and is escaped
/// like any other raw HTML); `$` (math), `{` (shortcodes and edit markup),
/// `[[` and `[^` (wikilinks, embeds, footnotes), `%%` (comments), a `#`
/// directly followed by a word (tags) and a `^` (block references) become
/// numeric character references, which no later pass reads as syntax; a line
/// starting with `:::` (fenced divs) loses its first colon the same way. A
/// backslash escape already in the text is kept, or turned into the same
/// reference when it names one of these characters. Headings, lists, tables,
/// emphasis and links are untouched.
pub(in crate::resolve) fn escape_derived_markdown(markdown: &str) -> String {
    let regions = crate::inert_regions::InertRegions::scan(markdown);
    let mut out = String::with_capacity(markdown.len());
    let mut cursor = 0;
    for range in regions.ranges() {
        if markdown.get(range.start..).is_some_and(|rest| rest.starts_with("<!--")) {
            continue;
        }
        escape_run(markdown, cursor, range.start, &mut out);
        out.push_str(markdown.get(range.clone()).unwrap_or(""));
        cursor = range.end;
    }
    escape_run(markdown, cursor, markdown.len(), &mut out);
    out
}

/// Escape `markdown[start..end]`, which holds no code.
fn escape_run(markdown: &str, start: usize, end: usize, out: &mut String) {
    let chars: Vec<char> = markdown.get(start..end).unwrap_or("").chars().collect();
    let mut line_blank_so_far = markdown
        .get(..start)
        .unwrap_or("")
        .rsplit('\n')
        .next()
        .is_some_and(|before| before.chars().all(char::is_whitespace));
    let at = |i: usize, pattern: &str| pattern.chars().enumerate().all(|(k, c)| chars.get(i + k) == Some(&c));
    let mut i = 0;
    while let Some(&ch) = chars.get(i) {
        // A backslash before ASCII punctuation is the deriver's own escape.
        // One for a character this function would neutralise is rewritten as
        // that character's reference, so it can never pair up with an escape
        // added here; any other is left alone.
        let escaped = if ch == '\\' { chars.get(i + 1).copied().filter(char::is_ascii_punctuation) } else { None };
        let target = escaped.unwrap_or(ch);
        let reference = match target {
            '<' => Some("&lt;"),
            '&' => Some("&amp;"),
            '$' => Some("&#36;"),
            '{' => Some("&#123;"),
            '^' => Some("&#94;"),
            '[' if escaped.is_some() || at(i, "[[") || at(i, "[^") => Some("&#91;"),
            '%' if escaped.is_some() || at(i, "%%") || (i > 0 && chars.get(i - 1) == Some(&'%')) => Some("&#37;"),
            '#' if escaped.is_some()
                || (chars.get(i + 1).is_some_and(|c| !c.is_whitespace() && *c != '#')
                    && !(i > 0 && chars.get(i - 1) == Some(&'#'))) =>
            {
                Some("&#35;")
            }
            ':' if escaped.is_none() && line_blank_so_far && at(i, ":::") => Some("&#58;"),
            _ => None,
        };
        match (reference, escaped) {
            (Some(reference), _) => out.push_str(reference),
            (None, Some(_)) => {
                out.push(ch);
                out.push(target);
            }
            (None, None) => out.push(ch),
        }
        let width = if escaped.is_some() { 2 } else { 1 };
        for &c in chars.get(i..i + width).unwrap_or(&[]) {
            if c == '\n' {
                line_blank_so_far = true;
            } else if !c.is_whitespace() {
                line_blank_so_far = false;
            }
        }
        i += width;
    }
}

pub(in crate::resolve) struct Spliced {
    pub content: String,
    /// `(derived source file, page)` pairs, like transclusion dependencies.
    pub deps: Vec<(String, String)>,
    /// The first ready derivation's `title` fact.
    pub title: Option<String>,
}

/// Replace every marker line with its derivation's text or placeholder.
pub(in crate::resolve) fn splice_markers(
    content: &str,
    source_path: &str,
    derivations: &Derivations<'_>,
) -> Spliced {
    let mut output = String::with_capacity(content.len());
    let mut deps = Vec::new();
    let mut title = None;
    let regions = crate::inert_regions::InertRegions::scan(content);
    let mut line_start = 0;
    for chunk in content.split_inclusive('\n') {
        let line = chunk.trim_end_matches(['\n', '\r']);
        let at = line_start;
        line_start += chunk.len();
        // A marker the resolver wrote starts its own comment; one inside a
        // fenced code block belongs to a region that began earlier.
        let inside_earlier_region = regions.ranges().iter().any(|r| r.contains(&at) && r.start < at);
        let Some((path, fragment)) = parse_marker(line).filter(|_| !inside_earlier_region) else {
            output.push_str(line);
            output.push('\n');
            continue;
        };
        deps.push((path.to_string(), source_path.to_string()));
        let state = (derivations.reader)(path, fragment);
        // Blank lines on both sides keep the block from fusing with a
        // neighbouring paragraph; the HTML block itself has none inside.
        output.push('\n');
        match &state {
            DerivedFragment::Ready { markdown, title: fact } => {
                if title.is_none() {
                    title = fact.clone().filter(|t| !t.trim().is_empty());
                }
                output.push_str(&escape_derived_markdown(markdown.trim_end()));
            }
            other => output.push_str(&placeholder_html(other).unwrap_or_default()),
        }
        output.push_str("\n\n");
    }
    if !content.ends_with('\n') && output.ends_with('\n') {
        output.pop();
    }
    Spliced { content: output, deps, title }
}
