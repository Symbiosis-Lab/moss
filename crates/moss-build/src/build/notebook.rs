//! Jupyter notebook support for moss static site generation.
//!
//! # What are Jupyter Notebooks?
//!
//! `.ipynb` files are JSON documents containing an ordered list of cells.
//! Each cell is one of:
//! - **Code**: source code with optional execution outputs (text, images, plots)
//! - **Markdown**: prose formatted with Markdown
//! - **Raw**: unprocessed text
//!
//! Format spec: <https://nbformat.readthedocs.io/en/latest/format_description.html>
//!
//! # How moss handles notebooks
//!
//! Notebooks follow the same mental model as source HTML files (like p5.js
//! sketches or interactive embeds):
//!
//! - **Scan phase**: Detect `.ipynb` files and add them to `ProjectStructure.notebook_files`.
//!   No download, no processing — just categorization. This keeps the scan fast.
//!
//! - **Background phase**: If any notebooks were detected:
//!   1. Download JupyterLite assets if not cached (`~/.moss/assets/jupyterlite/`)
//!   2. Copy JupyterLite assets to `<output>/jupyter/` (once, shared across notebooks)
//!   3. For each `.ipynb`: copy to output + generate a thin viewer HTML wrapper
//!   4. Emit progress events for the UI
//!
//! - **Run time (visitor's browser)**:
//!   - Each notebook URL serves a page with a JupyterLite iframe
//!   - JupyterLite loads directly (community standard — no click-to-activate)
//!   - `loading="lazy"` on the iframe defers load until viewport scroll
//!   - JupyterLite is a WASM-based Jupyter environment (~20MB) that runs
//!     entirely in the browser — no server needed
//!
//! # Embedding notebooks in markdown
//!
//! Users can embed notebooks from their `.md` files using iframes:
//!
//! ```markdown
//! # My Analysis
//! <iframe src="/notebooks/analysis" style="width:100%;height:80vh;border:none;"></iframe>
//! ```
//!
//! The notebook URL is derived the same way as markdown file URLs
//! (`resolve_path_with_overrides`), so Chinese directory names like `数据/`
//! are mapped to their overridden paths.
//!
//! # JupyterLite distribution
//!
//! JupyterLite requires Python to build (`jupyter lite build`). We maintain
//! a CI-only repo at `Symbiosis-Lab/jupyterlite-dist` that builds and publishes
//! the output as a downloadable zip on GitHub Releases. This avoids requiring
//! Python on the user's machine.
//!
//! The zip contains the complete JupyterLite static site: HTML, JS, CSS, and
//! WASM files. It's platform-independent (one zip for all OS/arch), unlike
//! FFmpeg which needs per-platform binaries.

use crate::types::content::FileInfo;
use crate::build::assets::asset_resolver::AssetConfig;

// =============================================================================
// Types
// =============================================================================

/// A notebook file queued for background processing.
///
/// Collects the metadata needed
/// for the background phase without re-reading the filesystem.
#[derive(Debug, Clone)]
pub struct NotebookItem {
    /// Relative path from the project root (e.g. "notebooks/analysis.ipynb").
    pub source_path: String,
}

// =============================================================================
// Configuration
// =============================================================================

/// The pinned JupyterLite bundle: a tagged `Symbiosis-Lab/jupyterlite-dist`
/// release plus the SHA-256 of its zip, which is also the cache key (see
/// `asset_resolver`). Bumping this pair IS the upgrade path — the old
/// `releases/latest` URL meant two machines building the same vault could
/// hold different bundles, and an installed machine never upgraded at all.
/// A bump ships behaviour changes from the bundle itself (0.7→0.8 silently
/// changed an unset `contentsAllJsonFile` from "use the default" to "serve
/// nothing"), so the `notebook-loads` render gate must pass against the new
/// bundle before a bump lands.
const JUPYTERLITE_DIST_TAG: &str = "v0.8.3-20260831";
const JUPYTERLITE_ZIP_SHA256: &str =
    "9c0f20abc244fb98e5d08c6bfdb6559fe8abc84adfc96ca00a40737af108c2b4";

