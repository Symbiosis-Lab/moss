//! Validation helpers — schema composition and diagnostic generation.
//!
//! `compose_schema` is called by the `get_schema` Tauri command in
//! `commands.rs`; `diagnose` is the whole body of `validate_content`, which
//! keeps only the read and the schema resolution.

/// A validation diagnostic exposed to the frontend.
///
/// Lives here rather than in the app crate because both carriers return it:
/// the desktop `validate_content` command and the HTTP arm.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct EditorDiagnostic {
    /// Severity: 1=Error, 2=Warning, 3=Info, 4=Hint (LSP-compatible).
    pub severity: u8,
    /// Human-readable message.
    pub message: String,
    /// Frontmatter field path (e.g. "title", "also_in[0]").
    pub path: Option<String>,
    /// Source line (1-based), if available.
    pub line: Option<usize>,
    /// Source column (1-based), if available.
    pub column: Option<usize>,
}

impl From<moss_core::validation::Diagnostic> for EditorDiagnostic {
    fn from(d: moss_core::validation::Diagnostic) -> Self {
        Self {
            severity: d.severity as u8,
            message: d.message,
            path: d.path,
            line: d.line,
            column: d.column,
        }
    }
}

/// Compose the effective schema by merging builtin fields with plugin
/// contributions.
///
/// Merge rules (see docs/reference/plugin-schema-contributions.md):
/// - Additive only: plugins add fields, cannot modify/remove builtin fields
/// - Builtin wins: if a plugin tries to redefine a builtin field, it is ignored
///   with a warning
/// - First-plugin wins: if two plugins define the same field, the first
///   (alphabetical) wins
/// - Every conflict produces a log warning — no silent overwrites
pub fn compose_schema(
    plugins: &[crate::plugins::types::Plugin],
) -> moss_core::schema::ContentSchema {
    let mut schema = moss_core::schema::builtin_schema();
    for plugin in plugins {
        if let Some(ref contributes) = plugin.manifest.contributes {
            if let Some(ref fm) = contributes.frontmatter {
                for (name, field_def) in &fm.fields {
                    if schema.frontmatter.fields.contains_key(name) {
                        log::warn!(
                            "Plugin '{}' tried to redefine field '{}', ignoring",
                            plugin.manifest.name,
                            name
                        );
                        continue;
                    }
                    let mut fd = field_def.clone();
                    fd.source = Some(plugin.manifest.name.clone());
                    schema.frontmatter.fields.insert(name.clone(), fd);
                }
            }
        }
    }
    schema
}

/// Every diagnostic the editor shows for one file: the schema's verdict on
/// the fields that parsed, plus the one thing the schema cannot see — whether
/// this file's `url:` collides with another file's. Corpus knowledge, so that
/// half comes from the last build rather than from the document; moss-core
/// stays pure and sees only what it is handed.
///
/// Malformed YAML is deliberately absent: that surface is the frontmatter
/// banner driven by `ParsedFrontmatter.frontmatter_error` (`parse_frontmatter`
/// + `loadFile` in editor-main.ts). A block that failed to parse yields zero
/// fields, and a schema complaint about each missing one would bury the syntax
/// error that caused them.
pub fn diagnose(
    project_root: Option<&std::path::Path>,
    source: &std::path::Path,
    frontmatter: &std::collections::HashMap<String, serde_yaml::Value>,
    schema: &moss_core::schema::ContentSchema,
) -> Vec<EditorDiagnostic> {
    let mut diags: Vec<EditorDiagnostic> =
        moss_core::validation::validate_frontmatter(frontmatter, schema)
            .into_iter()
            .map(EditorDiagnostic::from)
            .collect();
    if let Some(root) = project_root {
        diags.extend(url_collision_diagnostic(root, source));
    }
    diags
}

