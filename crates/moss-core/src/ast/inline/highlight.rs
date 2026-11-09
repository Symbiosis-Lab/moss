//! Obsidian-style `==highlight==` as [`Inline::Mark`].
//!
//! pulldown-cmark has no highlight option, so this is a pass over the finished
//! tree. When pulldown-cmark ships one, delete this module and turn that
//! option on in `parser_options`.
//!
//! A delimiter is a run of exactly two `=` inside one text node, so `a === b`
//! stays text. The opener needs a non-space after it and the closer one before
//! it; a non-text neighbour such as `**bold**` counts as non-space. A pair
//! lives among the siblings of one inline container and never spans a link's
//! text boundary. A backslash does not escape `==`: the parser merges an
//! escaped character into its neighbouring text before this pass runs.

use super::super::document::Document;
use super::super::node::Inline;

/// Wrap every `==…==` pair in `doc` in [`Inline::Mark`].
pub fn apply(doc: &mut Document) {
    super::for_each_inline_run_mut(doc, &mut in_inlines);
}

fn in_inlines(inlines: &mut Vec<Inline>) {
    for inline in inlines.iter_mut() {
        match inline {
            Inline::Emphasis(c)
            | Inline::Strong(c)
            | Inline::Strikethrough(c)
            | Inline::Mark(c) => in_inlines(c),
            Inline::Link { children, .. } => in_inlines(children),
            _ => {}
        }
    }
    pair(inlines);
}

/// One piece of a sibling list once text nodes are split at their `==` runs.
enum Piece {
    Node(Inline),
    Delim { can_open: bool, can_close: bool },
}

/// What a delimiter run became once pairs are known.
enum Role {
    Open,
    Close,
    Literal,
}

fn is_delim_text(inline: &Inline) -> bool {
    matches!(inline, Inline::Text(t) if t.contains("=="))
}

fn pair(inlines: &mut Vec<Inline>) {
    if !inlines.iter().any(is_delim_text) {
        return;
    }
    let mut pieces: Vec<Piece> = Vec::new();
    for (idx, inline) in inlines.iter().enumerate() {
        match inline {
            Inline::Text(t) if t.contains("==") => {
                let before_node = idx.checked_sub(1).map(|i| &inlines[i]);
                split_text(t, before_node, inlines.get(idx + 1), &mut pieces);
            }
            other => pieces.push(Piece::Node(other.clone())),
        }
    }

    // Pairs never nest: an opener is only ever followed by its own closer.
    let mut roles: Vec<Role> = pieces.iter().map(|_| Role::Literal).collect();
    let mut open: Option<usize> = None;
    let mut paired = false;
    for (i, piece) in pieces.iter().enumerate() {
        let Piece::Delim { can_open, can_close } = piece else { continue };
        match open {
            Some(o) if *can_close && i > o + 1 => {
                roles[o] = Role::Open;
                roles[i] = Role::Close;
                open = None;
                paired = true;
            }
            _ if *can_open => open = Some(i),
            _ => {}
        }
    }
    if !paired {
        return;
    }

    let mut out: Vec<Inline> = Vec::new();
    let mut current: Option<Vec<Inline>> = None;
    for (piece, role) in pieces.into_iter().zip(roles) {
        let leaf = match (piece, role) {
            (Piece::Node(n), _) => n,
            (Piece::Delim { .. }, Role::Open) => {
                current = Some(Vec::new());
                continue;
            }
            (Piece::Delim { .. }, Role::Close) => Inline::Mark(current.take().unwrap_or_default()),
            (Piece::Delim { .. }, Role::Literal) => Inline::Text("==".into()),
        };
        current.as_mut().unwrap_or(&mut out).push(leaf);
    }
    *inlines = out;
}

fn split_text(text: &str, prev: Option<&Inline>, next: Option<&Inline>, pieces: &mut Vec<Piece>) {
    let bytes = text.as_bytes();
    let mut seg_start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'=' {
            i += 1;
            continue;
        }
        let run_end = bytes[i..].iter().position(|&b| b != b'=').map_or(bytes.len(), |n| i + n);
        if run_end - i == 2 {
            let before = text.get(..i).and_then(|s| s.chars().next_back());
            let after = text.get(run_end..).and_then(|s| s.chars().next());
            let before_ok = match before {
                Some(c) => !c.is_whitespace(),
                None => prev.is_some_and(node_ends_non_space),
            };
            let after_ok = match after {
                Some(c) => !c.is_whitespace(),
                None => next.is_some_and(node_starts_non_space),
            };
            if i > seg_start {
                pieces.push(Piece::Node(Inline::Text(text.get(seg_start..i).unwrap_or_default().to_string())));
            }
            pieces.push(Piece::Delim { can_open: after_ok, can_close: before_ok });
            seg_start = run_end;
        }
        i = run_end;
    }
    if seg_start < text.len() {
        pieces.push(Piece::Node(Inline::Text(text.get(seg_start..).unwrap_or_default().to_string())));
    }
}

fn node_ends_non_space(inline: &Inline) -> bool {
    match inline {
        Inline::Text(t) => t.chars().next_back().is_some_and(|c| !c.is_whitespace()),
        Inline::LineBreak => false,
        _ => true,
    }
}

fn node_starts_non_space(inline: &Inline) -> bool {
    match inline {
        Inline::Text(t) => t.chars().next().is_some_and(|c| !c.is_whitespace()),
        Inline::LineBreak => false,
        _ => true,
    }
}

#[cfg(test)]
#[path = "highlight_tests.rs"]
mod tests;
