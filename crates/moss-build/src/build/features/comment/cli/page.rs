//! The comments of one page, as the editor lists them under it.

use std::path::Path;

use serde::Serialize;

use super::collect;

/// One comment under its page, for its owner to hide, show or mark as spam.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PageComment {
    pub source: String,
    pub id: String,
    pub author: String,
    /// The host of the writer's website field.
    pub website: Option<String>,
    pub created_at: String,
    /// Plain text, markup stripped, cut at a few hundred characters.
    pub text: String,
    /// Not shown on the site, by its own hide, a hidden parent or a blocked host.
    pub hidden: bool,
    /// Hidden by its own hide or a blocked host, not only a hidden parent, so Show brings it back.
    pub directly_hidden: bool,
}

#[derive(Debug, Default, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PageComments {
    /// Newest first.
    pub comments: Vec<PageComment>,
}

/// The comments of the page built from the source at `rel_path`. A source no
/// build has reached yet has none.
pub fn page_comments(project_root: &Path, rel_path: &str) -> PageComments {
    let Some(uid) = crate::build::emit::inventory::page_uid_for_source(project_root, rel_path) else {
        return PageComments::default();
    };
    let comments = collect(&project_root.to_string_lossy(), Some(&uid))
        .0
        .into_iter()
        .map(|r| PageComment {
            source: r.source,
            id: r.id,
            author: r.author,
            website: r.website,
            created_at: r.created_at,
            text: r.body,
            hidden: r.hidden,
            directly_hidden: r.directly_hidden,
        })
        .collect();
    PageComments { comments }
}

#[cfg(test)]
#[path = "page_tests.rs"]
mod tests;
