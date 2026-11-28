//! Bare `:::` lines that close nothing.
//!
//! Before moss 0.15.30 such a line right under a paragraph turned the
//! paragraph into a definition list holding `::`; now it shows as written.
//! Either way it is a stray mark, so each one the build warns about goes.

use super::FileChange;

/// Each page with its stray closers deleted. `pages` holds site-relative
/// paths and file texts.
pub fn plan(pages: &[(String, String)]) -> Vec<FileChange> {
    pages
        .iter()
        .filter_map(|(rel, text)| {
            let body = moss_core::frontmatter::body(text);
            let lines = moss_core::ast::shortcode_extract::stray_closer_lines(body);
            if lines.is_empty() {
                return None;
            }
            let above = text.matches('\n').count() - body.matches('\n').count();
            let after: String = text
                .split_inclusive('\n')
                .enumerate()
                .filter(|(i, _)| !(*i >= above && lines.contains(&(i - above))))
                .map(|(_, line)| line)
                .collect();
            Some(FileChange { path: rel.clone(), before: text.clone(), after })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stray_closer_under_a_paragraph_is_deleted_and_matched_ones_stay() {
        let text = "---\ntitle: t\n---\n\nPoster line.\n:::\n\n:::grid 2\na\n+++\nb\n:::\n";
        let changes = plan(&[("index.md".to_string(), text.to_string())]);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].after, "---\ntitle: t\n---\n\nPoster line.\n\n:::grid 2\na\n+++\nb\n:::\n");
    }

    #[test]
    fn a_page_with_balanced_fences_or_a_closer_in_code_is_left() {
        let pages = [
            ("a.md".to_string(), "::: {.note}\nx\n:::\n".to_string()),
            ("b.md".to_string(), "```\n:::\n```\n".to_string()),
        ];
        assert!(plan(&pages).is_empty());
    }
}
