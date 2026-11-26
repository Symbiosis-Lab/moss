//! The pages joined by `in_reply_to`, resolved once per build and kept both
//! ways: only the later page names the earlier one, so no outgoing link of the
//! earlier page records its continuations.

use std::collections::{HashMap, HashSet};

use moss_core::content_graph::ContentGraph;
use moss_core::fields::{TypedValue, IN_REPLY_TO};

use crate::build::types::ParsedDocument;

/// Indices are into the documents the index was built from.
#[derive(Debug, Default)]
pub struct Replies {
    parent: HashMap<usize, usize>,
    children: HashMap<usize, Vec<usize>>,
}

/// A page's thread: every page joined to it by `in_reply_to`, from the root
/// down, each continuation after the page it continues, oldest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    pub members: Vec<Member>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Member {
    pub index: usize,
    /// Steps from the root. A member deeper than the one before it continues
    /// that one; a branch shows as a member no deeper than an earlier one.
    pub depth: usize,
}

impl Replies {
    /// Resolves each page's `in_reply_to`. An outside address and a name that
    /// matches no page of the site are left out.
    pub fn build(documents: &[ParsedDocument], graph: &ContentGraph) -> Self {
        let mut replies = Self::default();
        let by_source: HashMap<&str, usize> =
            documents.iter().enumerate().filter_map(|(i, d)| Some((d.source_path.as_deref()?, i))).collect();
        for (reader, doc) in documents.iter().enumerate() {
            let Some(from) = doc.source_path.as_deref() else { continue };
            let Some(TypedValue::Page(link)) = doc.declared.get(IN_REPLY_TO) else { continue };
            let Some(name) = link.target.page() else { continue };
            let Some(target) = graph.resolve_path(name, from).and_then(|p| by_source.get(p.as_str()).copied()) else {
                continue;
            };
            if target != reader {
                replies.parent.insert(reader, target);
                replies.children.entry(target).or_default().push(reader);
            }
        }
        replies
    }

    /// The page `index` continues, if it continues one of this site.
    fn parent(&self, index: usize) -> Option<usize> {
        self.parent.get(&index).copied()
    }

    /// The first page of `index`'s thread. In a loop of replies, its first
    /// document, so every page of the loop reads the same thread.
    fn root(&self, index: usize) -> usize {
        let mut path = vec![index];
        while let Some(parent) = self.parent(path[path.len() - 1]) {
            if let Some(at) = path.iter().position(|p| *p == parent) {
                return path[at..].iter().copied().min().unwrap_or(parent);
            }
            path.push(parent);
        }
        path[path.len() - 1]
    }

    fn thread_from(&self, root: usize, documents: &[ParsedDocument]) -> Option<Thread> {
        let mut members = Vec::new();
        let mut visited = HashSet::new();
        let mut stack = vec![Member { index: root, depth: 0 }];
        while let Some(member) = stack.pop() {
            if !visited.insert(member.index) {
                continue;
            }
            members.push(member);
            let mut children: Vec<usize> =
                self.children.get(&member.index).into_iter().flatten().copied().filter(|c| !visited.contains(c)).collect();
            children.sort_by(|a, b| oldest_first(&documents[*a], &documents[*b]));
            stack.extend(children.into_iter().rev().map(|index| Member { index, depth: member.depth + 1 }));
        }
        (members.len() > 1).then_some(Thread { members })
    }
}

/// One page of a thread as a page stores it: by source path, so it outlives
/// any reordering of the build's documents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadMember {
    pub source_path: String,
    pub depth: usize,
}

/// Gives every page in a thread its thread, and the features of the pages it
/// draws whole beside its own. Each thread is walked once, from its root.
pub fn mark_threads(documents: &mut [ParsedDocument], graph: &ContentGraph) {
    let replies = Replies::build(documents, graph);
    let mut by_root: HashMap<usize, Option<Thread>> = HashMap::new();
    let mut threads = Vec::new();
    for at in replies.parent.keys().chain(replies.children.keys()).copied().collect::<std::collections::BTreeSet<_>>() {
        let root = replies.root(at);
        if let Some(thread) = by_root.entry(root).or_insert_with(|| replies.thread_from(root, documents)) {
            threads.push((at, thread.clone()));
        }
    }
    for (at, thread) in threads {
        let drawn: Vec<_> = thread.members.iter().filter(|m| m.index != at).map(|m| documents[m.index].features.clone()).collect();
        let stored = thread
            .members
            .iter()
            .filter_map(|m| Some(ThreadMember { source_path: documents[m.index].source_path.clone()?, depth: m.depth }))
            .collect();
        let doc = &mut documents[at];
        doc.thread = stored;
        for features in &drawn {
            doc.features.absorb(features);
        }
    }
}

fn oldest_first(a: &ParsedDocument, b: &ParsedDocument) -> std::cmp::Ordering {
    a.date.as_deref().unwrap_or("").cmp(b.date.as_deref().unwrap_or("")).then_with(|| a.source_path.cmp(&b.source_path))
}

#[cfg(test)]
#[path = "replies_tests.rs"]
mod tests;
