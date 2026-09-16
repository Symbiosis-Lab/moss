//! Embed (transclusion) resolution.
//!
//! Resolves `<!-- moss-embed:TARGET -->` placeholders produced by the wikilink
//! resolver into inlined file content.  Supports full-file and heading-scoped
//! embeds, recursive embedding with cycle detection, and frontmatter stripping.
//!
//! This module is pure Rust with zero I/O — the caller supplies a `file_reader`
//! closure that maps a relative path to file content.

use std::collections::HashSet;

use crate::heading::anchor::obsidian_heading_anchor;

use super::{Diagnostic, DiagnosticKind};

/// Maximum recursion depth for nested embeds.
///
/// Public because every consumer that walks the embed relation has to stop at
/// the same depth the resolver itself stopped at — notably
/// [`crate::dep_graph::DepGraph::embed_closure`], which bounds its traversal by
/// this constant rather than inventing a second limit that could disagree
/// (moss#922 Stage 7).
pub const MAX_EMBED_DEPTH: usize = 10;

/// Prefix for resolved embed markers.
const EMBED_PREFIX: &str = "<!-- moss-embed:";
/// Prefix for unresolved embed markers (left as-is).
const EMBED_UNRESOLVED_PREFIX: &str = "<!-- moss-embed-unresolved:";
/// Suffix for all embed markers.
const EMBED_SUFFIX: &str = " -->";

/// The result of resolving embed placeholders in a document.
#[derive(Debug)]
pub struct EmbedResult {
    /// The content with embed placeholders replaced by inlined file content.
    pub content: String,
    /// Diagnostics produced during resolution (missing files, cycles, etc.).
    pub diagnostics: Vec<Diagnostic>,
    /// `(target_path, source_path)` pairs for watch-mode invalidation.
    pub embed_deps: Vec<(String, String)>,
}

/// Resolve embed placeholders by inlining target file content.
///
/// Convenience wrapper that creates the visited set internally.
/// Use [`resolve_embeds_with_visited`] if you need to supply your own set
/// (e.g. to pre-seed cycle detection from an outer context).
pub fn resolve_embeds(
    content: &str,
    from_path: &str,
    file_reader: &dyn Fn(&str) -> Option<String>,
) -> EmbedResult {
    let mut visited = HashSet::new();
    resolve_embeds_inner(content, from_path, file_reader, &mut visited, 0)
}

/// Resolve embed placeholders with an externally-managed visited set.
///
/// `file_reader` is a closure that reads a file by its relative path.
/// This keeps moss-core I/O-free — the caller provides the reader.
///
/// `visited` tracks paths in the current embed chain for cycle detection.
pub fn resolve_embeds_with_visited(
    content: &str,
    from_path: &str,
    file_reader: &dyn Fn(&str) -> Option<String>,
    visited: &mut HashSet<String>,
) -> EmbedResult {
    resolve_embeds_inner(content, from_path, file_reader, visited, 0)
}

