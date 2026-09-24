//! Marker-handler implementations for Deferred embed renderers.
//!
//! Typed embed renderers in moss-core (Phase A–E) can't perform I/O. When a
//! renderer needs file content (notebook rendering, CSV parsing, plugin
//! scripts), it emits a `RenderedEmbed::Deferred { marker }` that the desktop
//! app resolves in a post-pass via
//! [`moss_core::resolve::embeds::resolve_deferred_markers`].
//!
//! This module builds a [`MarkerHandlers`] registry pre-populated with the
//! built-in resolvers:
//! - `moss-embed-ipynb:<path>` → `<iframe>` to the JupyterLite viewer page
//! - `moss-embed-table:<path>` → `<table>` via `moss_core::csv_table`
//!
//! Plugin-registered handlers (for `moss-embed-plugin-<name>:` markers) are
//! added by the desktop app's plugin runtime at pipeline init.

use std::path::{Path, PathBuf};

use moss_core::resolve::embed_renderer::{CLASS_EMBED, CLASS_EMBED_NOTEBOOK, CLASS_EMBED_TABLE};
use moss_core::resolve::embeds::{MarkerHandler, MarkerHandlers};
use moss_core::resolve::Diagnostic;

/// Construct a [`MarkerHandlers`] seeded with the built-in notebook and
/// table resolvers.
///
/// - `site_root`: absolute path to the user's site folder (for resolving
///   target paths to real files).
/// - `lang`: per-site language for localizing error messages emitted into
///   the visitor-facing HTML (e.g. "Notebook not found: …").
///
/// Plugin handlers should be added onto the returned registry by the
/// plugin runtime (see `moss-tauri::plugins::embed_adapter`).
pub fn builtin_marker_handlers(site_root: PathBuf, lang: crate::i18n::Language) -> MarkerHandlers<'static> {
    let mut handlers = MarkerHandlers::new();

    // --- Notebook (.ipynb) ---
    let notebook_root = site_root.clone();
    let notebook: MarkerHandler<'static> = Box::new(move |target: &str, diags: &mut Vec<Diagnostic>| {
        render_ipynb_embed(target, &notebook_root, lang, diags)
    });
    handlers.register("moss-embed-ipynb", notebook);

    // --- Tabular (.csv / .tsv) ---
    let table_root = site_root;
    let table: MarkerHandler<'static> = Box::new(move |target: &str, diags: &mut Vec<Diagnostic>| {
        render_table_embed(target, &table_root, lang, diags)
    });
    handlers.register("moss-embed-table", table);

    handlers
}

/// Resolve `<!-- moss-embed-ipynb:<path>[?query] -->` to an inline `<iframe>`
/// pointing at moss's JupyterLite viewer for the notebook.
///
/// Reuses moss's existing notebook infrastructure (see `build/notebook.rs`):
/// the site build copies the `.ipynb` into `.moss/build.nosync/current/...` and ships
/// a JupyterLite WASM runtime at `/jupyter/`. The viewer URL for a notebook
/// `foo.ipynb` is `/jupyter/notebooks/?path=<url-encoded filename>`.
///
/// Query string (if present) is currently ignored — reserved for future
/// cell-selection or view-mode params.
fn render_ipynb_embed(target: &str, site_root: &Path, lang: crate::i18n::Language, diags: &mut Vec<Diagnostic>) -> String {
    let (path, _query) = split_query(target);

    // Validate that the file exists, for diagnostics.
    let abs = site_root.join(path);
    if !abs.exists() {
        let msg = crate::i18n::t(lang, "notebook_not_found").replace("{}", path);
        diags.push(Diagnostic {
            message: msg.clone(),
            source_path: path.to_string(),
            reference: target.to_string(),
            kind: moss_core::resolve::DiagnosticKind::Other,
        });
        return format!(
            "<div class=\"{} moss-embed-error\">{}</div>",
            CLASS_EMBED,
            html_escape(&msg),
        );
    }

    // Derive just the filename for the JupyterLite URL parameter.
    let filename = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path);
    let encoded = url_encode_path(filename);

    // Emit an inline iframe. /jupyter/ is the base path for JupyterLite
    // assets; build/notebook.rs guarantees it exists when any .ipynb is in
    // the project.
    let _ = CLASS_EMBED_NOTEBOOK;
    format!(
        "<iframe class=\"{}\" data-type=\"notebook\" src=\"/jupyter/notebooks/?path={}\" loading=\"lazy\" \
         allow=\"clipboard-write\" \
         sandbox=\"allow-scripts allow-same-origin allow-popups allow-forms allow-modals\"></iframe>",
        CLASS_EMBED,
        encoded,
    )
}

