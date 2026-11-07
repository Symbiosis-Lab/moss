//! Check and render a page from host-supplied site values, without a build.

use crate::ast::{self, DefaultHooks, ParseConfig};
use crate::content_graph::ContentGraph;
use crate::frontmatter;
use crate::heading::{self, HeadingInputs, TitleOutcome};
use std::collections::HashMap;
use crate::resolve::md_extract::{extract_md_references, RefSyntax};
use crate::validation::{self, Diagnostic, Severity};

/// What a host knows about the site, as values.
pub struct SiteInputs {
    /// Site-relative paths of every file, for link resolution.
    pub files: Vec<String>,
    /// Site-relative paths to the source text of notes this page embeds.
    pub notes: HashMap<String, String>,
    /// The composed schema: built-in, site and plugin fields.
    pub schema: crate::schema::ContentSchema,
    /// BCP-47-ish language tag from the site configuration.
    pub lang: String,
}

/// Check frontmatter, moss syntax and page links. Lines count from the file's start.
pub fn check_page(source_path: &str, text: &str, site: &SiteInputs) -> Vec<Diagnostic> {
    let mut diagnostics = check_frontmatter(text, site);
    let body = frontmatter::body(text);
    let offset = line_at(text, text.len() - body.len()) - 1;
    diagnostics.extend(check_syntax(body, offset, site));

    let graph = graph(site);
    for reference in extract_md_references(body) {
        let (target, _) = split_suffix(&reference.text);
        if !is_local_target(target) {
            continue;
        }
        let check = match reference.syntax {
            RefSyntax::MarkdownLink { .. } | RefSyntax::Definition { .. } => {
                target.ends_with(".md")
            }
            RefSyntax::MarkdownImage { .. } | RefSyntax::StructuralAsset => false,
            _ => true,
        };
        if check && graph.resolve_path(target, source_path).is_none() {
            diagnostics.push(diagnostic(
                Severity::Warning,
                format!("unresolved link `{}`", reference.text),
                Some(offset + line_at(body, reference.byte_from)),
            ));
        }
    }
    diagnostics
}

fn check_frontmatter(text: &str, site: &SiteInputs) -> Vec<Diagnostic> {
    let span = frontmatter::frontmatter_span(text);
    let parsed = frontmatter::parse(text);
    if let Some(message) = parsed.frontmatter_error {
        let offset = span.as_ref().map_or(0, |s| line_at(text, s.fields.start) - 1);
        let line = message.rsplit_once(" at line ")
            .and_then(|(_, tail)| leading_number(tail))
            .map(|line| offset + line);
        return vec![diagnostic(Severity::Error, message, line)];
    }
    let fields = if span.as_ref().is_some_and(|s| s.kind == frontmatter::FrontmatterKind::Simplified) {
        frontmatter::frontmatter_map(text)
    } else {
        parsed.frontmatter
    };
    let mut diagnostics = validation::validate_frontmatter(&fields, &site.schema);
    if let Some(span) = span {
        let block = text.get(span.fields.clone()).unwrap_or_default();
        let offset = line_at(text, span.fields.start) - 1;
        for item in &mut diagnostics {
            if let Some(key) = item.path.as_deref().filter(|key| fields.contains_key(*key)) {
                item.line = field_line(block, key).map(|line| offset + line);
            }
        }
    }
    diagnostics
}

fn field_line(block: &str, key: &str) -> Option<usize> {
    let indent = block.lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .map(|line| line.len() - line.trim_start().len())
        .min()?;
    block.lines().enumerate().find_map(|(index, line)| {
        if line.len() - line.trim_start().len() != indent {
            return None;
        }
        if line.trim() == key {
            return Some(index + 1);
        }
        line.match_indices(':').find_map(|(colon, _)| {
            let candidate = line.get(..colon)?.trim();
            let decoded = serde_yaml::from_str::<String>(candidate).ok();
            (candidate == key || decoded.as_deref() == Some(key)).then_some(index + 1)
        })
    })
}