/// Inner recursive implementation with depth tracking.
fn resolve_embeds_inner(
    content: &str,
    from_path: &str,
    file_reader: &dyn Fn(&str) -> Option<String>,
    visited: &mut HashSet<String>,
    depth: usize,
) -> EmbedResult {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut embed_deps: Vec<(String, String)> = Vec::new();
    let mut output = String::with_capacity(content.len());

    for line in content.lines() {
        let trimmed = line.trim();

        // Leave unresolved markers as-is.
        if trimmed.starts_with(EMBED_UNRESOLVED_PREFIX) {
            output.push_str(line);
            output.push('\n');
            continue;
        }

        // Check for a resolved embed marker.
        if let Some(target) = parse_embed_marker(trimmed) {
            let (file_path, heading_anchor) = split_target(target);

            // Record the dependency regardless of whether we can resolve it.
            embed_deps.push((file_path.to_string(), from_path.to_string()));

            // Check depth limit.
            if depth >= MAX_EMBED_DEPTH {
                diagnostics.push(Diagnostic {
                    message: format!(
                        "Embed depth limit ({MAX_EMBED_DEPTH}) exceeded for '{file_path}'"
                    ),
                    source_path: from_path.to_string(),
                    reference: target.to_string(),
                    kind: DiagnosticKind::Other,
                });
                output.push_str(line);
                output.push('\n');
                continue;
            }

            // Check for cycles.
            if visited.contains(file_path) {
                diagnostics.push(Diagnostic {
                    message: format!("Circular embed detected: '{file_path}'"),
                    source_path: from_path.to_string(),
                    reference: target.to_string(),
                    kind: DiagnosticKind::Other,
                });
                output.push_str(line);
                output.push('\n');
                continue;
            }

            // Try to read the file.
            match file_reader(file_path) {
                None => {
                    diagnostics.push(Diagnostic {
                        message: format!("Embed target not found: '{file_path}'"),
                        source_path: from_path.to_string(),
                        reference: target.to_string(),
                        kind: DiagnosticKind::Other,
                    });
                    output.push_str(line);
                    output.push('\n');
                }
                Some(file_content) => {
                    let body = strip_frontmatter(&file_content);

                    let section = if let Some(anchor) = heading_anchor {
                        if let Some(block_id) = anchor.strip_prefix('^') {
                            // Block reference: find paragraph containing ^block_id
                            match extract_block_section(body, block_id) {
                                Some(section) => section,
                                None => {
                                    diagnostics.push(Diagnostic {
                                        message: format!(
                                            "Block reference '^{block_id}' not found in '{file_path}'"
                                        ),
                                        source_path: from_path.to_string(),
                                        reference: target.to_string(),
                                        kind: DiagnosticKind::Other,
                                    });
                                    body.to_string()
                                }
                            }
                        } else {
                            // Heading reference
                            match extract_heading_section(body, anchor) {
                                Some(section) => section,
                                None => {
                                    diagnostics.push(Diagnostic {
                                        message: format!(
                                            "Heading '#{anchor}' not found in '{file_path}'"
                                        ),
                                        source_path: from_path.to_string(),
                                        reference: target.to_string(),
                                        kind: DiagnosticKind::Other,
                                    });
                                    body.to_string()
                                }
                            }
                        }
                    } else {
                        body.to_string()
                    };

                    // Recurse into the inlined content.
                    visited.insert(file_path.to_string());
                    let nested =
                        resolve_embeds_inner(&section, file_path, file_reader, visited, depth + 1);
                    visited.remove(file_path);

                    diagnostics.extend(nested.diagnostics);
                    embed_deps.extend(nested.embed_deps);

                    // Append the resolved content. Ensure it ends with a newline
                    // so subsequent lines are not joined.
                    output.push_str(&nested.content);
                    if !nested.content.ends_with('\n') {
                        output.push('\n');
                    }
                }
            }
        } else {
            output.push_str(line);
            output.push('\n');
        }
    }

    // If the original content did not end with a newline, remove the trailing
    // one we added.
    if !content.ends_with('\n') && output.ends_with('\n') {
        output.pop();
    }

    EmbedResult {
        content: output,
        diagnostics,
        embed_deps,
    }
}

/// Parse an embed marker line and return the target string.
///
/// `<!-- moss-embed:path/to/file.md -->` => `Some("path/to/file.md")`
/// `<!-- moss-embed:path/to/file.md#heading -->` => `Some("path/to/file.md#heading")`
fn parse_embed_marker(line: &str) -> Option<&str> {
    let rest = line.strip_prefix(EMBED_PREFIX)?;
    let target = rest.strip_suffix(EMBED_SUFFIX)?;
    if target.is_empty() {
        return None;
    }
    Some(target)
}

/// Split a target string into (file_path, optional heading_anchor).
///
/// `"guide.md#getting-started"` => `("guide.md", Some("getting-started"))`
/// `"guide.md"` => `("guide.md", None)`
fn split_target(target: &str) -> (&str, Option<&str>) {
    match target.split_once('#') {
        Some((file_path, anchor)) => {
            if anchor.is_empty() {
                (file_path, None)
            } else {
                (file_path, Some(anchor))
            }
        }
        None => (target, None),
    }
}

/// Strip frontmatter from file content, in either dialect.
///
/// A transcluded page whose frontmatter is simplified used to have its field
/// lines rendered as prose inside the embedding page — this saw a YAML block or
/// nothing (moss#937).
fn strip_frontmatter(content: &str) -> &str {
    match crate::frontmatter::frontmatter_span(content) {
        // Char-aligned: `body` is a line-boundary offset from the splitter.
        #[allow(clippy::string_slice)]
        Some(span) => &content[span.body..],
        None => content,
    }
}

