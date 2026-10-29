//! Writing embeds out as text.
//!
//! [`expand_page`] replaces each line that holds only a ready derived embed,
//! or only an ordinary note transclusion, with the text it stands for, so a
//! page can be committed without the embed. Anything that cannot be expanded
//! yet (pending, skipped, failed, unresolved) is left as written.
//!
//! Derived text is inserted with the escaping and the blank lines the build
//! applies when it splices it, and a page that took its title from the file
//! (`inferred: [title, description, date]`) is given them, so the expanded page
//! renders as the embed did.

use crate::content_graph::ContentGraph;

use super::derived::{escape_derived_markdown, parse_marker, DerivedFacts, DerivedFragment, Derivations};
use super::{parse_embed_marker, select_section, split_target, strip_frontmatter};

/// One line that was replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpandedLine {
    /// 1-based line number in the original page.
    pub line: usize,
    /// The embed as written, e.g. `![[report.pdf#text]]`.
    pub embed: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expansion {
    pub content: String,
    pub expanded: Vec<ExpandedLine>,
}

/// Expand `raw` (a whole page, frontmatter included; kept byte-for-byte).
///
/// `file_reader` reads a site-relative file for note transclusion. The text
/// of a note is inserted as it stands: embeds inside it are not expanded.
pub fn expand_page(
    source_path: &str,
    raw: &str,
    graph: &ContentGraph,
    file_reader: &dyn Fn(&str) -> Option<String>,
    derivations: &Derivations<'_>,
) -> Expansion {
    let body_start = crate::frontmatter::frontmatter_span(raw).map_or(0, |span| span.body);
    let (head, body) = raw.split_at_checked(body_start).unwrap_or(("", raw));
    let masked = crate::inert_regions::mask_inert(body);
    let first_line = head.lines().count();

    let mut content = String::from(head);
    let mut expanded = Vec::new();
    let mut facts = DerivedFacts::default();
    for (index, (line, masked_line)) in body.split_inclusive('\n').zip(masked.split_inclusive('\n')).enumerate() {
        let text = line.trim_end_matches(['\n', '\r']);
        let ending = line.get(text.len()..).unwrap_or("");
        let replacement = (masked_line.trim() == text.trim())
            .then(|| sole_embed(text.trim()))
            .flatten()
            .and_then(|inner| expansion_for(inner, source_path, graph, file_reader, derivations));
        match replacement {
            Some(r) => {
                if r.derived {
                    facts.fill_from(&r.facts);
                    // Blank lines around derived text, as in the build, keep
                    // it from fusing with a neighbour or turning the line
                    // before it into a setext heading.
                    if content.len() > head.len() && !content.ends_with("\n\n") {
                        content.push('\n');
                    }
                    content.push_str(r.text.trim_end_matches('\n'));
                    content.push('\n');
                } else {
                    content.push_str(r.text.trim_end_matches('\n'));
                }
                content.push_str(ending);
                expanded.push(ExpandedLine { line: first_line + index + 1, embed: text.trim().to_string() });
            }
            None => content.push_str(line),
        }
    }
    let mut new_head = head.to_string();
    for key in ["title", "description", "date"] {
        if let Some(written) = with_inferred_field(&new_head, key, &facts) {
            new_head = written;
        }
    }
    content.replace_range(..head.len(), &new_head);
    Expansion { content, expanded }
}

/// The inside of `![[…]]` when it is the whole line.
fn sole_embed(trimmed: &str) -> Option<&str> {
    let inner = trimmed.strip_prefix("![[")?.strip_suffix("]]")?;
    (!inner.contains("]]") && !inner.contains("![[") && !inner.is_empty()).then_some(inner)
}

/// What one embed line is replaced with.
struct Replacement {
    text: String,
    /// Derived text rather than a note's: it is padded and may carry facts.
    derived: bool,
    facts: DerivedFacts,
}