/// Each top-level shortcode block is parsed on its own, so the parser's
/// line-less warnings belong to that block's opening line.
fn check_syntax(body: &str, offset: usize, site: &SiteInputs) -> Vec<Diagnostic> {
    // Built-in definitions are empty, so these are the plugin shortcodes: the
    // parser does not know them, and their bodies are the plugin's to judge.
    let plugin = |name: &str| site.schema.shortcodes.as_ref().is_some_and(|s| s.definitions.contains_key(name));
    let mut diagnostics = Vec::new();
    let mut rest = body.as_bytes().to_vec();
    for block in &ast::editor_scan::editor_scan(body).blocks {
        let range = block.open.from as usize..block.close.to as usize;
        let open_line = offset + line_at(body, range.start);
        for divider in block.dividers.iter().filter(|d| d.legacy) {
            diagnostics.push(legacy_divider(offset + line_at(body, divider.range.from as usize)));
        }
        if let Some(bytes) = rest.get_mut(range.clone()) {
            bytes.iter_mut().filter(|b| **b != b'\n').for_each(|b| *b = b' ');
        }
        if plugin(&block.name) {
            continue;
        }
        let source = body.get(range).unwrap_or_default();
        let mut quoted_lines = HashMap::new();
        for message in parse_warnings(source, open_line - 1) {
            // The scan already placed a hint on each legacy divider.
            if !message.starts_with("`---` on line ") {
                let line = message_line(&message).or_else(|| quoted_opener_line(source, &message, open_line, &mut quoted_lines)).unwrap_or(open_line);
                diagnostics.push(diagnostic(Severity::Warning, message, Some(line)));
            }
        }
    }
    // Blanking whole fence lines with ASCII keeps the bytes valid UTF-8.
    let rest = String::from_utf8(rest).unwrap_or_default();
    for message in parse_warnings(&rest, offset) {
        let line = message_line(&message);
        diagnostics.push(diagnostic(Severity::Warning, message, line));
    }
    diagnostics
}

/// The line inside `source` (which starts at `first_line`) that opens with a
/// `:::` fence the message quotes, so a nested block's warning is not pinned
/// to its outer block.
fn quoted_opener_line(source: &str, message: &str, first_line: usize, used: &mut HashMap<String, usize>) -> Option<usize> {
    message.split('`').skip(1).step_by(2).filter(|quote| quote.starts_with(":::")).find_map(|quote| {
        let count = used.entry(quote.to_string()).or_default();
        let line = source.lines().enumerate()
            .filter(|(_, line)| line.trim_start().starts_with(quote))
            .nth(*count).map(|(index, _)| first_line + index);
        *count += usize::from(line.is_some());
        line
    })
}

fn parse_warnings(source: &str, line_offset: usize) -> Vec<String> {
    ast::parse_with_config(source, &ParseConfig { warning_line_offset: line_offset, ..parse_config() }).warnings
}

fn legacy_divider(line: usize) -> Diagnostic {
    diagnostic(Severity::Hint, "use `+++` instead of `---` between grid cells".into(), Some(line))
}

fn diagnostic(severity: Severity, message: String, line: Option<usize>) -> Diagnostic {
    Diagnostic { severity, message, path: None, line, column: None }
}

fn message_line(message: &str) -> Option<usize> {
    let tail = message.strip_prefix("line ")
        .or_else(|| message.split_once(" on line ").map(|(_, tail)| tail))?;
    leading_number(tail)
}

fn leading_number(tail: &str) -> Option<usize> {
    tail.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
}

fn line_at(text: &str, byte: usize) -> usize {
    text.bytes().take(byte).filter(|b| *b == b'\n').count() + 1
}

fn graph(site: &SiteInputs) -> ContentGraph {
    ContentGraph::from_paths(&site.files.iter().map(String::as_str).collect::<Vec<_>>())
}

fn parse_config() -> ParseConfig {
    ParseConfig { math: true, hard_line_breaks: true, ..Default::default() }
}

/// The identity of a page's exact bytes: lowercase hex SHA-256. The engine's
/// save names its base revision with it and a preview carries it, so a host
/// can tell which text a render came from. mtimes are not used, because a
/// sync client can restore an older one and two writes can share a millisecond.
pub fn content_hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

/// A page's resolved title and rendered body, without site chrome.
pub struct RenderedPage {
    pub title: String,
    pub title_outcome: TitleOutcome,
    pub body_html: String,
    pub diagnostics: Vec<Diagnostic>,
}

