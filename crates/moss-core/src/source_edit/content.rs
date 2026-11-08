//! Parsing and path-only editor decisions.

use super::{ParsedFrontmatter, SourceRole};
use crate::page::content_hash;

/// The pure half: everything `parse_frontmatter` does once the bytes are in
/// hand. Split out so the blocking read can happen off the async runtime.
pub fn parse_frontmatter_of(content: String) -> Result<ParsedFrontmatter, String> {
    let hash = content_hash(content.as_bytes());
    let doc = crate::frontmatter::parse(&content);

    // Convert the HashMap<String, serde_yaml::Value> to serde_json::Value
    // for consistent JSON transport to the frontend.
    let fm_json = super::conversion::yaml_map_to_json(&doc.frontmatter)?;

    Ok(ParsedFrontmatter {
        hash,
        frontmatter: fm_json,
        body: doc.body,
        has_frontmatter: doc.frontmatter_range.is_some(),
        frontmatter_error: doc.frontmatter_error,
        still_arriving: false,
    })
}

/// Resolve the editor URL's `source` param to an absolute path. The param is
/// typically absolute (baked by `build_editor_url` from `resolve_page_source`),
/// but the file-tree event domain is project-root-relative — accept both:
/// absolute passes through, relative joins onto the project path (mirror of
/// the app's own `toAbsolute`).
pub fn resolve_bootstrap_source(project_path: Option<&str>, source: &str) -> Option<String> {
    if std::path::Path::new(source).is_absolute() {
        return Some(source.to_string());
    }
    project_path.map(|root| {
        std::path::Path::new(root)
            .join(source)
            .to_string_lossy()
            .to_string()
    })
}

/// Which file `editor_bootstrap` should pre-parse, if any: only `.md`/
/// `.markdown` sources (case-insensitive), resolved to an absolute path.
///
/// Pure — no I/O — so the async command can do the read itself, off the
/// runtime. `None` here (and any later failure) just means the frontend falls
/// back to its existing `parse_frontmatter` call.
pub fn bootstrap_source_path(
    project_path: Option<&str>,
    source: Option<&str>,
) -> Option<String> {
    let source = source?;
    let is_markdown = std::path::Path::new(source)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
        .unwrap_or(false);
    if !is_markdown {
        return None;
    }
    resolve_bootstrap_source(project_path, source)
}

/// Core of `compute_heading_state`: the JSON `Value` → `HeadingInputs` bridge
/// over `crate::heading::compute` — the single source of truth shared with
/// the build pipeline's `<h1 class="moss-article-title">` injection gate
/// (`build::markdown::pipeline`).
pub fn compute_heading_state_inner(
    file_path: &str,
    frontmatter: &serde_json::Value,
    body: &str,
) -> crate::heading::HeadingState {
    let title_str = match frontmatter.get("title") {
        Some(serde_json::Value::String(s)) => Some(s.as_str()),
        _ => None,
    };

    // Apply CriticMarkup acceptance before hero-from-body detection so the
    // editor sees the same normalized body the build pipeline does
    // (pipeline.rs:103). Otherwise an author with a leading `{>>comment<<}`
    // before `:::hero` would see the editor and pipeline disagree about
    // hero ownership of the heading slot.
    let accepted_body = crate::resolve::source::accept_criticmarkup(body);

    crate::heading::compute(crate::heading::HeadingInputs {
        file_path,
        frontmatter_title: title_str,
        body_markdown: &accepted_body,
        // The editor command receives a workspace-relative file_path, but
        // doesn't have the workspace folder's basename here. Self-named
        // home detection at the project root will fall through to the
        // path-based check (i.e. won't catch `<root>/<root>.md`). The
        // editor's heading state is already a "best effort" preview vs
        // the build pipeline's authoritative answer; this is one of the
        // pre-existing gaps documented at HeadingInputs::root_folder_name.
        root_folder_name: None,
        is_home_override: false,
        // The editor doesn't render slot files (`footer.md`) inline
        // — the slot-file flow is exclusively the build pipeline's
        // concern. Pass false here so the editor's heading-state preview
        // for an open `footer.md` shows the would-be H1 the author can
        // toggle off via `title: ""`. The pipeline's authoritative
        // answer still suppresses the H1 because `is_excluded_from_pages`
        // sees the reserved filename.
        slot_only: false,
        dated: ["date", "start"].iter().any(|k| frontmatter.get(*k).is_some_and(|v| !v.is_null())),
    })
}

/// A source's language and slot role, from its path inside the site.
pub fn describe_source_role_inner(rel_path: &str) -> SourceRole {
    let normalized = crate::slug::normalize_separators(rel_path);
    let normalized = normalized.trim_matches('/');

    let (language, within_tree) = super::policy::language_tree_split(normalized);
    let slot =
        super::policy::slot_from_filename(within_tree).map(str::to_string);

    let extension = std::path::Path::new(within_tree)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let can_be_page = super::policy::is_page_source(&extension);

    SourceRole {
        language,
        slot,
        can_be_page,
    }
}

/// A host-classified source read; platform error probing stays with the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceRead {
    Text(String),
    StillArriving,
    Failed(String),
}

/// Translate host read facts into the editor's pending-read result.
pub fn classify_source_read(path: &str, read: SourceRead) -> Result<Option<String>, String> {
    match read {
        SourceRead::Text(text) => Ok(Some(text)),
        SourceRead::StillArriving => Ok(None),
        SourceRead::Failed(error) => Err(format!("Failed to read file '{}': {}", path, error)),
    }
}