/// Returns the `AssetConfig` for downloading the pinned JupyterLite bundle.
pub fn jupyterlite_asset_config() -> AssetConfig {
    AssetConfig {
        name: "jupyterlite".to_string(),
        download_url: format!(
            "https://github.com/Symbiosis-Lab/jupyterlite-dist/releases/download/{JUPYTERLITE_DIST_TAG}/jupyterlite.zip"
        ),
        sha256: JUPYTERLITE_ZIP_SHA256.to_string(),
        required_disk_space: Some(100 * 1024 * 1024), // 100 MB headroom
    }
}

// =============================================================================
// Collection
// =============================================================================

/// Collects notebook files from a `ProjectStructure` into items for background
/// processing. Mirrors `collect_videos_for_conversion()` in `ffmpeg.rs`.
pub fn collect_notebooks(notebook_files: &[FileInfo]) -> Vec<NotebookItem> {
    notebook_files
        .iter()
        .map(|f| NotebookItem {
            source_path: f.path.clone(),
        })
        .collect()
}

// =============================================================================
// JupyterLite contents manifest
// =============================================================================

/// Rewrites the JupyterLite bundle's `jupyter-lite.json` for serving under
/// `/jupyter/`.
///
/// Two keys, both load-bearing:
///
/// - `baseUrl` — the bundle ships `"./"`, which resolves against the *page's*
///   location rather than JupyterLite's root and so breaks under a
///   subdirectory.
/// - `contentsAllJsonFile` — the name of the per-directory index JupyterLite
///   fetches under `api/contents/`. Since 0.8 its absence is not a default but
///   a switch: `_getServerDirectory` returns an empty directory without
///   fetching anything, so every served file reads as missing and opening a
///   notebook fails with "Could not find content with path". 0.7 read
///   `api/contents/all.json` unconditionally, which is why the same
///   [`generate_contents_manifest`] output worked before the bundle moved to
///   0.8 (2026-07-01) and stopped working after, with no moss change.
///
/// Returns `None` when the input is not JSON, leaving the file untouched.
pub fn patch_jupyterlite_config(config_json: &str) -> Option<String> {
    let mut config: serde_json::Value = serde_json::from_str(config_json).ok()?;
    let data = config.get_mut("jupyter-config-data")?;
    data["baseUrl"] = serde_json::json!("/jupyter/");
    data["contentsAllJsonFile"] = serde_json::json!(CONTENTS_ALL_JSON_FILE);
    serde_json::to_string_pretty(&config).ok()
}

/// Basename of the contents index, shared by the writer
/// ([`generate_contents_manifest`]'s caller) and the config key that tells
/// JupyterLite to read it.
pub const CONTENTS_ALL_JSON_FILE: &str = "all.json";

