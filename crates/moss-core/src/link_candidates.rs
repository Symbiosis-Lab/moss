//! Link resolution candidate generation.
//!
//! Given a markdown link target (e.g., "文字.md", "about", "docs/intro.md"),
//! generates a list of candidate file paths to try when resolving the link.
//! Pure function, no I/O — consumers check candidates against their own data source.

/// Generate candidate paths for resolving a markdown link target.
/// Returns paths in priority order. Consumers check each against filesystem or PageMap.
pub fn link_candidates(target: &str) -> Vec<String> {
    // Strip anchor fragment for file resolution
    let clean = target.split('#').next().unwrap_or(target);
    if clean.is_empty() {
        return vec![];
    }

    let mut candidates = Vec::new();

    // 1. Direct path as-is
    candidates.push(clean.to_string());

    // 2. With .md extension appended (if not already .md)
    if !clean.ends_with(".md") {
        candidates.push(format!("{}.md", clean));
    }

    // Compute stem (without .md) and filename (with .md) for directory-based candidates
    let (stem, filename) = if clean.ends_with(".md") {
        (clean.trim_end_matches(".md"), clean.to_string())
    } else {
        (clean, format!("{}.md", clean))
    };

    // 3. As directory with index.md (use stem, not full path with .md)
    let trimmed_stem = stem.trim_end_matches('/');
    let index_candidate = format!("{}/index.md", trimmed_stem);
    if !candidates.contains(&index_candidate) {
        candidates.push(index_candidate);
    }
    let folder_note = format!("{}/{}", stem, filename);
    if !candidates.contains(&folder_note) {
        candidates.push(folder_note);
    }

    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_md_extension() {
        let result = link_candidates("文字.md");
        assert_eq!(result, vec![
            "文字.md",
            "文字/index.md",
            "文字/文字.md",
        ]);
    }

    #[test]
    fn without_extension() {
        let result = link_candidates("文字");
        assert_eq!(result, vec![
            "文字",
            "文字.md",
            "文字/index.md",
            "文字/文字.md",
        ]);
    }

    #[test]
    fn with_path() {
        let result = link_candidates("docs/intro.md");
        assert_eq!(result, vec![
            "docs/intro.md",
            "docs/intro/index.md",
            "docs/intro/docs/intro.md",
        ]);
    }

    #[test]
    fn bare_directory_name() {
        let result = link_candidates("about");
        assert_eq!(result, vec![
            "about",
            "about.md",
            "about/index.md",
            "about/about.md",
        ]);
    }

    #[test]
    fn with_anchor_fragment() {
        let result = link_candidates("page.md#section");
        assert_eq!(result, vec![
            "page.md",
            "page/index.md",
            "page/page.md",
        ]);
    }

    #[test]
    fn empty_target() {
        let result = link_candidates("");
        assert!(result.is_empty());
    }

    #[test]
    fn anchor_only() {
        let result = link_candidates("#section");
        assert!(result.is_empty());
    }

    #[test]
    fn trailing_slash() {
        let result = link_candidates("docs/");
        assert!(result.contains(&"docs/index.md".to_string()));
    }
}
