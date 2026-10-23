//! A theme's scripts beside `script.js`: every `.js`/`.mjs` file under
//! `.moss/theme/` is served from a content-hashed URL through an import map
//! written in `<head>`, so a plain `import "./kernel.js"` (from `script.mjs` or
//! a classic script's `import()`) loads `kernel.<hash>.js` and an edit to
//! `kernel.js` changes only its own address. `script.js` stays the classic
//! script it has always been; `script.mjs`, when present, also loads as a
//! `<script type="module">`. Every file is published twice: plain (the theme
//! mirror) and hashed. A theme with no such file gets no map and no tag.

use crate::build::assets::paths::{compute_binary_hash, PathResolver};
use crate::build::cloud_ledger::InputEvidence;
use crate::build::cloud_readiness::read_optional_build_input;
use crate::build::context::BuildContext;
use crate::build::manifest::HashBucket;
use crate::build::served_path::ServedPath;
use std::collections::BTreeMap;
use std::path::Path;

const ENTRY: &str = "script.mjs";

struct ModuleFile {
    /// Path under `.moss/theme/`, forward slashes (`lib/kernel.mjs`).
    rel: String,
    hash: String,
    text: String,
}

/// Every theme JS file of one build, read once. Empty unless the theme has a
/// `script.mjs`.
#[derive(Default)]
pub struct ThemeModules {
    files: Vec<ModuleFile>,
}

impl ThemeModules {
    /// Reads `theme_dir`'s `.js`/`.mjs` files. `script.js` is skipped: it is the
    /// classic entry and has its own tag. A file still in the cloud is left out
    /// of this build (its plain URL still works) and the download triggers a
    /// rebuild, as for every theme file; a file that is not UTF-8 is left out
    /// with a warning, since a browser could not run it as a module either.
    pub fn read(theme_dir: &Path, evidence: &InputEvidence) -> Result<Self, String> {
        let mut files = Vec::new();
        for entry in walkdir::WalkDir::new(theme_dir).sort_by_file_name().into_iter().flatten() {
            if !entry.file_type().is_file() {
                continue;
            }
            let Ok(rel) = entry.path().strip_prefix(theme_dir) else { continue };
            let rel = moss_core::slug::normalize_separators(&rel.to_string_lossy());
            let is_js = rel.ends_with(".js") || rel.ends_with(".mjs");
            if !is_js || rel == "script.js" || ServedPath::for_theme_script_hashed(&rel, "").is_err() {
                continue;
            }
            if !crate::build::icloud::is_evicted(entry.path())
                && std::fs::read(entry.path()).is_ok_and(|b| std::str::from_utf8(&b).is_err())
            {
                crate::build::cli_output::log_warn_problem!(
                    ".moss/theme/{rel} is not UTF-8, so it is served as is but not fingerprinted"
                );
                continue;
            }
            if let Some(text) = read_optional_build_input(entry.path(), &format!("theme module {rel}"), evidence)? {
                let hash = compute_binary_hash(text.as_bytes());
                files.push(ModuleFile { rel, hash, text });
            }
        }
        Ok(Self { files })
    }

    fn entry(&self) -> Option<&ModuleFile> {
        self.files.iter().find(|f| f.rel == ENTRY)
    }

    /// Every module hash, for the build-wide asset key: a page that skips
    /// re-rendering must not keep an import map naming an old file.
    pub fn hashes(&self) -> impl Iterator<Item = &str> {
        self.files.iter().map(|f| f.hash.as_str())
    }

    /// The two shell fragments for a page: the user-script tags (`window.mossTheme`
    /// plus the classic `script.js` tag when `has_user_js`, plus the module tag
    /// when `script.mjs` exists; `None` with neither) and the import map for
    /// `<head>`. One owner, so the article and folder-index render paths cannot
    /// drift.
    pub fn shell_tags(&self, resolver: &PathResolver, has_user_js: bool) -> (Option<String>, String) {
        (resolver.user_js_tag(has_user_js, &self.entry_tag()), self.importmap_tag())
    }

    /// The `<script type="importmap">` tag for `<head>`, or `""` with no file.
    /// It must precede every module script, so it cannot sit with the user tag.
    fn importmap_tag(&self) -> String {
        if self.files.is_empty() {
            return String::new();
        }
        let imports: BTreeMap<String, String> = self
            .files
            .iter()
            .filter_map(|f| {
                let plain = ServedPath::for_theme_asset(&f.rel).ok()?.to_relative_url();
                let hashed = ServedPath::for_theme_script_hashed(&f.rel, &f.hash).ok()?.to_relative_url();
                Some((plain, hashed))
            })
            .collect();
        let json = serde_json::json!({ "imports": imports }).to_string().replace('<', "\\u003c");
        format!("\n    <script type=\"importmap\">{json}</script>")
    }

    /// The `<script type="module">` tag for `script.mjs`, or `""`.
    fn entry_tag(&self) -> String {
        match self.entry().and_then(|f| ServedPath::for_theme_script_hashed(&f.rel, &f.hash).ok()) {
            Some(path) => format!(
                "\n    <script type=\"module\" src=\"{}\"></script>",
                moss_core::media::html_escape(&path.to_relative_url())
            ),
            None => String::new(),
        }
    }

    /// Writes each hashed copy. The plain copies are the verbatim theme mirror's.
    pub fn emit(&self, ctx: &mut BuildContext<'_>) -> Result<(), String> {
        for f in &self.files {
            let path = ServedPath::for_theme_script_hashed(&f.rel, &f.hash).map_err(|e| e.to_string())?;
            ctx.emit(&path, f.text.as_bytes(), HashBucket::Files)
                .map_err(|e| format!("Failed to emit {}: {}", path.as_str(), e))?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "theme_modules_tests.rs"]
mod tests;