/// Generates the `api/contents/all.json` manifest that JupyterLite needs to
/// discover files placed in the `files/` directory.
///
/// Without this manifest, JupyterLite cannot find notebooks or data files even
/// if they exist in `/jupyter/files/`. The manifest follows the Jupyter Contents
/// API format.
///
/// # Arguments
///
/// * `notebook_filenames` - List of `.ipynb` filenames copied to `files/`.
/// * `notebook_contents` - Corresponding raw JSON content of each notebook.
/// * `data_filenames` - List of non-notebook filenames (CSV, JSON, etc.) copied
///   to `files/`. These are listed as regular files so JupyterLite's file browser
///   shows them and notebook code can access them via `open('data.csv')` or
///   `np.loadtxt('data.csv')`.
///
/// # Returns
///
/// JSON string for `api/contents/all.json`.
pub fn generate_contents_manifest(
    notebook_filenames: &[&str],
    notebook_contents: &[&str],
    data_filenames: &[&str],
) -> String {
    use serde_json::{json, Value};

    let mut entries: Vec<Value> = notebook_filenames.iter().zip(notebook_contents.iter())
        .filter_map(|(name, content)| {
            let parsed: Value = serde_json::from_str(content).ok()?;
            Some(json!({
                "name": name,
                "path": name,
                "last_modified": "2024-01-01T00:00:00.000000Z",
                "created": "2024-01-01T00:00:00.000000Z",
                "format": "json",
                "mimetype": null,
                "size": content.len(),
                "writable": true,
                "type": "notebook",
                "content": parsed,
            }))
        })
        .collect();

    // Add data file entries (CSV, JSON, etc.)
    for name in data_filenames {
        let ext_lower = std::path::Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        let mimetype = match ext_lower.as_str() {
            "csv" => "text/csv",
            "json" => "application/json",
            "tsv" => "text/tab-separated-values",
            "txt" => "text/plain",
            _ => "application/octet-stream",
        };
        entries.push(json!({
            "name": name,
            "path": name,
            "last_modified": "2024-01-01T00:00:00.000000Z",
            "created": "2024-01-01T00:00:00.000000Z",
            "format": "text",
            "mimetype": mimetype,
            "size": 0,
            "writable": true,
            "type": "file",
            "content": null,
        }));
    }

    let manifest = json!({
        "name": "",
        "path": "",
        "last_modified": "2024-01-01T00:00:00.000000Z",
        "created": "2024-01-01T00:00:00.000000Z",
        "format": "json",
        "mimetype": null,
        "size": null,
        "writable": true,
        "type": "directory",
        "content": entries,
    });

    serde_json::to_string_pretty(&manifest).unwrap_or_else(|_| "{}".to_string())
}

// =============================================================================
// Viewer HTML generation
// =============================================================================

/// Extract a title from an .ipynb notebook JSON string.
///
/// Tries the following sources in order:
/// 1. `metadata.title` field (if present and non-empty)
/// 2. First `# heading` line in the first markdown cell
/// 3. Returns `None` if neither is found (caller falls back to filename)
pub fn extract_notebook_title(ipynb_json: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(ipynb_json).ok()?;

    // 1. Check metadata.title
    if let Some(title) = parsed.get("metadata")
        .and_then(|m| m.get("title"))
        .and_then(|t| t.as_str())
    {
        let title = title.trim();
        if !title.is_empty() {
            return Some(title.to_string());
        }
    }

    // 2. First # heading in the first markdown cell
    if let Some(cells) = parsed.get("cells").and_then(|c| c.as_array()) {
        for cell in cells {
            if cell.get("cell_type").and_then(|t| t.as_str()) != Some("markdown") {
                continue;
            }
            // source is an array of strings (lines)
            let source_lines = match cell.get("source") {
                Some(serde_json::Value::Array(arr)) => {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                }
                Some(serde_json::Value::String(s)) => {
                    s.lines().collect::<Vec<_>>()
                }
                _ => continue,
            };
            for line in &source_lines {
                let trimmed = line.trim();
                if let Some(heading) = trimmed.strip_prefix("# ") {
                    let heading = heading.trim();
                    if !heading.is_empty() {
                        return Some(heading.to_string());
                    }
                }
            }
            // Only check the first markdown cell
            break;
        }
    }

    None
}

/// Generates a viewer HTML page that embeds JupyterLite with a specific notebook.
///
/// The generated page is a minimal HTML document with:
/// - A full-viewport JupyterLite iframe pointing to the notebook
/// - `loading="lazy"` on the iframe (native browser lazy-loading)
/// - No external dependencies — self-contained HTML
///
/// # Arguments
///
/// * `notebook_filename` - The filename of the `.ipynb` file (e.g. "analysis.ipynb").
///   Used to construct the JupyterLite URL parameter.
/// * `jupyter_base_path` - Base URL path where JupyterLite assets are served from
///   (e.g. "/jupyter"). Must not have a trailing slash.
/// * `notebook_content` - Optional raw .ipynb JSON. When provided, the title is
///   extracted from metadata or the first markdown heading instead of the filename.
///
/// # Returns
///
/// Complete HTML string for the viewer page.
pub fn generate_viewer_html(notebook_filename: &str, jupyter_base_path: &str) -> String {
    generate_viewer_html_with_content(notebook_filename, jupyter_base_path, None)
}