/// Resolve `<!-- moss-embed-table:<path> -->` to an inline HTML `<table>`
/// via `moss_core::csv_table::render`.
///
/// Separator inferred from extension: `,` for `.csv`, `\t` for `.tsv`.
/// The first row is treated as a header.
fn render_table_embed(target: &str, site_root: &Path, lang: crate::i18n::Language, diags: &mut Vec<Diagnostic>) -> String {
    let (path, _query) = split_query(target);

    let abs = site_root.join(path);
    // Defer, never wait. This runs inside the rayon render fan-out while the
    // stage write guard is held, so a bounded wait here is a bounded wait on
    // the whole site publishing — one evicted CSV per page, serially, before
    // anything reaches the preview. Baking an error box in would be worse, so
    // we do neither: ask for the download, say plainly that it is still coming,
    // and let the supervisor's arrival detection rebuild the page.
    if crate::build::icloud::is_evicted(&abs) {
        crate::build::cloud_readiness::request_download(&abs);
        let msg = crate::i18n::t(lang, "embed_downloading").replace("{}", path);
        return format!("<div class=\"{} moss-embed-pending\">{}</div>", CLASS_EMBED, msg);
    }
    let content = match std::fs::read_to_string(&abs) {
        Ok(s) => s,
        Err(e) if crate::build::icloud::is_offline_not_absent(&abs, &e) => {
            crate::build::cloud_readiness::request_download(&abs);
            let msg = crate::i18n::t(lang, "embed_downloading").replace("{}", path);
            return format!("<div class=\"{} moss-embed-pending\">{}</div>", CLASS_EMBED, msg);
        }
        Err(e) => {
            diags.push(Diagnostic {
                message: format!("Failed to read {}: {}", path, e),
                source_path: path.to_string(),
                reference: target.to_string(),
                kind: moss_core::resolve::DiagnosticKind::Other,
            });
            let msg = crate::i18n::t(lang, "table_not_found").replace("{}", path);
            return format!(
                "<div class=\"{} moss-embed-error\">{}</div>",
                CLASS_EMBED,
                html_escape(&msg),
            );
        }
    };

    let separator = if path.ends_with(".tsv") { '\t' } else { ',' };
    let options = moss_core::csv_table::CsvTableOptions {
        separator,
        has_header: true,
        caption: None,
        class: CLASS_EMBED.to_string(),
        data_type: Some("table".to_string()),
    };
    moss_core::csv_table::render(&content, &options)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Split a marker target into `(path, optional_query)` at the first `?`.
fn split_query(target: &str) -> (&str, Option<&str>) {
    match target.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (target, None),
    }
}