fn expansion_for(
    inner: &str,
    source_path: &str,
    graph: &ContentGraph,
    file_reader: &dyn Fn(&str) -> Option<String>,
    derivations: &Derivations<'_>,
) -> Option<Replacement> {
    // The same lowering the build applies decides what the embed stands for.
    let marker = crate::resolve::lower_embed(inner, graph, source_path, Some(derivations), &mut Vec::new())?;
    if let Some((path, fragment)) = parse_marker(&marker) {
        return match (derivations.reader)(path, fragment) {
            DerivedFragment::Ready { markdown, facts } => {
                Some(Replacement { text: escape_derived_markdown(markdown.trim_end()), derived: true, facts })
            }
            _ => None,
        };
    }
    let (path, anchor) = split_target(parse_embed_marker(&marker)?);
    let text = file_reader(path)?;
    let text = select_section(strip_frontmatter(&text), anchor)?;
    Some(Replacement { text, derived: false, facts: DerivedFacts::default() })
}

/// `head` (the frontmatter block, kept byte-for-byte) with `key` (`title`,
/// `description` or `date`) written in from `facts` and `key` taken out of `inferred`,
/// when [`DerivedFacts::fact_for`] says the page takes it and `inferred` is a
/// flow (`[title]`) or block list. A blank `key:` line is filled where it
/// stands; otherwise the line is added. `None` leaves the head alone.
fn with_inferred_field(head: &str, key: &str, facts: &DerivedFacts) -> Option<String> {
    let span = crate::frontmatter::frontmatter_span(head)?;
    let map = crate::frontmatter::frontmatter_map(head);
    let own = map.get(key).and_then(crate::frontmatter::value_as_string);
    let listed: Vec<String> = map
        .get("inferred")
        .and_then(|v| v.as_sequence())
        .map(|items| items.iter().filter_map(|i| i.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let value = facts.fact_for(key, own.as_deref(), &listed)?;
    let value_line = {
        let one_line = value.split_whitespace().collect::<Vec<_>>().join(" ");
        format!("{key}: {}\n", serde_yaml::to_string(&one_line).ok()?.trim_end())
    };
    let fields = head.get(span.fields.clone())?;
    let lines: Vec<&str> = fields.split_inclusive('\n').collect();
    let at = lines.iter().position(|l| l.starts_with("inferred:"))?;
    let inferred_line = *lines.get(at)?;
    let rest = inferred_line.trim_start_matches("inferred:").trim();
    let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    let newline = if inferred_line.ends_with("\r\n") { "\r\n" } else { "\n" };
    if let Some(list) = rest.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        let kept: Vec<&str> = list
            .split(',')
            .map(str::trim)
            .filter(|i| !i.is_empty() && i.trim_matches(['"', '\'']) != key)
            .collect();
        out.splice(at..=at, if kept.is_empty() { vec![] } else { vec![format!("inferred: [{}]{newline}", kept.join(", "))] });
    } else if rest.is_empty() {
        let item = (at + 1..lines.len())
            .take_while(|&i| lines.get(i).is_some_and(|l| l.trim_start().starts_with('-')))
            .find(|&i| lines.get(i).is_some_and(|l| l.trim_start().trim_start_matches('-').trim().trim_matches(['"', '\'']) == key))?;
        let items = (at + 1..lines.len()).take_while(|&i| lines.get(i).is_some_and(|l| l.trim_start().starts_with('-'))).count();
        if items == 1 {
            out.splice(at..=item, std::iter::empty());
        } else {
            out.remove(item);
        }
    } else {
        return None;
    }
    let own_line = out.iter().position(|l| l.starts_with(&format!("{key}:")));
    let mut value_line = value_line.replace('\n', newline);
    if let Some(at) = own_line {
        out[at] = std::mem::take(&mut value_line);
    }
    let mut fields_out: String = out.concat();
    if !fields_out.is_empty() && !fields_out.ends_with('\n') {
        fields_out.push_str(newline);
    }
    fields_out.push_str(&value_line);
    let mut result = String::from(head);
    result.replace_range(span.fields, &fields_out);
    Some(result)
}