/// Extract the section under a specific heading, identified by its anchor.
///
/// Returns everything from the heading line through the next heading of equal
/// or higher level (or end of file).
fn extract_heading_section(body: &str, target_anchor: &str) -> Option<String> {
    let lines: Vec<&str> = body.lines().collect();
    let mut start_idx = None;
    let mut heading_level = 0;

    // Slug the target the same way headings are slugged. The wikilink resolver
    // emits the marker with the RAW anchor the author typed (`#Template slots`),
    // so comparing it directly against slugged heading anchors (`template-slots`)
    // would falsely miss. `obsidian_heading_anchor` is idempotent, so a target
    // that is already in slug form (`#getting-started`) still matches.
    let target = obsidian_heading_anchor(target_anchor);

    // Find the heading whose anchor matches.
    for (i, line) in lines.iter().enumerate() {
        if let Some((level, text)) = parse_heading(line) {
            let anchor = obsidian_heading_anchor(text);
            if anchor == target {
                start_idx = Some(i);
                heading_level = level;
                break;
            }
        }
    }

    let start = start_idx?;

    // Find where the section ends: next heading of equal or higher level.
    let mut end_idx = lines.len();
    for i in (start + 1)..lines.len() {
        if let Some((level, _)) = parse_heading(lines[i]) {
            if level <= heading_level {
                end_idx = i;
                break;
            }
        }
    }

    let section = lines[start..end_idx].join("\n");
    Some(section)
}

/// Extract the line containing a block reference marker, with the marker stripped.
///
/// Block references are `^id` markers at the end of a line, preceded by a space.
/// Returns the line content without the marker, or `None` if not found.
///
/// The space-before-`^` check prevents substring collisions: looking up `^stem`
/// must not match a line tagged `^def-stem`.
fn extract_block_section(body: &str, block_id: &str) -> Option<String> {
    let marker = format!(" ^{}", block_id);
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(content) = trimmed.strip_suffix(marker.as_str()) {
            let content = content.trim_end();
            if !content.is_empty() {
                return Some(content.to_string());
            }
        }
    }
    None
}

/// Parse a markdown heading line into (level, text).
///
/// `"## Getting Started"` => `Some((2, "Getting Started"))`
fn parse_heading(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('#') {
        return None;
    }

    let level = trimmed.chars().take_while(|&c| c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }

    // Must be followed by a space (or be just hashes at end of line).
    // `level` counts ASCII '#' characters, each 1 byte, so byte-offset == count.
    #[allow(clippy::string_slice)]
    // Char-aligned: leading chars are all ASCII '#' (1-byte each), so `level` is a valid byte index.
    let rest = &trimmed[level..];
    if rest.is_empty() {
        return Some((level, ""));
    }
    let Some(after_space) = rest.strip_prefix(' ') else {
        return None;
    };

    Some((level, after_space.trim()))
}

// ---------------------------------------------------------------------------
// Deferred-marker post-pass
// ---------------------------------------------------------------------------

/// A resolver for one kind of Deferred embed marker.
///
/// Given the marker's target body (everything between `<!-- <prefix>:` and
/// ` -->`), returns the HTML to splice in. The `diagnostics` buffer is for
/// reporting I/O errors or invalid targets; the handler is expected to
/// return a best-effort fallback HTML even on failure so the build doesn't
/// stall.
pub type MarkerHandler<'a> = Box<dyn Fn(&str, &mut Vec<Diagnostic>) -> String + Send + Sync + 'a>;

/// Registry of marker-prefix → handler, used by
/// [`resolve_deferred_markers`] to dispatch Deferred embeds
/// ([`crate::resolve::embed_renderer::RenderedEmbed::Deferred`]) in a post-pass.
///
/// The built-in `moss-embed:` (markdown transclusion) is **not** dispatched
/// here — it's resolved by [`resolve_embeds`] in an earlier pass. This
/// registry handles typed prefixes: `moss-embed-ipynb`, `moss-embed-table`,
/// `moss-embed-plugin-<name>`, etc.
///
/// Use [`super::embed_renderer::MARKER_IPYNB`] / [`super::embed_renderer::MARKER_TABLE`]
/// (etc.) as prefixes to avoid stringly-typed drift.
pub struct MarkerHandlers<'a> {
    handlers: Vec<(String, MarkerHandler<'a>)>,
}

