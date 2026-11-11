//! A heading and the folder embeds under it, kept as one unit so the build can
//! leave the heading out while every one of its embeds lists nothing.

use std::collections::BTreeSet;

use moss_core::ast::Block;
use regex::Regex;

/// A heading of level 2 or below whose section is folder embeds and deeper
/// sections of the same kind, and nothing else.
#[derive(Debug, Clone)]
pub struct SectionEmission {
    /// The heading's HTML, permalink included.
    pub heading: String,
    /// The heading's anchor id.
    pub id: Option<String>,
    pub parts: Vec<SectionPart>,
}

#[derive(Debug, Clone)]
pub enum SectionPart {
    /// A folder-embed marker, then the listing it expands to.
    Embed(String),
    Section(SectionEmission),
}

impl SectionEmission {
    pub fn to_html(&self) -> String {
        let mut out = self.heading.clone();
        for part in &self.parts {
            match part {
                SectionPart::Embed(html) => out.push_str(html),
                SectionPart::Section(section) => out.push_str(&section.to_html()),
            }
        }
        out
    }

    pub(super) fn for_each_html(&self, f: &mut dyn FnMut(&str)) {
        f(&self.heading);
        for part in &self.parts {
            match part {
                SectionPart::Embed(html) => f(html),
                SectionPart::Section(section) => section.for_each_html(f),
            }
        }
    }

    pub(super) fn map_html(&mut self, f: &dyn Fn(&str) -> String) {
        self.heading = f(&self.heading);
        for part in &mut self.parts {
            match part {
                SectionPart::Embed(html) => *html = f(html),
                SectionPart::Section(section) => section.map_html(f),
            }
        }
    }

    /// Expands the embeds in document order with `expand`, dropping each
    /// section left with nothing, innermost first. True when this section
    /// itself is left with nothing; the ids of the sections dropped under it
    /// go to `dropped`.
    pub(super) fn settle(&mut self, expand: &dyn Fn(&str) -> String, dropped: &mut Vec<String>) -> bool {
        let mut kept = Vec::with_capacity(self.parts.len());
        for mut part in std::mem::take(&mut self.parts) {
            let empty = match &mut part {
                SectionPart::Embed(html) => {
                    *html = expand(html);
                    html.trim().is_empty()
                }
                SectionPart::Section(section) => {
                    let empty = section.settle(expand, dropped);
                    if empty {
                        dropped.extend(section.id.clone());
                    }
                    empty
                }
            };
            if !empty {
                kept.push(part);
            }
        }
        self.parts = kept;
        self.parts.is_empty()
    }
}

/// Where the section opened by the heading at `blocks[start]` ends (exclusive),
/// or `None` when that block does not open a section of this kind.
pub(super) fn section_end(blocks: &[Block], start: usize) -> Option<usize> {
    let Block::Heading { level, .. } = &blocks[start] else { return None };
    if *level < 2 {
        return None;
    }
    let mut at = start + 1;
    let mut parts = 0;
    while let Some(block) = blocks.get(at) {
        match block {
            Block::Heading { level: next, .. } if next <= level => break,
            Block::Heading { .. } => at = section_end(blocks, at)?,
            Block::Other(html) if super::is_folder_embed(html) => at += 1,
            _ => return None,
        }
        parts += 1;
    }
    (parts > 0).then_some(at)
}

/// The section of `blocks[start..end]`, each block rendered by `render`.
pub(super) fn build(blocks: &[Block], start: usize, end: usize, render: &mut dyn FnMut(usize) -> String) -> SectionEmission {
    let id = match &blocks[start] {
        Block::Heading { id, .. } => id.clone(),
        _ => None,
    };
    let mut parts = Vec::new();
    let mut at = start + 1;
    while at < end {
        if matches!(blocks[at], Block::Heading { .. }) {
            let inner_end = section_end(blocks, at).unwrap_or(end);
            parts.push(SectionPart::Section(build(blocks, at, inner_end, render)));
            at = inner_end;
        } else {
            parts.push(SectionPart::Embed(render(at)));
            at += 1;
        }
    }
    SectionEmission { heading: render(start), id, parts }
}

/// Each heading's anchor and its name: headings sharing a name were numbered
/// apart by the parser (`posts`, `posts-1`), in the order the page renders
/// them, which puts hoisted endnotes after the body.
pub(super) fn heading_names(doc: &moss_core::ast::Document) -> Vec<(String, String)> {
    let mut names = Vec::new();
    moss_core::ast::visit_blocks(doc, |block| {
        if let Block::Heading { id: Some(id), numbered_from, .. } = block {
            names.push((id.clone(), numbered_from.clone().unwrap_or_else(|| id.clone())));
        }
        true
    });
    names
}

/// The anchors of headings left out that a kept heading of the same name now
/// answers, as `(kept id, dropped id)`: the n-th kept heading of a name answers
/// the n-th id of that name when its heading was left out, which is the id it
/// would have had if the dropped ones had never been written.
pub(super) fn answered_by_namesakes(names: &[(String, String)], dropped: &[String]) -> Vec<(String, String)> {
    let gone = |id: &str| dropped.iter().any(|d| d == id);
    let dropped_names: BTreeSet<&str> = names.iter().filter(|(id, _)| gone(id)).map(|(_, name)| name.as_str()).collect();
    let mut answered = Vec::new();
    for name in dropped_names {
        let mut ids: Vec<&str> = names.iter().filter(|(_, n)| n == name).map(|(id, _)| id.as_str()).collect();
        ids.sort_by_key(|id| rendered_place(id, name));
        for (n, kept) in ids.iter().filter(|id| !gone(id)).enumerate() {
            if gone(ids[n]) {
                answered.push((kept.to_string(), ids[n].to_string()));
            }
        }
    }
    answered
}

/// Where a heading named `name` renders among its namesakes. The parser gives
/// the n-th of them `name-n` (the first keeps `name`) in rendered order, not
/// source order, so the number is the place.
fn rendered_place(id: &str, name: &str) -> usize {
    id.strip_prefix(name).and_then(|n| n.strip_prefix('-')).and_then(|n| n.parse().ok()).unwrap_or(0)
}

/// `html` with an anchor for `alias` inside the heading whose id is `id`, the
/// way the renderer places a heading's second id.
pub(super) fn add_heading_alias(html: &str, id: &str, alias: &str) -> String {
    let Ok(open) = Regex::new(&format!(r#"<h[1-6] id="{}"[^>]*>"#, regex::escape(id))) else {
        return html.to_string();
    };
    open.replace(html, |c: &regex::Captures| format!(r#"{}<span id="{alias}"></span>"#, &c[0])).into_owned()
}