/// Minimal HTML escaper for error-message text spliced back into content.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Percent-encode characters that would break a URL query parameter value.
///
/// NOT the same function as `notebook::url_encode_path`, despite the shared
/// name — this one preserves `/` and `~` because it encodes a whole path that
/// stays a path inside the query value, while notebook's encodes a single
/// filename component, where a `/` must escape. Neither is the asset-URL
/// encoder (`fuzzy_path::percent_encode_path_segments`); these are query-value
/// helpers and the pruner never sees their output. Named here so the next
/// reader doesn't "unify" three functions that legitimately differ.
fn url_encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char);
            }
            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_split_query() {
        assert_eq!(split_query("nb.ipynb"), ("nb.ipynb", None));
        assert_eq!(split_query("nb.ipynb?x=1"), ("nb.ipynb", Some("x=1")));
        assert_eq!(split_query("a/b.csv?"), ("a/b.csv", Some("")));
    }

    #[test]
    fn test_url_encode_path_ascii_safe() {
        assert_eq!(url_encode_path("hello.ipynb"), "hello.ipynb");
        assert_eq!(url_encode_path("dir/file-1.ipynb"), "dir/file-1.ipynb");
    }

    #[test]
    fn test_url_encode_path_unicode() {
        // Chinese characters should be percent-encoded.
        let out = url_encode_path("交互/音阶.ipynb");
        assert!(out.contains("%"), "got: {}", out);
        assert!(out.contains(".ipynb"), "got: {}", out);
    }

    #[test]
    fn test_render_ipynb_embed_missing_file() {
        let tmp = TempDir::new().unwrap();
        let mut diags = Vec::new();
        let out = render_ipynb_embed("nope.ipynb", tmp.path(), crate::i18n::Language::En, &mut diags);
        assert_eq!(diags.len(), 1);
        assert!(out.contains("moss-embed-error"), "got: {}", out);
        assert!(out.contains("Notebook not found"), "got: {}", out);
    }

    #[test]
    fn missing_embed_error_localizes_by_lang() {
        // The missing-notebook / missing-table diagnostics localize in the lang
        // the handler is built with. blocking.rs builds these per file from the
        // page's own (path-derived) language, so an English page shows the
        // English error even on a Chinese-default site.
        let tmp = TempDir::new().unwrap();
        let mut d = Vec::new();
        let zh_nb = render_ipynb_embed("nope.ipynb", tmp.path(), crate::i18n::Language::ZhHans, &mut d);
        assert!(zh_nb.contains("找不到笔记本"), "zh-hans notebook error, got: {zh_nb}");
        assert!(!zh_nb.contains("Notebook not found"), "no English leak, got: {zh_nb}");

        let mut d2 = Vec::new();
        let zh_tbl = render_table_embed("nope.csv", tmp.path(), crate::i18n::Language::ZhHans, &mut d2);
        assert!(zh_tbl.contains("找不到表格"), "zh-hans table error, got: {zh_tbl}");
    }

    #[test]
    fn test_render_ipynb_embed_present() {
        let tmp = TempDir::new().unwrap();
        let nb = tmp.path().join("nb.ipynb");
        fs::write(&nb, "{}").unwrap();
        let mut diags = Vec::new();
        let out = render_ipynb_embed("nb.ipynb", tmp.path(), crate::i18n::Language::En, &mut diags);
        assert!(diags.is_empty());
        assert!(out.contains("<iframe "), "got: {}", out);
        assert!(
            out.contains("/jupyter/notebooks/?path=nb.ipynb"),
            "got: {}",
            out
        );
        assert!(
            out.contains("class=\"moss-embed\" data-type=\"notebook\""),
            "got: {}",
            out
        );
    }

    #[test]
    fn test_render_table_embed_csv() {
        let tmp = TempDir::new().unwrap();
        let csv = tmp.path().join("data.csv");
        fs::write(&csv, "name,age\nAlice,30\n").unwrap();
        let mut diags = Vec::new();
        let out = render_table_embed("data.csv", tmp.path(), crate::i18n::Language::En, &mut diags);
        assert!(diags.is_empty());
        assert!(out.contains("<table "), "got: {}", out);
        assert!(out.contains("<th>name</th>"), "got: {}", out);
        assert!(out.contains("<td>Alice</td>"), "got: {}", out);
        assert!(
            out.contains("class=\"moss-embed\" data-type=\"table\""),
            "got: {}",
            out
        );
    }

    #[test]
    fn test_render_table_embed_tsv_separator() {
        let tmp = TempDir::new().unwrap();
        let tsv = tmp.path().join("data.tsv");
        fs::write(&tsv, "a\tb\n1\t2\n").unwrap();
        let mut diags = Vec::new();
        let out = render_table_embed("data.tsv", tmp.path(), crate::i18n::Language::En, &mut diags);
        assert!(diags.is_empty());
        assert!(out.contains("<th>a</th>"), "got: {}", out);
        assert!(out.contains("<td>1</td><td>2</td>"), "got: {}", out);
    }

    #[test]
    fn test_render_table_embed_missing() {
        let tmp = TempDir::new().unwrap();
        let mut diags = Vec::new();
        let out = render_table_embed("ghost.csv", tmp.path(), crate::i18n::Language::En, &mut diags);
        assert_eq!(diags.len(), 1);
        assert!(out.contains("moss-embed-error"), "got: {}", out);
    }

    #[test]
    fn test_builtin_marker_handlers_registered() {
        let tmp = TempDir::new().unwrap();
        let handlers = builtin_marker_handlers(tmp.path().to_path_buf(), crate::i18n::Language::En);
        assert!(!handlers.is_empty());
    }

    // -------------------------------------------------------------------------
    // End-to-end integration: real markdown → resolved HTML via the full pipeline
    // -------------------------------------------------------------------------
    //
    // Exercises the full moss-core RESOLVE phase with these marker handlers.
    //
    // Primary fixture: `crates/moss-build/tests/fixtures/embed_handlers/` — minimal
    // .ipynb + .csv checked into the repo. Always runs.
    //
    // Extended fixture: a real site's notebooks + CSVs, kept outside the
    // repo, used when the `MOSS_NOTEBOOK_FIXTURE` env var points to that
    // site's root. Skipped silently when unset.

    use moss_core::content_graph::ContentGraphBuilder;
    use moss_core::resolve::registry::RendererRegistry;

    /// In-repo minimal fixture, always present. Cargo runs tests from the
    /// crate root (`crates/moss-build/`), so this path is stable.
    fn in_repo_fixture_root() -> PathBuf {
        PathBuf::from("tests/fixtures/embed_handlers")
    }

    /// Optional extended fixture with real site content. Set
    /// `MOSS_NOTEBOOK_FIXTURE` to that site's root to opt in. Returns None
    /// when unset or the path doesn't exist.
    fn extended_fixture_root() -> Option<PathBuf> {
        let path = std::env::var("MOSS_NOTEBOOK_FIXTURE").ok()?;
        let p = PathBuf::from(path);
        p.exists().then_some(p)
    }

    #[test]
    fn test_e2e_notebook_embed_resolves_to_jupyter_iframe() {
        let root = in_repo_fixture_root();

        let mut builder = ContentGraphBuilder::new();
        builder.add_file("minimal.ipynb", "minimal");
        let graph = builder.build();

        let md = "# Analysis\n\n![[minimal.ipynb]]\n";
        let registry = RendererRegistry::empty().build();
        let handlers = builtin_marker_handlers(root.clone(), crate::i18n::Language::En);
        let file_reader = |path: &str| std::fs::read_to_string(root.join(path)).ok();

        let result = moss_core::resolve::resolve_content_with_handlers(
            "index.md",
            md,
            &graph,
            &file_reader,
            &registry,
            &handlers,
        );

        assert!(
            result.content_markdown.contains("<iframe "),
            "missing iframe in output:\n{}",
            result.content_markdown
        );
        assert!(
            result
                .content_markdown
                .contains("/jupyter/notebooks/?path=minimal.ipynb"),
            "wrong src in output:\n{}",
            result.content_markdown
        );
        assert!(
            result.content_markdown.contains(r#"data-type="notebook""#),
            "missing data-type:\n{}",
            result.content_markdown
        );
        assert!(
            !result.content_markdown.contains("<!-- moss-embed-ipynb:"),
            "marker not resolved:\n{}",
            result.content_markdown
        );
    }

    #[test]
    fn test_e2e_csv_embed_resolves_to_html_table() {
        let root = in_repo_fixture_root();

        let mut builder = ContentGraphBuilder::new();
        builder.add_file("minimal.csv", "minimal");
        let graph = builder.build();

        let md = "# Data\n\n![[minimal.csv]]\n";
        let registry = RendererRegistry::empty().build();
        let handlers = builtin_marker_handlers(root.clone(), crate::i18n::Language::En);
        let file_reader = |path: &str| std::fs::read_to_string(root.join(path)).ok();

        let result = moss_core::resolve::resolve_content_with_handlers(
            "index.md",
            md,
            &graph,
            &file_reader,
            &registry,
            &handlers,
        );

        assert!(
            result.content_markdown.contains("<table "),
            "missing table in output:\n{}",
            result.content_markdown
        );
        assert!(
            result.content_markdown.contains("<thead>"),
            "missing <thead> (header row assumed):\n{}",
            result.content_markdown
        );
        assert!(
            result.content_markdown.contains("<td>alpha</td>"),
            "missing fixture row content:\n{}",
            result.content_markdown
        );
        assert!(
            result.content_markdown.contains(r#"data-type="table""#),
            "missing data-type:\n{}",
            result.content_markdown
        );
        assert!(
            !result.content_markdown.contains("<!-- moss-embed-table:"),
            "marker not resolved:\n{}",
            result.content_markdown
        );
    }

    #[test]
    fn test_e2e_unresolved_marker_stays_visible() {
        let root = in_repo_fixture_root();

        let mut builder = ContentGraphBuilder::new();
        builder.add_file("minimal.ipynb", "minimal");
        let graph = builder.build();

        let md = "# Test\n\n![[minimal.ipynb]]\n";
        let registry = RendererRegistry::empty().build();
        // Handlers WITHOUT notebook support — marker must survive.
        let handlers = MarkerHandlers::new();
        let file_reader = |path: &str| std::fs::read_to_string(root.join(path)).ok();

        let result = moss_core::resolve::resolve_content_with_handlers(
            "index.md",
            md,
            &graph,
            &file_reader,
            &registry,
            &handlers,
        );

        assert!(
            result.content_markdown.contains("<!-- moss-embed-ipynb:"),
            "unregistered markers should be preserved, got:\n{}",
            result.content_markdown
        );
    }

    // -------------------------------------------------------------------------
    // Extended fixture (opt-in): real site content.
    //
    // Run with `MOSS_NOTEBOOK_FIXTURE=/path/to/site cargo test`.
    // -------------------------------------------------------------------------

    #[test]
    fn test_e2e_external_site_notebook() {
        let Some(root) = extended_fixture_root() else {
            eprintln!("skipping: MOSS_NOTEBOOK_FIXTURE not set");
            return;
        };
        let nb = "resources/orbit-model.ipynb";
        if !root.join(nb).exists() {
            eprintln!("skipping: {} not under MOSS_NOTEBOOK_FIXTURE", nb);
            return;
        }

        let mut builder = ContentGraphBuilder::new();
        builder.add_file(nb, "orbit-model");
        let graph = builder.build();

        let md = "# Real notebook\n\n![[orbit-model.ipynb]]\n";
        let registry = RendererRegistry::empty().build();
        let handlers = builtin_marker_handlers(root.clone(), crate::i18n::Language::En);
        let file_reader = |path: &str| std::fs::read_to_string(root.join(path)).ok();

        let result = moss_core::resolve::resolve_content_with_handlers(
            "index.md",
            md,
            &graph,
            &file_reader,
            &registry,
            &handlers,
        );

        assert!(
            result.content_markdown.contains("<iframe "),
            "extended fixture: iframe missing"
        );
        assert!(
            result
                .content_markdown
                .contains("/jupyter/notebooks/?path=orbit-model.ipynb")
        );
    }

    // Boundary regression tests for the typed embed renderer →
    // post-processor seam now live in the desktop app's own integration
    // tests, since they cross two crates and are broader than this module.
}