impl<'a> MarkerHandlers<'a> {
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    /// Register a handler for markers whose prefix is `<prefix>:`.
    ///
    /// `prefix` should NOT include the trailing colon — the scanner matches
    /// `<!-- <prefix>:` automatically.
    pub fn register(&mut self, prefix: impl Into<String>, handler: MarkerHandler<'a>) {
        self.handlers.push((prefix.into(), handler));
    }

    /// Find the handler whose prefix matches the body of this marker.
    /// Returns `(prefix, handler, target_body)`.
    fn find<'b>(
        &'b self,
        marker_body: &'b str,
    ) -> Option<(&'b str, &'b MarkerHandler<'a>, &'b str)> {
        self.handlers.iter().find_map(|(p, h)| {
            let needle = format!("{}:", p);
            marker_body
                .strip_prefix(needle.as_str())
                .map(|tail| (p.as_str(), h, tail))
        })
    }

    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }
}

impl<'a> Default for MarkerHandlers<'a> {
    fn default() -> Self {
        Self::new()
    }
}

/// Scan `content` for Deferred markers and dispatch each to its registered
/// handler. Markers with no registered handler are left intact (so they
/// survive to the final HTML as comments — visible, greppable, not silently
/// swallowed if a resolver is missing).
///
/// This is a pure string transform. The handler closures may do I/O; this
/// function does not.
pub fn resolve_deferred_markers(content: &str, handlers: &MarkerHandlers<'_>) -> DeferredResult {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    if handlers.is_empty() {
        return DeferredResult {
            content: content.to_string(),
            diagnostics,
        };
    }

    let mut out = String::with_capacity(content.len());
    let mut remaining = content;

    loop {
        // Find the next "<!-- " marker start.
        let Some((before, after_start)) = remaining.split_once("<!-- ") else {
            out.push_str(remaining);
            break;
        };
        // Copy everything up to the marker verbatim.
        out.push_str(before);

        // Find the closing " -->".
        let Some((marker_body, rest)) = after_start.split_once(" -->") else {
            // Unclosed; copy from "<!-- " onwards verbatim.
            out.push_str("<!-- ");
            out.push_str(after_start);
            break;
        };

        match handlers.find(marker_body) {
            Some((_prefix, handler, target)) => {
                let resolved = handler(target, &mut diagnostics);
                out.push_str(&resolved);
            }
            None => {
                // Unrecognized marker — leave it as-is.
                out.push_str("<!-- ");
                out.push_str(marker_body);
                out.push_str(" -->");
            }
        }
        remaining = rest;
    }

    DeferredResult {
        content: out,
        diagnostics,
    }
}