/// The `url:` collision recorded for this file by the last build, if any.
///
/// Reads `ArticleMap.url_collisions`, which the scan phase fills from
/// `resolve_duplicate_slugs_with_lang`. Silent when there is no map yet (a
/// vault that has never been built) — an absent record is "not known", never
/// "no collision", and inventing a clean verdict is how a stale surface
/// becomes worse than no surface.
///
/// See docs/archive/2026-09-02-url-collision-as-a-frontmatter-diagnostic.md.
pub(crate) fn url_collision_diagnostic(project_root: &std::path::Path, file_path: &std::path::Path) -> Vec<EditorDiagnostic> {
    let Ok(relative) = file_path.strip_prefix(project_root) else {
        return Vec::new();
    };
    let key = relative.to_string_lossy().replace('\\', "/");
    let map = match crate::build::scan::article_map::ArticleMap::load(
        &project_root.join(".moss"),
    ) {
        Ok(map) => map,
        Err(_) => return Vec::new(),
    };
    let Some(c) = map.url_collisions.get(&key) else {
        return Vec::new();
    };
    vec![EditorDiagnostic {
        // Warning, not Error: the page IS published and reachable, just not at
        // the address its author asked for.
        severity: 2,
        message: crate::infra::app_advisory::fmt(
            "url_taken",
            &[("keeper", &c.keeper), ("moved_to", &c.moved_to)],
        ),
        // Addresses the `url` chip. The whole point of this surface is that
        // the complaint sits on the field the author has to retype.
        path: Some("url".to_string()),
        line: None,
        column: None,
    }]
}

#[cfg(test)]
mod url_collision_tests {
    use crate::build::scan::article_map::ArticleMap;
    use crate::build::scan::slug::UrlCollision;

    /// A vault whose last build recorded one collision, keyed the way the
    /// build keys it: by `ParsedDocument.source_path`, which is vault-relative.
    fn vault_with_collision() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let moss = dir.path().join(".moss");
        std::fs::create_dir_all(moss.join("build.nosync")).unwrap();
        let mut map = ArticleMap::new();
        map.url_collisions.insert(
            "獎項/記憶獎/記憶獎.md".to_string(),
            UrlCollision {
                loser: "獎項/記憶獎/記憶獎.md".to_string(),
                keeper: "獎項/漫畫獎/漫畫獎.md".to_string(),
                wanted: "awards/comics/".to_string(),
                moved_to: "awards/comics-2/".to_string(),
            },
        );
        map.save(&moss).unwrap();
        dir
    }

    /// The seam this test exists for: the map is keyed by a VAULT-RELATIVE
    /// source path and the editor asks with an ABSOLUTE one. Nothing else in
    /// the build proves those two agree, and if they stop agreeing the chip
    /// simply never lights up — a silent regression with no red anywhere.
    #[test]
    fn an_absolute_editor_path_finds_the_relative_map_key() {
        let dir = vault_with_collision();
        let file = dir.path().join("獎項/記憶獎/記憶獎.md");

        let diags = super::url_collision_diagnostic(dir.path(), &file);

        assert_eq!(diags.len(), 1);
        // Addressed at the field the author has to retype, not at the file.
        assert_eq!(diags[0].path.as_deref(), Some("url"));
        // Warning, not error: the page is published, just not where she asked.
        assert_eq!(diags[0].severity, 2);
        assert!(diags[0].message.contains("漫畫獎"), "{}", diags[0].message);
        assert!(diags[0].message.contains("awards/comics-2/"), "{}", diags[0].message);
    }

    #[test]
    fn a_file_with_no_recorded_collision_is_silent() {
        let dir = vault_with_collision();
        let file = dir.path().join("獎項/漫畫獎/漫畫獎.md");
        assert!(super::url_collision_diagnostic(dir.path(), &file).is_empty());
    }

    /// A vault that has never been built has no map. Absence is "not known",
    /// never "no collision" — but it must also not invent a complaint.
    #[test]
    fn an_unbuilt_vault_reports_nothing_rather_than_failing() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.md");
        assert!(super::url_collision_diagnostic(dir.path(), &file).is_empty());
    }

    /// A path outside the vault cannot be keyed at all; it must not panic on
    /// the failed `strip_prefix`.
    #[test]
    fn a_path_outside_the_vault_is_silent() {
        let dir = vault_with_collision();
        let outside = std::path::Path::new("/somewhere/else/記憶獎.md");
        assert!(super::url_collision_diagnostic(dir.path(), outside).is_empty());
    }
}