/// Site-relative note paths needed to expand this page's transclusions.
/// A host can read these paths into `SiteInputs::notes` before rendering.
/// Paths are unique and in first-use order; links and media are excluded.
pub fn embedded_notes(source_path: &str, text: &str, site: &SiteInputs) -> Vec<String> {
    // Resolved before source preparation, as render_page and the build do, so
    // discovery names exactly the notes rendering will ask for.
    let resolved = crate::resolve::resolve_content(source_path, frontmatter::body(text), &graph(site), &|_| None);
    let mut paths = Vec::new();
    for (path, _) in resolved.embed_deps {
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

/// Render one page using the default math and diagram hooks.
pub fn render_page(source_path: &str, text: &str, site: &SiteInputs) -> RenderedPage {
    let fields = frontmatter::frontmatter_map(text);
    let body = frontmatter::body(text);
    let offset = line_at(text, text.len() - body.len()) - 1;
    let graph = graph(site);
    let resolved = crate::resolve::resolve_content(source_path, body, &graph, &|path| site.notes.get(path).cloned());
    let (markdown, warnings) = crate::resolve::prepare_markdown_source(&resolved.content_markdown, offset);
    let mut diagnostics: Vec<_> = warnings.into_iter().map(|message| {
        let line = message_line(&message);
        diagnostic(Severity::Warning, message, line)
    }).collect();
    let mut doc = ast::parse_with_config(&markdown, &ParseConfig { warning_line_offset: offset, ..parse_config() });
    diagnostics.extend(doc.warnings.iter().map(|message| diagnostic(Severity::Warning, message.clone(), message_line(message))));
    let resolution = ast::resolve_document(
        &mut doc, &crate::asset_snapshot::AssetSnapshot::new(), Some(&graph), source_path,
        parse_config().implicit_figure, &|target| page_url(target, source_path, &graph),
    );
    diagnostics.extend(resolved.diagnostics.into_iter().chain(resolution.diagnostics).map(|d| {
        Diagnostic { severity: Severity::Warning, message: d.message, path: Some(d.source_path), line: None, column: None }
    }));
    let inputs = HeadingInputs {
        file_path: source_path,
        frontmatter_title: fields.get("title").and_then(|v| v.as_str()),
        body_markdown: &markdown,
        root_folder_name: None,
        is_home_override: fields.get("home").and_then(|v| v.as_bool()) == Some(true),
        slot_only: source_path == "footer.md",
        dated: fields.contains_key("date") || fields.contains_key("start"),
    };
    let title = heading::compute(inputs).text;
    let title_outcome = heading::article_title_outcome(inputs, fields.get("nav").and_then(|v| v.as_bool()) == Some(true));
    RenderedPage { title, title_outcome, diagnostics, body_html: ast::render_document(&doc, &DefaultHooks::new()) }
}

fn split_suffix(target: &str) -> (&str, &str) {
    let split = target.find(['#', '?']).unwrap_or(target.len());
    (target.get(..split).unwrap_or(target), target.get(split..).unwrap_or_default())
}

fn is_local_target(target: &str) -> bool {
    !target.is_empty() && !target.starts_with("//") && !target.contains(':')
}

fn page_url(raw: &str, source_path: &str, graph: &ContentGraph) -> String {
    let (target, suffix) = split_suffix(raw.strip_prefix("moss-resolved:").unwrap_or(raw));
    if !is_local_target(target) {
        return raw.to_string();
    }
    let from = if raw.starts_with("moss-resolved:") { "" } else { source_path };
    let Some(path) = graph.resolve_path(target, from) else {
        return raw.to_string();
    };
    format!("{}{suffix}", ast::transform_markdown_link(path.trim_start_matches('/')))
}

/// Wrap rendered content in the published article shell for a host preview pane.
pub fn preview_document(page: &RenderedPage, lang: &str, css: &str, base_href: &str) -> String {
    let title = &page.title;
    let body = match page.title_outcome {
        TitleOutcome::None => page.body_html.clone(),
        outcome => heading::inject_article_title_h1(&page.body_html, title, false, outcome == TitleOutcome::Hidden),
    };
    format!(
        "<!doctype html>\n<html lang=\"{}\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><base href=\"{}\"><title>{}</title><style>{css}</style></head><body><main id=\"main-content\" tabindex=\"-1\"><article class=\"container\">{}</article></main></body></html>",
        crate::media::html_escape_attr(lang), crate::media::html_escape_attr(base_href),
        crate::media::html_escape_text(title), body,
    )
}

#[cfg(test)]
#[path = "page_tests.rs"]
mod tests;