/// Output of [`resolve_deferred_markers`].
#[derive(Debug)]
pub struct DeferredResult {
    pub content: String,
    pub diagnostics: Vec<Diagnostic>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn mock_reader(files: &HashMap<String, String>) -> impl Fn(&str) -> Option<String> + '_ {
        move |path: &str| files.get(path).cloned()
    }

    #[test]
    fn test_basic_embed() {
        let mut files = HashMap::new();
        files.insert(
            "note.md".to_string(),
            "---\ntitle: Note\n---\nHello from note.".to_string(),
        );

        let content = "Before.\n<!-- moss-embed:note.md -->\nAfter.";
        let result = resolve_embeds(content, "index.md", &mock_reader(&files));

        assert_eq!(result.content, "Before.\nHello from note.\nAfter.");
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn test_heading_scoped_embed() {
        let mut files = HashMap::new();
        files.insert(
            "guide.md".to_string(),
            "---\ntitle: Guide\n---\n# Intro\nIntro text.\n## Getting Started\nStart here.\n## Advanced\nAdvanced stuff."
                .to_string(),
        );

        let content = "<!-- moss-embed:guide.md#getting-started -->";
        let result = resolve_embeds(content, "index.md", &mock_reader(&files));

        assert!(result.content.contains("## Getting Started"));
        assert!(result.content.contains("Start here."));
        assert!(!result.content.contains("Advanced stuff."));
        assert!(!result.content.contains("Intro text."));
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn test_heading_scoped_embed_raw_anchor() {
        // Regression: the wikilink resolver emits the embed marker with the
        // RAW, un-slugged anchor exactly as the author typed it — e.g.
        // `![[slots#Template slots]]` becomes
        // `<!-- moss-embed:slots.md#Template slots -->` (space + capital T).
        // The resolver must slug the target anchor the same way it slugs each
        // heading, or it falsely reports "Heading not found" and inlines the
        // WHOLE file as a fallback (leaking every sibling section).
        let mut files = HashMap::new();
        files.insert(
            "slots.md".to_string(),
            "---\ntitle: Slots\n---\n## Template slots\nSlots are injection points. ^def-template-slots\n## How slots work\nOther content.".to_string(),
        );

        let content = "<!-- moss-embed:slots.md#Template slots -->";
        let result = resolve_embeds(content, "hooks.md", &mock_reader(&files));

        // No false "not found" diagnostic.
        assert!(
            result.diagnostics.is_empty(),
            "raw anchor should resolve cleanly, got diagnostics: {:?}",
            result.diagnostics
        );
        // Only the target section is inlined — the sibling section must NOT leak.
        assert!(
            result.content.contains("Slots are injection points."),
            "target section content missing: {}",
            result.content
        );
        assert!(
            !result.content.contains("Other content."),
            "whole-file leak: sibling section inlined:\n{}",
            result.content
        );
    }

    #[test]
    fn test_extract_heading_section_raw_anchor() {
        // `extract_heading_section` must accept a raw (un-slugged) target
        // anchor — spaces and mixed case — not just a pre-slugged one.
        let body = "## Template Slots\nBody.\n## Next\nNope.";
        let section =
            extract_heading_section(body, "Template Slots").expect("raw anchor should match");
        assert!(section.contains("Body."));
        assert!(!section.contains("Nope."));
    }

    #[test]
    fn test_heading_not_found() {
        let mut files = HashMap::new();
        files.insert(
            "guide.md".to_string(),
            "---\ntitle: Guide\n---\n# Intro\nIntro text.".to_string(),
        );

        let content = "<!-- moss-embed:guide.md#nonexistent -->";
        let result = resolve_embeds(content, "index.md", &mock_reader(&files));

        // Full body inlined when heading not found.
        assert!(result.content.contains("Intro text."));
        assert_eq!(result.diagnostics.len(), 1);
        assert!(result.diagnostics[0].message.contains("not found"));
    }

    #[test]
    fn test_circular_embed_detection() {
        let mut files = HashMap::new();
        files.insert(
            "a.md".to_string(),
            "A content.\n<!-- moss-embed:b.md -->".to_string(),
        );
        files.insert(
            "b.md".to_string(),
            "B content.\n<!-- moss-embed:a.md -->".to_string(),
        );

        let content = "<!-- moss-embed:a.md -->";
        let result = resolve_embeds(content, "index.md", &mock_reader(&files));

        // A's content should be inlined (including B's content), but the
        // circular reference back to A should produce a diagnostic.
        assert!(result.content.contains("A content."));
        assert!(result.content.contains("B content."));
        let cycle_diag = result
            .diagnostics
            .iter()
            .find(|d| d.message.contains("Circular"));
        assert!(cycle_diag.is_some(), "Expected a circular embed diagnostic");
    }

    #[test]
    fn test_max_depth_protection() {
        // Build a chain: file0 embeds file1, file1 embeds file2, ..., up to 12.
        let mut files = HashMap::new();
        for i in 0..12 {
            let next = i + 1;
            files.insert(
                format!("file{i}.md"),
                format!("Content {i}.\n<!-- moss-embed:file{next}.md -->"),
            );
        }
        files.insert("file12.md".to_string(), "End.".to_string());

        let content = "<!-- moss-embed:file0.md -->";
        let result = resolve_embeds(content, "index.md", &mock_reader(&files));

        // Should have a depth-exceeded diagnostic.
        let depth_diag = result
            .diagnostics
            .iter()
            .find(|d| d.message.contains("depth limit"));
        assert!(
            depth_diag.is_some(),
            "Expected a depth limit diagnostic, got: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn test_file_not_found() {
        let files = HashMap::new();

        let content = "<!-- moss-embed:missing.md -->";
        let result = resolve_embeds(content, "index.md", &mock_reader(&files));

        assert_eq!(result.diagnostics.len(), 1);
        assert!(result.diagnostics[0].message.contains("not found"));
        // The marker should be preserved when the file is not found.
        assert!(result.content.contains("<!-- moss-embed:missing.md -->"));
    }

    #[test]
    fn test_unresolved_marker_preserved() {
        let files = HashMap::new();

        let content = "Before.\n<!-- moss-embed-unresolved:some-ref -->\nAfter.";
        let result = resolve_embeds(content, "index.md", &mock_reader(&files));

        assert!(result
            .content
            .contains("<!-- moss-embed-unresolved:some-ref -->"));
        assert!(result.diagnostics.is_empty());
        assert_eq!(
            result.content,
            "Before.\n<!-- moss-embed-unresolved:some-ref -->\nAfter."
        );
    }

    #[test]
    fn test_recursive_embed() {
        let mut files = HashMap::new();
        files.insert(
            "a.md".to_string(),
            "A content.\n<!-- moss-embed:b.md -->".to_string(),
        );
        files.insert("b.md".to_string(), "B content.".to_string());

        let content = "<!-- moss-embed:a.md -->";
        let result = resolve_embeds(content, "index.md", &mock_reader(&files));

        assert!(result.content.contains("A content."));
        assert!(result.content.contains("B content."));
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn test_embed_deps_tracked() {
        let mut files = HashMap::new();
        files.insert(
            "a.md".to_string(),
            "A content.\n<!-- moss-embed:b.md -->".to_string(),
        );
        files.insert("b.md".to_string(), "B content.".to_string());

        let content = "<!-- moss-embed:a.md -->";
        let result = resolve_embeds(content, "index.md", &mock_reader(&files));

        // index.md -> a.md, a.md -> b.md
        assert!(
            result
                .embed_deps
                .contains(&("a.md".to_string(), "index.md".to_string())),
            "Missing dep: (a.md, index.md). Got: {:?}",
            result.embed_deps
        );
        assert!(
            result
                .embed_deps
                .contains(&("b.md".to_string(), "a.md".to_string())),
            "Missing dep: (b.md, a.md). Got: {:?}",
            result.embed_deps
        );
    }

    #[test]
    fn test_no_frontmatter() {
        let mut files = HashMap::new();
        files.insert(
            "plain.md".to_string(),
            "Just plain content.\nSecond line.".to_string(),
        );

        let content = "<!-- moss-embed:plain.md -->";
        let result = resolve_embeds(content, "index.md", &mock_reader(&files));

        assert_eq!(result.content, "Just plain content.\nSecond line.");
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn test_multiple_embeds() {
        let mut files = HashMap::new();
        files.insert("one.md".to_string(), "Content one.".to_string());
        files.insert("two.md".to_string(), "Content two.".to_string());

        let content = "<!-- moss-embed:one.md -->\n<!-- moss-embed:two.md -->";
        let result = resolve_embeds(content, "index.md", &mock_reader(&files));

        assert!(result.content.contains("Content one."));
        assert!(result.content.contains("Content two."));
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn test_content_around_embed_preserved() {
        let mut files = HashMap::new();
        files.insert("note.md".to_string(), "Note content.".to_string());

        let content = "Paragraph before.\n\n<!-- moss-embed:note.md -->\n\nParagraph after.";
        let result = resolve_embeds(content, "index.md", &mock_reader(&files));

        assert!(result.content.contains("Paragraph before."));
        assert!(result.content.contains("Note content."));
        assert!(result.content.contains("Paragraph after."));
        assert!(result.diagnostics.is_empty());
    }

    // ----- Helper unit tests -----

    #[test]
    fn test_parse_embed_marker() {
        assert_eq!(
            parse_embed_marker("<!-- moss-embed:path/to/file.md -->"),
            Some("path/to/file.md")
        );
        assert_eq!(
            parse_embed_marker("<!-- moss-embed:file.md#heading -->"),
            Some("file.md#heading")
        );
        assert_eq!(parse_embed_marker("<!-- moss-embed: -->"), None);
        assert_eq!(parse_embed_marker("not an embed marker"), None);
        assert_eq!(
            parse_embed_marker("<!-- moss-embed-unresolved:ref -->"),
            None
        );
    }

    #[test]
    fn test_split_target() {
        assert_eq!(split_target("file.md"), ("file.md", None));
        assert_eq!(
            split_target("file.md#heading"),
            ("file.md", Some("heading"))
        );
        assert_eq!(split_target("file.md#"), ("file.md", None));
        assert_eq!(
            split_target("path/to/file.md#deep-heading"),
            ("path/to/file.md", Some("deep-heading"))
        );
    }

    #[test]
    fn test_strip_frontmatter_basic() {
        let input = "---\ntitle: Test\n---\nBody content.";
        assert_eq!(strip_frontmatter(input), "Body content.");
    }

    #[test]
    fn test_strip_frontmatter_none() {
        let input = "No frontmatter here.\nJust content.";
        assert_eq!(strip_frontmatter(input), input);
    }

    #[test]
    fn test_strip_frontmatter_no_closing() {
        let input = "---\ntitle: Test\nNo closing delimiter.";
        // No closing `---`, so treat as no frontmatter.
        assert_eq!(strip_frontmatter(input), input);
    }

    #[test]
    fn test_extract_heading_section_basic() {
        let body = "# Intro\nIntro text.\n## Getting Started\nStart here.\n## Advanced\nAdvanced.";
        let section = extract_heading_section(body, "getting-started");
        assert!(section.is_some());
        let s = section.unwrap();
        assert!(s.contains("## Getting Started"));
        assert!(s.contains("Start here."));
        assert!(!s.contains("Advanced."));
        assert!(!s.contains("Intro text."));
    }

    #[test]
    fn test_extract_heading_section_last() {
        let body = "# Intro\nIntro text.\n## Last Section\nLast content.";
        let section = extract_heading_section(body, "last-section");
        assert!(section.is_some());
        let s = section.unwrap();
        assert!(s.contains("## Last Section"));
        assert!(s.contains("Last content."));
    }

    #[test]
    fn test_extract_heading_section_not_found() {
        let body = "# Intro\nIntro text.";
        assert!(extract_heading_section(body, "nonexistent").is_none());
    }

    #[test]
    fn test_parse_heading() {
        assert_eq!(parse_heading("# Title"), Some((1, "Title")));
        assert_eq!(parse_heading("## Sub Title"), Some((2, "Sub Title")));
        assert_eq!(parse_heading("###### Deep"), Some((6, "Deep")));
        assert_eq!(parse_heading("Not a heading"), None);
        assert_eq!(parse_heading("#NoSpace"), None);
        assert_eq!(parse_heading(""), None);
    }

    #[test]
    fn test_block_ref_embed() {
        let mut files = HashMap::new();
        files.insert(
            "concepts.md".to_string(),
            "---\ntitle: Concepts\n---\nA **stem** is a folder's own page. ^def-stem\n\nA **leaf** is an article. ^def-leaf"
                .to_string(),
        );

        let content = "<!-- moss-embed:concepts.md#^def-stem -->";
        let result = resolve_embeds(content, "index.md", &mock_reader(&files));

        assert!(result.content.contains("stem"), "Should contain 'stem'");
        assert!(
            !result.content.contains("leaf"),
            "Should not contain 'leaf'"
        );
        assert!(
            !result.content.contains("^def-stem"),
            "Should strip block ref marker"
        );
        assert!(result.diagnostics.is_empty(), "Should have no diagnostics");
    }

    #[test]
    fn test_block_ref_not_found() {
        let mut files = HashMap::new();
        files.insert(
            "note.md".to_string(),
            "---\ntitle: Note\n---\nSome content.".to_string(),
        );

        let content = "<!-- moss-embed:note.md#^nonexistent -->";
        let result = resolve_embeds(content, "index.md", &mock_reader(&files));

        assert_eq!(result.diagnostics.len(), 1);
        assert!(result.diagnostics[0].message.contains("Block reference"));
    }

    #[test]
    fn test_extract_block_section_basic() {
        let body =
            "First paragraph.\n\nA **stem** is a folder's own page. ^def-stem\n\nLast paragraph.";
        let section = extract_block_section(body, "def-stem");
        assert!(section.is_some());
        let s = section.unwrap();
        assert!(s.contains("stem"));
        assert!(!s.contains("^def-stem"));
        assert!(!s.contains("Last paragraph"));
    }

    #[test]
    fn test_extract_block_section_not_found() {
        let body = "No block refs here.";
        assert!(extract_block_section(body, "missing").is_none());
    }

    #[test]
    fn test_extract_block_section_no_substring_collision() {
        let body = "About stems. ^stem\nA **stem** is a folder's own page. ^def-stem";
        // Looking for "stem" must match the line tagged ^stem, not ^def-stem
        let section = extract_block_section(body, "stem");
        assert!(section.is_some());
        assert!(section.as_ref().unwrap().contains("About stems"));
        assert!(!section.unwrap().contains("folder"));
    }

    // --- MarkerHandlers / resolve_deferred_markers ---

    #[test]
    fn test_deferred_markers_empty_handlers_noop() {
        let content = "before <!-- moss-embed-ipynb:x.ipynb --> after";
        let handlers = MarkerHandlers::new();
        let r = resolve_deferred_markers(content, &handlers);
        assert_eq!(r.content, content);
    }

    #[test]
    fn test_deferred_markers_dispatches_single() {
        let content = "before <!-- moss-embed-ipynb:nb.ipynb --> after";
        let mut h = MarkerHandlers::new();
        h.register(
            "moss-embed-ipynb",
            Box::new(|target, _| format!("<div class=\"nb\">{}</div>", target)),
        );
        let r = resolve_deferred_markers(content, &h);
        assert_eq!(r.content, "before <div class=\"nb\">nb.ipynb</div> after");
    }

    #[test]
    fn test_deferred_markers_dispatches_multiple_different_prefixes() {
        let content = "a <!-- moss-embed-ipynb:n.ipynb --> b <!-- moss-embed-table:d.csv --> c";
        let mut h = MarkerHandlers::new();
        h.register("moss-embed-ipynb", Box::new(|t, _| format!("[nb:{}]", t)));
        h.register("moss-embed-table", Box::new(|t, _| format!("[tbl:{}]", t)));
        let r = resolve_deferred_markers(content, &h);
        assert_eq!(r.content, "a [nb:n.ipynb] b [tbl:d.csv] c");
    }

    #[test]
    fn test_deferred_markers_unknown_prefix_left_intact() {
        let content = "before <!-- moss-embed-unknown:foo --> after";
        let mut h = MarkerHandlers::new();
        h.register("moss-embed-ipynb", Box::new(|_, _| String::new()));
        let r = resolve_deferred_markers(content, &h);
        // Unknown marker preserved verbatim so bugs are visible.
        assert!(r.content.contains("<!-- moss-embed-unknown:foo -->"));
    }

    #[test]
    fn test_deferred_markers_handler_can_emit_diagnostics() {
        let content = "<!-- moss-embed-ipynb:bad -->";
        let mut h = MarkerHandlers::new();
        h.register(
            "moss-embed-ipynb",
            Box::new(|t, diags| {
                diags.push(Diagnostic {
                    message: format!("synthetic failure for {}", t),
                    source_path: "".to_string(),
                    reference: t.to_string(),
                    kind: DiagnosticKind::Other,
                });
                "<div class=\"error\"></div>".to_string()
            }),
        );
        let r = resolve_deferred_markers(content, &h);
        assert_eq!(r.diagnostics.len(), 1);
        assert!(r.diagnostics[0].message.contains("synthetic failure"));
    }

    #[test]
    fn test_deferred_markers_prefix_matching_exact() {
        // A handler for "moss-embed" must NOT swallow "moss-embed-ipynb".
        let content = "<!-- moss-embed-ipynb:nb.ipynb -->";
        let mut h = MarkerHandlers::new();
        h.register("moss-embed", Box::new(|_, _| "WRONG".to_string()));
        let r = resolve_deferred_markers(content, &h);
        // "moss-embed:" would match; "moss-embed-ipynb:" would not.
        // Our format!("{}:", prefix) matches "moss-embed:", which doesn't
        // prefix "moss-embed-ipynb:" — correct disjoint behavior.
        assert!(r.content.contains("moss-embed-ipynb"), "got: {}", r.content);
    }

    #[test]
    fn test_deferred_markers_unclosed_marker_preserved() {
        let content = "before <!-- moss-embed-ipynb:no-closing";
        let mut h = MarkerHandlers::new();
        h.register("moss-embed-ipynb", Box::new(|_, _| "NO".to_string()));
        let r = resolve_deferred_markers(content, &h);
        assert_eq!(r.content, content);
    }
}