/// Like `generate_viewer_html` but accepts optional notebook JSON content
/// for title extraction.
pub fn generate_viewer_html_with_content(
    notebook_filename: &str,
    jupyter_base_path: &str,
    notebook_content: Option<&str>,
) -> String {
    // Extract title: try notebook JSON first, then fall back to filename
    let title_from_content = notebook_content.and_then(extract_notebook_title);
    let raw_title = title_from_content.as_deref().unwrap_or_else(|| {
        notebook_filename
            .strip_suffix(".ipynb")
            .unwrap_or(notebook_filename)
    });
    // HTML-escape the title to prevent XSS from notebook metadata
    let title = crate::build::features::html_escape(raw_title);

    // URL-encode the filename for use in the query parameter.
    // Handles spaces, non-ASCII (e.g., Chinese characters), and special chars.
    let encoded_filename = url_encode_path(notebook_filename);

    format!(
        r#"<!DOCTYPE html>
<html data-moss-html-version="1" lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
body {{ margin: 0; overflow: hidden; background: #faf8f5; }}
@media (prefers-color-scheme: dark) {{ body {{ background: #111; }} }}
iframe {{ width: 100%; height: 100vh; border: none; display: block; }}
#ld {{
  position: fixed; inset: 0; z-index: 10;
  display: flex; flex-direction: column;
  align-items: center; justify-content: center;
  background: #faf8f5;
  transition: opacity 0.6s ease;
}}
@media (prefers-color-scheme: dark) {{ #ld {{ background: #111; }} }}
#ld.done {{ opacity: 0; pointer-events: none; }}
#ld .d span {{
  display: inline-block; width: 6px; height: 6px;
  border-radius: 50%; background: #bbb; margin: 0 4px;
  animation: p 1.4s ease-in-out infinite;
}}
#ld .d span:nth-child(2) {{ animation-delay: 0.2s; }}
#ld .d span:nth-child(3) {{ animation-delay: 0.4s; }}
@keyframes p {{
  0%, 80%, 100% {{ opacity: 0.3; transform: scale(0.8); }}
  40% {{ opacity: 1; transform: scale(1); }}
}}
#ld .f {{
  margin-top: 16px; font-family: -apple-system, system-ui, sans-serif;
  font-size: 13px; color: #aaa; letter-spacing: 0.02em;
}}
@media (prefers-color-scheme: dark) {{
  #ld .d span {{ background: #666; }}
  #ld .f {{ color: #555; }}
}}
</style>
</head>
<body>
<div id="ld">
  <div class="d"><span></span><span></span><span></span></div>
  <div class="f">{notebook_filename}</div>
</div>
<iframe src="{jupyter_base_path}/notebooks/?path={encoded_filename}"
        loading="lazy"
        allow="clipboard-write"
        sandbox="allow-scripts allow-same-origin allow-popups allow-forms allow-modals"
        onload="document.getElementById('ld').classList.add('done')"></iframe>
</body>
</html>"#,
        title = title,
        jupyter_base_path = jupyter_base_path,
        notebook_filename = notebook_filename,
        encoded_filename = encoded_filename,
    )
}

/// Percent-encode a single path COMPONENT for use in a URL.
/// Encodes spaces, non-ASCII, and every URL-special character, preserving only
/// alphanumeric, `-`, `_` and `.` — `/` is escaped on purpose, since a
/// component that contains one must not become a path boundary.
/// (`embed_handlers::url_encode_path` shares the name but preserves `/` and
/// `~`; it encodes a whole path. Both are query-value helpers, distinct from
/// the asset-URL encoder `fuzzy_path::percent_encode_path_segments`.)
fn url_encode_path(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len() * 2);
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                encoded.push(byte as char);
            }
            _ => {
                encoded.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    encoded
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jupyterlite_asset_config_is_pinned() {
        let config = jupyterlite_asset_config();
        assert_eq!(config.name, "jupyterlite");
        assert!(config.download_url.contains("jupyterlite-dist"));
        assert!(
            !config.download_url.contains("latest"),
            "the bundle URL must be a tagged release — `latest` is what let \
             two machines building the same vault hold different bundles"
        );
        assert_eq!(config.sha256.len(), 64, "a real SHA-256 hex digest");
        assert!(config.required_disk_space.is_some());
    }

    #[test]
    fn test_collect_notebooks_empty() {
        let items = collect_notebooks(&[]);
        assert!(items.is_empty());
    }

    #[test]
    fn test_collect_notebooks_maps_paths() {
        let files = vec![
            FileInfo {
                path: "notebooks/analysis.ipynb".to_string(),
                file_type: "ipynb".to_string(),
                size: 1024,
                modified: None,
            },
            FileInfo {
                path: "data/report.ipynb".to_string(),
                file_type: "ipynb".to_string(),
                size: 2048,
                modified: None,
            },
        ];

        let items = collect_notebooks(&files);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].source_path, "notebooks/analysis.ipynb");
        assert_eq!(items[1].source_path, "data/report.ipynb");
    }

    #[test]
    fn test_viewer_html_contains_iframe() {
        let html = generate_viewer_html("test.ipynb", "/jupyter");
        assert!(html.contains("<iframe"), "Should contain an iframe");
        assert!(
            html.contains("/jupyter/notebooks/?path=test.ipynb"),
            "Iframe src should point to JupyterLite with the notebook path"
        );
    }

    #[test]
    fn test_viewer_html_has_lazy_loading() {
        let html = generate_viewer_html("test.ipynb", "/jupyter");
        assert!(
            html.contains(r#"loading="lazy""#),
            "Iframe should have loading=lazy"
        );
    }

    #[test]
    fn test_viewer_html_title_from_filename() {
        let html = generate_viewer_html("My Analysis.ipynb", "/jupyter");
        assert!(
            html.contains("<title>My Analysis</title>"),
            "Title should be derived from filename without .ipynb extension"
        );
    }

    #[test]
    fn test_viewer_html_is_valid_document() {
        let html = generate_viewer_html("test.ipynb", "/jupyter");
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<html"));
        assert!(html.contains("</html>"));
        assert!(html.contains("<head>"));
        assert!(html.contains("</head>"));
        assert!(html.contains("<body>"));
        assert!(html.contains("</body>"));
    }

    #[test]
    fn test_viewer_html_sandbox_attributes() {
        let html = generate_viewer_html("test.ipynb", "/jupyter");
        // JupyterLite needs scripts and same-origin for WASM execution
        assert!(html.contains("allow-scripts"));
        assert!(html.contains("allow-same-origin"));
    }

    #[test]
    fn test_viewer_html_has_loading_state() {
        let html = generate_viewer_html("analysis.ipynb", "/jupyter");
        // Loading overlay with pulsing dots
        assert!(html.contains(r#"id="ld""#), "Should have loading overlay element");
        assert!(html.contains("@keyframes p"), "Should have pulse animation keyframes");
        // Filename shown as context clue
        assert!(html.contains("analysis.ipynb"), "Should show notebook filename in loading state");
        // Fade-out on iframe load
        assert!(html.contains("classList.add('done')"), "Should fade out loading on iframe onload");
    }

    #[test]
    fn test_viewer_html_loading_dark_mode() {
        let html = generate_viewer_html("test.ipynb", "/jupyter");
        assert!(html.contains("prefers-color-scheme: dark"), "Should have dark mode styles");
        assert!(html.contains("#111"), "Dark mode background should be #111");
    }

    #[test]
    fn test_viewer_html_custom_base_path() {
        let html = generate_viewer_html("test.ipynb", "/assets/jupyter");
        assert!(
            html.contains("/assets/jupyter/notebooks/"),
            "Should use custom base path"
        );
    }

    #[test]
    fn test_viewer_html_encodes_special_characters() {
        let html = generate_viewer_html("my analysis.ipynb", "/jupyter");
        assert!(
            html.contains("my%20analysis.ipynb"),
            "Spaces should be percent-encoded in the URL: {}",
            html
        );
        // Non-ASCII characters in iframe src (title still has readable form)
        let html_cn = generate_viewer_html("数据分析.ipynb", "/jupyter");
        // The iframe src should have encoded bytes, not raw Chinese characters
        assert!(
            html_cn.contains("path=%E6%95%B0"),
            "Non-ASCII in iframe src should be percent-encoded"
        );
        // The title should still be human-readable
        assert!(
            html_cn.contains("<title>数据分析</title>"),
            "Title should remain human-readable"
        );
    }

    #[test]
    fn test_url_encode_path_preserves_safe_chars() {
        assert_eq!(url_encode_path("test.ipynb"), "test.ipynb");
        assert_eq!(url_encode_path("my-notebook_v2.ipynb"), "my-notebook_v2.ipynb");
    }

    #[test]
    fn test_url_encode_path_encodes_spaces() {
        assert_eq!(url_encode_path("my file.ipynb"), "my%20file.ipynb");
    }

    /// JupyterLite 0.8 skips the contents index entirely when
    /// `contentsAllJsonFile` is unset, so every served notebook reads as
    /// missing. Asserting the key is present is asserting the notebooks load.
    #[test]
    fn patched_config_names_the_contents_index() {
        let shipped = r#"{"jupyter-lite-schema-version":0,
            "jupyter-config-data":{"appVersion":"0.8.0","baseUrl":"./"}}"#;
        let patched = patch_jupyterlite_config(shipped).expect("valid JSON patches");
        let data: serde_json::Value = serde_json::from_str(&patched).unwrap();
        let cfg = &data["jupyter-config-data"];
        assert_eq!(cfg["baseUrl"], "/jupyter/");
        assert_eq!(
            cfg["contentsAllJsonFile"], CONTENTS_ALL_JSON_FILE,
            "without this key 0.8 never fetches api/contents/{CONTENTS_ALL_JSON_FILE}"
        );
        assert_eq!(cfg["appVersion"], "0.8.0", "unrelated keys survive");
    }

    #[test]
    fn unpatchable_config_leaves_the_file_alone() {
        assert!(patch_jupyterlite_config("not json").is_none());
        assert!(patch_jupyterlite_config(r#"{"no-config-data":1}"#).is_none());
    }

    #[test]
    fn test_contents_manifest_includes_data_files() {
        let notebook_json = r#"{"cells":[],"metadata":{},"nbformat":4,"nbformat_minor":5}"#;
        let manifest = generate_contents_manifest(
            &["test.ipynb"],
            &[notebook_json],
            &["data.csv", "config.json"],
        );
        let parsed: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        let content = parsed["content"].as_array().unwrap();

        // Should have 3 entries: 1 notebook + 2 data files
        assert_eq!(content.len(), 3, "Manifest should contain notebook + data files");

        // First entry is notebook
        assert_eq!(content[0]["type"], "notebook");
        assert_eq!(content[0]["name"], "test.ipynb");

        // Second entry is CSV data file
        assert_eq!(content[1]["type"], "file");
        assert_eq!(content[1]["name"], "data.csv");
        assert_eq!(content[1]["mimetype"], "text/csv");
        assert_eq!(content[1]["format"], "text");

        // Third entry is JSON data file
        assert_eq!(content[2]["type"], "file");
        assert_eq!(content[2]["name"], "config.json");
        assert_eq!(content[2]["mimetype"], "application/json");
    }

    #[test]
    fn test_contents_manifest_empty_data_files() {
        let notebook_json = r#"{"cells":[],"metadata":{},"nbformat":4,"nbformat_minor":5}"#;
        let manifest = generate_contents_manifest(&["nb.ipynb"], &[notebook_json], &[]);
        let parsed: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        let content = parsed["content"].as_array().unwrap();
        assert_eq!(content.len(), 1, "Should only have notebook when no data files");
    }

    // =========================================================================
    // extract_notebook_title tests
    // =========================================================================

    #[test]
    fn test_extract_title_from_metadata() {
        let ipynb = r#"{
            "cells": [],
            "metadata": { "title": "My Analysis" },
            "nbformat": 4,
            "nbformat_minor": 5
        }"#;
        assert_eq!(extract_notebook_title(ipynb), Some("My Analysis".to_string()));
    }

    #[test]
    fn test_extract_title_from_first_markdown_heading() {
        let ipynb = r##"{
            "cells": [
                {
                    "cell_type": "code",
                    "source": ["import pandas as pd"]
                },
                {
                    "cell_type": "markdown",
                    "source": ["# Data Exploration\n", "Some text here"]
                }
            ],
            "metadata": {},
            "nbformat": 4,
            "nbformat_minor": 5
        }"##;
        assert_eq!(extract_notebook_title(ipynb), Some("Data Exploration".to_string()));
    }

    #[test]
    fn test_extract_title_metadata_takes_precedence() {
        let ipynb = r##"{
            "cells": [
                {
                    "cell_type": "markdown",
                    "source": ["# Heading from cell"]
                }
            ],
            "metadata": { "title": "Title from metadata" },
            "nbformat": 4,
            "nbformat_minor": 5
        }"##;
        assert_eq!(extract_notebook_title(ipynb), Some("Title from metadata".to_string()));
    }

    #[test]
    fn test_extract_title_falls_back_to_none() {
        let ipynb = r#"{
            "cells": [
                {
                    "cell_type": "code",
                    "source": ["print('hello')"]
                }
            ],
            "metadata": {},
            "nbformat": 4,
            "nbformat_minor": 5
        }"#;
        assert_eq!(extract_notebook_title(ipynb), None);
    }

    #[test]
    fn test_extract_title_empty_metadata_title() {
        let ipynb = r#"{
            "cells": [],
            "metadata": { "title": "  " },
            "nbformat": 4,
            "nbformat_minor": 5
        }"#;
        assert_eq!(extract_notebook_title(ipynb), None);
    }

    #[test]
    fn test_extract_title_source_as_string() {
        // Some notebook formats use a single string for source instead of array
        let ipynb = r##"{
            "cells": [
                {
                    "cell_type": "markdown",
                    "source": "# String Source Title\nBody text"
                }
            ],
            "metadata": {},
            "nbformat": 4,
            "nbformat_minor": 5
        }"##;
        assert_eq!(extract_notebook_title(ipynb), Some("String Source Title".to_string()));
    }

    #[test]
    fn test_viewer_html_uses_notebook_title() {
        let ipynb = r##"{
            "cells": [
                {
                    "cell_type": "markdown",
                    "source": ["# My Custom Title\n"]
                }
            ],
            "metadata": {},
            "nbformat": 4,
            "nbformat_minor": 5
        }"##;
        let html = generate_viewer_html_with_content("analysis.ipynb", "/jupyter", Some(ipynb));
        assert!(
            html.contains("<title>My Custom Title</title>"),
            "Title should come from notebook content, not filename"
        );
    }

    #[test]
    fn test_viewer_html_falls_back_to_filename_title() {
        let html = generate_viewer_html_with_content("analysis.ipynb", "/jupyter", None);
        assert!(
            html.contains("<title>analysis</title>"),
            "Title should fall back to filename without .ipynb"
        );
    }

    #[test]
    fn test_viewer_html_escapes_title_xss() {
        let ipynb = r#"{
            "cells": [],
            "metadata": { "title": "</title><script>alert(1)</script>" },
            "nbformat": 4,
            "nbformat_minor": 5
        }"#;
        let html = generate_viewer_html_with_content("test.ipynb", "/jupyter", Some(ipynb));
        assert!(
            !html.contains("<script>alert(1)</script>"),
            "Title must be HTML-escaped to prevent XSS"
        );
        assert!(
            html.contains("&lt;/title&gt;&lt;script&gt;alert(1)&lt;/script&gt;"),
            "HTML entities should be escaped in title"
        );
    }
}
